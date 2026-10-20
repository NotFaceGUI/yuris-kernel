//! 渲染 pass:一个目标 + 有序操作。
//!
//! pass 顺序是契约的一部分:
//! 1. `Texture(target)` 的复制/图像加工(pass 按命令顺序);
//! 2. 组的离屏 pass(后序:子组先于父组);
//! 3. 屏幕 pass(最后)。
//!
//! 后端不得重排;可以在不改变可见结果的前提下去重/缓存。

use crate::frame::Clear;
use crate::layer::LayerId;
use crate::op::Op;

/// pass 目标。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PassTarget {
    /// 最终屏幕(视口尺寸 = `CompositorFrame.viewport`)。
    Screen,
    /// 图层组的离屏目标。
    Offscreen(LayerId),
    /// 某个图层的纹理(复制/图像加工,不直接呈现)。
    Texture(LayerId),
}

impl PassTarget {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Screen => "screen",
            Self::Offscreen(_) => "offscreen",
            Self::Texture(_) => "texture",
        }
    }
}

/// 一个 pass。
#[derive(Clone, Debug, PartialEq)]
pub struct Pass {
    pub target: PassTarget,
    pub ops: Vec<Op>,
    /// 离屏目标的清屏;屏幕 pass 用 `CompositorFrame.clear`。
    pub clear: Option<Clear>,
}

impl Pass {
    pub fn new(target: PassTarget) -> Self {
        Self {
            target,
            ops: Vec::new(),
            clear: None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
}
