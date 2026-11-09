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
//!
//! # 全局字形图集(T3)
//!
//! 字形位图在 `yu_font::GlyphCache` 里已按完整样式键跨层复用;**上传**原先
//! 仍按文本任务重复打包。这里再加一层显存侧共享:所有层共用一个字形图集,
//! 键 = 已合成字形的**像素内容身份**(宽/高/像素指纹 —— 字体、字号、样式、
//! 亚像素偏移、描边/阴影/旋转/配色都已烘进像素,因此键等价于完整样式键),
//! draw list 只记图集坐标。图集分页(每页 1024²)、有字节上限,只淘汰
//! "本帧没有任务引用"的旧页;页满/预算耗尽时返回错误,由调用方回退 CPU
//! 合成(像素仍正确,只是慢),不做近似。

use std::collections::BTreeMap;
use std::rc::Rc;

use yu_gfx::LayerId;
use yu_log::Diag;

use crate::device::GpuContext;
use crate::surfaces::{LayerSurfaces, TexRef};

/// `Rgba` 打包:`r | g<<8 | b<<16 | a<<24`(与 shader 一致)。

/// 图集键:字形**像素内容**身份。
///
/// 字体/字号/样式/亚像素/描边/阴影/旋转/配色全部已烘进 `rgba`,因此
/// 像素身份就是这些因素的最完整键(不同样式只要产出像素相同,共享
/// 图集条目是逐像素等价的)。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct GlyphAtlasKey {
    width: u32,
    height: u32,
    pixels: u64,
}

impl GlyphAtlasKey {
    fn of(image: &yu_font::GlyphImage) -> Self {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        fnv_mix(&mut hash, u64::from(image.width));
        fnv_mix(&mut hash, u64::from(image.height));
        for chunk in image.rgba.chunks_exact(4) {
            let word = u32::from(chunk[0])
                | (u32::from(chunk[1]) << 8)
                | (u32::from(chunk[2]) << 16)
                | (u32::from(chunk[3]) << 24);
            fnv_mix(&mut hash, u64::from(word));
        }
        Self {
            width: image.width,
            height: image.height,
            pixels: hash,
        }
    }
}

fn fnv_mix(hash: &mut u64, value: u64) {
    *hash ^= value;
    *hash = hash.wrapping_mul(0x100_0000_01b3);
}

/// 图集页边长(RGBA8 = 4MB/页)与总预算(8 页 = 32MB)。
///
/// 字形通常 10–40px 见方;一页可放数千个,菜单 170 字形远小于一页。
const ATLAS_PAGE: u32 = 1024;
const ATLAS_BUDGET_BYTES: usize = 32 * 1024 * 1024;

/// 图集空闲回收:连续这么多帧没有任何任务引用该页就释放(文本层已缓存
/// 的目标表面不受影响;文本变化重烘时字形会重新上传)。
const ATLAS_IDLE_FRAMES: u64 = 120;

struct AtlasPage {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    /// shelf 打包:当前行起点与行高。
    x: u32,
    y: u32,
    row_h: u32,
    /// 最近一次被任务引用的帧代(淘汰只清旧代)。
    generation: u64,
}

#[derive(Clone, Copy)]
struct AtlasEntry {
    page: usize,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

/// 跨层共享的字形图集。
struct GlyphAtlas {
    pages: Vec<AtlasPage>,
    entries: BTreeMap<GlyphAtlasKey, AtlasEntry>,
    generation: u64,
    hits: u64,
    misses: u64,
    evictions: u64,
}

impl GlyphAtlas {
    fn new() -> Self {
        Self {
            pages: Vec::new(),
            entries: BTreeMap::new(),
            generation: 0,
            hits: 0,
            misses: 0,
            evictions: 0,
        }
    }

    /// 新一帧:推进代,回收连续多帧无引用的页(预算内也回收)。
    fn begin_frame(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        loop {
            let victim = self
                .pages
                .iter()
                .enumerate()
                .filter(|(_, page)| {
                    page.generation.saturating_add(ATLAS_IDLE_FRAMES) < self.generation
                })
                .min_by_key(|(_, page)| page.generation)
                .map(|(index, _)| index);
            let Some(victim) = victim else {
                break;
            };
            self.evict_page(victim);
        }
        let budget_pages = ATLAS_BUDGET_BYTES / atlas_page_bytes();
        while self.pages.len() > budget_pages {
            let victim = self
                .pages
                .iter()
                .enumerate()
                .filter(|(_, page)| page.generation < self.generation.saturating_sub(1))
                .min_by_key(|(_, page)| page.generation)
                .map(|(index, _)| index);
            let Some(victim) = victim else {
                break;
            };
            self.evict_page(victim);
        }
    }

    fn evict_page(&mut self, index: usize) {
        self.pages.remove(index);
        self.entries.retain(|_, entry| entry.page != index);
        for entry in self.entries.values_mut() {
            if entry.page > index {
                entry.page -= 1;
            }
        }
        self.evictions += 1;
    }

    fn stats(&self) -> String {
        format!(
            "{}p/{}g/{}h/{}m/{}e",
            self.pages.len(),
            self.entries.len(),
            self.hits,
            self.misses,
            self.evictions
        )
    }

    /// 取或放入一个字形,返回 `(页下标, x, y, w, h)`。
    fn ensure(
        &mut self,
        ctx: &GpuContext,
        image: &yu_font::GlyphImage,
    ) -> Result<(usize, u32, u32, u32, u32), String> {
        let key = GlyphAtlasKey::of(image);
        if let Some(entry) = self.entries.get(&key) {
            self.pages[entry.page].generation = self.generation;
            self.hits += 1;
            return Ok((entry.page, entry.x, entry.y, entry.width, entry.height));
        }
        let (width, height) = (image.width, image.height);
        if width == 0 || height == 0 {
            return Err("text.gpu.atlas_empty_glyph".to_string());
        }
        if width > ATLAS_PAGE || height > ATLAS_PAGE {
            // 单字形大于整页:图集放不下,调用方回退 CPU(不拆分字形)。
            return Err("text.gpu.atlas_glyph_too_large".to_string());
        }
        let (page_index, x, y) = self.allocate(ctx, width, height)?;
        let page = &mut self.pages[page_index];
        ctx.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &page.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            &image.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        page.generation = self.generation;
        self.entries.insert(
            key,
            AtlasEntry {
                page: page_index,
                x,
                y,
                width,
                height,
            },
        );
        self.misses += 1;
        Ok((page_index, x, y, width, height))
    }

    fn allocate(
        &mut self,
        ctx: &GpuContext,
        width: u32,
        height: u32,
    ) -> Result<(usize, u32, u32), String> {
        // 先试当前页;放不下再新开一页(预算内),否则淘汰旧代页。
        for (index, page) in self.pages.iter_mut().enumerate() {
            if page.x + width > ATLAS_PAGE {
                page.y += page.row_h;
                page.x = 0;
                page.row_h = 0;
            }
            if page.y + height > ATLAS_PAGE {
                continue;
            }
            let (x, y) = (page.x, page.y);
            page.x += width;
            page.row_h = page.row_h.max(height);
            return Ok((index, x, y));
        }
        let budget_pages = ATLAS_BUDGET_BYTES / atlas_page_bytes();
        if self.pages.len() >= budget_pages {
            let victim = self
                .pages
                .iter()
                .enumerate()
                .filter(|(_, page)| page.generation < self.generation)
                .min_by_key(|(_, page)| page.generation)
                .map(|(index, _)| index);
            if let Some(victim) = victim {
                self.evict_page(victim);
            }
        }
        if self.pages.len() >= budget_pages {
            return Err("text.gpu.atlas_full".to_string());
        }
        let texture = ctx.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("yu-backend-wgpu glyph atlas"),
            size: wgpu::Extent3d {
                width: ATLAS_PAGE,
                height: ATLAS_PAGE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let index = self.pages.len();
        self.pages.push(AtlasPage {
            texture,
            view,
            x: width,
            y: 0,
            row_h: height,
            generation: self.generation,
        });
        Ok((index, 0, 0))
    }
}

fn atlas_page_bytes() -> usize {
    (ATLAS_PAGE as usize) * (ATLAS_PAGE as usize) * 4
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuDraw {
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    /// 图集内左上角。
    ax: u32,
    ay: u32,
    blend: u32,
    pad: u32,
}

/// 一个待编码的 GPU 文本任务(基面拷贝 + compute)。
///
/// 一个文本层可能因为字形跨图集页而拆成多个任务:同一目标表面,每个任务
/// 绑定自己的图集页;只有第一个任务负责把基面拷到目标表面。
pub(crate) struct TextGpuJob {
    pub base: Rc<TexRef>,
    pub target: Rc<TexRef>,
    bind_group: wgpu::BindGroup,
    pub draw_count: u32,
    pub glyphs: u32,
    pub copy_base: bool,
}

struct Entry {
    identity: u64,
    target: Rc<TexRef>,
    bytes: usize,
    last_use: u64,
}

/// GPU 文本管线 + 每层缓存 + 跨层字形图集。
pub(crate) struct TextGpuRenderer {
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    cache: BTreeMap<LayerId, Entry>,
    atlas: GlyphAtlas,
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
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Uint,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
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
            atlas: GlyphAtlas::new(),
            clock: 0,
            total_jobs: 0,
            total_glyphs: 0,
            total_draws: 0,
        })
    }

    /// 新一帧开始:推进图集代(淘汰"上一帧也没用"的旧页)。
    pub fn begin_frame(&mut self) {
        self.atlas.begin_frame();
    }

    /// 图集诊断(`页/字形/命中/未命中/淘汰`)。
    pub fn atlas_stats(&self) -> String {
        self.atlas.stats()
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
        // 引擎重启 = 新运行代际:图集里的字形属于上一局的字体/文本,
        // 一并释放(重启后按需重新上传)。
        self.atlas = GlyphAtlas::new();
    }

    /// 拷贝基面 + 字形进图集 + 建 bind group,返回目标表面与待编码任务。
    ///
    /// 一个文本层的字形可能分布在不同图集页(页满后新开页),因此返回
    /// **任务列表**:同一目标表面,第一个任务负责基面拷贝。
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        &mut self,
        ctx: &GpuContext,
        surfaces: &mut LayerSurfaces,
        layer: &LayerId,
        identity: u64,
        base: Rc<TexRef>,
        placements: &[yu_render_core::GlyphPlacement],
    ) -> Result<(Rc<TexRef>, Vec<TextGpuJob>), String> {
        if placements.is_empty() {
            return Err("text.gpu.no_glyphs".to_string());
        }
        let width = base.width;
        let height = base.height;
        // 同一层文本逐字变化时复用上一张目标表面(encode 会先整面拷贝基面,
        // 再写新字形);否则每显示一个字都 `acquire` 一张层尺寸纹理,
        // 一局游戏累积数千张,render 从 ~1ms 涨到几十 ms。
        let reused = self
            .cache
            .get(layer)
            .filter(|entry| {
                entry.target.width == width
                    && entry.target.height == height
                    && entry.target.id != base.id
            })
            .map(|entry| Rc::clone(&entry.target));
        let target = match reused {
            Some(target) => target,
            None => surfaces.acquire(ctx, width, height, "text_gpu")?,
        };
        // 按图集页分组:同一页的字形进同一个 draw list / compute job。
        let mut by_page: BTreeMap<usize, Vec<GpuDraw>> = BTreeMap::new();
        let mut unique_glyphs = std::collections::BTreeSet::new();
        for placement in placements {
            unique_glyphs.insert(GlyphAtlasKey::of(&placement.image));
            let (page, ax, ay, glyph_w, glyph_h) = self.atlas.ensure(ctx, &placement.image)?;
            by_page.entry(page).or_default().push(GpuDraw {
                x: placement.x,
                y: placement.y,
                w: glyph_w,
                h: glyph_h,
                ax,
                ay,
                blend: u32::from(placement.blend),
                pad: 0,
            });
        }
        let mut jobs = Vec::with_capacity(by_page.len());
        let mut copy_base = true;
        let mut draw_bytes_total = 0usize;
        for (page, draws) in by_page {
            let draw_bytes = (draws.len() * std::mem::size_of::<GpuDraw>()).max(4) as u64;
            draw_bytes_total += draw_bytes as usize;
            let draw_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("yu-backend-wgpu text draws"),
                size: draw_bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
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
                        resource: wgpu::BindingResource::TextureView(&self.atlas.pages[page].view),
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
            jobs.push(TextGpuJob {
                base: Rc::clone(&base),
                target: Rc::clone(&target),
                bind_group,
                draw_count,
                glyphs: if copy_base {
                    unique_glyphs.len() as u32
                } else {
                    0
                },
                copy_base,
            });
            copy_base = false;
        }
        let bytes = (width as usize) * (height as usize) * 4 + draw_bytes_total;
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
        self.total_jobs += jobs.len() as u64;
        self.total_glyphs += unique_glyphs.len() as u64;
        self.total_draws += placements.len() as u64;
        Ok((target, jobs))
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
    /// 编码:基面整面拷贝到目标表面(每个目标一次),再按 draw 列表 dispatch。
    pub fn encode(&self, jobs: &[TextGpuJob], encoder: &mut wgpu::CommandEncoder) {
        for job in jobs {
            if job.copy_base && job.base.id != job.target.id {
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
