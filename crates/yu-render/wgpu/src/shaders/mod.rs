//! 引擎着色器 → WGSL 注册表。
//!
//! 每个引擎 pass 一个独立文件;`docs/shader-parity.md` 记录 blob 地址、
//! 参数与触发条件。文件名对应引擎 pass,不是语料名。
//!
//! - `quad.wgsl`:普通图层 quad(含固定管线 stage0/stage1 源色归约)
//! - `transition_fixed.wgsl`:固定管线双纹理过渡(mask G,`_DXMODE=0`)
//! - `transition_psc.wgsl`:引擎 effect Pass1(psC,mask R)
//! - `transition_psd.wgsl`:引擎 effect Pass0(psD,交叉淡化)
//! - `scale_psb.wgsl`:引擎 effect Pass2(4-tap 锐利双线性缩放)
//! - `scale_psa.wgsl`:引擎 effect Pass3(16-tap 双三次缩放)
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

/// 引擎 effect Pass2:4-tap 锐利双线性缩放(`DAT_00801334 == 2`)。
pub const SCALE_PSB: &str = include_str!("scale_psb.wgsl");

/// 引擎 effect Pass3:16-tap 双三次缩放(`DAT_00801334 == 3`)。
pub const SCALE_PSA: &str = include_str!("scale_psa.wgsl");

/// 快照上的 RECTPAINT 族(整像素 / 只写 alpha)。
pub const PAINT: &str = include_str!("paint.wgsl");

/// `CGACT[TRANSITION=1]` 双源整数 lerp(GPU;与 soft `blend_transition` 逐位一致)。
pub const LERP: &str = include_str!("lerp.wgsl");

/// `CGACT[MERGE=1]` 的 GPU 整数合成(与 soft `apply_merge(…, false)` 逐位一致)。
pub const MERGE: &str = include_str!("merge.wgsl");

/// `CGACT[MERGE2/XOR/AND]` 的 GPU 合成(lower + upper + mode uniform)。
pub const COMPOSITE: &str = include_str!("composite.wgsl");

/// 逐层原地滤镜族(REVERSE/镜像/MEIDO/SAIDO/AKARUSA/MONO/SEPIA/MOSAIC/ROTATION)。
pub const TRANSFORM: &str = include_str!("transform.wgsl");

/// 区域重采样(`scale_region` 的 GPU 等价;物化 UV 采样修饰)。
pub const RESAMPLE: &str = include_str!("resample.wgsl");
