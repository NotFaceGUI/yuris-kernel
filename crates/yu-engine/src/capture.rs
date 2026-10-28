//! `SAVE[ID]` / `LOAD[ID]` 的图层像素通道(Engine 装配)。
//!
//! - **读取**(`SAVE[ID]`):官方在命令处理器里直接读图层表面(读取端
//!   `0x4477c0` 首 u32 = `0x640`)。本内核的层表面常驻在渲染器里(soft 是
//!   `Surface`,wgpu 是 GPU 纹理),抓帧器只保留**按需**取得的快照像素:
//!   `observe` 每帧只登记新出现的 `CopyId`,由宿主在渲染后读回这一张
//!   (低频;不再是每帧 CPU 镜像预捕获)。
//! - **写入**(`LOAD[ID]`):把图像编码成 PNG,写入工程的合成资产叠层
//!   ([`yu_vfs::SharedMem`],内容哈希命名),再把图层纹理指到该资产并注入
//!   尺寸。渲染器走常规资产管线,无需任何后端改动。

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use yu_backend_soft::{png, scale_region, Surface, Texture, TextureStore};
use yu_gfx::{CompositorFrame, CopyId, CopySource, GfxState, Layer, LayerCapture, Op, SizeU32, TextureRef};
use yu_log::Diag;
use yu_rt::event::CapturedImage;
use yu_vfs::SharedMem;

/// 抓帧器:快照像素缓存 + 运行时合成资产叠层。
pub struct GfxCapture {
    /// 资产解码(与主渲染器同一份 vfs/编解码口径)。
    textures: TextureStore,
    /// 合成资产叠层(`LOAD[ID]` 写入点;渲染器与 VM 共享)。
    overlay: SharedMem,
    /// 已经在离屏渲染器里冻结过的快照(`observe` 去重)。
    seen: BTreeSet<CopyId>,
    /// 快照像素(**按需**读回/软后端直接借用;不是每帧镜像)。
    pixels: BTreeMap<CopyId, Rc<Texture>>,
    /// 抓帧时仍缺像素的快照(下一帧渲染后由宿主补齐)。
    wanted: BTreeSet<CopyId>,
    /// 抓帧缺像素次数(显式计数,不静默)。
    pub missing_pixels: u64,
}

impl GfxCapture {
    pub fn new(textures: TextureStore, overlay: SharedMem) -> Self {
        Self {
            textures,
            overlay,
            seen: BTreeSet::new(),
            pixels: BTreeMap::new(),
            wanted: BTreeSet::new(),
            missing_pixels: 0,
        }
    }

    /// 在 `frame` 渲染后调用:登记本帧新出现的快照,返回需要宿主读回像素的
    /// `CopyId`(wgpu:读回这一张;soft:直接借用 CPU 表面)。
    pub fn observe(&mut self, frame: &CompositorFrame) -> Vec<CopyId> {
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
        // 快照只随帧内引用存活:同步一次集合,避免无界增长。
        if !fresh.is_empty() || !self.wanted.is_empty() {
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
            self.wanted.retain(|id| live.contains(id) && !self.pixels.contains_key(id));
            for id in self.wanted.iter() {
                if !fresh.contains(id) {
                    fresh.push(*id);
                }
            }
        }
        fresh
    }

    /// 写入一张从渲染器读回的快照像素(宿主在 `observe` 之后调用)。
    pub fn insert_mirror(&mut self, id: CopyId, width: u32, height: u32, rgba: Vec<u8>) {
        self.wanted.remove(&id);
        self.pixels
            .insert(id, Rc::new(Texture { width, height, rgba }));
    }
}

impl LayerCapture for GfxCapture {
    fn capture(&mut self, state: &GfxState, id: &[u8]) -> Result<Option<CapturedImage>, Diag> {
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
        let frame = state.frame();
        let surface = match &texture {
            TextureRef::Snapshot(copy_id) => match self.pixels.get(copy_id) {
                Some(mirror) => Some(Surface {
                    width: mirror.width,
                    height: mirror.height,
                    rgba: mirror.rgba.clone(),
                }),
                None => {
                    // 命令时刻还没有像素:登记为 wanted,下一帧渲染后补齐。
                    self.wanted.insert(*copy_id);
                    self.missing_pixels += 1;
                    log::warn!(
                        target: "yu-engine",
                        "[caption] snapshot {} pixels not materialized yet; deferred",
                        copy_id.0
                    );
                    None
                }
            },
            TextureRef::Asset(key) => self
                .textures
                .resolve(key)
                .ok()
                .map(|texture| Surface {
                    width: texture.width,
                    height: texture.height,
                    rgba: texture.rgba.clone(),
                }),
            TextureRef::SolidBlack1x1 => Some(Surface::new(1, 1, [0, 0, 0, 255])),
            TextureRef::None | TextureRef::Offscreen(_) => None,
        };
        let _ = &frame;
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

/// 内容哈希键:同名资产必然同字节,渲染器缓存无需失效。
fn synthetic_key(png: &[u8]) -> Vec<u8> {
    use sha2::{Digest, Sha256};
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
