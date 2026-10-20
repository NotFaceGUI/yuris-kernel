//! 图层组绘制契约。
//!
//! 手册 `cg/cg.html`「レイヤセットについて」与 `basic/layer.html`:
//! 组先把子层合成到自己的离屏目标,组的 `A`/`MODE` 作用于**合成后的位图**;
//! `E=0` 的整组不显示,但组内子层仍可被 `CGACT[COPY2]` 截图。
//!
//! 引擎证据:`FUN_00413dc8`(`0x413dc8`)先递归子节点再画本节点;
//! 组的离屏目标/合成纹理在 `+0x61c`/`+0x90`(旧线 02 文档,REA 复核)。

use crate::layer::Layer;

/// 把组的离屏合成纹理作为普通层画到父层。
#[derive(Clone, Debug, PartialEq)]
pub struct GroupDraw {
    /// 组自身属性(`A`/`MODE`/`E`/`Z`/变换)。组层没有 `texture`。
    pub layer: Layer,
    /// 组的离屏目标 ID(等于组的 `LayerId`)。
    pub offscreen: crate::layer::LayerId,
}
