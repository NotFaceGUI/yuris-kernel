//! 过渡绘制契约(`T`/`TA`/`TID`)。
//!
//! 手册 `cg/cg.html`:`T` 过渡进行度、`TA` 过渡精度、`TID` 过渡图层 ID。
//! 引擎证据:`FUN_00414ba4`(`0x414ba4`)行 905-909 计算
//! `YS_FadeVal = T / (256/(256/TA + 128)) * (1/256)`,
//! 双纹理 `YS_Texture1`(本层)/`YS_Texture2`(配对层)在 EnterPass(1) 下取样。
//!
//! 任务 J2 追加取证(`docs/title-motion-ledger.md` EVID-T-03..09):
//! - 双纹理门控 = 配对层指针非空 && `TA >= 1`(asm `0x415558-0x415577`);
//!   配对缺失或 `TA<1` 回退单纹理普通层(微场景 `j2-micro-trans1` TR7)。
//! - `T` 由脚本推进(`es_Sprite.yst` 的 `@TP`);`+0x1e8` 只在调试组 RT
//!   分支使用,不是过渡推进者。
//! - 配对纹理的 **G 通道**是 mask(微场景 `j2-micro-transA`);覆盖度模型见
//!   [`crate::layer::transition_coverage`]。
//! - 本语料 `_DXMODE=0`,走固定管线双纹理路径;HLSL 路径(ps_2_0)公式在
//!   EVID-T-09 备档,不在此契约内。

pub use crate::layer::Transition;

use crate::layer::{Layer, LayerId, TextureRef};

/// 一次双纹理过渡绘制。
#[derive(Clone, Debug, PartialEq)]
pub struct TransitionDraw {
    /// 本层属性(纹理、alpha、blend、变换)。
    pub layer: Layer,
    /// 本层纹理(引擎 `YS_Texture1`)。
    pub from: TextureRef,
    /// 配对纹理(引擎 `YS_Texture2`;`TID` 解析结果,`None` = 无配对)。
    /// 后端按 G 通道作为 mask 取样。
    pub to: Option<TextureRef>,
    /// `YS_FadeVal` 原式计算结果(不 clamp)。
    pub fade: f64,
    /// `T` 原值;`>= 256` 时覆盖度为满(`is_complete`)。
    pub t: i16,
    /// `TA` 原值(审计;`<1` 不会产生本 op)。
    pub ta: i16,
    /// 配对层 ID(原始语义,便于审计)。
    pub tid: Option<LayerId>,
    /// `from` 实际所属的图层(用于取该层**加工后**的纹理)。
    ///
    /// ERIS 精灵系统在 `_DXMODE=0` 下先 `CGACT[REVERSE=1]` 反色 mask 缓冲层
    /// 再交给转场;引擎读的是该层当前像素,因此后端必须把图像加工的覆盖
    /// 纹理也用于过渡取样(EVID-J3-05)。
    pub from_layer: Option<LayerId>,
    /// `to`(mask)实际所属的图层,同上。
    pub to_layer: Option<LayerId>,
}

impl TransitionDraw {
    /// 是否已完成(`T >= 256`):后端直接把覆盖度视为满。
    pub fn complete(&self) -> bool {
        self.t >= 256
    }
}
