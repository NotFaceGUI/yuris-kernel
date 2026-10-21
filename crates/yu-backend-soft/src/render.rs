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
//! - `Point` 最近邻,`Linear/Anisotropic` 双线性;缩放采样全部用整数
//!   16.16 定点,避免浮点跨平台差异;
//! - 颜色空间按存储值直接混合,不做 sRGB↔线性转换。

use std::collections::BTreeMap;

use yu_gfx::{
    BlendMode, Clear, CompositorFrame, FilterMode, ImageAction, ImageOp, Layer, LayerId, Op,
    OpStatus, PassTarget, TextureRef,
};

use crate::imageop::apply_rect_paint;
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
    pub filter_mag: &'static str,
    pub filter_min: &'static str,
    pub filter_mip: &'static str,
    /// `engine_settings` / `placeholder`。
    pub filter_source: &'static str,
    pub color_space: &'static str,
}

/// `render_frame` 的输出。
#[derive(Clone, Debug)]
pub struct RenderOutput {
    pub surface: Surface,
    pub report: RenderReport,
}

/// 软件渲染器:持有纹理解析缓存与帧内离屏表面。
pub struct SoftRenderer {
    textures: TextureStore,
}

impl SoftRenderer {
    pub fn new(textures: TextureStore) -> Self {
        Self { textures }
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
        let mut st = RenderState::new(frame);
        let mut screen: Option<Surface> = None;

        for pass in &frame.passes {
            st.report.passes += 1;
            match &pass.target {
                PassTarget::Texture(target) => {
                    for op in &pass.ops {
                        st.report.ops_total += 1;
                        match op {
                            Op::Image(image) if op.status() == OpStatus::Executable => {
                                match self.apply_image_op(frame, target, image, &mut st) {
                                    Ok(()) => st.report.ops_drawn += 1,
                                    Err(reason) => st.skip_op(op, reason),
                                }
                            }
                            _ => {
                                let reason = match op.status() {
                                    OpStatus::Unimplemented(why) => why.to_string(),
                                    OpStatus::Executable => {
                                        "texture_pass.not_executed".to_string()
                                    }
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
                            st.skip_op(op, "group.size.zero");
                        }
                        continue;
                    }
                    let clear = pass.clear.unwrap_or(Clear::explicit([0, 0, 0, 0]));
                    let mut surface = Surface::new(width, height, clear.rgba);
                    for op in &pass.ops {
                        self.draw_op(frame, op, &mut surface, &mut st);
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
                        self.draw_op(frame, op, &mut surface, &mut st);
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
        let surface = screen.unwrap_or_else(|| {
            Surface::new(
                frame.viewport.width,
                frame.viewport.height,
                frame.clear.rgba,
            )
        });
        RenderOutput {
            surface,
            report: st.finish(),
        }
    }

    /// 执行一个可执行的 `Op::Image`(当前只有 RECTPAINT/2/3,EVID-J-02..05)。
    ///
    /// 目标图层的基图在帧内第一次被加工时拷贝出可变副本;后续加工继续
    /// 作用在同一副本上。图层绘制阶段优先使用该副本。
    fn apply_image_op(
        &mut self,
        frame: &CompositorFrame,
        pass_target: &LayerId,
        image: &ImageOp,
        st: &mut RenderState,
    ) -> Result<(), String> {
        let target = image.target.as_ref().unwrap_or(&image.source);
        if target != pass_target {
            return Err("gfx.imageop.target.mismatch".to_string());
        }
        let Some(set) = image.set.as_ref().and_then(|v| v.as_int()) else {
            return Err("gfx.imageop.set.missing".to_string());
        };
        let region = match (image.x, image.y, image.x2, image.y2) {
            (Some(x), Some(y), Some(x2), Some(y2)) => Some((x, y, x2, y2)),
            (None, None, None, None) => None,
            _ => return Err("gfx.imageop.region.partial".to_string()),
        };
        if !matches!(
            image.action,
            ImageAction::RectPaint | ImageAction::RectPaint2 | ImageAction::RectPaint3
        ) {
            return Err("gfx.imageop.unsupported".to_string());
        }
        let mut texture = match st.texture_overrides.remove(target) {
            Some(texture) => texture,
            None => self.base_texture(frame, target)?,
        };
        apply_rect_paint(&mut texture, image.action.clone(), set, region)?;
        st.texture_overrides.insert(target.clone(), texture);
        Ok(())
    }

    /// 取目标图层的基图副本。快照/离屏/无纹理的语义未取证,显式失败。
    fn base_texture(
        &mut self,
        frame: &CompositorFrame,
        target: &LayerId,
    ) -> Result<Texture, String> {
        let Some(layer) = frame.layers.iter().find(|layer| &layer.id == target) else {
            return Err("gfx.imageop.target.unsupported".to_string());
        };
        match &layer.texture {
            TextureRef::Asset(key) => self
                .textures
                .resolve(key)
                .map(|texture| Texture {
                    width: texture.width,
                    height: texture.height,
                    rgba: texture.rgba.clone(),
                })
                .map_err(|diag| diag.code.to_string()),
            TextureRef::SolidBlack1x1 => Ok(Texture::solid_black_1x1()),
            TextureRef::Snapshot(_) => Err("gfx.imageop.copy_source.unsupported".to_string()),
            TextureRef::Offscreen(_) => Err("gfx.imageop.target.group".to_string()),
            TextureRef::None => Err("gfx.imageop.texture.none".to_string()),
        }
    }

    fn draw_op(
        &mut self,
        frame: &CompositorFrame,
        op: &Op,
        dst: &mut Surface,
        st: &mut RenderState,
    ) {
        st.report.ops_total += 1;
        match op {
            Op::Layer(layer) => self.draw_layer(frame, op, layer, dst, st),
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
                    draw_image(dst, image, x, y, width, height, alpha, mode, filter);
                }
                st.report.ops_drawn += 1;
            }
            Op::Transition(_) => {
                let reason = status_reason(op).unwrap_or("transition.unsupported");
                st.skip_op(op, reason);
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

    fn draw_layer(
        &mut self,
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
                match self.textures.resolve(key) {
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
                            );
                        }
                        st.report.ops_drawn += 1;
                    }
                    Err(diag) => st.skip_op(op, diag.code.to_string()),
                }
            }
            TextureRef::SolidBlack1x1 => {
                let Ok(texture) = self.textures.resolve_ref(&layer.texture) else {
                    st.skip_op(op, "texture.none");
                    return;
                };
                let width = explicit_width.unwrap_or(texture.width);
                let height = explicit_height.unwrap_or(texture.height);
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
                    );
                }
                st.report.ops_drawn += 1;
            }
            TextureRef::Snapshot(_) => st.skip_op(op, "snapshot.unsupported"),
            TextureRef::None => st.skip_op(op, "texture.none"),
        }
    }
}

struct RenderState {
    offscreens: BTreeMap<LayerId, Surface>,
    /// 帧内 RECTPAINT 族加工后的图层纹理(基图副本);只影响本帧。
    texture_overrides: BTreeMap<LayerId, Texture>,
    skipped: BTreeMap<(&'static str, String, String), u32>,
    report: RenderReport,
}

impl RenderState {
    fn new(frame: &CompositorFrame) -> Self {
        let visible_layers = frame.layers.iter().filter(|l| l.visible).count();
        Self {
            offscreens: BTreeMap::new(),
            texture_overrides: BTreeMap::new(),
            skipped: BTreeMap::new(),
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
                skipped_text: frame.text.len(),
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
pub fn frame_has_drawable_output(frame: &CompositorFrame) -> bool {
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
                && offscreens
                    .get(&group.offscreen)
                    .copied()
                    .unwrap_or(false)
        }
        // Transition/Mask/Copy/Image 的像素语义当前后端不执行。
        Op::Transition(_) | Op::Mask(_) | Op::Copy(_) | Op::Image(_) => false,
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
        .map_or(
            (frame.viewport.width, frame.viewport.height),
            |size| (size.width, size.height),
        )
}

fn status_reason(op: &Op) -> Option<&'static str> {
    match op.status() {
        OpStatus::Executable => None,
        OpStatus::Unimplemented(why) => Some(why),
    }
}

fn supported_blend(mode: BlendMode) -> Option<BlendMode> {
    match mode {
        BlendMode::Normal | BlendMode::Add | BlendMode::Subtract => Some(mode),
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
        Op::Image(image) => image
            .target
            .as_ref()
            .unwrap_or(&image.source)
            .display(),
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
    for py in clip_y0..clip_y1 {
        for px in clip_x0..clip_x1 {
            // i128: X/Y 是脚本 i64,极端值下 `px - x` 也不能溢出。
            let dx = (i128::from(px) - i128::from(x)) as u32;
            let dy = (i128::from(py) - i128::from(y)) as u32;
            let src = if direct {
                image.pixel(dx, dy)
            } else {
                sample(image, dx, dy, dest_width, dest_height, filter)
            };
            let src_alpha = mul255(src[3], alpha);
            let dst_x = px as u32;
            let dst_y = py as u32;
            let dst_px = dst
                .pixel(dst_x, dst_y)
                .expect("clipped destination is inside surface");
            let out = blend_pixel(dst_px, src, src_alpha, blend);
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
            let sx = (i64::from(dx) * 2 + 1)
                .saturating_mul(i64::from(image.width))
                / (i64::from(dest_width) * 2);
            let sy = (i64::from(dy) * 2 + 1)
                .saturating_mul(i64::from(image.height))
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

/// 直通 alpha 的 D3D9 混合(`MODE 0/1/2`)。
///
/// 状态来源:官方 0.495 `yu-ris.exe`(sha256 `4f5eefa3…`)`FUN_00414ba4`
/// (`0x414ba4`,REA/Ghidra 12.1.4):MODE 0 设 SRCBLEND=5(SRCALPHA)/
/// DESTBLEND=6(INVSRCALPHA)/BLENDOP=1(ADD),MODE 1 为 5/2(ONE)/1,
/// MODE 2 为 5/2/3(REVSUBTRACT);`FUN_00462e40` 只设置
/// `D3DRS_ALPHABLENDENABLE`(0x1B);反编译全树没有
/// `D3DRS_SEPARATEALPHABLENDENABLE`(0xCE)写入,所以 alpha 通道与 RGB
/// 使用同一组因子,alpha 源项是 `a²`(不是 `a`)。
///
/// 证据:`EVID-J-01`(`docs/fidelity-ledger-j.md`);D3D9 头
/// `D3DBLEND_SRCALPHA=5` / `D3DRS_SEPARATEALPHABLENDENABLE=206`。
fn blend_pixel(dst: [u8; 4], src: [u8; 4], src_alpha: u8, mode: BlendMode) -> [u8; 4] {
    let a = u32::from(src_alpha);
    let a2 = u32::from(mul255(src_alpha, src_alpha));
    let inv = 255 - a;
    match mode {
        BlendMode::Normal => [
            ((u32::from(src[0]) * a + u32::from(dst[0]) * inv + 127) / 255) as u8,
            ((u32::from(src[1]) * a + u32::from(dst[1]) * inv + 127) / 255) as u8,
            ((u32::from(src[2]) * a + u32::from(dst[2]) * inv + 127) / 255) as u8,
            (a2 + (u32::from(dst[3]) * inv + 127) / 255).min(255) as u8,
        ],
        BlendMode::Add => [
            (u32::from(mul255(src[0], src_alpha)) + u32::from(dst[0])).min(255) as u8,
            (u32::from(mul255(src[1], src_alpha)) + u32::from(dst[1])).min(255) as u8,
            (u32::from(mul255(src[2], src_alpha)) + u32::from(dst[2])).min(255) as u8,
            (a2 + u32::from(dst[3])).min(255) as u8,
        ],
        BlendMode::Subtract => [
            dst[0].saturating_sub(mul255(src[0], src_alpha)),
            dst[1].saturating_sub(mul255(src[1], src_alpha)),
            dst[2].saturating_sub(mul255(src[2], src_alpha)),
            dst[3].saturating_sub(a2 as u8),
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
        let out = blend_pixel([0, 0, 0, 255], [255, 255, 255, 255], 128, BlendMode::Normal);
        assert_eq!(out, [128, 128, 128, 191]);
        let over_transparent = blend_pixel([0, 0, 0, 0], [255, 255, 255, 255], 128, BlendMode::Normal);
        assert_eq!(over_transparent, [128, 128, 128, 64]);
    }

    #[test]
    fn blend_add_and_subtract() {
        assert_eq!(
            blend_pixel([10, 20, 30, 255], [100, 100, 100, 255], 128, BlendMode::Add),
            [60, 70, 80, 255]
        );
        assert_eq!(
            blend_pixel(
                [100, 100, 100, 255],
                [200, 200, 200, 255],
                128,
                BlendMode::Subtract
            ),
            // RevSubtract 同时作用于 alpha: 255 - As² = 255 - 64 = 191。
            [0, 0, 0, 191]
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
}
