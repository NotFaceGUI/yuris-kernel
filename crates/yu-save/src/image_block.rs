//! 图像块(btype 1):`SAVE[DNO ID=图层]` / `LOAD[FILE DNO ID=图层]`。
//!
//! 载荷实测(块体解压后的 raw):
//!
//! ```text
//! 0.495  data/save/save00001.sd DNO=3   ulen=31608
//!   [u32 0x640 = 1600][u32 30000][A: 1600B][B: 30000B]
//! 0.479  save00001.sd DNO=3             ulen=57440
//!   [u32 0x600 = 1536][u32 55896][A: 1536B][B: 55896B]
//! ```
//!
//! - `B` = `w*h*4` 的 **BGRA8**(0.495 样本 100x75,即保存瞬间的画面)。
//!   手册无页。证据(真值层级 3,2026-10-31):`<game-b>` v494 官方
//!   save00001–5 的 DNO=3 块按 BGRA 解释是自然色(暖色卧室/蓝天校园/
//!   肤色),按 RGBA 解释整图红蓝互换(读档界面缩略图整体偏蓝,即
//!   `DIV-GFX-071` 的现象);官方 alpha 行 0/37/74 = 255、其余恒 254
//!   (与场景无关的采集痕迹),本内核落盘 alpha 仍写 255,见台账残余;
//! - `A` = 表面描述。**描述符长度与 w/h 偏移随存档容器版本变化**:
//!   - `0xC0`(0.450–0.466 全系):A=**1520**,w/h 在 **+0x68/+0x6c**;
//!   - `0x138`(0.470–0.479(0.13)):A=**1536**,w/h 在 **+0x70/+0x74**;
//!   - `0x1E0`(0.479(0.14)–0.495)/`0x1E1`(0.481 商业)/`0x1E6`:A=**1600**,
//!     w/h 在 **+0x78/+0x7c**。
//!   0x450–0.466 的 +0x68 不是猜测:官方 `yu-ris.exe` 全系
//!   (0.459 `FUN_004382d0` @0x438f5f.. / 0.466 `FUN_00438b78`
//!   @0x43904a..)都硬校验 `payload[0] == 0x5F0`,随后从描述符
//!   `+0x68/+0x6c` 取 w/h 建表面;A≠0x5F0 报错 0x1a64e 退出。
//!   0x470–0.479(0.13) 的 +0x70 也不是猜测:0.47x 全系 exe 的
//!   LOAD 都硬校验 `payload[0] == 0x600`(0.479 `<v479-x> exe`
//!   `FUN_00440490` 0x440894),从 `+0x70/+0x74` 取 w/h。
//!   0x1E0 的 +0x78 由 0.495 官方 LOAD(0x44734e 版本 / 0x4477de
//!   A==0x640)与官方存档样本双重确认。
//!   旧注释写的 0.479 +0x68 是误读(2026-11-20 按 0.450–0.495 全系
//!   SDK 机器码更正)。
//! - `A` 的其余字节是常量与未初始化空洞;读取端以 `lenB == w*h*4` 定案。
//! - 段长表就是前两个 u32;两段紧接其后。
//!
//! 写入端**按目标存档容器版本选布局**(不是固定跟随 0.495):写 0.479
//! 工程时用 A=1536/+0x70,官方 0.479 LOAD 才收;写 0.495/0.481/0.474
//! 时用 A=1600/+0x78。版本→策略的**唯一表**在 [`crate::profile`];
//! 未取证的容器版本(如 0x0555)不给布局,由调用方显式跳过图像块,
//! 避免再产出官方读不了的缩略图。加新版本只改 `profile_for` 一行。
//! 内部表示(`CapturedImage`)恒为 RGBA8,编解码各做一次 R/B 互换。

use yu_rt::event::CapturedImage;

/// 一个容器版本对应的图像块描述符布局。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    /// `A` 段字节数(块首第一个 u32)。
    pub descriptor_len: usize,
    /// w/h 在 `A` 内的偏移(u32 LE;宽在 `+off`,高在 `+off+4`)。
    pub wh_offset: usize,
}

/// 0.450–0.466(`0xC0`)官方布局:A=1520,w/h 在 +0x68。
pub const LAYOUT_0466: Layout = Layout {
    descriptor_len: 1520,
    wh_offset: 0x68,
};

/// 0.470–0.479(0.13)(`0x138`)官方布局:A=1536,w/h 在 +0x70。
pub const LAYOUT_0479: Layout = Layout {
    descriptor_len: 1536,
    wh_offset: 0x70,
};

/// 0.479(0.14)–0.495(`0x1E0`)官方布局:A=1600,w/h 在 +0x78。
pub const LAYOUT_0495: Layout = Layout {
    descriptor_len: 1600,
    wh_offset: 0x78,
};

/// 由存档容器版本选图像块布局。
///
/// **唯一版本表在 [`crate::profile::profile_for`]**(图像布局与块类型 0
/// 的 var_id 一起维护);本函数只是便捷查询,避免两处各写一份 match。
/// 返回 `None` = 该版本没有取证的布局(当前未列出的版本属此列),
/// 调用方必须显式跳过图像块而不是猜一个布局写出去。
#[must_use]
pub fn layout_for_container_version(version: u32) -> Option<Layout> {
    crate::profile::profile_for(version).and_then(|profile| profile.image_layout)
}

/// `0x80..0x90` 在两个官方样本里为常量(`00 01 00 01 00000000 00010000 00000100`)。
const DESCRIPTOR_CONST: [u8; 16] = [
    0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00,
];

/// R/B 互换(自逆):磁盘 B 段是官方 BGRA8,内部表示是 RGBA8,编解码各走一次。
fn swap_rb(pixels: &[u8]) -> Vec<u8> {
    let mut out = pixels.to_vec();
    for px in out.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    out
}

/// 组装图像块 raw 载荷(`[lenA][lenB][A][B]`;B 段按官方像素序落 BGRA)。
pub fn encode(image: &CapturedImage, layout: Layout) -> Vec<u8> {
    let mut descriptor = vec![0u8; layout.descriptor_len];
    let wh = layout.wh_offset;
    descriptor[wh..wh + 4].copy_from_slice(&image.width.to_le_bytes());
    descriptor[wh + 4..wh + 8].copy_from_slice(&image.height.to_le_bytes());
    descriptor[0x80..0x90].copy_from_slice(&DESCRIPTOR_CONST);
    let mut out = Vec::with_capacity(8 + descriptor.len() + image.rgba.len());
    out.extend_from_slice(&(descriptor.len() as u32).to_le_bytes());
    out.extend_from_slice(&(image.rgba.len() as u32).to_le_bytes());
    out.extend_from_slice(&descriptor);
    out.extend_from_slice(&swap_rb(&image.rgba));
    out
}

/// 给定 A 段长度时优先检查的 w/h 偏移(布局自证)。
///
/// `A` 长度本身就是版本指纹:1536/1600 分别唯一对应 0.479/0.495 系。
/// 未知长度仍试两个已知偏移(读旧内核产物/异常样本时保持可诊断),
/// 但只有 `w*h*4 == lenB` 才接受。
fn wh_offsets(a_len: usize) -> &'static [usize] {
    match a_len {
        1520 => &[LAYOUT_0466.wh_offset],
        1536 => &[LAYOUT_0479.wh_offset],
        1600 => &[LAYOUT_0495.wh_offset],
        _ => &[
            LAYOUT_0466.wh_offset,
            LAYOUT_0479.wh_offset,
            LAYOUT_0495.wh_offset,
        ],
    }
}

/// 解析图像块 raw 载荷。失败返回人读原因(诊断 detail)。
pub fn decode(raw: &[u8]) -> Result<CapturedImage, String> {
    if raw.len() < 8 {
        return Err(format!("图像块长度 {} < 8(段长表缺失)", raw.len()));
    }
    let a_len = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as usize;
    let b_len = u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]) as usize;
    let a_end = 8usize
        .checked_add(a_len)
        .ok_or_else(|| format!("A 段长度 {a_len} 溢出"))?;
    let b_end = a_end
        .checked_add(b_len)
        .ok_or_else(|| format!("B 段长度 {b_len} 溢出"))?;
    if b_end > raw.len() {
        return Err(format!(
            "图像块截断:A={a_len} B={b_len} 需 {b_end} 字节,实有 {}",
            raw.len()
        ));
    }
    let a = &raw[8..a_end];
    let b = &raw[a_end..b_end];
    for off in wh_offsets(a_len) {
        let off = *off;
        if a.len() < off + 8 {
            continue;
        }
        let w = u32::from_le_bytes([a[off], a[off + 1], a[off + 2], a[off + 3]]);
        let h = u32::from_le_bytes([a[off + 4], a[off + 5], a[off + 6], a[off + 7]]);
        if w == 0 || h == 0 {
            continue;
        }
        let expected = (w as usize)
            .checked_mul(h as usize)
            .and_then(|n| n.checked_mul(4));
        if expected == Some(b_len) {
            return Ok(CapturedImage {
                width: w,
                height: h,
                rgba: swap_rb(b),
            });
        }
    }
    Err(format!(
        "图像块尺寸不可判:A={a_len}B={b_len}(w*h*4 与 B 段长度不符)"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(w: u32, h: u32) -> CapturedImage {
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for i in 0..w * h {
            rgba.extend_from_slice(&[(i & 0xff) as u8, 0x40, 0x80, 0xff]);
        }
        CapturedImage {
            width: w,
            height: h,
            rgba,
        }
    }

    #[test]
    fn roundtrip_0495_100x75() {
        let img = sample(100, 75);
        let raw = encode(&img, LAYOUT_0495);
        assert_eq!(raw.len(), 8 + 1600 + 30000);
        assert_eq!(&raw[0..4], &1600u32.to_le_bytes());
        assert_eq!(&raw[4..8], &30000u32.to_le_bytes());
        assert_eq!(decode(&raw).unwrap(), img);
    }

    /// 0.459/0.466 官方 LOAD 硬校验 A==0x5F0 并从 +0x68 取 w/h。
    #[test]
    fn roundtrip_0466_uses_official_descriptor() {
        let img = sample(160, 90);
        let raw = encode(&img, LAYOUT_0466);
        assert_eq!(raw.len(), 8 + 1520 + 160 * 90 * 4);
        assert_eq!(&raw[0..4], &1520u32.to_le_bytes());
        let a = &raw[8..8 + 1520];
        assert_eq!(u32::from_le_bytes(a[0x68..0x6c].try_into().unwrap()), 160);
        assert_eq!(u32::from_le_bytes(a[0x6c..0x70].try_into().unwrap()), 90);
        assert_eq!(decode(&raw).unwrap(), img);
    }

    /// 0.479 官方 LOAD 硬校验 A==0x600 并从 +0x70 取 w/h;写出的块必须逐项命中。
    #[test]
    fn roundtrip_0479_uses_official_descriptor() {
        let img = sample(137, 102);
        let raw = encode(&img, LAYOUT_0479);
        assert_eq!(raw.len(), 8 + 1536 + 137 * 102 * 4);
        assert_eq!(&raw[0..4], &1536u32.to_le_bytes());
        let a = &raw[8..8 + 1536];
        assert_eq!(u32::from_le_bytes(a[0x70..0x74].try_into().unwrap()), 137);
        assert_eq!(u32::from_le_bytes(a[0x74..0x78].try_into().unwrap()), 102);
        assert_eq!(decode(&raw).unwrap(), img);
    }

    /// 布局选择由容器版本驱动;未取证版本(如 0x0555)必须返回 `None`,
    /// 由写盘端显式跳过,而不是猜一个布局。
    #[test]
    fn version_selects_layout_and_unknown_versions_do_not_guess() {
        let img = sample(3, 2);
        assert!(decode(&encode(&img, LAYOUT_0466)).is_ok());
        assert!(decode(&encode(&img, LAYOUT_0479)).is_ok());
        assert!(decode(&encode(&img, LAYOUT_0495)).is_ok());
        assert_eq!(layout_for_container_version(0x00C0), Some(LAYOUT_0466));
        assert_eq!(layout_for_container_version(0x0138), Some(LAYOUT_0479));
        assert_eq!(layout_for_container_version(0x01DA), Some(LAYOUT_0495));
        assert_eq!(layout_for_container_version(0x01E0), Some(LAYOUT_0495));
        assert_eq!(layout_for_container_version(0x01E1), Some(LAYOUT_0495));
        assert_eq!(layout_for_container_version(0x01E6), Some(LAYOUT_0495));
        assert_eq!(layout_for_container_version(0x01EF), Some(LAYOUT_0495));
        assert_eq!(layout_for_container_version(0x0555), None);
    }

    /// 磁盘像素序 = 官方 BGRA(`DIV-GFX-071`;此前官方样本是黑屏,
    /// 通道序从未被钉过,按 RGBA 落盘导致读官方存档整图红蓝互换)。
    #[test]
    fn disk_pixel_order_is_official_bgra() {
        for layout in [LAYOUT_0466, LAYOUT_0479, LAYOUT_0495] {
            let img = CapturedImage {
                width: 2,
                height: 1,
                rgba: vec![0x10, 0x20, 0x30, 0xFF, 0x40, 0x50, 0x60, 0x80],
            };
            let raw = encode(&img, layout);
            let b_at = 8 + layout.descriptor_len;
            assert_eq!(
                &raw[b_at..b_at + 8],
                &[0x30, 0x20, 0x10, 0xFF, 0x60, 0x50, 0x40, 0x80]
            );
            assert_eq!(decode(&raw).unwrap(), img);
        }
    }

    #[test]
    fn rejects_truncated_or_inconsistent() {
        let img = sample(4, 4);
        for layout in [LAYOUT_0466, LAYOUT_0479, LAYOUT_0495] {
            let mut raw = encode(&img, layout);
            raw.truncate(raw.len() - 1);
            assert!(decode(&raw).is_err());
            let mut raw = encode(&img, layout);
            raw[4..8].copy_from_slice(&7u32.to_le_bytes());
            assert!(decode(&raw).is_err());
        }
    }
}
