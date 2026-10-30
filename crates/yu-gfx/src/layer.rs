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

    /// 需要**整数像素**的槽读取:整数取原值,浮点四舍五入。
    ///
    /// 只用于语义上确实是整数的槽:层 `Z`(排序键)、层 `SX/SY`(纹理尺寸)。
    /// 位置类(`CG[X/Y]`、`CGACT` 源区域)一律走 `as_f64` 保留亚像素 ——
    /// 取整会把慢速移动量化成阶梯(2026-10-26 实测)。
    ///
    /// 与 `as_int()` 的区别只在这里:后者遇 `fract()!=0` 返回 `None`
    /// (调用方会当成「参数未给」),本函数把可表示的浮点规约成最近的整数。
    pub fn as_px(&self) -> Option<i64> {
        match self {
            Self::Int(i) => Some(*i),
            Self::Float(f) if f.is_finite() => Some(f.round() as i64),
            _ => None,
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

    /// 归约到**官方固定管线实际执行**的模式。
    ///
    /// 证据(REA 0.495 `FUN_00414ba4`,sha256 `4f5eefa3…`):绘制体按第 5 个
    /// 栈参数(MODE)走 if/else 链,分支只有 `0/1/2/3/5/6/7/9`(早期分派链
    /// `0x4155dd` 的比较集合与之相同);**`4/8/10..18` 落进最后的 `else`,
    /// 一条 D3D 纹理阶段/混合状态都不设**。手册 `cg/cg.html` 列了这些名字
    /// (除算/差分/Overlay..PinLight),但引擎没实现 —— 是
    /// `unsupported-by-official`,不是"手册缺页"。
    ///
    /// 因此内核把这 11 个值按「无模式特有状态」对齐 = 按通常路径绘制,
    /// 而不是把整层丢掉。官方那层的真实像素取决于**上一层留下的 D3D 状态**
    /// (顺序相关),不可复现也不应复现。
    pub const fn engine_effective(self) -> Self {
        match self {
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
            | Self::PinLight => Self::Normal,
            other => other,
        }
    }

    /// D3D9 固定管线状态(最终状态,后写覆盖先写)。
    ///
    /// 状态序列逐条取自 `FUN_00414ba4` 对应分支;数值是 D3D9 原始枚举,
    /// 见 `docs/render-parity.md` §3。归约到
    /// [`BlendMode::engine_effective`] 之后每种模式都有状态:官方无 case 的
    /// 模式取通常路径(见该方法文档)。
    pub fn fixed_state(self) -> Option<FixedPipelineState> {
        use BlendFactor as B;
        use BlendOp as O;
        use StageArg as A;
        use StageOp as P;

        let disable = StageState::disabled();
        Some(match self.engine_effective() {
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
            // 官方固定管线**没有 case**(绘制体 `FUN_00414ba4` 的 MODE 分支
            // 只有 0/1/2/3/5/6/7/9),手册名仅文档 ⇒ 内核按通常路径绘制
            // 对齐(见 `engine_effective`)。这不是"待对拍",是官方无实现。
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
            | Self::PinLight => BlendConfidence::Unsupported,
        }
    }

    /// 是否需要着色器路径。
    ///
    /// **恒 `false`**:官方 0.495 的 3 个 ps_2_0 blob 是转场/缩放 pass
    /// (`YS_ShaderTest1` / `YS_TexDiffU` / `YS_TexSize`),不是 Photoshop
    /// 混合模式;MODE 4/8/10..18 在固定管线上也没有 case。保留本方法只为
    /// 调用点(`json.rs` 的 `"shader"/"fixed"` 字段)兼容。
    #[allow(clippy::unused_self)]
    pub const fn needs_shader(self) -> bool {
        // 目前没有任何模式走 shader;将来若有,在这里加分支。
        false
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
    /// **官方引擎没有这个模式的实现**:手册 `cg/cg.html` 列了名字、编译器
    /// 接受该值,但绘制体(`FUN_00414ba4`)里没有对应分支。内核按通常路径
    /// 绘制对齐,不丢层。
    ///
    /// 与 `Likely` 的区别:后者是"照官方状态画、只差像素对拍";这里是
    /// "官方根本没有状态可照",归 `unsupported-by-official`。
    Unsupported,
}

impl BlendConfidence {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Confirmed => "confirmed",
            Self::Likely => "likely",
            Self::Unverified => "unverified",
            Self::Unimplemented => "unimplemented",
            Self::Unsupported => "unsupported",
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

/// 图层的仿射/滤镜字段,逐项对应引擎 0.495 图层结构。
///
/// 绑定证据(REA 0.495 CG 处理体 `0x42a514` 的属性块;槽号 = 命令参数声明下标,
/// 处理体按 `[0x684160 + 声明下标] → slot → [slot*8 + 0x683740]` 取值):
///
/// | 参数 | 写入点 | 字段 | 类型 |
/// |---|---|---|---|
/// | `X` | `0x42aa9c` | `+0x1b0` | i64(`X*100`) |
/// | `FX` | `0x42ab3b` | `+0x1b0` | i64(直写) |
/// | `Y`/`FY` | `0x42abf0`/`0x42ac82` | `+0x1b8` | i64 |
/// | `FBX` | `0x42b10f` | `+0x1c0` | i64 |
/// | `FBY` | `0x42b16f` | `+0x1c8` | i64 |
/// | `FCX` | `0x42b394` | `+0x1d0` | i64 |
/// | `FCY` | `0x42b3f4` | `+0x1d8` | i64 |
/// | `FEX` | `0x42b454` | `+0x1e0` | i64 |
/// | `FEY` | `0x42b4b4` | `+0x1e8` | i64 |
/// | `FEZ` | `0x42b514` | `+0x1f0` | i64 |
/// | `FRX` | `0x42b1ff` | `+0x1f8` | f32 |
/// | `FRY` | `0x42b296` | `+0x1fc` | f32 |
/// | `FRZ` | `0x42b32d` | `+0x200` | f32 |
/// | `FSD` | `0x42a798` | `+0x204` | i32 |
/// | `FMD` | `0x42b583` | `+0x208` | i32 |
/// | `FQU` | `0x42b5ac` | `+0x20c` | i8 |
/// | `F` | `0x42b63a` | `+0x214` | i16 |
///
/// 绘制侧(`FUN_00414ba4` 行 117-125)把 `+0x1b0/1b8/1c0/1c8/1d0/1d8` 与
/// `+0x1f8/1fc/200` 统一 `/100.0`;`+0x1e0/1e8` 在 `0x414031/0x41403b`
/// 另按 i64 读取。
///
/// `X`/`FX`、`Y`/`FY` 两行只说明写入点:位置本身存在 `Transform::x/y`
/// (像素口径 = 引擎 `+0x68`)。`CG[X=n]` 写 `+0x68=n`、`+0x1b0=n*100`;
/// `CG[FX=n]` 写 `+0x1b0=n`、`+0x68=n/100`。本结构的其余字段一律存
/// **引擎 `+0x1xx` 原值**(百分位 / 角度×100 / 位掩码),不做换算。
#[derive(Clone, Debug, PartialEq)]
pub struct Affine {
    /// `FBX`/`FBY`:基准缩放百分位(`+0x1c0`/`+0x1c8`)。
    pub bx: f64,
    pub by: f64,
    /// `FCX`/`FCY`:旋转中心(`+0x1d0`/`+0x1d8`)。
    pub cx: f64,
    pub cy: f64,
    /// `FRX`/`FRY`/`FRZ`:旋转角(度;`+0x1f8/1fc/200`,引擎存 f32)。
    pub rx: f64,
    pub ry: f64,
    pub rz: f64,
    /// `FEX`/`FEY`/`FEZ`:附加偏移(`+0x1e0/1e8/1f0`)。
    pub ex: f64,
    pub ey: f64,
    pub ez: f64,
    /// `FSD`:滤镜强度(`+0x204`)。
    pub sd: i32,
    /// `FMD`:滤镜模式(`+0x208`)。
    pub md: i32,
    /// `FQU`:滤镜质量(`+0x20c`,引擎按有符号字节存取)。
    pub qu: i8,
    /// `F`:滤镜开关位(`+0x214`,引擎按有符号字存取)。
    pub flags: i16,
}

impl Affine {
    /// 引擎层构造 `FUN_00465730` 的初值:`+0x1c0/+0x1c8` = 10000
    /// (`0x465771`/`0x46577d`,绘制侧 ÷100 = 100%),`+0x204` = 0x80
    /// (`0x465789`),`+0x208` = 0x808080(`0x465793`),其余零初始化。
    pub const fn engine_default() -> Self {
        Self {
            bx: 10000.0,
            by: 10000.0,
            cx: 0.0,
            cy: 0.0,
            rx: 0.0,
            ry: 0.0,
            rz: 0.0,
            ex: 0.0,
            ey: 0.0,
            ez: 0.0,
            sd: 0x80,
            md: 0x0080_8080,
            qu: 0,
            flags: 0,
        }
    }

    /// 是否仍是引擎初值(用于判断本层是否带仿射/滤镜)。
    pub fn is_default(&self) -> bool {
        *self == Self::engine_default()
    }
}

impl Default for Affine {
    fn default() -> Self {
        Self::engine_default()
    }
}

/// 图层变换:引擎单位的位置 + 仿射/滤镜字段。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Transform {
    /// `CG[X=...]` 的**原始浮点**像素位置。
    ///
    /// 引擎 D3D9 顶点是 `float`,脚本 `MATH3D` 逐帧算出的位置本身就是浮点
    /// (实测推近镜头 `X=260.71190987262014 → 261.79777521694757`,每帧
    /// 只走 1.08px)。若在写入时四舍五入成整数,0.3px/帧的位移会被量化成
    /// 「一帧不动、下一帧跳 1px」的阶梯 —— 观感就是「移动像格子一样」。
    /// 所以这里保留浮点,量化只发生在后端顶点构造(与官方一致的 float)。
    pub x: f64,
    /// `CG[Y=...]` 原始浮点像素位置(理由同 `x`)。
    pub y: f64,
    /// `FX/FBX/FRX/...` 仿射与滤镜字段(引擎 `+0x1b0..+0x214`)。
    pub affine: Affine,
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

/// 过渡覆盖度:配对纹理的 **G 通道**是 mask,`fade` 是推进量,`ta` 决定软硬。
///
/// 单一公式(不分档):
///
/// ```text
/// c     = 1 - 2*fade                      等值线(随推进从 1 扫到 0)
/// K     = 30 + 70*fade                    等值线附近的**亚像素**过渡(1/K ≈ 0.02..0.033)
/// cell  = 0.16 / TA^2                     官方的「一格」融合带(`+0xb0` 分块表)
/// a     = clamp((m - c) / (1/K + cell), 0, 1)
/// ```
///
/// 官方 `FUN_00414ba4` 把层按 `+0x130`(部件)×`+0xb0`(分块)切成 quad 网格,
/// 每格顶点带累计覆盖比例、格内 16.16 定点插值(比例项 `*0.25`),所以
/// **柔带宽度 ≈ 一格**:`TA` 越大格越密越贴等值线(脚本注释「くっきり」),
/// 越小格越粗、边界越宽。这里用「亚像素带 + 一格带」相加表达同一件事。
///
/// `0.16/TA^2` 的两个锚点:
/// 1. 官方 micro 网格(`j2-micro-trans8`,TA=128)要求 TA=128 的格带几乎为零,
///    否则 `mask=128,T=128` 的 α=0.498 会被推到 0.43(超差 16/255);
/// 2. 整场帧(`\FOUT(1500,5)`,TA=4)实测幕边柔带 ≈40px,掩膜梯度 ≈0.001/px
///    ⇒ ≈0.04 mask 单位;`0.16/4^2 = 0.01` 与亚像素带相加 ≈0.036(≈36px)。
///
/// 真正 1:1 要落地那张 quad 网格(`DIV-GFX-065 §12.3`),此式是行为替身。
///
/// 返回 0..255 的覆盖度(未乘图层 A / 纹理 α)。
pub fn transition_coverage(mask: u8, fade: f64, ta: i16, complete: bool) -> u8 {
    if complete {
        return 255;
    }
    let threshold = 1.0 - 2.0 * fade;
    let slope = 30.0 + 70.0 * fade;
    // `0.16/TA²`:TA=4 → 0.01(与亚像素带相加 ≈0.036,约 36px 融合带);
    // TA=128 → 1e-5(被亚像素带淹没,标题/快门逐位不变)。
    let cell = if ta >= 1 {
        0.16 / (f64::from(ta) * f64::from(ta))
    } else {
        0.0
    };
    let band = 1.0 / slope + cell;
    let value = (f64::from(mask) / 255.0 - threshold) / band;
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
    /// `TEX`:`CG` 写入、`CGINFO[TEX]` 读回的表面类型码。
    ///
    /// 证据(`es_Sprite.yst` **源码**,非编译产物):
    /// - `CG[ID=... TEX=@_DXMODE]` 在建层时写入(`:4091/4144/4396/5280/...`);
    /// - `CGINFO[ID=... TEX=1 LET=@texchk]` 随后读回(`:3454/4155/5848/7770`);
    /// - `IF[@texchk==2]` 的四处分支注释为「E-moteレイヤなら」⇒ **2 = E-mote 层**。
    ///
    /// 语料里**没有任何脚本**把 `TEX` 设成 2(E-mote 由引擎装载时置位);
    /// 本内核 E-mote 属 `deferred(master)`,故该字段只会是 `CG[TEX=]` 的显式
    /// 值(语料中恒为 `@_DXMODE` = 0)。未设置 = 0:层对象零初始化,语义即
    /// 「非 E-mote、非纹理化」。
    pub tex: i64,
    /// 官方 `[层+0x86]`:该层像素**带需要处理的 alpha**(逐层标志)。
    ///
    /// 证据(REA 0.495):
    /// - 文本绘制体 `FUN_00406184` 入口无条件 `mov byte [edx+0x86], 1`
    ///   (`0x406196`)—— 往层里画过字,该层就有有效 alpha;
    /// - `RECTPAINT/2/3` 的处理块在参数值 `>= 2` 时置 1
    ///   (`0x42df5b` 一带:`c6 80 86 00 00 00 01`);
    /// - `RESIZE` 执行体改尺寸后置 1(`0x43c9xx`);
    /// - `CGACT` 图像动作分派 `FUN_0042f809` 把它作为第 9 个参数传给
    ///   模糊执行体(`0x430191`:`movsx ecx, byte [<层>+0x86]`),
    ///   并在收尾把它从源层复制给目标层(`0x430628`)。
    ///
    /// 对模糊族的意义:`BLUR`/`BLUR2/3/4` 在 0 时只平均 R/G/B(alpha 保留),
    /// 在 1 时四通道一起平均;`BLUR6` 在 1 时改为 alpha-only 阴影遮罩
    /// (RGB 归零);`BLUR5` 恒为遮罩、不看该位。
    ///
    /// 未设置 = 0:官方层对象零初始化,`CG[FILE=...]` 新建的层在画字/上色
    /// 之前该位为 0。
    pub alpha_aware: bool,
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

#[cfg(test)]
mod tests {
    use super::transition_coverage;

    /// `TA=128`(标题 logo 浮现、立绘快门)的格带只有 `0.16/128² ≈ 1e-5`,
    /// 被亚像素带(≈0.02)吞掉 ⇒ 与旧实现逐值差 ≤2/255。用户已验收的标题
    /// 动画依赖这个性质。
    #[test]
    fn transition_coverage_keeps_hard_edge_at_ta_128() {
        for &fade in &[0.0f64, 0.127, 0.254, 0.381, 0.5] {
            let threshold = 1.0 - 2.0 * fade;
            let slope = 30.0 + 70.0 * fade;
            for mask in 0..=255u8 {
                let old = {
                    let value = slope * (f64::from(mask) / 255.0 - threshold);
                    if value <= 0.0 {
                        0.0
                    } else if value >= 1.0 {
                        255.0
                    } else {
                        (value * 255.0).round().clamp(0.0, 255.0)
                    }
                };
                let got = f64::from(transition_coverage(mask, fade, 128, false));
                assert!(
                    (got - old).abs() <= 2.0,
                    "TA=128 应与旧式相差 ≤2/255(mask={mask} fade={fade}: 旧 {old} 新 {got})"
                );
            }
        }
    }

    /// `TA=4`(演出 1 最后淡出)是**宽渐变档**:等值线附近必须出现中间值,
    /// 且覆盖区内部仍能到满(不是整屏半透明)。
    #[test]
    fn transition_coverage_is_soft_at_ta_4() {
        let fade = 0.132; // T≈45/256 的 YS_FadeVal(TA=4)
        let threshold = 1.0 - 2.0 * fade;
        let at = |m: f64| transition_coverage((m * 255.0).round() as u8, fade, 4, false);
        // 等值线附近是部分覆盖(u8 量化后可以有几级),带宽 0.04 内单调上升。
        let edge = at(threshold);
        assert!(edge <= 32, "等值线上应是低覆盖,实得 {edge}");
        let mid = at(threshold + 0.02);
        assert!(mid > 10 && mid < 245, "等值线附近应有中间值,实得 {mid}");
        assert_eq!(at(threshold + 0.06), 255, "覆盖区内部必须能到满");
        // 掩膜比阈值低时不覆盖。
        assert_eq!(at((threshold - 0.02).max(0.0)), 0);
    }
}
