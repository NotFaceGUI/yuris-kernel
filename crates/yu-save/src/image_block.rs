//! 图像块(btype 1):`SAVE[DNO ID=图层]` / `LOAD[FILE DNO ID=图层]`。
//!
//! 载荷实测(块体解压后的 raw):
//!
//! ```text
//! 0.495  data/save/save00001.sd DNO=3   ulen=31608
//!   [u32 0x640 = 1600][u32 30000][A: 1600B][B: 30000B]
//!   0.479  save30001.sd DNO=3           ulen=1548
//!   [u32 1536][u32 4][A: 1536B][B: 4B]
//! ```
//!
//! - `B` = `w*h*4` 的 RGBA8(0.495 样本 100x75,即保存瞬间的画面);
//! - `A` = 表面描述:**0.495 的 w/h 在 +0x78/+0x7c,0.479 在 +0x68/+0x6c**
//!   (A 长度同时从 1536 长到 1600 字节)。其余字节是两个样本共有的常量与
//!   未初始化空洞;读取端以 `lenB == w*h*4` 定案,不依赖单一版本的偏移。
//! - 段长表就是前两个 u32;两段紧接其后。
//!
//! 写入端跟随 0.495(A=1600、w/h 在 +0x78),即本内核对齐的引擎版本。

use yu_rt::event::CapturedImage;

/// `A` 段长度(0.495 样本)。
pub const DESCRIPTOR_LEN: usize = 1600;
/// 0.495 的 w/h 偏移。
const WH_495: usize = 0x78;
/// 0.479 的 w/h 偏移。
const WH_479: usize = 0x68;
/// 0x80..0x90 在两个官方样本里为常量(`00 01 00 01 00000000 00010000 00000100`)。
const DESCRIPTOR_CONST: [u8; 16] = [
    0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x01,
    0x00,
];

/// 组装图像块 raw 载荷(`[lenA][lenB][A][B]`)。
pub fn encode(image: &CapturedImage) -> Vec<u8> {
    let mut descriptor = vec![0u8; DESCRIPTOR_LEN];
    descriptor[WH_495..WH_495 + 4].copy_from_slice(&image.width.to_le_bytes());
    descriptor[WH_495 + 4..WH_495 + 8].copy_from_slice(&image.height.to_le_bytes());
    descriptor[0x80..0x90].copy_from_slice(&DESCRIPTOR_CONST);
    let mut out = Vec::with_capacity(8 + descriptor.len() + image.rgba.len());
    out.extend_from_slice(&(descriptor.len() as u32).to_le_bytes());
    out.extend_from_slice(&(image.rgba.len() as u32).to_le_bytes());
    out.extend_from_slice(&descriptor);
    out.extend_from_slice(&image.rgba);
    out
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
    for (origin, off) in [("0.495", WH_495), ("0.479", WH_479)] {
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
                rgba: b.to_vec(),
            });
        }
        // 0.495 偏移命中但长度不符:继续试 0.479(表头布局不同)。
        let _ = origin;
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
    fn roundtrip_100x75() {
        let img = sample(100, 75);
        let raw = encode(&img);
        assert_eq!(raw.len(), 8 + DESCRIPTOR_LEN + 30000);
        assert_eq!(&raw[0..4], &1600u32.to_le_bytes());
        assert_eq!(&raw[4..8], &30000u32.to_le_bytes());
        assert_eq!(decode(&raw).unwrap(), img);
    }

    #[test]
    fn decode_accepts_0479_descriptor_layout() {
        // 0.479: A=1536、w/h 在 +0x68/+0x6c。
        let img = sample(3, 2);
        let mut a = vec![0u8; 1536];
        a[WH_479..WH_479 + 4].copy_from_slice(&3u32.to_le_bytes());
        a[WH_479 + 4..WH_479 + 8].copy_from_slice(&2u32.to_le_bytes());
        let mut raw = Vec::new();
        raw.extend_from_slice(&1536u32.to_le_bytes());
        raw.extend_from_slice(&24u32.to_le_bytes());
        raw.extend_from_slice(&a);
        raw.extend_from_slice(&img.rgba);
        assert_eq!(decode(&raw).unwrap(), img);
    }

    #[test]
    fn rejects_truncated_or_inconsistent() {
        let img = sample(4, 4);
        let mut raw = encode(&img);
        raw.truncate(raw.len() - 1);
        assert!(decode(&raw).is_err());
        let mut raw = encode(&img);
        raw[4..8].copy_from_slice(&7u32.to_le_bytes());
        assert!(decode(&raw).is_err());
    }
}
