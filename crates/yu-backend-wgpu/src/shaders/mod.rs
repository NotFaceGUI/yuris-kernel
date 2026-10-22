//! 引擎着色器 → WGSL 注册表。
//!
//! 每个引擎 pass 一个独立文件;`docs/shader-parity.md` 记录 blob 地址、
//! 参数与触发条件。文件名对应引擎 pass,不是语料名。
//!
//! - `quad.wgsl`:普通图层 quad(含固定管线 stage0/stage1 源色归约)
//! - `transition_fixed.wgsl`:固定管线双纹理过渡(mask G,`_DXMODE=0`)
//! - `transition_psc.wgsl`:引擎 effect Pass1(psC,mask R)
//! - `transition_psd.wgsl`:引擎 effect Pass0(psD,交叉淡化)
//! - `paint.wgsl`:快照 RECTPAINT 族

/// 普通纹理 quad;`SRC_MODE` 由管线常量选择:
/// 0 = `T`(MODE 0/1),1 = `T·A`(MODE 2/5/6/9),
/// 2 = `T·A + (1−C.a)`(MODE 3/7 的两阶段白混)。
pub const QUAD: &str = include_str!("quad.wgsl");

/// 固定管线双纹理过渡(配对纹理 G 通道覆盖度模型)。
pub const TRANSITION_FIXED: &str = include_str!("transition_fixed.wgsl");

/// 引擎 effect Pass1:mask = 配对纹理 R,`saturate((FadeVal-0.5*mask.r)*FadeVal2)`。
pub const TRANSITION_PSC: &str = include_str!("transition_psc.wgsl");

/// 引擎 effect Pass0:`mix(tex1, tex2, FadeVal)` × `YS_AlphaVal`。
pub const TRANSITION_PSD: &str = include_str!("transition_psd.wgsl");

/// 快照上的 RECTPAINT 族(整像素 / 只写 alpha)。
pub const PAINT: &str = include_str!("paint.wgsl");
