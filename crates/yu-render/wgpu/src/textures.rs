//! GPU 纹理与渲染目标缓存。
//!
//! 像素字节**只**来自 [`yu_render_core::TextureStore`]:同一套 vfs 探测/
//! 解码/缓存,保证软硬后端吃到的是同一份 RGBA8。本模块负责:
//!
//! - 按资产 key 上传一次,之后只复用 `wgpu::TextureView`/bind group;
//! - `SolidBlack1x1` 与资产分开缓存(它由引擎语义生成,不经过 vfs);
//! - 离屏/屏幕目标按 `LayerId`/视图尺寸复用,尺寸变化才重建。
//!
//! 所有纹理格式固定 [`wgpu::TextureFormat::Rgba8Unorm`],不做 sRGB 解码。

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

use bytemuck::Pod;
use yu_render_core::TextureStore;
use yu_gfx::{CopyId, LayerId};

use crate::surfaces::TexRef;
use yu_log::Diag;

use crate::device::{code, GpuContext};
use crate::pipelines::TARGET_FORMAT;

/// 纹理身份必须跨“资产缓存”和“渲染目标缓存”唯一,否则 bind group 缓存
/// 会用 `(id, filter)` 把两个不同的 view 混在一起。用进程级单调计数器
/// 统一分配;id 不进入任何报告/像素输出,因此不影响确定性。
static NEXT_TEXTURE_ID: AtomicU64 = AtomicU64::new(1);

pub(crate) fn allocate_texture_id() -> u64 {
    NEXT_TEXTURE_ID.fetch_add(1, Ordering::Relaxed)
}

/// 纹理用途:既能被采样,也能作为渲染目标/COPY 源/COPY 目标。
const TARGET_USAGE: wgpu::TextureUsages = wgpu::TextureUsages::RENDER_ATTACHMENT
    .union(wgpu::TextureUsages::TEXTURE_BINDING)
    .union(wgpu::TextureUsages::COPY_SRC)
    .union(wgpu::TextureUsages::COPY_DST);

/// 一张已上传的纹理 + 稳定身份。
pub struct GpuTexture {
    /// 本进程内单调递增;bind group 缓存键的一部分。
    pub id: u64,
    pub width: u32,
    pub height: u32,
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
}

/// 资产 key → GPU 纹理 + 1×1 黑图;按身份缓存,不逐帧重传。
pub struct TextureCache {
    textures: BTreeMap<u64, GpuTexture>,
    asset_ids: BTreeMap<Vec<u8>, u64>,
    solid_id: Option<u64>,
    /// 累计上传字节(解码后的 RGBA8 字节数,不含 wgpu 内部行填充)。
    pub total_upload_bytes: u64,
    /// 其中**资产首次解码**带来的字节(`ensure_asset`)。与覆盖/合成上传分开
    /// 记账:前者是加载新美术的一次性成本,后者才是每帧渲染预算。
    pub asset_upload_bytes: u64,
    /// 累计上传纹理数。
    pub total_uploads: u64,
}

impl TextureCache {
    pub fn new() -> Self {
        Self {
            textures: BTreeMap::new(),
            asset_ids: BTreeMap::new(),
            solid_id: None,
            total_upload_bytes: 0,
            asset_upload_bytes: 0,
            total_uploads: 0,
        }
    }

    pub fn cached_textures(&self) -> usize {
        self.textures.len()
    }

    pub fn get(&self, id: u64) -> Option<&GpuTexture> {
        self.textures.get(&id)
    }

    pub fn asset_id(&self, key: &[u8]) -> Option<u64> {
        self.asset_ids.get(key).copied()
    }

    /// 解析并上传一个资产;已上传时直接返回 id。
    ///
    /// `resolve` 的成功结果由 [`TextureStore`] 缓存,失败也缓存(不重复探测)。
    pub fn ensure_asset(
        &mut self,
        ctx: &GpuContext,
        store: &mut TextureStore,
        key: &[u8],
    ) -> Result<u64, Diag> {
        if let Some(id) = self.asset_ids.get(key) {
            return Ok(*id);
        }
        let texture = store.resolve(key)?;
        let uploaded = self.upload(
            ctx,
            texture.width,
            texture.height,
            &texture.rgba,
            "asset texture",
        )?;
        self.asset_ids.insert(key.to_vec(), uploaded);
        let texture = store.resolve(key)?;
        self.asset_upload_bytes = self
            .asset_upload_bytes
            .saturating_add(texture.rgba.len() as u64);
        Ok(uploaded)
    }

    /// `FILE=""` 的 1×1 不透明黑图;首次使用时上传。
    pub fn ensure_solid_black(&mut self, ctx: &GpuContext) -> Result<u64, Diag> {
        if let Some(id) = self.solid_id {
            return Ok(id);
        }
        let id = self.upload(ctx, 1, 1, &[0, 0, 0, 255], "solid black 1x1")?;
        self.solid_id = Some(id);
        Ok(id)
    }

    /// 上传一帧内生成的 RGBA8(图像加工结果)。调用方负责缓存生命周期。
    pub(crate) fn upload(
        &mut self,
        ctx: &GpuContext,
        width: u32,
        height: u32,
        rgba: &[u8],
        label: &str,
    ) -> Result<u64, Diag> {
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| {
                Diag::fatal(code::LIMIT, yu_log::Boundary::Platform)
                    .with_detail(format!("{label} 尺寸 {width}x{height} 溢出 usize"))
            })?;
        if rgba.len() != expected {
            return Err(
                Diag::fatal(code::TARGET, yu_log::Boundary::Platform).with_detail(format!(
                    "{label} 字节数 {} 与 {width}x{height}x4={expected} 不符",
                    rgba.len()
                )),
            );
        }
        ctx.check_dimensions(width, height, label)?;

        let descriptor = wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TARGET_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        };
        let texture = ctx.checked_oom(&format!("{label} {width}x{height}"), || {
            use wgpu::util::DeviceExt as _;
            ctx.device.create_texture_with_data(
                &ctx.queue,
                &descriptor,
                wgpu::util::TextureDataOrder::LayerMajor,
                rgba,
            )
        })?;
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let id = allocate_texture_id();
        self.textures.insert(
            id,
            GpuTexture {
                id,
                width,
                height,
                texture,
                view,
            },
        );
        self.total_upload_bytes += rgba.len() as u64;
        self.total_uploads += 1;
        Ok(id)
    }

}

impl Default for TextureCache {
    fn default() -> Self {
        Self::new()
    }
}

/// 一个可渲染的 RGBA8 目标(屏幕或组离屏)。
pub struct RenderTarget {
    pub id: u64,
    pub width: u32,
    pub height: u32,
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
}

/// 屏幕目标 + 按 `LayerId` 的离屏目标集合 + 按 `CopyId` 的快照集合
/// + 显示缩放输出目标。
pub struct TargetSet {
    screen: Option<RenderTarget>,
    offscreen: BTreeMap<LayerId, RenderTarget>,
    /// 图层级 GPU 合成结果(`CGACT[TRANSITION]`/`MERGE` 的离屏目标)。
    ///
    /// 键含**槽位**(0/1):`MERGE` 链要读上一槽、写下一槽(ping-pong),
    /// 同一帧内不能对同一纹理读写。与组离屏分开存:组目标按组状态重建,
    /// 这里按图层像素缓存复用。
    composites: BTreeMap<(LayerId, u8), RenderTarget>,
    /// 快照纹理:COPY 时刻冻结。**可以别名源内容纹理**(零拷贝;W3 第一态),
    /// 也可以是自己的一张区域拷贝。按 `CopyId` 跨帧复用。
    snapshots: BTreeMap<CopyId, std::rc::Rc<TexRef>>,
    scale: Option<RenderTarget>,
}

/// 合成目标硬上限(每个约等于一张整屏/精灵缓冲)。
const COMPOSITE_TARGET_CAP: usize = 32;
/// 单个 GPU 目标上限(字节)。4096² RGBA = 64MB;超过 128MB 的单个目标
/// 一定是畸形尺寸(COPY 区域异常),拒绝创建而不是让 wgpu 抛 OOM。
const MAX_TARGET_BYTES: usize = 128 * 1024 * 1024;
/// 全部目标(屏幕/离屏/合成/快照/缩放)的预算。GPU 还要放资产纹理与
/// 交换链;2GB 给目标,超出时拒绝新建(调用方回退 CPU 或跳过该 op),
/// 避免 wgpu 直接 `Out of Memory` 崩进程。
const MAX_TOTAL_TARGET_BYTES: usize = 2 * 1024 * 1024 * 1024;

impl TargetSet {
    pub fn new() -> Self {
        Self {
            screen: None,
            offscreen: BTreeMap::new(),
            composites: BTreeMap::new(),
            snapshots: BTreeMap::new(),
            scale: None,
        }
    }

    pub fn screen(&self) -> Option<&RenderTarget> {
        self.screen.as_ref()
    }

    /// 显示缩放输出目标(引擎 effect Pass2/Pass3 的渲染目标)。
    pub fn scale(&self) -> Option<&RenderTarget> {
        self.scale.as_ref()
    }

    pub fn offscreen(&self, id: &LayerId) -> Option<&RenderTarget> {
        self.offscreen.get(id)
    }

    pub fn snapshot(&self, id: CopyId) -> Option<&std::rc::Rc<TexRef>> {
        self.snapshots.get(&id)
    }

    /// 冻结一张快照(COPY 规划期)。内容纹理由 `LayerSurfaces` 的池/层内容
    /// 或本模块新建,这里只登记句柄。
    pub fn set_snapshot(&mut self, id: CopyId, tex: std::rc::Rc<TexRef>) {
        self.snapshots.insert(id, tex);
    }

    pub fn offscreen_count(&self) -> usize {
        self.offscreen.len()
    }

    pub fn snapshot_count(&self) -> usize {
        self.snapshots.len()
    }

    /// 按稳定的纹理 id 找 view(过渡/复制执行期用)。
    pub fn view_by_id(&self, id: u64) -> Option<&wgpu::TextureView> {
        if let Some(target) = &self.screen {
            if target.id == id {
                return Some(&target.view);
            }
        }
        for target in self
            .offscreen
            .values()
            .chain(self.composites.values())
        {
            if target.id == id {
                return Some(&target.view);
            }
        }
        for target in self.snapshots.values() {
            if target.id == id {
                return Some(&target.view);
            }
        }
        None
    }

    /// 按稳定的纹理 id 找纹理(COPY 源用)。
    pub fn texture_by_id(&self, id: u64) -> Option<&wgpu::Texture> {
        if let Some(target) = &self.screen {
            if target.id == id {
                return Some(&target.texture);
            }
        }
        for target in self
            .offscreen
            .values()
            .chain(self.composites.values())
        {
            if target.id == id {
                return Some(&target.texture);
            }
        }
        for target in self.snapshots.values() {
            if target.id == id {
                return Some(&target.texture);
            }
        }
        None
    }

    /// 只保留本帧仍被引用的快照(`CopyId` 由目标层或本帧复制引用)。
    pub fn prune_snapshots(
        &mut self,
        live: &std::collections::BTreeSet<CopyId>,
        pipelines: &mut crate::pipelines::PipelineCache,
    ) {
        let stale: Vec<CopyId> = self
            .snapshots
            .keys()
            .filter(|id| !live.contains(id))
            .copied()
            .collect();
        for id in stale {
            if let Some(tex) = self.snapshots.remove(&id) {
                if std::rc::Rc::strong_count(&tex) == 1 {
                    pipelines.forget_texture(tex.id);
                }
            }
        }
    }

    /// 丢弃全部快照纹理(渲染器 `reset()` 用;DIV-HOST-001)。
    ///
    /// 引擎重启 = 新设备 ⇒ 快照不再有效;`CopyId` 在新 `GfxState` 里从头
    /// 分配,旧纹理若保留会与同 id 的新快照冲突。
    pub fn clear_snapshots(&mut self, pipelines: &mut crate::pipelines::PipelineCache) -> usize {
        let removed = self.snapshots.len();
        for (_, target) in std::mem::take(&mut self.snapshots) {
            if std::rc::Rc::strong_count(&target) == 1 {
                pipelines.forget_texture(target.id);
            }
        }
        removed
    }

    /// 只保留本帧仍被引用的组离屏目标(与合成目标同理:整张纹理只增不删
    /// 会在长会话里吃光显存)。
    pub fn prune_offscreen(
        &mut self,
        live: &std::collections::BTreeSet<LayerId>,
        pipelines: &mut crate::pipelines::PipelineCache,
    ) {
        let stale: Vec<LayerId> = self
            .offscreen
            .keys()
            .filter(|id| !live.contains(id))
            .cloned()
            .collect();
        for id in stale {
            if let Some(target) = self.offscreen.remove(&id) {
                pipelines.forget_texture(target.id);
            }
        }
    }

    /// 丢弃全部离屏组目标(尺寸/内容都是上一轮 `GfxState` 的)。
    pub fn clear_offscreen(&mut self, pipelines: &mut crate::pipelines::PipelineCache) -> usize {
        let mut removed = self.offscreen.len();
        for (_, target) in std::mem::take(&mut self.offscreen) {
            pipelines.forget_texture(target.id);
        }
        for (_, target) in std::mem::take(&mut self.composites) {
            pipelines.forget_texture(target.id);
            removed += 1;
        }
        removed
    }

    /// 屏幕目标:尺寸不变则复用。重建时把旧 id 的 bind group 一并作废。
    pub fn ensure_screen(
        &mut self,
        ctx: &GpuContext,
        width: u32,
        height: u32,
        pipelines: &mut crate::pipelines::PipelineCache,
    ) -> Result<&RenderTarget, Diag> {
        let reusable = self
            .screen
            .as_ref()
            .is_some_and(|t| t.width == width && t.height == height);
        if !reusable {
            if let Some(old) = self.screen.take() {
                pipelines.forget_texture(old.id);
            }
            let target = create_target(ctx, width, height, "screen")?;
            self.screen = Some(target);
        }
        Ok(self.screen.as_ref().expect("screen target set above"))
    }

    /// 显示缩放输出目标:尺寸不变则复用(只有宿主请求缩放时才创建)。
    pub fn ensure_scale(
        &mut self,
        ctx: &GpuContext,
        width: u32,
        height: u32,
        pipelines: &mut crate::pipelines::PipelineCache,
    ) -> Result<&RenderTarget, Diag> {
        let reusable = self
            .scale
            .as_ref()
            .is_some_and(|t| t.width == width && t.height == height);
        if !reusable {
            if let Some(old) = self.scale.take() {
                pipelines.forget_texture(old.id);
            }
            let target = create_target(ctx, width, height, "display scale")?;
            self.scale = Some(target);
        }
        Ok(self.scale.as_ref().expect("scale target set above"))
    }

    pub fn ensure_offscreen(
        &mut self,
        ctx: &GpuContext,
        id: &LayerId,
        width: u32,
        height: u32,
        pipelines: &mut crate::pipelines::PipelineCache,
    ) -> Result<&RenderTarget, Diag> {
        let reusable = self
            .offscreen
            .get(id)
            .is_some_and(|t| t.width == width && t.height == height);
        if !reusable {
            if !self.budget_ok(width, height) {
                return Err(Diag::fatal(code::TARGET, yu_log::Boundary::Platform)
                    .with_detail("组离屏目标预算不足"));
            }
            if let Some(old) = self.offscreen.remove(id) {
                pipelines.forget_texture(old.id);
            }
            let target = create_target(ctx, width, height, "offscreen group")?;
            self.offscreen.insert(id.clone(), target);
        }
        Ok(self.offscreen.get(id).expect("offscreen target set above"))
    }

    /// 图层级 GPU 合成目标(`CGACT[TRANSITION]`/`MERGE` 的结果面;按
    /// `(图层, 槽位)` 复用)。`slot` 只允许 0/1。
    pub fn ensure_composite(
        &mut self,
        ctx: &GpuContext,
        id: &LayerId,
        slot: u8,
        width: u32,
        height: u32,
        pipelines: &mut crate::pipelines::PipelineCache,
    ) -> Result<&RenderTarget, Diag> {
        let key = (id.clone(), slot);
        let reusable = self
            .composites
            .get(&key)
            .is_some_and(|t| t.width == width && t.height == height);
        if !reusable {
            if !self.budget_ok(width, height) {
                return Err(Diag::fatal(code::TARGET, yu_log::Boundary::Platform)
                    .with_detail("图层合成目标预算不足"));
            }
            if let Some(old) = self.composites.remove(&key) {
                pipelines.forget_texture(old.id);
            }
            let target = create_target(ctx, width, height, "layer composite")?;
            self.composites.insert(key.clone(), target);
        }
        Ok(self
            .composites
            .get(&key)
            .expect("composite target set above"))
    }

    /// 当前图层级 GPU 合成目标数(诊断:证明 GPU 合成路径确实生效)。
    /// `(合成目标, 组离屏, 快照)` 计数(诊断/退出摘要)。
    pub fn resource_counts(&self) -> (usize, usize, usize) {
        (self.composites.len(), self.offscreen.len(), self.snapshots.len())
    }

    pub fn composite_count(&self) -> usize {
        self.composites.len()
    }

    /// 当前全部目标占用字节(诊断/预算;按 RGBA8 计)。
    pub fn allocated_bytes(&self) -> usize {
        let mut total = 0usize;
        for target in self
            .screen
            .iter()
            .chain(self.offscreen.values())
            .chain(self.composites.values())
            .chain(self.scale.iter())
        {
            total += (target.width as usize) * (target.height as usize) * 4;
        }
        for target in self.snapshots.values() {
            total += target.bytes();
        }
        total
    }

    /// 目标创建的预算检查:单张过大或总量超预算都拒绝(返回 `None` 让调用方
    /// 回退),避免 wgpu 的致命 OOM。
    fn budget_ok(&self, width: u32, height: u32) -> bool {
        let bytes = (width as usize) * (height as usize) * 4;
        bytes <= MAX_TARGET_BYTES && self.allocated_bytes() + bytes <= MAX_TOTAL_TARGET_BYTES
    }

    /// 只保留本帧仍存在的图层的合成目标,并设硬上限。
    ///
    /// 这些目标是**整张纹理**,叠加快照/离屏后显存压力很大:合成目标表
    /// 此前只增不删,切场景/差分累积数百张 ⇒ wgpu `Out of Memory`(实测
    /// frame 1679 崩溃)。合成目标只服务当前帧(当帧用 `ensure_composite`
    /// 会重建),所以按「图层本帧是否存活」剪枝 + 超限整表清空。
    pub fn prune_composites(
        &mut self,
        live: &std::collections::BTreeSet<LayerId>,
        pipelines: &mut crate::pipelines::PipelineCache,
    ) {
        let stale: Vec<(LayerId, u8)> = self
            .composites
            .keys()
            .filter(|(id, _)| !live.contains(id))
            .cloned()
            .collect();
        for key in stale {
            if let Some(target) = self.composites.remove(&key) {
                pipelines.forget_texture(target.id);
            }
        }
        if self.composites.len() > COMPOSITE_TARGET_CAP {
            for (_, target) in std::mem::take(&mut self.composites) {
                pipelines.forget_texture(target.id);
            }
        }
    }
}

impl Default for TargetSet {
    fn default() -> Self {
        Self::new()
    }
}

fn create_target(
    ctx: &GpuContext,
    width: u32,
    height: u32,
    what: &'static str,
) -> Result<RenderTarget, Diag> {
    ctx.check_dimensions(width, height, what)?;
    let descriptor = wgpu::TextureDescriptor {
        label: Some(what),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: TARGET_FORMAT,
        usage: TARGET_USAGE,
        view_formats: &[],
    };
    // 显存分配失败要能定位到具体目标:scope 抓 OOM 并转 Diag(不 panic)。
    let texture = ctx.checked_oom(&format!("{what} {width}x{height}"), || {
        ctx.device.create_texture(&descriptor)
    })?;
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let id = allocate_texture_id();
    Ok(RenderTarget {
        id,
        width,
        height,
        texture,
        view,
    })
}

/// 顶点:位置(NDC)、UV、层 alpha(已 clamp 到 0..1)。
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
    pub alpha: f32,
}

impl Vertex {
    pub const SIZE: u64 = std::mem::size_of::<Self>() as u64;
}
