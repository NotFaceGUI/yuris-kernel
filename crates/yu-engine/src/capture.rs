//! `SAVE[ID]` / `LOAD[ID]` 的图层像素通道(Engine 装配)。
//!
//! - **读取**(`SAVE[ID]`):官方在命令处理器里直接读图层表面(读取端
//!   `0x4477c0` 首 u32 = `0x640`)。本内核的层表面常驻在渲染器里(soft 是
//!   `Surface`,wgpu 是 GPU 纹理),抓帧器只保留**按需**取得的快照像素:
//!   - wgpu:`SAVE[ID]` 命令时刻通过 `SnapshotSource` 读上一帧已冻结快照,
//!     没有 SAVE 不读;
//!   - soft:快照本来就是 CPU 表面,渲染后镜像新快照(旧语义,无 GPU 等待)。
//!   `observe` 只补“本帧 SAVE 已登记、但当帧尚未渲染”的 `wanted`。
//! - **写入**(`LOAD[ID]`):把图像编码成 PNG,写入工程的合成资产叠层
//!   ([`yu_vfs::SharedMem`],内容哈希命名),再把图层纹理指到该资产并注入
//!   尺寸。渲染器走常规资产管线,无需任何后端改动。

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::sync::Arc;

use yu_backend_soft::{png, scale_region, SoftRenderer, Surface, Texture, TextureStore};
use yu_gfx::{CompositorFrame, CopyId, CopySource, GfxState, Layer, LayerCapture, Op, SizeU32, TextureRef};
use yu_log::Diag;
use yu_rt::event::CapturedImage;
use yu_vfs::SharedMem;

/// 抓帧器:快照像素缓存 + 运行时合成资产叠层。
pub struct GfxCapture {
    /// 资产解码 + 按需回放(官方 `SAVE[ID]` 在命令时刻读层表面;同 tick
    /// `COPY2→SAVE` 只能靠回放 `state.frame()` 取快照)。
    soft: SoftRenderer,
    /// 上一帧呈现面(`COPY ID=:/W0/` 的后缓冲,回放用)。
    last_screen: Option<Arc<Surface>>,
    /// 合成资产叠层(`LOAD[ID]` 写入点;渲染器与 VM 共享)。
    overlay: SharedMem,
    /// eager(soft)路径已镜像过的快照(渲染后去重)。
    seen: BTreeSet<CopyId>,
    /// 快照像素(**按需**读回/软后端直接借用;不是每帧镜像)。
    pixels: BTreeMap<CopyId, Rc<Texture>>,
    /// 抓帧时仍缺像素的快照(下一帧渲染后由宿主补齐)。
    wanted: BTreeSet<CopyId>,
    /// `SAVE[ID]` 命令时刻的按需快照读回端口(wgpu;soft 走 CPU 镜像)。
    source: Option<Box<dyn yu_render_core::SnapshotSource>>,
    /// 是否在渲染后主动镜像所有新快照(soft:CPU 表面,便宜且保持旧语义)。
    eager: bool,
    /// 抓帧缺像素次数(显式计数,不静默)。
    pub missing_pixels: u64,
    /// 命令时刻软回放次数(诊断:同 tick `COPY2→SAVE` 的兜底路径)。
    pub snapshot_replays: u64,
}

impl GfxCapture {
    pub fn new(textures: TextureStore, overlay: SharedMem) -> Self {
        Self {
            soft: SoftRenderer::new(textures),
            last_screen: None,
            overlay,
            seen: BTreeSet::new(),
            pixels: BTreeMap::new(),
            wanted: BTreeSet::new(),
            source: None,
            // 默认保持旧语义:未注入按需端口时仍镜像新快照,避免独立
            // 使用 `GfxCapture` 时悄悄丢 SAVE 像素。wgpu 引擎显式关掉。
            eager: true,
            missing_pixels: 0,
            snapshot_replays: 0,
        }
    }

    /// 在 `frame` 渲染后调用:返回需要宿主读回像素的 `CopyId`。
    ///
    /// - wgpu:只返回本帧 `SAVE[ID]` 已登记、但当帧尚未渲染的 `wanted`;
    ///   上一帧快照由 `SnapshotSource` 在 SAVE 命令时刻按需同步读回。
    /// - soft:eager 镜像所有新快照(CPU 表面拷贝,不是 GPU 同步等待),
    ///   保证下一帧 SAVE 也能拿到像素。
    pub fn observe(&mut self, frame: &CompositorFrame) -> Vec<CopyId> {
        // 即使没有 wanted 也要清理死快照,避免 pixels/seen 长期滞留。
        self.retain_live(frame);
        if !self.eager {
            // wgpu:按需端口在 SAVE 命令时刻读上一帧快照;渲染后只补
            // “本帧 SAVE 已登记、但当帧尚未渲染”的 wanted 快照。
            self.wanted
                .retain(|id| !self.pixels.contains_key(id));
            return self.wanted.iter().copied().collect();
        }
        // soft:快照是 CPU 表面,渲染后镜像全部新快照(旧语义;成本是
        // 内存拷贝,不是 GPU 同步读回)。这保证“下一帧 SAVE”仍拿得到像素。
        let mut fresh = Vec::new();
        for pass in &frame.passes {
            for op in &pass.ops {
                let Op::Copy(copy) = op else { continue };
                if copy.status != yu_gfx::OpStatus::Executable {
                    continue;
                }
                if self.seen.insert(copy.id) {
                    fresh.push(copy.id);
                }
            }
        }
        if fresh.is_empty() && self.wanted.is_empty() {
            return Vec::new();
        }
        self.pixels.retain(|id, _| self.seen.contains(id));
        self.wanted
            .retain(|id| !self.pixels.contains_key(id));
        for id in &self.wanted {
            if !fresh.contains(id) {
                fresh.push(*id);
            }
        }
        fresh
    }

    fn retain_live(&mut self, frame: &CompositorFrame) {
        let live: BTreeSet<CopyId> = frame
            .passes
            .iter()
            .flat_map(|pass| pass.ops.iter())
            .filter_map(|op| match op {
                Op::Copy(copy) => Some(copy.id),
                _ => None,
            })
            .chain(frame.layers.iter().filter_map(|layer| match &layer.texture {
                TextureRef::Snapshot(id) => Some(*id),
                _ => None,
            }))
            .collect();
        self.seen.retain(|id| live.contains(id));
        self.pixels.retain(|id, _| live.contains(id));
        self.wanted.retain(|id| live.contains(id));
    }

    /// 写入一张从渲染器读回的快照像素(宿主在 `observe` 之后调用)。
    pub fn insert_mirror(&mut self, id: CopyId, width: u32, height: u32, rgba: Vec<u8>) {
        self.wanted.remove(&id);
        self.pixels
            .insert(id, Rc::new(Texture { width, height, rgba }));
    }
}

impl GfxCapture {
    pub fn set_source(&mut self, source: Box<dyn yu_render_core::SnapshotSource>) {
        self.source = Some(source);
    }

    pub fn set_eager(&mut self, eager: bool) {
        self.eager = eager;
    }

    /// 记录上一帧呈现面(`COPY ID=:/W0/` 的回放输入;引擎每帧渲染后注入)。
    pub fn set_last_screen(&mut self, screen: Option<Arc<Surface>>) {
        self.last_screen = screen;
    }

    /// 取快照像素:先查镜像;wgpu 端口存在时按需读上一帧快照并缓存。
    fn snapshot_pixels(&mut self, id: CopyId) -> Option<Rc<Texture>> {
        if let Some(mirror) = self.pixels.get(&id) {
            return Some(Rc::clone(mirror));
        }
        let surface = self.source.as_mut()?.read(id)?;
        let mirror = Rc::new(Texture {
            width: surface.width,
            height: surface.height,
            rgba: surface.rgba,
        });
        self.pixels.insert(id, Rc::clone(&mirror));
        Some(mirror)
    }

    /// 从已有的源快照/资产按 `COPY` 区域裁剪出目标快照,避免整帧软回放。
    ///
    /// 存档链 `SLTEMP2 = COPY2(SLTEMP)` 的源快照在打开存档界面时已冻结,
    /// 这里只做一次裁剪;组/后缓冲源(`source_texture == None`)或带文本的
    /// 拷贝返回 `None`,交给 [`Self::replay_snapshot`] 兜底。
    fn chain_snapshot(&mut self, id: CopyId, state: &GfxState) -> Option<Rc<Texture>> {
        const MAX_DEPTH: usize = 8;
        let mut chain: Vec<&yu_gfx::CopyOp> = Vec::new();
        let mut current = id;
        let mut base: Option<Rc<Texture>> = None;
        for _ in 0..MAX_DEPTH {
            let copy = state
                .copies()
                .iter()
                .rev()
                .find(|copy| copy.id == current && copy.status == yu_gfx::OpStatus::Executable)?;
            // 文本是渲染期覆盖层,裁剪不会带上;交给整帧回放保证内容完整。
            if !copy.texts.is_empty() {
                return None;
            }
            let source = copy.source_texture.as_ref()?;
            chain.push(copy);
            match source {
                TextureRef::Snapshot(prev) => {
                    // 源快照已物化(上一帧注册表/软镜像)⇒ 直接作为基面。
                    if let Some(pixels) = self.snapshot_pixels(*prev) {
                        base = Some(pixels);
                        break;
                    }
                    current = *prev;
                }
                TextureRef::Asset(key) => {
                    base = Some(self.soft.textures_mut().resolve_shared(key).ok()?);
                    break;
                }
                TextureRef::SolidBlack1x1 => {
                    base = Some(Rc::new(Texture::solid_black_1x1()));
                    break;
                }
                TextureRef::None | TextureRef::Offscreen(_) => return None,
            }
        }
        let mut base = base?;
        for copy in chain.into_iter().rev() {
            let (x0, y0, x1, y1) = copy.region.resolve(base.width, base.height);
            let cx0 = x0.max(0);
            let cy0 = y0.max(0);
            let cx1 = x1.min(i64::from(base.width) - 1);
            let cy1 = y1.min(i64::from(base.height) - 1);
            let width = (cx1 - cx0 + 1).max(0) as u32;
            let height = (cy1 - cy0 + 1).max(0) as u32;
            if width == 0 || height == 0 || width > base.width || height > base.height {
                return None;
            }
            base = Rc::new(crop_texture(&base, cx0 as u32, cy0 as u32, width, height));
        }
        self.pixels.insert(id, Rc::clone(&base));
        Some(base)
    }

    /// 命令时刻按 `state.frame()` 软回放一次,取尚未渲染的快照。
    ///
    /// 官方 `COPY2` 在命令处理器里立即写层表面,因此同 tick 的
    /// `COPY2→SAVE[ID]` 能读到像素(`es_saveload` pc313/pc319);
    /// 主渲染器的快照要到帧末才存在,这里用与主后端同口径的 soft
    /// 参考实现回放到命令时刻,低频(仅缺镜像的 SAVE)。
    fn replay_snapshot(&mut self, id: CopyId, state: &GfxState) -> Option<Rc<Texture>> {
        let frame = state.frame();
        self.soft.set_last_screen(self.last_screen.clone());
        let started = std::time::Instant::now();
        self.soft.render_frame(&frame);
        let Some(surface) = self.soft.snapshot_surface(id) else {
            log::warn!(
                target: "yu-engine",
                "[caption] soft replay did not materialize snapshot {}",
                id.0
            );
            return None;
        };
        let mirror = Rc::new(Texture {
            width: surface.width,
            height: surface.height,
            rgba: surface.rgba.clone(),
        });
        self.pixels.insert(id, Rc::clone(&mirror));
        self.snapshot_replays += 1;
        if std::env::var("YURIS_CAPTION_TIMING").is_ok() {
            log::debug!(
                target: "yu-engine",
                "[caption] SAVE[ID] replay snapshot={} {}x{} {}us",
                id.0,
                surface.width,
                surface.height,
                started.elapsed().as_micros()
            );
        }
        Some(mirror)
    }
}

impl LayerCapture for GfxCapture {
    fn capture(&mut self, state: &GfxState, id: &[u8]) -> Result<Option<CapturedImage>, Diag> {
        // 抓帧器自己的资产缓存也要走帧边界预算:连续 SAVE 资产层而不触发
        // 软回放时,`begin_frame` 不会由回放调用,预算必须在这里执行。
        self.soft.textures_mut().begin_frame();
        let parsed = yu_gfx::path::normalize(id, None, false);
        let Some(layer) = state.layer(&parsed.path) else {
            return Ok(None);
        };
        let texture = layer.texture.clone();
        // 快照/资产都是「图层表面」的来源;其余纹理形态(离屏组/空层)没有
        // 可直接序列化的像素面 ⇒ 交给 VM 报 Unimplemented。
        if matches!(texture, TextureRef::None | TextureRef::Offscreen(_)) {
            return Ok(None);
        }
        // 表面**尺寸**:图层自己经 `CGACT[SIZE]` 定过尺寸就用它;否则沿复制
        // 链找上游层的尺寸(`es_SaveLoad` 的缩略图链:`SLTEMP` 被
        // `CGACT[SIZE2]` 定成 100×75 → `SLTEMP2` 只是 `COPY2` 的落点)。
        let size = intended_size(state, layer);
        let surface = match &texture {
            TextureRef::Snapshot(copy_id) => {
                // 快路径:上一帧已渲染的镜像/注册表;同 tick 新建的快照
                // 优先用源快照裁剪,最后才整帧回放。
                let mirror = self
                    .snapshot_pixels(*copy_id)
                    .or_else(|| self.chain_snapshot(*copy_id, state))
                    .or_else(|| self.replay_snapshot(*copy_id, state));
                if mirror.is_none() {
                    self.wanted.insert(*copy_id);
                    self.missing_pixels += 1;
                    log::warn!(
                        target: "yu-engine",
                        "[caption] snapshot {} pixels not materialized (readback + replay failed); deferred",
                        copy_id.0
                    );
                }
                mirror.map(|mirror| Surface {
                    width: mirror.width,
                    height: mirror.height,
                    rgba: mirror.rgba.clone(),
                })
            }
            TextureRef::Asset(key) => self
                .soft
                .textures_mut()
                .resolve(key)
                .ok()
                .map(|texture| Surface {
                    width: texture.width,
                    height: texture.height,
                    rgba: texture.rgba.clone(),
                }),
            TextureRef::SolidBlack1x1 => {
                // `CG[FILE=""]` 的占位面:如果 SAVE 落到它,说明该层没有拿到
                // 真正的拷贝(常见于 COPY2 源缺失)。显式留痕,不静默存 1×1。
                log::warn!(
                    target: "yu-engine",
                    "[caption] SAVE[ID] target {} is the 1x1 black placeholder (COPY missing?)",
                    parsed.path.display()
                );
                Some(Surface::new(1, 1, [0, 0, 0, 255]))
            }
            TextureRef::None | TextureRef::Offscreen(_) => None,
        };
        let surface = surface.map(|surface| match size {
            Some((width, height))
                if width > 1
                    && height > 1
                    && (width != surface.width || height != surface.height) =>
            {
                // 官方 `CGACT[SIZE2]` 是真实 StretchRect(表面尺寸随之改变);
                // 本内核的渲染器把缩放留到绘制时,这里在抓帧点补一次双线性
                // 重采样,得到与官方同尺寸的表面。
                let src = Texture {
                    width: surface.width,
                    height: surface.height,
                    rgba: surface.rgba,
                };
                let region = (
                    0,
                    0,
                    i64::from(src.width) - 1,
                    i64::from(src.height) - 1,
                );
                let scaled = scale_region(&src, width, height, Some(region), true);
                Surface {
                    width: scaled.width,
                    height: scaled.height,
                    rgba: scaled.rgba,
                }
            }
            _ => surface,
        });
        Ok(surface.map(|surface| CapturedImage {
            width: surface.width,
            height: surface.height,
            rgba: surface.rgba,
        }))
    }

    fn restore(
        &mut self,
        state: &mut GfxState,
        id: &[u8],
        image: &CapturedImage,
    ) -> Result<bool, Diag> {
        if image.rgba.len() != image.len_bytes() || image.len_bytes() == 0 {
            return Ok(false);
        }
        let surface = Surface {
            width: image.width,
            height: image.height,
            rgba: image.rgba.clone(),
        };
        let encode_started = std::time::Instant::now();
        let png = png::png_bytes(&surface).map_err(|e| {
            yu_gfx::error::recoverable(
                yu_gfx::error::code::INFO_UNSUPPORTED,
                format!("LOAD[ID] 缩略图 PNG 编码失败: {e}"),
            )
        })?;
        let encode_us = encode_started.elapsed().as_micros() as u64;
        if std::env::var("YURIS_CAPTION_TIMING").is_ok() {
            // 缩略图编码是命令时刻的一次性成本:留在统一控制台(debug),
            // 回答「存档/读档这一帧有没有编码尖峰」。
            log::debug!(
                target: "yu-engine",
                "[caption] LOAD[ID] PNG {}x{} {}B encode={}us",
                image.width,
                image.height,
                png.len(),
                encode_us
            );
        }
        let key = synthetic_key(&png);
        self.overlay.insert(&key, png).map_err(|d| {
            yu_gfx::error::recoverable(
                yu_gfx::error::code::INFO_UNSUPPORTED,
                format!("LOAD[ID] 缩略图写入合成资产叠层失败: {d}"),
            )
        })?;
        let parsed = yu_gfx::path::normalize(id, None, false);
        Ok(state.set_layer_asset(
            &parsed.path,
            key,
            SizeU32 {
                width: image.width,
                height: image.height,
            },
        ))
    }
}

/// 离屏抓帧句柄(`Rc` 共享给 `GfxHost` 与 `Engine` 的逐帧 `observe`)。
#[derive(Clone)]
pub struct SharedCapture(pub Rc<std::cell::RefCell<GfxCapture>>);

impl SharedCapture {
    pub fn set_source(&mut self, source: Box<dyn yu_render_core::SnapshotSource>) {
        self.0.borrow_mut().set_source(source);
    }

    pub fn set_eager(&mut self, eager: bool) {
        self.0.borrow_mut().set_eager(eager);
    }

    pub fn set_last_screen(&self, screen: Option<Arc<Surface>>) {
        self.0.borrow_mut().set_last_screen(screen);
    }
}

impl LayerCapture for SharedCapture {
    fn capture(&mut self, state: &GfxState, id: &[u8]) -> Result<Option<CapturedImage>, Diag> {
        self.0.borrow_mut().capture(state, id)
    }

    fn restore(
        &mut self,
        state: &mut GfxState,
        id: &[u8],
        image: &CapturedImage,
    ) -> Result<bool, Diag> {
        self.0.borrow_mut().restore(state, id, image)
    }
}

/// 从源纹理取闭区间矩形(COPY 区域),输出落点恒为 (0,0)。
fn crop_texture(src: &Texture, x: u32, y: u32, width: u32, height: u32) -> Texture {
    if x == 0 && y == 0 && width == src.width && height == src.height {
        return Texture {
            width,
            height,
            rgba: src.rgba.clone(),
        };
    }
    let stride = src.width as usize * 4;
    let row = width as usize * 4;
    let mut rgba = Vec::with_capacity(row * height as usize);
    for row_index in 0..height {
        let start = (y + row_index) as usize * stride + x as usize * 4;
        rgba.extend_from_slice(&src.rgba[start..start + row]);
    }
    Texture {
        width,
        height,
        rgba,
    }
}

/// 内容哈希键:同名资产必然同字节,渲染器缓存无需失效。
fn synthetic_key(png: &[u8]) -> Vec<u8> {    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(png);
    let mut key = b"__synth/thumb-".to_vec();
    for byte in &digest[..8] {
        key.extend_from_slice(format!("{byte:02x}").as_bytes());
    }
    key.extend_from_slice(b".png");
    key
}

/// 抓帧表面尺寸:图层自身定过尺寸就用它,否则沿复制链找上游层。
///
/// 证据:官方 `CGACT[SIZE/SIZE2]` 是真实 StretchRect,层表面尺寸随之改变;
/// `COPY2` 复制的是源**表面**(尺寸随源),`CG` 的 `SX/SY` 只对图层集有效
/// (手册 `cg/cg.html`)。语料链 `SLTEMP`(`SIZE2` 100×75)→ `SLTEMP2`
/// (`COPY2` 落点) ⇒ 缩略图表面 = 100×75。
fn intended_size(state: &GfxState, layer: &Layer) -> Option<(u32, u32)> {
    fn usable(size: SizeU32) -> Option<(u32, u32)> {
        // 1×1 是 `FILE=""` 的占位尺寸,不代表用户声明的表面尺寸。
        (size.width > 1 || size.height > 1).then_some((size.width, size.height))
    }
    if let Some(size) = layer.image_size().and_then(usable) {
        return Some(size);
    }
    let mut texture = layer.texture.clone();
    for _ in 0..8 {
        let TextureRef::Snapshot(copy_id) = texture else {
            break;
        };
        let Some(copy) = state
            .copies()
            .iter()
            .rev()
            .find(|copy| copy.id == copy_id)
        else {
            break;
        };
        let CopySource::Layer(source) = &copy.source else {
            break;
        };
        let Some(source_layer) = state.layer(source) else {
            break;
        };
        if let Some(size) = source_layer.image_size().and_then(usable) {
            return Some(size);
        }
        texture = source_layer.texture.clone();
    }
    None
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;
    use yu_vfs::{Codec, MemSource};

    struct FakeSource {
        calls: Rc<RefCell<u32>>,
    }

    impl yu_render_core::SnapshotSource for FakeSource {
        fn read(&mut self, id: CopyId) -> Option<Surface> {
            *self.calls.borrow_mut() += 1;
            (id.0 == 7).then(|| Surface::new(2, 2, [1, 2, 3, 255]))
        }
    }

    #[test]
    fn snapshot_source_reads_on_demand_and_caches() {
        let store = TextureStore::new(
            Box::new(MemSource::new(Codec::Cp932)),
            Codec::Cp932,
            Vec::new(),
        );
        let mut capture = GfxCapture::new(store, SharedMem::new(Codec::Cp932));
        let calls = Rc::new(RefCell::new(0));
        capture.set_source(Box::new(FakeSource {
            calls: Rc::clone(&calls),
        }));
        capture.set_eager(false);

        let first = capture.snapshot_pixels(CopyId(7)).expect("source read");
        assert_eq!((first.width, first.height), (2, 2));
        assert_eq!(first.rgba, Surface::new(2, 2, [1, 2, 3, 255]).rgba);
        assert_eq!(*calls.borrow(), 1, "首次按需读取");

        let second = capture.snapshot_pixels(CopyId(7)).expect("cached");
        assert_eq!(Rc::ptr_eq(&first, &second), true);
        assert_eq!(*calls.borrow(), 1, "第二次必须命中缓存");

        assert!(capture.snapshot_pixels(CopyId(8)).is_none());
        assert_eq!(*calls.borrow(), 2, "未知快照只查一次端口");
    }
}
