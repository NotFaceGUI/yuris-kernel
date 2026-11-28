//! `yscfg.ybn`(魔数 `YSCF`)· 引擎配置(分辨率、扩展名优先序、开关、标题)。
//!
//! 四个头部布局,由**宽高范围 + 标题长度 + 文件尾精确闭合**裁决
//! (多个候选同时闭合 = 不唯一 = 失败,不按"第一个能解析的"取):
//!
//! - `New`(0.495 ERIS 90 字节):宽 `+0x10`/高 `+0x14`,`+0x4C u16 titleLen`,
//!   `+0x4E` 标题;含 3 个 filePriority 字段。
//! - `Old`(v459/0.466、v474 商业包 74/78 字节):宽 `+0x10`/高 `+0x14`,
//!   `+0x40 u16 titleLen`,`+0x42` 标题;没有 filePriority 区。引擎侧证据:
//!   v474 商业样本引擎 `FUN_004529d4` 直接读 `+0x40` 的 u16 与 `+0x42`
//!   起的标题。
//! - `V292`(官方 0.292 示例 78 字节):宽 `+0x14`/高 `+0x18`,
//!   `+0x40 u16 titleLen`,`+0x42` 标题。证据:官方 0.292 `yu-ris.exe`
//!   (sha256 `72628dfa…`)装载器 `FUN_0043f3a4` 逐字段读取 +0x14/+0x18/
//!   +0x1C(8 字节扩展名序)/+0x24(4 字节音频序)/+0x28(thread)/
//!   +0x2C、+0x30、+0x34、+0x38(四个 dword)/+0x40(u16 标题长)/+0x42(标题)。
//! - `V263`(0.263 商业样本 `<0.263 商业>`,98 字节):宽 `+0x14`/高 `+0x18`,
//!   `+0x3C u16 titleLen`,`+0x3E` 标题。文件恰好 0x3E+标题长闭合;
//!   `+0x1C` 图像序、`+0x24` 音频序、`+0x28` thread 与 V292 同型,
//!   `+0x2C..+0x3C` 为四个 dword(最后一个在 V292 是 filePriorityRelease,
//!   本布局下该区只剩三个,`+0x3C` 已是标题长)。证据:真实 `yscfg.ybn`
//!   逐字节闭合(98 = 62 + 36,标题长度字段即 +0x3C),见 DIV-010。
//! - `V255`(官方 0.255 示例 62 字节):宽 `+0x14`/高 `+0x18`,
//!   `+0x30 u16 titleLen`,`+0x32` 标题。固定区更短,扩展名序一类字段
//!   在本布局下不存在(按 0/默认值,`has_image_ext_order() == false`)。
//!
//! ```text
//! +0x00 char[4] "YSCF"
//! +0x04 u32     版本(诊断字段;不参与布局选择)
//! +0x08 u32/u32 头部填充(观测为 0,原样保留)
//! [Old/New]  +0x10 u32 画面宽;+0x14 u32 画面高
//! [V292/V255]+0x14 u32 画面宽;+0x18 u32 画面高
//! [Old/New/V292] +0x1C u8[8] 图像扩展名优先序:1=png 2=bmp 3=jpg 4=gif 5=psb,0=停
//! [Old/New/V292] +0x24 u8[4] 音频扩展名优先序:1=ogg 2=wav,0=停
//! +0x28 u32     thread(Old/New/V292;必须 0 或 1;否则引擎报错 0xdbf9c)
//! +0x2C u8[8]   开关(原样保留;布局相关的安全切片)
//! +0x34 u8[]    装载器开关区(New 0x14 字节 / Old 0x0C 字节;原样保留)
//! [New] +0x48 u32 观测为 0;+0x4C u16 titleLen;+0x4E 标题
//! [Old/V292] +0x40 u16 titleLen;+0x42 标题
//! [V255] +0x30 u16 titleLen;+0x32 标题
//! ```
//!
//! # 宽高的引擎侧校验(0.495 `FUN_0045dbb0`;0.292 `FUN_0043f3a4` 同型)
//!
//! 反编译显示两字段都用 **32 位**读,判定是**无符号减法**:
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

use crate::code;
use crate::container::{read_header, truncated, u16_at, u32_at};
use yu_log::{Boundary, Diag};

/// YSCF 头部布局。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum YscfLayout {
    /// 0.495:宽 +0x10/高 +0x14,titleLen @ 0x4C,title @ 0x4E。
    New,
    /// v459/0.466/v474:宽 +0x10/高 +0x14,titleLen @ 0x40。
    Old,
    /// 官方 0.292:宽 +0x14/高 +0x18,titleLen @ 0x40。
    V292,
    /// 0.263 商业样本:宽 +0x14/高 +0x18,titleLen @ 0x3C。
    V263,
    /// 官方 0.255:宽 +0x14/高 +0x18,titleLen @ 0x30。
    V255,
    /// v500 商业包:宽 +0x10/高 +0x14,titleLen @ 0x58,title @ 0x5A。
    ///
    /// 证据:`<v500-sample>` 的 `ysbin/yscfg.ybn`(141B):
    /// `+0x10=1920`、`+0x14=1080`、`+0x58 u16=51`、`+0x5A` 标题,
    /// 文件恰好在标题末尾闭合。`+0x48..+0x58` 为新增未知 dword 区。
    V500,
}

impl YscfLayout {
    /// 布局候选(固定顺序;只影响诊断文案,不影响裁决的唯一性)。
    const ALL: [YscfLayout; 6] = [
        YscfLayout::New,
        YscfLayout::Old,
        YscfLayout::V292,
        YscfLayout::V263,
        YscfLayout::V255,
        YscfLayout::V500,
    ];

    fn width_off(self) -> usize {
        match self {
            YscfLayout::New | YscfLayout::Old | YscfLayout::V500 => 0x10,
            YscfLayout::V292 | YscfLayout::V263 | YscfLayout::V255 => 0x14,
        }
    }

    fn height_off(self) -> usize {
        self.width_off() + 4
    }

    fn title_len_off(self) -> usize {
        match self {
            YscfLayout::New => 0x4C,
            YscfLayout::Old | YscfLayout::V292 => 0x40,
            YscfLayout::V263 => 0x3C,
            YscfLayout::V255 => 0x30,
            YscfLayout::V500 => 0x58,
        }
    }

    fn title_off(self) -> usize {
        self.title_len_off() + 2
    }

    /// 本布局是否携带 `+0x1C` 的图像扩展名优先序。
    ///
    /// 0.255 的固定区到 `+0x30` 就结束;该区域没有已取证字段,
    /// 读取时按 0(宿主可显式注入探测序,见 `yu-gfx` 的 A5 契约)。
    #[must_use]
    pub fn has_image_ext_order(self) -> bool {
        !matches!(self, YscfLayout::V255)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            YscfLayout::New => "new",
            YscfLayout::Old => "old",
            YscfLayout::V292 => "v292",
            YscfLayout::V263 => "v263",
            YscfLayout::V255 => "v255",
            YscfLayout::V500 => "v500",
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
    /// `+0x18` 开关(原样保留;V292/V255 的该偏移是画面高,故为 0)。
    pub flag_18: u32,
    /// 图像扩展名优先序(值语义见模块文档;V255 = 空)。
    pub image_ext_order: [u8; 8],
    /// 音频扩展名优先序。
    pub sound_ext_order: [u8; 4],
    /// `+0x28` 开关(引擎要求 0 或 1;V255 未取证,固定 0)。
    pub flag_28: u32,
    /// `+0x2C` 起的 8 个开关(原样保留;V255 = 0)。
    pub switches: [u8; 8],
    /// `+0x2C` 起的七个 u32 原始值。
    ///
    /// New:REA `FUN_0045d950` 分别拷到
    /// `DAT_00802888/802898/8028bc/8028c0/8028c4/8028c8/8028cc`;
    /// V292:引擎 `FUN_0043f3a4` 只读 `+0x2C/+0x30/+0x34/+0x38` 四个
    /// dword(其余置 0)。本层只保留原始值供 trace,不据此改源顺序。
    pub file_priority_raw: [u32; 7],
    /// `+0x34` 起的未命名开关区(原样保留;V292/V255 = 0)。
    pub unknown_34: [u8; 0x14],
    /// `+0x48` 未命名字段(原样保留;V292/V255 = 0)。
    pub unknown_48: u32,
    /// 标题(CP932 原始字节)。
    pub title: Vec<u8>,
}

impl Yscf {
    /// 解码 `yscfg.ybn` 字节,执行引擎侧的字段校验。
    ///
    /// 四个布局都作为候选;**恰好一个**闭合才接受;多个同时闭合时按
    /// "不唯一"失败(不猜布局)。
    pub fn parse(buf: &[u8], origin: &[u8]) -> Result<Self, Diag> {
        let header = read_header(buf, origin, b"YSCF")?;
        let mut accepted: Vec<Yscf> = Vec::new();
        let mut first_error: Option<Diag> = None;
        for layout in YscfLayout::ALL {
            match parse_with_layout(buf, origin, header.version, layout) {
                Ok(y) => accepted.push(y),
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }
        match accepted.len() {
            1 => Ok(accepted.pop().expect("长度 1")),
            0 => Err(first_error.unwrap_or_else(|| {
                Diag::fatal(code::YSCF_FIELD, Boundary::Format)
                    .at_data(origin, 0x10)
                    .with_detail("yscfg.ybn 无任何结构闭合的头部布局")
            })),
            _ => {
                let names: Vec<&str> = accepted.iter().map(|y| y.layout.as_str()).collect();
                Err(Diag::fatal(code::YSCF_FIELD, Boundary::Format)
                    .at_data(origin, 0x10)
                    .with_detail(format!(
                        "yscfg.ybn 有多个布局同时闭合({}),无法唯一裁决",
                        names.join("/")
                    )))
            }
        }
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
    let width = u32_at(buf, layout.width_off());
    let height = u32_at(buf, layout.height_off());
    let flag_28 = match layout {
        // V255 的 +0x28 未取证;该偏移在 0.255 样本里为 0。
        YscfLayout::V255 => 0,
        _ => u32_at(buf, 0x28),
    };
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
            layout.width_off() as u64,
            format!("画面宽 {width:#x} 不在 0x80..=0x2000(引擎无符号判定)"),
        )?;
        check(
            bad_height,
            layout.height_off() as u64,
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

    let (file_priority_raw, switches, unknown_34, unknown_48, flag_18) = match layout {
        YscfLayout::New | YscfLayout::V500 => (
            [
                u32_at(buf, 0x2C),
                u32_at(buf, 0x30),
                u32_at(buf, 0x34),
                u32_at(buf, 0x38),
                u32_at(buf, 0x3C),
                u32_at(buf, 0x40),
                u32_at(buf, 0x44),
            ],
            buf[0x2C..0x34].try_into().unwrap(),
            buf[0x34..0x48].try_into().unwrap(),
            u32_at(buf, 0x48),
            u32_at(buf, 0x18),
        ),
        YscfLayout::Old => {
            let mut unknown = [0u8; 0x14];
            unknown[..0x0C].copy_from_slice(&buf[0x34..0x40]);
            ([0u32; 7], [0u8; 8], unknown, 0, u32_at(buf, 0x18))
        }
        YscfLayout::V292 | YscfLayout::V263 => (
            [
                u32_at(buf, 0x2C),
                u32_at(buf, 0x30),
                u32_at(buf, 0x34),
                u32_at(buf, 0x38),
                0,
                0,
                0,
            ],
            [0u8; 8],
            [0u8; 0x14],
            0,
            0,
        ),
        YscfLayout::V255 => ([0u32; 7], [0u8; 8], [0u8; 0x14], 0, 0),
    };

    let (image_ext_order, sound_ext_order) = if layout.has_image_ext_order() {
        (
            buf[0x1C..0x24].try_into().unwrap(),
            buf[0x24..0x28].try_into().unwrap(),
        )
    } else {
        ([0u8; 8], [0u8; 4])
    };

    Ok(Yscf {
        version,
        layout,
        width,
        height,
        flag_18,
        image_ext_order,
        sound_ext_order,
        flag_28,
        switches,
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

    /// v500 商业包:标题在 +0x58(u16 len)/+0x5A,文件尾恰好闭合。
    #[test]
    fn v500_layout_uses_0x58_caption() {
        let title = b"SampleTitle";
        let mut buf = base(0x5A + title.len());
        buf[0x58..0x5A].copy_from_slice(&(title.len() as u16).to_le_bytes());
        buf[0x5A..].copy_from_slice(title);

        let cfg = Yscf::parse(&buf, b"v500/yscfg.ybn").unwrap();
        assert_eq!(cfg.layout, YscfLayout::V500);
        assert_eq!((cfg.width, cfg.height), (1280, 720));
        assert_eq!(cfg.title, title);
    }

    /// 官方 0.255 样本头(62 字节)逐字节复刻:宽 +0x14/高 +0x18,标题 +0x32。
    #[test]
    fn v255_layout_uses_0x30_caption() {
        let title = b"ERIS Ver0.55";
        let mut buf = vec![0u8; 0x32 + title.len()];
        buf[..4].copy_from_slice(b"YSCF");
        buf[4..8].copy_from_slice(&255u32.to_le_bytes());
        buf[0x14..0x18].copy_from_slice(&800u32.to_le_bytes());
        buf[0x18..0x1C].copy_from_slice(&600u32.to_le_bytes());
        buf[0x24..0x28].copy_from_slice(&1u32.to_le_bytes());
        buf[0x30..0x32].copy_from_slice(&(title.len() as u16).to_le_bytes());
        buf[0x32..].copy_from_slice(title);

        let cfg = Yscf::parse(&buf, b"v255/yscfg.ybn").unwrap();
        assert_eq!(cfg.layout, YscfLayout::V255);
        assert_eq!((cfg.width, cfg.height), (800, 600));
        assert_eq!(cfg.title, title);
        assert!(!cfg.layout.has_image_ext_order());
    }

    /// 官方 0.292 样本头(78 字节):宽 +0x14/高 +0x18,扩展名序在 +0x1C。
    #[test]
    fn v292_layout_reads_engine_fields() {
        let title = b"ERIS Ver0.92";
        let mut buf = vec![0u8; 0x42 + title.len()];
        buf[..4].copy_from_slice(b"YSCF");
        buf[4..8].copy_from_slice(&292u32.to_le_bytes());
        buf[0x14..0x18].copy_from_slice(&800u32.to_le_bytes());
        buf[0x18..0x1C].copy_from_slice(&600u32.to_le_bytes());
        buf[0x1C..0x24].copy_from_slice(&[1, 2, 3, 4, 5, 0, 0, 0]);
        buf[0x24..0x28].copy_from_slice(&[1, 2, 0, 0]);
        buf[0x2C..0x30].copy_from_slice(&1u32.to_le_bytes());
        buf[0x40..0x42].copy_from_slice(&(title.len() as u16).to_le_bytes());
        buf[0x42..].copy_from_slice(title);

        let cfg = Yscf::parse(&buf, b"v292/yscfg.ybn").unwrap();
        assert_eq!(cfg.layout, YscfLayout::V292);
        assert_eq!((cfg.width, cfg.height), (800, 600));
        assert_eq!(cfg.image_ext_order, [1, 2, 3, 4, 5, 0, 0, 0]);
        assert_eq!(cfg.sound_ext_order, [1, 2, 0, 0]);
        assert_eq!(cfg.file_priority_raw[0], 1);
        assert_eq!(cfg.title, title);
    }

    /// 0.263 商业样本头(98 字节):宽 +0x14/高 +0x18,标题 +0x3E。
    #[test]
    fn v263_layout_uses_0x3c_caption() {
        let title = b"TestGame";
        let mut buf = vec![0u8; 0x3E + title.len()];
        buf[..4].copy_from_slice(b"YSCF");
        buf[4..8].copy_from_slice(&263u32.to_le_bytes());
        buf[0x14..0x18].copy_from_slice(&800u32.to_le_bytes());
        buf[0x18..0x1C].copy_from_slice(&600u32.to_le_bytes());
        buf[0x1C..0x24].copy_from_slice(&[1, 2, 3, 4, 5, 0, 0, 0]);
        buf[0x24..0x28].copy_from_slice(&[1, 2, 0, 0]);
        buf[0x30..0x34].copy_from_slice(&1u32.to_le_bytes());
        buf[0x3C..0x3E].copy_from_slice(&(title.len() as u16).to_le_bytes());
        buf[0x3E..].copy_from_slice(title);

        let cfg = Yscf::parse(&buf, b"v263/yscfg.ybn").unwrap();
        assert_eq!(cfg.layout, YscfLayout::V263);
        assert_eq!((cfg.width, cfg.height), (800, 600));
        assert_eq!(cfg.image_ext_order, [1, 2, 3, 4, 5, 0, 0, 0]);
        assert_eq!(cfg.sound_ext_order, [1, 2, 0, 0]);
        assert_eq!(cfg.title, title);
    }

    /// 两个布局都能走完(标题 + 尾部)时必须判为不唯一,不能取第一个。
    ///
    /// V292/V255 的宽高偏移相同;空标题 + 全零中部会让两条同时闭合。
    #[test]
    fn multiple_closing_layouts_are_rejected() {
        let mut buf = vec![0u8; 0x42];
        buf[..4].copy_from_slice(b"YSCF");
        buf[0x14..0x18].copy_from_slice(&800u32.to_le_bytes());
        buf[0x18..0x1C].copy_from_slice(&600u32.to_le_bytes());
        // 0x30 与 0x40 的 titleLen 都是 0;中部全零 ⇒ V255/V292 同时闭合。
        let error = Yscf::parse(&buf, b"ambiguous").unwrap_err();
        assert_eq!(error.code, code::YSCF_FIELD);
        assert!(error.detail.contains("多个布局"), "{}", error.detail);
    }
}
