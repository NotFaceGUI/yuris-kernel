//! 管线与 bind group 缓存。
//!
//! 管线按 `(blend mode, Rgba8Unorm)` 缓存;本 crate 只有一个目标格式,所以
//! 键就是 [`BlendMode`]。bind group 按 `(texture id, sampler kind)` 缓存,
//! 纹理身份不变就不会重建。
//!
//! 混合状态按 `yu-gfx` 固定管线的 D3D9 状态表实现(见
//! `docs/render-parity.md` §5.2/§12.3),`MODE 0/1/2`:
//!
//! | MODE | color | alpha(同因子) |
//! |---|---|---|
//! | 0 normal | `src·a + dst·(1−a)` | `a·a + dst·(1−a)` |
//! | 1 add | `src·a + dst` | `a·a + dst` |
//! | 2 subtract | `dst − src·a` | `dst − a·a` |
//!
//! 着色器输出 `alpha = texture.a × layer_alpha`,RGB 不预乘;`a` 即这个
//! 乘积。D3D9 未启用 `SEPARATEALPHABLENDENABLE` 时 alpha 与 RGB 使用同一组
//! 因子,所以 alpha 源项是 `a²`(SRCALPHA 因子作用于 alpha 通道)。
//! soft 当前把 alpha 源项写成 `a`(已知 P1 偏离,见 README 差异账本);
//! 本后端按状态表执行,两者只在半透明 alpha 上分叉,计数不受影响。
//! 其余 MODE 返回 `None`,由执行器跳过并计数。

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

/// D3D9 固定管线状态 → wgpu 混合状态。只接受 0/1/2。
pub fn blend_state(mode: BlendMode) -> Option<wgpu::BlendState> {
    use wgpu::{BlendComponent, BlendFactor, BlendOperation};

    let color = |src_factor, dst_factor, operation| BlendComponent {
        src_factor,
        dst_factor,
        operation,
    };
    Some(match mode {
        BlendMode::Normal => wgpu::BlendState {
            color: color(
                BlendFactor::SrcAlpha,
                BlendFactor::OneMinusSrcAlpha,
                BlendOperation::Add,
            ),
            alpha: color(
                BlendFactor::SrcAlpha,
                BlendFactor::OneMinusSrcAlpha,
                BlendOperation::Add,
            ),
        },
        BlendMode::Add => wgpu::BlendState {
            color: color(BlendFactor::SrcAlpha, BlendFactor::One, BlendOperation::Add),
            alpha: color(BlendFactor::SrcAlpha, BlendFactor::One, BlendOperation::Add),
        },
        BlendMode::Subtract => wgpu::BlendState {
            color: color(
                BlendFactor::SrcAlpha,
                BlendFactor::One,
                BlendOperation::ReverseSubtract,
            ),
            alpha: color(
                BlendFactor::SrcAlpha,
                BlendFactor::One,
                BlendOperation::ReverseSubtract,
            ),
        },
        _ => return None,
    })
}

const SHADER: &str = r#"
struct VsIn {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) alpha: f32,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) alpha: f32,
};

@vertex
fn vs_main(in: VsIn) -> VsOut {
    return VsOut(vec4<f32>(in.position, 0.0, 1.0), in.uv, in.alpha);
}

@group(0) @binding(0) var src_texture: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let texel = textureSample(src_texture, src_sampler, in.uv);
    // 纹理 alpha × 层 alpha 只乘一次;RGB 保持直通(不预乘)。
    return vec4<f32>(texel.rgb, texel.a * in.alpha);
}
"#;

/// 管线 + bind group 缓存。
pub struct PipelineCache {
    bind_group_layout: wgpu::BindGroupLayout,
    layout: wgpu::PipelineLayout,
    shader: wgpu::ShaderModule,
    samplers: [wgpu::Sampler; 2],
    pipelines: BTreeMap<BlendMode, wgpu::RenderPipeline>,
    bind_groups: BTreeMap<(u64, FilterKind), wgpu::BindGroup>,
    pub total_pipelines_created: u64,
    pub total_bind_groups_created: u64,
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
        let label = format!("yu-backend-wgpu blend {}", mode.name());
        let pipeline = ctx.checked(code::TARGET, "渲染管线", || {
            ctx.device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(&label),
                    layout: Some(&self.layout),
                    vertex: wgpu::VertexState {
                        module: &self.shader,
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
        self.pipelines.insert(mode, pipeline);
        self.total_pipelines_created += 1;
        Ok(true)
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

fn make_sampler(ctx: &GpuContext, kind: FilterKind) -> wgpu::Sampler {
    let filter = match kind {
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
    fn only_modes_0_1_2_have_blend_state() {
        for mode in BlendMode::ALL {
            let expected = matches!(
                mode,
                BlendMode::Normal | BlendMode::Add | BlendMode::Subtract
            );
            assert_eq!(blend_state(mode).is_some(), expected, "{}", mode.name());
        }
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
