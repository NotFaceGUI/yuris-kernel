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

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};

use bytemuck::Pod;
use yu_gfx::{CopyId, LayerId};
use yu_render_core::TextureStore;

use crate::surfaces::{Content, LayerSurfaces, TexRef};
use yu_log::Diag;

use crate::device::{code, GpuContext};
use crate::pipelines::{PipelineCache, TARGET_FORMAT};

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
    .union(wgpu::TextureUsages::COPY_DST)
    // GPU 文本 pass 在层表面就地写字形(compute storage read_write)。
    .union(wgpu::TextureUsages::STORAGE_BINDING);

/// 一张已上传的纹理 + 稳定身份。
pub struct GpuTexture {
    /// 本进程内单调递增;bind group 缓存键的一部分。
    pub id: u64,
    pub width: u32,
    pub height: u32,
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
}

/// 影片帧槽位数(每资产)。`帧号 % MOVIE_SLOTS` 决定槽;4 足以覆盖
/// 「同帧内多图层引用同一影片的相邻帧」,且与影片长度无关。
const MOVIE_SLOTS: usize = 4;

/// 影片帧的一个 GPU 槽位(`id == 0` = 未使用)。
#[derive(Clone, Copy, Default)]
struct MovieSlot {
    /// 当前槽里的帧号;`None` = 空槽。
    frame: Option<u32>,
    id: u64,
    width: u32,
    height: u32,
}

/// 资产 key → GPU 纹理 + 1×1 黑图;按身份缓存,不逐帧重传。
pub struct TextureCache {
    textures: BTreeMap<u64, GpuTexture>,
    /// 资产 key → (GPU 纹理 id, CPU 侧内容版本)。
    ///
    /// 版本来自 [`TextureStore::generation`](yu_render_core::TextureStore::generation)
    /// (`insert_cached` 推进,`touch` 不动)。影片帧每帧由宿主
    /// `insert_rgba` 覆盖 `:movie/current`,版本不变才复用 GPU 纹理;
    /// 版本变了就 `write_texture` 就地覆写,画面才会逐帧更新。
    ///
    /// 不能用 `Rc::as_ptr`:旧 `Rc` 释放后新分配可能复用同一地址,
    /// 会把「内容已换」误判成「未变」⇒ 影片静止。
    asset_ids: BTreeMap<Vec<u8>, (u64, u64)>,
    /// 资产纹理 id 集合(反向判定用;逐字符清理文本纹理时不能扫资产表)。
    asset_texture_ids: std::collections::BTreeSet<u64>,
    /// `YMV` 影片帧的 GPU 纹理槽(每资产固定 [`MOVIE_SLOTS`] 个,环形复用)。
    ///
    /// 影片帧数无上界(样例 306 帧 × 400×300),若按 `(资产, 帧)` 建纹理,
    /// bind group 缓存会持有旧 view ⇒ GPU 内存随播放时长线性增长。改为一帧
    /// 一个槽位、按 `帧号 % MOVIE_SLOTS` 复用同一张纹理(`write_texture`
    /// 就地覆写),槽数与帧数无关;同帧内相邻几帧并存也不会互相覆盖。
    movie_slots: BTreeMap<Vec<u8>, Vec<MovieSlot>>,
    solid_id: Option<u64>,
    /// 整层 `RECTPAINT*` 后的实色 1×1 纹理(按 RGBA 去重)。
    solid_fills: BTreeMap<[u8; 4], u64>,
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
            asset_texture_ids: std::collections::BTreeSet::new(),
            movie_slots: BTreeMap::new(),
            solid_id: None,
            solid_fills: BTreeMap::new(),
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
        self.asset_ids.get(key).map(|(id, _)| *id)
    }

    /// 解析并上传一个资产;已上传且内容未变时直接返回 id。
    ///
    /// `resolve_shared` 的成功结果由 [`TextureStore`] 缓存,失败也缓存
    /// (不重复探测)。宿主每帧用 `insert_rgba` 替换 `:movie/current` 的
    /// `Rc<Texture>`;这里比较 `Rc` 身份,变了就**就地覆写同一张 GPU 纹理**
    /// (`write_texture`),旧 view/bind group 继续有效——否则影片会静止在
    /// 第一帧。
    pub fn ensure_asset(
        &mut self,
        ctx: &GpuContext,
        store: &mut TextureStore,
        key: &[u8],
    ) -> Result<u64, Diag> {
        let texture = store.resolve_shared(key)?;
        let generation = store.generation(key).unwrap_or(0);
        let (width, height) = (texture.width, texture.height);
        if let Some((id, cached)) = self.asset_ids.get(key).copied() {
            if cached == generation {
                return Ok(id);
            }
            // CPU 侧内容已换:尺寸相同就覆写,否则摘除重建。
            let overwritten = self
                .textures
                .get(&id)
                .is_some_and(|entry| entry.width == width && entry.height == height);
            if overwritten {
                let entry = self.textures.get(&id).expect("checked above");
                let upload = wgpu::TexelCopyTextureInfo {
                    texture: &entry.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                };
                ctx.queue.write_texture(
                    upload,
                    &texture.rgba,
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
                self.total_upload_bytes = self
                    .total_upload_bytes
                    .saturating_add(texture.rgba.len() as u64);
                self.total_uploads = self.total_uploads.saturating_add(1);
                self.asset_ids.insert(key.to_vec(), (id, generation));
                return Ok(id);
            }
            // 尺寸变了(同一 key 的合成资产换了分辨率):旧纹理不再被
            // `asset_ids` 引用,从 pinned 集合摘除后它就是 transient,
            // `clear_transient` 会删除并 `forget_texture` 作废旧 view;
            // 本次直接上传新纹理,不在这里动 bind group 缓存。
            self.asset_texture_ids.remove(&id);
            self.asset_ids.remove(key);
        }
        let bytes = texture.rgba.len();
        let started = std::time::Instant::now();
        let uploaded = self.upload(ctx, width, height, &texture.rgba, "asset texture")?;
        let upload_us = started.elapsed().as_micros() as u64;
        self.asset_ids.insert(key.to_vec(), (uploaded, generation));
        self.asset_texture_ids.insert(uploaded);
        self.asset_upload_bytes = self.asset_upload_bytes.saturating_add(bytes as u64);
        if std::env::var_os("YURIS_ASSET_TIMING").is_some() {
            log::warn!(
                target: "yu-render",
                "[asset] upload key={} size={}x{} bytes={} upload={}us",
                String::from_utf8_lossy(key),
                width,
                height,
                bytes,
                upload_us
            );
        }
        Ok(uploaded)
    }

    /// `YMV` 影片帧:确保该 `(资产, 帧)` 的像素已在其槽位纹理里。
    ///
    /// 槽位 = `帧号 % MOVIE_SLOTS`;同槽换帧时用 `write_texture` 就地覆写
    /// (纹理 id / view / bind group 全部复用),因此播放任意长影片 GPU
    /// 纹理数恒定 ≤ `MOVIE_SLOTS × 影片数`。像素解码走
    /// [`TextureStore::resolve_movie`](yu_render_core::TextureStore::resolve_movie),
    /// 与软后端同源。
    pub fn ensure_movie(
        &mut self,
        ctx: &GpuContext,
        store: &mut TextureStore,
        asset: &[u8],
        frame: u32,
    ) -> Result<u64, Diag> {
        let index = (frame as usize) % MOVIE_SLOTS;
        if let Some(slot) = self.movie_slots.get(asset).and_then(|s| s.get(index)) {
            if slot.frame == Some(frame) {
                return Ok(slot.id);
            }
        }
        let pixels = store.resolve_movie(asset, frame)?;
        let (width, height, bytes) = (pixels.width, pixels.height, pixels.rgba.as_slice());
        let reusable = self
            .movie_slots
            .get(asset)
            .and_then(|s| s.get(index))
            .filter(|s| s.width == width && s.height == height)
            .map(|s| s.id);
        let id = match reusable {
            Some(id) => {
                let texture = self.textures.get(&id).expect("movie slot texture tracked");
                let upload = wgpu::TexelCopyTextureInfo {
                    texture: &texture.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                };
                ctx.queue.write_texture(
                    upload,
                    bytes,
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
                id
            }
            None => {
                let new_id = self.upload(ctx, width, height, bytes, "movie frame")?;
                self.total_upload_bytes =
                    self.total_upload_bytes.saturating_add(bytes.len() as u64);
                if let Some(old) = self
                    .movie_slots
                    .get(asset)
                    .and_then(|s| s.get(index))
                    .filter(|s| s.id != 0 && s.id != new_id)
                    .map(|s| s.id)
                {
                    // 尺寸变化:旧槽纹理不再被引用(同 id 不复用)。
                    self.textures.remove(&old);
                }
                new_id
            }
        };
        let slots = self.movie_slots.entry(asset.to_vec()).or_default();
        while slots.len() <= index {
            slots.push(MovieSlot::default());
        }
        slots[index] = MovieSlot {
            frame: Some(frame),
            id,
            width,
            height,
        };
        Ok(id)
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

    /// 1×1 实色纹理(`CG[FILE=""]` + 整层 `RECTPAINT*`;`Layer.fill`)。
    pub fn ensure_solid_color(
        &mut self,
        ctx: &GpuContext,
        rgba: [u8; 4],
    ) -> Result<u64, Diag> {
        if let Some(id) = self.solid_fills.get(&rgba) {
            return Ok(*id);
        }
        let id = self.upload(ctx, 1, 1, &rgba, "solid color 1x1")?;
        self.solid_fills.insert(rgba, id);
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

    /// 该纹理是否是长期缓存(资产或 1×1 黑图),不可被临时上传回收。
    fn is_pinned(&self, id: u64) -> bool {
        self.solid_id == Some(id) || self.asset_texture_ids.contains(&id)
    }

    /// 删除一张临时上传并作废引用它的 bind group。
    ///
    /// 纹理像素可能仍被 `LayerSurfaces` 的 `Rc<TexRef>` 持有(跨帧内容),
    /// 这里只从缓存表摘除并释放 bind group,不会提前销毁正在使用的纹理。
    pub fn remove(&mut self, id: u64, pipelines: &mut PipelineCache) {
        if self.is_pinned(id) {
            return;
        }
        if self.textures.remove(&id).is_some() {
            pipelines.forget_texture(id);
        }
    }

    /// 重启/回收:清掉所有非资产上传(文本覆盖、回缓冲、复制文字等)。
    ///
    /// 资产纹理与 1×1 黑图保留:它们只依赖游戏数据,重启后仍可复用。
    pub fn clear_transient(&mut self, pipelines: &mut PipelineCache) {
        let stale: Vec<u64> = self
            .textures
            .keys()
            .filter(|id| !self.is_pinned(**id))
            .copied()
            .collect();
        for id in stale {
            if self.textures.remove(&id).is_some() {
                pipelines.forget_texture(id);
            }
        }
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
    /// 区域截图的**零拷贝采样窗**(`TextureRef::Snapshot` 的 UV 别名)。
    ///
    /// ERIS 菜单构造一次做 260+ 个按钮状态 `COPY2` 切片;每个都新建一张
    /// 小纹理(实测 ~86µs/张,构造帧 `image_us≈25ms`)。源内容不可变,切片
    /// 可直接引用源纹理的 UV 子窗口,由图层绘制/文本烘焙按采样窗消费。
    /// 与 `snapshots` 同生命周期(按 `CopyId` 剪枝)。
    snapshot_contents: BTreeMap<CopyId, Content>,
    /// 屏幕快照合成专用的图层表面账。
    ///
    /// `screen_snapshot_content` 用**捕获时**的 alpha/texture 合成冻结画面;
    /// 若写进活的 `LayerSurfaces`,`sync_base` 会把同名活层在两套 base
    /// token 之间来回清空(读档时背景层被反复重建,烘焙跳过永远补不回来)。
    /// 每个 `CopyId` 一份,保留跨帧缓存但绝不污染活层。
    snapshot_surfaces: BTreeMap<CopyId, LayerSurfaces>,
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
            snapshot_contents: BTreeMap::new(),
            snapshot_surfaces: BTreeMap::new(),
            scale: None,
        }
    }

    /// 取/还屏幕快照合成专用的表面账(见字段注释)。
    pub(crate) fn take_snapshot_surfaces(&mut self, id: CopyId) -> LayerSurfaces {
        self.snapshot_surfaces.remove(&id).unwrap_or_default()
    }

    pub(crate) fn put_snapshot_surfaces(&mut self, id: CopyId, surfaces: LayerSurfaces) {
        self.snapshot_surfaces.insert(id, surfaces);
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

    /// 零拷贝区域截图的采样窗(没有物化纹理时存在)。
    pub fn snapshot_content(&self, id: CopyId) -> Option<&Content> {
        self.snapshot_contents.get(&id)
    }

    /// 快照是否已冻结(物化纹理或 UV 别名都算)。
    pub fn has_snapshot(&self, id: CopyId) -> bool {
        self.snapshots.contains_key(&id) || self.snapshot_contents.contains_key(&id)
    }

    /// 已冻结的快照 id 集合(供 `live_snapshots` 停止展开其内部复制链)。
    pub(crate) fn materialized_ids(&self) -> BTreeSet<CopyId> {
        self.snapshots
            .keys()
            .chain(self.snapshot_contents.keys())
            .copied()
            .collect()
    }

    /// 当前存活快照的句柄表(按需读回端口跨帧使用;只克隆 `Rc`)。
    pub(crate) fn snapshot_refs(&self) -> BTreeMap<CopyId, std::rc::Rc<TexRef>> {
        self.snapshots.clone()
    }

    /// 冻结一张快照(COPY 规划期)。内容纹理由 `LayerSurfaces` 的池/层内容
    /// 或本模块新建,这里只登记句柄。
    pub fn set_snapshot(&mut self, id: CopyId, tex: std::rc::Rc<TexRef>) {
        self.snapshots.insert(id, tex);
        self.snapshot_contents.remove(&id);
    }

    /// 冻结一张零拷贝区域截图(源纹理 + UV 窗口)。
    pub fn set_snapshot_content(&mut self, id: CopyId, content: Content) {
        self.snapshot_contents.insert(id, content);
        self.snapshots.remove(&id);
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
        for target in self.offscreen.values().chain(self.composites.values()) {
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
        for target in self.offscreen.values().chain(self.composites.values()) {
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
            .chain(self.snapshot_contents.keys())
            .filter(|id| !live.contains(id))
            .copied()
            .collect();
        for id in stale {
            if let Some(tex) = self.snapshots.remove(&id) {
                // bind group 可能仍持有 view;即使 `Rc` 还被层内容引用,
                // 这里的快照登记已经失效,必须作废旧 group(需要时从
                // `TexRef` 的 view 按需重建)。
                pipelines.forget_texture(tex.id);
            }
            self.snapshot_contents.remove(&id);
        }
        self.snapshot_surfaces.retain(|id, _| live.contains(id));
    }

    /// 丢弃全部快照纹理(渲染器 `reset()` 用;DIV-HOST-001)。
    ///
    /// 引擎重启 = 新设备 ⇒ 快照不再有效;`CopyId` 在新 `GfxState` 里从头
    /// 分配,旧纹理若保留会与同 id 的新快照冲突。
    pub fn clear_snapshots(&mut self, pipelines: &mut crate::pipelines::PipelineCache) -> usize {
        let removed = self.snapshots.len() + self.snapshot_contents.len();
        for (_, target) in std::mem::take(&mut self.snapshots) {
            pipelines.forget_texture(target.id);
        }
        self.snapshot_contents.clear();
        self.snapshot_surfaces.clear();
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
        (
            self.composites.len(),
            self.offscreen.len(),
            self.snapshots.len(),
        )
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
