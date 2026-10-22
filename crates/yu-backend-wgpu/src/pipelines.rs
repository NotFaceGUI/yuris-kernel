//! 管线与 bind group 缓存。
//!
//! 管线按 `(blend mode, Rgba8Unorm)` 缓存;本 crate 只有一个目标格式,所以
//! 键就是 [`BlendMode`]。bind group 按 `(texture id, sampler kind)` 缓存,
//! 纹理身份不变就不会重建。
//!
//! 混合状态按 `yu-gfx` 固定管线的 D3D9 状态表实现(见
//! `docs/render-parity.md` §5.2/§12.3):
//!
//! | MODE | stage 源色 | color | alpha(同因子) |
//! |---|---|---|---|
//! | 0 normal | `T` | `src·a + dst·(1−a)` | `a·a + dst·(1−a)` |
//! | 1 add | `T` | `src·a + dst` | `a·a + dst` |
//! | 2 subtract | `T·A` | `dst − src·a` | `dst − a·a` |
//! | 6 lighten | `T·A` | `max(src,dst)` | `max(a,dst.a)` |
//! | 7 darken | `T·A+(1−a)` | `min(src,dst)` | `min(a,dst.a)` |
//!
//! 着色器输出 `alpha = texture.a × layer_alpha`,RGB 不预乘;`a` 即这个
//! 乘积。D3D9 未启用 `SEPARATEALPHABLENDENABLE` 时 alpha 与 RGB 使用同一组
//! 因子,所以 alpha 源项是 `a²`(SRCALPHA 因子作用于 alpha 通道)。
//! MODE 3/5/9 的颜色因子由 `BlendFactor::Src/OneMinusDst/OneMinusSrc`
//! 对应 D3D9 的 `SRCCOLOR/INVDESTCOLOR/INVSRCCOLOR`;其余 MODE 返回 `None`。

use std::collections::BTreeMap;

use yu_gfx::{BlendMode, FilterMode};
use yu_log::Diag;

use crate::device::{code, GpuContext};
use crate::textures::Vertex;
/// 全 crate 唯一的目标格式。管线键里的 format 维度因此是常量;
/// 若未来引入第二个格式,把 `pipelines` 的键改成 `(BlendMode, TextureFormat)`。
pub const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// 采样器种类。`Anisotropic` 在本后端退化为线性(无 mip/无各向异性数据)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FilterKind {
    Point = 0,
    Linear = 1,
}

impl FilterKind {
    pub const fn from_mode(mode: FilterMode) -> Self {
        match mode {
            FilterMode::Point => Self::Point,
            FilterMode::Linear | FilterMode::Anisotropic => Self::Linear,
        }
    }
}

/// D3D9 固定管线状态 → wgpu 混合状态。
///
/// WebGPU 的 `BlendFactor::Src` / `OneMinusDst` / `OneMinusSrc` 分别是
/// D3D9 `SRCCOLOR` / `INVDESTCOLOR` / `INVSRCCOLOR` 的**逐通道**等价物
/// (`Src` 对 alpha 通道就是 `As`),所以 MODE 3/5/9 可以精确表达,不需要
/// framebuffer fetch/离屏 ping-pong:
/// - Multiply  = `dst * src`        → src=Zero, dst=Src
/// - Screen    = `src*(1-dst)+dst`  → src=OneMinusDst, dst=One
/// - Exclusion = `src*(1-dst)+dst*(1-src)` → src=OneMinusDst, dst=OneMinusSrc
///
/// 源色归约(stage0/stage1)在主 shader 里用 `SRC_MODE` 常量完成;这里只
/// 下发最终 blend 因子。`MODE 7` 的白色 TFACTOR 与 `MODE 6` 的单 stage
/// 已由状态表定案(见 `docs/render-parity.md` §5.2)。
pub fn blend_state(mode: BlendMode) -> Option<wgpu::BlendState> {
    use wgpu::{BlendComponent, BlendFactor, BlendOperation};

    let color = |src_factor, dst_factor, operation| BlendComponent {
        src_factor,
        dst_factor,
        operation,
    };
    // D3D9 未启用 SEPARATEALPHABLENDENABLE:alpha 与 RGB 同因子。
    let both = |src_factor, dst_factor, operation| wgpu::BlendState {
        color: color(src_factor, dst_factor, operation),
        alpha: color(src_factor, dst_factor, operation),
    };
    Some(match mode {
        BlendMode::Normal => both(
            BlendFactor::SrcAlpha,
            BlendFactor::OneMinusSrcAlpha,
            BlendOperation::Add,
        ),
        BlendMode::Add => both(BlendFactor::SrcAlpha, BlendFactor::One, BlendOperation::Add),
        BlendMode::Subtract => both(
            BlendFactor::SrcAlpha,
            BlendFactor::One,
            BlendOperation::ReverseSubtract,
        ),
        BlendMode::Multiply => both(BlendFactor::Zero, BlendFactor::Src, BlendOperation::Add),
        BlendMode::Screen => both(BlendFactor::OneMinusDst, BlendFactor::One, BlendOperation::Add),
        BlendMode::Lighten => both(BlendFactor::One, BlendFactor::One, BlendOperation::Max),
        BlendMode::Darken => both(BlendFactor::One, BlendFactor::One, BlendOperation::Min),
        BlendMode::Exclusion => both(
            BlendFactor::OneMinusDst,
            BlendFactor::OneMinusSrc,
            BlendOperation::Add,
        ),
        _ => return None,
    })
}

/// 固定管线 stage0/stage1 的源色归约,对应 WGSL 的 `SRC_MODE` 常量。
pub fn src_mode(mode: BlendMode) -> Option<u32> {
    Some(match mode {
        BlendMode::Normal | BlendMode::Add => 0,
        BlendMode::Subtract
        | BlendMode::Screen
        | BlendMode::Lighten
        | BlendMode::Exclusion => 1,
        BlendMode::Multiply | BlendMode::Darken => 2,
        _ => return None,
    })
}

const SHADER: &str = crate::shaders::QUAD;

/// 双纹理过渡:配对纹理 G 通道为 mask(EVID-T-05)。
///
/// `coverage = clamp((30+70*fade) * (mask - (1-2*fade)), 0, 1)`;
/// `complete=1`(T>=256)直接为满。与 `yu_gfx::transition_coverage` 同式,
/// 并按 soft 的 u8 量化链输出 src alpha(淡入淡出 0 差异)。
const TRANSITION_SHADER: &str = crate::shaders::TRANSITION_FIXED;

/// 快照上的 RECTPAINT 族(拷贝完成后在快照纹理上原地绘制)。
///
/// `write_alpha_only=1` 对应 RECTPAINT2;否则整像素写入(颜色由 uniform 给出,
/// 已含 alpha 规则)。混合为替换,不做 blend。
const PAINT_SHADER: &str = crate::shaders::PAINT;

/// 管线 + bind group 缓存。
pub struct PipelineCache {
    bind_group_layout: wgpu::BindGroupLayout,
    layout: wgpu::PipelineLayout,
    shader: wgpu::ShaderModule,
    samplers: [wgpu::Sampler; 2],
    pipelines: BTreeMap<BlendMode, wgpu::RenderPipeline>,
    bind_groups: BTreeMap<(u64, FilterKind), wgpu::BindGroup>,
    /// 双纹理过渡(纹理 + mask + fade uniform)。
    transition_layout: wgpu::BindGroupLayout,
    transition_pipeline_layout: wgpu::PipelineLayout,
    transition_shader: wgpu::ShaderModule,
    transition_pipelines: BTreeMap<BlendMode, wgpu::RenderPipeline>,
    /// 引擎 effect Pass1(psC,mask R;`_DXMODE≠0`)。
    transition_psc_shader: wgpu::ShaderModule,
    transition_psc_pipelines: BTreeMap<BlendMode, wgpu::RenderPipeline>,
    /// 引擎 effect Pass0(psD,交叉淡化;`_DXMODE≠0` 且 TA<1)。
    transition_psd_shader: wgpu::ShaderModule,
    transition_psd_pipelines: BTreeMap<BlendMode, wgpu::RenderPipeline>,
    /// 快照上的 RECTPAINT 族:`[整像素, 只写 alpha]`。
    paint_layout: wgpu::BindGroupLayout,
    paint_pipeline_layout: wgpu::PipelineLayout,
    paint_shader: wgpu::ShaderModule,
    paint_pipelines: [Option<wgpu::RenderPipeline>; 2],
    /// RECTPAINT 的颜色 uniform/bind group(同一颜色跨帧复用;语义不变)。
    paint_groups: BTreeMap<(bool, [u32; 4]), wgpu::BindGroup>,
    pub total_pipelines_created: u64,
    pub total_bind_groups_created: u64,
}

/// 过渡管线变体:固定管线模型 / 引擎 psC / 引擎 psD。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TransitionVariant {
    Fixed,
    PsC,
    PsD,
}

impl PipelineCache {
    pub fn new(ctx: &GpuContext) -> Result<Self, Diag> {
        let shader = ctx
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("yu-backend-wgpu quad"),
                source: wgpu::ShaderSource::Wgsl(SHADER.into()),
            });
        let bind_group_layout =
            ctx.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("yu-backend-wgpu texture+sampler"),
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Texture {
                                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                                view_dimension: wgpu::TextureViewDimension::D2,
                                multisampled: false,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 1,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                            count: None,
                        },
                    ],
                });
        let layout = ctx
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("yu-backend-wgpu layout"),
                bind_group_layouts: &[Some(&bind_group_layout)],
                immediate_size: 0,
            });
        let transition_shader =
            ctx.device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("yu-backend-wgpu transition"),
                    source: wgpu::ShaderSource::Wgsl(TRANSITION_SHADER.into()),
                });
        let transition_psc_shader =
            ctx.device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("yu-backend-wgpu transition psC"),
                    source: wgpu::ShaderSource::Wgsl(crate::shaders::TRANSITION_PSC.into()),
                });
        let transition_psd_shader =
            ctx.device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("yu-backend-wgpu transition psD"),
                    source: wgpu::ShaderSource::Wgsl(crate::shaders::TRANSITION_PSD.into()),
                });
        let transition_layout =
            ctx.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("yu-backend-wgpu transition layout"),
                    entries: &[
                        texture_entry(0),
                        sampler_entry(1),
                        texture_entry(2),
                        sampler_entry(3),
                        wgpu::BindGroupLayoutEntry {
                            binding: 4,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Uniform,
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                    ],
                });
        let transition_pipeline_layout =
            ctx.device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("yu-backend-wgpu transition pipeline layout"),
                    bind_group_layouts: &[Some(&transition_layout)],
                    immediate_size: 0,
                });
        let paint_shader = ctx
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("yu-backend-wgpu paint"),
                source: wgpu::ShaderSource::Wgsl(PAINT_SHADER.into()),
            });
        let paint_layout = ctx
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("yu-backend-wgpu paint layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });
        let paint_pipeline_layout =
            ctx.device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("yu-backend-wgpu paint pipeline layout"),
                    bind_group_layouts: &[Some(&paint_layout)],
                    immediate_size: 0,
                });
        let samplers = [
            make_sampler(ctx, FilterKind::Point),
            make_sampler(ctx, FilterKind::Linear),
        ];
        Ok(Self {
            bind_group_layout,
            layout,
            shader,
            samplers,
            pipelines: BTreeMap::new(),
            bind_groups: BTreeMap::new(),
            transition_layout,
            transition_pipeline_layout,
            transition_shader,
            transition_pipelines: BTreeMap::new(),
            transition_psc_shader,
            transition_psc_pipelines: BTreeMap::new(),
            transition_psd_shader,
            transition_psd_pipelines: BTreeMap::new(),
            paint_layout,
            paint_pipeline_layout,
            paint_shader,
            paint_pipelines: [None, None],
            paint_groups: BTreeMap::new(),
            total_pipelines_created: 0,
            total_bind_groups_created: 0,
        })
    }

    pub fn pipelines(&self) -> usize {
        self.pipelines.len()
    }

    pub fn bind_groups(&self) -> usize {
        self.bind_groups.len()
    }

    pub fn pipeline(&self, mode: BlendMode) -> Option<&wgpu::RenderPipeline> {
        self.pipelines.get(&mode)
    }

    pub fn bind_group(&self, texture_id: u64, filter: FilterKind) -> Option<&wgpu::BindGroup> {
        self.bind_groups.get(&(texture_id, filter))
    }

    /// 为某个混合模式建管线;不支持的模式返回 `None`(调用方已跳过)。
    pub fn ensure_pipeline(&mut self, ctx: &GpuContext, mode: BlendMode) -> Result<bool, Diag> {
        if self.pipelines.contains_key(&mode) {
            return Ok(true);
        }
        let Some(blend) = blend_state(mode) else {
            return Ok(false);
        };
        let Some(src_mode) = src_mode(mode) else {
            return Ok(false);
        };
        let constants = [("SRC_MODE", f64::from(src_mode))];
        let label = format!("yu-backend-wgpu blend {}", mode.name());
        let pipeline = ctx.checked(code::TARGET, "渲染管线", || {
            ctx.device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(&label),
                    layout: Some(&self.layout),
                    vertex: wgpu::VertexState {
                        module: &self.shader,
                        entry_point: Some("vs_main"),
                        compilation_options: wgpu::PipelineCompilationOptions {
                            constants: &constants,
                            ..Default::default()
                        },
                        buffers: &[Some(wgpu::VertexBufferLayout {
                            array_stride: Vertex::SIZE,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &[
                                wgpu::VertexAttribute {
                                    format: wgpu::VertexFormat::Float32x2,
                                    offset: 0,
                                    shader_location: 0,
                                },
                                wgpu::VertexAttribute {
                                    format: wgpu::VertexFormat::Float32x2,
                                    offset: 8,
                                    shader_location: 1,
                                },
                                wgpu::VertexAttribute {
                                    format: wgpu::VertexFormat::Float32,
                                    offset: 16,
                                    shader_location: 2,
                                },
                            ],
                        })],
                    },
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleList,
                        strip_index_format: None,
                        front_face: wgpu::FrontFace::Ccw,
                        cull_mode: None,
                        unclipped_depth: false,
                        polygon_mode: wgpu::PolygonMode::Fill,
                        conservative: false,
                    },
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &self.shader,
                        entry_point: Some("fs_main"),
                        compilation_options: wgpu::PipelineCompilationOptions {
                            constants: &constants,
                            ..Default::default()
                        },
                        targets: &[Some(wgpu::ColorTargetState {
                            format: TARGET_FORMAT,
                            blend: Some(blend),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    multiview_mask: None,
                    cache: None,
                })
        })?;
        self.pipelines.insert(mode, pipeline);
        self.total_pipelines_created += 1;
        Ok(true)
    }

    /// 为某个混合模式建过渡管线;不支持的模式返回 `None`。
    pub fn ensure_transition_pipeline(
        &mut self,
        ctx: &GpuContext,
        mode: BlendMode,
    ) -> Result<bool, Diag> {
        self.ensure_transition_variant(
            ctx,
            TransitionVariant::Fixed,
            mode,
        )
    }

    /// 引擎 psC(mask R)过渡管线。
    pub fn ensure_psc_pipeline(
        &mut self,
        ctx: &GpuContext,
        mode: BlendMode,
    ) -> Result<bool, Diag> {
        self.ensure_transition_variant(ctx, TransitionVariant::PsC, mode)
    }

    /// 引擎 psD(交叉淡化)过渡管线。
    pub fn ensure_psd_pipeline(
        &mut self,
        ctx: &GpuContext,
        mode: BlendMode,
    ) -> Result<bool, Diag> {
        self.ensure_transition_variant(ctx, TransitionVariant::PsD, mode)
    }

    fn ensure_transition_variant(
        &mut self,
        ctx: &GpuContext,
        variant: TransitionVariant,
        mode: BlendMode,
    ) -> Result<bool, Diag> {
        let cached = match variant {
            TransitionVariant::Fixed => self.transition_pipelines.contains_key(&mode),
            TransitionVariant::PsC => self.transition_psc_pipelines.contains_key(&mode),
            TransitionVariant::PsD => self.transition_psd_pipelines.contains_key(&mode),
        };
        if cached {
            return Ok(true);
        }
        let Some(blend) = blend_state(mode) else {
            return Ok(false);
        };
        let (shader, label) = match variant {
            TransitionVariant::Fixed => (&self.transition_shader, "transition"),
            TransitionVariant::PsC => (&self.transition_psc_shader, "transition psC"),
            TransitionVariant::PsD => (&self.transition_psd_shader, "transition psD"),
        };
        let label = format!("yu-backend-wgpu {} {}", label, mode.name());
        let pipeline = ctx.checked(code::TARGET, "过渡管线", || {
            ctx.device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(&label),
                    layout: Some(&self.transition_pipeline_layout),
                    vertex: vertex_state(shader),
                    primitive: primitive_state(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: shader,
                        entry_point: Some("fs_main"),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: TARGET_FORMAT,
                            blend: Some(blend),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    multiview_mask: None,
                    cache: None,
                })
        })?;
        match variant {
            TransitionVariant::Fixed => {
                self.transition_pipelines.insert(mode, pipeline);
            }
            TransitionVariant::PsC => {
                self.transition_psc_pipelines.insert(mode, pipeline);
            }
            TransitionVariant::PsD => {
                self.transition_psd_pipelines.insert(mode, pipeline);
            }
        }
        self.total_pipelines_created += 1;
        Ok(true)
    }

    pub fn transition_pipeline(&self, mode: BlendMode) -> Option<&wgpu::RenderPipeline> {
        self.transition_pipelines.get(&mode)
    }

    pub fn psc_pipeline(&self, mode: BlendMode) -> Option<&wgpu::RenderPipeline> {
        self.transition_psc_pipelines.get(&mode)
    }

    pub fn psd_pipeline(&self, mode: BlendMode) -> Option<&wgpu::RenderPipeline> {
        self.transition_psd_pipelines.get(&mode)
    }

    /// 为一次过渡建 bind group(纹理 + mask + fade uniform)。
    pub fn create_transition_bind_group(
        &mut self,
        ctx: &GpuContext,
        from: &wgpu::TextureView,
        mask: &wgpu::TextureView,
        filter: FilterKind,
        fade: &wgpu::Buffer,
    ) -> Result<wgpu::BindGroup, Diag> {
        let group = ctx.checked(code::TARGET, "过渡 bind group", || {
            ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("yu-backend-wgpu transition bind group"),
                layout: &self.transition_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(from),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.samplers[filter as usize]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(mask),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::Sampler(&self.samplers[filter as usize]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: fade.as_entire_binding(),
                    },
                ],
            })
        })?;
        self.total_bind_groups_created += 1;
        Ok(group)
    }

    /// 快照矩形填充管线:`alpha_only=false` 整像素,`true` 只写 alpha。
    pub fn ensure_paint_pipeline(
        &mut self,
        ctx: &GpuContext,
        alpha_only: bool,
    ) -> Result<(), Diag> {
        let index = usize::from(alpha_only);
        if self.paint_pipelines[index].is_some() {
            return Ok(());
        }
        let label = if alpha_only {
            "yu-backend-wgpu paint alpha"
        } else {
            "yu-backend-wgpu paint rgba"
        };
        let pipeline = ctx.checked(code::TARGET, "快照填充管线", || {
            ctx.device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(&self.paint_pipeline_layout),
                    vertex: vertex_state(&self.paint_shader),
                    primitive: primitive_state(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &self.paint_shader,
                        entry_point: Some("fs_main"),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: TARGET_FORMAT,
                            blend: None,
                            write_mask: if alpha_only {
                                wgpu::ColorWrites::ALPHA
                            } else {
                                wgpu::ColorWrites::ALL
                            },
                        })],
                    }),
                    multiview_mask: None,
                    cache: None,
                })
        })?;
        self.paint_pipelines[index] = Some(pipeline);
        self.total_pipelines_created += 1;
        Ok(())
    }

    pub fn paint_pipeline(&self, alpha_only: bool) -> Option<&wgpu::RenderPipeline> {
        self.paint_pipelines[usize::from(alpha_only)].as_ref()
    }

    /// 为 `(alpha_only, color)` 建/复用 RECTPAINT bind group。
    /// 同一颜色(通常是黑幕/按钮涂色)在大量 op 间复用,语义不变。
    pub fn ensure_paint_bind_group(
        &mut self,
        ctx: &GpuContext,
        alpha_only: bool,
        color: [f32; 4],
    ) -> Result<(), Diag> {
        let key = (alpha_only, color.map(f32::to_bits));
        if self.paint_groups.contains_key(&key) {
            return Ok(());
        }
        let uniform = wgpu::util::DeviceExt::create_buffer_init(
            &ctx.device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("yu-backend-wgpu paint color"),
                contents: bytemuck::cast_slice(&color),
                usage: wgpu::BufferUsages::UNIFORM,
            },
        );
        let group = ctx.checked(code::TARGET, "填充 bind group", || {
            ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("yu-backend-wgpu paint bind group"),
                layout: &self.paint_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                }],
            })
        })?;
        self.paint_groups.insert(key, group);
        self.total_bind_groups_created += 1;
        Ok(())
    }

    pub fn paint_bind_group(
        &self,
        alpha_only: bool,
        color: [u32; 4],
    ) -> Option<&wgpu::BindGroup> {
        self.paint_groups.get(&(alpha_only, color))
    }

    /// 为 `(texture, sampler)` 建 bind group;已存在则复用。
    pub fn ensure_bind_group(
        &mut self,
        ctx: &GpuContext,
        texture_id: u64,
        view: &wgpu::TextureView,
        filter: FilterKind,
    ) -> Result<(), Diag> {
        let key = (texture_id, filter);
        if self.bind_groups.contains_key(&key) {
            return Ok(());
        }
        let bind_group = ctx.checked(code::TARGET, "bind group", || {
            ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("yu-backend-wgpu bind group"),
                layout: &self.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.samplers[filter as usize]),
                    },
                ],
            })
        })?;
        self.bind_groups.insert(key, bind_group);
        self.total_bind_groups_created += 1;
        Ok(())
    }

    /// 目标纹理被重建时作废旧 bind group,避免持有已废弃的 view。
    pub fn forget_texture(&mut self, texture_id: u64) {
        self.bind_groups.retain(|(id, _), _| *id != texture_id);
    }
}

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

fn vertex_state(shader: &wgpu::ShaderModule) -> wgpu::VertexState<'_> {
    wgpu::VertexState {
        module: shader,
        entry_point: Some("vs_main"),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        buffers: &[Some(wgpu::VertexBufferLayout {
            array_stride: Vertex::SIZE,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x2,
                    offset: 0,
                    shader_location: 0,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x2,
                    offset: 8,
                    shader_location: 1,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32,
                    offset: 16,
                    shader_location: 2,
                },
            ],
        })],
    }
}

fn primitive_state() -> wgpu::PrimitiveState {
    wgpu::PrimitiveState {
        topology: wgpu::PrimitiveTopology::TriangleList,
        strip_index_format: None,
        front_face: wgpu::FrontFace::Ccw,
        cull_mode: None,
        unclipped_depth: false,
        polygon_mode: wgpu::PolygonMode::Fill,
        conservative: false,
    }
}

fn make_sampler(ctx: &GpuContext, kind: FilterKind) -> wgpu::Sampler {    let filter = match kind {
        FilterKind::Point => wgpu::FilterMode::Nearest,
        FilterKind::Linear => wgpu::FilterMode::Linear,
    };
    ctx.device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("yu-backend-wgpu sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: filter,
        min_filter: filter,
        // 引擎 MIP=-1(NONE);本后端不生成 mip,固定 LOD 0。
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        lod_min_clamp: 0.0,
        lod_max_clamp: 0.0,
        compare: None,
        anisotropy_clamp: 1,
        border_color: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blend_state_matches_backend_capabilities() {
        for mode in BlendMode::ALL {
            let expected = matches!(
                mode,
                BlendMode::Normal
                    | BlendMode::Add
                    | BlendMode::Subtract
                    | BlendMode::Multiply
                    | BlendMode::Screen
                    | BlendMode::Lighten
                    | BlendMode::Darken
                    | BlendMode::Exclusion
            );
            assert_eq!(blend_state(mode).is_some(), expected, "{}", mode.name());
        }
    }

    #[test]
    fn lighten_darken_use_max_min() {
        let lighten = blend_state(BlendMode::Lighten).expect("lighten");
        assert_eq!(lighten.color.operation, wgpu::BlendOperation::Max);
        assert_eq!(lighten.alpha.operation, wgpu::BlendOperation::Max);
        let darken = blend_state(BlendMode::Darken).expect("darken");
        assert_eq!(darken.color.operation, wgpu::BlendOperation::Min);
        assert_eq!(darken.alpha.operation, wgpu::BlendOperation::Min);
    }

    #[test]
    fn color_factor_modes_use_componentwise_d3d9_equivalents() {
        // D3D9 SRCCOLOR/INVDESTCOLOR/INVSRCCOLOR ↔ WebGPU Src/OneMinusDst/OneMinusSrc。
        let multiply = blend_state(BlendMode::Multiply).expect("multiply");
        assert_eq!(multiply.color.src_factor, wgpu::BlendFactor::Zero);
        assert_eq!(multiply.color.dst_factor, wgpu::BlendFactor::Src);
        assert_eq!(multiply.alpha.dst_factor, wgpu::BlendFactor::Src);

        let screen = blend_state(BlendMode::Screen).expect("screen");
        assert_eq!(screen.color.src_factor, wgpu::BlendFactor::OneMinusDst);
        assert_eq!(screen.color.dst_factor, wgpu::BlendFactor::One);
        assert_eq!(screen.alpha.src_factor, wgpu::BlendFactor::OneMinusDst);

        let exclusion = blend_state(BlendMode::Exclusion).expect("exclusion");
        assert_eq!(exclusion.color.src_factor, wgpu::BlendFactor::OneMinusDst);
        assert_eq!(exclusion.color.dst_factor, wgpu::BlendFactor::OneMinusSrc);
        assert_eq!(exclusion.alpha.src_factor, wgpu::BlendFactor::OneMinusDst);
        assert_eq!(exclusion.alpha.dst_factor, wgpu::BlendFactor::OneMinusSrc);
    }

    #[test]
    fn src_mode_follows_two_stage_state() {
        assert_eq!(src_mode(BlendMode::Normal), Some(0));
        assert_eq!(src_mode(BlendMode::Add), Some(0));
        assert_eq!(src_mode(BlendMode::Subtract), Some(1));
        assert_eq!(src_mode(BlendMode::Lighten), Some(1));
        assert_eq!(src_mode(BlendMode::Darken), Some(2));
        assert_eq!(src_mode(BlendMode::Multiply), Some(2));
        assert_eq!(src_mode(BlendMode::Divide), None);
    }

    #[test]
    fn normal_uses_d3d9_same_alpha_factor() {
        let state = blend_state(BlendMode::Normal).expect("normal");
        assert_eq!(state.color.src_factor, wgpu::BlendFactor::SrcAlpha);
        assert_eq!(state.color.dst_factor, wgpu::BlendFactor::OneMinusSrcAlpha);
        assert_eq!(state.color.operation, wgpu::BlendOperation::Add);
        // D3D9 未开 separate alpha:alpha 源项也是 SRCALPHA,即 a·a + dst·(1−a)。
        assert_eq!(state.alpha.src_factor, wgpu::BlendFactor::SrcAlpha);
        assert_eq!(state.alpha.dst_factor, wgpu::BlendFactor::OneMinusSrcAlpha);
    }

    #[test]
    fn add_and_subtract_use_d3d9_same_alpha_factor() {
        let add = blend_state(BlendMode::Add).expect("add");
        assert_eq!(add.color.src_factor, wgpu::BlendFactor::SrcAlpha);
        assert_eq!(add.color.dst_factor, wgpu::BlendFactor::One);
        assert_eq!(add.alpha.src_factor, wgpu::BlendFactor::SrcAlpha);
        assert_eq!(add.alpha.dst_factor, wgpu::BlendFactor::One);

        let sub = blend_state(BlendMode::Subtract).expect("subtract");
        assert_eq!(sub.color.src_factor, wgpu::BlendFactor::SrcAlpha);
        assert_eq!(sub.color.dst_factor, wgpu::BlendFactor::One);
        assert_eq!(sub.color.operation, wgpu::BlendOperation::ReverseSubtract);
        assert_eq!(sub.alpha.src_factor, wgpu::BlendFactor::SrcAlpha);
        assert_eq!(sub.alpha.operation, wgpu::BlendOperation::ReverseSubtract);
    }

    #[test]
    fn filter_kind_maps_anisotropic_to_linear() {
        assert_eq!(FilterKind::from_mode(FilterMode::Point), FilterKind::Point);
        assert_eq!(
            FilterKind::from_mode(FilterMode::Linear),
            FilterKind::Linear
        );
        assert_eq!(
            FilterKind::from_mode(FilterMode::Anisotropic),
            FilterKind::Linear
        );
    }
}
