//! 文本栅格化与合成(两条引擎文本路径共用)。
//!
//! # 路径
//!
//! 1. **`CGACT[TEXT=1]`**(ERIS 对话文本的真实路径,手册 `cg/cgact.html`
//!    例 25/26):每个绘制项在目标图层局部坐标按 `FONT` 快照绘制字符串。
//!    字符步进用字形 advance(引擎逐字符 `TextOutA`,DIV-FONT-010);
//!    `SET` 选择合成重复次数(REA `FUN_004056b8` 的 1/9/3,`Likely`)。
//! 2. **自动文本层**(`TEXT[ID]` + `WORD`/`_`/`RETURNCODE`):`gfx` 已按
//!    `_TEXT_FLAG` 逐字 `'1'/'2'` 排好版,这里只负责把每个字符栅格化到位。
//!
//! # 证据
//!
//! - 单字覆盖 = GDI `CreateFontIndirectA`+`TextOutA` 8-bit mask
//!   (REA `FUN_00445421`/`FUN_004450c0`);本实现用 `yu-font` 的 swash 覆盖,
//!   不宣称与 GDI 字节级一致(DIV-FONT-002)。
//! - `DRAWMODE=0` 覆盖含透明像素,`1` 源合成(手册 `font/font.html`)。
//! - 基线 = 单元顶边 + 字体 ascent(近似 GDI 的 `lfHeight` 单元,DIV-GFX-056)。

use std::collections::BTreeMap;
use std::time::Instant;

use yu_font::cache::CacheLimits;
use yu_font::{cp932, AaMode, DrawMode, FontRenderer, FontSource, Gradation, Rgb, Rotation};
use yu_font::{StylePolicy, TextStyle as FontStyle};
use yu_gfx::{TextDraw, TextFontRequest, TextLayoutFrame};

use crate::texture::Texture;

/// 一次文本合成的统计(进 `RenderReport`)。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextPaintStats {
    /// 处理的绘制项(字符串)数。
    pub draws: usize,
    /// 成功栅格化并写入的字符数。
    pub glyphs: usize,
    /// 字体缺字/不可解码字符数。
    pub missing: usize,
    /// 去重后的失败原因(稳定排序)。
    pub diags: Vec<String>,
}

impl TextPaintStats {
    fn note(&mut self, what: &str) {
        if !self.diags.iter().any(|d| d == what) {
            self.diags.push(what.to_string());
            self.diags.sort();
        }
    }

    fn merge(&mut self, other: TextPaintStats) {
        self.draws += other.draws;
        self.glyphs += other.glyphs;
        self.missing += other.missing;
        for d in other.diags {
            self.note(&d);
        }
    }
}

/// 文本合成器:持有字体源/栅格器/字形缓存(懒加载系统字体)。
pub struct TextComposer {
    renderer: Option<FontRenderer>,
    /// 首次初始化失败原因(`None` = 尚未尝试)。
    failure: Option<String>,
    /// 字体身份 → face 解析缓存(键是请求的稳定显示形式)。
    faces: BTreeMap<String, Option<yu_font::FaceId>>,
    /// 首次系统字体枚举耗时(毫秒)。只在加载后的第一帧报告一次
    /// (`text.font_init`),用于解释“第一次打开テキスト页签”的尖峰。
    load_ms: Option<u128>,
}

impl Default for TextComposer {
    fn default() -> Self {
        Self::new()
    }
}

impl TextComposer {
    /// 懒加载:首次绘制时才枚举系统字体(标题/无文本帧零成本)。
    #[must_use]
    pub fn new() -> Self {
        Self {
            renderer: None,
            failure: None,
            faces: BTreeMap::new(),
            load_ms: None,
        }
    }

    /// 用调用方提供的字体源构造(测试/自带字体)。
    #[must_use]
    pub fn with_source(source: FontSource) -> Self {
        Self {
            renderer: Some(FontRenderer::new(source, CacheLimits::default())),
            failure: None,
            faces: BTreeMap::new(),
            load_ms: None,
        }
    }

    /// 字体是否可用(会触发一次懒加载)。
    pub fn available(&mut self) -> bool {
        self.ensure()
    }

    /// 预枚举系统字体(桌面宿主启动时调用)。
    ///
    /// `fontdb::Database::load_system_fonts()` 在本机实测 ~130ms;不预热
    /// 的话,第一次 `CGACT[TEXT]`(設定→テキスト页签)会卡这一下。
    /// 只初始化字体源,不改变任何渲染语义。
    pub fn warm(&mut self) -> bool {
        let ok = self.ensure();
        // 预热耗时由 `Engine::new` 的 startup 日志记录;不要让它出现在
        // 第一帧的 `text_diags` 里(那会把启动成本误报成帧内成本)。
        let _ = self.load_ms.take();
        ok
    }

    /// 不可用原因(`font.missing` 等)。
    #[must_use]
    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }

    fn ensure(&mut self) -> bool {
        if self.renderer.is_some() {
            return true;
        }
        if let Some(why) = &self.failure {
            let _ = why;
            return false;
        }
        let started = Instant::now();
        match FontSource::load_system_fonts() {
            Ok(source) => {
                self.renderer = Some(FontRenderer::new(source, CacheLimits::default()));
                self.load_ms = Some(started.elapsed().as_millis());
                true
            }
            Err(diag) => {
                self.failure = Some(format!("{}: {}", diag.code, diag.detail));
                false
            }
        }
    }

    /// 解析 `FONT` 字体身份请求(`NAME` → `NO` → `BNO` → 引擎默认回退表)。
    fn resolve_face(&mut self, request: &TextFontRequest) -> Option<yu_font::FaceId> {
        let key = format!("{request:?}");
        if let Some(hit) = self.faces.get(&key) {
            return *hit;
        }
        if !self.ensure() {
            return None;
        }
        let renderer = self.renderer.as_ref()?;
        let source = renderer.source();
        let face = match request {
            TextFontRequest::Name(bytes) => {
                let text: String = cp932::decode(bytes).chars.into_iter().collect();
                source.find_family(&text)
            }
            TextFontRequest::No(no) => {
                if *no >= 0 {
                    source.face_by_no(*no as usize)
                } else {
                    None
                }
            }
            // BNO 的字体列表语义未取证(DIV-FONT-007):不解析,退回默认。
            TextFontRequest::Bno(_) => source.default_face().ok(),
            TextFontRequest::Default => source.default_face().ok(),
        };
        self.faces.insert(key, face);
        face
    }

    /// gfx 字体快照 → `yu-font` 样式(带 face 解析)。
    ///
    /// 返回值第三项 = 是否走了 `BNO` 的“默认字体”回退(DIV-FONT-007
    /// 未取证);调用方据此发一次性报告,不静默回退。
    fn font_style(&mut self, style: &yu_gfx::TextStyle) -> Option<(FontStyle, u16, bool)> {
        let request = style.font_request();
        let bno_fallback = matches!(request, TextFontRequest::Bno(_));
        let face = self.resolve_face(&request)?;
        // SX/SY=0 → 20:引擎默认样式(`FUN_00464fac`;见
        // `docs/font-contract.md` §3.3“引擎默认 SX=SY=20”)。
        let sx = style.size_x.clamp(0, 0x100) as u16;
        let sy = style.size_y.clamp(0, 0x100) as u16;
        let sx = if sx == 0 { 20 } else { sx };
        let sy = if sy == 0 { 20 } else { sy };
        let mut s = FontStyle::engine_default(face);
        s.sx = sx;
        s.sy = sy;
        s.bold = style.bold;
        s.aa = if style.antialias {
            AaMode::Grayscale
        } else {
            AaMode::None
        };
        s.color = Rgb::from_rgb_u32(style.color & 0x00FF_FFFF);
        // COLOR2 缺省回退 COLOR:引擎样式字段默认 `0xffffff`
        // (`docs/font-provenance.md`:COLOR2 槽 `+0x2c`,初值 `0x5d1708`,
        // 默认 `0xffffff`);脚本未给 COLOR2 时按单色绘制。该“未给”分支
        // 本身无单独手册例,保留回退并在审计表挂账。
        s.color2 = Rgb::from_rgb_u32(style.color2.unwrap_or(style.color) & 0x00FF_FFFF);
        s.gradation =
            Gradation::from_script(style.gradation).unwrap_or(Gradation::Direction { octant: 1 });
        s.rotation = Rotation::from_script(style.rotation).unwrap_or(Rotation::None);
        s.draw_mode = if style.draw_mode != 0 {
            DrawMode::Blend
        } else {
            DrawMode::Replace
        };
        Some((s, sy, bno_fallback))
    }

    /// 在 `dst`(RGBA8,`w×h`)上绘制一个 `CGACT[TEXT]` 项。
    fn paint_one(
        &mut self,
        dst: &mut [u8],
        w: u32,
        h: u32,
        draw: &TextDraw,
        stats: &mut TextPaintStats,
    ) {
        if !self.ensure() {
            if let Some(why) = self.failure.clone() {
                stats.note(&format!("text.font_unavailable: {why}"));
            }
            stats.draws += 1;
            return;
        }
        if let Some(ms) = self.load_ms.take() {
            stats.note(&format!("text.font_init: {ms}ms"));
        }
        let Some((font_style, sy, bno_fallback)) = self.font_style(&draw.style) else {
            stats.draws += 1;
            stats.note("text.font_missing");
            return;
        };
        if bno_fallback {
            stats.note(
                "text.font.bno_fallback: BNO 字体列表语义未取证(DIV-FONT-007),按默认字体绘制",
            );
        }
        let ascent = {
            let renderer = self.renderer.as_ref().expect("ensured above");
            match renderer.source().ascent_px(font_style.face, sy) {
                Ok(a) => a.round() as i32,
                Err(_) => {
                    stats.draws += 1;
                    stats.note("text.metrics_unavailable");
                    return;
                }
            }
        };
        let blended = font_style.draw_mode == DrawMode::Blend;
        let passes = draw.composite_passes();
        let decoded = cp932::decode(&draw.bytes);
        if decoded.invalid > 0 {
            stats.missing += decoded.invalid;
            stats.note("text.decode_invalid");
        }
        let mut pen_x = draw.x as i32;
        let baseline = draw.y as i32 + ascent;
        let renderer = self.renderer.as_mut().expect("ensured above");
        let mut drew_any = false;
        for ch in &decoded.chars {
            match renderer.render_glyph(*ch, &font_style, StylePolicy::Reference) {
                Ok(image) => {
                    let gx = pen_x + image.left;
                    let gy = baseline + image.top;
                    for _ in 0..passes {
                        image.draw_onto(dst, w, h, gx, gy, blended);
                    }
                    let step = ((image.advance as f32) / 256.0).round() as i32;
                    pen_x += step.max(0);
                    stats.glyphs += 1;
                    drew_any = true;
                }
                Err(diag) => {
                    stats.missing += 1;
                    stats.note(&format!("text.glyph_missing: {}", diag.code));
                }
            }
        }
        let _ = drew_any;
        stats.draws += 1;
    }

    /// 按 `CGACT[TEXT]` 绘制项在 RGBA8 缓冲上合成(原地)。
    pub fn paint_draws(
        &mut self,
        dst: &mut [u8],
        w: u32,
        h: u32,
        draws: &[TextDraw],
    ) -> TextPaintStats {
        let mut stats = TextPaintStats::default();
        if w == 0 || h == 0 || dst.len() < (w as usize) * (h as usize) * 4 {
            return stats;
        }
        for draw in draws {
            self.paint_one(dst, w, h, draw, &mut stats);
        }
        stats
    }

    /// 按 `gfx` 已排版的 `_TEXT_FLAG` 单元在 RGBA8 缓冲上合成(原地)。
    pub fn paint_layout(
        &mut self,
        dst: &mut [u8],
        w: u32,
        h: u32,
        layout: &TextLayoutFrame,
    ) -> TextPaintStats {
        self.paint_layout_with(dst, w, h, layout, None)
    }

    /// 同 [`TextComposer::paint_layout`];`blend_override` 强制覆盖/合成。
    ///
    /// 屏幕叠加(没有文本图层承载基图)必须用**合成**,否则 `DRAWMODE=0`
    /// 会把字形框内的屏幕像素写成透明(`DIV-GFX-057`)。图层内合成时用
    /// `FONT[DRAWMODE]`。
    pub fn paint_layout_with(
        &mut self,
        dst: &mut [u8],
        w: u32,
        h: u32,
        layout: &TextLayoutFrame,
        blend_override: Option<bool>,
    ) -> TextPaintStats {
        let mut stats = TextPaintStats::default();
        if w == 0 || h == 0 || dst.len() < (w as usize) * (h as usize) * 4 {
            return stats;
        }
        if !self.ensure() {
            if let Some(why) = self.failure.clone() {
                stats.note(&format!("text.font_unavailable: {why}"));
            }
            return stats;
        }
        if let Some(ms) = self.load_ms.take() {
            stats.note(&format!("text.font_init: {ms}ms"));
        }
        let style = &layout.style;
        let Some((font_style, sy, bno_fallback)) = self.font_style(style) else {
            stats.note("text.font_missing");
            return stats;
        };
        if bno_fallback {
            stats.note(
                "text.font.bno_fallback: BNO 字体列表语义未取证(DIV-FONT-007),按默认字体绘制",
            );
        }
        let ascent = {
            let renderer = self.renderer.as_ref().expect("ensured above");
            match renderer.source().ascent_px(font_style.face, sy) {
                Ok(a) => a.round() as i32,
                Err(_) => {
                    stats.note("text.metrics_unavailable");
                    return stats;
                }
            }
        };
        // 自动文本层:1 次合成(引擎未取证重复;`_TEXT_FLAG` 只控排版)。
        let blended = blend_override.unwrap_or(font_style.draw_mode == DrawMode::Blend);
        let renderer = self.renderer.as_mut().expect("ensured above");
        let visible = layout.reveal.unwrap_or(usize::MAX);
        for glyph in layout.glyphs.iter().take(visible) {
            let Some(ch) = cp932::decode_char(&glyph.bytes) else {
                stats.missing += 1;
                stats.note("text.decode_invalid");
                continue;
            };
            match renderer.render_glyph(ch, &font_style, StylePolicy::Reference) {
                Ok(image) => {
                    let gx = glyph.x as i32 + image.left;
                    let gy = glyph.y as i32 + ascent + image.top;
                    image.draw_onto(dst, w, h, gx, gy, blended);
                    stats.glyphs += 1;
                }
                Err(diag) => {
                    stats.missing += 1;
                    stats.note(&format!("text.glyph_missing: {}", diag.code));
                }
            }
        }
        stats.draws = layout.chars;
        stats
    }

    /// 为图层合成"基图 + 文本"的副本。
    ///
    /// 返回 `None` 表示没有可绘制的文本;`Some` 时像素已包含文本。
    /// 图层局部坐标与基图一致(超出基图的部分被裁剪 —— 引擎写入图层像素,
    /// 同样受图层尺寸限制)。
    pub fn compose_layer(
        &mut self,
        base: &Texture,
        draws: &[TextDraw],
        layout: Option<&TextLayoutFrame>,
    ) -> (Option<Texture>, TextPaintStats) {
        if draws.is_empty() && layout.is_none() {
            return (None, TextPaintStats::default());
        }
        let mut out = base.clone();
        let mut stats = self.paint_draws(&mut out.rgba, out.width, out.height, draws);
        if let Some(layout) = layout {
            stats.merge(self.paint_layout(&mut out.rgba, out.width, out.height, layout));
        }
        (Some(out), stats)
    }
}

/// 解析图层的基图(文本合成的输入)。
///
/// 只支持资产/1×1 黑/无纹理(透明):快照与离屏组需要渲染器的像素面,
/// 由调用方显式拒绝(不猜)。
pub fn layer_base(
    textures: &mut crate::texture::TextureStore,
    layer: &yu_gfx::Layer,
) -> Result<Texture, String> {
    match &layer.texture {
        yu_gfx::TextureRef::Asset(key) => textures
            .resolve(key)
            .cloned()
            .map_err(|d| d.code.to_string()),
        yu_gfx::TextureRef::SolidBlack1x1 => Ok(Texture::solid_black_1x1()),
        yu_gfx::TextureRef::None => {
            let size = layer
                .image_size()
                .ok_or_else(|| "text.layer_size_unknown".to_string())?;
            Ok(Texture {
                width: size.width,
                height: size.height,
                rgba: vec![0; (size.width as usize) * (size.height as usize) * 4],
            })
        }
        yu_gfx::TextureRef::Snapshot(_) | yu_gfx::TextureRef::Offscreen(_) => {
            Err("text.layer_source_unsupported".to_string())
        }
    }
}
