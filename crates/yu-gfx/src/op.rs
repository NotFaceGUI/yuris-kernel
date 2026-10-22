//! 显示列表操作(`Op`)。
//!
//! 后端只执行本枚举;任何语义缺项都用 [`OpStatus::Unimplemented`] 显式标记,
//! 不允许后端“猜”公式或静默跳过。

use crate::group::GroupDraw;
use crate::layer::{CopyId, EngineValue, Layer, LayerId};
use crate::transition::TransitionDraw;

/// 一条绘制/复制/图像操作。
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    /// 普通纹理层。
    Layer(Layer),
    /// 图层组的离屏合成结果作为一层绘制。
    Group(GroupDraw),
    /// 双纹理过渡。
    Transition(TransitionDraw),
    /// 带外部掩码(`MID`)的层。
    Mask(MaskDraw),
    /// `CGACT[COPY/COPY2/TEXCOPY/TEXSSCOPY]`(渲染到纹理)。
    Copy(CopyOp),
    /// `CGACT` 图像加工(涂色/滤波/旋转/文字烘焙)。
    Image(ImageOp),
}

impl Op {
    pub fn status(&self) -> OpStatus {
        match self {
            Self::Layer(l) => map_layer_status(l.status),
            Self::Group(g) => map_layer_status(g.layer.status),
            Self::Transition(t) => map_layer_status(t.layer.status),
            Self::Mask(m) => map_layer_status(m.layer.status),
            Self::Copy(c) => c.status,
            Self::Image(i) => i.status,
        }
    }
}

fn map_layer_status(s: crate::layer::LayerStatus) -> OpStatus {
    match s {
        crate::layer::LayerStatus::Executable => OpStatus::Executable,
        crate::layer::LayerStatus::Unimplemented(why) => OpStatus::Unimplemented(why),
    }
}

/// 操作可执行性。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpStatus {
    /// 语义已取证,后端应执行。
    Executable,
    /// 语义缺项;后端必须跳过并保留诊断,不得近似。
    Unimplemented(&'static str),
}

/// 外部掩码层。
#[derive(Clone, Debug, PartialEq)]
pub struct MaskDraw {
    /// 被掩码的层(其 `mask` 字段同时给出掩码 ID)。
    pub layer: Layer,
    /// 掩码层 ID。
    pub mask: LayerId,
}

/// 复制种类(手册 `cg/cgact.html`)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyKind {
    /// `COPY=1`:图像 + 属性。
    Copy,
    /// `COPY2=1`:仅图像。
    Copy2,
    /// `TEXCOPY`:纹理复制(参数表 slot 68)。
    TexCopy,
    /// `TEXSSCOPY`(slot 69)。
    TexSsCopy,
}

impl CopyKind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Copy => "copy",
            Self::Copy2 => "copy2",
            Self::TexCopy => "texcopy",
            Self::TexSsCopy => "texsscopy",
        }
    }
}

/// 复制源。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CopySource {
    /// 单个图层(图像或已合成的组)。
    Layer(LayerId),
    /// 图层组(取组离屏合成结果)。
    Group(LayerId),
}

/// 复制区域(引擎 `X/Y/X2/Y2` 原值;`Full` = 未给区域)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyRegion {
    Full,
    /// `x/y` = 起点,`x2/y2` = 终点或相对值(引擎语义未逐项取证)。
    Partial {
        x: i64,
        y: i64,
        x2: i64,
        y2: i64,
    },
}

/// 复制时序。
///
/// 任务 J2(EVID-T-12 / EVID-J-11):官方在**命令执行时**取源层的当前状态
/// (组源 = 立刻重绘组的当前内容),旧接手线的 `last_rendered_frame` 已被
/// 受控微场景 Refuted。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyTiming {
    /// 命令执行时当前状态;快照在目标层存活期间保持(后端按 `CopyId` 缓存)。
    CommandCurrent,
}

/// 一次渲染到纹理复制。
#[derive(Clone, Debug, PartialEq)]
pub struct CopyOp {
    /// 命令执行序(COPY 与图像加工共用一个单调序号)。
    ///
    /// 证据:官方在命令处理器里**立即**执行(EVID-T-15);`frame()` 按本序号
    /// 交错输出 copy/image 纹理 pass,而不是按类型分组(DIV-GFX-029)。
    pub seq: u64,
    /// 快照 ID(目标层的 `TextureRef::Snapshot` 引用它)。
    pub id: CopyId,
    pub kind: CopyKind,
    pub source: CopySource,
    /// 复制目标图层路径。
    pub dest: LayerId,
    pub region: CopyRegion,
    pub timing: CopyTiming,
    pub status: OpStatus,
}

/// `CGACT` 图像加工种类(像素语义属图像层;gfx 只记录事实)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImageAction {
    RectPaint,
    RectPaint2,
    RectPaint3,
    Text,
    SizeNearest,
    SizeBilinear,
    Rotation,
    Resize,
    Trim,
    Merge,
    Merge2,
    /// `REVERSE=1`:RGB 逐通道取反(`255-x`),alpha 不变。
    Reverse,
    /// `HORIZONTAL=1`:水平镜像。
    Horizontal,
    Other(&'static str),
}

impl ImageAction {
    pub fn name(&self) -> &str {
        match self {
            Self::RectPaint => "rectpaint",
            Self::RectPaint2 => "rectpaint2",
            Self::RectPaint3 => "rectpaint3",
            Self::Text => "text",
            Self::SizeNearest => "size",
            Self::SizeBilinear => "size2",
            Self::Rotation => "rotation",
            Self::Resize => "resize",
            Self::Trim => "trim",
            Self::Merge => "merge",
            Self::Merge2 => "merge2",
            Self::Reverse => "reverse",
            Self::Horizontal => "horizontal",
            Self::Other(s) => s,
        }
    }
}

/// 一次图像加工操作。
///
/// J2 放行的动作:`RECTPAINT/2/3`(EVID-J-02..05)、`REVERSE`/`HORIZONTAL`
/// (EVID-T-13);其余像素算法仍未取证,`status` 为 `Unimplemented`。
#[derive(Clone, Debug, PartialEq)]
pub struct ImageOp {
    /// 命令执行序(与 [`CopyOp::seq`] 同一序列,用于交错输出)。
    pub seq: u64,
    /// 源图层(`ID`;`ID2` 存在时加工结果写入目标层)。
    pub source: LayerId,
    /// `ID2`(复制/加工结果目标;`None` = 原地加工)。
    pub target: Option<LayerId>,
    pub action: ImageAction,
    /// `SET` 原值。
    pub set: Option<EngineValue>,
    /// `SET2` 原值。
    pub set2: Option<EngineValue>,
    /// `SETSTR` 原文(CP932 字节)。
    pub set_str: Option<Vec<u8>>,
    /// `SIZE/SIZE2` 的目标宽(手册 `cg/cgact.html` 例 3/4)。
    pub sx: Option<i64>,
    /// `SIZE/SIZE2` 的目标高。
    pub sy: Option<i64>,
    pub x: Option<i64>,
    pub y: Option<i64>,
    pub x2: Option<i64>,
    pub y2: Option<i64>,
    pub status: OpStatus,
}
