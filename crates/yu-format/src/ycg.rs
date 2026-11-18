//! YCG(YU-RIS CG 容器)解码。
//!
//! 4.7x/4.8x 商业包把 `.png` 条目存成 YCG:扩展名是 `.png`,字节不是 PNG。
//! 解码缺失时 `TextureStore` 报 `asset.decode`(命中但格式未知),整屏 CG/UI
//! 不显示——这是 47x 社团游戏全黑屏的直接原因。
//!
//! # 布局(官方 0.479 引擎实测)
//!
//! ```text
//! +0x00 char[4] "YCG\0"                  magic(按 dword 比较 0x00474359)
//! +0x04 u32     width
//! +0x08 u32     height
//! +0x0C u32     位深(实测恒 32;官方读取路径不校验也不使用)
//! +0x10 u32     method: 0 = 原样 BGRA, 1 = 两段 zlib
//! +0x14 u32     (实测 2,读取路径未使用)
//! +0x18 u32     (实测 0,读取路径未使用)
//! +0x1C u32     (实测 = 行分块索引;读取路径未使用)
//! +0x20 u32     raw1     方法 1:第一段解压后字节数
//! +0x24 u32     packed1  方法 1:第一段压缩字节数
//! +0x28 u32     (实测 = 第一段行数;读取路径未使用)
//! +0x2C u32     (实测 = 第二段末行号;读取路径未使用)
//! +0x30 u32     raw2     方法 1:第二段解压后字节数
//! +0x34 u32     packed2  方法 1:第二段压缩字节数
//! +0x38 byte[]  方法 0: width*height*4 字节 BGRA
//!               方法 1: packed1 字节 zlib 流 + packed2 字节 zlib 流
//! ```
//!
//! 像素序 **BGRA8**、首行在上;`raw1 + raw2 = width*height*4`。
//!
//! # 证据(官方 SDK 0.479 `yu-ris.exe`,sha256
//! `479f6bf712edeb34ed3e5d0b1d3e6287309c01d58f30cda700751fe10b6a1cc0`)
//!
//! - `FUN_004254e4`(`0x4254e4..0x4257b9`):magic `0x474359` 校验;
//!   `width=[+0x04]`、`height=[+0x08]`;`width/height > 0x2000` 直接报错;
//!   `method=[+0x10]`;方法 0 `memcpy(dst, data+0x38, width*height*4)`;
//!   方法 1 调 `FUN_0048384c` 两次:第一段 `src=data+0x38`,
//!   长度 `[+0x24]`,输出长度 `[+0x20]`;第二段
//!   `src=data+0x38+[+0x24]`,长度 `[+0x34]`,输出长度 `[+0x30]`;
//!   第二段写到 `dst + [+0x20]`。
//! - `FUN_0048384c`(`0x48384c..0x4838ff`):zlib `uncompress` 包装
//!   (内部 `FUN_004839ac` + 版本串 `"1.2.3"`),成功时回写实际解压长度。
//! - 真实样本:`<games>/<0.479 样本 A>/pac/cg.ypf` → `cg/tr/bg_black.png`
//!   (1280×960,method 1,两段 2407B zlib → 各 2,457,600B);
//!   `cgsys.ypf` → `cgsys/main/tip_namewindow_txspace.png`
//!   (350×33,method 1,44B→22,400B + 46B→23,800B)。
//!
//! 官方上限 0x2000×0x2000;我们按同一上限拒绝畸形头。方法 0/1 之外官方
//! 不写像素(静默黑),本实现显式报错,避免把“读不懂”伪装成黑图。

use flate2::read::ZlibDecoder;
use std::io::Read as _;
use yu_log::{Boundary, Diag};

use crate::code;
use crate::container::{truncated, u32_at};

/// 魔数(4 字节,含结尾 NUL)。
pub const MAGIC: &[u8; 4] = b"YCG\0";
/// 头部字节数;像素/压缩流从这里开始。
pub const HEADER_BYTES: usize = 0x38;
/// 官方 `FUN_004254e4` 的尺寸上限(`cmp …,0x2000`)。
pub const MAX_DIMENSION: u32 = 0x2000;

/// 解出的 YCG 图像(RGBA8,首行在上)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct YcgImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// 字节是否以 YCG 魔数开头。
#[must_use]
pub fn is_ycg(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

/// 解码一个 YCG 容器。错误是 `Diag`(格式域),调用方按资产失败处理。
pub fn decode(bytes: &[u8]) -> Result<YcgImage, Diag> {
    if bytes.len() < HEADER_BYTES {
        return Err(truncated(
            bytes,
            HEADER_BYTES,
            bytes.len(),
            "YCG 头部",
        ));
    }
    if !is_ycg(bytes) {
        return Err(Diag::fatal(code::MAGIC, Boundary::Format).with_detail(format!(
            "YCG 魔数不符: {:02x?}",
            &bytes[..4]
        )));
    }
    let width = u32_at(bytes, 0x04);
    let height = u32_at(bytes, 0x08);
    if width == 0 || height == 0 {
        return Err(Diag::fatal(code::LIMIT, Boundary::Format)
            .with_detail(format!("YCG 尺寸为 0: {width}x{height}")));
    }
    if width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(Diag::fatal(code::LIMIT, Boundary::Format).with_detail(format!(
            "YCG 尺寸超过官方上限 {MAX_DIMENSION}: {width}x{height}"
        )));
    }
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|px| px.checked_mul(4))
        .ok_or_else(|| {
            Diag::fatal(code::LIMIT, Boundary::Format)
                .with_detail(format!("YCG 像素字节溢出: {width}x{height}"))
        })?;
    let method = u32_at(bytes, 0x10);
    let mut pixels = Vec::with_capacity(expected);
    match method {
        // 0 = 原样 BGRA。
        0 => {
            let body = &bytes[HEADER_BYTES..];
            if body.len() < expected {
                return Err(truncated(bytes, HEADER_BYTES + expected, bytes.len(), "YCG 原样像素"));
            }
            pixels.extend_from_slice(&body[..expected]);
        }
        // 1 = 两段 zlib:raw1/packed1 @0x20/@0x24,raw2/packed2 @0x30/@0x34。
        1 => {
            let raw1 = u32_at(bytes, 0x20) as usize;
            let packed1 = u32_at(bytes, 0x24) as usize;
            let raw2 = u32_at(bytes, 0x30) as usize;
            let packed2 = u32_at(bytes, 0x34) as usize;
            if raw1.checked_add(raw2) != Some(expected) {
                return Err(Diag::fatal(code::YCG_CODEC, Boundary::Format).with_detail(format!(
                    "YCG 两段长度 raw1={raw1}+raw2={raw2} != 像素字节 {expected}"
                )));
            }
            let mut offset = HEADER_BYTES;
            for (packed, raw, which) in [(packed1, raw1, 1u8), (packed2, raw2, 2u8)] {
                let end = offset.checked_add(packed).ok_or_else(|| {
                    Diag::fatal(code::LIMIT, Boundary::Format)
                        .with_detail("YCG 压缩流偏移溢出".to_string())
                })?;
                if end > bytes.len() {
                    return Err(truncated(bytes, end, bytes.len(), "YCG zlib 流"));
                }
                // `raw == 0` 的段仍占 `packed` 字节（官方按 `[+0x24]` 跳过
                // 第一段的位置再读第二段；`cgsys/dummy` 就是 raw1=0/
                // packed1=8 的 1×1 透明图）。不能不前进偏移。
                if raw == 0 {
                    offset = end;
                    continue;
                }
                let mut decoder = ZlibDecoder::new(&bytes[offset..end]);
                let mut decoded = Vec::with_capacity(raw);
                decoder.read_to_end(&mut decoded).map_err(|error| {
                    Diag::fatal(code::YCG_CODEC, Boundary::Format).with_detail(format!(
                        "YCG 第 {which} 段 zlib 解压失败: {error}"
                    ))
                })?;
                if decoded.len() != raw {
                    return Err(Diag::fatal(code::YCG_CODEC, Boundary::Format).with_detail(
                        format!(
                            "YCG 第 {which} 段长度不符: 头部 {raw},实际 {}",
                            decoded.len()
                        ),
                    ));
                }
                pixels.extend_from_slice(&decoded);
                offset = end;
            }
        }
        other => {
            return Err(
                Diag::fatal(code::YCG_UNSUPPORTED, Boundary::Format)
                    .with_detail(format!("YCG method {other}(官方只写 method 0/1 像素)")),
            )
        }
    }
    if pixels.len() != expected {
        return Err(Diag::fatal(code::YCG_CODEC, Boundary::Format).with_detail(format!(
            "YCG 像素字节不符: 期望 {expected},实际 {}",
            pixels.len()
        )));
    }
    // YCG 存 BGRA;渲染契约是 RGBA8(与 PNG/BMP 解码输出同口径)。
    for px in pixels.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    Ok(YcgImage {
        width,
        height,
        rgba: pixels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(width: u32, height: u32, method: u32) -> Vec<u8> {
        let mut out = vec![0u8; HEADER_BYTES];
        out[..4].copy_from_slice(MAGIC);
        out[0x04..0x08].copy_from_slice(&width.to_le_bytes());
        out[0x08..0x0c].copy_from_slice(&height.to_le_bytes());
        out[0x0c..0x10].copy_from_slice(&32u32.to_le_bytes());
        out[0x10..0x14].copy_from_slice(&method.to_le_bytes());
        out
    }

    fn zlib(data: &[u8]) -> Vec<u8> {
        use flate2::write::ZlibEncoder;
        use std::io::Write as _;
        let mut encoder = ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(data).expect("zlib write");
        encoder.finish().expect("zlib finish")
    }

    #[test]
    fn method1_zero_raw_first_stream_still_advances_offset() {
        // 真实样本 `cgsys/dummy.png`：1×1、method 1、raw1=0/packed1=8
        // （空 zlib 段）、raw2=4/packed2=12。第一段 raw=0 但字节仍占位，
        // 第二段必须从 `0x38+packed1` 开始。
        let mut bytes = header(1, 1, 1);
        let empty = zlib(&[]);
        let pixel = zlib(&[0, 0, 0, 0]);
        bytes[0x20..0x24].copy_from_slice(&0u32.to_le_bytes());
        bytes[0x24..0x28].copy_from_slice(&(empty.len() as u32).to_le_bytes());
        bytes[0x30..0x34].copy_from_slice(&4u32.to_le_bytes());
        bytes[0x34..0x38].copy_from_slice(&(pixel.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&empty);
        bytes.extend_from_slice(&pixel);
        let image = decode(&bytes).expect("decode");
        assert_eq!(image.rgba, vec![0, 0, 0, 0]);
    }

    #[test]
    fn method0_stored_bgra_becomes_rgba() {
        let mut bytes = header(2, 1, 0);
        // BGRA: (10,20,30,255), (1,2,3,4)
        bytes.extend_from_slice(&[10, 20, 30, 255, 1, 2, 3, 4]);
        let image = decode(&bytes).expect("decode");
        assert_eq!((image.width, image.height), (2, 1));
        assert_eq!(image.rgba, vec![30, 20, 10, 255, 3, 2, 1, 4]);
    }

    #[test]
    fn method1_two_zlib_streams_concatenate_in_order() {
        // 3×2 = 24 字节;第一段 1 行(12B),第二段 1 行(12B)。
        let mut bytes = header(3, 2, 1);
        let row1 = [1u8, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255];
        let row2 = [11u8, 12, 13, 255, 14, 15, 16, 255, 17, 18, 19, 255];
        let packed1 = zlib(&row1);
        let packed2 = zlib(&row2);
        bytes[0x20..0x24].copy_from_slice(&(row1.len() as u32).to_le_bytes());
        bytes[0x24..0x28].copy_from_slice(&(packed1.len() as u32).to_le_bytes());
        bytes[0x30..0x34].copy_from_slice(&(row2.len() as u32).to_le_bytes());
        bytes[0x34..0x38].copy_from_slice(&(packed2.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&packed1);
        bytes.extend_from_slice(&packed2);
        let image = decode(&bytes).expect("decode");
        let mut expect = Vec::new();
        for row in [row1, row2] {
            for px in row.chunks_exact(4) {
                expect.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
            }
        }
        assert_eq!(image.rgba, expect);
    }

    #[test]
    fn rejects_unsupported_method_and_bad_lengths() {
        let mut bytes = header(1, 1, 2);
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        assert!(decode(&bytes).is_err(), "method 2 必须显式失败");

        let mut bytes = header(1, 1, 1);
        bytes[0x20..0x24].copy_from_slice(&8u32.to_le_bytes());
        bytes[0x24..0x28].copy_from_slice(&0u32.to_le_bytes());
        bytes[0x30..0x34].copy_from_slice(&4u32.to_le_bytes());
        bytes[0x34..0x38].copy_from_slice(&0u32.to_le_bytes());
        assert!(decode(&bytes).is_err(), "raw1+raw2 必须等于像素字节");
    }

    #[test]
    fn respects_official_dimension_limit() {
        let bytes = header(MAX_DIMENSION + 1, 1, 0);
        assert!(decode(&bytes).is_err(), "超过 0x2000 必须拒绝");
    }
}
