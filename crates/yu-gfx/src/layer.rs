//! 图层/混合/变换契约。
//!
//! 证据来源(详见 `docs/render-parity.md`):
//! - 手册 `cg/cg.html`(CG 参数与 MODE 0..18 名称)、`basic/layer.html`(ID/路径/Z/图层组)。
//! - 官方引擎 0.495 `yu-ris.exe`(sha256 `4f5eefa3…`)Ghidra 反编译:
//!   `FUN_00414ba4`(逐层绘制,`0x414ba4`;A 的 clamp、MODE 状态序列、FadeVal 公式)
//!   `FUN_00413dc8`(递归遍历,`0x413dc8`;E/纹理判空与子层先画)。
//! - 真实语料:ERIS 首屏 `CG` 实际参数(`docs/render-parity.md` §2 的 census)。
//!
//! **本模块只保存引擎单位与原始值**:不做颜色归一化、不做几何取整、
//! 不把 `A` 换成 0..1 浮点。后端按 `docs/render-parity.md` 的后端契约执行。

use std::collections::BTreeMap;

use yu_rt::value::Value;

/// 图层 ID / 规范化后的路径(CP932 原始字节;引擎按字节比较,大小写敏感)。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LayerId(pub Vec<u8>);

impl LayerId {
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Self(bytes.into())
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn as_str_lossy(&self) -> std::borrow::Cow<'_, str> {
        String::from_utf8_lossy(&self.0)
    }

    /// 稳定显示形式:可打印 ASCII 原样,其余字节 `\xNN`。
    pub fn display(&self) -> String {
        let mut s = String::new();
        for &b in &self.0 {
            if (0x20..0x7f).contains(&b) {
                s.push(b as char);
            } else {
                s.push_str(&format!("\\x{b:02x}"));
            }
        }
        s
    }
}

/// 引擎标量(参数原值)。字符串保留 CP932 字节。
#[derive(Clone, Debug, PartialEq)]
pub enum EngineValue {
    Int(i64),
    Float(f64),
    Str(Vec<u8>),
}

impl EngineValue {
    pub fn from_value(v: &Value) -> Self {
        match v {
            Value::Int(i) => Self::Int(*i),
            Value::Float(f) => Self::Float(*f),
            Value::Str(b) => Self::Str(b.clone()),
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(i) => Some(*i),
            Self::Float(f) if f.fract() == 0.0 => Some(*f as i64),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Int(i) => Some(*i as f64),
            Self::Float(f) => Some(*f),
            Self::Str(_) => None,
        }
    }

    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Str(b) => Some(b),
            _ => None,
        }
    }
}

/// 引擎屏幕/离屏尺寸(像素)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SizeU32 {
    pub width: u32,
    pub height: u32,
}

/// 引擎整数矩形(左上 + 宽高,像素,不做半像素修正)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RectI {
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
}

/// 纹理引用:后端句柄/资产 key,**不是像素字节或 GPU 类型**。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextureRef {
    /// 逻辑资产路径(CP932 原始字节;不在这里做扩展名探测)。
    Asset(Vec<u8>),
    /// `FILE=""`:手册 `cg/cg.html` 「1x1 サイズの黒画像生成」。
    SolidBlack1x1,
    /// 图层组的实时离屏合成纹理(组自身不落盘)。
    Offscreen(LayerId),
    /// `CGACT[COPY/COPY2]` 或 `CGACT[TEXCOPY]` 产生的纹理快照。
    Snapshot(CopyId),
    /// 尚未解析(例如只给了 `RID`/`MID` 的层)。
    None,
}

/// 纹理快照编号(稳定、单调;只在本帧内有效)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CopyId(pub u64);

/// 图像格式(引擎 `yscfg` 扩展名探测码)。
///
/// 证据:`FUN_0040c9b4`(`0x40c9b4`)按 `DAT_00801600` 的字节查表:
/// `1→".png"`、`2→".bmp"`、`3→".jpg"`、`4→".gif"`;手册 `cg/cg.html`
/// 给出默认顺序 `.PNG .BMP .JPG .GIF`。ERIS `yscfg` 的 8 字节为
/// `01 02 03 04 05 00 00 00`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Bmp,
    Jpg,
    Gif,
    /// 引擎码 5:`FUN_0040c9b4` 行 383 明写 `.psb`。
    Psb,
    /// 表内其他未定义码(原样保留)。
    Other(u8),
}

impl ImageFormat {
    pub fn from_code(code: u8) -> Self {
        match code {
            1 => Self::Png,
            2 => Self::Bmp,
            3 => Self::Jpg,
            4 => Self::Gif,
            5 => Self::Psb,
            other => Self::Other(other),
        }
    }

    pub fn extension(self) -> Option<&'static str> {
        match self {
            Self::Png => Some(".png"),
            Self::Bmp => Some(".bmp"),
            Self::Jpg => Some(".jpg"),
            Self::Gif => Some(".gif"),
            Self::Psb => Some(".psb"),
            Self::Other(_) => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Bmp => "bmp",
            Self::Jpg => "jpg",
            Self::Gif => "gif",
            Self::Psb => "psb",
            Self::Other(_) => "other",
        }
    }
}

/// 手册 `cg/cg.html` 的 `MODE=0..18`。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BlendMode {
    Normal,
    Add,
    Subtract,
    Multiply,
    Divide,
    Screen,
    Lighten,
    Darken,
    Difference,
    Exclusion,
    Overlay,
    ColorDodge,
    ColorBurn,
    LinearBurn,
    HardLight,
    SoftLight,
    VividLight,
    LinearLight,
    PinLight,
}

impl BlendMode {
    pub const ALL: [BlendMode; 19] = [
        BlendMode::Normal,
        BlendMode::Add,
        BlendMode::Subtract,
        BlendMode::Multiply,
        BlendMode::Divide,
        BlendMode::Screen,
        BlendMode::Lighten,
        BlendMode::Darken,
        BlendMode::Difference,
        BlendMode::Exclusion,
        BlendMode::Overlay,
        BlendMode::ColorDodge,
        BlendMode::ColorBurn,
        BlendMode::LinearBurn,
        BlendMode::HardLight,
        BlendMode::SoftLight,
        BlendMode::VividLight,
        BlendMode::LinearLight,
        BlendMode::PinLight,
    ];

    pub fn from_code(code: i64) -> Option<Self> {
        Some(match code {
            0 => Self::Normal,
            1 => Self::Add,
            2 => Self::Subtract,
            3 => Self::Multiply,
            4 => Self::Divide,
            5 => Self::Screen,
            6 => Self::Lighten,
            7 => Self::Darken,
            8 => Self::Difference,
            9 => Self::Exclusion,
            10 => Self::Overlay,
            11 => Self::ColorDodge,
            12 => Self::ColorBurn,
            13 => Self::LinearBurn,
            14 => Self::HardLight,
            15 => Self::SoftLight,
            16 => Self::VividLight,
            17 => Self::LinearLight,
            18 => Self::PinLight,
            _ => return None,
        })
    }

    pub const fn code(self) -> u8 {
        match self {
            Self::Normal => 0,
            Self::Add => 1,
            Self::Subtract => 2,
            Self::Multiply => 3,
            Self::Divide => 4,
            Self::Screen => 5,
            Self::Lighten => 6,
            Self::Darken => 7,
            Self::Difference => 8,
            Self::Exclusion => 9,
            Self::Overlay => 10,
            Self::ColorDodge => 11,
            Self::ColorBurn => 12,
            Self::LinearBurn => 13,
            Self::HardLight => 14,
            Self::SoftLight => 15,
            Self::VividLight => 16,
            Self::LinearLight => 17,
            Self::PinLight => 18,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Add => "add",
            Self::Subtract => "subtract",
            Self::Multiply => "multiply",
            Self::Divide => "divide",
            Self::Screen => "screen",
            Self::Lighten => "lighten",
            Self::Darken => "darken",
            Self::Difference => "difference",
            Self::Exclusion => "exclusion",
            Self::Overlay => "overlay",
            Self::ColorDodge => "color_dodge",
            Self::ColorBurn => "color_burn",
            Self::LinearBurn => "linear_burn",
            Self::HardLight => "hard_light",
            Self::SoftLight => "soft_light",
            Self::VividLight => "vivid_light",
            Self::LinearLight => "linear_light",
            Self::PinLight => "pin_light",
        }
    }

    /// 手册日文名(只用于文档/诊断,不进 JSON 契约)。
    pub const fn manual_name(self) -> &'static str {
        match self {
            Self::Normal => "通常",
            Self::Add => "加算",
            Self::Subtract => "減算",
            Self::Multiply => "乗算",
            Self::Divide => "除算",
            Self::Screen => "スクリーン",
            Self::Lighten => "比較(明)",
            Self::Darken => "比較(暗)",
            Self::Difference => "差分",
            Self::Exclusion => "除外",
            Self::Overlay => "オーバーレイ",
            Self::ColorDodge => "覆い焼きカラー",
            Self::ColorBurn => "焼き込みカラー",
            Self::LinearBurn => "焼き込みリニア",
            Self::HardLight => "ハードライト",
            Self::SoftLight => "ソフトライト",
            Self::VividLight => "ビビッドライト",
            Self::LinearLight => "リニアライト",
            Self::PinLight => "ピンライト",
        }
    }

    /// 固定管线状态(仅 0/1/2/3/5/6/7/9;其余返回 `None`)。
    ///
    /// 状态序列逐条取自 `FUN_00414ba4`(`0x414ba4`)对应分支;数值是
    /// D3D9 原始枚举,见 `docs/render-parity.md` §3。
    ///
    /// TODO(DIV-GFX-001/002):MODE 3/5/6/7/9 的辅助纹理内容、MODE
    /// 4/8/10..18 的着色器本体未取证;`fixed_state()` 返回 `None` 或
    /// `confidence()` 标 Unverified,后端不得执行。
    pub fn fixed_state(self) -> Option<FixedPipelineState> {
        use BlendFactor as B;
        use BlendOp as O;
        use StageArg as A;
        use StageOp as P;

        let disable = StageState::disabled();
        Some(match self {
            Self::Normal => FixedPipelineState {
                stages: [
                    StageState {
                        color_op: P::SelectArg1,
                        color_arg1: Some(A::Texture),
                        color_arg2: None,
                        alpha_op: P::Modulate,
                        alpha_arg1: Some(A::Texture),
                        alpha_arg2: Some(A::TFactor),
                    },
                    StageState {
                        color_op: P::SelectArg1,
                        color_arg1: Some(A::Texture),
                        color_arg2: None,
                        // 引擎先置 MODULATE/TEXTURE,TFACTOR,最后置 ALPHAOP=DISABLE。
                        alpha_op: P::Disable,
                        alpha_arg1: None,
                        alpha_arg2: None,
                    },
                ],
                texture_factor: TextureFactorRule::AlphaGray,
                src_blend: B::SrcAlpha,
                dst_blend: B::InvSrcAlpha,
                blend_op: O::Add,
            },
            Self::Add => FixedPipelineState {
                stages: [
                    StageState {
                        color_op: P::SelectArg1,
                        color_arg1: Some(A::Texture),
                        color_arg2: None,
                        alpha_op: P::Modulate,
                        alpha_arg1: Some(A::Texture),
                        alpha_arg2: Some(A::Diffuse),
                    },
                    disable,
                ],
                texture_factor: TextureFactorRule::AlphaGray,
                src_blend: B::SrcAlpha,
                dst_blend: B::One,
                blend_op: O::Add,
            },
            Self::Subtract => FixedPipelineState {
                stages: [
                    StageState {
                        color_op: P::Modulate,
                        color_arg1: Some(A::Diffuse),
                        color_arg2: Some(A::Texture),
                        alpha_op: P::Modulate,
                        alpha_arg1: Some(A::Diffuse),
                        alpha_arg2: Some(A::Texture),
                    },
                    disable,
                ],
                texture_factor: TextureFactorRule::AlphaGray,
                src_blend: B::SrcAlpha,
                dst_blend: B::One,
                blend_op: O::RevSubtract,
            },
            Self::Multiply => FixedPipelineState {
                stages: [
                    StageState {
                        color_op: P::Modulate,
                        color_arg1: Some(A::Diffuse),
                        color_arg2: Some(A::Texture),
                        alpha_op: P::Modulate,
                        alpha_arg1: Some(A::Diffuse),
                        alpha_arg2: Some(A::Texture),
                    },
                    StageState {
                        color_op: P::ModulateAlphaAddColor,
                        color_arg1: Some(A::Current),
                        color_arg2: Some(A::Texture),
                        alpha_op: P::SelectArg1,
                        alpha_arg1: Some(A::Current),
                        alpha_arg2: None,
                    },
                ],
                texture_factor: TextureFactorRule::OpaqueWhite,
                src_blend: B::Zero,
                dst_blend: B::SrcColor,
                blend_op: O::Add,
            },
            Self::Screen => FixedPipelineState {
                stages: [
                    StageState {
                        color_op: P::Modulate,
                        color_arg1: Some(A::Texture),
                        color_arg2: Some(A::Diffuse),
                        alpha_op: P::Modulate,
                        alpha_arg1: Some(A::Texture),
                        alpha_arg2: Some(A::Diffuse),
                    },
                    StageState {
                        color_op: P::ModulateAlphaAddColor,
                        color_arg1: Some(A::Current),
                        color_arg2: Some(A::Texture),
                        alpha_op: P::SelectArg1,
                        alpha_arg1: Some(A::Current),
                        alpha_arg2: None,
                    },
                ],
                texture_factor: TextureFactorRule::AlphaGray,
                src_blend: B::InvDestColor,
                dst_blend: B::One,
                blend_op: O::Add,
            },
            Self::Lighten => FixedPipelineState {
                stages: [
                    StageState {
                        color_op: P::Modulate,
                        color_arg1: Some(A::Texture),
                        color_arg2: Some(A::Diffuse),
                        alpha_op: P::Modulate,
                        alpha_arg1: Some(A::Texture),
                        alpha_arg2: Some(A::Diffuse),
                    },
                    disable,
                ],
                texture_factor: TextureFactorRule::AlphaGray,
                src_blend: B::One,
                dst_blend: B::One,
                blend_op: O::Max,
            },
            Self::Darken => FixedPipelineState {
                stages: [
                    StageState {
                        color_op: P::Modulate,
                        color_arg1: Some(A::Texture),
                        color_arg2: Some(A::Diffuse),
                        alpha_op: P::Modulate,
                        alpha_arg1: Some(A::Texture),
                        alpha_arg2: Some(A::Diffuse),
                    },
                    StageState {
                        color_op: P::ModulateAlphaAddColor,
                        color_arg1: Some(A::Current),
                        color_arg2: Some(A::TFactor),
                        alpha_op: P::SelectArg1,
                        alpha_arg1: Some(A::Current),
                        alpha_arg2: None,
                    },
                ],
                texture_factor: TextureFactorRule::OpaqueWhite,
                src_blend: B::One,
                dst_blend: B::One,
                blend_op: O::Min,
            },
            Self::Exclusion => FixedPipelineState {
                stages: [
                    StageState {
                        color_op: P::Modulate,
                        color_arg1: Some(A::Texture),
                        color_arg2: Some(A::Diffuse),
                        alpha_op: P::Modulate,
                        alpha_arg1: Some(A::Texture),
                        alpha_arg2: Some(A::Diffuse),
                    },
                    disable,
                ],
                texture_factor: TextureFactorRule::AlphaGray,
                src_blend: B::InvDestColor,
                dst_blend: B::InvSrcColor,
                blend_op: O::Add,
            },
            _ => return None,
        })
    }

    /// 取证等级(与 `docs/render-parity.md` 的结论列一致)。
    pub const fn confidence(self) -> BlendConfidence {
        match self {
            Self::Normal => BlendConfidence::Likely,
            Self::Add | Self::Subtract => BlendConfidence::Confirmed,
            // 手册 + 引擎状态表(aux 白/黑、BCA、DIFFUSE A 灰度)一致;
            // 官方逐像素微场景因 YSCom 工具 attach 失败暂缺 ⇒ Likely。
            Self::Multiply | Self::Screen | Self::Lighten | Self::Darken | Self::Exclusion => {
                BlendConfidence::Likely
            }
            Self::Divide
            | Self::Difference
            | Self::Overlay
            | Self::ColorDodge
            | Self::ColorBurn
            | Self::LinearBurn
            | Self::HardLight
            | Self::SoftLight
            | Self::VividLight
            | Self::LinearLight
            | Self::PinLight => BlendConfidence::Unimplemented,
        }
    }

    /// 是否需要着色器路径(4/8/10..18)。
    pub const fn needs_shader(self) -> bool {
        matches!(
            self,
            Self::Divide
                | Self::Difference
                | Self::Overlay
                | Self::ColorDodge
                | Self::ColorBurn
                | Self::LinearBurn
                | Self::HardLight
                | Self::SoftLight
                | Self::VividLight
                | Self::LinearLight
                | Self::PinLight
        )
    }
}

/// 混合模式取证等级。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlendConfidence {
    /// 手册 + 引擎状态 + 真实使用,语义可执行。
    Confirmed,
    /// 手册 + 引擎状态;D3D 阶段级联的可见结果仍待像素对拍。
    Likely,
    /// 固定管线状态表已取证,但辅助纹理/运算本体未取证。
    Unverified,
    /// 只有手册名;着色器本体未定位。
    Unimplemented,
}

impl BlendConfidence {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Confirmed => "confirmed",
            Self::Likely => "likely",
            Self::Unverified => "unverified",
            Self::Unimplemented => "unimplemented",
        }
    }
}

/// D3D9 `D3DTOP_*`(仅本引擎用到的值)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageOp {
    Disable,
    SelectArg1,
    SelectArg2,
    Modulate,
    ModulateAlphaAddColor,
}

impl StageOp {
    pub const fn d3d_code(self) -> u8 {
        match self {
            Self::Disable => 1,
            Self::SelectArg1 => 2,
            Self::SelectArg2 => 3,
            Self::Modulate => 4,
            Self::ModulateAlphaAddColor => 16,
        }
    }
}

/// D3D9 `D3DTA_*`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageArg {
    Diffuse,
    Current,
    Texture,
    TFactor,
}

impl StageArg {
    pub const fn d3d_code(self) -> u8 {
        match self {
            Self::Diffuse => 0,
            Self::Current => 1,
            Self::Texture => 2,
            Self::TFactor => 3,
        }
    }
}

/// D3D9 `D3DBLEND_*`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlendFactor {
    Zero,
    One,
    SrcColor,
    InvSrcColor,
    SrcAlpha,
    InvSrcAlpha,
    DestColor,
    InvDestColor,
}

impl BlendFactor {
    pub const fn d3d_code(self) -> u8 {
        match self {
            Self::Zero => 1,
            Self::One => 2,
            Self::SrcColor => 3,
            Self::InvSrcColor => 4,
            Self::SrcAlpha => 5,
            Self::InvSrcAlpha => 6,
            Self::DestColor => 9,
            Self::InvDestColor => 10,
        }
    }
}

/// D3D9 `D3DBLENDOP_*`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlendOp {
    Add,
    Subtract,
    RevSubtract,
    Min,
    Max,
}

impl BlendOp {
    pub const fn d3d_code(self) -> u8 {
        match self {
            Self::Add => 1,
            Self::Subtract => 2,
            Self::RevSubtract => 3,
            Self::Min => 4,
            Self::Max => 5,
        }
    }
}

/// 一个纹理阶段(最终状态,后写覆盖先写)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StageState {
    pub color_op: StageOp,
    pub color_arg1: Option<StageArg>,
    pub color_arg2: Option<StageArg>,
    pub alpha_op: StageOp,
    pub alpha_arg1: Option<StageArg>,
    pub alpha_arg2: Option<StageArg>,
}

impl StageState {
    pub const fn disabled() -> Self {
        Self {
            color_op: StageOp::Disable,
            color_arg1: None,
            color_arg2: None,
            alpha_op: StageOp::Disable,
            alpha_arg1: None,
            alpha_arg2: None,
        }
    }
}

/// `D3DRS_TEXTUREFACTOR` 的来源规则。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureFactorRule {
    /// `(A<<24)|(A<<16)|(A<<8)|A`,A = `min(max(A,0),255)`。
    AlphaGray,
    /// `0xFFFFFFFF`(MODE 3/7 分支内显式覆盖)。
    OpaqueWhite,
}

/// 固定管线最终状态(逐层每部件下发)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FixedPipelineState {
    pub stages: [StageState; 2],
    pub texture_factor: TextureFactorRule,
    pub src_blend: BlendFactor,
    pub dst_blend: BlendFactor,
    pub blend_op: BlendOp,
}

/// 滤镜/仿射参数(引擎 `FX/FY/FBX/FBY/FRX/FRY/FRZ/FCX/FCY/FEX/FEY/FEZ/FSD/FMD/FQU/FID/F`)。
///
/// 这些参数在 ERIS 里以 **×100 的整数**出现(例:`FX=@fx*100`、`FBX=10000`、
/// `FCX=5000`);引擎绘制函数对 `+0x1b0..+0x200` 的字段统一 `/100.0`,
/// 旋转角再 `×π/180`。**槽位→结构体偏移的逐项绑定尚未取证**(旧线 G1),
/// 因此本结构只保留原值,不解释哪个参数是缩放/旋转/中心。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FilterParam {
    Fx,
    Fy,
    Fbx,
    Fby,
    Frx,
    Fry,
    Frz,
    Fcx,
    Fcy,
    Fex,
    Fey,
    Fez,
    Fsd,
    Fmd,
    Fqu,
    Fid,
    F,
}

impl FilterParam {
    pub const ALL: [FilterParam; 17] = [
        FilterParam::Fx,
        FilterParam::Fy,
        FilterParam::Fbx,
        FilterParam::Fby,
        FilterParam::Frx,
        FilterParam::Fry,
        FilterParam::Frz,
        FilterParam::Fcx,
        FilterParam::Fcy,
        FilterParam::Fex,
        FilterParam::Fey,
        FilterParam::Fez,
        FilterParam::Fsd,
        FilterParam::Fmd,
        FilterParam::Fqu,
        FilterParam::Fid,
        FilterParam::F,
    ];

    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "FX" => Self::Fx,
            "FY" => Self::Fy,
            "FBX" => Self::Fbx,
            "FBY" => Self::Fby,
            "FRX" => Self::Frx,
            "FRY" => Self::Fry,
            "FRZ" => Self::Frz,
            "FCX" => Self::Fcx,
            "FCY" => Self::Fcy,
            "FEX" => Self::Fex,
            "FEY" => Self::Fey,
            "FEZ" => Self::Fez,
            "FSD" => Self::Fsd,
            "FMD" => Self::Fmd,
            "FQU" => Self::Fqu,
            "FID" => Self::Fid,
            "F" => Self::F,
            _ => return None,
        })
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Fx => "FX",
            Self::Fy => "FY",
            Self::Fbx => "FBX",
            Self::Fby => "FBY",
            Self::Frx => "FRX",
            Self::Fry => "FRY",
            Self::Frz => "FRZ",
            Self::Fcx => "FCX",
            Self::Fcy => "FCY",
            Self::Fex => "FEX",
            Self::Fey => "FEY",
            Self::Fez => "FEZ",
            Self::Fsd => "FSD",
            Self::Fmd => "FMD",
            Self::Fqu => "FQU",
            Self::Fid => "FID",
            Self::F => "F",
        }
    }
}

/// 滤镜/仿射参数原值集合(BTreeMap 保证稳定序列化)。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FilterTransform {
    pub values: BTreeMap<FilterParam, EngineValue>,
}

impl FilterTransform {
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn set(&mut self, param: FilterParam, value: EngineValue) {
        self.values.insert(param, value);
    }
}

/// 图层变换:只保存引擎单位的位置;滤镜参数原样保存。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Transform {
    /// `CG[X=...]`,引擎 i64 像素(不在这里做半像素/缩放)。
    pub x: i64,
    /// `CG[Y=...]`,引擎 i64 像素。
    pub y: i64,
    /// `CG[FX/FY/...]` 原值。
    pub filter: FilterTransform,
}

/// 过渡状态(`CG[T=... TA=... TID=...]`)。
///
/// 默认值来自引擎层构造 `FUN_00465730`:`+0x82(T)=0x100`,
/// `+0x84(TA)` 由零初始化保持 0。`TID` 默认空 = 不配对。
#[derive(Clone, Debug, PartialEq)]
pub struct Transition {
    /// `T` 原值(手册 0..256)。
    pub t: i16,
    /// `TA` 原值(手册 0..256;`<1` 时引擎不走过渡 pass)。
    pub ta: i16,
    /// `TID` 配对图层(规范化后的路径;空串 = 无配对)。
    pub tid: Option<LayerId>,
    /// 脚本原样给的 `TID` 字符串(引擎 `+0x4c` 保存原文,CGINFO 查询返回它)。
    pub tid_raw: Option<Vec<u8>>,
}

impl Default for Transition {
    fn default() -> Self {
        Self {
            t: 256,
            ta: 0,
            tid: None,
            tid_raw: None,
        }
    }
}

impl Transition {
    /// 引擎 `YS_FadeVal` 公式(`FUN_00414ba4` 行 905-909):
    /// `T / (256 / (256/TA + 128)) * (1/256)`。
    ///
    /// `TA < 1` = 无过渡(引擎门控),返回 `None`。
    pub fn fade_value(&self) -> Option<f64> {
        if self.ta < 1 {
            return None;
        }
        let ta = f64::from(self.ta);
        let denom = 256.0 / (256.0 / ta + 128.0);
        Some(f64::from(self.t) / denom * (1.0 / 256.0))
    }

    /// 过渡完成(`T >= 256`)。官方微场景 `j2-micro-trans6`:T=256 时
    /// mask=0 也完全可见;该分支优先于 fade 公式(EVID-T-05 §2.1)。
    pub fn is_complete(&self) -> bool {
        self.t >= 256
    }
}

/// 过渡覆盖度:配对纹理的 **G 通道**是 mask,`fade` 是推进量。
///
/// 证据(`docs/title-motion-ledger.md` EVID-T-05 / §2.1):官方 0.495 固定管线
/// 双纹理路径的 4×9 微场景测量(T∈{64,128,192,256} × mask 9 级,
/// TA∈{2,64,128,192} × T 网格)。拟合形式:
///
/// ```text
/// threshold = 1 - 2*fade
/// K         = 30 + 70*fade           (边缘斜率随推进变陡)
/// coverage  = clamp(K * (mask - threshold), 0, 1)
/// T >= 256  => 255
/// ```
///
/// 在 TA=128(本语料 title 的取值)下,三个中点(T=64/128/192)的
/// 官方测量与模型差 ≤ 8/255;`TA!=128` 与 `T≈255` 的残差见 DIV-GFX-045。
///
/// 返回 0..255 的覆盖度(未乘图层 A / 纹理 α)。`mask` 是配对纹理 G 通道。
pub fn transition_coverage(mask: u8, fade: f64, complete: bool) -> u8 {
    if complete {
        return 255;
    }
    let threshold = 1.0 - 2.0 * fade;
    let slope = 30.0 + 70.0 * fade;
    let value = slope * (f64::from(mask) / 255.0 - threshold);
    if value <= 0.0 {
        0
    } else if value >= 1.0 {
        255
    } else {
        (value * 255.0).round().clamp(0.0, 255.0) as u8
    }
}

/// 固定管线路径的层不透明度:`min(max(A,0),255)/255`。
///
/// 证据:`FUN_00414ba4` 行 116/135-140(A 存 s16,clamp 0..255)与
/// 行 149-150(TFACTOR = A 灰度复制);MODE 0/1/2/3/5/6/7/9 走此路径。
/// `A=256` 在固定管线里与 255 等值。
pub fn fixed_pipeline_alpha(a: i16) -> f64 {
    f64::from(a.clamp(0, 255)) / 255.0
}

/// 着色器路径的层不透明度:`A / 256`。
///
/// 证据:`FUN_00414ba4` 行 910-913
/// `SetFloat("YS_AlphaVal", (float)A * (1/256))`;`A=256` 恰为 1.0,
/// 不经过 clamp。两条路径不得混用同一缩放。
pub fn shader_alpha(a: i16) -> f64 {
    f64::from(a) / 256.0
}

/// 图层的一个部件(基图之外的立绘差分/叠加图)。
///
/// 引擎证据:`FUN_00414ba4` 行 155-156(`+0xa6/+0xa7` 部件计数)、
/// 行 200/219/985(`+0x130/+0xb0` 两张 int 表)、行 343-390(逐部件
/// `SetTexture(0, 基址+步进+0x21c)`)。**几何语义未取证**:`raw` 原样
/// 保存引擎整数表,`texture` 只表示部件图;非空 `parts` 的图层在
/// gfx 里标 `Unimplemented`(DIV-GFX-004)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    pub texture: TextureRef,
    /// 引擎部件表的原始 i32 序列(未解释)。
    pub raw: Vec<i32>,
}

/// 图层种类。
///
/// 图像层的 `size` 是引擎层矩形的 `+0x78/+0x7c`:
/// - 手册 `cg/cg.html` 说 `SX/SY` 只对图层组有效;
/// - 但真实 ERIS 用 `CG[FILE="" SX=800 SY=600]` 画全屏黑幕
///   (`es_ERIS.yst:579/600`),引擎层结构确实保存独立宽高
///   (`FUN_00465730` 默认 1,`FUN_0043772c` 按 SX/SY 写回)。
///
/// 因此 `FILE=""` + `SX/SY` 按该尺寸绘制;资源图的 SX/SY 缩放语义未取证,
/// 由 `LayerStatus::Unimplemented` 挡住。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayerKind {
    Image {
        /// 显式绘制尺寸(像素;`None` = 纹理自然尺寸)。
        size: Option<SizeU32>,
    },
    Group {
        /// `SX/SY`(手册:图层组尺寸;`None` = 未指定)。
        size: Option<SizeU32>,
    },
}

/// 图层执行状态(依赖未实现语义的图层由后端跳过)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerStatus {
    Executable,
    Unimplemented(&'static str),
}

/// 一个合成层(图像层或图层组)。
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    /// 规范化路径(`:/W0/ABC`;组没有尾部 `/`)。
    pub id: LayerId,
    /// 创建序号(同 Z 的确定性 tie-break;手册称同 Z 不確定)。
    pub seq: u64,
    /// 引擎 Z(i64;大者在前)。
    pub z: i64,
    /// `A` 原值 0..256(不 clamp、不归一化)。
    pub alpha: i16,
    /// `MODE`。
    pub blend: BlendMode,
    /// `E`(0 = 不显示;组 = 整组不合成到父层,但子层仍可被 COPY 使用)。
    pub visible: bool,
    pub kind: LayerKind,
    pub texture: TextureRef,
    /// 基图之外的部件(首屏为空;引擎多部件几何未取证)。
    pub parts: Vec<Part>,
    pub transform: Transform,
    /// `MID` 外部掩码层。
    pub mask: Option<LayerId>,
    /// `T/TA/TID`。
    pub transition: Option<Transition>,
    /// 范围裁剪(目前只由显式数据填充;组尺寸由离屏 RT 天然裁剪)。
    pub clip: Option<RectI>,
    /// 有效状态(= `texture_status` 优先,否则 `param_status`)。
    pub status: LayerStatus,
    /// 纹理级状态(`CGACT` 复制/图像加工留下)。
    pub(crate) texture_status: LayerStatus,
    /// 属性级状态(`MODE`/`MID`/滤镜/部件等参数留下)。
    pub(crate) param_status: LayerStatus,
    /// 已识别但未解释/非视觉的参数(`LINT`/`LINT2`/`TEX`/...),原样保留。
    pub extra: BTreeMap<String, EngineValue>,
}

impl Layer {
    pub fn is_group(&self) -> bool {
        matches!(self.kind, LayerKind::Group { .. })
    }

    pub fn group_size(&self) -> Option<SizeU32> {
        match self.kind {
            LayerKind::Group { size } => size,
            LayerKind::Image { .. } => None,
        }
    }

    pub fn image_size(&self) -> Option<SizeU32> {
        match self.kind {
            LayerKind::Image { size } => size,
            LayerKind::Group { .. } => None,
        }
    }

    /// 属性更新后重算有效状态。纹理级 Unimplemented 优先(后端不能跳过纹理
    /// 却执行属性)。
    pub(crate) fn refresh_status(&mut self) {
        self.status = match self.texture_status {
            LayerStatus::Executable => self.param_status,
            other => other,
        };
    }

    pub(crate) fn set_texture_status(&mut self, status: LayerStatus) {
        self.texture_status = status;
        self.refresh_status();
    }

    /// 换图/新建:纹理与属性都回到可执行基线。
    pub(crate) fn reset_status(&mut self) {
        self.texture_status = LayerStatus::Executable;
        self.param_status = LayerStatus::Executable;
        self.refresh_status();
    }
}
