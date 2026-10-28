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
use crate::ScaleFilter;
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
        BlendMode::Screen => both(
            BlendFactor::OneMinusDst,
            BlendFactor::One,
            BlendOperation::Add,
        ),
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
        BlendMode::Subtract | BlendMode::Screen | BlendMode::Lighten | BlendMode::Exclusion => 1,
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

/// 引擎显示缩放滤镜变体(effect Pass2/Pass3)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ScaleVariant {
    /// Pass2 (psB):4-tap 锐利双线性。
    PsB,
    /// Pass3 (psA):16-tap 双三次。
    PsA,
}

impl ScaleVariant {
    pub(crate) const fn from_filter(filter: ScaleFilter) -> Self {
        match filter {
            ScaleFilter::Sharp => Self::PsB,
            ScaleFilter::Bicubic => Self::PsA,
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::PsB => 0,
            Self::PsA => 1,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::PsB => "scale psB",
            Self::PsA => "scale psA",
        }
    }
}

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
    /// 引擎显示缩放 effect Pass2/Pass3(`DAT_00801334` 2/3)。
    scale_layout: wgpu::BindGroupLayout,
    scale_pipeline_layout: wgpu::PipelineLayout,
    scale_shaders: [wgpu::ShaderModule; 2],
    scale_pipelines: [Option<wgpu::RenderPipeline>; 2],
    /// 缩放 bind group:按 `(源纹理 id, 变体, uniform)` 缓存。
    scale_groups: BTreeMap<(u64, ScaleVariant, [u32; 4]), wgpu::BindGroup>,
    /// 快照上的 RECTPAINT 族:`[整像素, 只写 alpha]`。
    paint_layout: wgpu::BindGroupLayout,
    paint_pipeline_layout: wgpu::PipelineLayout,
    paint_shader: wgpu::ShaderModule,
    paint_pipelines: [Option<wgpu::RenderPipeline>; 2],
    /// `CGACT[MERGE/MERGE2]` 的 GPU 合成管线:`false` = 只写 RGB(保留目标
    /// alpha,`MERGE`),`true` = 连同 alpha 一起写(`MERGE2`)。
    merge_pipelines: [Option<wgpu::RenderPipeline>; 2],
    /// `CGACT[TRANSITION]` 的双源整数 lerp(两张纹理 → 目标表面)。
    lerp_shader: wgpu::ShaderModule,
    lerp_layout: wgpu::BindGroupLayout,
    lerp_pipeline_layout: wgpu::PipelineLayout,
    lerp_pipeline: Option<wgpu::RenderPipeline>,
    /// `CGACT[MERGE=1]` 的 GPU 整数合成(两张纹理,无 uniform)。
    layer_merge_shader: wgpu::ShaderModule,
    layer_merge_layout: wgpu::BindGroupLayout,
    layer_merge_pipeline_layout: wgpu::PipelineLayout,
    layer_merge_pipeline: Option<wgpu::RenderPipeline>,
    /// RECTPAINT 的颜色 uniform/bind group(同一颜色跨帧复用;语义不变)。
    paint_groups: BTreeMap<(bool, [u32; 4]), wgpu::BindGroup>,
    /// `CGACT[MERGE2/XOR/AND]` 的双纹理合成(lower + upper + mode uniform)。
    composite_shader: wgpu::ShaderModule,
    composite_layout: wgpu::BindGroupLayout,
    composite_pipeline_layout: wgpu::PipelineLayout,
    composite_pipeline: Option<wgpu::RenderPipeline>,
    /// 逐层原地滤镜族(reverse/镜像/meido/saido/akarusa/mono/sepia/mosaic/rotation)。
    transform_shader: wgpu::ShaderModule,
    transform_layout: wgpu::BindGroupLayout,
    transform_pipeline_layout: wgpu::PipelineLayout,
    transform_pipeline: Option<wgpu::RenderPipeline>,
    /// 区域重采样(`scale_region` 的 GPU 等价;物化 UV 采样窗)。
    resample_shader: wgpu::ShaderModule,
    resample_layout: wgpu::BindGroupLayout,
    resample_pipeline_layout: wgpu::PipelineLayout,
    resample_pipeline: Option<wgpu::RenderPipeline>,
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
        let transition_shader = ctx
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("yu-backend-wgpu transition"),
                source: wgpu::ShaderSource::Wgsl(TRANSITION_SHADER.into()),
            });
        let transition_psc_shader = ctx
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("yu-backend-wgpu transition psC"),
                source: wgpu::ShaderSource::Wgsl(crate::shaders::TRANSITION_PSC.into()),
            });
        let transition_psd_shader = ctx
            .device
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
        // `CGACT[TRANSITION]` 的双源整数 lerp(GPU):两张纹理 + f。
        let lerp_shader = ctx
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("yu-backend-wgpu lerp"),
                source: wgpu::ShaderSource::Wgsl(crate::shaders::LERP.into()),
            });
        let lerp_layout = ctx
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("yu-backend-wgpu lerp layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
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
        let lerp_pipeline_layout =
            ctx.device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("yu-backend-wgpu lerp pipeline layout"),
                    bind_group_layouts: &[Some(&lerp_layout)],
                    immediate_size: 0,
                });
        // `CGACT[MERGE]` 的 GPU 整数合成(两张纹理,无参数)。
        let layer_merge_shader = ctx
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("yu-backend-wgpu merge"),
                source: wgpu::ShaderSource::Wgsl(crate::shaders::MERGE.into()),
            });
        let layer_merge_layout = ctx
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("yu-backend-wgpu merge layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                ],
            });
        let layer_merge_pipeline_layout =
            ctx.device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("yu-backend-wgpu merge pipeline layout"),
                    bind_group_layouts: &[Some(&layer_merge_layout)],
                    immediate_size: 0,
                });
        // `CGACT[MERGE2/XOR/AND]`:两张纹理 + mode uniform。
        let composite_shader = ctx
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("yu-backend-wgpu composite"),
                source: wgpu::ShaderSource::Wgsl(crate::shaders::COMPOSITE.into()),
            });
        let composite_layout = ctx
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("yu-backend-wgpu composite layout"),
                entries: &[
                    texture_entry_unfilterable(0),
                    texture_entry_unfilterable(1),
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
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
        let composite_pipeline_layout =
            ctx.device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("yu-backend-wgpu composite pipeline layout"),
                    bind_group_layouts: &[Some(&composite_layout)],
                    immediate_size: 0,
                });
        // 原地滤镜族:一张纹理 + 参数 uniform。
        let transform_shader = ctx
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("yu-backend-wgpu transform"),
                source: wgpu::ShaderSource::Wgsl(crate::shaders::TRANSFORM.into()),
            });
        let transform_layout = ctx
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("yu-backend-wgpu transform layout"),
                entries: &[
                    texture_entry_unfilterable(0),
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
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
        let transform_pipeline_layout =
            ctx.device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("yu-backend-wgpu transform pipeline layout"),
                    bind_group_layouts: &[Some(&transform_layout)],
                    immediate_size: 0,
                });
        let resample_shader = ctx
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("yu-backend-wgpu resample"),
                source: wgpu::ShaderSource::Wgsl(crate::shaders::RESAMPLE.into()),
            });
        let resample_layout = ctx
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("yu-backend-wgpu resample layout"),
                entries: &[
                    texture_entry_unfilterable(0),
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
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
        let resample_pipeline_layout =
            ctx.device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("yu-backend-wgpu resample pipeline layout"),
                    bind_group_layouts: &[Some(&resample_layout)],
                    immediate_size: 0,
                });
        let samplers = [
            make_sampler(ctx, FilterKind::Point),
            make_sampler(ctx, FilterKind::Linear),
        ];
        let scale_psb_shader = ctx
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("yu-backend-wgpu scale psB"),
                source: wgpu::ShaderSource::Wgsl(crate::shaders::SCALE_PSB.into()),
            });
        let scale_psa_shader = ctx
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("yu-backend-wgpu scale psA"),
                source: wgpu::ShaderSource::Wgsl(crate::shaders::SCALE_PSA.into()),
            });
        let scale_layout = ctx
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("yu-backend-wgpu scale layout"),
                entries: &[
                    texture_entry(0),
                    sampler_entry(1),
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
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
        let scale_pipeline_layout =
            ctx.device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("yu-backend-wgpu scale pipeline layout"),
                    bind_group_layouts: &[Some(&scale_layout)],
                    immediate_size: 0,
                });
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
            scale_layout,
            scale_pipeline_layout,
            scale_shaders: [scale_psb_shader, scale_psa_shader],
            scale_pipelines: [None, None],
            scale_groups: BTreeMap::new(),
            paint_layout,
            paint_pipeline_layout,
            paint_shader,
            paint_pipelines: [None, None],
            merge_pipelines: [None, None],
            lerp_shader,
            lerp_layout,
            lerp_pipeline_layout,
            lerp_pipeline: None,
            layer_merge_shader,
            layer_merge_layout,
            layer_merge_pipeline_layout,
            layer_merge_pipeline: None,
            composite_shader,
            composite_layout,
            composite_pipeline_layout,
            composite_pipeline: None,
            transform_shader,
            transform_layout,
            transform_pipeline_layout,
            transform_pipeline: None,
            resample_shader,
            resample_layout,
            resample_pipeline_layout,
            resample_pipeline: None,
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
        self.ensure_transition_variant(ctx, TransitionVariant::Fixed, mode)
    }

    /// 引擎 psC(mask R)过渡管线。
    pub fn ensure_psc_pipeline(&mut self, ctx: &GpuContext, mode: BlendMode) -> Result<bool, Diag> {
        self.ensure_transition_variant(ctx, TransitionVariant::PsC, mode)
    }

    /// 引擎 psD(交叉淡化)过渡管线。
    pub fn ensure_psd_pipeline(&mut self, ctx: &GpuContext, mode: BlendMode) -> Result<bool, Diag> {
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

    /// 为显示缩放变体建管线(替换写,无 blend;全屏顶点由 shader 生成)。
    pub fn ensure_scale_pipeline(
        &mut self,
        ctx: &GpuContext,
        filter: ScaleFilter,
    ) -> Result<(), Diag> {
        let variant = ScaleVariant::from_filter(filter);
        if self.scale_pipelines[variant.index()].is_some() {
            return Ok(());
        }
        let label = format!("yu-backend-wgpu {}", variant.label());
        let shader = &self.scale_shaders[variant.index()];
        let pipeline = ctx.checked(code::TARGET, "显示缩放管线", || {
            ctx.device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(&label),
                    layout: Some(&self.scale_pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: shader,
                        entry_point: Some("vs_main"),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        buffers: &[],
                    },
                    primitive: primitive_state(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: shader,
                        entry_point: Some("fs_main"),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: TARGET_FORMAT,
                            blend: None,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    multiview_mask: None,
                    cache: None,
                })
        })?;
        self.scale_pipelines[variant.index()] = Some(pipeline);
        self.total_pipelines_created += 1;
        Ok(())
    }

    pub fn scale_pipeline(&self, filter: ScaleFilter) -> Option<&wgpu::RenderPipeline> {
        let variant = ScaleVariant::from_filter(filter);
        self.scale_pipelines[variant.index()].as_ref()
    }

    /// 为缩放 pass 建/复用 bind group。
    ///
    /// `tex_size` 是源纹理尺寸(引擎 `YS_TexSize`);`param` 对 psB 是
    /// `SharpN`(引擎恒为 1.25),对 psA 是 `YS_TexDiffU`(引擎恒为 2/W)。
    /// 采样器固定 point(D3DTEXF_POINT,`FUN_004195ec` 硬编码)。
    pub fn ensure_scale_bind_group(
        &mut self,
        ctx: &GpuContext,
        texture_id: u64,
        view: &wgpu::TextureView,
        filter: ScaleFilter,
        tex_size: [f32; 2],
        param: [f32; 2],
    ) -> Result<(), Diag> {
        let uniform = [tex_size[0], tex_size[1], param[0], param[1]];
        let key = (
            texture_id,
            ScaleVariant::from_filter(filter),
            uniform.map(f32::to_bits),
        );
        if self.scale_groups.contains_key(&key) {
            return Ok(());
        }
        let buffer = wgpu::util::DeviceExt::create_buffer_init(
            &ctx.device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("yu-backend-wgpu scale uniform"),
                contents: bytemuck::cast_slice(&uniform),
                usage: wgpu::BufferUsages::UNIFORM,
            },
        );
        let group = ctx.checked(code::TARGET, "缩放 bind group", || {
            ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("yu-backend-wgpu scale bind group"),
                layout: &self.scale_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(
                            &self.samplers[FilterKind::Point as usize],
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: buffer.as_entire_binding(),
                    },
                ],
            })
        })?;
        // buffer 必须活到 bind group 销毁:wgpu 的 BindGroup 持有资源引用,
        // 但为了明确生命周期,把 buffer 存进 group 缓存不现实;wgpu 内部
        // 引用计数保证存活,这里可以释放 Rust 句柄。
        self.scale_groups.insert(key, group);
        self.total_bind_groups_created += 1;
        Ok(())
    }

    pub fn scale_bind_group(
        &self,
        texture_id: u64,
        filter: ScaleFilter,
        tex_size: [f32; 2],
        param: [f32; 2],
    ) -> Option<&wgpu::BindGroup> {
        let uniform = [tex_size[0], tex_size[1], param[0], param[1]];
        self.scale_groups.get(&(
            texture_id,
            ScaleVariant::from_filter(filter),
            uniform.map(f32::to_bits),
        ))
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

    /// `CGACT[TRANSITION]` 的双源整数 lerp 管线(无顶点缓冲,全屏三角形)。
    pub fn ensure_lerp_pipeline(&mut self, ctx: &GpuContext) -> Result<(), Diag> {
        if self.lerp_pipeline.is_some() {
            return Ok(());
        }
        let pipeline = ctx.checked(code::TARGET, "lerp 管线", || {
            ctx.device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("yu-backend-wgpu lerp"),
                    layout: Some(&self.lerp_pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &self.lerp_shader,
                        entry_point: Some("vs_main"),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        buffers: &[],
                    },
                    primitive: primitive_state(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &self.lerp_shader,
                        entry_point: Some("fs_main"),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: TARGET_FORMAT,
                            blend: None,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    multiview_mask: None,
                    cache: None,
                })
        })?;
        self.lerp_pipeline = Some(pipeline);
        self.total_pipelines_created += 1;
        Ok(())
    }

    pub fn lerp_pipeline(&self) -> Option<&wgpu::RenderPipeline> {
        self.lerp_pipeline.as_ref()
    }

    /// 双源 lerp 的 bind group(两张纹理 + `f` uniform)。
    pub fn create_lerp_bind_group(
        &mut self,
        ctx: &GpuContext,
        from: &wgpu::TextureView,
        to: &wgpu::TextureView,
        f: u32,
    ) -> Result<wgpu::BindGroup, Diag> {
        let values: [u32; 4] = [f, 0, 0, 0];
        let buffer = wgpu::util::DeviceExt::create_buffer_init(
            &ctx.device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("yu-backend-wgpu lerp f"),
                contents: bytemuck::cast_slice(&values),
                usage: wgpu::BufferUsages::UNIFORM,
            },
        );
        let group = ctx.checked(code::TARGET, "lerp bind group", || {
            ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("yu-backend-wgpu lerp bind group"),
                layout: &self.lerp_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(from),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(to),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: buffer.as_entire_binding(),
                    },
                ],
            })
        })?;
        self.total_bind_groups_created += 1;
        Ok(group)
    }

    /// `CGACT[MERGE]` 的 GPU 整数合成管线(无顶点缓冲,全屏三角形)。
    pub fn ensure_layer_merge_pipeline(&mut self, ctx: &GpuContext) -> Result<(), Diag> {
        if self.layer_merge_pipeline.is_some() {
            return Ok(());
        }
        let pipeline = ctx.checked(code::TARGET, "merge 管线", || {
            ctx.device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("yu-backend-wgpu merge"),
                    layout: Some(&self.layer_merge_pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &self.layer_merge_shader,
                        entry_point: Some("vs_main"),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        buffers: &[],
                    },
                    primitive: primitive_state(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &self.layer_merge_shader,
                        entry_point: Some("fs_main"),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: TARGET_FORMAT,
                            blend: None,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    multiview_mask: None,
                    cache: None,
                })
        })?;
        self.layer_merge_pipeline = Some(pipeline);
        self.total_pipelines_created += 1;
        Ok(())
    }

    pub fn layer_merge_pipeline(&self) -> Option<&wgpu::RenderPipeline> {
        self.layer_merge_pipeline.as_ref()
    }

    /// `CGACT[MERGE2/XOR/AND]` 的双纹理合成管线(无顶点缓冲,全屏三角形)。
    pub fn ensure_composite_pipeline(&mut self, ctx: &GpuContext) -> Result<(), Diag> {
        if self.composite_pipeline.is_some() {
            return Ok(());
        }
        let pipeline = ctx.checked(code::TARGET, "composite 管线", || {
            fullscreen_pipeline(
                ctx,
                "yu-backend-wgpu composite",
                &self.composite_shader,
                &self.composite_pipeline_layout,
            )
        })?;
        self.composite_pipeline = Some(pipeline);
        self.total_pipelines_created += 1;
        Ok(())
    }

    pub fn composite_pipeline(&self) -> Option<&wgpu::RenderPipeline> {
        self.composite_pipeline.as_ref()
    }

    /// MERGE2/XOR/AND 的 bind group(`mode`:0=MERGE2,1=XOR,2=AND)。
    pub fn create_composite_bind_group(
        &mut self,
        ctx: &GpuContext,
        lower: &wgpu::TextureView,
        upper: &wgpu::TextureView,
        mode: u32,
    ) -> Result<wgpu::BindGroup, Diag> {
        let uniform = [mode, 0u32, 0, 0];
        let buffer = wgpu::util::DeviceExt::create_buffer_init(
            &ctx.device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("yu-backend-wgpu composite uniform"),
                contents: bytemuck::cast_slice(&uniform),
                usage: wgpu::BufferUsages::UNIFORM,
            },
        );
        let group = ctx.checked(code::TARGET, "composite bind group", || {
            ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("yu-backend-wgpu composite bind group"),
                layout: &self.composite_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(lower),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(upper),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: buffer.as_entire_binding(),
                    },
                ],
            })
        })?;
        self.total_bind_groups_created += 1;
        Ok(group)
    }

    /// 原地滤镜族管线(无顶点缓冲,全屏三角形)。
    pub fn ensure_transform_pipeline(&mut self, ctx: &GpuContext) -> Result<(), Diag> {
        if self.transform_pipeline.is_some() {
            return Ok(());
        }
        let pipeline = ctx.checked(code::TARGET, "transform 管线", || {
            fullscreen_pipeline(
                ctx,
                "yu-backend-wgpu transform",
                &self.transform_shader,
                &self.transform_pipeline_layout,
            )
        })?;
        self.transform_pipeline = Some(pipeline);
        self.total_pipelines_created += 1;
        Ok(())
    }

    pub fn transform_pipeline(&self) -> Option<&wgpu::RenderPipeline> {
        self.transform_pipeline.as_ref()
    }

    /// 滤镜 bind group。`params = [mode, set, set2, pad]`;`m` 只给 ROTATION。
    pub fn create_transform_bind_group(
        &mut self,
        ctx: &GpuContext,
        view: &wgpu::TextureView,
        params: [i32; 4],
        m: [f32; 4],
    ) -> Result<wgpu::BindGroup, Diag> {
        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct Uniform {
            mode: u32,
            set: i32,
            set2: i32,
            pad: u32,
            m: [f32; 4],
        }
        let uniform = [Uniform {
            mode: params[0] as u32,
            set: params[1],
            set2: params[2],
            pad: params[3] as u32,
            m,
        }];
        let buffer = wgpu::util::DeviceExt::create_buffer_init(
            &ctx.device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("yu-backend-wgpu transform uniform"),
                contents: bytemuck::cast_slice(&uniform),
                usage: wgpu::BufferUsages::UNIFORM,
            },
        );
        let group = ctx.checked(code::TARGET, "transform bind group", || {
            ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("yu-backend-wgpu transform bind group"),
                layout: &self.transform_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: buffer.as_entire_binding(),
                    },
                ],
            })
        })?;
        self.total_bind_groups_created += 1;
        Ok(group)
    }

    /// 区域重采样管线(`scale_region` 的 GPU 等价)。
    pub fn ensure_resample_pipeline(&mut self, ctx: &GpuContext) -> Result<(), Diag> {
        if self.resample_pipeline.is_some() {
            return Ok(());
        }
        let pipeline = ctx.checked(code::TARGET, "resample 管线", || {
            fullscreen_pipeline(
                ctx,
                "yu-backend-wgpu resample",
                &self.resample_shader,
                &self.resample_pipeline_layout,
            )
        })?;
        self.resample_pipeline = Some(pipeline);
        self.total_pipelines_created += 1;
        Ok(())
    }

    pub fn resample_pipeline(&self) -> Option<&wgpu::RenderPipeline> {
        self.resample_pipeline.as_ref()
    }

    /// 重采样 bind group:`region` = 闭区间源矩形,`dst` = 输出尺寸。
    pub fn create_resample_bind_group(
        &mut self,
        ctx: &GpuContext,
        view: &wgpu::TextureView,
        region: (i64, i64, i64, i64),
        dst: (u32, u32),
        bilinear: bool,
    ) -> Result<wgpu::BindGroup, Diag> {
        let uniform = [
            region.0 as i32,
            region.1 as i32,
            (region.2 - region.0 + 1) as i32,
            (region.3 - region.1 + 1) as i32,
            dst.0 as i32,
            dst.1 as i32,
            i32::from(bilinear),
            0i32,
        ];
        let buffer = wgpu::util::DeviceExt::create_buffer_init(
            &ctx.device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("yu-backend-wgpu resample uniform"),
                contents: bytemuck::cast_slice(&uniform),
                usage: wgpu::BufferUsages::UNIFORM,
            },
        );
        let group = ctx.checked(code::TARGET, "resample bind group", || {
            ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("yu-backend-wgpu resample bind group"),
                layout: &self.resample_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: buffer.as_entire_binding(),
                    },
                ],
            })
        })?;
        self.total_bind_groups_created += 1;
        Ok(group)
    }


    /// 双纹理 MERGE 的 bind group(lower + upper)。
    pub fn create_layer_merge_bind_group(
        &mut self,
        ctx: &GpuContext,
        lower: &wgpu::TextureView,
        upper: &wgpu::TextureView,
    ) -> Result<wgpu::BindGroup, Diag> {
        let group = ctx.checked(code::TARGET, "merge bind group", || {
            ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("yu-backend-wgpu merge bind group"),
                layout: &self.layer_merge_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(lower),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(upper),
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

    /// `CGACT[MERGE/MERGE2]` 合成管线。
    ///
    /// 官方 `FUN_004405a4` 把上层**表面**用通配绘制原语画到目标表面
    /// (`DAT_00801458(surface, …, X, Y, ratio, 0)`),是 D3D9 的一次带纹理
    /// quad,不是逐像素循环。这里用与图层相同的 QUAD shader + `Normal`
    /// 混合(`SRCALPHA/INVSRCALPHA`,与引擎未开 SEPARATEALPHABLENDENABLE
    /// 时一致):
    /// - `MERGE`:只写 RGB ⇒ 目标 alpha 保持不变(与官方「保留下层 alpha」
    ///   的可观测结果一致);
    /// - `MERGE2`:连 alpha 一起写。
    pub fn ensure_merge_pipeline(
        &mut self,
        ctx: &GpuContext,
        write_alpha: bool,
    ) -> Result<(), Diag> {
        let index = usize::from(write_alpha);
        if self.merge_pipelines[index].is_some() {
            return Ok(());
        }
        let Some(mut blend) = blend_state(BlendMode::Normal) else {
            return Ok(());
        };
        let Some(src_mode) = src_mode(BlendMode::Normal) else {
            return Ok(());
        };
        // `MERGE` 只写 RGB:alpha 用 `One/Zero`(恒等)混合,配合
        // `ColorWrites::COLOR` 不写出 —— 即目标 alpha 保持不变。
        // 直接沿用 Normal 的 alpha 混合会让 alpha 被上层覆盖,
        // 与软后端 `apply_merge`(merge2=false 不写 alpha)不一致。
        if !write_alpha {
            blend.alpha = wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::Zero,
                operation: wgpu::BlendOperation::Add,
            };
        }
        let constants = [("SRC_MODE", f64::from(src_mode))];
        let label = if write_alpha {
            "yu-backend-wgpu merge2"
        } else {
            "yu-backend-wgpu merge"
        };
        let pipeline = ctx.checked(code::TARGET, "MERGE 合成管线", || {
            ctx.device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(label),
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
                    primitive: primitive_state(),
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
                            write_mask: if write_alpha {
                                wgpu::ColorWrites::ALL
                            } else {
                                wgpu::ColorWrites::COLOR
                            },
                        })],
                    }),
                    multiview_mask: None,
                    cache: None,
                })
        })?;
        self.merge_pipelines[index] = Some(pipeline);
        self.total_pipelines_created += 1;
        Ok(())
    }

    pub fn merge_pipeline(&self, write_alpha: bool) -> Option<&wgpu::RenderPipeline> {
        self.merge_pipelines[usize::from(write_alpha)].as_ref()
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

    pub fn paint_bind_group(&self, alpha_only: bool, color: [u32; 4]) -> Option<&wgpu::BindGroup> {
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
        self.scale_groups.retain(|(id, _, _), _| *id != texture_id);
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

fn texture_entry_unfilterable(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

/// 全屏三角形的渲染管线(无顶点缓冲、无混合、替换写入)。
fn fullscreen_pipeline(
    ctx: &GpuContext,
    label: &str,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
) -> wgpu::RenderPipeline {
    ctx.device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: primitive_state(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: TARGET_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        })
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
