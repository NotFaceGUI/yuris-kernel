//! GPU 文本 pass:把 `CGACT[TEXT]` 字形直接写进图层常驻表面(compute)。
//!
//! # 为什么
//!
//! 旧路径把每个文本层整张基面 `read_back` 到 CPU、合成后再上传;语料实测
//! 角色出场帧 **2 次全屏同步读回 = 69–73ms**(`tcpu_rb`)。官方 `0x4056b8`
//! 在层表面就地写字形,不存在「先读回整张基面」这一步。
//!
//! # 做法
//!
//! - 基面仍是 GPU 纹理(`raw.tex`):先 `copy_texture_to_texture` 到本次
//!   文本修订的目标表面(恒等采样窗);缩放修饰走 CPU 回退。
//! - 字形位图仍由 `TextComposer`(fontdue)在 CPU 栅格化(与 soft 同一份
//!   像素),按 `composite_offsets` 打包成 draw list,上传一个小 storage
//!   buffer。
//! - compute shader 用 **整数运算**复刻 `Rgba::over` / `replace`
//!   (`yu-font/src/color.rs`),逐像素读目标表面、混合、写回;因此不需要
//!   GPU→CPU 读回,也不引入浮点混合误差。
//! - 能力不支持 `Rgba8Unorm` storage read-write 时,调用方走 CPU 合成回退
//!   (计数 + `text.gpu.unsupported` 诊断)。
//!
//! 目标表面来自 `LayerSurfaces::acquire`(带 `STORAGE_BINDING`),由 content
//! `Content` 持有;文本修订不变时缓存命中,不重拷、不重 dispatch。

use std::collections::BTreeMap;
use std::rc::Rc;

use yu_gfx::LayerId;
use yu_log::Diag;

use crate::device::GpuContext;
use crate::surfaces::{LayerSurfaces, TexRef};

/// `Rgba` 打包:`r | g<<8 | b<<16 | a<<24`(与 shader 一致)。
fn pack_rgba(rgba: &[u8]) -> Vec<u32> {
    rgba.chunks_exact(4)
        .map(|p| {
            u32::from(p[0])
                | (u32::from(p[1]) << 8)
                | (u32::from(p[2]) << 16)
                | (u32::from(p[3]) << 24)
        })
        .collect()
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuDraw {
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    src: u32,
    blend: u32,
    pad0: u32,
    pad1: u32,
}

/// 一个待编码的 GPU 文本任务(基面拷贝 + compute)。
pub(crate) struct TextGpuJob {
    pub base: Rc<TexRef>,
    pub target: Rc<TexRef>,
    bind_group: wgpu::BindGroup,
    pub draw_count: u32,
    pub glyphs: u32,
}

struct Entry {
    identity: u64,
    target: Rc<TexRef>,
    bytes: usize,
    last_use: u64,
}

/// GPU 文本管线 + 每层缓存。
pub(crate) struct TextGpuRenderer {
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    cache: BTreeMap<LayerId, Entry>,
    clock: u64,
    pub total_jobs: u64,
    pub total_glyphs: u64,
    pub total_draws: u64,
}

/// 缓存上限:整张全屏文本表面 3.6MB + 小 buffer;32 条约 120MB。
const TEXT_GPU_BUDGET_BYTES: usize = 128 * 1024 * 1024;
const TEXT_GPU_MAX_ENTRIES: usize = 32;

impl TextGpuRenderer {
    pub fn new(ctx: &GpuContext) -> Result<Self, Diag> {
        let error_scope = ctx.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let shader = ctx
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("yu-backend-wgpu text glyph"),
                source: wgpu::ShaderSource::Wgsl(include_str!("shaders/text_glyph.wgsl").into()),
            });
        let layout = ctx
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("yu-backend-wgpu text glyph layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::StorageTexture {
                            access: wgpu::StorageTextureAccess::ReadWrite,
                            format: wgpu::TextureFormat::Rgba8Unorm,
                            view_dimension: wgpu::TextureViewDimension::D2,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });
        let pipeline_layout = ctx
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("yu-backend-wgpu text glyph pipeline layout"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
        let pipeline = ctx
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("yu-backend-wgpu text glyph pipeline"),
                layout: Some(&pipeline_layout),
                module: &shader,
                entry_point: Some("main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                cache: None,
            });
        if let Some(error) = pollster::block_on(error_scope.pop()) {
            return Err(
                Diag::fatal(crate::device::code::TARGET, yu_log::Boundary::Platform)
                    .with_detail(format!("GPU 文本管线创建失败: {error}")),
            );
        }
        Ok(Self {
            pipeline,
            layout,
            cache: BTreeMap::new(),
            clock: 0,
            total_jobs: 0,
            total_glyphs: 0,
            total_draws: 0,
        })
    }

    /// 同一层 + 同一文本修订/基面身份:直接复用已写好的目标表面。
    pub fn cached(&mut self, layer: &LayerId, identity: u64) -> Option<Rc<TexRef>> {
        let entry = self.cache.get_mut(layer)?;
        if entry.identity != identity {
            return None;
        }
        self.clock += 1;
        entry.last_use = self.clock;
        Some(Rc::clone(&entry.target))
    }

    pub fn clear(&mut self) {
        self.cache.clear();
    }

    /// 拷贝基面 + 打包字形 + 建 bind group,返回目标表面与待编码任务。
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        &mut self,
        ctx: &GpuContext,
        surfaces: &mut LayerSurfaces,
        layer: &LayerId,
        identity: u64,
        base: Rc<TexRef>,
        placements: &[yu_render_core::GlyphPlacement],
    ) -> Result<(Rc<TexRef>, TextGpuJob), String> {
        if placements.is_empty() {
            return Err("text.gpu.no_glyphs".to_string());
        }
        let width = base.width;
        let height = base.height;
        let target = surfaces.acquire(ctx, width, height, "text_gpu")?;
        // 同一字形(Arc 相同)只打包一次。
        let mut packed: BTreeMap<usize, u32> = BTreeMap::new();
        let mut pixels: Vec<u32> = Vec::new();
        let mut draws: Vec<GpuDraw> = Vec::with_capacity(placements.len());
        for placement in placements {
            let key = std::sync::Arc::as_ptr(&placement.image) as usize;
            let src = match packed.get(&key) {
                Some(offset) => *offset,
                None => {
                    let offset = pixels.len() as u32;
                    pixels.extend_from_slice(&pack_rgba(&placement.image.rgba));
                    packed.insert(key, offset);
                    offset
                }
            };
            draws.push(GpuDraw {
                x: placement.x,
                y: placement.y,
                w: placement.image.width,
                h: placement.image.height,
                src,
                blend: u32::from(placement.blend),
                pad0: 0,
                pad1: 0,
            });
        }
        let glyph_bytes = (pixels.len() * std::mem::size_of::<u32>()).max(4) as u64;
        let draw_bytes = (draws.len() * std::mem::size_of::<GpuDraw>()).max(4) as u64;
        let glyph_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("yu-backend-wgpu text glyphs"),
            size: glyph_bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let draw_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("yu-backend-wgpu text draws"),
            size: draw_bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        ctx.queue
            .write_buffer(&glyph_buf, 0, bytemuck::cast_slice(&pixels));
        ctx.queue
            .write_buffer(&draw_buf, 0, bytemuck::cast_slice(&draws));
        let draw_count = draws.len() as u32;
        let count_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("yu-backend-wgpu text draw count"),
            size: 4,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        ctx.queue
            .write_buffer(&count_buf, 0, &draw_count.to_le_bytes());
        let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("yu-backend-wgpu text glyph bind group"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&target.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: glyph_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: draw_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: count_buf.as_entire_binding(),
                },
            ],
        });
        let bytes =
            (width as usize) * (height as usize) * 4 + glyph_bytes as usize + draw_bytes as usize;
        self.clock += 1;
        self.cache.insert(
            layer.clone(),
            Entry {
                identity,
                target: Rc::clone(&target),
                bytes,
                last_use: self.clock,
            },
        );
        self.evict();
        self.total_jobs += 1;
        self.total_glyphs += packed.len() as u64;
        self.total_draws += draws.len() as u64;
        Ok((
            Rc::clone(&target),
            TextGpuJob {
                base,
                target,
                bind_group,
                draw_count: draws.len() as u32,
                glyphs: packed.len() as u32,
            },
        ))
    }

    fn evict(&mut self) {
        while self.cache.len() > TEXT_GPU_MAX_ENTRIES
            || self.cache.values().map(|e| e.bytes).sum::<usize>() > TEXT_GPU_BUDGET_BYTES
        {
            let victim = self
                .cache
                .iter()
                .min_by_key(|(_, entry)| entry.last_use)
                .map(|(id, _)| id.clone());
            let Some(victim) = victim else {
                break;
            };
            self.cache.remove(&victim);
        }
    }
}

impl TextGpuRenderer {
    /// 编码:基面整面拷贝到目标表面,再按 draw 列表 dispatch compute。
    pub fn encode(&self, jobs: &[TextGpuJob], encoder: &mut wgpu::CommandEncoder) {
        for job in jobs {
            if job.base.id != job.target.id {
                encoder.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &job.base.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::TexelCopyTextureInfo {
                        texture: &job.target.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::Extent3d {
                        width: job.base.width,
                        height: job.base.height,
                        depth_or_array_layers: 1,
                    },
                );
            }
        }
        for job in jobs {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("yu-backend-wgpu text glyph pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &job.bind_group, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
    }
}
