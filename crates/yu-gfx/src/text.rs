//! 文本层模型(只定义“画什么”,不做字形栅格化)。
//!
//! 证据:
//! - 手册 `font/font.html`:NAME/NO/COLOR/COLOR2/SX/SY/BOLD/ANTIALIASING/DRAWMODE。
//! - 手册 `font/fontinfo.html`:读取当前字体属性。
//! - 真实语料:ERIS 首屏 `FONT` 3 次(见 `docs/render-parity.md` §10)。
//!
//! 字形栅格化、字体回退、行距/描边/窗口定位全部留给字体/后端会话;
//! 本模块只给它们一个稳定的输入契约。

use crate::layer::LayerId;

/// 字体属性快照(`FONT` 命令的累积状态)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextStyle {
    /// `NAME`(CP932 原始字节;`None` = 未设置)。
    pub name: Option<Vec<u8>>,
    /// `COLOR` 0xRRGGBB。
    pub color: u32,
    /// `COLOR2`(渐变终点;`None` = 无渐变)。
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
    /// 行距(手册 FONT 页未给;`None` = 字体/窗口会话决定)。
    pub line_spacing: Option<i64>,
    /// 描边(手册 FONT 页未给;`None` = 未设置)。
    pub stroke: Option<TextStroke>,
}

/// 描边样式(模型占位;像素语义归字体/后端会话)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextStroke {
    /// 0xRRGGBB。
    pub color: u32,
    /// 引擎像素宽度。
    pub width: i64,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            name: None,
            color: 0x00FF_FFFF,
            color2: None,
            size_x: 0,
            size_y: 0,
            bold: false,
            antialias: true,
            draw_mode: 0,
            line_spacing: None,
            stroke: None,
        }
    }
}

/// 字形图集引用(后端/字体会话提供;gfx 不生成字形)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlyphAtlasRef {
    /// 后端资产 key。
    pub key: Vec<u8>,
    /// 图集版本(后端递增;用于缓存失效)。
    pub version: u32,
}

/// 一段待显示的文本(`VmEvent::Text` 的累积结果)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextRun {
    /// 文本层 ID(`TEXT[ID=...]`;空 = 默认层)。
    pub id: LayerId,
    /// CP932 原始字节(引擎文本缓冲原样)。
    pub bytes: Vec<u8>,
    /// 当前字体属性。
    pub style: TextStyle,
    /// 文本层原点(引擎像素;`None` = 未定位,等待窗口/字体会话)。
    pub origin: Option<(i64, i64)>,
    /// 字形图集(未提供 = 后端自行栅格化)。
    pub atlas: Option<GlyphAtlasRef>,
    /// 该段来自的事件帧号。
    pub frame: u64,
}
