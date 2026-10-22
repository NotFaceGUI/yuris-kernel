//! `yu-backend-wgpu` · `CompositorFrame` 的 wgpu 后端。
//!
//! 语义唯一来源是 `yu-gfx` 的 [`CompositorFrame`];`yu-backend-soft` 是整数
//! 参考执行器。本 crate 与软后端**互为实现,不互为真值**:像素允许舍入差,
//! 跳过/执行计数与跳过原因必须逐项一致(见 README 的差异账本)。
//!
//! # 执行契约
//!
//! 1. 按 `frame.passes` 语义顺序执行:纹理图像加工 → Offscreen(子先父后)
//!    → 复制/快照填充命令段 → Screen。COPY 需要组离屏结果,因此命令段放在
//!    Offscreen 之后;可见结果不变(复制只写快照,不直接呈现)。
//! 2. 固定管线 alpha = `min(max(A,0),255)/255`;着色器输出
//!    `alpha = texture.a × layer_alpha`(只乘一次),RGB 不预乘。
//! 3. 只执行 `MODE 0/1/2`;其余模式或 `OpStatus::Unimplemented` 跳过并计数。
//! 4. `frame.filter` 是全局设置;`Point` → nearest,
//!    `Linear/Anisotropic` → linear;无 mip。
//! 5. 组离屏尺寸 = `SX/SY`(未给用视口),clear = `pass.clear`;组的
//!    `A/MODE` 只在画进父目标时作用一次。
//! 6. Screen 尺寸 = `frame.viewport`,clear = `frame.clear`。
//! 7. 目标格式固定 [`wgpu::TextureFormat::Rgba8Unorm`];颜色按存储值直接
//!    混合,不做 sRGB↔线性转换。
//! 8. 双纹理过渡用独立 pipeline(配对 G 通道 mask,EVID-T-05);COPY/快照
//!    按 `CopyId` 冻结(EVID-T-12)。
//!
//! # 模块
//!
//! - [`device`]:instance/adapter/device 与 `platform.gpu.*` 诊断码;
//! - [`textures`]:`yu_backend_soft::TextureStore` 字节 → GPU 上传/目标缓存;
//! - [`pipelines`]:blend 0/1/2 × `Rgba8Unorm` 管线与 bind group 缓存;
//! - [`passes`]:pass 顺序、纹理解析、计数与顶点生成;
//! - [`readback`]:GPU 纹理 → 256 字节行对齐缓冲 → 紧凑 RGBA8。
//!
//! # 最小用法
//!
//! ```no_run
//! # use yu_backend_wgpu::WgpuRenderer;
//! # use yu_backend_soft::TextureStore;
//! # use yu_gfx::CompositorFrame;
//! # use yu_vfs::{Codec, Mount};
//! # fn demo(frame: &CompositorFrame, mount: Mount) {
//! let store = TextureStore::new(
//!     Box::new(mount),
//!     Codec::Cp932,
//!     frame.image_probe_order.clone(),
//! );
//! let mut backend = WgpuRenderer::new(store).expect("GPU 初始化");
//! let out = backend.render_frame(frame).expect("渲染");
//! assert_eq!((out.surface.width, out.surface.height),
//!            (frame.viewport.width, frame.viewport.height));
//! # let _ = out.report;
//! # }
//! ```
//!
//! 本 crate 只渲染到离屏 RGBA8 并读回;surface 呈现、窗口、winit/wasm 绑定
//! 留给宿主会话。库内不读文件、不读环境变量、不拼路径。

#![forbid(unsafe_code)]
#![allow(clippy::result_large_err)]

pub mod device;
pub mod passes;
pub mod pipelines;
pub mod readback;
mod shaders;
pub mod textures;

pub use device::{AdapterSummary, DeviceOptions, GpuContext};
pub use readback::ReadbackCache;
pub use textures::{GpuTexture, RenderTarget, TextureCache};

pub use yu_backend_soft::{AssetIssue, RenderReport, SkippedOp, Surface, TextureStore};

use yu_gfx::{BlendMode, CompositorFrame};
use yu_log::Diag;

use crate::device::backend_name;
use crate::passes::PlannedTarget;
use crate::pipelines::PipelineCache;
use crate::textures::{TargetSet, Vertex};

/// 一帧的 GPU 统计(报告/示例打印用)。
#[derive(Clone, Debug)]
pub struct GpuStats {
    pub adapter: AdapterSummary,
    /// 本帧实际 draw call 数(每个可执行 op 一次)。
    pub draw_calls: u64,
    /// 本帧新上传纹理数。
    pub texture_uploads: u64,
    /// 本帧新上传字节(解码后的 RGBA8 字节,不含 wgpu 内部行填充)。
    pub upload_bytes: u64,
    /// 累计上传纹理数。
    pub total_texture_uploads: u64,
    /// 累计上传字节。
    pub total_upload_bytes: u64,
    /// 累计创建的管线数。
    pub pipelines_created: u64,
    /// 累计创建的 bind group 数。
    pub bind_groups_created: u64,
    /// 当前管线缓存条目数。
    pub pipelines: usize,
    /// 当前 bind group 缓存条目数。
    pub bind_groups: usize,
    /// 当前缓存的纹理数(资产 + 1×1 黑图)。
    pub cached_textures: usize,
    /// 当前缓存的离屏目标数。
    pub offscreen_targets: usize,
    /// 本帧顶点缓冲字节数。
    pub vertex_bytes: u64,
    /// 本帧读回字节数。
    pub readback_bytes: u64,
}

/// `render_frame` 的输出。
#[derive(Clone, Debug)]
pub struct WgpuRenderOutput {
    pub surface: Surface,
    /// 与 `yu-backend-soft::RenderReport` 同一结构/字段语义。
    pub report: RenderReport,
    pub stats: GpuStats,
}

struct VertexBufferSlot {
    buffer: wgpu::Buffer,
    capacity: u64,
}

/// wgpu 渲染器:持有设备、纹理解析缓存、GPU 纹理/目标/管线缓存。
pub struct WgpuRenderer {
    ctx: GpuContext,
    textures: TextureStore,
    gpu_textures: TextureCache,
    pipelines: PipelineCache,
    targets: TargetSet,
    vertex_buffer: Option<VertexBufferSlot>,
    readback: ReadbackCache,
    total_draw_calls: u64,
    /// 图像加工覆盖纹理的跨帧缓存:`LayerId -> (gpu_id, w, h, content_hash)`。
    /// 内容未变时复用 GPU 纹理,避免逐帧重传(600 帧验收)。
    uploaded_overrides: std::collections::BTreeMap<yu_gfx::LayerId, (u64, u32, u32, u64)>,
    /// `CGACT[SIZE/SIZE2]` 结果缓存(与 soft 同一键/算法;EVID-J3-04)。
    image_cache: std::collections::BTreeMap<yu_backend_soft::ImageCacheKey, passes::Override>,
    /// 上一帧呈现内容(`COPY ID=:/W0/` 读后缓冲;EVID-J3-06)。
    backbuffer: Option<Surface>,
    /// 后缓冲 GPU 纹理缓存:`(content_hash, gpu_id)`。
    backbuffer_gpu: Option<(u64, u64)>,
}

impl WgpuRenderer {
    /// 用默认 [`DeviceOptions`] 初始化。
    ///
    /// 无适配器时返回 [`device::code::ADAPTER_UNAVAILABLE`];调用方应显式
    /// 跳过并计数,不要空跑通过。
    pub fn new(textures: TextureStore) -> Result<Self, Diag> {
        Self::new_with_options(textures, &DeviceOptions::default())
    }

    /// 指定后端/适配器名称子串等选项初始化。
    pub fn new_with_options(textures: TextureStore, options: &DeviceOptions) -> Result<Self, Diag> {
        let ctx = GpuContext::new(options)?;
        let pipelines = PipelineCache::new(&ctx)?;
        Ok(Self {
            ctx,
            textures,
            gpu_textures: TextureCache::new(),
            pipelines,
            targets: TargetSet::new(),
            vertex_buffer: None,
            readback: ReadbackCache::new(),
            total_draw_calls: 0,
            uploaded_overrides: std::collections::BTreeMap::new(),
            image_cache: std::collections::BTreeMap::new(),
            backbuffer: None,
            backbuffer_gpu: None,
        })
    }

    pub fn adapter(&self) -> &AdapterSummary {
        &self.ctx.adapter_info
    }

    pub fn backend(&self) -> &'static str {
        backend_name(self.ctx.backend)
    }

    pub fn textures(&self) -> &TextureStore {
        &self.textures
    }

    pub fn textures_mut(&mut self) -> &mut TextureStore {
        &mut self.textures
    }

    /// 累计 draw call 数(跨帧)。
    pub fn total_draw_calls(&self) -> u64 {
        self.total_draw_calls
    }

    /// 执行一帧并读回 `frame.viewport` 尺寸的 RGBA8。
    pub fn render_frame(&mut self, frame: &CompositorFrame) -> Result<WgpuRenderOutput, Diag> {
        self.textures.begin_frame();
        let uploads_before = self.gpu_textures.total_uploads;
        let bytes_before = self.gpu_textures.total_upload_bytes;

        let (plan, report) = passes::plan(
            &self.ctx,
            frame,
            &mut self.textures,
            &mut self.gpu_textures,
            &mut self.pipelines,
            &mut self.targets,
            &mut self.uploaded_overrides,
            &mut self.image_cache,
            self.backbuffer.as_ref(),
            &mut self.backbuffer_gpu,
        )?;

        let vertex_bytes = bytemuck::cast_slice::<Vertex, u8>(&plan.vertices);
        if !plan.vertices.is_empty() {
            self.upload_vertices(vertex_bytes)?;
        }
        let draw_calls = plan.draw_calls() as u64;
        self.total_draw_calls += draw_calls;

        let mut surface = self.execute(frame, &plan)?;
        // 屏幕是 opaque 窗口(与 soft 的最终屏幕归一、浏览器 canvas 的
        // alphaMode=opaque 一致);组 RT alpha 不进入呈现读回。
        for px in surface.rgba.chunks_exact_mut(4) {
            px[3] = 255;
        }

        self.backbuffer = Some(surface.clone());
        let stats = GpuStats {
            adapter: self.ctx.adapter_info.clone(),
            draw_calls,
            texture_uploads: self.gpu_textures.total_uploads - uploads_before,
            upload_bytes: self.gpu_textures.total_upload_bytes - bytes_before,
            total_texture_uploads: self.gpu_textures.total_uploads,
            total_upload_bytes: self.gpu_textures.total_upload_bytes,
            pipelines_created: self.pipelines.total_pipelines_created,
            bind_groups_created: self.pipelines.total_bind_groups_created,
            pipelines: self.pipelines.pipelines(),
            bind_groups: self.pipelines.bind_groups(),
            cached_textures: self.gpu_textures.cached_textures(),
            offscreen_targets: self.targets.offscreen_count(),
            vertex_bytes: vertex_bytes.len() as u64,
            readback_bytes: surface.rgba.len() as u64,
        };

        Ok(WgpuRenderOutput {
            surface,
            report,
            stats,
        })
    }

    fn upload_vertices(&mut self, bytes: &[u8]) -> Result<(), Diag> {
        let needed = bytes.len() as u64;
        let recreate = self
            .vertex_buffer
            .as_ref()
            .is_none_or(|slot| slot.capacity < needed);
        if recreate {
            let capacity = needed.next_power_of_two().max(64 * 1024);
            let buffer = self.ctx.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("yu-backend-wgpu vertices"),
                size: capacity,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.vertex_buffer = Some(VertexBufferSlot { buffer, capacity });
        }
        let slot = self
            .vertex_buffer
            .as_ref()
            .expect("vertex buffer created above");
        self.ctx.queue.write_buffer(&slot.buffer, 0, bytes);
        Ok(())
    }

    fn execute(
        &mut self,
        frame: &CompositorFrame,
        plan: &passes::FramePlan,
    ) -> Result<Surface, Diag> {
        let width = frame.viewport.width;
        let height = frame.viewport.height;
        let mut encoder = self
            .ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("yu-backend-wgpu frame"),
            });

        {
            let pipelines = &self.pipelines;
            let targets = &self.targets;
            let gpu = &self.gpu_textures;
            let vertex_buffer = self.vertex_buffer.as_ref().map(|slot| &slot.buffer);
            for pass in &plan.passes {
                let Some(planned_target) = pass.target.as_ref() else {
                    continue;
                };
                if matches!(planned_target, PlannedTarget::Commands) {
                    // COPY:GPU 纹理复制(源 → 快照)。
                    for copy in &pass.copies {
                        let source = gpu
                            .get(copy.source_id)
                            .map(|texture| &texture.texture)
                            .or_else(|| targets.texture_by_id(copy.source_id))
                            .ok_or_else(|| {
                                Diag::fatal(
                                    device::code::TARGET,
                                    yu_log::Boundary::Platform,
                                )
                                .with_detail(format!(
                                    "COPY 源纹理 {} 缺失",
                                    copy.source_id
                                ))
                            })?;
                        let dest = targets.snapshot(copy.dest).ok_or_else(|| {
                            Diag::fatal(device::code::TARGET, yu_log::Boundary::Platform)
                                .with_detail("COPY 目标快照缺失")
                        })?;
                        encoder.copy_texture_to_texture(
                            wgpu::TexelCopyTextureInfo {
                                texture: source,
                                mip_level: 0,
                                origin: wgpu::Origin3d {
                                    x: copy.origin.0,
                                    y: copy.origin.1,
                                    z: 0,
                                },
                                aspect: wgpu::TextureAspect::All,
                            },
                            wgpu::TexelCopyTextureInfo {
                                texture: &dest.texture,
                                mip_level: 0,
                                origin: wgpu::Origin3d::ZERO,
                                aspect: wgpu::TextureAspect::All,
                            },
                            wgpu::Extent3d {
                                width: copy.extent.0,
                                height: copy.extent.1,
                                depth_or_array_layers: 1,
                            },
                        );
                    }
                    // 快照上的 RECTPAINT 族:逐条独立 render pass(Load 保留内容)。
                    for paint in &pass.paints {
                        let target = targets.snapshot(paint.target).ok_or_else(|| {
                            Diag::fatal(device::code::TARGET, yu_log::Boundary::Platform)
                                .with_detail("快照填充目标缺失")
                        })?;
                        let pipeline = pipelines
                            .paint_pipeline(paint.alpha_only)
                            .ok_or_else(|| {
                                Diag::fatal(
                                    device::code::TARGET,
                                    yu_log::Boundary::Platform,
                                )
                                .with_detail("快照填充管线缺失")
                            })?;
                        let bind = pipelines
                            .paint_bind_group(paint.alpha_only, paint.color)
                            .ok_or_else(|| {
                                Diag::fatal(
                                    device::code::TARGET,
                                    yu_log::Boundary::Platform,
                                )
                                .with_detail("快照填充 bind group 缺失")
                            })?;
                        let attachments = [Some(wgpu::RenderPassColorAttachment {
                            view: &target.view,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Load,
                                store: wgpu::StoreOp::Store,
                            },
                        })];
                        let mut render_pass =
                            encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                label: Some("yu-backend-wgpu snapshot paint"),
                                color_attachments: &attachments,
                                depth_stencil_attachment: None,
                                timestamp_writes: None,
                                occlusion_query_set: None,
                                multiview_mask: None,
                            });
                        if let Some(buffer) = vertex_buffer {
                            render_pass.set_vertex_buffer(0, buffer.slice(..));
                        }
                        render_pass.set_pipeline(pipeline);
                        render_pass.set_bind_group(0, bind, &[]);
                        render_pass.draw(paint.first_vertex..paint.first_vertex + 6, 0..1);
                    }
                    continue;
                }
                let target = match planned_target {
                    PlannedTarget::Screen => targets.screen().ok_or_else(|| {
                        Diag::fatal(device::code::TARGET, yu_log::Boundary::Platform)
                            .with_detail("缺少屏幕目标")
                    })?,
                    PlannedTarget::Offscreen(id) => targets.offscreen(id).ok_or_else(|| {
                        Diag::fatal(device::code::TARGET, yu_log::Boundary::Platform)
                            .with_detail(format!("缺少离屏目标 {}", id.display()))
                    })?,
                    PlannedTarget::Commands => unreachable!("handled above"),
                };
                let attachments = [Some(wgpu::RenderPassColorAttachment {
                    view: &target.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(pass.clear),
                        store: wgpu::StoreOp::Store,
                    },
                })];
                let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("yu-backend-wgpu pass"),
                    color_attachments: &attachments,
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                if let Some(buffer) = vertex_buffer {
                    if !pass.draws.is_empty() {
                        render_pass.set_vertex_buffer(0, buffer.slice(..));
                    }
                }
                let mut current_pipeline: Option<(BlendMode, u8)> = None;
                let mut current_bind = None;
                for (index, draw) in pass.draws.iter().enumerate() {
                    // 0=quad,1=固定过渡,2=psC,3=psD;管线键必须区分变体。
                    let (pipeline, pipe_kind) = match &draw.kind {
                        passes::DrawKind::Quad => (pipelines.pipeline(draw.blend), 0u8),
                        passes::DrawKind::Transition { .. } => {
                            (pipelines.transition_pipeline(draw.blend), 1)
                        }
                        passes::DrawKind::TransitionPsc { .. } => {
                            (pipelines.psc_pipeline(draw.blend), 2)
                        }
                        passes::DrawKind::TransitionPsd { .. } => {
                            (pipelines.psd_pipeline(draw.blend), 3)
                        }
                    };
                    let pipeline = pipeline.ok_or_else(|| {
                        Diag::fatal(device::code::TARGET, yu_log::Boundary::Platform)
                            .with_detail(format!(
                                "缺少 blend {} 的{}管线",
                                draw.blend.name(),
                                match pipe_kind {
                                    0 => "",
                                    _ => "过渡",
                                }
                            ))
                    })?;
                    let key = (draw.blend, pipe_kind);
                    if current_pipeline != Some(key) {
                        render_pass.set_pipeline(pipeline);
                        current_pipeline = Some(key);
                    }
                    let bind = match &draw.kind {
                        passes::DrawKind::Transition { bind_index }
                        | passes::DrawKind::TransitionPsc { bind_index }
                        | passes::DrawKind::TransitionPsd { bind_index } => {
                            &plan.transition_bind_groups[*bind_index as usize]
                        }
                        passes::DrawKind::Quad => pipelines
                            .bind_group(draw.texture_id, draw.filter)
                            .ok_or_else(|| {
                                Diag::fatal(
                                    device::code::TARGET,
                                    yu_log::Boundary::Platform,
                                )
                                .with_detail(format!(
                                    "缺少纹理 {} / {:?} 的 bind group",
                                    draw.texture_id, draw.filter
                                ))
                            })?,
                    };
                    let bind_key = match &draw.kind {
                        passes::DrawKind::Transition { bind_index }
                        | passes::DrawKind::TransitionPsc { bind_index }
                        | passes::DrawKind::TransitionPsd { bind_index } => {
                            (u64::MAX - u64::from(*bind_index), draw.filter)
                        }
                        passes::DrawKind::Quad => (draw.texture_id, draw.filter),
                    };
                    if current_bind != Some(bind_key) {
                        render_pass.set_bind_group(0, bind, &[]);
                        current_bind = Some(bind_key);
                    }
                    let first = pass.first_vertex + (index as u32) * 6;
                    render_pass.draw(first..first + 6, 0..1);
                }
            }
        }

        let screen = self.targets.screen().ok_or_else(|| {
            Diag::fatal(device::code::TARGET, yu_log::Boundary::Platform)
                .with_detail("帧结束时没有屏幕目标")
        })?;
        self.readback
            .read(&self.ctx, &screen.texture, width, height, encoder)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertex_layout_is_tightly_packed() {
        assert_eq!(Vertex::SIZE, 20);
        let vertex = Vertex {
            position: [1.0, 2.0],
            uv: [3.0, 4.0],
            alpha: 5.0,
        };
        let bytes = bytemuck::bytes_of(&vertex);
        assert_eq!(bytes.len(), 20);
        assert_eq!(f32::from_ne_bytes(bytes[0..4].try_into().unwrap()), 1.0);
        assert_eq!(f32::from_ne_bytes(bytes[16..20].try_into().unwrap()), 5.0);
    }
}
