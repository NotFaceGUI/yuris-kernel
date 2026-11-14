//! `yscfg.ybn`(魔数 `YSCF`)· 引擎配置(分辨率、扩展名优先序、开关、标题)。
//!
//! 两个头部布局,由**标题长度 + 文件尾精确闭合**判定:
//!
//! - `New`(0.495 ERIS 90 字节):`+0x4C u16 titleLen`,`+0x4E` 标题;
//!   含 3 个 filePriority 字段。
//! - `Old`(v474 商业包 74 字节):`+0x40 u16 titleLen`,`+0x42` 标题;
//!   没有 filePriority 区。引擎侧证据:v474 商业样本引擎 `FUN_004529d4` 直接
//!   读 `+0x40` 的 u16 与 `+0x42` 起的标题,`+0x28..+0x38` 依次为
//!   thread/debugMode/sound/windowResize/windowFrame。
//!
//! ```text
//! +0x00 char[4] "YSCF"
//! +0x04 u32     版本(诊断字段;不参与布局选择)
//! +0x08 u32/u32 头部填充(观测为 0,原样保留)
//! +0x10 u32     画面宽
//! +0x14 u32     画面高
//! +0x18 u32     开关(原样保留)
//! +0x1C u8[8]   图像扩展名优先序:1=png 2=bmp 3=jpg 4=gif 5=psb,0=停
//! +0x24 u8[4]   音频扩展名优先序:1=ogg 2=wav,0=停
//! +0x28 u32     thread(必须 0 或 1;否则引擎报错 0xdbf9c)
//! +0x2C u8[8]   开关(原样保留)
//! +0x34 u8[]    装载器开关区(New 0x14 字节 / Old 0x0C 字节;原样保留)
//! [New] +0x48 u32 观测为 0;+0x4C u16 titleLen;+0x4E 标题
//! [Old] +0x40 u16 titleLen;+0x42 标题
//! ```
//!
//! # 宽高的引擎侧校验(0.495 `FUN_0045dbb0`;v474 `FUN_004529d4` 同型)
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

use crate::code;
use crate::container::{read_header, truncated, u16_at, u32_at};
use yu_log::{Boundary, Diag};

/// YSCF 头部布局。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum YscfLayout {
    /// 0.495:titleLen @ 0x4C,title @ 0x4E;含 filePriority 区。
    New,
    /// v474:titleLen @ 0x40,title @ 0x42;无 filePriority 区。
    Old,
}

impl YscfLayout {
    fn title_len_off(self) -> usize {
        match self {
            YscfLayout::New => 0x4C,
            YscfLayout::Old => 0x40,
        }
    }

    fn title_off(self) -> usize {
        self.title_len_off() + 2
    }

    pub fn as_str(self) -> &'static str {
        match self {
            YscfLayout::New => "new",
            YscfLayout::Old => "old",
        }
    }
}

/// 已解码的引擎配置。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Yscf {
    /// 容器头版本(`+0x04`;诊断字段,不参与布局选择)。
    pub version: u32,
    /// 判定出的头部布局。
    pub layout: YscfLayout,
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
    /// `+0x2C,+0x30,+0x34,+0x38,+0x3C,+0x40,+0x44` 七个 u32 原始值。
    ///
    /// REA `FUN_0045d950` 分别拷到 `DAT_00802888/802898/8028bc/8028c0/8028c4/8028c8/8028cc`;
    /// 这些是文件优先级一类的配置，**不是** `DAT_0080288c` 模式位。
    /// 本层只保留原始值供 trace，不据此改源顺序。
    pub file_priority_raw: [u32; 7],
    /// `+0x34` 起的未命名开关区(原样保留)。
    pub unknown_34: [u8; 0x14],
    /// `+0x48` 未命名字段(原样保留)。
    pub unknown_48: u32,
    /// 标题(CP932 原始字节)。
    pub title: Vec<u8>,
}

impl Yscf {
    /// 解码 `yscfg.ybn` 字节,执行引擎侧的字段校验。
    ///
    /// `New` / `Old` 两种头部都作为候选;只有"标题长度 + 文件尾精确闭合
    /// (允许尾部零填充)"的布局才被接受。
    pub fn parse(buf: &[u8], origin: &[u8]) -> Result<Self, Diag> {
        let header = read_header(buf, origin, b"YSCF")?;
        let mut first_error = None;
        for layout in [YscfLayout::New, YscfLayout::Old] {
            match parse_with_layout(buf, origin, header.version, layout) {
                Ok(y) => return Ok(y),
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }
        Err(first_error.unwrap_or_else(|| {
            Diag::fatal(code::YSCF_FIELD, Boundary::Format)
                .at_data(origin, 0x10)
                .with_detail("yscfg.ybn 无任何结构闭合的头部布局")
        }))
    }
}

fn parse_with_layout(
    buf: &[u8],
    origin: &[u8],
    version: u32,
    layout: YscfLayout,
) -> Result<Yscf, Diag> {
    let title_len_off = layout.title_len_off();
    let title_off = layout.title_off();
    if buf.len() < title_off {
        return Err(truncated(origin, title_off, buf.len(), "YSCF 头部"));
    }
    let width = u32_at(buf, 0x10);
    let height = u32_at(buf, 0x14);
    let flag_28 = u32_at(buf, 0x28);
    let title_len = usize::from(u16_at(buf, title_len_off));
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
        title_len_off as u64,
        format!("标题长度 {title_len} 超过 0x80"),
    )?;
    let end = title_off
        .checked_add(title_len)
        .ok_or_else(|| limit(origin, title_off as u64, "YSCF 标题长度"))?;
    if end > buf.len() {
        return Err(truncated(origin, end, buf.len(), "YSCF 标题"));
    }
    if buf[end..].iter().any(|&b| b != 0) {
        return Err(Diag::fatal(code::YSCF_FIELD, Boundary::Format)
            .at_data(origin, end as u64)
            .with_detail(format!(
                "{} 布局走完标题后仍有 {} 字节非零尾部",
                layout.as_str(),
                buf.len() - end
            )));
    }

    let (file_priority_raw, unknown_34, unknown_48) = match layout {
        YscfLayout::New => (
            [
                u32_at(buf, 0x2C),
                u32_at(buf, 0x30),
                u32_at(buf, 0x34),
                u32_at(buf, 0x38),
                u32_at(buf, 0x3C),
                u32_at(buf, 0x40),
                u32_at(buf, 0x44),
            ],
            buf[0x34..0x48].try_into().unwrap(),
            u32_at(buf, 0x48),
        ),
        YscfLayout::Old => {
            let mut unknown = [0u8; 0x14];
            unknown[..0x0C].copy_from_slice(&buf[0x34..0x40]);
            ([0u32; 7], unknown, 0)
        }
    };

    Ok(Yscf {
        version,
        layout,
        width,
        height,
        flag_18: u32_at(buf, 0x18),
        image_ext_order: buf[0x1C..0x24].try_into().unwrap(),
        sound_ext_order: buf[0x24..0x28].try_into().unwrap(),
        flag_28,
        switches: buf[0x2C..0x34].try_into().unwrap(),
        file_priority_raw,
        unknown_34,
        unknown_48,
        title: buf[title_off..end].to_vec(),
    })
}

fn limit(origin: &[u8], at: u64, what: &str) -> Diag {
    Diag::fatal(code::LIMIT, Boundary::Format)
        .at_data(origin, at)
        .with_detail(format!("{what} 超出可表示范围"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(len: usize) -> Vec<u8> {
        let mut buf = vec![0u8; len];
        buf[..4].copy_from_slice(b"YSCF");
        buf[4..8].copy_from_slice(&474u32.to_le_bytes());
        buf[0x10..0x14].copy_from_slice(&1280u32.to_le_bytes());
        buf[0x14..0x18].copy_from_slice(&720u32.to_le_bytes());
        buf[0x18..0x1C].copy_from_slice(&1u32.to_le_bytes());
        buf[0x28..0x2C].copy_from_slice(&0u32.to_le_bytes()); // thread
        buf[0x30..0x34].copy_from_slice(&1u32.to_le_bytes()); // sound
        buf[0x34..0x38].copy_from_slice(&1u32.to_le_bytes()); // resize
        buf[0x38..0x3C].copy_from_slice(&1u32.to_le_bytes()); // frame
        buf
    }

    #[test]
    fn old_layout_uses_0x40_caption() {
        let title = b"old-title";
        let mut buf = base(0x42 + title.len());
        buf[0x40..0x42].copy_from_slice(&(title.len() as u16).to_le_bytes());
        buf[0x42..].copy_from_slice(title);

        let cfg = Yscf::parse(&buf, b"v474/yscfg.ybn").unwrap();
        assert_eq!(cfg.layout, YscfLayout::Old);
        assert_eq!((cfg.width, cfg.height), (1280, 720));
        assert_eq!(cfg.title, title);
        assert_eq!(cfg.file_priority_raw, [0u32; 7]);
    }

    #[test]
    fn new_layout_uses_0x4c_caption() {
        let title = b"new-title";
        let mut buf = base(0x4E + title.len());
        buf[0x4C..0x4E].copy_from_slice(&(title.len() as u16).to_le_bytes());
        buf[0x4E..].copy_from_slice(title);

        let cfg = Yscf::parse(&buf, b"v495/yscfg.ybn").unwrap();
        assert_eq!(cfg.layout, YscfLayout::New);
        assert_eq!(cfg.title, title);
    }
}
