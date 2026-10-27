//! `SAVE[ID]` / `LOAD[ID]` 的图层像素通道(Engine 装配)。
//!
//! - **读取**(`SAVE[ID]`):官方在命令处理器里直接读图层表面(读取端
//!   `0x4477c0` 首 u32 = `0x640`)。本内核没有常驻的层像素面,因此用一份
//!   离屏软后端**按主渲染器同序回放** COPY/图像加工命令:
//!   1. [`GfxCapture::observe`] 在每帧渲染前把「有新 COPY 的帧」喂给离屏
//!      渲染器(`last_screen` = 上一帧呈现结果),快照因此与主渲染器在
//!      **同一时刻**冻结(截图层保留的是按 SAVE 菜单那一帧的画面);
//!   2. [`LayerCapture::capture`] 在 `SAVE[ID]` 命令时刻回放当前帧,取目标
//!      层的快照表面(含 `CGACT[SIZE2]` 的缩放结果,按图层声明尺寸重采样)。
//! - **写入**(`LOAD[ID]`):把图像编码成 PNG,写入工程的合成资产叠层
//!   ([`yu_vfs::SharedMem`],内容哈希命名),再把图层纹理指到该资产并注入
//!   尺寸。渲染器走常规资产管线,无需任何后端改动。

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use yu_backend_soft::{png, scale_region, SoftRenderer, Surface, Texture, TextureStore};
use yu_gfx::{CompositorFrame, CopyId, CopySource, GfxState, Layer, LayerCapture, Op, SizeU32, TextureRef};
use yu_log::Diag;
use yu_rt::event::CapturedImage;
use yu_vfs::SharedMem;

/// 抓帧渲染器 + 运行时合成资产叠层。
pub struct GfxCapture {
    /// 离屏抓帧(`SoftRenderer` 是参考实现,与主后端逐像素同口径)。
    soft: SoftRenderer,
    /// 上一帧呈现的屏幕(`COPY ID=:/W0/` 的后缓冲)。
    last_screen: Arc<Mutex<Option<Arc<Surface>>>>,
    /// 合成资产叠层(`LOAD[ID]` 写入点;渲染器与 VM 共享)。
    overlay: SharedMem,
    /// 已经在离屏渲染器里冻结过的快照(`observe` 去重;未出现新 COPY 的
    /// 帧不重复整帧回放,避免给每帧加一份软渲染成本)。
    seen: BTreeSet<CopyId>,
}

impl GfxCapture {
    pub fn new(
        store: TextureStore,
        last_screen: Arc<Mutex<Option<Arc<Surface>>>>,
        overlay: SharedMem,
    ) -> Self {
        Self {
            soft: SoftRenderer::new(store),
            last_screen,
            overlay,
            seen: BTreeSet::new(),
        }
    }

    /// 在 `frame` 之前调用(参数 = 该帧即将渲染的合成帧 + **上一帧**呈现面)。
    ///
    /// 只在该帧出现**新 CopyId** 时整帧回放:离屏渲染器的快照表因此与主
    /// 渲染器同刻冻结 —— `es_SaveLoad` 的截图层(`COPY2 :/W0/ → SLTEMP`)
    /// 保留的是按下存档菜单那一帧的画面,而不是保存确认时(存档界面)的画面。
    pub fn observe(&mut self, frame: &CompositorFrame, previous_screen: Option<Arc<Surface>>) {
        let fresh: Vec<CopyId> = frame
            .passes
            .iter()
            .flat_map(|pass| pass.ops.iter())
            .filter_map(|op| match op {
                Op::Copy(copy) if copy.status == yu_gfx::OpStatus::Executable => {
                    (!self.seen.contains(&copy.id)).then_some(copy.id)
                }
                _ => None,
            })
            .collect();
        if fresh.is_empty() {
            return;
        }
        self.seen.extend(fresh);
        // 快照只随帧内引用存活:同步一次存活集合,避免无界增长。
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
        self.soft.set_last_screen(previous_screen);
        self.soft.render_frame(frame);
    }
}

fn lock_screen(
    screen: &Arc<Mutex<Option<Arc<Surface>>>>,
) -> std::sync::MutexGuard<'_, Option<Arc<Surface>>> {
    screen
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
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
        self.soft.set_last_screen(lock_screen(&self.last_screen).clone());
        self.soft.render_frame(&frame);
        let surface = match &texture {
            TextureRef::Snapshot(copy_id) => self.soft.snapshot_surface(*copy_id).cloned(),
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
            TextureRef::SolidBlack1x1 => Some(Surface::new(1, 1, [0, 0, 0, 255])),
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
        let png = png::png_bytes(&surface).map_err(|e| {
            yu_gfx::error::recoverable(
                yu_gfx::error::code::INFO_UNSUPPORTED,
                format!("LOAD[ID] 缩略图 PNG 编码失败: {e}"),
            )
        })?;
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
pub struct SharedCapture(pub Rc<RefCell<GfxCapture>>);

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
