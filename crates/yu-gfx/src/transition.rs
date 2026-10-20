//! 过渡绘制契约(`T`/`TA`/`TID`)。
//!
//! 手册 `cg/cg.html`:`T` 过渡进行度、`TA` 过渡精度、`TID` 过渡图层 ID。
//! 引擎证据:`FUN_00414ba4`(`0x414ba4`)行 905-909 计算
//! `YS_FadeVal = T / (256/(256/TA + 128)) * (1/256)`,
//! 双纹理 `YS_Texture1`(本层)/`YS_Texture2`(配对层)在 EnterPass(1) 下取样。
//!
//! **未取证**:`TID` 配对层缺失时的第二纹理来源、掩码 `MID` 的取值通道、
//! 进度推进由 `T`/`+0x1e8` 谁驱动。这些在 `docs/render-parity.md` 挂号;
//! 本契约只输出已计算出的 `fade` 与原始 `T/TA`。

pub use crate::layer::Transition;

use crate::layer::{Layer, LayerId, TextureRef};

/// 一次双纹理过渡绘制。
#[derive(Clone, Debug, PartialEq)]
pub struct TransitionDraw {
    /// 本层属性(纹理、alpha、blend、变换)。
    pub layer: Layer,
    /// 本层纹理(引擎 `YS_Texture1`)。
    pub from: TextureRef,
    /// 配对纹理(引擎 `YS_Texture2`;`TID` 解析结果,`None` = 未取证)。
    pub to: Option<TextureRef>,
    /// `YS_FadeVal` 原式计算结果(不 clamp)。
    pub fade: f64,
    /// 配对层 ID(原始语义,便于审计)。
    pub tid: Option<LayerId>,
}
