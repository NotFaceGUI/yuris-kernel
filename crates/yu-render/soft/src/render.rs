//! 确定性软件合成器。
//!
//! 严格按 [`CompositorFrame::passes`] 顺序执行;任何未取证/未实现的语义都
//! 跳过并写入 [`RenderReport`],不近似、不猜测。
//!
//! 像素规则(`docs/render-parity.md` §12):
//!
//! - 固定管线 alpha = `min(max(A,0),255)/255`;纹理 alpha 与层 alpha 在混合
//!   方程里相乘,纹理保持直通 alpha(不预乘);
//! - `MODE 0`: RGB `src·a + dst·(1−a)` / alpha `a² + dst·(1−a)`;
//!   `MODE 1`: RGB `src·a + dst` / alpha `a² + dst`;
//!   `MODE 2`: RGB `dst − src·a` / alpha `dst − a²`。
//!   alpha 源项是 `a²` 而不是 `a`:D3D9 未启用 `SEPARATEALPHABLENDENABLE`
//!   时 alpha 通道复用同一组因子(`SRCALPHA` 因子为 `(As,As,As,As)`)。
//!   证据:`EVID-J-01`(`docs/fidelity-ledger-j.md`)。
//! - 双纹理过渡(`EVID-T-05`):RGB 取本层纹理;覆盖度 =
//!   [`yu_gfx::transition_coverage`](mask = 配对纹理 G 通道,fade = YS_FadeVal,
//!   `T>=256` 为满),再乘纹理 α 与层 `A`。
//! - `Point` 最近邻,`Linear/Anisotropic` 双线性;缩放采样全部用整数
//!   16.16 定点,避免浮点跨平台差异;
//! - 颜色空间按存储值直接混合,不做 sRGB↔线性转换。

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::sync::Arc;

use yu_gfx::{
    transition_coverage, BlendMode, Clear, CompositorFrame, CopyId, CopyOp, CopySource, FilterMode,
    ImageAction, ImageOp, Layer, LayerId, Op, OpStatus, PassTarget, TextDraw, TextureRef,
};

use crate::imageop::{apply_horizontal, apply_rect_paint, apply_reverse};
use crate::text::{layer_base, text_base_key, texture_content_hash, TextComposer, TextPaintStats};
use crate::texture::{AssetIssue, Texture, TextureStore};

/// RGBA8 表面(行优先,直通 alpha)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Surface {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Surface {
    /// 用单一 RGBA 颜色填满。
    pub fn new(width: u32, height: u32, rgba: [u8; 4]) -> Self {
        let pixels = (width as usize).saturating_mul(height as usize);
        let mut bytes = Vec::with_capacity(pixels.saturating_mul(4));
        for _ in 0..pixels {
            bytes.extend_from_slice(&rgba);
        }
        Self {
            width,
            height,
            rgba: bytes,
        }
    }

    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = ((y as usize) * (self.width as usize) + x as usize) * 4;
        Some([
            self.rgba[i],
            self.rgba[i + 1],
            self.rgba[i + 2],
            self.rgba[i + 3],
        ])
    }

    fn set_pixel(&mut self, x: u32, y: u32, px: [u8; 4]) {
        let i = ((y as usize) * (self.width as usize) + x as usize) * 4;
        self.rgba[i..i + 4].copy_from_slice(&px);
    }

    fn from_texture(texture: &Texture) -> Self {
        Self {
            width: texture.width,
            height: texture.height,
            rgba: texture.rgba.clone(),
        }
    }
}

/// 一种被跳过的操作(按 `(kind, reason, target)` 聚合计数)。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SkippedOp {
    /// `layer` / `group` / `transition` / `mask` / `copy` / `image`。
    pub kind: &'static str,
    /// 跳过原因(`OpStatus` 的稳定 code,或后端自身的原因)。
    pub reason: String,
    /// 操作目标图层/组(稳定显示形式)。
    pub target: String,
    /// 同一 key 在本帧的出现次数。
    pub count: u32,
}

/// 一帧的渲染摘要。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderReport {
    pub frame: u64,
    pub viewport_width: u32,
    pub viewport_height: u32,
    /// `CompositorFrame.layers` 数量(含不可见层)。
    pub layers: usize,
    pub visible_layers: usize,
    /// pass 总数(含空 pass)。
    pub passes: usize,
    /// 实际执行并写入 offscreen 表面的组 pass 数。
    pub offscreen_passes: usize,
    pub ops_total: usize,
    /// 真正执行(含完全在视口外、未写像素的操作)。
    pub ops_drawn: usize,
    pub skipped_ops_total: u32,
    pub skipped: Vec<SkippedOp>,
    /// 本帧解析失败的资产(`asset.*`)。
    pub missing_assets: Vec<AssetIssue>,
    /// 本帧 `TextRun` 数量(全部未栅格化)。
    pub skipped_text: usize,
    /// `CGACT[TEXT]` 绘制项应用数。
    pub text_draws: usize,
    /// 文本栅格化成功写入的字符数。
    pub text_glyphs: usize,
    /// 文本缺字/不可解码字符数。
    pub text_missing: usize,
    /// 文本失败原因(去重、稳定排序)。
    pub text_diags: Vec<String>,
    pub filter_mag: &'static str,
    pub filter_min: &'static str,
    pub filter_mip: &'static str,
    /// `engine_settings` / `placeholder`。
    pub filter_source: &'static str,
    pub color_space: &'static str,
}

/// `render_frame` 的输出。
///
/// `surface` 用 `Arc` 共享:软后端要把同一份屏幕像素留作下一帧的
/// `COPY ID=:/W0/` 后缓冲,引擎/宿主又要消费返回值。入 `Arc` 后
/// 每帧不再复制 800×600×4 ≈ 1.9MB 的整帧。
#[derive(Clone, Debug)]
pub struct RenderOutput {
    pub surface: Arc<Surface>,
    pub report: RenderReport,
}

/// `CopyId -> 已捕获的快照表面`。
///
/// EVID-T-12:官方 COPY 在**命令执行时**取源层当前状态,快照在目标层存活
/// 期间保持。因此同一 `CopyId` 只捕获一次;新的 COPY 命令由 gfx 分配新 id。
pub(crate) type Snapshots = BTreeMap<CopyId, Surface>;

/// 软件渲染器:持有纹理解析缓存、帧内离屏表面与持久快照。
pub struct SoftRenderer {
    textures: TextureStore,
    snapshots: Snapshots,
    /// 上一帧呈现的屏幕内容(引擎 `COPY ID=:/W0/` 读的是当时后缓冲)。
    last_screen: Option<Arc<Surface>>,
    /// `CGACT[SIZE/SIZE2]`/`REVERSE`/`HORIZONTAL` 结果缓存(源身份 + 参数)。
    ///
    /// 精灵脚本每帧对所有可见层重发 SIZE;官方是 GPU StretchRect,
    /// 本后端若每帧重算 800×600 会把帧耗时推到数百 ms(EVID-J3-04)。
    image_cache: BTreeMap<ImageCacheKey, Rc<Texture>>,
    /// 文本栅格化/合成器(懒加载系统字体;无文本帧零成本)。
    text: TextComposer,
}

/// 图像加工的源身份(资产 key / 1×1 黑 / 冻结快照)。
///
/// 用它而不是图层名做缓存键:同一份像素无论挂在哪个图层名下,加工结果相同
/// (EVID-T-10:图层名只是脚本约定,不参与引擎语义)。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ImageSource {
    Asset(Vec<u8>),
    Solid,
    Snapshot(u64),
}

/// 图像加工的确定性签名(同一源 + 同一签名 ⇒ 同一结果)。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ImageOpSig {
    /// `CGACT[SIZE]`(最近邻)/`SIZE2`(双线性)。
    Size {
        bilinear: bool,
        width: u32,
        height: u32,
    },
    /// `CGACT[REVERSE]`:RGB 取反。
    Reverse,
    /// `CGACT[HORIZONTAL]`:水平镜像。
    Horizontal,
}

/// 跨帧图像结果缓存键。
///
/// 脚本每帧都会重发同样的 `CGACT[SIZE]`/`REVERSE`(title 期实测每帧 51 条
/// `SIZE` + 1 条 `REVERSE`);结果只依赖源像素与参数,缓存后帧内无像素工作
/// (EVID-J3-08)。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ImageCacheKey {
    pub source: ImageSource,
    pub op: ImageOpSig,
    pub region: Option<(i64, i64, i64, i64)>,
}

/// 兼容旧名(任务内已有引用)。
pub type SizeSource = ImageSource;
/// 兼容旧名。
pub type SizeKey = ImageCacheKey;

impl SoftRenderer {
    pub fn new(textures: TextureStore) -> Self {
        Self {
            textures,
            snapshots: BTreeMap::new(),
            last_screen: None,
            image_cache: BTreeMap::new(),
            text: TextComposer::new(),
        }
    }

    /// 用调用方字体源构造文本合成器(测试/自带字体)。
    pub fn with_text_source(mut self, source: yu_font::FontSource) -> Self {
        self.text = TextComposer::with_source(source);
        self
    }

    /// 文本合成器(诊断/测试用)。
    pub fn text_composer(&mut self) -> &mut TextComposer {
        &mut self.text
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

    /// 丢弃跨帧状态(重启用;DIV-HOST-001)。
    ///
    /// 引擎重启 = 新设备:后缓冲、快照、图像加工缓存都属于上一轮
    /// `GfxState`。`CopyId` 在新 `GfxState` 里从 0 重新分配,若不清理,
    /// 新复制会命中同 id 的旧快照(冻结早退)而拿到重启前的像素。
    ///
    /// 纹理解析缓存(`TextureStore`)保留 —— 它只依赖游戏数据。
    pub fn reset(&mut self) {
        self.snapshots.clear();
        self.last_screen = None;
        self.image_cache.clear();
    }

    pub fn textures(&self) -> &TextureStore {
        &self.textures
    }

    pub fn textures_mut(&mut self) -> &mut TextureStore {
        &mut self.textures
    }

    /// 执行一帧。输出表面尺寸恒为 `frame.viewport`。
    pub fn render_frame(&mut self, frame: &CompositorFrame) -> RenderOutput {
        self.textures.begin_frame();
        prune_snapshots(&mut self.snapshots, frame);
        let mut st = RenderState::new(frame, &mut self.text);
        let mut screen: Option<Surface> = None;

        for pass in &frame.passes {
            st.report.passes += 1;
            match &pass.target {
                PassTarget::Texture(target) => {
                    for op in &pass.ops {
                        st.report.ops_total += 1;
                        match op {
                            Op::Image(image) if op.status() == OpStatus::Executable => {
                                match apply_image_op(
                                    &mut self.textures,
                                    &self.snapshots,
                                    frame,
                                    target,
                                    image,
                                    &mut st,
                                    &mut self.image_cache,
                                ) {
                                    Ok(()) => st.report.ops_drawn += 1,
                                    Err(reason) => st.skip_op(op, reason),
                                }
                            }
                            Op::Copy(copy) if op.status() == OpStatus::Executable => {
                                match apply_copy_op(
                                    &mut self.textures,
                                    &mut self.snapshots,
                                    frame,
                                    target,
                                    copy,
                                    &mut st,
                                    self.last_screen.as_deref(),
                                ) {
                                    Ok(()) => st.report.ops_drawn += 1,
                                    Err(reason) => st.skip_op(op, reason),
                                }
                            }
                            _ => {
                                let reason = match op.status() {
                                    OpStatus::Unimplemented(why) => why.to_string(),
                                    OpStatus::Executable => "texture_pass.not_executed".to_string(),
                                };
                                st.skip_op(op, reason);
                            }
                        }
                    }
                }
                PassTarget::Offscreen(id) => {
                    let (width, height) = group_surface_size(frame, id);
                    if width == 0 || height == 0 {
                        for op in &pass.ops {
                            st.report.ops_total += 1;
                            st.skip_op(op, "group.size.zero");
                        }
                        continue;
                    }
                    let clear = pass.clear.unwrap_or(Clear::explicit([0, 0, 0, 0]));
                    let mut surface = Surface::new(width, height, clear.rgba);
                    for op in &pass.ops {
                        draw_op(
                            &mut self.textures,
                            &self.snapshots,
                            frame,
                            op,
                            &mut surface,
                            &mut st,
                        );
                    }
                    st.report.offscreen_passes += 1;
                    st.offscreens.insert(id.clone(), surface);
                }
                PassTarget::Screen => {
                    // 契约:屏幕 pass 的清屏来自 CompositorFrame.clear
                    // (`Pass.clear` 只服务离屏目标)。
                    let clear = frame.clear;
                    let mut surface =
                        Surface::new(frame.viewport.width, frame.viewport.height, clear.rgba);
                    for op in &pass.ops {
                        draw_op(
                            &mut self.textures,
                            &self.snapshots,
                            frame,
                            op,
                            &mut surface,
                            &mut st,
                        );
                    }
                    screen = Some(surface);
                }
            }
        }

        debug_assert_eq!(
            st.report.ops_total,
            st.report.ops_drawn + st.report.skipped_ops_total as usize,
            "每个 op 必须恰好被绘制或跳过"
        );
        st.report.missing_assets = self.textures.issues();
        let mut surface = screen.unwrap_or_else(|| {
            Surface::new(
                frame.viewport.width,
                frame.viewport.height,
                frame.clear.rgba,
            )
        });
        // 自动文本层(`TEXT[ID]` 不存在/为空时的屏幕叠加):`_TEXT_FLAG` 已由
        // gfx 排版;引擎在窗口 CG 之上绘制文本层(DIV-GFX-057,`Likely`)。
        if let Some(layout) = frame.text_layout.as_ref() {
            match &layout.target {
                None => {
                    // 屏幕叠加:先画到透明覆盖层,再用与 wgpu 相同的纹理
                    // 合成路径(MODE 0)贴上屏幕 —— 两端像素公式一致。
                    let mut overlay = Surface::new(surface.width, surface.height, [0, 0, 0, 0]);
                    let stats = st.text.paint_layout_with(
                        &mut overlay.rgba,
                        overlay.width,
                        overlay.height,
                        layout,
                        Some(true),
                    );
                    let painted = stats.glyphs;
                    merge_text_stats(&mut st.report, stats);
                    if painted > 0 {
                        let image = ImageRef::from_surface(&overlay);
                        draw_image(
                            &mut surface,
                            image,
                            0,
                            0,
                            overlay.width,
                            overlay.height,
                            255,
                            BlendMode::Normal,
                            frame.filter.mag,
                            None,
                        );
                    }
                }
                Some(target) if !frame.layers.iter().any(|l| &l.id == target) => {
                    let why = format!("text.layout_target_missing: {}", target.display());
                    if !st.report.text_diags.contains(&why) {
                        st.report.text_diags.push(why);
                        st.report.text_diags.sort();
                    }
                }
                Some(_) => {}
            }
        }
        // 屏幕是 opaque 窗口:组 RT 的 alpha 只参与“组合成到屏幕”这一步,
        // 最终呈现不带逐像素 alpha(D3D9 窗口后缓冲不用于桌面混合;浏览器
        // canvas 也是 alphaMode=opaque)。不归一会在按钮抗锯齿等半透明像素上
        // 留下 <255 的屏幕 alpha,使 soft/wgpu 与浏览器读回的同帧哈希不同。
        for px in surface.rgba.chunks_exact_mut(4) {
            px[3] = 255;
        }
        let surface = Arc::new(surface);
        self.last_screen = Some(Arc::clone(&surface));
        RenderOutput {
            surface,
            report: st.finish(),
        }
    }
}

/// 只保留本帧仍被引用的快照(`CopyId` 由目标层 `Snapshot` 或本帧复制引用)。
fn prune_snapshots(snapshots: &mut Snapshots, frame: &CompositorFrame) {
    if snapshots.is_empty() {
        return;
    }
    let mut live = std::collections::BTreeSet::new();
    for layer in &frame.layers {
        if let TextureRef::Snapshot(id) = &layer.texture {
            live.insert(*id);
        }
    }
    for pass in &frame.passes {
        for op in &pass.ops {
            if let Op::Copy(copy) = op {
                live.insert(copy.id);
            }
        }
    }
    snapshots.retain(|id, _| live.contains(id));
}

/// 执行一个可执行的 `Op::Image`。
///
/// 目标图层的基图在帧内第一次被加工时拷贝出可变副本;后续加工继续
/// 作用在同一副本上。图层绘制阶段优先使用该副本。
fn apply_image_op(
    textures: &mut TextureStore,
    snapshots: &Snapshots,
    frame: &CompositorFrame,
    pass_target: &LayerId,
    image: &ImageOp,
    st: &mut RenderState,
    image_cache: &mut BTreeMap<ImageCacheKey, Rc<Texture>>,
) -> Result<(), String> {
    let target = image.target.as_ref().unwrap_or(&image.source);
    if target != pass_target {
        return Err("gfx.imageop.target.mismatch".to_string());
    }
    let region = match (image.x, image.y, image.x2, image.y2) {
        (Some(x), Some(y), Some(x2), Some(y2)) => Some((x, y, x2, y2)),
        (None, None, None, None) => None,
        _ => return Err("gfx.imageop.region.partial".to_string()),
    };
    if matches!(
        image.action,
        ImageAction::SizeNearest | ImageAction::SizeBilinear
    ) {
        let (Some(dw), Some(dh)) = (image.sx, image.sy) else {
            return Err("gfx.imageop.size.missing".to_string());
        };
        // 缩放尺寸是浮点(实测 `SY=825.5`);`<= 0` 才是「无尺寸」。
        if dw <= 0.0 || dh <= 0.0 {
            return Ok(());
        }
        // 纹理像素只能是整数:四舍五入(与 `yu-gfx` 写层尺寸的口径一致)。
        let (dw, dh) = (dw.round() as u32, dh.round() as u32);
        let layer = frame
            .layers
            .iter()
            .find(|layer| layer.id == image.source)
            .ok_or_else(|| "gfx.copy.source.missing".to_string())?;
        let source_key = if st.texture_overrides.contains_key(&image.source) {
            None
        } else {
            match &layer.texture {
                TextureRef::Asset(key) => Some(SizeSource::Asset(key.clone())),
                TextureRef::SolidBlack1x1 => Some(SizeSource::Solid),
                TextureRef::Snapshot(id) => Some(SizeSource::Snapshot(id.0)),
                _ => None,
            }
        };
        let key = source_key.as_ref().map(|source| ImageCacheKey {
            source: source.clone(),
            op: ImageOpSig::Size {
                bilinear: image.action == ImageAction::SizeBilinear,
                width: dw,
                height: dh,
            },
            region,
        });
        if let Some(key) = &key {
            if let Some(hit) = image_cache.get(key) {
                st.texture_overrides.insert(target.clone(), hit.clone());
                return Ok(());
            }
        }
        let source = layer_surface(textures, snapshots, frame, &image.source, st, None)?;
        if region.is_none() {
            // 无区域 = 整幅源缩放到目标尺寸。绘制目标层时纹理会按
            // `layer.image_size()`(= `SX×SY`)拉伸,因此**像素无需重采样**:
            // 直接复用源纹理即可(与引擎的 StretchRect 等价,且省掉整幅
            // CPU 缩放;`SPLTRBUF→SPLTR` 每帧一条曾占 ~14 ms)。
            let texture = crate::texture::Texture {
                width: source.width,
                height: source.height,
                rgba: source.rgba,
            };
            let shared = Rc::new(texture);
            if let Some(key) = key {
                image_cache.insert(key, shared.clone());
            }
            st.texture_overrides.insert(target.clone(), shared);
            return Ok(());
        }
        let source = crate::texture::Texture {
            width: source.width,
            height: source.height,
            rgba: source.rgba,
        };
        let scaled = Rc::new(crate::imageop::scale_region(
            &source,
            dw,
            dh,
            region,
            image.action == ImageAction::SizeBilinear,
        ));
        if let Some(key) = key {
            image_cache.insert(key, scaled.clone());
        }
        st.texture_overrides.insert(target.clone(), scaled);
        return Ok(());
    }
    // `REVERSE`/`HORIZONTAL`:整层像素变换,同样按“源身份 + 操作”缓存。
    // 脚本每帧重发同一条(配对 mask 每帧一次),缓存后帧内零像素工作。
    if matches!(image.action, ImageAction::Reverse | ImageAction::Horizontal) {
        if let Some(src) = st.texture_overrides.get(&image.source) {
            // 源已在本帧加工过:基于当前像素做一次(结果随源变化,不缓存)。
            let mut texture = (**src).clone();
            match image.action {
                ImageAction::Reverse => apply_reverse(&mut texture),
                _ => apply_horizontal(&mut texture),
            }
            st.texture_overrides
                .insert(target.clone(), Rc::new(texture));
            return Ok(());
        }
        if let Some(layer) = frame.layers.iter().find(|l| l.id == image.source) {
            let sig = if image.action == ImageAction::Reverse {
                ImageOpSig::Reverse
            } else {
                ImageOpSig::Horizontal
            };
            let source_id = match &layer.texture {
                TextureRef::Asset(key) => Some(ImageSource::Asset(key.clone())),
                TextureRef::SolidBlack1x1 => Some(ImageSource::Solid),
                TextureRef::Snapshot(id) => Some(ImageSource::Snapshot(id.0)),
                _ => None,
            };
            let key = source_id.map(|source| ImageCacheKey {
                source,
                op: sig,
                region: None,
            });
            if let Some(key) = &key {
                if let Some(hit) = image_cache.get(key) {
                    st.texture_overrides.insert(target.clone(), hit.clone());
                    return Ok(());
                }
            }
            let mut texture = base_texture(textures, snapshots, frame, target)?;
            match image.action {
                ImageAction::Reverse => apply_reverse(&mut texture),
                _ => apply_horizontal(&mut texture),
            }
            let shared = Rc::new(texture);
            if let Some(key) = key {
                image_cache.insert(key, shared.clone());
            }
            st.texture_overrides.insert(target.clone(), shared);
            return Ok(());
        }
    }
    let mut texture = match st.texture_overrides.remove(target) {
        Some(texture) => Rc::try_unwrap(texture).unwrap_or_else(|rc| (*rc).clone()),
        None => base_texture(textures, snapshots, frame, target)?,
    };
    match image.action {
        ImageAction::RectPaint | ImageAction::RectPaint2 | ImageAction::RectPaint3 => {
            let Some(set) = image.set.as_ref().and_then(|v| v.as_int()) else {
                return Err("gfx.imageop.set.missing".to_string());
            };
            apply_rect_paint(&mut texture, image.action.clone(), set, region)?;
        }
        ImageAction::Reverse => {
            if region.is_some() {
                return Err("gfx.imageop.region.unsupported".to_string());
            }
            apply_reverse(&mut texture);
        }
        ImageAction::Horizontal => {
            if region.is_some() {
                return Err("gfx.imageop.region.unsupported".to_string());
            }
            apply_horizontal(&mut texture);
        }
        _ => return Err("gfx.imageop.unsupported".to_string()),
    }
    st.texture_overrides
        .insert(target.clone(), Rc::new(texture));
    Ok(())
}

/// 执行一次 COPY/COPY2。
///
/// 语义(EVID-T-11/T-12):
/// - 源 = 命令执行时的当前状态:图层取当前基图/快照,组取当前子层合成;
/// - 区域 `X/Y/X2/Y2` 是**源**纹理坐标、含端点;裁剪块写到目标 `(0,0)`;
/// - 无区域 = 整图复制;目标表面尺寸 = 目标层显式尺寸,否则取快照尺寸;
/// - 同一 `CopyId` 只捕获一次(快照在目标层存活期间冻结)。
#[allow(clippy::too_many_arguments)]
fn apply_copy_op(
    textures: &mut TextureStore,
    snapshots: &mut Snapshots,
    frame: &CompositorFrame,
    pass_target: &LayerId,
    copy: &CopyOp,
    st: &mut RenderState,
    last_screen: Option<&Surface>,
) -> Result<(), String> {
    if &copy.dest != pass_target {
        return Err("gfx.copy.target.mismatch".to_string());
    }
    if snapshots.contains_key(&copy.id) {
        // 已捕获:快照冻结,不再从当前状态重算。
        return Ok(());
    }
    // 引擎 `COPY ID=:/W0/` 复制的是**当前后缓冲**,即上一帧呈现结果
    // (`es_Scenario`/`es_ERIS` 的 CGS1/CGS100 截屏盖屏机制)。若按“命令执行
    // 时已消费过本帧事件”的组内容重绘,会把本帧新出现的 title 层也截进去,
    // 导致 fade 起点错位(EVID-J3-06)。
    //
    // **首帧没有后缓冲**(DIV-GFX-051):D3D9 设备创建后后缓冲即存在,内容
    // 是初始化清屏色(DIV-GFX-010:`Clear::default_black` 的来源)。主组已
    // 直绘、不再有离屏 RT,因此这里用 `frame.clear` 合成一张视口大小的
    // 后缓冲初值——与“官方首帧截图 = 启动黑屏”一致,而不是报
    // `offscreen_missing` 跳过。
    let main_group = frame
        .windows
        .iter()
        .find(|w| w.number == 0)
        .and_then(|w| w.layer_set.as_ref());
    let source = match &copy.source {
        CopySource::Group(id) if Some(id) == main_group => match last_screen {
            Some(surface) => surface.clone(),
            None => Surface::new(
                frame.viewport.width,
                frame.viewport.height,
                frame.clear.rgba,
            ),
        },
        CopySource::Group(id) => compose_group(textures, snapshots, frame, id, st)?,
        CopySource::Layer(id) => {
            let mut surface = layer_surface(textures, snapshots, frame, id, st, Some(copy))?;
            if !copy.texts.is_empty() {
                compose_copy_texts(&mut surface, copy, st);
            }
            surface
        }
    };
    let dest_size = frame
        .layers
        .iter()
        .find(|layer| layer.id == copy.dest)
        .and_then(|layer| layer.image_size())
        .map(|size| (size.width, size.height));
    let (width, height) = dest_size.unwrap_or((source.width, source.height));
    let mut surface = Surface::new(width, height, [0, 0, 0, 0]);
    let (x0, y0, x1, y1) = copy.region.resolve(source.width, source.height);
    blit(&source, &mut surface, x0, y0, x1, y1);
    snapshots.insert(copy.id, surface);
    Ok(())
}

/// 在 COPY 源面上合成命令时刻冻结的文本(DIV-GFX-058:官方 `CGACT[TEXT]`
/// 写层表面,COPY 天然携带已画文本)。
fn compose_copy_texts(surface: &mut Surface, copy: &CopyOp, st: &mut RenderState) {
    let base = Texture {
        width: surface.width,
        height: surface.height,
        rgba: std::mem::take(&mut surface.rgba),
    };
    let (composed, stats) = st.text.compose_layer(&base, &copy.texts, None);
    merge_text_stats(&mut st.report, stats);
    if let Some(texture) = composed {
        surface.width = texture.width;
        surface.height = texture.height;
        surface.rgba = texture.rgba;
    } else {
        surface.rgba = base.rgba;
    }
}

/// 把 `src` 的矩形(含端点)写到 `dst` 的 `(0,0)`;越界裁剪。
///
/// 快照 CPU 镜像的构造(wgpu 端 DIV-GFX-058)复用同一裁剪规则。
pub fn blit(src: &Surface, dst: &mut Surface, x0: i64, y0: i64, x1: i64, y1: i64) {
    if x1 < x0 || y1 < y0 || src.width == 0 || src.height == 0 {
        return;
    }
    let x0 = x0.max(0);
    let y0 = y0.max(0);
    let x1 = x1.min(i64::from(src.width) - 1);
    let y1 = y1.min(i64::from(src.height) - 1);
    if x1 < x0 || y1 < y0 {
        return;
    }
    let run = (x1 - x0 + 1) as u32;
    let run = run.min(dst.width);
    for y in y0..=y1 {
        let dy = (y - y0) as u32;
        if dy >= dst.height {
            break;
        }
        for i in 0..run {
            let dx = i;
            let Some(px) = src.pixel((x0 as u32) + i, y as u32) else {
                break;
            };
            dst.set_pixel(dx, dy, px);
        }
    }
}

/// 组源:按当前子层状态重绘组的离屏内容(EVID-T-12 的“命令执行时当前状态”)。
fn compose_group(
    textures: &mut TextureStore,
    snapshots: &Snapshots,
    frame: &CompositorFrame,
    id: &LayerId,
    st: &mut RenderState,
) -> Result<Surface, String> {
    if let Some(surface) = st.offscreens.get(id) {
        return Ok(surface.clone());
    }
    let pass = frame
        .passes
        .iter()
        .find(|pass| matches!(&pass.target, PassTarget::Offscreen(target) if target == id))
        .ok_or_else(|| "gfx.copy.source.offscreen_missing".to_string())?;
    let (width, height) = group_surface_size(frame, id);
    if width == 0 || height == 0 {
        return Err("group.size.zero".to_string());
    }
    let clear = pass.clear.unwrap_or(Clear::explicit([0, 0, 0, 0]));
    let mut surface = Surface::new(width, height, clear.rgba);
    // 这是为 COPY 复用组内容而提前重绘(EVID-T-12:复制取命令执行时状态),
    // 不是 CompositorFrame 的独立 pass:该组的 offscreen pass 稍后仍会被
    // 正常绘制并计一次数。若把这次重绘也计入报告,soft 的 ops_total 会比
    // wgpu 多出组内 op 数(两端报告必须逐字段一致)。因此只保留像素/覆盖
    // 纹理副作用,报告计数与跳过聚合回滚。
    let report = st.report.clone();
    let skipped = st.skipped.clone();
    for op in &pass.ops {
        draw_op(textures, snapshots, frame, op, &mut surface, st);
    }
    st.report = report;
    st.skipped = skipped;
    Ok(surface)
}

/// 图层源:当前基图(帧内加工副本优先)。
///
/// 源层可能已被 `CGEND` 删除(`DIV-GFX-053`):此时回退到 `CopyOp` 在命令
/// 时刻记录的 `source_texture` 重放。
fn layer_surface(
    textures: &mut TextureStore,
    snapshots: &Snapshots,
    frame: &CompositorFrame,
    id: &LayerId,
    st: &mut RenderState,
    fallback: Option<&CopyOp>,
) -> Result<Surface, String> {
    // 文本覆盖纹理不作为 COPY 源(DIV-GFX-058):快照面 = 基图(+图像加工)
    // + `copy.texts` 在命令时刻冻结的文本,不带帧末实时文本。
    if let Some(texture) = st
        .texture_overrides
        .get(id)
        .filter(|_| !st.text_override_ids.contains(id))
    {
        return Ok(Surface::from_texture(texture));
    }
    let layer = frame.layers.iter().find(|layer| &layer.id == id);
    let texture = match layer {
        Some(layer) => &layer.texture,
        None => fallback
            .and_then(|copy| copy.source_texture.as_ref())
            .ok_or_else(|| "gfx.copy.source.gone".to_string())?,
    };
    match texture {
        TextureRef::Asset(key) => textures
            .resolve(key)
            .map(Surface::from_texture)
            .map_err(|diag| diag.code.to_string()),
        TextureRef::SolidBlack1x1 => Ok(Surface::from_texture(&Texture::solid_black_1x1())),
        TextureRef::Snapshot(copy_id) => snapshots
            .get(copy_id)
            .cloned()
            .ok_or_else(|| "gfx.copy.source.snapshot_missing".to_string()),
        TextureRef::Offscreen(group) => compose_group(textures, snapshots, frame, group, st),
        TextureRef::None => Err("gfx.copy.source.texture_none".to_string()),
    }
}

/// 取目标图层的基图副本(资产/1×1 黑/快照)。
fn base_texture(
    textures: &mut TextureStore,
    snapshots: &Snapshots,
    frame: &CompositorFrame,
    target: &LayerId,
) -> Result<Texture, String> {
    let Some(layer) = frame.layers.iter().find(|layer| &layer.id == target) else {
        return Err("gfx.imageop.target.unsupported".to_string());
    };
    match &layer.texture {
        TextureRef::Asset(key) => textures
            .resolve(key)
            .map(|texture| Texture {
                width: texture.width,
                height: texture.height,
                rgba: texture.rgba.clone(),
            })
            .map_err(|diag| diag.code.to_string()),
        TextureRef::SolidBlack1x1 => Ok(Texture::solid_black_1x1()),
        TextureRef::Snapshot(copy_id) => snapshots
            .get(copy_id)
            .map(|surface| Texture {
                width: surface.width,
                height: surface.height,
                rgba: surface.rgba.clone(),
            })
            .ok_or_else(|| "gfx.imageop.copy_source.unsupported".to_string()),
        TextureRef::Offscreen(_) => Err("gfx.imageop.target.group".to_string()),
        TextureRef::None => Err("gfx.imageop.texture.none".to_string()),
    }
}

fn draw_op(
    textures: &mut TextureStore,
    snapshots: &Snapshots,
    frame: &CompositorFrame,
    op: &Op,
    dst: &mut Surface,
    st: &mut RenderState,
) {
    st.report.ops_total += 1;
    match op {
        Op::Layer(layer) => {
            apply_layer_text(textures, snapshots, frame, layer, st);
            draw_layer(textures, snapshots, frame, op, layer, dst, st)
        }
        Op::Group(group) => {
            if !group.layer.visible {
                st.skip_op(op, "invisible");
                return;
            }
            if let Some(why) = status_reason(op) {
                st.skip_op(op, why);
                return;
            }
            let Some(mode) = supported_blend(group.layer.blend) else {
                st.skip_op(op, format!("blend.{}", group.layer.blend.name()));
                return;
            };
            let Some(surface) = st.offscreens.get(&group.offscreen) else {
                st.skip_op(op, "offscreen.missing");
                return;
            };
            let (width, height) = (surface.width, surface.height);
            if width == 0 || height == 0 {
                st.skip_op(op, "group.size.zero");
                return;
            }
            let (x, y) = (group.layer.transform.x, group.layer.transform.y);
            let alpha = layer_alpha(group.layer.alpha);
            let filter = frame.filter.mag;
            {
                let image = ImageRef::from_surface(surface);
                draw_image(dst, image, x, y, width, height, alpha, mode, filter, None);
            }
            st.report.ops_drawn += 1;
        }
        Op::Transition(transition) => {
            draw_transition(textures, snapshots, frame, op, transition, dst, st)
        }
        Op::Mask(_) => {
            let reason = status_reason(op).unwrap_or("mask.unsupported");
            st.skip_op(op, reason);
        }
        Op::Copy(_) => {
            let reason = status_reason(op).unwrap_or("copy.unsupported");
            st.skip_op(op, reason);
        }
        Op::Image(_) => {
            let reason = status_reason(op).unwrap_or("imageop.unsupported");
            st.skip_op(op, reason);
        }
    }
}

/// 把文本合成进图层基图的副本,供随后的常规图层绘制使用。
///
/// 化学等价性:引擎把文字写进图层像素,再按图层 `A/MODE` 合成;这里先生成
/// "基图+文本"的覆盖纹理,再走同一 `draw_image` 路径(EVID/论证见
/// `docs/divergences/gfx-text.md` DIV-GFX-055)。
fn apply_layer_text(
    textures: &mut TextureStore,
    snapshots: &Snapshots,
    frame: &CompositorFrame,
    layer: &Layer,
    st: &mut RenderState,
) {
    let draws: Vec<TextDraw> = frame
        .text_draws
        .iter()
        .filter(|d| d.target == layer.id)
        .cloned()
        .collect();
    let layout = frame
        .text_layout
        .as_ref()
        .filter(|l| l.target.as_ref() == Some(&layer.id));
    if draws.is_empty() && layout.is_none() {
        return;
    }
    if !layer.visible {
        return;
    }
    // 同一层一帧内可能被绘制多次(COPY 的按需组重绘 + 自身 pass);
    // 覆盖纹理已含文本,重复合成会在 DRAWMODE=1 下加深。
    if !st.text_composed.insert(layer.id.clone()) {
        return;
    }
    let base = match st.texture_overrides.get(&layer.id) {
        Some(base) => base.clone(),
        None => match layer_base(textures, snapshots, layer) {
            Ok(t) => Rc::new(t),
            Err(reason) => {
                let why = format!("text.layer_base: {reason}");
                if !st.report.text_diags.contains(&why) {
                    st.report.text_diags.push(why);
                    st.report.text_diags.sort();
                }
                return;
            }
        },
    };
    // 持久文本是烘焙语义:内容(revision)与基面都没变 ⇒ 直接复用上次合成
    // 结果,不再逐帧重烤(P8 性能;wgpu 的上传缓存随内容哈希一起命中)。
    let revision = frame.text_revisions.get(&layer.id).copied().unwrap_or(0);
    let override_hash = st
        .texture_overrides
        .get(&layer.id)
        .map(|t| texture_content_hash(t));
    let base_key = text_base_key(&layer.texture, override_hash);
    let (composed, stats) = st
        .text
        .compose_layer_cached(&layer.id, revision, base_key, &base, &draws, layout);
    merge_text_stats(&mut st.report, stats);
    if let Some(composed) = composed {
        st.texture_overrides.insert(layer.id.clone(), composed);
        st.text_override_ids.insert(layer.id.clone());
    }
}

fn merge_text_stats(report: &mut RenderReport, stats: TextPaintStats) {
    report.text_draws += stats.draws;
    report.text_glyphs += stats.glyphs;
    report.text_missing += stats.missing;
    for d in stats.diags {
        if !report.text_diags.contains(&d) {
            report.text_diags.push(d);
            report.text_diags.sort();
        }
    }
}

/// 双纹理过渡:RGB 取本层纹理,alpha = 纹理 α × 覆盖度 × 层 A。
///
/// 覆盖度的 mask 取自配对纹理的 G 通道(EVID-T-05);`T>=256` 为满。
fn draw_transition(
    textures: &mut TextureStore,
    snapshots: &Snapshots,
    frame: &CompositorFrame,
    op: &Op,
    transition: &yu_gfx::TransitionDraw,
    dst: &mut Surface,
    st: &mut RenderState,
) {
    let layer = &transition.layer;
    if !layer.visible {
        st.skip_op(op, "invisible");
        return;
    }
    if let Some(why) = status_reason(op) {
        st.skip_op(op, why);
        return;
    }
    let Some(mode) = supported_blend(layer.blend) else {
        st.skip_op(op, format!("blend.{}", layer.blend.name()));
        return;
    };
    // 配对缺失(不应到达;gfx 已在 op_for_layer 回退普通层)按单纹理绘制。
    let Some(to) = transition.to.as_ref() else {
        draw_layer(textures, snapshots, frame, op, layer, dst, st);
        return;
    };
    // 引擎取样的是图层**当前像素**(EVID-T-05):脚本可能先对配对层做
    // `CGACT[REVERSE]` 等加工,因此优先用该层的图像加工覆盖纹理。
    let from = match resolve_source_with_override(
        textures,
        snapshots,
        st,
        transition.from_layer.as_ref(),
        &transition.from,
    ) {
        Ok(image) => image,
        Err(reason) => {
            st.skip_op(op, reason);
            return;
        }
    };
    let mask = match resolve_source_with_override(
        textures,
        snapshots,
        st,
        transition.to_layer.as_ref(),
        to,
    ) {
        Ok(image) => image,
        Err(reason) => {
            st.skip_op(op, reason);
            return;
        }
    };
    let from_ref = from.as_image();
    let mask_ref = mask.as_image();
    let (width, height) = match layer.image_size() {
        Some(size) => (size.width, size.height),
        None => (from_ref.width, from_ref.height),
    };
    let filter = pick_filter(frame, from_ref.width, from_ref.height, width, height);
    let (x, y) = (layer.transform.x, layer.transform.y);
    // `_DXMODE≠0`:引擎 effect 的两个转场 pass(EVID-T-09/台账 §2)。
    // pass1(psC)=mask R 通道;pass0(psD)=交叉淡化。固定管线路径
    // (`dx_mode=0`)保持 G 通道拟合模型(J3 600 帧基线)。
    let hlsl_mask = frame.dx_mode != 0 && transition.ta >= 1;
    let crossfade = frame.dx_mode != 0 && transition.ta < 1;
    let alpha = if frame.dx_mode != 0 {
        // `YS_AlphaVal = A/256`;draw_image 的 alpha 走 u8/255 链,
        // 这里等价量化到 round(A*255/256)。
        ((f64::from(layer.alpha.clamp(0, 256)) * 255.0) / 256.0).round() as u8
    } else {
        layer_alpha(layer.alpha)
    };
    draw_image(
        dst,
        from_ref,
        x,
        y,
        width,
        height,
        alpha,
        mode,
        filter,
        Some(TransitionMask {
            image: mask_ref,
            fade: transition.fade,
            fade2: f64::from(transition.ta),
            complete: transition.complete(),
            hlsl_mask,
            crossfade,
        }),
    );
    st.report.ops_drawn += 1;
}

/// 一次过渡取样需要的图像(拥有型:资产解析的可变借用不能跨到绘制阶段)。
struct OwnedImage {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl OwnedImage {
    fn as_image(&self) -> ImageRef<'_> {
        ImageRef {
            width: self.width,
            height: self.height,
            rgba: &self.rgba,
        }
    }
}

/// 把 [`TextureRef`] 解析成可采样图像(帧内加工副本优先)。
///
/// 软后端是参考实现:资产/快照/离屏内容在这里复制一份,避免可变借用
/// (`TextureStore::resolve`)跨到绘制阶段。过渡 op 的基数很小。
/// 过渡取样:优先取图层加工后的覆盖纹理(EVID-J3-05),否则按 `TextureRef` 解析。
fn resolve_source_with_override(
    textures: &mut TextureStore,
    snapshots: &Snapshots,
    st: &RenderState,
    layer: Option<&LayerId>,
    texture: &TextureRef,
) -> Result<OwnedImage, String> {
    if let Some(id) = layer {
        if let Some(owned) = st.texture_overrides.get(id) {
            return Ok(OwnedImage {
                width: owned.width,
                height: owned.height,
                rgba: owned.rgba.clone(),
            });
        }
    }
    resolve_source(textures, snapshots, st, texture)
}

fn resolve_source(
    textures: &mut TextureStore,
    snapshots: &Snapshots,
    st: &RenderState,
    texture: &TextureRef,
) -> Result<OwnedImage, String> {
    match texture {
        TextureRef::Asset(key) => textures
            .resolve(key)
            .map(|texture| OwnedImage {
                width: texture.width,
                height: texture.height,
                rgba: texture.rgba.clone(),
            })
            .map_err(|diag| diag.code.to_string()),
        TextureRef::SolidBlack1x1 => Ok(OwnedImage {
            width: 1,
            height: 1,
            rgba: SOLID_BLACK.to_vec(),
        }),
        TextureRef::Snapshot(copy_id) => snapshots
            .get(copy_id)
            .map(|surface| OwnedImage {
                width: surface.width,
                height: surface.height,
                rgba: surface.rgba.clone(),
            })
            .ok_or_else(|| "snapshot.unsupported".to_string()),
        TextureRef::Offscreen(id) => st
            .offscreens
            .get(id)
            .map(|surface| OwnedImage {
                width: surface.width,
                height: surface.height,
                rgba: surface.rgba.clone(),
            })
            .ok_or_else(|| "offscreen.missing".to_string()),
        TextureRef::None => Err("texture.none".to_string()),
    }
}

fn draw_layer(
    textures: &mut TextureStore,
    snapshots: &Snapshots,
    frame: &CompositorFrame,
    op: &Op,
    layer: &Layer,
    dst: &mut Surface,
    st: &mut RenderState,
) {
    if !layer.visible {
        st.skip_op(op, "invisible");
        return;
    }
    if let Some(why) = status_reason(op) {
        st.skip_op(op, why);
        return;
    }
    if !layer.parts.is_empty() {
        st.skip_op(op, "gfx.parts.unsupported");
        return;
    }
    let Some(mode) = supported_blend(layer.blend) else {
        st.skip_op(op, format!("blend.{}", layer.blend.name()));
        return;
    };
    if let Some(texture) = st.texture_overrides.get(&layer.id) {
        let (width, height) = match layer.image_size() {
            Some(size) => (size.width, size.height),
            None => (texture.width, texture.height),
        };
        let filter = pick_filter(frame, texture.width, texture.height, width, height);
        let (x, y) = (layer.transform.x, layer.transform.y);
        {
            let image = ImageRef::from_texture(texture);
            draw_image(
                dst,
                image,
                x,
                y,
                width,
                height,
                layer_alpha(layer.alpha),
                mode,
                filter,
                None,
            );
        }
        st.report.ops_drawn += 1;
        return;
    }
    let (explicit_width, explicit_height) = match layer.image_size() {
        Some(size) => (Some(size.width), Some(size.height)),
        None => (None, None),
    };

    match &layer.texture {
        TextureRef::Asset(key) => {
            if explicit_width.is_some() || explicit_height.is_some() {
                // 资源图 + SX/SY 的缩放/裁剪未取证;gfx 已把层标
                // Unimplemented,这里再显式挡一次,防止上游变化。
                st.skip_op(op, "asset.size.unsupported");
                return;
            }
            match textures.resolve(key) {
                Ok(texture) => {
                    let (width, height) = (texture.width, texture.height);
                    let filter = pick_filter(frame, width, height, width, height);
                    let (x, y) = (layer.transform.x, layer.transform.y);
                    {
                        let image = ImageRef::from_texture(texture);
                        draw_image(
                            dst,
                            image,
                            x,
                            y,
                            width,
                            height,
                            layer_alpha(layer.alpha),
                            mode,
                            filter,
                            None,
                        );
                    }
                    st.report.ops_drawn += 1;
                }
                Err(diag) => st.skip_op(op, diag.code.to_string()),
            }
        }
        TextureRef::SolidBlack1x1 => {
            let width = explicit_width.unwrap_or(1);
            let height = explicit_height.unwrap_or(1);
            let filter = pick_filter(frame, 1, 1, width, height);
            let (x, y) = (layer.transform.x, layer.transform.y);
            {
                let image = ImageRef {
                    width: 1,
                    height: 1,
                    rgba: SOLID_BLACK,
                };
                draw_image(
                    dst,
                    image,
                    x,
                    y,
                    width,
                    height,
                    layer_alpha(layer.alpha),
                    mode,
                    filter,
                    None,
                );
            }
            st.report.ops_drawn += 1;
        }
        TextureRef::Offscreen(id) => {
            let Some(surface) = st.offscreens.get(id) else {
                st.skip_op(op, "offscreen.missing");
                return;
            };
            let (width, height) = (surface.width, surface.height);
            let (x, y) = (layer.transform.x, layer.transform.y);
            {
                let image = ImageRef::from_surface(surface);
                draw_image(
                    dst,
                    image,
                    x,
                    y,
                    width,
                    height,
                    layer_alpha(layer.alpha),
                    mode,
                    frame.filter.mag,
                    None,
                );
            }
            st.report.ops_drawn += 1;
        }
        TextureRef::Snapshot(copy_id) => {
            let Some(surface) = snapshots.get(copy_id) else {
                st.skip_op(op, "snapshot.unsupported");
                return;
            };
            let (width, height) = match layer.image_size() {
                Some(size) => (size.width, size.height),
                None => (surface.width, surface.height),
            };
            let filter = pick_filter(frame, surface.width, surface.height, width, height);
            let (x, y) = (layer.transform.x, layer.transform.y);
            {
                let image = ImageRef::from_surface(surface);
                draw_image(
                    dst,
                    image,
                    x,
                    y,
                    width,
                    height,
                    layer_alpha(layer.alpha),
                    mode,
                    filter,
                    None,
                );
            }
            st.report.ops_drawn += 1;
        }
        TextureRef::None => st.skip_op(op, "texture.none"),
    }
}

/// 1×1 黑图(`FILE=""`;手册 `cg/cg.html`)。
const SOLID_BLACK: &[u8] = &[0, 0, 0, 255];

struct RenderState<'a> {
    offscreens: BTreeMap<LayerId, Surface>,
    /// 帧内图像加工后的图层纹理(基图副本);只影响本帧。
    texture_overrides: BTreeMap<LayerId, Rc<Texture>>,
    /// `texture_overrides` 中由文本合成产生的层(DIV-GFX-058):COPY 取源面
    /// 时必须跳过 —— 快照只携带命令时刻冻结的 `copy.texts`,不带帧末实时文本。
    text_override_ids: BTreeSet<LayerId>,
    /// 本帧已做过文本合成的层。同一层可能在一帧内被绘制多次(COPY 的按需
    /// 组重绘 + 它自己的离屏/屏幕 pass);文本只合成一次,避免 `DRAWMODE=1`
    /// 的重复合成加深。
    text_composed: BTreeSet<LayerId>,
    skipped: BTreeMap<(&'static str, String, String), u32>,
    report: RenderReport,
    /// 文本栅格化/合成(两条文本路径共用)。
    text: &'a mut TextComposer,
}

impl<'a> RenderState<'a> {
    fn new(frame: &CompositorFrame, text: &'a mut TextComposer) -> Self {
        let visible_layers = frame.layers.iter().filter(|l| l.visible).count();
        Self {
            offscreens: BTreeMap::new(),
            texture_overrides: BTreeMap::new(),
            text_override_ids: BTreeSet::new(),
            text_composed: BTreeSet::new(),
            skipped: BTreeMap::new(),
            text,
            report: RenderReport {
                frame: frame.frame,
                viewport_width: frame.viewport.width,
                viewport_height: frame.viewport.height,
                layers: frame.layers.len(),
                visible_layers,
                passes: 0,
                offscreen_passes: 0,
                ops_total: 0,
                ops_drawn: 0,
                skipped_ops_total: 0,
                skipped: Vec::new(),
                missing_assets: Vec::new(),
                skipped_text: frame.text.iter().filter(|t| t.flags.is_none()).count(),
                text_draws: 0,
                text_glyphs: 0,
                text_missing: 0,
                text_diags: Vec::new(),
                filter_mag: frame.filter.mag.as_str(),
                filter_min: frame.filter.min.as_str(),
                filter_mip: frame.filter.mip.as_str(),
                filter_source: frame.filter.source.as_str(),
                color_space: frame.color_space.as_str(),
            },
        }
    }

    fn skip_op(&mut self, op: &Op, reason: impl Into<String>) {
        self.report.skipped_ops_total += 1;
        let key = (op_kind(op), reason.into(), op_target(op));
        *self.skipped.entry(key).or_insert(0) += 1;
    }

    fn finish(mut self) -> RenderReport {
        self.report.skipped = self
            .skipped
            .into_iter()
            .map(|((kind, reason, target), count)| SkippedOp {
                kind,
                reason,
                target,
                count,
            })
            .collect();
        self.report
    }
}

/// 预测 `frame_has_drawable_output`:按当前后端能力判断屏幕 pass 是否至少
/// 有一个可执行绘制操作。用于 headless 循环选择“首个可绘制帧”,不做像素工作。
///
/// 文本也算可见输出:自动文本层(`text_layout`)或图层文本(`text_draws`)
/// 非空时,即使没有任何可执行图层 op,画面也不是黑屏。
pub fn frame_has_drawable_output(frame: &CompositorFrame) -> bool {
    if frame.text_layout.is_some() || !frame.text_draws.is_empty() {
        return true;
    }
    let mut offscreen_drawable: BTreeMap<&LayerId, bool> = BTreeMap::new();
    for pass in &frame.passes {
        match &pass.target {
            PassTarget::Offscreen(id) => {
                let drawable = pass
                    .ops
                    .iter()
                    .any(|op| op_predict_drawable(op, &offscreen_drawable));
                offscreen_drawable.insert(id, drawable);
            }
            PassTarget::Screen => {
                if pass
                    .ops
                    .iter()
                    .any(|op| op_predict_drawable(op, &offscreen_drawable))
                {
                    return true;
                }
            }
            PassTarget::Texture(_) => {}
        }
    }
    false
}

fn op_predict_drawable(op: &Op, offscreens: &BTreeMap<&LayerId, bool>) -> bool {
    if !matches!(op.status(), OpStatus::Executable) {
        return false;
    }
    match op {
        Op::Layer(layer) => layer_predict_drawable(layer, offscreens),
        Op::Group(group) => {
            group.layer.visible
                && group.layer.parts.is_empty()
                && supported_blend(group.layer.blend).is_some()
                && offscreens.get(&group.offscreen).copied().unwrap_or(false)
        }
        // 过渡把本层纹理画出来;配对缺失时 gfx 已回退普通层。
        Op::Transition(transition) => {
            transition.layer.visible
                && transition.layer.parts.is_empty()
                && supported_blend(transition.layer.blend).is_some()
                && matches!(
                    transition.from,
                    TextureRef::Asset(_) | TextureRef::SolidBlack1x1
                )
        }
        // Copy/Image 在 Texture pass 内执行,不直接产出屏幕像素。
        Op::Mask(_) | Op::Copy(_) | Op::Image(_) => false,
    }
}

fn layer_predict_drawable(layer: &Layer, offscreens: &BTreeMap<&LayerId, bool>) -> bool {
    if !layer.visible || !layer.parts.is_empty() || supported_blend(layer.blend).is_none() {
        return false;
    }
    match &layer.texture {
        TextureRef::Asset(_) => layer.image_size().is_none(),
        TextureRef::SolidBlack1x1 => true,
        TextureRef::Offscreen(id) => offscreens.get(id).copied().unwrap_or(false),
        TextureRef::Snapshot(_) | TextureRef::None => false,
    }
}

fn group_surface_size(frame: &CompositorFrame, id: &LayerId) -> (u32, u32) {
    frame
        .layers
        .iter()
        .find(|layer| &layer.id == id)
        .and_then(|layer| layer.group_size())
        .map_or((frame.viewport.width, frame.viewport.height), |size| {
            (size.width, size.height)
        })
}

fn status_reason(op: &Op) -> Option<&'static str> {
    match op.status() {
        OpStatus::Executable => None,
        OpStatus::Unimplemented(why) => Some(why),
    }
}

fn supported_blend(mode: BlendMode) -> Option<BlendMode> {
    match mode {
        // 状态表已取证的固定管线模式(见 `docs/render-parity.md` §5.2);
        // 4/8/10..18 依赖未定位的 HLSL 本体,继续跳过。
        BlendMode::Normal
        | BlendMode::Add
        | BlendMode::Subtract
        | BlendMode::Multiply
        | BlendMode::Screen
        | BlendMode::Lighten
        | BlendMode::Darken
        | BlendMode::Exclusion => Some(mode),
        _ => None,
    }
}

fn layer_alpha(alpha: i16) -> u8 {
    alpha.clamp(0, 255) as u8
}

fn pick_filter(
    frame: &CompositorFrame,
    source_width: u32,
    source_height: u32,
    dest_width: u32,
    dest_height: u32,
) -> FilterMode {
    if dest_width > source_width || dest_height > source_height {
        frame.filter.mag
    } else {
        frame.filter.min
    }
}

fn op_kind(op: &Op) -> &'static str {
    match op {
        Op::Layer(_) => "layer",
        Op::Group(_) => "group",
        Op::Transition(_) => "transition",
        Op::Mask(_) => "mask",
        Op::Copy(_) => "copy",
        Op::Image(_) => "image",
    }
}

fn op_target(op: &Op) -> String {
    match op {
        Op::Layer(layer) => layer.id.display(),
        Op::Group(group) => group.layer.id.display(),
        Op::Transition(transition) => transition.layer.id.display(),
        Op::Mask(mask) => mask.layer.id.display(),
        Op::Copy(copy) => copy.dest.display(),
        Op::Image(image) => image.target.as_ref().unwrap_or(&image.source).display(),
    }
}

#[derive(Clone, Copy)]
struct ImageRef<'a> {
    width: u32,
    height: u32,
    rgba: &'a [u8],
}

impl<'a> ImageRef<'a> {
    fn from_texture(texture: &'a Texture) -> Self {
        Self {
            width: texture.width,
            height: texture.height,
            rgba: &texture.rgba,
        }
    }

    fn from_surface(surface: &'a Surface) -> Self {
        Self {
            width: surface.width,
            height: surface.height,
            rgba: &surface.rgba,
        }
    }

    fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y as usize) * (self.width as usize) + x as usize) * 4;
        [
            self.rgba[i],
            self.rgba[i + 1],
            self.rgba[i + 2],
            self.rgba[i + 3],
        ]
    }
}

#[derive(Clone, Copy)]
struct TransitionMask<'a> {
    /// 配对纹理。固定管线用 G 通道(EVID-T-05);psC 用 R 通道。
    image: ImageRef<'a>,
    /// `YS_FadeVal`。
    fade: f64,
    /// `YS_FadeVal2`(HLSL psC 的 TA;固定管线不用)。
    fade2: f64,
    /// `T >= 256`。
    complete: bool,
    /// `_DXMODE≠0`:psC(mask R)路径。
    hlsl_mask: bool,
    /// `_DXMODE≠0`:psD(交叉淡化)路径。
    crossfade: bool,
}

#[allow(clippy::too_many_arguments)]
fn draw_image(
    dst: &mut Surface,
    image: ImageRef<'_>,
    x: i64,
    y: i64,
    dest_width: u32,
    dest_height: u32,
    alpha: u8,
    blend: BlendMode,
    filter: FilterMode,
    transition: Option<TransitionMask<'_>>,
) {
    if dest_width == 0
        || dest_height == 0
        || image.width == 0
        || image.height == 0
        || dst.width == 0
        || dst.height == 0
    {
        return;
    }
    let clip_x0 = x.max(0);
    let clip_y0 = y.max(0);
    let clip_x1 = x
        .saturating_add(i64::from(dest_width))
        .min(i64::from(dst.width));
    let clip_y1 = y
        .saturating_add(i64::from(dest_height))
        .min(i64::from(dst.height));
    if clip_x1 <= clip_x0 || clip_y1 <= clip_y0 {
        return;
    }
    let direct = dest_width == image.width && dest_height == image.height;
    // psC/psD 由像素着色器接管源色;固定管线才做 stage0/stage1 归约。
    let shader_stage = transition.is_some_and(|mask| mask.hlsl_mask || mask.crossfade);
    for py in clip_y0..clip_y1 {
        for px in clip_x0..clip_x1 {
            // i128: X/Y 是脚本 i64,极端值下 `px - x` 也不能溢出。
            let dx = (i128::from(px) - i128::from(x)) as u32;
            let dy = (i128::from(py) - i128::from(y)) as u32;
            let mut src = if direct {
                image.pixel(dx, dy)
            } else {
                sample(image, dx, dy, dest_width, dest_height, filter)
            };
            let layer_a = alpha;
            let alpha = match transition {
                Some(mask) => {
                    let mask_px =
                        if dest_width == mask.image.width && dest_height == mask.image.height {
                            mask.image.pixel(dx, dy)
                        } else {
                            sample(mask.image, dx, dy, dest_width, dest_height, filter)
                        };
                    if mask.crossfade {
                        // pass0(psD):`oC0 = tex1*(1-FadeVal) + tex2*FadeVal`,
                        // 再乘 `YS_AlphaVal`(由本函数的 alpha 承担)。
                        let f = mask.fade.clamp(0.0, 1.0);
                        let inv = 1.0 - f;
                        for c in 0..4 {
                            src[c] = (f64::from(src[c]) * inv + f64::from(mask_px[c]) * f)
                                .round()
                                .clamp(0.0, 255.0) as u8;
                        }
                        alpha
                    } else {
                        let coverage = if mask.hlsl_mask {
                            // pass1(psC):mask = 配对纹理 **R**,
                            // `saturate((FadeVal - 0.5*tex2.r) * FadeVal2)`。
                            let c = (mask.fade - 0.5 * f64::from(mask_px[0]) / 255.0)
                                .mul_add(mask.fade2, 0.0)
                                .clamp(0.0, 1.0);
                            (c * 255.0).round().clamp(0.0, 255.0) as u8
                        } else {
                            transition_coverage(mask_px[1], mask.fade, mask.complete)
                        };
                        mul255(alpha, coverage)
                    }
                }
                None => alpha,
            };
            let src_alpha = mul255(src[3], alpha);
            let dst_x = px as u32;
            let dst_y = py as u32;
            let dst_px = dst
                .pixel(dst_x, dst_y)
                .expect("clipped destination is inside surface");
            let out = if shader_stage {
                blend_pixel_shader(dst_px, src, layer_a, src_alpha, blend)
            } else {
                blend_pixel(dst_px, src, layer_a, src_alpha, blend)
            };
            dst.set_pixel(dst_x, dst_y, out);
        }
    }
}

fn sample(
    image: ImageRef<'_>,
    dx: u32,
    dy: u32,
    dest_width: u32,
    dest_height: u32,
    filter: FilterMode,
) -> [u8; 4] {
    match filter {
        FilterMode::Point => {
            let sx = (i64::from(dx) * 2 + 1).saturating_mul(i64::from(image.width))
                / (i64::from(dest_width) * 2);
            let sy = (i64::from(dy) * 2 + 1).saturating_mul(i64::from(image.height))
                / (i64::from(dest_height) * 2);
            image.pixel(
                (sx as u32).min(image.width - 1),
                (sy as u32).min(image.height - 1),
            )
        }
        FilterMode::Linear | FilterMode::Anisotropic => {
            let (x0, fx) = axis_sample(dx, dest_width, image.width);
            let (y0, fy) = axis_sample(dy, dest_height, image.height);
            let x1 = (x0 + 1).min(image.width - 1);
            let y1 = (y0 + 1).min(image.height - 1);
            let c00 = image.pixel(x0, y0);
            let c10 = image.pixel(x1, y0);
            let c01 = image.pixel(x0, y1);
            let c11 = image.pixel(x1, y1);
            let fx = u64::from(fx);
            let fy = u64::from(fy);
            let w00 = (65536 - fx) * (65536 - fy);
            let w10 = fx * (65536 - fy);
            let w01 = (65536 - fx) * fy;
            let w11 = fx * fy;
            let mix = |a: u8, b: u8, c: u8, d: u8| -> u8 {
                let sum = u64::from(a) * w00
                    + u64::from(b) * w10
                    + u64::from(c) * w01
                    + u64::from(d) * w11
                    + (1 << 31);
                (sum >> 32).min(255) as u8
            };
            [
                mix(c00[0], c10[0], c01[0], c11[0]),
                mix(c00[1], c10[1], c01[1], c11[1]),
                mix(c00[2], c10[2], c01[2], c11[2]),
                mix(c00[3], c10[3], c01[3], c11[3]),
            ]
        }
    }
}

/// 一维 16.16 采样:返回(左/上索引, 小数部分 0..65535)。
fn axis_sample(d: u32, dest_len: u32, source_len: u32) -> (u32, u32) {
    let t16 = (i64::from(d) * 2 + 1)
        .saturating_mul(i64::from(source_len))
        .saturating_mul(32768)
        / i64::from(dest_len)
        - 32768;
    let t16 = t16.max(0);
    let index = (t16 >> 16) as u32;
    if index >= source_len {
        (source_len - 1, 0)
    } else {
        (index, (t16 & 0xffff) as u32)
    }
}

fn mul255(value: u8, alpha: u8) -> u8 {
    ((u32::from(value) * u32::from(alpha) + 127) / 255) as u8
}

/// 直通 alpha 的 D3D9 混合(`MODE 0/1/2/3/5/6/7/9`)。
///
/// 状态来源:官方 0.495 `yu-ris.exe`(sha256 `4f5eefa3…`)`FUN_00414ba4`
/// (`0x414ba4`,REA/Ghidra 12.1.4):MODE 0 设 SRCBLEND=5(SRCALPHA)/
/// DESTBLEND=6(INVSRCALPHA)/BLENDOP=1(ADD),MODE 1 为 5/2(ONE)/1,
/// MODE 2 为 5/2/3(REVSUBTRACT);`FUN_00462e40` 只设置
/// `D3DRS_ALPHABLENDENABLE`(0x1B);反编译全树没有
/// `D3DRS_SEPARATEALPHABLENDENABLE`(0xCE)写入,所以 alpha 通道与 RGB
/// 使用同一组因子,alpha 源项是 `a²`(不是 `a`)。
///
/// MODE 3/5/6/7/9 的**两阶段纹理运算**(2026-10-21 复核):
/// stage0 的 `MOD(D,T)` 使用顶点 DIFFUSE,引擎在 `[ESP+0x2e8]` 写入
/// `A` 灰度(`FUN_00414ba4` asm `0x414d09..0x414d21`);stage1 的
/// `BLENDCURRENTALPHA` 配合白纹理(`DAT_00801d98`,MODE 3)/黑纹理
/// (`DAT_00801d9c`,MODE 5)/白 TFACTOR(MODE 7)把源色归约成:
/// - 0/1: `T`
/// - 2/5/6/9: `T·A`
/// - 3/7: `T·A + (1 − C.a)`
///
/// 因此这里先算 stage 源色,再套 D3D9 的 blend 因子。
///
/// 证据:`EVID-J-01`(`docs/fidelity-ledger-j.md`);D3D9 头
/// `D3DBLEND_SRCALPHA=5`、`D3DTOP_BLENDCURRENTALPHA=16`、
/// `D3DRS_SEPARATEALPHABLENDENABLE=206`;`DAT_00801d98/9c` 的错误串
/// “WHITETEXBUF/BLACKTEXBUF ERROR.”(`FUN_00462e40` 行 282-310)。
fn blend_pixel(
    dst: [u8; 4],
    src: [u8; 4],
    layer_alpha: u8,
    src_alpha: u8,
    mode: BlendMode,
) -> [u8; 4] {
    blend_pixel_with_stage(dst, src, layer_alpha, src_alpha, mode, false)
}

/// 引擎 HLSL 转场(psC/psD)的混合:像素着色器已接管源色,
/// 不再套固定管线 stage0/stage1 的 `MOD(D,T)`/`BCA` 归约。
fn blend_pixel_shader(
    dst: [u8; 4],
    src: [u8; 4],
    layer_alpha: u8,
    src_alpha: u8,
    mode: BlendMode,
) -> [u8; 4] {
    blend_pixel_with_stage(dst, src, layer_alpha, src_alpha, mode, true)
}

/// 固定管线的两阶段纹理归约(render-parity §5.2):返回 0..1 的源 RGB。
///
/// - `MODE 0/1`:stage0 `SELECTARG1(T)` = `T`;
/// - `MODE 2/5/6/9`:stage0 `MOD(D,T)` = `T·A`(顶点 DIFFUSE 的 A 灰度);
/// - `MODE 3/7`:stage1 `BLENDCURRENTALPHA` 白/黑纹理再混一次
///   `T·A + (1 − C.a)`;
/// - `shader_stage`(psC/psD)由像素着色器直接给出源色,不再归约。
///
/// 单独成函数是因为交叉检查测试(`blend_pixel_matches_generic_d3d9_equation`)
/// 要与 `blend_pixel` 复用同一份归约,避免“第二套手写公式”。
fn stage_reduce(
    src: [u8; 4],
    layer: f64,
    a: f64,
    mode: BlendMode,
    shader_stage: bool,
) -> (f64, f64, f64) {
    let tex = |i: usize| f64::from(src[i]) / 255.0;
    if shader_stage {
        return (tex(0), tex(1), tex(2));
    }
    match mode {
        BlendMode::Normal | BlendMode::Add => (tex(0), tex(1), tex(2)),
        BlendMode::Multiply | BlendMode::Darken => (
            tex(0) * layer + (1.0 - a),
            tex(1) * layer + (1.0 - a),
            tex(2) * layer + (1.0 - a),
        ),
        _ => (tex(0) * layer, tex(1) * layer, tex(2) * layer),
    }
}

fn blend_pixel_with_stage(
    dst: [u8; 4],
    src: [u8; 4],
    layer_alpha: u8,
    src_alpha: u8,
    mode: BlendMode,
    shader_stage: bool,
) -> [u8; 4] {
    // D3D9 在浮点混合单元里做 `src*factor + dst*factor`,再按 rgba8 unorm
    // 四舍五入写回。J3 起按同一模型用 f64 复算(而不是分项整数取整):
    // 分项 `mul255` 与 GPU 的“先求和再取整”在淡入淡出区间会差 1 LSB,
    // 导致 soft/wgpu 同帧哈希不同。微场景(EVID-J-01/02..05)的取值在
    // 两种模型下相同,见 600 帧逐帧对拍。
    let q = |v: f64| v.round().clamp(0.0, 255.0) as u8;
    // `a` = 片元 alpha(C.a = 纹理 alpha × 层 A),`layer` = 顶点 DIFFUSE 的
    // A 灰度(层 A,不含纹理 alpha)。两者只在 T.a≠1 时不同。
    let a = f64::from(src_alpha) / 255.0;
    let inv = 1.0 - a;
    let layer = f64::from(layer_alpha) / 255.0;
    // stage0/stage1 归约后的源色(0..1)。
    let (s0, s1, s2) = stage_reduce(src, layer, a, mode, shader_stage);
    // 先量化 stage 源色(与 GPU shader 的 `round(...*255)/255` 对齐)。
    let s = [
        f64::from(q(s0 * 255.0)),
        f64::from(q(s1 * 255.0)),
        f64::from(q(s2 * 255.0)),
    ];
    let d = |i: usize| f64::from(dst[i]);
    match mode {
        BlendMode::Normal => [
            q(s[0] * a + d(0) * inv),
            q(s[1] * a + d(1) * inv),
            q(s[2] * a + d(2) * inv),
            q(a * a * 255.0 + d(3) * inv),
        ],
        BlendMode::Add => [
            q(s[0] * a + d(0)),
            q(s[1] * a + d(1)),
            q(s[2] * a + d(2)),
            q(a * a * 255.0 + d(3)),
        ],
        BlendMode::Subtract => [
            q(d(0) - s[0] * a),
            q(d(1) - s[1] * a),
            q(d(2) - s[2] * a),
            q(d(3) - a * a * 255.0),
        ],
        BlendMode::Multiply => [
            q(d(0) * s[0] / 255.0),
            q(d(1) * s[1] / 255.0),
            q(d(2) * s[2] / 255.0),
            q(d(3) * a),
        ],
        BlendMode::Screen => [
            q(s[0] + d(0) - s[0] * d(0) / 255.0),
            q(s[1] + d(1) - s[1] * d(1) / 255.0),
            q(s[2] + d(2) - s[2] * d(2) / 255.0),
            q(a * (1.0 - d(3) / 255.0) * 255.0 + d(3)),
        ],
        BlendMode::Lighten => [
            q(s[0].max(d(0))),
            q(s[1].max(d(1))),
            q(s[2].max(d(2))),
            q((a * 255.0).max(d(3))),
        ],
        BlendMode::Darken => [
            q(s[0].min(d(0))),
            q(s[1].min(d(1))),
            q(s[2].min(d(2))),
            q((a * 255.0).min(d(3))),
        ],
        BlendMode::Exclusion => [
            q(s[0] + d(0) - 2.0 * s[0] * d(0) / 255.0),
            q(s[1] + d(1) - 2.0 * s[1] * d(1) / 255.0),
            q(s[2] + d(2) - 2.0 * s[2] * d(2) / 255.0),
            q(a * 255.0 * (1.0 - d(3) / 255.0) + d(3) * (1.0 - a)),
        ],
        _ => dst,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blend_normal_uses_source_alpha_squared_for_alpha_channel() {
        // RGB 用 As;alpha 同因子 => As² + Ad·(1−As)。
        let out = blend_pixel(
            [0, 0, 0, 255],
            [255, 255, 255, 255],
            128,
            128,
            BlendMode::Normal,
        );
        assert_eq!(out, [128, 128, 128, 191]);
        let over_transparent = blend_pixel(
            [0, 0, 0, 0],
            [255, 255, 255, 255],
            128,
            128,
            BlendMode::Normal,
        );
        assert_eq!(over_transparent, [128, 128, 128, 64]);
    }

    #[test]
    fn blend_add_and_subtract() {
        assert_eq!(
            blend_pixel(
                [10, 20, 30, 255],
                [100, 100, 100, 255],
                128,
                128,
                BlendMode::Add
            ),
            [60, 70, 80, 255]
        );
        assert_eq!(
            blend_pixel(
                [100, 100, 100, 255],
                [200, 200, 200, 255],
                128,
                128,
                BlendMode::Subtract
            ),
            // stage0 = MOD(D,T) = T·A;blend = dst − stage·C.a = 100 − 200·A²,
            // A=128/255 => 100 − 50.4 = 49.6 -> 50;alpha 255 − A² = 191。
            [50, 50, 50, 191]
        );
    }

    #[test]
    fn blend_multiply_screen_lighten_darken_exclusion_opaque() {
        let dst = [100, 150, 200, 255];
        let src = [200, 100, 50, 255];
        assert_eq!(
            blend_pixel(dst, src, 255, 255, BlendMode::Multiply),
            [78, 59, 39, 255]
        );
        assert_eq!(
            blend_pixel(dst, src, 255, 255, BlendMode::Screen),
            [222, 191, 211, 255]
        );
        assert_eq!(
            blend_pixel(dst, src, 255, 255, BlendMode::Lighten),
            [200, 150, 200, 255]
        );
        assert_eq!(
            blend_pixel(dst, src, 255, 255, BlendMode::Darken),
            [100, 100, 50, 255]
        );
        assert_eq!(
            blend_pixel(dst, src, 255, 255, BlendMode::Exclusion),
            // D3D9 对此模式用 INVDSTCOLOR/INVSRCCOLOR 同因子,alpha 也被
            // `1−src` 乘:源 alpha=1 => dst.a×0 = 0(状态表语义,不是笔误)。
            [143, 132, 172, 0]
        );
    }

    #[test]
    fn blend_multiply_uses_stage1_white_mix_for_partial_alpha() {
        // A=128:stage1 白混 = T·A + (1−C.a) ≈ 0.89;
        // blend = dst·stage/255 ≈ 89(向白色中性点淡化,不是变黑)。
        assert_eq!(
            blend_pixel(
                [100, 100, 100, 255],
                [200, 200, 200, 255],
                128,
                128,
                BlendMode::Multiply
            ),
            [89, 89, 89, 128]
        );
    }

    #[test]
    fn alpha_clamps_to_255() {
        assert_eq!(layer_alpha(256), 255);
        assert_eq!(layer_alpha(-3), 0);
        assert_eq!(layer_alpha(128), 128);
    }

    #[test]
    fn point_sample_uses_pixel_centers() {
        let tex = Texture {
            width: 2,
            height: 1,
            rgba: vec![255, 0, 0, 255, 0, 255, 0, 255],
        };
        let image = ImageRef::from_texture(&tex);
        let sampled: Vec<u8> = (0..3)
            .map(|dx| sample(image, dx, 0, 3, 1, FilterMode::Point)[1])
            .collect();
        assert_eq!(sampled, vec![0, 255, 255]);
    }

    #[test]
    fn transition_coverage_matches_official_micro_scene() {
        // TA=128 的官方 T 扫描(docs/title-motion-ledger.md §2.1)。
        // 模型在三个中点上的官方残差 ≤ 8/255(DIV-GFX-045)。
        let fade = |t: i16| f64::from(t) / (256.0 / (256.0 / 128.0 + 128.0)) * (1.0 / 256.0);
        let near = |actual: u8, expected: i32| (i32::from(actual) - expected).abs() <= 8;
        assert!(near(transition_coverage(0, fade(64), false), 0));
        assert!(near(transition_coverage(128, fade(64), false), 0));
        assert!(near(transition_coverage(192, fade(64), false), 63));
        assert!(near(transition_coverage(128, fade(128), false), 127));
        assert!(near(transition_coverage(64, fade(192), false), 191));
        assert!(near(transition_coverage(0, fade(192), false), 0));
        assert_eq!(transition_coverage(0, fade(256), true), 255);
    }

    #[test]
    fn copy_region_blit_places_crop_at_origin() {
        let src = Surface {
            width: 4,
            height: 2,
            rgba: vec![
                1, 0, 0, 255, 1, 0, 0, 255, 2, 0, 0, 255, 2, 0, 0, 255, 1, 0, 0, 255, 1, 0, 0, 255,
                2, 0, 0, 255, 2, 0, 0, 255,
            ],
        };
        let mut dst = Surface::new(4, 2, [0, 0, 0, 0]);
        blit(&src, &mut dst, 2, 0, 3, 1);
        assert_eq!(dst.pixel(0, 0), Some([2, 0, 0, 255]));
        assert_eq!(dst.pixel(1, 0), Some([2, 0, 0, 255]));
        assert_eq!(dst.pixel(2, 0), Some([0, 0, 0, 0]));
    }

    #[test]
    fn shader_transition_bypasses_fixed_stage_reduction() {
        // psC/psD 的像素着色器直接给出源色;固定管线的 MOD(D,T) 不再生效。
        // MODE 2 + A=128:固定路径 = dst − (T·A)·A = 50;
        // HLSL 路径 = dst − T·A = 0。
        assert_eq!(
            blend_pixel(
                [100, 100, 100, 255],
                [200, 200, 200, 255],
                128,
                128,
                BlendMode::Subtract
            ),
            [50, 50, 50, 191]
        );
        assert_eq!(
            blend_pixel_shader(
                [100, 100, 100, 255],
                [200, 200, 200, 255],
                128,
                128,
                BlendMode::Subtract
            ),
            [0, 0, 0, 191]
        );
    }

    /// 网格输入:(dst, src, layer A)。覆盖全 0 / 全满 / 半透明 / 奇数低位,
    /// 以及 `Multiply` 白混、`Exclusion` 同因子 alpha 等分支的敏感点。
    const BLEND_GRID: &[([u8; 4], [u8; 4], u8)] = &[
        ([0, 0, 0, 0], [0, 0, 0, 0], 0),
        ([0, 0, 0, 0], [0, 0, 0, 0], 255),
        ([255, 255, 255, 255], [255, 255, 255, 255], 255),
        ([255, 255, 255, 255], [255, 255, 255, 255], 128),
        ([0, 0, 0, 255], [255, 255, 255, 255], 128),
        ([10, 20, 30, 40], [50, 60, 70, 80], 0),
        ([10, 20, 30, 40], [50, 60, 70, 80], 128),
        ([10, 20, 30, 40], [50, 60, 70, 80], 255),
        ([100, 150, 200, 255], [200, 100, 50, 255], 128),
        ([100, 150, 200, 255], [200, 100, 50, 255], 64),
        ([37, 41, 43, 211], [13, 244, 99, 7], 192),
        ([128, 128, 128, 128], [200, 200, 200, 255], 128),
    ];

    /// 测试用的通用 D3D9 混合方程:因子/运算全部读自
    /// [`yu_gfx::FixedPipelineState`](即 `fixed_state()` 的状态表),
    /// 不含任何按模式手写的公式。源操作数 = stage 归约后的 RGB + 片元
    /// alpha(EVID-J-01:oC0.a = `纹理α × 层A`,无 SEPARATEALPHA)。
    fn d3d9_equation_from_state(
        dst: [u8; 4],
        reduced_src: [u8; 3],
        src_alpha: u8,
        state: &yu_gfx::FixedPipelineState,
    ) -> [u8; 4] {
        let q = |v: f64| v.round().clamp(0.0, 255.0) as u8;
        let s: [f64; 4] = [
            f64::from(reduced_src[0]),
            f64::from(reduced_src[1]),
            f64::from(reduced_src[2]),
            f64::from(src_alpha),
        ];
        let d: [f64; 4] = dst.map(f64::from);
        // D3D9 的因子是**绝对的**:`SRC*` 永远读源操作数(oC0)、`DEST*`
        // 永远读目标像素,与因子写在 SRCBLEND 还是 DESTBLEND 无关
        // (`d3d9types.h` `D3DBLEND` 语义;MODE 0 的 `a²` 正是 DESTBLEND=
        // INVSRCALPHA 读回同一个源 alpha 的结果)。
        let factor = |kind: yu_gfx::BlendFactor| -> [f64; 4] {
            match kind {
                yu_gfx::BlendFactor::Zero => [0.0; 4],
                yu_gfx::BlendFactor::One => [1.0; 4],
                yu_gfx::BlendFactor::SrcColor => s.map(|c| c / 255.0),
                yu_gfx::BlendFactor::InvSrcColor => s.map(|c| 1.0 - c / 255.0),
                yu_gfx::BlendFactor::SrcAlpha => [s[3] / 255.0; 4],
                yu_gfx::BlendFactor::InvSrcAlpha => [1.0 - s[3] / 255.0; 4],
                yu_gfx::BlendFactor::DestColor => d.map(|c| c / 255.0),
                yu_gfx::BlendFactor::InvDestColor => d.map(|c| 1.0 - c / 255.0),
            }
        };
        let fs = factor(state.src_blend);
        let fd = factor(state.dst_blend);
        let mut out = [0u8; 4];
        for i in 0..4 {
            let v = match state.blend_op {
                yu_gfx::BlendOp::Add => s[i] * fs[i] + d[i] * fd[i],
                yu_gfx::BlendOp::Subtract => s[i] * fs[i] - d[i] * fd[i],
                yu_gfx::BlendOp::RevSubtract => d[i] * fd[i] - s[i] * fs[i],
                yu_gfx::BlendOp::Min => s[i].min(d[i]),
                yu_gfx::BlendOp::Max => s[i].max(d[i]),
            };
            out[i] = q(v);
        }
        out
    }

    /// P2-8(backend-soft-review):手写 blend 公式必须能从 `fixed_state()`
    /// 的 D3D9 状态**机械推出**——状态表与公式任何一侧单独改动都会在此失败,
    /// 而不是漂移到像素里。stage 归约复用 [`stage_reduce`](单一事实来源),
    /// 方程因子/运算由状态表通用求值。
    #[test]
    fn blend_pixel_matches_generic_d3d9_equation_from_state_table() {
        for mode in BlendMode::ALL {
            // 可执行集 = 有状态表的集合:执行门也由状态表驱动。
            assert_eq!(
                supported_blend(mode).is_some(),
                mode.fixed_state().is_some(),
                "mode {} 的可执行集与状态表不一致",
                mode.code()
            );
            let Some(state) = mode.fixed_state() else {
                continue;
            };
            for &(dst, src, layer_alpha) in BLEND_GRID {
                let src_alpha = mul255(src[3], layer_alpha);
                let got = blend_pixel(dst, src, layer_alpha, src_alpha, mode);
                let a = f64::from(src_alpha) / 255.0;
                let layer = f64::from(layer_alpha) / 255.0;
                let q = |v: f64| v.round().clamp(0.0, 255.0) as u8;
                let (s0, s1, s2) = stage_reduce(src, layer, a, mode, false);
                let reduced = [q(s0 * 255.0), q(s1 * 255.0), q(s2 * 255.0)];
                let expected = d3d9_equation_from_state(dst, reduced, src_alpha, &state);
                assert_eq!(
                    got,
                    expected,
                    "mode {} dst={dst:?} src={src:?} A={layer_alpha}",
                    mode.code()
                );
            }
        }
    }
}
