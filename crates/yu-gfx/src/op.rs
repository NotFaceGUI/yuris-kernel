//! 显示列表操作(`Op`)。
//!
//! 后端只执行本枚举;任何语义缺项都用 [`OpStatus::Unimplemented`] 显式标记,
//! 不允许后端“猜”公式或静默跳过。

use std::sync::Arc;

use crate::group::GroupDraw;
use crate::layer::{CopyId, EngineValue, Layer, LayerId, SizeU32, TextureRef};
use crate::text::TextDraw;
use crate::transition::TransitionDraw;

/// 一条绘制/复制/图像操作。
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    /// 普通纹理层。
    Layer(Arc<Layer>),
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
    pub layer: Arc<Layer>,
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

/// 复制源。**类型即后端需要读取的像素来源**，后端不得再按窗口表推断。
///
/// 三种源对应三种互斥的读取契约：
///
/// | 源 | 命令时刻需要什么 | 像素来源 |
/// |---|---|---|
/// | `Layer` | 源层当前 `(layer, epoch)` 表面 + 冻结的 `source_texture`/`texts` | 层表面 |
/// | `Group` | 组内成员层的命令时刻像素 | 逐成员现绘到离屏 |
/// | `Screen` | **上次呈现帧**的真实屏幕；若 `frame.screen_snapshots[copy.id]` 存在则用它 | 屏幕表面 |
///
/// `Screen` 是主窗口 `:/W0/`：官方 `es_Scenario.yst:1174-1182` 的
/// `_DXMODE=0` ELSE 分支直接 `COPY2 :/W0/`。它不是离屏组，也不能零拷贝
/// 别名/延迟物化——屏幕表面每帧被呈现改写，别名会让快照随下一帧变化
/// （冻结语义只允许不可变源，EVID-T-12）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CopySource {
    /// 单个图层表面（命令时刻的层像素）。
    Layer(LayerId),
    /// 普通图层组：取组离屏的当前合成结果。
    Group(LayerId),
    /// 主窗口屏幕 `:/W0/`：上次呈现帧（或命令时刻屏幕快照）。
    ///
    /// payload 只是主窗口组标识（诊断/JSON 兼容）；像素语义是**屏幕**，
    /// 不是离屏组合成 —— 后端按变体匹配，不得再用这个 id 去查窗口表。
    Screen(LayerId),
}

impl CopySource {
    /// 是否为主窗口屏幕（后端只读屏幕表面，不组合成员层）。
    pub const fn is_screen(&self) -> bool {
        matches!(self, Self::Screen(_))
    }

    /// 源标识：`Layer`/`Group` 返回层/组 id；`Screen` 返回其窗口组 id
    /// （像素语义仍是屏幕，仅用于诊断/日志定位）。
    pub fn layer(&self) -> Option<&LayerId> {
        match self {
            Self::Layer(id) | Self::Group(id) | Self::Screen(id) => Some(id),
        }
    }
}

/// 复制区域(引擎 `X/Y/X2/Y2` 原值;`Full` = 未给区域)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyRegion {
    Full,
    /// 源纹理坐标矩形(含端点)。
    ///
    /// 未指定的字段 = **源整幅范围**,不是 0。证据:
    /// - 手册 `cg/cgact.html` 例 29/30 只给部分坐标时仍是整层截图;
    /// - ERIS 按钮切片 `CGACT[ID="BT.OFF.TMP" ID2=$off COPY2=1 X=0 X2=@sx-1]`
    ///   不带 `Y/Y2`,官方 title 按钮是整高切片(不是 1px 高);
    /// - `j2-micro-copy` 显式给全四个坐标,行为不变(EVID-T-11)。
    ///
    /// 旧实现把缺省值填 0,导致只复制首行(`DIV-GFX-054`)。
    Partial {
        x: Option<i64>,
        y: Option<i64>,
        x2: Option<i64>,
        y2: Option<i64>,
    },
}

impl CopyRegion {
    /// 解析为源纹理上的闭区间 `(x0, y0, x1, y1)`。
    ///
    /// 缺省:x0/y0 = 0,x1 = 宽−1,y1 = 高−1。显式负值/越界由调用方裁剪。
    pub fn resolve(&self, width: u32, height: u32) -> (i64, i64, i64, i64) {
        match *self {
            CopyRegion::Full => (
                0,
                0,
                i64::from(width).saturating_sub(1),
                i64::from(height).saturating_sub(1),
            ),
            CopyRegion::Partial { x, y, x2, y2 } => (
                x.unwrap_or(0),
                y.unwrap_or(0),
                x2.unwrap_or_else(|| i64::from(width).saturating_sub(1)),
                y2.unwrap_or_else(|| i64::from(height).saturating_sub(1)),
            ),
        }
    }
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
    /// **命令执行时**源图像层的基础纹理(`CopySource::Layer` 才有)。
    ///
    /// 官方 COPY 在命令到达时立即取像素(EVID-T-15);源层可能在同一帧稍后
    /// 被 `CGEND` 删除(ERIS 按钮快照:`CG[ID="BT.OFF.TMP" FILE=…]` → 切片
    /// `COPY2` → 全部按钮建完后 `CGEND[BT.OFF.TMP]`)。本内核的纹理 pass 在
    /// 帧末回放,那时源节点已不存在;因此必须在命令时刻把基础纹理固定下来
    /// (`DIV-GFX-053`)。
    pub source_texture: Option<TextureRef>,
    /// **命令执行时**源图层已登记的文本绘制项(`CopySource::Layer` 才冻结;
    /// 组源为空 —— 组内层文本由后端按组当前状态合成)。
    ///
    /// 官方 `CGACT[TEXT]` 即时写层表面(DIV-GFX-058),COPY 天然携带已画
    /// 文本;命令之后再画的文本不得进入快照(与 [`Self::source_texture`]
    /// 同一命令时刻语义,EVID-T-15/DIV-GFX-053)。内部字段,不进 JSON
    /// (与 `source_texture` 同先例)。
    pub texts: Vec<TextDraw>,
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
    /// `VERTICAL=1`:垂直镜像(上下翻转),alpha 不变。
    Vertical,
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
            Self::Vertical => "vertical",
            Self::Other(s) => s,
        }
    }

    /// 缩放族:`SIZE`/`SIZE2` 与 `SIZE3`/`SIZE4`/`SIZE5`。
    ///
    /// `SIZE3/4/5` 手册 `cg/cgact.html` 无页,但语料用法与 `SIZE` 完全同形
    /// (同为 `SX/SY` 目标尺寸,可带源区域):
    /// - `es_ERIS.yst:631` `CGACT[ID=$id SIZE4=1 SX=@sx/2 SY=@sy/2]`(CG 半尺寸);
    /// - `es_System.yst:261` `CGACT[ID=$SRCID ID2=$DSTID SIZE4=1 SX=@SSX SY=@SSY]`
    ///   (二分降采样,与 `SIZE2` 交替出现在同一循环里)。
    ///
    /// ⇒ 几何语义(区域 + 目标尺寸)有语料锚点;三者相对 `SIZE2` 的**重采样
    /// 滤波差异**未取证,按双线性执行(与 `SIZE2` 同),DIV-GFX-039 记
    /// `partial(no-manual)`。关键点:不实现会让整层被标 Unimplemented ⇒
    /// CG/立绘永久不上屏。
    pub fn is_scale(&self) -> bool {
        matches!(
            self,
            Self::SizeNearest | Self::SizeBilinear | Self::Other("size3" | "size4" | "size5")
        )
    }

    /// `RESIZE`:改层尺寸、像素不重采样(手册例 21)。
    pub fn is_resize(&self) -> bool {
        matches!(self, Self::Resize)
    }

    /// `TRIM`:裁掉完全透明边缘(手册例 22)。
    pub fn is_trim(&self) -> bool {
        matches!(self, Self::Trim)
    }

    /// `MEIDO`:逐通道明度缩放(手册例 7)。
    pub fn is_meido(&self) -> bool {
        matches!(self, Self::Other("meido"))
    }

    /// 逐层**原地滤镜**族(手册例 5..17 + 无页同族键)。
    ///
    /// 这些动作只改本层像素,不涉及 ID2;判定为可执行后由后端按 `action`
    /// 分派像素算法。`MERGE/MERGE2/XOR/AND` 是**两图层**动作,不在此列。
    pub fn is_in_place_filter(&self) -> bool {
        if matches!(self, Self::Rotation | Self::Resize | Self::Trim) {
            return true;
        }
        matches!(
            self,
            Self::Other(
                "meido"
                    | "saido"
                    | "akarusa"
                    | "mono"
                    | "sepia"
                    | "mosaic"
                    | "blur"
                    | "blur2"
                    | "blur3"
                    | "blur4"
                    | "blur5"
                    | "blur6"
                    | "diffusion"
                    | "whirlpool"
                    | "raster"
                    | "raster1"
                    | "raster2"
                    | "raster3"
                    | "rotation2"
            )
        )
    }

    /// 两图层合成族(`ID` 上层、`ID2` 下层、结果写 `ID2`)。
    ///
    /// `mask_alpha` 不在 `CGACT` 参数表里,但复用同一后端路径:
    /// `CG[MID]` 的官方语义是 `target.a = mask.R`(REA `0x42a88c`)。
    pub fn is_layer_composite(&self) -> bool {
        matches!(
            self,
            Self::Merge | Self::Merge2 | Self::Other("xor" | "and" | "mask_alpha")
        )
    }

    /// 这三种动作会**改变层尺寸**(像素面重建),不只是改像素。
    pub fn changes_extent(&self) -> bool {
        self.is_resize() || self.is_trim()
    }

    /// 走双线性(或更高画质)重采样。
    pub fn is_bilinear_scale(&self) -> bool {
        matches!(
            self,
            Self::SizeBilinear | Self::Other("size3" | "size4" | "size5")
        )
    }
}

/// 命令时刻目标层的基面状态。
///
/// 与 [`ImageOp::source_texture`] 对称:官方 `CGACT` 在命令处理体内立即执行
/// (`FUN_0042f809`),目标层随后被 `CGEND` 删除**不会撤销**已经写入的
/// 像素。延迟回放到帧末时目标层可能已不存在;这里冻结目标层当时的纹理、
/// 实色填充和显式尺寸,后端才能按官方命令时刻重建基面。
///
/// **只在目标层已被删除时使用**。目标层仍存活(包括随后被 `CG[FILE]`
/// 换图)时后端用活层当前像素;否则缩略图/hover 槽位会把上一张图写回来。
///
/// 典型语料(CG 鉴赏预览,`es_cgmode.yst`):
/// `CG[FILE=""] SX/SY` → `RECTPAINT3` 刷白 → `MERGE` 合成 CG → `COPY2`
/// 拷回 → `CGEND` 删除白底。白底在 `MERGE/COPY2` 之后被删,但它的尺寸和
/// 白色填充仍是这两条命令的输入。
#[derive(Clone, Debug, PartialEq)]
pub struct ImageTargetBase {
    /// 目标层命令时刻的纹理。
    pub texture: TextureRef,
    /// `CG[FILE=""]`/整层 `RECTPAINT*` 的实色像素(`SolidBlack1x1` 使用)。
    pub fill: Option<[u8; 4]>,
    /// 目标层命令时刻的显式尺寸(像素;`None` = 纹理自然尺寸)。
    pub size: Option<SizeU32>,
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
    /// 第二源图层(`ID2`;仅 `TRANSITION` 用)。
    ///
    /// 语料唯一用法 `es_Sprite.yst:7978`:
    /// `CGACT[TRANSITION=1 SET=@L.F2(@LNO) ID=$LB1 ID2=$LB2 ID3=$LB3]`
    /// —— `$LB1`=旧图、`$LB2`=新图,混合结果写进 `$LB3`,随后
    /// `es.CGACT_SIZE` 再从 `$LB3` 缩放到可见层。所以这个动作的
    /// 「目标」是 **ID3**,`ID2` 是第二个输入,与本结构其余动作不同。
    pub source2: Option<LayerId>,
    /// 第三源图层(`ID3`;**仅 `TRANSITION2`** 的逐像素遮罩/权重图)。
    ///
    /// 内部字段,与 [`Self::source2`] 同先例不进 display JSON。官方
    /// `TRANSITION2` 执行体 `0x43bca0`:`ID`=源 A、`ID2`=源 B、`ID3`=遮罩、
    /// `ID4`=目标;`SET`=进度、`SET2`=遮罩对比(DIV-GFX-061 补记)。
    pub source3: Option<LayerId>,
    /// `ID2`(复制/加工结果目标;`None` = 原地加工)。
    pub target: Option<LayerId>,
    /// 命令时刻源层的基图(`DIV-GFX-053` 的 image 版)。
    ///
    /// 与 [`CopyOp::source_texture`] 同理由:官方 `CGACT` 在命令到达时就
    /// **立即执行**(REA `0x42f809`:分配新缓冲 → 读源 → 写回),源层随后被
    /// `CGEND` 删除不影响结果。本内核的像素工作推迟到帧末回放,那时源节点
    /// 可能已不存在 —— 记录命令时刻的 `TextureRef` 才能按官方语义重放。
    ///
    /// 语料:`es_SPExec.yst:341` 的
    /// `CGACT[ID="sp.tmpB" ID2=$lid … MERGE=1]` 紧跟
    /// `CGEND[ID="sp.tmpB"]`;不记录会让 MERGE 整条被丢弃,动画帧层只剩
    /// 前一步的 `RECTPAINT3` 清空结果 ⇒ 立绘在眨眼/表情切换时闪空白。
    pub source_texture: Option<TextureRef>,
    /// 命令时刻目标层的基面(纹理/实色填充/显式尺寸)。
    ///
    /// 目标层在后续命令里被 `CGEND` 删除或换图时,它仍是本动作的“下层”。
    /// 官方在命令体内已经完成像素运算,删除不撤销;延迟回放必须用这份快照
    /// 重建目标基面(见 [`ImageTargetBase`])。
    pub target_base: Option<ImageTargetBase>,
    pub action: ImageAction,
    /// `SET` 原值。
    pub set: Option<EngineValue>,
    /// `SET2` 原值。
    pub set2: Option<EngineValue>,
    /// `SETSTR` 原文(CP932 字节)。
    pub set_str: Option<Vec<u8>>,
    /// `SIZE/SIZE2` 的目标宽(手册 `cg/cgact.html` 例 3/4)。
    ///
    /// **浮点**:缩放后的实际尺寸常带小数(实测 `SIZE=1 SX=1101 SY=825.5`)。
    /// 用整数会丢掉 `.5`,并且旧实现因此在 `as_int()` 的 `fract()==0`
    /// 守卫处被判「尺寸缺失」。
    pub sx: Option<f64>,
    /// `SIZE/SIZE2` 的目标高。
    pub sy: Option<f64>,
    /// 源区域左上 X(**浮点**)。
    ///
    /// 精灵系统把位置拆成「整数部分」+「小数部分」两条参数传进来
    /// (`es_Sprite.yst`: `CGACT[… SIZE2=1 FX=@X1*100+@FLX …]`)。取整会让
    /// 慢速移动(每帧 0.1–0.5px)变成「几帧不动再跳 1px」的抖动。
    ///
    /// 注意 `FX/100 = X1 - LEFT_X` 是**相对屏幕位置的偏移**,不是源坐标;
    /// 见 [`Self::region_from_fixed`]。
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub x2: Option<f64>,
    pub y2: Option<f64>,
    /// 源区域是否按「图层位置 + 百分位偏移」给出(`SIZE2` 的 `FX/FY/FX2/FY2`)。
    ///
    /// `es.CGACT_SIZE` 的中画质分支只发定点键:`FX=@X1*100+@FLX`,其中
    /// `@FLX = -(@LEFT_X*100 % 100)`。因此 `FX/100 = X1 - LEFT_X`
    /// (源起点 − 屏幕位置),真实源起点是 `LEFT_X + FX/100`。若直接拿
    /// `FX/100` 当源坐标,UV 窗口整体偏移 `frac(LEFT_X)`,而且逐帧变化 ——
    /// 立绘慢速移动时采样窗口在纹理上来回滑动 = 抽搐 + 边缘 texel 拉伸闪烁。
    ///
    /// 标志为 `true` 时,`frame()` 输出前会把目标层的最终位置加到
    /// `x/y/x2/y2`,后端因此始终只看到绝对源坐标。
    pub region_from_fixed: bool,
    /// 命令时刻源层的官方 `[+0x86]`(见 [`crate::layer::Layer::alpha_aware`])。
    ///
    /// 只有模糊族消费它:官方 `CGACT` 分派把该位作为第 9 个参数传给
    /// `FUN_00431d28`/`43265c`/`432e58`/`433718`/`434748`/`434b7c`
    /// (`0x430191`:`movsx ecx, byte [<层>+0x86]`)。
    pub alpha_aware: bool,
    /// 命令时刻**上层**(`ID`)的图层不透明度 `[层+0x80]`,单位 0..256。
    ///
    /// 只有合并族(`MERGE`/`MERGE2`)消费它。REA 0.495:
    /// - `MERGE` 执行体 `FUN_0043b33c` 一般路径为
    ///   `blend(dst, src, srcLayerAlpha, span)`(`DAT_005d161c`);
    /// - `MERGE2` 执行体 `FUN_0043b8ec` 一般路径为
    ///   `blend(dst, src, dstLayerAlpha, srcLayerAlpha, span)`(`DAT_005d1624`)
    ///   —— **多一个下层的层 alpha**。
    ///
    /// 命令到达时快照:回放期层状态可能已被 `CG[A=…]` 改写。
    pub source_alpha: i16,
    /// 命令时刻**下层**(`ID2`;缺省 = 目标)的图层不透明度 `[层+0x80]`。
    ///
    /// 仅 `MERGE2` 消费;`MERGE` 不吃(见 [`Self::source_alpha`])。
    pub target_alpha: i16,
    /// `RESIZE` 执行前的源表面尺寸(像素)。
    ///
    /// 官方 `FUN_0043c971` 同时改写图层矩形 `+0x78/+0x7c`:
    /// `newW = srcW + X + X2`、`newH = srcH + Y + Y2`。gfx 在命令时刻已经
    /// 用这个基数更新 `LayerKind::Image.size`,以便随后的 `CGINFO[SX/SY]`
    /// 读到新矩形;后端回放时若按**已更新**的矩形重建 1×1 符号表面,会把
    /// 增量再加一遍,所以这里冻结执行前的基数。
    ///
    /// 语料:0.486 `es_Sprite.yst` 的 `es.SP.RESIZE` 对
    /// `CG[FILE=""]` 的空目标连续发四条 `RESIZE`,把 1×1 扩到旧立绘尺寸。
    pub resize_base: Option<(u32, u32)>,
    pub status: OpStatus,
}
