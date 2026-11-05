//! 过渡绘制契约(`T`/`TA`/`TID`)。
//!
//! 手册 `cg/cg.html`:`T` 过渡进行度、`TA` 过渡精度、`TID` 过渡图层 ID。
//! 引擎证据:`FUN_00414ba4`(`0x414ba4`)行 905-909 计算
//! `YS_FadeVal = T / (256/(256/TA + 128)) * (1/256)`,
//! 双纹理 `YS_Texture1`(本层)/`YS_Texture2`(配对层)在 effect Pass1 下取样。
//!
//! 官方路径(2026-11-05 定稿,`docs/divergences/DIV-GFX-065`):
//! - 双纹理门控 = 配对层指针非空 && `TA >= 1`(asm `0x415558-0x415577`);
//!   配对缺失或 `TA<1` 回退单纹理普通层(微场景 `j2-micro-trans1` TR7)。
//! - `T` 由脚本推进(`es_Sprite.yst` 的 `@TP`);`+0x1e8` 只在调试组 RT
//!   分支使用,不是过渡推进者。
//! - 引擎始终创建 effect;配对且 `TA>=1` 时 `BeginPass(1)`(psC,asm
//!   `0x414dfa`)——`_DXMODE` 只是脚本变量,不是渲染开关。
//! - psC 取配对纹理 **R 通道**:
//!   `a = tex1.a * sat((YS_FadeVal - 0.5*mask) * TA) * A/256`;
//!   `mask` 按官方 micro 表校准为 `(texel+1)/256`。
//! - `YS_Texture2` 取配对层**基图**(`+0x21c`),不是 `CGACT` 加工表面
//!   (`+0x90`)。

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
    /// 后端按 **R 通道**作为 mask 取样。
    pub to: Option<TextureRef>,
    /// 配对层的**基图**(引擎 `YS_Texture2 = 层+0x21c`)。
    ///
    /// 官方转场取样的是层基图,不是 `CGACT` 加工表面(`+0x90`)。
    /// 配对层本身是 `FILE=""` 的 1×1 空层、图案由
    /// `CGACT[ID=$TRBUF ID2=$TR1 SIZE=1]` 做进表面时,基图取自该
    /// 加工命令的 `source_texture`(图案资产本体);没有基图时后端
    /// 回退 `to` + `to_layer` 的加工表面。
    pub to_base: Option<TextureRef>,
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
