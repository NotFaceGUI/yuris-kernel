//! 文本层模型(只定义“画什么”,不做字形栅格化)。
//!
//! 证据:
//! - 手册 `font/font.html`:NAME/NO/COLOR/COLOR2/SX/SY/BOLD/ANTIALIASING/DRAWMODE。
//! - 手册 `font/fontinfo.html`:读取当前字体属性。
//! - 手册 `cg/cgact.html` 例 25/26:`CGACT[TEXT=1 SETSTR=$A X=30 Y=50]` 在图层
//!   的 `(X,Y)` 位置绘制字符串。
//! - 真实语料 ERIS:`Text/es_Text.yst` 每字符读 `$_TEXT_FLAG`,`'1'` → `SX/2`、
//!   `'2'` → `SX` 计算步进,再以 `CGACT[TEXT=1 SETSTR=<char> X=… Y=…]` 绘制;
//!   即**排版由脚本/引擎的逐字标志流决定**,不是字形 advance。
//! - REA(0.495):`FUN_004046fc`/`FUN_00404914` 自动文本渲染器;`FUN_004056b8`
//!   取单字 8-bit mask 后按 ANTIALIASING 选择 AA/非 AA 合成器,并按一个
//!   栈参数重复合成 `1`(0)/`9`(1..10)/`3`(>=11) 次。ERIS 的 `CGACT[TEXT]`
//!   只用 `SET=0/1`(填色/加粗轮廓)⇒ 重复次数映射记录为 `Likely`
//!   (`docs/divergences/gfx-text.md` DIV-GFX-055)。
//!
//! 字形栅格化、字体回退全部留给 `yu-font`;本模块只给后端一个稳定的输入契约。

use crate::layer::LayerId;

/// 字体属性快照(`FONT` 命令的累积状态)。
///
/// 默认值来自 REA `FUN_00464fac`:SX=SY=20、COLOR=COLOR2=0xffffff、
/// ANTIALIASING=1、GRADATION=1、ROTATION=0、DRAWMODE=0、BOLD=0。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TextStyle {
    /// `NAME`(CP932 原始字节;`None` = 未设置)。
    pub name: Option<Vec<u8>>,
    /// `NO`(字体列表序号;手册 `font/font.html` 例 2)。
    pub font_no: Option<i64>,
    /// `COLOR` 0xRRGGBB。
    pub color: u32,
    /// `COLOR2`(渐变终点;引擎默认与 `COLOR` 相同 = 0xffffff)。
    pub color2: Option<u32>,
    /// `SX`(横幅;0 = 未设置)。
    pub size_x: i64,
    /// `SY`(纵幅;0 = 未设置)。
    pub size_y: i64,
    /// `BOLD`。
    pub bold: bool,
    /// `ANTIALIASING`。
    pub antialias: bool,
    /// `DRAWMODE` 0=上書き / 1=合成。
    pub draw_mode: i64,
    /// `GRADATION` 0..=8(默认 1;方向映射未取证,DIV-FONT-003)。
    ///
    /// 只保存原值;后端必须按 `Unverified` 处理渲染效果(纯色等价于 0/1)。
    pub gradation: i64,
    /// `ROTATION` 0..=8(默认 0;引擎未见读取,DIV-FONT-004)。
    ///
    /// 只保存原值;`Unverified`。
    pub rotation: i64,
    /// 行距(手册 FONT 页未给;`None` = 字体/窗口会话决定)。
    pub line_spacing: Option<i64>,
    /// 描边(手册 FONT 页未给;`None` = 未设置)。
    pub stroke: Option<TextStroke>,
}

/// 描边样式(模型占位;像素语义归字体/后端会话)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextStroke {
    /// 0xRRGGBB。
    pub color: u32,
    /// 引擎像素宽度。
    pub width: i64,
}

/// 引擎默认字号(`FUN_00464fac`;`docs/font-contract.md` §3.3:
/// “引擎默认 SX=SY=20”)。
const DEFAULT_FONT_SIZE: i64 = 20;

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            name: None,
            font_no: None,
            color: 0x00FF_FFFF,
            color2: Some(0x00FF_FFFF),
            size_x: DEFAULT_FONT_SIZE,
            size_y: DEFAULT_FONT_SIZE,
            bold: false,
            antialias: true,
            draw_mode: 0,
            gradation: 1,
            rotation: 0,
            line_spacing: None,
            stroke: None,
        }
    }
}

impl TextStyle {
    /// FONT 命令 clamp 复刻(REA `0x444428..0x444b1c`;与 `yu_font::FontSlot`
    /// 同口径)。gfx 只保存值,渲染侧再做范围校验。
    pub fn clamp(&mut self) {
        self.color &= 0x00FF_FFFF;
        if let Some(c) = &mut self.color2 {
            *c &= 0x00FF_FFFF;
        }
        self.size_x = self.size_x.clamp(0, 0x100);
        self.size_y = self.size_y.clamp(0, 0x100);
        self.gradation = self.gradation.clamp(0, 8);
        self.rotation = self.rotation.clamp(0, 8);
        self.draw_mode = self.draw_mode.clamp(0, 1);
    }

    /// 单元宽度(像素):全角 = `SX`,半角 = `SX/2`。
    ///
    /// 证据:ERIS `Text/es_Text.yst` `es.TX.BUFSET.SIZE` 逐字读
    /// `_TEXT_FLAG`:`"1"` → `@TX.SX/2`、`"2"` → `@TX.SX`(COV-M-03 的消费者)。
    /// `SX=0` 时按引擎默认 20(REA `FUN_00464fac`)。
    pub fn cell_width(&self, full: bool) -> i64 {
        let sx = if self.size_x == 0 {
            DEFAULT_FONT_SIZE
        } else {
            self.size_x
        };
        if full {
            sx
        } else {
            sx / 2
        }
    }

    /// 行高(像素)。引擎行距未单独取证:使用 `SY`(`SY=0` → 20)。
    /// `line_spacing` 若显式设置则覆盖。`Unverified`(DIV-GFX-056)。
    pub fn line_height(&self) -> i64 {
        if let Some(v) = self.line_spacing {
            return v;
        }
        if self.size_y == 0 {
            20
        } else {
            self.size_y
        }
    }

    /// 字体身份请求(`NAME`/`NO` → `yu-font` 发现链)。
    ///
    /// 引擎的 `FONT` 处理器按 ysc 槽序写入(`NO` 在 `NAME` 之前),后写的
    /// 覆盖 ⇒ `NAME` 优先于 `NO`。`BNO` **不是字体身份**:它是文字样式
    /// 槽号选择器(REA `FUN_004039d0` 写 `[0x5d1684]`),由
    /// `GfxState::select_style_slot` 处理,不进本请求。
    pub fn font_request(&self) -> TextFontRequest {
        if let Some(name) = &self.name {
            TextFontRequest::Name(name.clone())
        } else if let Some(no) = self.font_no {
            TextFontRequest::No(no)
        } else {
            TextFontRequest::Default
        }
    }
}

/// 字体身份请求;由后端用 `yu-font` 发现链解析(`NAME` → `find_family`,
/// `NO` → `face_by_no`,其余 → `default_face` 的引擎回退表)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextFontRequest {
    /// 显式字体名(CP932 原始字节)。
    Name(Vec<u8>),
    /// 字体列表序号(`FONT[NO]`)。
    No(i64),
    /// 未设置 → 引擎默认 family(REA `0x403b8e..0x403bdc` 回退表)。
    Default,
}

/// 字形图集引用(后端/字体会话提供;gfx 不生成字形)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlyphAtlasRef {
    /// 后端资产 key。
    pub key: Vec<u8>,
    /// 图集版本(后端递增;用于缓存失效)。
    pub version: u32,
}

/// 一段待显示的文本(`VmEvent::Text` / `$_TEXT` 的累积结果)。
///
/// **跨帧持久**(DIV-GFX-034):文本缓冲只在 `TEXT[CLEAR]`、
/// `GfxState::clear_text`、目标图层被 `CGEND` 删除或重新 `CG` 创建时清空。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextRun {
    /// 文本层 ID(`TEXT[ID=...]`;空 = 默认层)。
    pub id: LayerId,
    /// CP932 原始字节(引擎文本缓冲原样)。
    pub bytes: Vec<u8>,
    /// `_TEXT_FLAG` 控制字节流(与 `bytes` 中的**字符**一一对应,含
    /// `'1'/'2'/'R'/'P'/'r'/'p'/'C'/'E'`)。`None` = 宿主尚未同步引擎
    /// 状态(`Vm::text_state()`),排版必须视为不可用而不是自行猜测。
    pub flags: Option<Vec<u8>>,
    /// 当前字体属性(最近一次 `FONT` 快照)。
    pub style: TextStyle,
    /// 文本层原点(引擎像素;`None` = 未定位,等待窗口/字体会话)。
    pub origin: Option<(i64, i64)>,
    /// 当前应显示的字符数(`TEXT[OUTPUTMODE=0]` 手动模式的 reveal 计数)。
    ///
    /// `None` = 全部显示。VM 侧 reveal 查询 API 尚未提供(交接件 H-P2-02);
    /// 本字段是后端截断渲染的唯一入口,不允许后端自行计时。
    pub reveal: Option<usize>,
    /// 字形图集(未提供 = 后端自行栅格化)。
    pub atlas: Option<GlyphAtlasRef>,
    /// 该段来自的事件帧号。
    pub frame: u64,
}

/// 一个已排版的字符单元(后端按 `bytes` 查字体栅格化)。
///
/// 坐标是**文本层局部**像素:原点是文本层/文本窗口左上角,`y` 是单元顶边。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextGlyph {
    /// CP932 单字符字节(1 或 2 字节;控制标志不产生 glyph)。
    pub bytes: Vec<u8>,
    /// 是否全角(`_TEXT_FLAG == '2'`)。用于后端选择单元宽度。
    pub full: bool,
    /// 单元左上角 X(局部像素)。
    pub x: i64,
    /// 单元左上角 Y(局部像素)。
    pub y: i64,
}

/// 自动文本层(`TEXT[ID]` + `WORD`/`_`/`RETURNCODE`)的排版结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextLayoutFrame {
    /// 目标文本层(`TEXT[ID]` 且该层存在);`None` = 屏幕坐标叠加。
    pub target: Option<LayerId>,
    /// 文本层局部原点(`target` 存在 = 层内坐标;否则 = 屏幕坐标)。
    pub origin: (i64, i64),
    /// 字体快照。
    pub style: TextStyle,
    /// 逐字单元(按 `_TEXT_FLAG` 的 `'1'/'2'` 步进)。
    pub glyphs: Vec<TextGlyph>,
    /// 行高(像素;`Unverified`)。
    pub line_height: i64,
    /// 本帧可见的字节数 = `glyphs` 覆盖的字符数(审计用)。
    pub chars: usize,
    /// 当前应显示的字符数(`None` = 全部;H-P2-02 的渲染截断入口)。
    pub reveal: Option<usize>,
    /// 排版证据等级:`_TEXT_FLAG` 完整 = `Confirmed`;否则不产出本结构。
    pub flags_source: TextFlagsSource,
}

/// `TextLayoutFrame` 的宽度证据来源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextFlagsSource {
    /// `_TEXT_FLAG` 逐字 `'1'/'2'` 流(一手证据,COV-M-03)。
    TextFlag,
}

/// `CGACT[TEXT=1]` 的一次绘制(ERIS 对话文本的**真实路径**)。
///
/// 手册 `cg/cgact.html` 例 25/26:`TEXT=1 SETSTR=<字符串> X/Y` 在图层上绘制。
/// 字形/颜色来自当时 `FONT` 状态(REA `0x404a1b..0x404ac7` 读取 AA/DRAWMODE
/// 与文本对象字段)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextDraw {
    /// 目标图层。
    pub target: LayerId,
    /// `SETSTR` 原始 CP932 字节(字符串整体)。
    pub bytes: Vec<u8>,
    /// `X`(图层局部,缺省 0)。
    pub x: i64,
    /// `Y`(图层局部,缺省 0)。
    pub y: i64,
    /// 绘制时的 `FONT` 快照。
    pub style: TextStyle,
    /// `SET` 原值。已取证映射(REA `FUN_004056b8`):`SET=0` 单次;
    /// `SET=1..=10` 3×3 邻域 9 次;`SET>=11` 3 次阶梯偏移。
    pub set: i64,
    /// `CGACT[BNO]`(调试字体槽;原值保留,语义未取证 DIV-FONT-007)。
    pub bno: Option<i64>,
}

impl TextDraw {
    /// `SET` → 合成偏移列表(REA 0.495 `FUN_004056b8`,asm `0x405703..0x405c32`)。
    ///
    /// 引擎**不是**在同一位置重复画 N 次,而是对每次 `FUN_00406184` 调用
    /// 传入不同的 X/Y 偏移(asm 每处 `LEA EDI,[EAX+EDI]` / `SUB EDI,EAX`
    /// 组装,`0x20`/`0x24` 两个栈参数):
    ///
    /// - `SET=0`:1 次,偏移 `(0,0)`;
    /// - `SET=1..=10`:9 次,3×3 邻域,步长 = `SET`
    ///   (即 `ERIS es_Font.yst` 的 `SET=1` = 外扩 1 px 的轮廓/加粗);
    /// - `SET>=11`:3 次,偏移 `(SET-10,0)`、`(0,SET-10)`、
    ///   `(SET-10,SET-10)`(asm `0x40571c: LEA EDI,[EDI+EAX-0xa]` 及
    ///   `0x405789` 同式)。
    ///
    /// 同位置叠 9 次会把抗锯齿边缘压实成实心(字变粗变白),
    /// 与官方"外扩 1 px 但保留 AA"不同 ⇒ 必须按偏移画。
    pub fn composite_offsets(&self) -> ([(i32, i32); 9], usize) {
        let mut out = [(0i32, 0i32); 9];
        if self.set == 0 {
            return (out, 1);
        }
        if self.set < 11 {
            let step = self.set.max(1) as i32;
            let mut n = 0;
            for dy in [-step, 0, step] {
                for dx in [-step, 0, step] {
                    out[n] = (dx, dy);
                    n += 1;
                }
            }
            return (out, n);
        }
        let d = (self.set - 10) as i32;
        out[0] = (d, 0);
        out[1] = (0, d);
        out[2] = (d, d);
        (out, 3)
    }

    /// 合成次数(= [`Self::composite_offsets`] 的有效长度)。
    pub fn composite_passes(&self) -> u32 {
        self.composite_offsets().1 as u32
    }

    /// 命令时刻去重键(不含 `target`:键控按目标层分桶)。
    ///
    /// 官方 `CGACT[TEXT]` 即时写层表面;同层同参的重复命令在像素上**不是**
    /// 幂等的(`FONT[DRAWMODE=1]` 下重复合成会加深),因此跨帧持久的绘制项
    /// 必须按本键精确去重(DIV-GFX-055 预登记;P8 取证 es_Text.yst 每帧
    /// 重发同集合,去重后逐像素等价)。
    pub fn dedup_key(&self) -> TextDrawKey {
        TextDrawKey {
            bytes: self.bytes.clone(),
            x: self.x,
            y: self.y,
            set: self.set,
            bno: self.bno,
            style: self.style.clone(),
        }
    }
}

/// [`TextDraw::dedup_key`] 的键型(精确相等 = 同层同参重复命令)。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TextDrawKey {
    bytes: Vec<u8>,
    x: i64,
    y: i64,
    set: i64,
    bno: Option<i64>,
    style: TextStyle,
}

impl TextDrawKey {
    /// 与一条绘制项逐字段比对(等价于 `draw.dedup_key() == *self`,免分配)。
    ///
    /// 增量烘焙的前缀校验按此逐项确认;`dedup_key()` 每次构造都要克隆
    /// bytes/style,逐帧逐项调用会白耗分配。
    #[must_use]
    pub fn matches_draw(&self, draw: &TextDraw) -> bool {
        self.bytes == draw.bytes
            && self.x == draw.x
            && self.y == draw.y
            && self.set == draw.set
            && self.bno == draw.bno
            && self.style == draw.style
    }
}

/// 单层持久文本绘制项的硬上限(DIV-GFX-055 预登记“加去重/上限”)。
///
/// 语料规模参考:对话一页 ~ 数千(逐字 + 影/轮廓/填色),存读档 8 板 ×
/// 16 行 × ≤6 条 ≈ 768。超限丢最旧并在诊断里显式计数,不静默无界。
pub const MAX_TEXT_DRAWS_PER_LAYER: usize = 16_384;

/// 按 `_TEXT_FLAG` 逐字排版(等价于引擎逐字标志流)。
///
/// 规则(全部有证据或显式挂账):
/// - `'1'` 半角 → 步进 `SX/2`;`'2'` 全角 → `SX`(ERIS `es.TX.BUFSET.SIZE`);
/// - `'R'`/`'r'`/`'P'`/`'p'`/`'E'` = 换行,但**只在当前行已有字符时**推进
///   (引擎一行会同时出现 `C R P E`,不能每个标志各占一行;`Unverified`,
///   DIV-GFX-056);
/// - `'C'` = 点击等待,不改变排版;
/// - 其它字节按半角处理并保留(不发明语义)。
///
/// `flags` 必须与 `bytes` 的字符序列对齐(`'1'/'2'` 每个字符一个)。若对齐
/// 失败(标志不足/多余、字节不是合法 CP932 前缀),返回 `None` — 调用方不得
/// 猜测布局。
pub fn layout_text(
    bytes: &[u8],
    flags: &[u8],
    origin: (i64, i64),
    style: &TextStyle,
) -> Option<Vec<TextGlyph>> {
    let mut glyphs = Vec::new();
    let (mut pen_x, mut pen_y) = origin;
    let mut line_has_ink = false;
    let mut i = 0usize;
    for &flag in flags {
        match flag {
            b'1' | b'2' => {
                if i >= bytes.len() {
                    return None;
                }
                let full = flag == b'2';
                let len = if full { 2 } else { 1 };
                if i + len > bytes.len() {
                    return None;
                }
                // 全角标志必须对应双字节字符;半角必须单字节。
                if full != is_cp932_lead(bytes[i]) {
                    return None;
                }
                glyphs.push(TextGlyph {
                    bytes: bytes[i..i + len].to_vec(),
                    full,
                    x: pen_x,
                    y: pen_y,
                });
                pen_x += style.cell_width(full);
                line_has_ink = true;
                i += len;
            }
            b'R' | b'r' | b'P' | b'p' | b'E' => {
                if line_has_ink {
                    pen_x = origin.0;
                    pen_y += style.line_height();
                    line_has_ink = false;
                }
            }
            b'C' => {}
            _ => {
                // 未取证的控制字节:不猜语义;按半角字符尝试消费以保持对齐。
                if i >= bytes.len() {
                    return None;
                }
                let len = if is_cp932_lead(bytes[i]) { 2 } else { 1 };
                if i + len > bytes.len() {
                    return None;
                }
                glyphs.push(TextGlyph {
                    bytes: bytes[i..i + len].to_vec(),
                    full: len == 2,
                    x: pen_x,
                    y: pen_y,
                });
                pen_x += style.cell_width(len == 2);
                line_has_ink = true;
                i += len;
            }
        }
    }
    if i != bytes.len() {
        return None;
    }
    Some(glyphs)
}

/// CP932 首字节(= 双字节字符)。与 `yu-rt::text::is_lead` 同口径。
fn is_cp932_lead(b: u8) -> bool {
    (0x81..=0x9F).contains(&b) || (0xE0..=0xFC).contains(&b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style() -> TextStyle {
        TextStyle {
            size_x: 12,
            size_y: 12,
            ..TextStyle::default()
        }
    }

    #[test]
    fn layout_uses_text_flag_cells() {
        // "AこB":A 半角,こ 全角(0x82B1),B 半角。
        let bytes = b"A\x82\xB1B";
        let flags = b"121";
        let glyphs = layout_text(bytes, flags, (10, 20), &style()).unwrap();
        assert_eq!(glyphs.len(), 3);
        assert_eq!((glyphs[0].x, glyphs[0].y), (10, 20));
        assert!(!glyphs[0].full);
        assert_eq!((glyphs[1].x, glyphs[1].y), (16, 20), "半角 = SX/2 = 6");
        assert!(glyphs[1].full);
        assert_eq!((glyphs[2].x, glyphs[2].y), (28, 20), "全角 = SX = 12");
    }

    #[test]
    fn line_break_only_after_ink() {
        // 「エピローグ。」CP932(6 个全角)+ 自动行尾 CRPE ⇒ 恰好换 1 行。
        let cp932: Vec<u8> = vec![
            0x83, 0x47, 0x83, 0x73, 0x83, 0x8D, 0x81, 0x5B, 0x83, 0x4F, 0x81, 0x42,
        ];
        let flags = b"222222CRPE";
        let glyphs = layout_text(&cp932, flags, (0, 0), &style()).unwrap();
        assert_eq!(glyphs.len(), 6);
        assert_eq!(glyphs[5].x, 60);
        assert!(glyphs.iter().all(|g| g.y == 0), "CRPE 只换一行");
    }

    #[test]
    fn flags_must_align_with_bytes() {
        assert!(layout_text(b"A", b"2", (0, 0), &style()).is_none());
        assert!(layout_text(b"A", b"11", (0, 0), &style()).is_none());
        assert!(layout_text(b"\x82\xB1", b"1", (0, 0), &style()).is_none());
    }

    #[test]
    fn unknown_flag_keeps_bytes_visible() {
        // 未取证标志按半角消费,不丢字节(不猜语义)。
        let glyphs = layout_text(b"AB", b"X1", (0, 0), &style()).unwrap();
        assert_eq!(glyphs.len(), 2);
    }

    #[test]
    fn composite_passes_follow_rea_branches() {
        let mut d = TextDraw {
            target: LayerId::new(b"T".to_vec()),
            bytes: vec![b'A'],
            x: 0,
            y: 0,
            style: style(),
            set: 0,
            bno: None,
        };
        // SET=0 → 1 次,偏移 (0,0)
        assert_eq!(d.composite_passes(), 1);
        let (off, n) = d.composite_offsets();
        assert_eq!((n, off[0]), (1, (0, 0)));

        // SET=1..=10 → 3×3 邻域 9 次,步长 = SET
        d.set = 1;
        assert_eq!(d.composite_passes(), 9);
        let (off, n) = d.composite_offsets();
        assert_eq!(n, 9);
        assert!(off[..9].contains(&(-1, -1)));
        assert!(off[..9].contains(&(0, 0)));
        assert!(off[..9].contains(&(1, 1)));

        d.set = 10;
        assert_eq!(d.composite_passes(), 9);
        let (off, _) = d.composite_offsets();
        assert!(off[..9].contains(&(-10, 10)));

        // SET>=11 → 3 次阶梯偏移 (d,0)/(0,d)/(d,d),d = SET-10
        d.set = 11;
        assert_eq!(d.composite_passes(), 3);
        let (off, n) = d.composite_offsets();
        assert_eq!(n, 3);
        assert_eq!((off[0], off[1], off[2]), ((1, 0), (0, 1), (1, 1)));
    }
}
