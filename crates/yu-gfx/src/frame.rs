//! `CompositorFrame`:后端唯一需要执行的“画什么”契约。
//!
//! 设计约束:
//! - 字段顺序固定、无散列表迭代序、无墙钟/线程/绝对路径;
//! - 纹理是资产 key/离屏句柄,不是字节;
//! - alpha/颜色/几何保留引擎单位(`A` 0..256、`X/Y` i64、滤镜参数原值);
//! - `layers` 是完整状态快照(含不可见层),`passes` 是绘制列表。
//!
//! 证据与后端执行细节见 `docs/render-parity.md`。

use std::collections::BTreeMap;

use crate::error::FrameDiag;
use crate::layer::{ImageFormat, Layer, LayerId, SizeU32};
use crate::pass::Pass;
use crate::text::{TextDraw, TextLayoutFrame, TextRun};

/// 视口(引擎像素;来自工程 `yscfg` 或宿主注入)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
}

impl Viewport {
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }
}

/// 清屏颜色与来源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clear {
    /// RGBA8,按存储值(不做线性化)。
    pub rgba: [u8; 4],
    pub source: ClearSource,
}

impl Clear {
    /// 引擎未逐帧显式清屏;D3D9 设备初始化后可见背景为不透明黑。
    /// 这是 Let-likely 决策,写入 `docs/render-parity.md` 的 divergence。
    ///
    /// TODO(DIV-GFX-010):引擎帧入口无 `Clear` 证据;官方截图/设备状态
    /// 未采集前保持 Likely。
    pub const fn default_black() -> Self {
        Self {
            rgba: [0, 0, 0, 255],
            source: ClearSource::EngineDefaultBlack,
        }
    }

    pub const fn explicit(rgba: [u8; 4]) -> Self {
        Self {
            rgba,
            source: ClearSource::Explicit,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClearSource {
    /// 引擎默认背景(未在帧入口显式 Clear;旧线 01 §3 指出 Clear 只在调试分支)。
    EngineDefaultBlack,
    /// 调用方显式设定。
    Explicit,
}

/// 颜色空间。
///
/// 引擎是 D3D9 固定管线/HLSL,渲染路径未设置 `D3DRS_SRGBWRITEENABLE`
/// (反编译全树无 state 194 调用),因此混合发生在**存储值空间**(sRGB 编码值
/// 直接参与算术),不是线性空间。见 `docs/render-parity.md` §10。
///
/// TODO(DIV-GFX-009):未用官方截图逐像素验证;保持 Likely。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorSpace {
    /// 8-bit sRGB 编码存储,gamma 空间混合(D3D9 默认)。
    GammaEncodedSrgb8,
}

impl ColorSpace {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GammaEncodedSrgb8 => "gamma_encoded_srgb8",
        }
    }
}

/// 引擎纹理过滤设置(`0=POINT / 1=LINEAR / 2=ANISOTROPIC`;MIP 另有 `-1=NONE`)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterMode {
    Point,
    Linear,
    Anisotropic,
}

impl FilterMode {
    pub fn from_code(code: u8) -> Option<Self> {
        Some(match code {
            0 => Self::Point,
            1 => Self::Linear,
            2 => Self::Anisotropic,
            _ => return None,
        })
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Point => "point",
            Self::Linear => "linear",
            Self::Anisotropic => "anisotropic",
        }
    }
}

/// MIP 过滤(引擎 `-1 = NONE`)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MipFilter {
    None,
    Point,
    Linear,
    Anisotropic,
}

impl MipFilter {
    pub fn from_code(code: i8) -> Option<Self> {
        Some(match code {
            -1 => Self::None,
            0 => Self::Point,
            1 => Self::Linear,
            2 => Self::Anisotropic,
            _ => return None,
        })
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Point => "point",
            Self::Linear => "linear",
            Self::Anisotropic => "anisotropic",
        }
    }
}

/// 过滤设置来源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterSource {
    /// 宿主从引擎设置注入。
    EngineSettings,
    /// 占位值(`GfxConfig::new` 默认);后端不得把它当成已取证设置。
    Placeholder,
}

impl FilterSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EngineSettings => "engine_settings",
            Self::Placeholder => "placeholder",
        }
    }
}

/// 全局纹理过滤(来自引擎设置,逐帧下发到 stage 0/1;不是逐层属性)。
///
/// 证据:`FUN_0043cd6c`(`0x43cd6c`)帧入口对 stage 0/1 各下发
/// `D3DSAMP_MAGFILTER/MINFILTER/MIPFILTER`,值来自
/// `DAT_005b4804`(MAG)/`DAT_005b4808`(MIN)/`DAT_0080131c`(MIP)。
/// 旧线“固定 Linear”是已知 ❌,不继承。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextureFilter {
    pub mag: FilterMode,
    pub min: FilterMode,
    pub mip: MipFilter,
    pub source: FilterSource,
}

impl TextureFilter {
    /// 宿主注入的引擎设置。
    pub const fn engine_settings(mag: FilterMode, min: FilterMode, mip: MipFilter) -> Self {
        Self {
            mag,
            min,
            mip,
            source: FilterSource::EngineSettings,
        }
    }

    /// 默认值未知(引擎设置工具写入);用显式“未设置”占位。
    /// 后端看到 `source=placeholder` 时不得声称已对齐过滤语义。
    pub const fn unknown() -> Self {
        Self {
            mag: FilterMode::Point,
            min: FilterMode::Point,
            mip: MipFilter::None,
            source: FilterSource::Placeholder,
        }
    }
}

/// 一个窗口的状态(手册 `window/window.html`;`NO=0` = 主窗口)。
///
/// gfx 只对**主窗口**执行 Screen 合成;`1..3` 的状态原样输出给
/// window/host 会话(DIV-GFX-018)。`X/Y/SX/SY` 是窗口几何,不是图层偏移。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowState {
    /// `NO`(0..3)。
    pub number: u8,
    /// `CGID` 指定的显示图层组;`None` = 未指定(主窗口回退到
    /// `GfxConfig::main_layer_set`)。
    pub layer_set: Option<LayerId>,
    /// `E`/`A`(β4 的 `A` 只表示显示/非显示)。
    pub visible: bool,
    /// `A` 原值 0..256。
    pub alpha: i16,
    pub x: i64,
    pub y: i64,
    /// `SX/SY`。
    pub size: Option<SizeU32>,
    /// `Z` 窗口叠放(0 前面 / 1 常前 / 2 最背 / 3 主窗口后);gfx 只记录。
    pub z: i64,
}

impl WindowState {
    pub fn new(number: u8, layer_set: Option<LayerId>) -> Self {
        Self {
            number,
            layer_set,
            visible: true,
            alpha: 256,
            x: 0,
            y: 0,
            size: None,
            z: 0,
        }
    }
}

/// 一帧的合成契约。
#[derive(Clone, Debug, PartialEq)]
pub struct CompositorFrame {
    /// 逻辑帧号(来自 `VmEvent` 的 frame)。
    pub frame: u64,
    pub viewport: Viewport,
    /// 引擎 `@_DXMODE` 的宿主注入值(0 = 固定管线路径)。
    ///
    /// 注意:官方引擎没有名为 `_DXMODE` 的渲染开关;该变量只改脚本分支
    /// (`CGACT[TRANSITION]`/`TID` 与 `DXBUF`)。本字段是内核把脚本开关
    /// 映射为“请求 HLSL 转场路径”的显式契约,默认 0 不改变现有工程行为。
    /// 证据与范围见 `docs/shader-parity.md` §3/§6。
    pub dx_mode: u32,
    pub clear: Clear,
    pub color_space: ColorSpace,
    pub filter: TextureFilter,
    /// 图像扩展名探测顺序(来自 `yscfg` 的 8 字节;`Other` 保留原码)。
    pub image_probe_order: Vec<ImageFormat>,
    /// 窗口状态(按 `NO` 升序;`NO=0` 主窗口)。
    pub windows: Vec<WindowState>,
    /// 完整图层状态(含不可见层),按 `(z, seq, id)` 稳定排序。
    pub layers: Vec<Layer>,
    /// 绘制列表(只含可见/需要执行的 op)。
    pub passes: Vec<Pass>,
    /// 本帧待显示文本(跨帧文本缓冲的快照;DIV-GFX-034)。
    pub text: Vec<TextRun>,
    /// 自动文本层排版结果(`_TEXT_FLAG` 逐字步进;缺失 ⇒ `None`)。
    pub text_layout: Option<TextLayoutFrame>,
    /// `CGACT[TEXT=1]` 绘制项(按目标层 ID 稳定排序;层内保持命令序)。
    pub text_draws: Vec<TextDraw>,
    /// 每层 `text_draws` 修订号(`CGACT[TEXT]` 内容每次实际改动 +1)。
    ///
    /// 后端用「层 + 修订号 + 基面身份」缓存文本合成结果:官方文本烘焙
    /// 后不随帧变化(P8/DIV-GFX-058),逐帧重烤是纯浪费。
    pub text_revisions: BTreeMap<LayerId, u64>,
    /// 本帧诊断(按 `(code, command, detail)` 稳定排序)。
    pub diagnostics: Vec<FrameDiag>,
}

impl CompositorFrame {
    pub fn new(frame: u64, viewport: Viewport) -> Self {
        Self {
            frame,
            viewport,
            dx_mode: 0,
            clear: Clear::default_black(),
            color_space: ColorSpace::GammaEncodedSrgb8,
            filter: TextureFilter::unknown(),
            image_probe_order: default_probe_order(),
            windows: Vec::new(),
            layers: Vec::new(),
            passes: Vec::new(),
            text: Vec::new(),
            text_layout: None,
            text_draws: Vec::new(),
            text_revisions: BTreeMap::new(),
            diagnostics: Vec::new(),
        }
    }
}

/// 手册 `cg/cg.html` 的默认扩展名顺序 `.PNG .BMP .JPG .GIF`;
/// 真实 `yscfg` 可能覆盖(ERIS = `01 02 03 04 05`)。
pub fn default_probe_order() -> Vec<ImageFormat> {
    vec![
        ImageFormat::from_code(1),
        ImageFormat::from_code(2),
        ImageFormat::from_code(3),
        ImageFormat::from_code(4),
    ]
}

/// `yscfg` 的 8 字节 → 探测顺序。尾部 0 不计入;未知码保留为 `Other`。
pub fn probe_order_from_codes(codes: &[u8; 8]) -> Vec<ImageFormat> {
    codes
        .iter()
        .copied()
        .take_while(|&c| c != 0)
        .map(ImageFormat::from_code)
        .collect()
}
