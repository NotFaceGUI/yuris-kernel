//! `yscfg.ybn`(魔数 `YSCF`)· 引擎配置(分辨率、扩展名优先序、开关、标题)。
//!
//! 布局(旁线 `format-ybn.md` §6,各字段的引擎校验来自装载器 `FUN_0045dbb0`;
//! ERIS 语料 90 字节闭合,宽 800 × 高 600):
//!
//! ```text
//! +0x00 char[4] "YSCF"
//! +0x04 u32     版本
//! +0x08 u32/u32 头部填充(观测为 0,原样保留)
//! +0x10 u32     画面宽
//! +0x14 u32     画面高
//! +0x18 u32     开关(原样保留)
//! +0x1C u8[8]   图像扩展名优先序:1=png 2=bmp 3=jpg 4=gif 5=psb,0=停
//! +0x24 u8[4]   音频扩展名优先序:1=ogg 2=wav,0=停
//! +0x28 u32     开关(必须 0 或 1;否则引擎报错 0xdbf9c)
//! +0x2C u8[8]   8 个开关(原样保留)
//! +0x34 u8[0x14] 装载器逐个读取的开关(未命名,原样保留)
//! +0x48 u32     观测为 0(原样保留)
//! +0x4C u16     标题字节数(≤ 0x80,否则引擎报错 0xdbf88)
//! +0x4E byte[]  标题(CP932 字节)
//! ```
//!
//! # 宽高的引擎侧校验(0.495 `FUN_0045dbb0`)
//!
//! 反编译显示两字段都用 **32 位**读(`*(int *)(local_40 + 0x10)` /
//! `+0x14`,即 dword),判定是**无符号减法**:
//!
//! ```text
//! if (width != 0 || height != 0)
//!     error if (0x1F80 < (u32)(width  - 0x80)
//!            || 0x1FF0 < (u32)(height - 0x10));
//! ```
//!
//! 等价于:宽 ∈ `0x80..=0x2000`,高 ∈ `0x10..=0x2000`,且**宽高必须同时
//! 为 0 或同时有效**(单独为 0 时减法下溢 → 巨大无符号值 → 报错)。注意
//! 上界是 `0x2000`,不是 `0x1F80`/`0x1FF0` —— 后两者是减法的被比较值。
//! 旧文档把宽度记成 u16、上界记成 0x1F80,均与引擎不符,已按反编译修正。

use crate::container::{read_header, truncated, u16_at, u32_at};
use crate::code;
use yu_log::{Boundary, Diag};

/// 已解码的引擎配置。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Yscf {
    /// 容器头版本(`+0x04`;ERIS 语料 495。R12 起 yu-rt 适配层需要)。
    pub version: u32,
    /// 画面宽(0 = 引擎视作未设置;与高必须同时为 0)。
    pub width: u32,
    /// 画面高(0 = 引擎视作未设置;与宽必须同时为 0)。
    pub height: u32,
    /// `+0x18` 开关(原样保留)。
    pub flag_18: u32,
    /// 图像扩展名优先序(值语义见模块文档)。
    pub image_ext_order: [u8; 8],
    /// 音频扩展名优先序。
    pub sound_ext_order: [u8; 4],
    /// `+0x28` 开关(引擎要求 0 或 1)。
    pub flag_28: u32,
    /// `+0x2C` 起的 8 个开关(原样保留)。
    pub switches: [u8; 8],
    /// `+0x34` 起的未命名开关区(原样保留)。
    pub unknown_34: [u8; 0x14],
    /// `+0x48` 未命名字段(原样保留)。
    pub unknown_48: u32,
    /// 标题(CP932 原始字节)。
    pub title: Vec<u8>,
}

impl Yscf {
    /// 解码 `yscfg.ybn` 字节,执行引擎侧的字段校验。
    pub fn parse(buf: &[u8], origin: &[u8]) -> Result<Self, Diag> {
        let header = read_header(buf, origin, b"YSCF")?;
        if buf.len() < 0x4E {
            return Err(truncated(origin, 0x4E, buf.len(), "YSCF 头部"));
        }
        let width = u32_at(buf, 0x10);
        let height = u32_at(buf, 0x14);
        let flag_28 = u32_at(buf, 0x28);
        let title_len = usize::from(u16_at(buf, 0x4C));
        let check = |bad: bool, at: u64, detail: String| -> Result<(), Diag> {
            if bad {
                Err(Diag::fatal(code::YSCF_FIELD, Boundary::Format)
                    .at_data(origin, at)
                    .with_detail(detail))
            } else {
                Ok(())
            }
        };
        // 引擎:`(width != 0 || height != 0) && (0x1F80 < width-0x80 || 0x1FF0 < height-0x10)`
        // —— 全无符号,宽高任一为 0 而另一非 0 时减法下溢。
        if width != 0 || height != 0 {
            let bad_width = width.wrapping_sub(0x80) > 0x1F80;
            let bad_height = height.wrapping_sub(0x10) > 0x1FF0;
            check(
                bad_width,
                0x10,
                format!("画面宽 {width:#x} 不在 0x80..=0x2000(引擎无符号判定)"),
            )?;
            check(
                bad_height,
                0x14,
                format!("画面高 {height:#x} 不在 0x10..=0x2000(引擎无符号判定)"),
            )?;
        }
        check(
            flag_28 > 1,
            0x28,
            format!("开关 +0x28 = {flag_28},只允许 0/1"),
        )?;
        check(
            title_len > 0x80,
            0x4C,
            format!("标题长度 {title_len} 超过 0x80"),
        )?;
        if 0x4E + title_len > buf.len() {
            return Err(truncated(origin, 0x4E + title_len, buf.len(), "YSCF 标题"));
        }
        Ok(Self {
            version: header.version,
            width,
            height,
            flag_18: u32_at(buf, 0x18),
            image_ext_order: buf[0x1C..0x24].try_into().unwrap(),
            sound_ext_order: buf[0x24..0x28].try_into().unwrap(),
            flag_28,
            switches: buf[0x2C..0x34].try_into().unwrap(),
            unknown_34: buf[0x34..0x48].try_into().unwrap(),
            unknown_48: u32_at(buf, 0x48),
            title: buf[0x4E..0x4E + title_len].to_vec(),
        })
    }
}
