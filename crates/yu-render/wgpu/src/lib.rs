//! `yu-backend-wgpu` · `CompositorFrame` 的 wgpu 后端。
//!
//! 语义唯一来源是 `yu-gfx` 的 [`CompositorFrame`]。
//!
//! # 定位:独立的 GPU 实现
//!
//! 本 crate 是 `--backend wgpu`(GPU 模式)的**独立实现**:像素工作必须由
//! GPU 自己完成,运行期**不得**调用 `yu-backend-soft` 的像素算法
//! (`apply_*` / `blend_*` / `scale_region` / `blit`)。软后端是同契约的
//! **CPU 模式**与对拍基线,不是本 crate 的兜底,也不是实现库 ——
//! 违反这条会让双后端失去交叉验证价值,并让 GPU 模式被 CPU 逐像素循环
//! 拖住(已发生过多轮)。允许共享的只有契约与平台设施:
//! `Surface`/`RenderReport`/`SkippedOp`/`AssetIssue`、`TextureStore`(资产解码)、
//! `TextComposer`(字形栅格化)。判定规则与迁移台账见
//! `docs/architecture/backends.md`。
//!
//! 像素允许舍入差;跳过/执行计数与跳过原因必须逐项一致(差异账本见 README)。
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
//! - [`textures`]:`yu_render_core::TextureStore` 字节 → GPU 上传/目标缓存;
//! - [`pipelines`]:blend 0/1/2 × `Rgba8Unorm` 管线与 bind group 缓存;
//! - [`passes`]:pass 顺序、纹理解析、计数与顶点生成;
//! - [`readback`]:GPU 纹理 → 256 字节行对齐缓冲 → 紧凑 RGBA8。
//!
//! # 最小用法
//!
//! ```no_run
//! # use yu_backend_wgpu::WgpuRenderer;
//! # use yu_render_core::TextureStore;
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
pub(crate) mod plan;
pub mod readback;
mod shaders;
pub mod surfaces;
mod text_gpu;
pub mod textures;

pub use device::{AdapterSummary, AndroidNdkDisplay, DeviceOptions, GpuBackend, GpuContext};
pub use readback::ReadbackCache;
pub use textures::{GpuTexture, RenderTarget, TextureCache};

pub use yu_render_core::{AssetIssue, RenderReport, SkippedOp, Surface, TextureStore};

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;

use yu_gfx::{BlendMode, CompositorFrame, CopyId, Viewport};
use yu_log::Diag;

use crate::device::backend_name;
use crate::passes::PlannedTarget;
use crate::pipelines::PipelineCache;
use crate::surfaces::LayerSurfaces;
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
    /// 当前图层级 GPU 合成目标数(`CGACT[TRANSITION]` 的 GPU 路径)。
    pub layer_composites: usize,
    /// 本帧顶点缓冲字节数。
    pub vertex_bytes: u64,
    /// 本帧读回字节数。
    pub readback_bytes: u64,
}

/// `render_frame` 的输出。
///
/// `surface` 用 `Arc` 共享:未缩放帧的读回结果同时是下一帧 `:/W0/`
/// 后缓冲,引擎/宿主也要消费。共享后每帧省掉一次 1.9MB 整帧拷贝。
#[derive(Clone, Debug)]
pub struct WgpuRenderOutput {
    pub surface: Arc<Surface>,
    /// 与 `yu-backend-soft::RenderReport` 同一结构/字段语义。
    pub report: RenderReport,
    pub stats: GpuStats,
}

/// [`WgpuRenderer::render_frame_streaming`] 的产物。
///
/// `surface` 是**上一帧**呈现内容(后缓冲语义);屏幕纹理本体经
/// [`WgpuRenderer::screen_target`] 采样,`screen_*` 是它的身份键。
pub struct StreamingFrameOutput {
    pub surface: Arc<Surface>,
    pub report: RenderReport,
    pub stats: GpuStats,
    pub screen_id: u64,
    pub screen_width: u32,
    pub screen_height: u32,
}

/// 引擎显示缩放滤镜(effect Pass2/Pass3)。
///
/// 证据:`FUN_004195ec`(`0x4195ec`)按 `DAT_00801334` 选 pass:
/// `2 → BeginPass(2)`(psB),否则 `BeginPass(3)`(psA);采样器被硬编码为
/// `D3DTEXF_POINT`(1/1/1)。4/8 组 texel UV 由 `FUN_0041a308` 生成,
/// 详见 `docs/shader-parity.md` §3.3。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScaleFilter {
    /// Pass2 (psB):4-tap 锐利双线性;`SharpN = 1.25`(FXO 初值,引擎不写)。
    Sharp,
    /// Pass3 (psA):16-tap 双三次;`YS_TexDiffU = (2/W, 0)`。
    Bicubic,
}

struct VertexBufferSlot {
    buffer: wgpu::Buffer,
    capacity: u64,
}

/// `SAVE[ID]` 命令时刻的按需快照读回端口。
struct WgpuSnapshotSource {
    ctx: GpuContext,
    readback: readback::ReadbackCache,
    registry: Rc<RefCell<BTreeMap<CopyId, Rc<surfaces::TexRef>>>>,
}

impl yu_render_core::SnapshotSource for WgpuSnapshotSource {
    fn read(&mut self, id: CopyId) -> Option<Surface> {
        let (width, height, texture) = {
            let registry = self.registry.borrow();
            let target = registry.get(&id)?;
            (target.width, target.height, target.texture.clone())
        };
        let encoder = self
            .ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("yu-backend-wgpu snapshot source"),
            });
        self.readback
            .read_now(&self.ctx, &texture, width, height, encoder)
            .ok()
    }
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
    /// 显示缩放输出的读回(与主读回分开:同帧两次读共用一块缓冲会互踩)。
    scale_readback: ReadbackCache,
    /// 快照按需读回(SAVE 抓帧器缺失 CPU 镜像时;与屏幕流式读回分开,
    /// 不能共用缓冲:流式读回的下一次 `collect` 会被踩掉)。
    snapshot_readback: ReadbackCache,
    /// 本帧 GPU 纹理拷贝字节/次数(诊断;`copy_texture_to_texture` 的读写合计)。
    frame_copy_bytes: u64,
    frame_copy_count: u64,
    total_draw_calls: u64,
    /// 图层常驻表面(每层一张 GPU 纹理;`CGACT` 就地改它)。
    surfaces: LayerSurfaces,
    /// 文本合成的上传缓存(内容哈希命中时不重传)。
    text_uploads: plan::TextUploads,
    /// 上一帧呈现内容(`COPY ID=:/W0/` 读后缓冲;EVID-J3-06)。
    backbuffer: Option<Arc<Surface>>,
    /// 后缓冲 GPU 纹理缓存:`(content_hash, gpu_id)`。
    backbuffer_gpu: Option<(u64, u64)>,
    /// 文本栅格化/合成器(与 soft 同一实现;懒加载系统字体)。
    text: yu_render_core::TextComposer,
    /// GPU 文本 pass(字形直接写层表面;能力不支持时为 `None` → CPU 回退)。
    text_gpu: Option<text_gpu::TextGpuRenderer>,
    /// 快照句柄表(每帧刷新;供 SAVE[ID] 在命令时刻按需读回上一帧快照)。
    snapshot_registry: Rc<RefCell<BTreeMap<CopyId, Rc<surfaces::TexRef>>>>,
    /// 最近一帧的阶段耗时(性能取证;不进入 `RenderReport` 的对账字段)。
    last_phases: yu_render_core::RenderPhases,
    /// 手机画质选项:把 `SIZE`(官方最近邻)也按双线性采样。
    ///
    /// 默认 false(与官方一致);Android 壳可在配置页开启,缓解高 DPI 屏幕
    /// 上 CG/差分缩放的锯齿。见 `DIV-GFX-083-mobile-bilinear-size`。
    force_bilinear_scale: bool,
}

impl WgpuRenderer {
    /// 用默认 [`DeviceOptions`] 初始化。
    ///
    /// 无适配器时返回 [`device::code::ADAPTER_UNAVAILABLE`];调用方应显式
    /// 跳过并计数,不要空跑通过。
    pub fn new(textures: TextureStore) -> Result<Self, Diag> {
        Self::new_with_options(textures, &DeviceOptions::default())
    }

    /// 在命令时刻预热资产:解码 + GPU 上传(官方 `CG[FILE]` 在命令处理体内
    /// 同步装载纹理;渲染首用只命中缓存)。失败由 `TextureStore` 缓存,
    /// 渲染阶段的 `asset.*` 诊断照常产出。
    pub fn preload_assets(&mut self, keys: &[Vec<u8>]) -> (u64, u64) {
        self.textures.preload_parallel(keys);
        let mut errors = 0u64;
        for key in keys {
            if let Err(diag) = self
                .gpu_textures
                .ensure_asset(&self.ctx, &mut self.textures, key)
            {
                errors += 1;
                // 与 soft 后端同口径:官方 `CG[FILE]` 装载失败静默,缺资产
                // 由 `RenderReport`(missing assets / `asset.missing` 跳过项)
                // 统计,不在启动时按 WARN 刷屏。
                log::debug!(
                    target: "yu-render",
                    "[asset] preload miss key={}: {} {}",
                    String::from_utf8_lossy(key),
                    diag.code,
                    diag.detail
                );
            }
        }
        (keys.len() as u64, errors)
    }

    /// 预取(默认关闭):只解 CPU 像素进预取池,不上传 GPU。真正的
    /// `CG[FILE]` 到来时 `preload_assets` 命中池再上传,像素与路径不变。
    pub fn prefetch_assets(&mut self, keys: &[Vec<u8>]) -> u64 {
        self.textures.prefetch_request(keys)
    }

    /// 预取是否生效。
    pub fn prefetch_enabled(&self) -> bool {
        self.textures.prefetch_enabled()
    }

    /// 开关快照延迟物化(默认跟随 `YURIS_SNAPSHOT_DEFER`;测试/宿主用)。
    pub fn set_defer_snapshots(&mut self, enabled: bool) {
        self.targets.set_defer_enabled(enabled);
    }

    /// 快照延迟物化是否生效。
    pub fn defer_snapshots_enabled(&self) -> bool {
        self.targets.defer_enabled()
    }

    /// 开启/关闭 `SIZE` 的双线性采样(手机画质选项;默认关闭 = 官方最近邻)。
    pub fn set_force_bilinear_scale(&mut self, enabled: bool) {
        self.force_bilinear_scale = enabled;
    }

    /// 刷新快照句柄表(主渲染提交之后调用;供下一次 SAVE[ID] 按需读回)。
    pub fn refresh_snapshot_registry(&self) {
        *self.snapshot_registry.borrow_mut() = self.targets.snapshot_refs();
    }

    /// 构造按需快照读回端口(共享同一设备/队列与快照句柄表)。
    pub fn snapshot_source(&self) -> Box<dyn yu_render_core::SnapshotSource> {
        Box::new(WgpuSnapshotSource {
            ctx: self.ctx.clone(),
            readback: readback::ReadbackCache::new(),
            registry: Rc::clone(&self.snapshot_registry),
        })
    }

    /// 用宿主预建的 GPU 上下文初始化(跨平台注入口)。
    ///
    /// 宿主自己决定底层接口(GLES/Vulkan)与回退顺序 —— Android 模拟器的
    /// gfxstream Vulkan 会在 `vkQueueSubmit` 内空指针崩溃,需要宿主选 GLES。
    pub fn new_with_context(textures: TextureStore, ctx: GpuContext) -> Result<Self, Diag> {
        Self::from_context(textures, ctx)
    }

    /// 指定后端/适配器名称子串等选项初始化。
    pub fn new_with_options(textures: TextureStore, options: &DeviceOptions) -> Result<Self, Diag> {
        let ctx = GpuContext::new(options)?;
        Self::from_context(textures, ctx)
    }

    /// 实际构造(`new_with_options` 与 `new_with_context` 共用)。
    fn from_context(textures: TextureStore, ctx: GpuContext) -> Result<Self, Diag> {
        let pipelines = PipelineCache::new(&ctx)?;
        let gpu_text_enabled = std::env::var("YURIS_TEXT_GPU")
            .map(|v| {
                !matches!(
                    v.trim().to_ascii_lowercase().as_str(),
                    "0" | "off" | "false"
                )
            })
            .unwrap_or(true);
        let text_gpu = if ctx.text_storage && gpu_text_enabled {
            match text_gpu::TextGpuRenderer::new(&ctx) {
                Ok(renderer) => Some(renderer),
                Err(diag) => {
                    log::warn!(
                        target: "yu-render",
                        "[text-gpu] 禁用,回退 CPU 合成: {}",
                        diag.detail
                    );
                    None
                }
            }
        } else {
            None
        };
        Ok(Self {
            ctx,
            textures,
            gpu_textures: TextureCache::new(),
            pipelines,
            targets: TargetSet::new(),
            vertex_buffer: None,
            readback: ReadbackCache::new(),
            scale_readback: ReadbackCache::new(),
            snapshot_readback: ReadbackCache::new(),
            frame_copy_bytes: 0,
            frame_copy_count: 0,
            total_draw_calls: 0,
            surfaces: LayerSurfaces::new(),
            text_uploads: plan::TextUploads::default(),
            backbuffer: None,
            backbuffer_gpu: None,
            text: yu_render_core::TextComposer::new(),
            text_gpu,
            snapshot_registry: Rc::new(RefCell::new(BTreeMap::new())),
            last_phases: yu_render_core::RenderPhases::default(),
            force_bilinear_scale: false,
        })
    }

    /// 用调用方字体源构造文本合成器(测试/自带字体)。
    pub fn with_text_source(mut self, source: yu_font::FontSource) -> Self {
        self.text = yu_render_core::TextComposer::with_source(source);
        self
    }

    /// 文本合成器(诊断/测试用)。
    pub fn text_composer(&mut self) -> &mut yu_render_core::TextComposer {
        &mut self.text
    }

    /// 最近一帧的阶段耗时(微秒;宿主逐帧 CSV / 长帧取证)。
    #[must_use]
    pub fn render_phases(&self) -> yu_render_core::RenderPhases {
        self.last_phases
    }

    /// GPU 资源计数:`(合成目标, 组离屏, 快照)`。用于退出摘要核对显存不泄漏。
    #[must_use]
    pub fn resource_counts(&self) -> (usize, usize, usize) {
        (
            self.surfaces.stats().live_surfaces,
            self.targets.offscreen_count(),
            self.targets.snapshot_count(),
        )
    }

    /// 常驻层表面统计(退出摘要/诊断)。
    #[must_use]
    pub fn surface_stats(&self) -> crate::surfaces::SurfaceStats {
        self.surfaces.stats()
    }

    /// 全部 GPU 目标占用字节(诊断:确认显存不随场景累积)。
    #[must_use]
    pub fn target_bytes(&self) -> usize {
        self.targets.allocated_bytes() + self.surfaces.total_bytes()
    }

    /// 快照纹理是否存在(存档抓帧的按需读回入口)。
    #[must_use]
    pub fn has_snapshot(&self, id: CopyId) -> bool {
        self.targets.snapshot(id).is_some()
    }

    /// 预枚举系统字体(宿主启动时;见 `TextComposer::warm`)。
    pub fn warm_text(&mut self) {
        let _ = self.text.warm();
    }

    /// 系统字体源摘要(预热后非空;见 `TextComposer::source_summary`)。
    #[must_use]
    pub fn font_summary(&self) -> Option<&str> {
        self.text.source_summary()
    }

    /// 强制所有 `FONT` 请求使用指定 family(`None` = 恢复脚本语义)。
    pub fn set_force_font(&mut self, family: Option<String>) {
        self.text.set_force_font(family);
    }

    pub fn adapter(&self) -> &AdapterSummary {
        &self.ctx.adapter_info
    }

    /// 底层 GPU 上下文。宿主窗口/上屏层必须复用同一个 instance/device/queue,
    /// 否则纹理不能跨设备共享。
    pub fn gpu(&self) -> &GpuContext {
        &self.ctx
    }

    pub fn backend(&self) -> &'static str {
        backend_name(self.ctx.backend)
    }

    /// 丢弃跨帧状态(重启用;DIV-HOST-001)。
    ///
    /// 引擎重启 = 新设备:后缓冲、快照、离屏组目标、图像加工缓存都属于上一
    /// 轮 `GfxState`。`CopyId` 在新 `GfxState` 里从 0 重新分配,若不清理,
    /// 新复制会命中同 id 的旧快照(冻结早退)而拿到重启前的像素。
    ///
    /// 纹理解析缓存(`TextureStore`)与管线缓存保留 —— 它们只依赖游戏数据,
    /// 与运行代际无关。
    pub fn reset(&mut self) {
        self.backbuffer = None;
        self.backbuffer_gpu = None;
        self.surfaces.reset();
        for id in self.text_uploads.clear() {
            self.gpu_textures.remove(id, &mut self.pipelines);
        }
        if let Some(text_gpu) = &mut self.text_gpu {
            text_gpu.clear();
        }
        self.snapshot_registry.borrow_mut().clear();
        self.targets.clear_snapshots(&mut self.pipelines);
        self.targets.clear_offscreen(&mut self.pipelines);
        // 重启 = 新运行代际:文本/回缓冲等临时上传与 bind group 全部作废。
        // 资产纹理保留(只依赖游戏数据),避免重启后重新解码。
        self.gpu_textures.clear_transient(&mut self.pipelines);
        self.pipelines.clear_runtime_groups();
        self.readback = ReadbackCache::new();
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
        self.render_inner(frame, None, false).map(|(out, _)| out)
    }

    /// 执行一帧,并在读回前跑引擎的显示缩放 pass(effect Pass2/Pass3)。
    ///
    /// 合成仍在 `frame.viewport`(引擎内部游戏画面)完成;缩放 pass 把屏幕
    /// 纹理按 `output` 尺寸重采样后读回。`output == frame.viewport` 时仍执行
    /// 同一 pass(对应引擎的 1:1 缩放路径)。
    ///
    /// 这是加法 API:`render_frame` 行为不变;需要窗口缩放/滤镜的宿主显式
    /// 调用本方法。`frame.viewport` 仍是引擎逻辑分辨率。
    pub fn render_frame_scaled(
        &mut self,
        frame: &CompositorFrame,
        output: Viewport,
        filter: ScaleFilter,
    ) -> Result<WgpuRenderOutput, Diag> {
        self.render_inner(frame, Some((output, filter)), false)
            .map(|(out, _)| out)
    }

    /// 流水线友好帧(桌面宿主专用):读回改为**异步一帧滞后**
    /// (见 [`readback`模块文档]`crate::readback`),不再每帧同步等待 GPU;
    /// 同时返回屏幕目标的身份,宿主可直接采样该纹理呈现
    /// (省掉「读回 CPU → 再整帧上传」的两次整帧拷贝)。
    ///
    /// 返回的 `surface` 是**上一帧**呈现内容(后缓冲/COPY :/W0/ 语义,
    /// 与精确模式下的「上一帧后缓冲」一致);需要当帧像素时用
    /// [`Self::read_screen_now`]。
    pub fn render_frame_streaming(
        &mut self,
        frame: &CompositorFrame,
    ) -> Result<StreamingFrameOutput, Diag> {
        let (out, screen) = self.render_inner(frame, None, true)?;
        let Some(screen) = screen else {
            return Err(
                Diag::fatal(device::code::TARGET, yu_log::Boundary::Platform)
                    .with_detail("流式帧没有屏幕目标"),
            );
        };
        Ok(StreamingFrameOutput {
            surface: out.surface,
            report: out.report,
            stats: out.stats,
            screen_id: screen.0,
            screen_width: screen.1,
            screen_height: screen.2,
        })
    }

    /// 当前屏幕目标(流式呈现用):`(代际 id, 宽, 高, 纹理)`。
    ///
    /// 宿主以 `(id, 宽, 高)` 为键缓存视图/bind group;尺寸或代际变化
    /// (视口重建)时重建。队列提交顺序保证「上一提交采样 < 下一提交写入」,
    /// 跨提交直接采样屏幕纹理是安全的。
    #[must_use]
    pub fn screen_target(&self) -> Option<(u64, u32, u32, &wgpu::Texture)> {
        let target = self.targets.screen()?;
        Some((target.id, target.width, target.height, &target.texture))
    }

    /// 同步读回一张**快照**纹理(SAVE 抓帧器的按需镜像;低频)。
    ///
    /// 只读回该快照,不重放整帧:差分切换每帧产生新 CopyId,旧实现让离屏
    /// 软后端整帧重放(实测 300–360ms/次),这里 1–8ms。
    pub fn read_snapshot(&mut self, id: CopyId) -> Option<Surface> {
        let (width, height, texture) = {
            let target = self.targets.snapshot(id)?;
            (target.width, target.height, target.texture.clone())
        };
        let encoder = self
            .ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("yu-backend-wgpu read snapshot"),
            });
        let surface = self
            .snapshot_readback
            .read_now(&self.ctx, &texture, width, height, encoder)
            .ok()?;
        Some(surface)
    }

    /// 同步读回**当前**屏幕纹理(`--shot` 末帧精确像素;低频)。
    pub fn read_screen_now(&mut self) -> Result<Surface, Diag> {
        let (width, height, texture) = {
            let target = self.targets.screen().ok_or_else(|| {
                Diag::fatal(device::code::TARGET, yu_log::Boundary::Platform)
                    .with_detail("没有屏幕目标可读")
            })?;
            (target.width, target.height, target.texture.clone())
        };
        let encoder = self
            .ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("yu-backend-wgpu read screen now"),
            });
        let mut surface = self
            .readback
            .read_now(&self.ctx, &texture, width, height, encoder)?;
        force_opaque(&mut surface);
        Ok(surface)
    }

    /// 已物化层的**逻辑显示尺寸**(`层名, 宽, 高`),供宿主做逐帧判异。
    ///
    /// 用**逻辑尺寸**而不是纹理尺寸:`SIZE*` 是零拷贝 UV 别名,内容可能是
    /// 一张 1×1 纯色纹理而逻辑尺寸是 643×825(画出来正常)。用纹理尺寸判
    /// 会把合法的纯色别名误报成"退化成 1×1"。
    ///
    /// 只读元数据,不做 GPU 读回,可以每帧调用。
    pub fn materialized_layer_sizes(&self) -> Vec<(String, u32, u32)> {
        self.surfaces
            .materialized_layer_logical()
            .into_iter()
            .map(|(id, w, h)| (id.display(), w, h))
            .collect()
    }

    /// **全量资源快照**(诊断;每 60 帧打一次)。
    ///
    /// 一次给齐「会不会越来越卡」需要的所有计数,避免反复取数:
    ///
    /// - `created/reused/released`:本帧新建/池复用/归还的层表面纹理数。
    ///   `created` 持续偏高而 `reused` 很低 ⇒ 纹理每帧重建(GPU 侧碎片化,
    ///   驱动越来越慢,而 CPU 的 `render_ms` 完全看不出来)。
    /// - `materialized`:本帧为 UV 别名重采样的次数(每次一张全尺寸纹理)。
    /// - `pooled/pooled_bytes`:池里空闲纹理;池只增不减也会吃显存。
    /// - `live_surfaces/live_bytes`:常驻层表面。
    /// - `composites/offscreen/snapshots`:合成目标/离屏/快照数。
    /// - `dd/dfr`:同帧快照去重次数 / 延迟物化登记数(见 `DIV-GFX-082`)。
    /// - `atlas`:字形图集 `页/字形/命中/未命中/淘汰`。
    /// - `pf`:预取池 `命中/解码失败/占用MB/挂起`(`YURIS_PREFETCH=1` 才有值)。
    pub fn resource_snapshot(&self) -> String {
        let s = self.surfaces.stats();
        let (composites, offscreen, snapshots) = self.targets.resource_counts();
        let atlas = self
            .text_gpu
            .as_ref()
            .map_or_else(|| "-".to_string(), |text| text.atlas_stats());
        let (pf_hits, _, pf_errors, pf_bytes, pf_pending) = self.textures.prefetch_stats();
        format!(
            "created={} reused={} released={} mat={} passes={} blits={} dd={} dfr={} \
             dsw={} tel={} live={}/{}MB pooled={}/{}MB comps={} off={} snap={} \
             bg={} atlas={} pf={}h/{}e/{}MB/{}p vram={}MB",
            s.created,
            s.reused,
            s.released,
            s.materialized,
            s.surface_passes,
            s.blits,
            s.snapshot_deduped,
            s.snapshots_deferred,
            s.dead_writes,
            s.transitions_elided,
            s.live_surfaces,
            s.live_bytes / (1024 * 1024),
            s.pooled,
            s.pooled_bytes / (1024 * 1024),
            composites,
            offscreen,
            snapshots,
            self.pipelines.bind_group_count(),
            atlas,
            pf_hits,
            pf_errors,
            pf_bytes / (1024 * 1024),
            pf_pending,
            self.target_bytes() / (1024 * 1024),
        )
    }

    /// 当前缓存的采样 bind group 数(诊断/回归门禁)。
    pub fn bind_group_count(&self) -> usize {
        self.pipelines.bind_group_count()
    }

    /// 把所有**已物化**的层表面按名字过滤后逐个读回,交给调用方落盘。
    ///
    /// 用途:立绘分块合成出问题时,把每一层的实际像素单独导成 PNG,直接
    /// 肉眼定位「哪一块脏了」,不必再从加工链反推。
    ///
    /// `filter` 对 `LayerId::display()` 做子串匹配(空串 = 全部)。
    /// `emit(name, Surface)` 每层调用一次。低频路径:每次都全量读回 GPU
    /// 纹理,会阻塞队列,不适合逐帧调用。
    pub fn dump_layers(
        &mut self,
        filter: &str,
        mut emit: impl FnMut(&str, &Surface),
    ) -> Result<usize, Diag> {
        let items = self.surfaces.materialized_layers();
        let mut written = 0usize;
        for (id, tex) in items {
            let name = id.display();
            if !filter.is_empty() && !name.contains(filter) {
                continue;
            }
            let encoder = self
                .ctx
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("yu-backend-wgpu dump layer"),
                });
            let surface = self
                .readback
                .read_now(&self.ctx, &tex.texture, tex.width, tex.height, encoder)?;
            emit(&name, &surface);
            written += 1;
        }
        // 快照也导出:过渡的 `from` 常常直接是某个 `Snapshot(CopyId)` 基图,
        // 层本身没有物化 content。不导出快照就无法回答「from 到底是什么像素」。
        // 只导与 `filter` 相关层的快照会漏掉(层名里没有 CopyId),所以这里
        // 全量导出,名字带 `snap<N>` 前缀便于和层区分。
        let snaps: Vec<(u64, Rc<crate::surfaces::TexRef>)> = self
            .targets
            .snapshot_refs()
            .into_iter()
            .map(|(id, tex)| (id.0, tex))
            .collect();
        for (copy_id, tex) in snaps {
            let encoder = self
                .ctx
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("yu-backend-wgpu dump snapshot"),
                });
            let surface = self
                .readback
                .read_now(&self.ctx, &tex.texture, tex.width, tex.height, encoder)?;
            emit(&format!("snap{copy_id}"), &surface);
            written += 1;
        }
        Ok(written)
    }

    fn render_inner(
        &mut self,
        frame: &CompositorFrame,
        scale: Option<(Viewport, ScaleFilter)>,
        streaming: bool,
    ) -> Result<(WgpuRenderOutput, Option<(u64, u32, u32)>), Diag> {
        let timing = std::env::var("YU_RENDER_TIMING").is_ok();
        let started = std::time::Instant::now();
        self.textures.begin_frame();
        // CPU 预算刚回收了一批资产 ⇒ 同步释放它们的 GPU 纹理。
        // 两边不同步时 GPU 显存随「见过的资产种类」单调增长(手机上表现为
        // 越玩越卡、最终 OOM);详见 `TextureCache::release_evicted`。
        let evicted = self.textures.take_evicted_keys();
        if !evicted.is_empty() {
            let released = self
                .gpu_textures
                .release_evicted(&evicted, &mut self.pipelines);
            log::debug!(
                target: "yu-render",
                "[asset] gpu textures released={released} (cache evicted={})",
                evicted.len()
            );
        }
        self.frame_copy_bytes = 0;
        self.frame_copy_count = 0;
        let snapshots_before = self.targets.resource_counts().2;
        let uploads_before = self.gpu_textures.total_uploads;
        let bytes_before = self.gpu_textures.total_upload_bytes;
        let asset_before = self.gpu_textures.asset_upload_bytes;

        let (plan, report, mut phases) = plan::plan(
            &self.ctx,
            frame,
            &mut self.textures,
            &mut self.gpu_textures,
            &mut self.pipelines,
            &mut self.targets,
            &mut self.surfaces,
            &mut self.text_uploads,
            self.backbuffer.as_deref(),
            &mut self.backbuffer_gpu,
            &mut self.text,
            &mut self.text_gpu,
            self.force_bilinear_scale,
        )?;
        let t_plan = started.elapsed();

        let vertex_bytes = bytemuck::cast_slice::<Vertex, u8>(&plan.vertices);
        if !plan.vertices.is_empty() {
            self.upload_vertices(vertex_bytes)?;
        }
        let draw_calls = plan.draw_calls() as u64 + u64::from(scale.is_some());
        self.total_draw_calls += draw_calls;

        let mut encoder = self
            .ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("yu-backend-wgpu frame"),
            });
        if let Some(text_gpu) = &self.text_gpu {
            text_gpu.encode(&plan.text_jobs, &mut encoder);
        }
        self.encode_passes(&plan, &mut encoder)?;
        let t_encode = started.elapsed() - t_plan;

        // 帧末剪枝采样 bind group:只保留当前仍存活的纹理。
        //
        // `PipelineCache::ensure_bind_group` 按 `(texture_id, filter)` 缓存。
        // 图层表面在换差分/过渡时每帧换新纹理,旧 bind group 若不剪掉会
        // 永久钉住旧 TextureView ⇒ D3D12 显存单调增长,最终
        // `transition surface 1332x3351: Out of Memory`,立绘/表情退化成
        // 1×1。这里必须在 `encode_passes` **之后**:本帧规划期的临时表面
        // bind group 在编码时仍要被命令缓冲引用,提前清会画不出内容。
        {
            let mut live_tex = std::collections::BTreeSet::new();
            self.surfaces.live_texture_ids(&mut live_tex);
            self.targets.live_texture_ids(&mut live_tex);
            self.gpu_textures.live_texture_ids(&mut live_tex);
            self.text_uploads.live_texture_ids(&mut live_tex);
            self.pipelines.retain_textures(&live_tex);
        }

        // 屏幕读回:即使随后缩放也要保留缩放前的游戏画面作为 `:/W0/`
        // 后缓冲(引擎的 COPY 读内部画面,不读窗口缩放结果)。
        //
        // # 两条路径的语义**不同**,不能一视同仁
        //
        // - `streaming == false`:headless / 用例 golden / `--shot`。这里
        //   `last_surface` 必须**每帧都是当帧像素**(像素哈希与 golden 全靠
        //   它),所以无条件 `read_now`。**这条路径一个字节都不许动。**
        // - `streaming == true`:桌面/安卓实时呈现。宿主走零拷贝
        //   (`screen_texture`)上屏,`last_surface` 只是 `copy :/W0/` 的兜底
        //   来源,允许滞后 —— 而 `read()` 本来就是异步一帧滞后。
        //
        // 实时路径下**只在真有 CPU 消费者时才读**:`backbuffer` 的唯一严格
        // 消费者是主窗口截屏在**没有命令时刻快照**时的兜底分支
        // (`backbuffer_content`);有快照时 `screen_snapshot_content` 用 GPU
        // 侧的图层/绘制契约重合成,完全不碰 CPU 面。
        //
        // 真机实测(Android Vulkan 1100×825):无条件读回每帧 1~33ms
        // (`phases[readback=33159]us`),是「立绘/镜头运动卡顿」的主因之一。
        let screen_width = frame.viewport.width;
        let screen_height = frame.viewport.height;
        let needs_cpu_screen = !streaming || frame_needs_cpu_backbuffer(frame);
        // 归因(只读,`YURIS_READBACK_WHY` 开关):
        // 真机实测 `rb` p99=15.7ms / max=719ms 且每帧都非零,必须先确定
        // 「是走读回分支,还是仅仅在等 `queue.submit`」——两者的修法完全不同。
        if std::env::var_os("YURIS_READBACK_WHY").is_some() {
            let group_copies: Vec<String> = frame
                .passes
                .iter()
                .flat_map(|p| p.ops.iter())
                .filter_map(|op| match op {
                    yu_gfx::Op::Copy(c) => Some(c),
                    _ => None,
                })
                .filter(|c| matches!(c.source, yu_gfx::CopySource::Group(_)))
                .map(|c| {
                    format!(
                        "copy{}snap={}",
                        c.id.0,
                        frame.screen_snapshots.contains_key(&c.id)
                    )
                })
                .collect();
            let total_copy = frame
                .passes
                .iter()
                .flat_map(|p| p.ops.iter())
                .filter(|op| matches!(op, yu_gfx::Op::Copy(_)))
                .count();
            log::warn!(
                target: "yu-kernel",
                "[rbwhy] frame={} need={} streaming={} copies={} groupsnap=[{}] snapshots={}",
                frame.frame,
                needs_cpu_screen,
                streaming,
                total_copy,
                group_copies.join(","),
                frame.screen_snapshots.len()
            );
        }
        // **计时起点移到这里**:上面只做判断,不含 GPU 提交。旧实现把
        // `t_read0` 放在判断之前,`phases.readback_us` 实际量的是
        // 「提交 + 可能的读回」,读数被 `queue.submit` 污染(真机日志里
        // `rb` 每帧 374µs~15.7ms 无法区分是哪个)。
        let t_read0 = started.elapsed();
        let mut screen_surface = {
            if !needs_cpu_screen {
                // 实时路径且本帧无 `:/W0/` 兜底需求:不提交读回拷贝。
                // encoder 仍要提交(它带着本帧全部绘制);`last_surface`
                // 沿用上一份 —— 本帧没有消费者读它。
                self.ctx.queue.submit(Some(encoder.finish()));
                match self.backbuffer.as_deref() {
                    Some(prev) => prev.clone(),
                    None => Surface::new(screen_width, screen_height, [0, 0, 0, 255]),
                }
            } else {
                let screen = self.targets.screen().ok_or_else(|| {
                    Diag::fatal(device::code::TARGET, yu_log::Boundary::Platform)
                        .with_detail("帧结束时没有屏幕目标")
                })?;
                let t_inside = started.elapsed();
                let out = if streaming {
                    self.readback.read(
                        &self.ctx,
                        &screen.texture,
                        screen_width,
                        screen_height,
                        encoder,
                    )?
                } else {
                    self.readback.read_now(
                        &self.ctx,
                        &screen.texture,
                        screen_width,
                        screen_height,
                        encoder,
                    )?
                };
                // 细分:判断之后到进入 read 之前的耗时(应≈0)+ read 内部耗时。
                if std::env::var_os("YURIS_READBACK_WHY").is_some() {
                    log::warn!(
                        target: "yu-kernel",
                        "[rbtime] frame={} pre={}us read={}us total={}us",
                        frame.frame,
                        (t_inside - t_read0).as_micros(),
                        (started.elapsed() - t_inside).as_micros(),
                        (started.elapsed() - t_read0).as_micros()
                    );
                }
                out
            }
        };
        let t_readback = started.elapsed() - t_read0;
        phases.plan_us = t_plan.as_micros() as u64;
        phases.encode_us = t_encode.as_micros() as u64;
        phases.readback_us = t_readback.as_micros() as u64;
        phases.draw_calls = draw_calls;
        phases.copy_bytes = self.frame_copy_bytes;
        phases.copy_count = self.frame_copy_count;
        phases.snapshot_alloc = self
            .targets
            .resource_counts()
            .2
            .saturating_sub(snapshots_before) as u64;
        phases.uploads = self.gpu_textures.total_uploads - uploads_before;
        phases.upload_bytes = self.gpu_textures.total_upload_bytes - bytes_before;
        phases.upload_asset_bytes = self
            .gpu_textures
            .asset_upload_bytes
            .saturating_sub(asset_before);
        // 屏幕是 opaque 窗口(与 soft 的最终屏幕归一、浏览器 canvas 的
        // alphaMode=opaque 一致);组 RT alpha 不进入呈现读回。
        force_opaque(&mut screen_surface);
        // 后缓冲与返回值共享同一份像素(未缩放时):不再 clone 整帧。
        let screen_surface = Arc::new(screen_surface);

        let readback_bytes;
        let surface = match scale {
            Some((output, filter)) => {
                let mut encoder =
                    self.ctx
                        .device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("yu-backend-wgpu display scale"),
                        });
                self.encode_scale(filter, output, &mut encoder)?;
                let target = self.targets.scale().ok_or_else(|| {
                    Diag::fatal(device::code::TARGET, yu_log::Boundary::Platform)
                        .with_detail("缺少显示缩放目标")
                })?;
                // 缩放输出只服务对账/测试的精确像素:保持当帧同步读,
                // 且用独立的缓冲(与主读回同帧两次读会互踩)。
                let mut surface = self.scale_readback.read_now(
                    &self.ctx,
                    &target.texture,
                    output.width,
                    output.height,
                    encoder,
                )?;
                force_opaque(&mut surface);
                readback_bytes = screen_surface.rgba.len() as u64 + surface.rgba.len() as u64;
                Arc::new(surface)
            }
            None => {
                readback_bytes = screen_surface.rgba.len() as u64;
                Arc::clone(&screen_surface)
            }
        };
        let screen_handle = if streaming {
            self.targets
                .screen()
                .map(|target| (target.id, target.width, target.height))
        } else {
            None
        };
        if timing {
            let total = started.elapsed();
            log::trace!(
                target: "yu-render",
                "[rt] host render_total={:.2}ms plan={:.2} encode={:.2} readback={:.2} tail={:.2} uploads={} bytes={}",
                total.as_secs_f64() * 1e3,
                t_plan.as_secs_f64() * 1e3,
                t_encode.as_secs_f64() * 1e3,
                t_readback.as_secs_f64() * 1e3,
                (total - t_plan - t_encode - t_readback).as_secs_f64() * 1e3,
                self.gpu_textures.total_uploads - uploads_before,
                self.gpu_textures.total_upload_bytes - bytes_before,
            );
        }

        self.backbuffer = Some(screen_surface);
        phases.tail_us = (started.elapsed().as_micros() as u64)
            .saturating_sub(phases.plan_us + phases.encode_us + phases.readback_us);
        phases.readback_bytes = readback_bytes;
        self.last_phases = phases;
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
            layer_composites: self.surfaces.stats().surface_passes as usize,
            vertex_bytes: vertex_bytes.len() as u64,
            readback_bytes,
        };

        Ok((
            WgpuRenderOutput {
                surface,
                report,
                stats,
            },
            screen_handle,
        ))
    }

    fn upload_vertices(&mut self, bytes: &[u8]) -> Result<(), Diag> {
        let needed = bytes.len() as u64;
        let recreate = self
            .vertex_buffer
            .as_ref()
            .is_none_or(|slot| slot.capacity < needed);
        if recreate {
            let capacity = needed.next_power_of_two().max(64 * 1024);
            let buffer = self
                .ctx
                .checked_oom(&format!("vertex buffer {capacity}B"), || {
                    self.ctx.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("yu-backend-wgpu vertices"),
                        size: capacity,
                        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    })
                })?;
            self.vertex_buffer = Some(VertexBufferSlot { buffer, capacity });
        }
        let slot = self
            .vertex_buffer
            .as_ref()
            .expect("vertex buffer created above");
        self.ctx.queue.write_buffer(&slot.buffer, 0, bytes);
        Ok(())
    }

    fn encode_passes(
        &mut self,
        plan: &passes::FramePlan,
        encoder: &mut wgpu::CommandEncoder,
    ) -> Result<(), Diag> {
        let pipelines = &self.pipelines;
        let targets = &self.targets;
        let gpu = &self.gpu_textures;
        let vertex_buffer = self.vertex_buffer.as_ref().map(|slot| &slot.buffer);
        let resolve_view = |id: u64| -> Option<&wgpu::TextureView> {
            plan.views
                .get(&id)
                .or_else(|| gpu.get(id).map(|texture| &texture.view))
                .or_else(|| targets.view_by_id(id))
        };
        let resolve_texture = |id: u64| -> Option<&wgpu::Texture> {
            plan.textures
                .get(&id)
                .or_else(|| gpu.get(id).map(|texture| &texture.texture))
                .or_else(|| targets.texture_by_id(id))
        };
        for pass in &plan.passes {
            // 先清屏(池里复用的表面可能带旧内容;TRANSPARENT = 引擎空面)。
            for id in &pass.clear_targets {
                let Some(view) = resolve_view(*id) else {
                    return Err(
                        Diag::fatal(device::code::TARGET, yu_log::Boundary::Platform)
                            .with_detail(format!("清屏目标 {id} 缺失")),
                    );
                };
                let attachments = [Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })];
                let _ = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("yu-backend-wgpu clear"),
                    color_attachments: &attachments,
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
            }
            // 清成**指定实色**(`RECTPAINT{RectPaint,RectPaint3}` 的全图替换)。
            // 与画一个全屏 quad 逐像素等价,但走硬件 clear 路径 —— 立绘过渡链
            // 每帧要清 4 张 1332×3351,这是纯带宽浪费的主要来源。
            for (id, color) in &pass.clear_colors {
                let Some(view) = resolve_view(*id) else {
                    return Err(
                        Diag::fatal(device::code::TARGET, yu_log::Boundary::Platform)
                            .with_detail(format!("实色清屏目标 {id} 缺失")),
                    );
                };
                let attachments = [Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(color[0]),
                            g: f64::from(color[1]),
                            b: f64::from(color[2]),
                            a: f64::from(color[3]),
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })];
                let _ = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("yu-backend-wgpu clear color"),
                    color_attachments: &attachments,
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
            }
            let Some(planned_target) = pass.target.as_ref() else {
                continue;
            };
            if matches!(planned_target, PlannedTarget::Commands) {
                // 纹理区域复制(COPY 快照/RESIZE/TRIM/保留内容)。
                for blit in &pass.blits {
                    let source = resolve_texture(blit.source_id).ok_or_else(|| {
                        Diag::fatal(device::code::TARGET, yu_log::Boundary::Platform).with_detail(
                            format!("blit({}) 源纹理 {} 缺失", blit.label, blit.source_id),
                        )
                    })?;
                    let dest = resolve_texture(blit.dest_id).ok_or_else(|| {
                        Diag::fatal(device::code::TARGET, yu_log::Boundary::Platform).with_detail(
                            format!("blit({}) 目标纹理 {} 缺失", blit.label, blit.dest_id),
                        )
                    })?;
                    encoder.copy_texture_to_texture(
                        wgpu::TexelCopyTextureInfo {
                            texture: source,
                            mip_level: 0,
                            origin: wgpu::Origin3d {
                                x: blit.src_origin.0,
                                y: blit.src_origin.1,
                                z: 0,
                            },
                            aspect: wgpu::TextureAspect::All,
                        },
                        wgpu::TexelCopyTextureInfo {
                            texture: dest,
                            mip_level: 0,
                            origin: wgpu::Origin3d {
                                x: blit.dst_origin.0,
                                y: blit.dst_origin.1,
                                z: 0,
                            },
                            aspect: wgpu::TextureAspect::All,
                        },
                        wgpu::Extent3d {
                            width: blit.width,
                            height: blit.height,
                            depth_or_array_layers: 1,
                        },
                    );
                    self.frame_copy_bytes = self.frame_copy_bytes.saturating_add(
                        u64::from(blit.width)
                            .saturating_mul(u64::from(blit.height))
                            .saturating_mul(8),
                    );
                    self.frame_copy_count += 1;
                }
                // 表面上的 RECTPAINT 族:每条独立 render pass(Load 保留内容)。
                for paint in &pass.paints {
                    let Some(view) = resolve_view(paint.target_id) else {
                        return Err(
                            Diag::fatal(device::code::TARGET, yu_log::Boundary::Platform)
                                .with_detail(format!("填充目标 {} 缺失", paint.target_id)),
                        );
                    };
                    let pipeline = pipelines.paint_pipeline(paint.alpha_only).ok_or_else(|| {
                        Diag::fatal(device::code::TARGET, yu_log::Boundary::Platform)
                            .with_detail("填充管线缺失")
                    })?;
                    let bind = pipelines
                        .paint_bind_group(paint.alpha_only, paint.color)
                        .ok_or_else(|| {
                            Diag::fatal(device::code::TARGET, yu_log::Boundary::Platform)
                                .with_detail("填充 bind group 缺失")
                        })?;
                    let attachments = [Some(wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })];
                    let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("yu-backend-wgpu surface paint"),
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
            let target_view: &wgpu::TextureView = match planned_target {
                PlannedTarget::Screen => {
                    targets.screen().map(|target| &target.view).ok_or_else(|| {
                        Diag::fatal(device::code::TARGET, yu_log::Boundary::Platform)
                            .with_detail("缺少屏幕目标")
                    })?
                }
                PlannedTarget::Offscreen(id) => targets
                    .offscreen(id)
                    .map(|target| &target.view)
                    .ok_or_else(|| {
                        Diag::fatal(device::code::TARGET, yu_log::Boundary::Platform)
                            .with_detail(format!("缺少离屏目标 {}", id.display()))
                    })?,
                PlannedTarget::Surface(id) => resolve_view(*id).ok_or_else(|| {
                    Diag::fatal(device::code::TARGET, yu_log::Boundary::Platform)
                        .with_detail(format!("缺少层表面 {id}"))
                })?,
                PlannedTarget::Commands => unreachable!("handled above"),
            };
            let load = if pass.load {
                wgpu::LoadOp::Load
            } else {
                wgpu::LoadOp::Clear(pass.clear)
            };
            let attachments = [Some(wgpu::RenderPassColorAttachment {
                view: target_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load,
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
                let (pipeline, pipe_kind) = match &draw.kind {
                    passes::DrawKind::Quad => (pipelines.pipeline(draw.blend, false), 0u8),
                    passes::DrawKind::GroupQuad => (pipelines.pipeline(draw.blend, true), 4u8),
                    passes::DrawKind::TransitionPsc { .. } => {
                        (pipelines.psc_pipeline(draw.blend), 2)
                    }
                    passes::DrawKind::TransitionPsd { .. } => {
                        (pipelines.psd_pipeline(draw.blend), 3)
                    }
                    passes::DrawKind::Aux { pipeline, .. } => match pipeline {
                        passes::AuxPipeline::LayerMerge => (pipelines.layer_merge_pipeline(), 10),
                        passes::AuxPipeline::Composite => (pipelines.composite_pipeline(), 6),
                        passes::AuxPipeline::Lerp => (pipelines.lerp_pipeline(), 7),
                        passes::AuxPipeline::Transform => (pipelines.transform_pipeline(), 8),
                        passes::AuxPipeline::Resample => (pipelines.resample_pipeline(), 9),
                    },
                };
                let pipeline = pipeline.ok_or_else(|| {
                    Diag::fatal(device::code::TARGET, yu_log::Boundary::Platform).with_detail(
                        format!(
                            "缺少 blend {} 的{}管线",
                            draw.blend.name(),
                            if pipe_kind == 0 { "" } else { "表面写入" }
                        ),
                    )
                })?;
                let key = (draw.blend, pipe_kind);
                if current_pipeline != Some(key) {
                    render_pass.set_pipeline(pipeline);
                    current_pipeline = Some(key);
                }
                let bind = match &draw.kind {
                    passes::DrawKind::TransitionPsc { bind_index }
                    | passes::DrawKind::TransitionPsd { bind_index } => {
                        &plan.transition_bind_groups[*bind_index as usize]
                    }
                    passes::DrawKind::Aux { bind_index, .. } => {
                        &plan.aux_bind_groups[*bind_index as usize]
                    }
                    passes::DrawKind::Quad | passes::DrawKind::GroupQuad => pipelines
                        .bind_group(draw.texture_id, draw.filter)
                        .ok_or_else(|| {
                            Diag::fatal(device::code::TARGET, yu_log::Boundary::Platform)
                                .with_detail(format!(
                                    "缺少纹理 {} / {:?} 的 bind group",
                                    draw.texture_id, draw.filter
                                ))
                        })?,
                };
                let bind_key = match &draw.kind {
                    passes::DrawKind::TransitionPsc { bind_index }
                    | passes::DrawKind::TransitionPsd { bind_index } => {
                        (u64::MAX - u64::from(*bind_index), draw.filter)
                    }
                    passes::DrawKind::Aux { bind_index, .. } => {
                        (u64::MAX / 2 - u64::from(*bind_index), draw.filter)
                    }
                    passes::DrawKind::Quad | passes::DrawKind::GroupQuad => {
                        (draw.texture_id, draw.filter)
                    }
                };
                if current_bind != Some(bind_key) {
                    render_pass.set_bind_group(0, bind, &[]);
                    current_bind = Some(bind_key);
                }
                // 全屏三角形(合成/滤镜/重采样)不读顶点缓冲。
                if matches!(draw.kind, passes::DrawKind::Aux { .. }) {
                    render_pass.draw(0..3, 0..1);
                } else {
                    let first = pass.first_vertex + (index as u32) * 6;
                    render_pass.draw(first..first + 6, 0..1);
                }
            }
        }
        Ok(())
    }

    /// 引擎显示缩放 pass:全屏输出 `output` 尺寸,point 采样屏幕纹理。
    ///
    /// 管线/绑定按 `(源纹理 id, 变体, uniform)` 缓存;屏幕纹理重建时由
    /// `forget_texture` 作废旧绑定。blend 关闭(替换写),alpha 直通。
    fn encode_scale(
        &mut self,
        filter: ScaleFilter,
        output: Viewport,
        encoder: &mut wgpu::CommandEncoder,
    ) -> Result<(), Diag> {
        let screen = self.targets.screen().ok_or_else(|| {
            Diag::fatal(device::code::TARGET, yu_log::Boundary::Platform)
                .with_detail("显示缩放缺少屏幕源纹理")
        })?;
        let source_id = screen.id;
        let source_width = screen.width;
        let source_height = screen.height;
        let source_view = screen.view.clone();

        self.pipelines.ensure_scale_pipeline(&self.ctx, filter)?;
        // psB 的 SharpN 取 FXO 初值;psA 的 YS_TexDiffU 取 (2/W, 0)。
        let param = match filter {
            ScaleFilter::Sharp => [1.25, 1.25],
            ScaleFilter::Bicubic => [2.0 / source_width as f32, 0.0],
        };
        self.pipelines.ensure_scale_bind_group(
            &self.ctx,
            source_id,
            &source_view,
            filter,
            [source_width as f32, source_height as f32],
            param,
        )?;
        self.targets
            .ensure_scale(&self.ctx, output.width, output.height, &mut self.pipelines)?;

        let pipeline = self.pipelines.scale_pipeline(filter).ok_or_else(|| {
            Diag::fatal(device::code::TARGET, yu_log::Boundary::Platform)
                .with_detail("缺少显示缩放管线")
        })?;
        let bind = self
            .pipelines
            .scale_bind_group(
                source_id,
                filter,
                [source_width as f32, source_height as f32],
                param,
            )
            .ok_or_else(|| {
                Diag::fatal(device::code::TARGET, yu_log::Boundary::Platform)
                    .with_detail("缺少显示缩放 bind group")
            })?;
        let target = self.targets.scale().ok_or_else(|| {
            Diag::fatal(device::code::TARGET, yu_log::Boundary::Platform)
                .with_detail("缺少显示缩放目标")
        })?;

        let attachments = [Some(wgpu::RenderPassColorAttachment {
            view: &target.view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
        })];
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("yu-backend-wgpu display scale"),
            color_attachments: &attachments,
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        render_pass.set_pipeline(pipeline);
        render_pass.set_bind_group(0, bind, &[]);
        render_pass.draw(0..6, 0..1);
        Ok(())
    }
}

/// 实时路径下,本帧是否真的需要一份 **CPU** 后缓冲(整屏读回)。
///
/// 唯一的严格消费者是 `COPY ID=:/W0/` 的兜底分支:该复制**没有**命令时刻
/// 快照时,后端只能拿 CPU 后缓冲重新上传(`backbuffer_content`)。有快照时
/// `screen_snapshot_content` 直接用快照的图层/绘制契约在 GPU 上重合成,
/// 完全不碰 CPU 面。
///
/// 存档缩略图那条(`capture_bridge`)用的是「上一帧屏幕」,本身允许滞后。
///
/// **只用于 `streaming == true`**:headless 的像素哈希与 golden 依赖
/// 每帧精确的 `last_surface`,那条路径必须无条件读回。
fn frame_needs_cpu_backbuffer(frame: &yu_gfx::CompositorFrame) -> bool {
    for pass in &frame.passes {
        for op in &pass.ops {
            let yu_gfx::Op::Copy(copy) = op else {
                continue;
            };
            if !matches!(copy.source, yu_gfx::CopySource::Group(_)) {
                continue;
            }
            if !frame.screen_snapshots.contains_key(&copy.id) {
                return true;
            }
        }
    }
    false
}

fn force_opaque(surface: &mut Surface) {
    // 每 64 字节一次写 16 个 alpha。`chunks_exact_mut(4)` 的迭代器与
    // 边界检查在 debug 构建下实测 9ms/帧(1100×825),展开后 0.6ms;
    // release 两者都接近内存带宽,结果逐字节相同。
    let rgba = &mut surface.rgba;
    let mut chunks = rgba.chunks_exact_mut(64);
    for px in &mut chunks {
        px[3] = 255;
        px[7] = 255;
        px[11] = 255;
        px[15] = 255;
        px[19] = 255;
        px[23] = 255;
        px[27] = 255;
        px[31] = 255;
        px[35] = 255;
        px[39] = 255;
        px[43] = 255;
        px[47] = 255;
        px[51] = 255;
        px[55] = 255;
        px[59] = 255;
        px[63] = 255;
    }
    for px in chunks.into_remainder().chunks_exact_mut(4) {
        px[3] = 255;
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
