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
use std::rc::Rc;
use std::time::Instant;

use yu_font::cache::CacheLimits;
use yu_font::{cp932, AaMode, DrawMode, FontRenderer, FontSource, Gradation, Rgb, Rotation};
use yu_font::{StylePolicy, TextStyle as FontStyle};
use yu_gfx::{LayerId, TextDraw, TextFontRequest, TextLayoutFrame, TextureRef};

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
    faces: BTreeMap<String, Option<(yu_font::FaceId, FaceFallback)>>,
    /// 首次系统字体枚举耗时(毫秒)。只在加载后的第一帧报告一次
    /// (`text.font_init`),用于解释“第一次打开テキスト页签”的尖峰。
    load_ms: Option<u128>,
    /// 「基图 + 文本」合成结果缓存:`CGACT[TEXT]` 是跨帧持久的烘焙
    /// (DIV-GFX-058),内容没变就不该每帧重烤。键 = 层 + 文本修订号 +
    /// 基面身份;自动文本层(布局每帧变化)不进缓存。
    compose_cache: BTreeMap<LayerId, ComposeCacheEntry>,
    /// `face -> 是否存在真实粗体面`(weight>=600)。**缓存**:
    /// `FontSource::resolve_style` 内部走 fontdb 查询,每帧每文本项调用一次
    /// 会拖出秒级 render 尖峰(实测 1061ms),因此只在首次解析时求值。
    real_bold: BTreeMap<yu_font::FaceId, bool>,
    /// 「要求 BOLD 但本机无粗体面」待报告(发现时置位,发出后清除;
    /// 不逐帧重发,避免刷帧日志)。
    pending_bold_note: bool,
}

/// 一条合成缓存(见 [`TextComposer::compose_cache`])。
#[derive(Clone)]
struct ComposeCacheEntry {
    revision: u64,
    base_key: u64,
    texture: Rc<Texture>,
}

/// 纹理内容哈希(FNV-1a;仅用于帧内图像加工覆盖纹理的缓存键)。
pub fn texture_content_hash(texture: &Texture) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325_u64;
    for &b in &texture.rgba {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// 文本合成基面的身份哈希(资产 key / 1×1 黑 / 快照 id / 无纹理尺寸 +
/// 帧内图像加工覆盖哈希)。与 `TextDraw` 的层修订号一起做缓存键。
pub fn text_base_key(texture: &TextureRef, override_hash: Option<u64>) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325_u64; // FNV-1a
    let mut feed = |bytes: &[u8]| {
        for &b in bytes {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x100_0000_01b3);
        }
    };
    match texture {
        TextureRef::Asset(key) => {
            feed(b"A");
            feed(key);
        }
        TextureRef::SolidBlack1x1 => feed(b"B"),
        TextureRef::Snapshot(id) => {
            feed(b"S");
            feed(&id.0.to_le_bytes());
        }
        TextureRef::Offscreen(id) => {
            feed(b"O");
            feed(id.as_bytes());
        }
        TextureRef::None => feed(b"E"),
    }
    if let Some(hash) = override_hash {
        feed(b"X");
        feed(&hash.to_le_bytes());
    }
    h
}

/// 字体身份解析的回落种类(`font_style` 第三项;进 `text_diags`)。
#[derive(Clone, Debug, PartialEq, Eq)]
enum FaceFallback {
    /// 正常解析。
    None,
    /// `BNO` 调试槽未取证,按默认字体绘制(DIV-FONT-007)。
    Bno,
    /// 字体名在本机不存在(或为空):按引擎回退表默认 family(DIV-GFX-059)。
    Name(Vec<u8>),
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
            compose_cache: BTreeMap::new(),
            real_bold: BTreeMap::new(),
            pending_bold_note: false,
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
            compose_cache: BTreeMap::new(),
            real_bold: BTreeMap::new(),
            pending_bold_note: false,
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
    fn resolve_face(
        &mut self,
        request: &TextFontRequest,
    ) -> Option<(yu_font::FaceId, FaceFallback)> {
        let key = format!("{request:?}");
        if let Some(hit) = self.faces.get(&key) {
            return hit.clone();
        }
        if !self.ensure() {
            return None;
        }
        let renderer = self.renderer.as_ref()?;
        let source = renderer.source();
        let face = match request {
            TextFontRequest::Name(bytes) => {
                let text: String = cp932::decode(bytes).chars.into_iter().collect();
                match source.find_family(&text) {
                    Some(face) => Some((face, FaceFallback::None)),
                    // 名字查不到(空串/本机未装):引擎按默认 family 回退表
                    // 选第一个已安装 family(`0x403b8e..0x403bdc`),不是错误。
                    None => source
                        .default_face()
                        .ok()
                        .map(|face| (face, FaceFallback::Name(bytes.clone()))),
                }
            }
            TextFontRequest::No(no) => {
                if *no >= 0 {
                    source
                        .face_by_no(*no as usize)
                        .map(|face| (face, FaceFallback::None))
                } else {
                    None
                }
            }
            // BNO 的字体列表语义未取证(DIV-FONT-007):不解析,退回默认。
            TextFontRequest::Bno(_) => source
                .default_face()
                .ok()
                .map(|face| (face, FaceFallback::Bno)),
            TextFontRequest::Default => source
                .default_face()
                .ok()
                .map(|face| (face, FaceFallback::None)),
        };
        self.faces.insert(key, face.clone());
        face
    }

    /// gfx 字体快照 → `yu-font` 样式(带 face 解析)。
    ///
    /// 返回值第三项 = 是否走了 `BNO` 的“默认字体”回退(DIV-FONT-007
    /// 未取证);调用方据此发一次性报告,不静默回退。
    fn font_style(&mut self, style: &yu_gfx::TextStyle) -> Option<(FontStyle, u16, FaceFallback)> {
        let request = style.font_request();
        let (face, fallback) = self.resolve_face(&request)?;
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
        // BOLD:只接受真实粗体面。本机 family 无 weight>=600 的 face 时
        // **不做合成加粗**(实测合成膨胀让墨迹 +70%,与官方截图不符),
        // 按原 face 绘制并显式报告 `text.font.bold_missing`。
        if s.bold {
            let has_real = match self.real_bold.get(&face) {
                Some(&v) => v,
                None => {
                    // 纯内存表扫描(不触发 fontdb 加载 ⇒ 无单帧尖峰)。
                    let v = self
                        .renderer
                        .as_ref()
                        .expect("ensured above")
                        .source()
                        .family_has_bold(face);
                    self.real_bold.insert(face, v);
                    if !v {
                        self.pending_bold_note = true;
                    }
                    v
                }
            };
            if !has_real {
                s.bold = false;
            }
        }
        Some((s, sy, fallback))
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
        let Some((font_style, sy, fallback)) = self.font_style(&draw.style) else {
            stats.draws += 1;
            stats.note("text.font_missing");
            return;
        };
        if self.pending_bold_note {
            self.pending_bold_note = false;
            stats.note(
                "text.font.bold_missing: 本机 family 无粗体面(weight>=600);按原 face 绘制,不做合成加粗",
            );
        }
        match &fallback {
            FaceFallback::None => {}
            FaceFallback::Bno => stats.note(
                "text.font.bno_fallback: BNO 字体列表语义未取证(DIV-FONT-007),按默认字体绘制",
            ),
            FaceFallback::Name(bytes) => stats.note(&format!(
                "text.font.name_fallback: {}",
                String::from_utf8_lossy(bytes)
            )),
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
        // 引擎按 **偏移** 多次合成(3×3 邻域),不是同位置重复画
        // (REA `FUN_004056b8`;见 `TextDraw::composite_offsets`)。
        // 同位置叠 9 次会把 AA 边缘压实成实心 ⇒ 字变粗变白。
        let (offsets, passes) = draw.composite_offsets();
        let decoded = cp932::decode(&draw.bytes);
        if decoded.invalid > 0 {
            stats.missing += decoded.invalid;
            stats.note("text.decode_invalid");
        }
        let mut pen_x = draw.x as i32;
        let baseline = draw.y as i32 + ascent;
        let renderer = self.renderer.as_mut().expect("ensured above");
        let mut drew_any = false;
        let mut dbg_glyph_ns: u128 = 0;
        let mut dbg_draw_ns: u128 = 0;
        for ch in &decoded.chars {
            let tg = std::time::Instant::now();
            let rendered = renderer.render_glyph(*ch, &font_style, StylePolicy::Reference);
            dbg_glyph_ns += tg.elapsed().as_nanos();
            match rendered {
                Ok(image) => {
                    let gx = pen_x + image.left;
                    let gy = baseline + image.top;
                    let td = std::time::Instant::now();
                    for &(dx, dy) in &offsets[..passes] {
                        image.draw_onto(dst, w, h, gx + dx, gy + dy, blended);
                    }
                    dbg_draw_ns += td.elapsed().as_nanos();
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
        if std::env::var("YU_P8_TIMING").is_ok() && dbg_glyph_ns + dbg_draw_ns > 500_000 {
            eprintln!(
                "[p8] draw chars={} glyph={}us onto={}us passes={}",
                decoded.chars.len(),
                dbg_glyph_ns / 1000,
                dbg_draw_ns / 1000,
                passes
            );
        }
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
        let Some((font_style, sy, fallback)) = self.font_style(style) else {
            stats.note("text.font_missing");
            return stats;
        };
        match &fallback {
            FaceFallback::None => {}
            FaceFallback::Bno => stats.note(
                "text.font.bno_fallback: BNO 字体列表语义未取证(DIV-FONT-007),按默认字体绘制",
            ),
            FaceFallback::Name(bytes) => stats.note(&format!(
                "text.font.name_fallback: {}",
                String::from_utf8_lossy(bytes)
            )),
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

    /// 同 [`TextComposer::compose_layer`],但按「层 + 文本修订号 + 基面身份」
    /// 缓存结果(P8:持久文本是烘焙语义,内容不变不重合成)。
    ///
    /// 只在 `layout.is_none()`(纯 `CGACT[TEXT]`)时缓存:自动文本层的布局
    /// 每帧变化,缓存会读到旧内容。缓存命中返回同一 `Rc`(wgpu 侧内容哈希
    /// 不变 ⇒ 上传缓存也命中),统计按 0 字形报告(两端一致)。
    pub fn compose_layer_cached(
        &mut self,
        layer: &LayerId,
        revision: u64,
        base_key: u64,
        base: &Texture,
        draws: &[TextDraw],
        layout: Option<&TextLayoutFrame>,
    ) -> (Option<Rc<Texture>>, TextPaintStats) {
        if layout.is_some() {
            let (out, stats) = self.compose_layer(base, draws, layout);
            return (out.map(Rc::new), stats);
        }
        if draws.is_empty() {
            return (None, TextPaintStats::default());
        }
        if let Some(hit) = self.compose_cache.get(layer) {
            if hit.revision == revision
                && hit.base_key == base_key
                && hit.texture.width == base.width
                && hit.texture.height == base.height
            {
                return (Some(hit.texture.clone()), TextPaintStats::default());
            }
        }
        let (out, stats) = self.compose_layer(base, draws, None);
        let Some(texture) = out else {
            return (None, stats);
        };
        let texture = Rc::new(texture);
        if self.compose_cache.len() >= 256 && !self.compose_cache.contains_key(layer) {
            // 上限保护:文本层数量远小于此;极端语料下整体清空(一次性重烤),
            // 不引入跨帧无界内存。
            self.compose_cache.clear();
        }
        self.compose_cache.insert(
            layer.clone(),
            ComposeCacheEntry {
                revision,
                base_key,
                texture: texture.clone(),
            },
        );
        (Some(texture), stats)
    }

    /// 丢弃一个层的合成分缓存(层删除后调用;保守清理由上限保护兜底)。
    pub fn forget_compose_cache(&mut self, layer: &LayerId) {
        self.compose_cache.remove(layer);
    }
}

/// 解析图层的基图(文本合成的输入)。
///
/// 支持资产/1×1 黑/无纹理(透明)/快照(DIV-GFX-058:按钮标签等
/// `CGACT[TEXT]` 直接画在 COPY2 产生的快照精灵层上,基面 = 快照像素);
/// 离屏组仍显式拒绝(组合成的 CPU 面归渲染器,未接线,不猜)。
pub fn layer_base(
    textures: &mut crate::texture::TextureStore,
    snapshots: &crate::render::Snapshots,
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
        yu_gfx::TextureRef::Snapshot(copy_id) => snapshots
            .get(copy_id)
            .map(|surface| Texture {
                width: surface.width,
                height: surface.height,
                rgba: surface.rgba.clone(),
            })
            .ok_or_else(|| "text.layer_snapshot_missing".to_string()),
        yu_gfx::TextureRef::Offscreen(_) => Err("text.layer_source_unsupported".to_string()),
    }
}
