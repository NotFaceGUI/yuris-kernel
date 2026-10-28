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
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use yu_backend_soft::{png, scale_region, SoftRenderer, Surface, Texture, TextureStore};
use yu_gfx::{CompositorFrame, CopyId, CopySource, GfxState, Layer, LayerCapture, Op, SizeU32, TextureRef};
use yu_log::Diag;
use yu_rt::event::CapturedImage;
use yu_vfs::SharedMem;

/// 镜像表条目上限(超过后按当前帧存活集合剪一次;见 `GfxCapture::observe`)。
const MIRROR_PRUNE_THRESHOLD: usize = 4096;

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
    /// 从**主渲染器**接过来的快照 CPU 镜像(`CopyId → 像素`)。
    ///
    /// 主渲染器在 1.4/1.6 段已经为每个可从 CPU 源解析的 COPY 建了同构镜像
    /// (`capture_copy_mirror`:目标尺寸 + 区域裁剪,与 soft `apply_copy_op`
    /// 同构造)。旧实现不用它们,只要出现新 CopyId 就让离屏软后端**整帧
    /// 回放**,在「每帧都换 CopyId」的场景里等于每帧多做一次全量软渲染
    /// (实测 100–375ms/帧)。现在先查镜像,只有镜像缺失(组源/后缓冲源)
    /// 才回放。
    mirrors: BTreeMap<CopyId, Rc<Texture>>,
    /// 是否允许「整帧软后端回放」兜底。GPU 后端 = false(改用按需读回);
    /// soft 后端 = true。
    allow_replay: bool,
}

impl GfxCapture {
    pub fn new(
        store: TextureStore,
        last_screen: Arc<Mutex<Option<Arc<Surface>>>>,
        overlay: SharedMem,
        allow_replay: bool,
    ) -> Self {
        Self {
            soft: SoftRenderer::new(store),
            last_screen,
            overlay,
            seen: BTreeSet::new(),
            mirrors: BTreeMap::new(),
            allow_replay,
        }
    }

    /// 在 `frame` 渲染后调用(参数 = 该帧合成帧 + **上一帧**呈现面 +
    /// 主渲染器的快照镜像查询)。
    ///
    /// 语义与旧实现相同:快照在**创建它的那一帧**冻结 —— `es_SaveLoad` 的
    /// 截图层(`COPY2 :/W0/ → SLTEMP`)保留的是按下存档菜单那一帧的画面。
    /// 差别只在实现:能从主渲染器拿到 CPU 镜像时**零回放**;只有镜像缺失
    /// (组源/后缓冲源)才让离屏软后端整帧回放一次。
    pub fn observe(
        &mut self,
        frame: &CompositorFrame,
        previous_screen: Option<Arc<Surface>>,
        mirror: &dyn Fn(CopyId) -> Option<Rc<Texture>>,
    ) -> Vec<CopyId> {
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
            // 镜像条目持有 `Rc<Texture>`(可能几 MB/张)。新 CopyId 稀少的
            // 游戏里 `live` 剪枝可能很久不触发,给一个尺寸上限兜底:
            // 超过上限时按当前帧存活集合剪一次(`live` 只扫 ops,罕见路径)。
            if self.mirrors.len() > MIRROR_PRUNE_THRESHOLD {
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
                self.mirrors.retain(|id, _| live.contains(id));
            }
            return Vec::new();
        }
        self.seen.extend(fresh.iter().copied());
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
        self.mirrors.retain(|id, _| live.contains(id));
        let mut missing = Vec::new();
        for id in fresh {
            match mirror(id) {
                Some(texture) => {
                    self.mirrors.insert(id, texture);
                }
                None => missing.push(id),
            }
        }
        // GPU 后端:缺失镜像由宿主按需**读回该快照**(1–8ms/张),不再整帧
        // 软后端重放(差分切换时实测 300–360ms/帧)。软后端没有 GPU 可读,
        // 仍回放一次(仅 soft 路径)。
        if !missing.is_empty() && self.allow_replay {
            self.soft.set_last_screen(previous_screen);
            self.soft.render_frame(frame);
        }
        missing
    }

    /// 写入一张从 GPU 读回的快照镜像(宿主在 `observe` 之后调用)。
    pub fn insert_mirror(&mut self, id: CopyId, width: u32, height: u32, rgba: Vec<u8>) {
        self.mirrors
            .insert(id, Rc::new(Texture { width, height, rgba }));
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
        let surface = match &texture {
            // 主渲染器转交的镜像优先:命中时**不做软后端整帧回放**。
            TextureRef::Snapshot(copy_id) => match self.mirrors.get(copy_id) {
                Some(mirror) => Some(Surface {
                    width: mirror.width,
                    height: mirror.height,
                    rgba: mirror.rgba.clone(),
                }),
                None if self.allow_replay => {
                    self.soft.set_last_screen(lock_screen(&self.last_screen).clone());
                    self.soft.render_frame(&frame);
                    self.soft.snapshot_surface(*copy_id).cloned()
                }
                // GPU 后端:镜像由宿主在帧末按需读回;仍未命中就显式报缺
                // (交给 VM 记 `gfx.capture.mirror_missing`,不静默回放整帧)。
                None => {
                    log::warn!(
                        target: "yu-engine",
                        "[caption] snapshot {} has no CPU mirror (GPU path; save image unavailable)",
                        copy_id.0
                    );
                    None
                }
            },
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
