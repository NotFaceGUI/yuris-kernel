//! YDG(YU-RIS Dynamic Graphic)容器与内置 QOI 帧解码。
//!
//! 0.500 商业分支的 UI/CG 主力格式(`<v500-relirium>` 的 `cgsys/**` 全是
//! `.ydg`)。缺这一层时 `CG[FILE=...]` 的探测链找不到任何图:脚本按
//! `yscfg` 扩展码表先探 `.ydg`,扩展名不认识 ⇒ 整屏按钮/提示缺失。
//!
//! # 布局(官方 0.500 `Relirium.exe` 实测,构建 `0,500,0,0`)
//!
//! ```text
//! +0x00 char[4] "YDG\0"
//! +0x04 char[8] "YU-RIS\0\0"
//! +0x0C u32     版本/调度值(样本恒 100)
//! +0x10 u32     头长(样本恒 0x30;帧表从 +0x34 起,不在头长处)
//! +0x14 u32     文件字节数
//! +0x18 u32     保留(0)
//! +0x1C u32     保留(0)
//! +0x20 u16     画布宽
//! +0x22 u16     画布高(=各帧高之和)
//! +0x24 u32     保留(0)
//! +0x28 u32     保留(0)
//! +0x2C u32     保留(0)
//! +0x30 u32     帧数(官方上限 16)
//! +0x34 帧表[帧数],每项 16 字节:
//!   +0x00 u32 帧数据偏移(相对文件头)
//!   +0x04 u32 帧数据字节数
//!   +0x08 u8  编码码:0 = QOI;2 = WebP(样本外,走宿主解码)
//!   +0x09 u8  保留
//!   +0x0A u16 帧高
//! ```
//!
//! 帧在画布上**纵向堆叠**(偏移累加帧高);官方把每帧解码进同一个
//! `宽 × 总高` 的 BGRA 缓冲。这里的“帧”是**按行切分的瓦片**,不是动画
//! 帧(实测 1920×1080 标题背景 = 4×1920×270;按钮提示 581×86 = 2×581×43)。
//! 解码方要把所有瓦片拼回整图,不能只取第 0 片。
//!
//! # 证据
//!
//! - 头/帧表:`FUN_00449a08`(`0x449a08`,463B):magic `0x474459`/
//!   `0x522d5559`/`0x5349`;宽高读 `+0x20/+0x22`,上限 `0x2000`;
//!   帧数读 `+0x30`,上限 `0x11`;帧表项 `offset/len/codec/height` 写入
//!   24 字节工作项(`+0x0A` = 帧高,`+0x08` 低字节 = 编码码)。
//! - QOI 解码:`FUN_0044c670`(`0x44c670`,1206B):校验 `qoif`、宽高、
//!   channels 3/4、colorspace,输出 **B,G,R,A** 字节序;本文按 QOI 规范
//!   输出 RGBA(与 `Texture` 契约一致),像素值与官方逐字节同源。
//! - `FUN_00449594`(122B)只做 QOI 头尺寸读取。
//!
//! 未取证:瓦片切分是否只按行、以及 codec=2(WebP)在官方缓冲里的字节序
//! (QOI 已由 `FUN_0044c670` 证实输出 B,G,R,A)。

use yu_log::{Boundary, Diag};

use crate::code;

/// 文件魔数前缀 12 字节。
pub const MAGIC: [u8; 12] = *b"YDG\0YU-RIS\0\0";
/// 帧表起点(帧数之后)。
const FRAME_TABLE: usize = 0x34;
/// 官方帧数上限(`uVar8 < 0x11`)。
pub const MAX_FRAMES: usize = 16;
/// 官方宽/高上限(`0x2000 < width/height` 拒绝)。
pub const MAX_DIM: u16 = 0x2000;

/// 帧编码码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum YdgCodec {
    /// 0:QOI(Quiet OK Image),见 [`decode_qoi`]。
    Qoi,
    /// 2:WebP(官方走 libwebp;本内核由宿主 `image` 解码整帧)。
    WebP,
    /// 其它未取证码(原样保留)。
    Other(u8),
}

impl YdgCodec {
    #[must_use]
    pub fn from_code(code: u8) -> Self {
        match code {
            0 => Self::Qoi,
            2 => Self::WebP,
            other => Self::Other(other),
        }
    }
}

/// 帧表项(借用原文件字节)。
#[derive(Clone, Copy, Debug)]
pub struct YdgFrame<'a> {
    pub codec: YdgCodec,
    pub height: u16,
    /// 帧数据(已按 offset/length 切片)。
    pub bytes: &'a [u8],
}

/// 已解析的 YDG 容器。
#[derive(Clone, Debug)]
pub struct Ydg<'a> {
    pub width: u16,
    /// 画布总高(= Σ 帧高)。
    pub height: u16,
    pub frames: Vec<YdgFrame<'a>>,
}

/// 是否为 YDG(按 12 字节前缀判断,不做扩展名猜测)。
#[must_use]
pub fn is_ydg(bytes: &[u8]) -> bool {
    bytes.len() >= FRAME_TABLE && bytes.starts_with(&MAGIC)
}

fn malformed(detail: impl Into<String>) -> Diag {
    Diag::fatal(code::MAGIC, Boundary::Format).with_detail(detail.into())
}

fn broken(code: &'static str, detail: impl Into<String>) -> Diag {
    Diag::fatal(code, Boundary::Format).with_detail(detail.into())
}

/// 解析头部与帧表(不检查编码是否支持)。
pub fn parse(bytes: &[u8]) -> Result<Ydg<'_>, Diag> {
    if !is_ydg(bytes) {
        return Err(malformed("YDG 魔数不匹配"));
    }
    let u16_at = |off: usize| -> Result<u16, Diag> {
        let pair = bytes
            .get(off..off + 2)
            .ok_or_else(|| broken(code::TRUNCATED, format!("YDG 头在 {off:#x} 截断")))?;
        Ok(u16::from_le_bytes([pair[0], pair[1]]))
    };
    let u32_at = |off: usize| -> Result<u32, Diag> {
        let quad = bytes
            .get(off..off + 4)
            .ok_or_else(|| broken(code::TRUNCATED, format!("YDG 头在 {off:#x} 截断")))?;
        Ok(u32::from_le_bytes([quad[0], quad[1], quad[2], quad[3]]))
    };
    let width = u16_at(0x20)?;
    let height = u16_at(0x22)?;
    if width == 0 || height == 0 || width > MAX_DIM || height > MAX_DIM {
        return Err(broken(
            code::LIMIT,
            format!("YDG 画布非法: {width}x{height}"),
        ));
    }
    let file_size = u32_at(0x14)? as usize;
    if file_size > bytes.len() {
        return Err(broken(
            code::COUNT,
            format!("YDG 头声明 {file_size} 字节,实际 {}", bytes.len()),
        ));
    }
    let count = u32_at(0x30)? as usize;
    if count == 0 || count > MAX_FRAMES {
        return Err(broken(code::LIMIT, format!("YDG 帧数非法: {count}")));
    }
    let mut frames = Vec::with_capacity(count);
    let mut total_height = 0u32;
    for index in 0..count {
        let base = FRAME_TABLE + index * 16;
        let offset = u32_at(base)? as usize;
        let len = u32_at(base + 4)? as usize;
        let codec = YdgCodec::from_code(bytes[base + 8]);
        let frame_h = u16_at(base + 0x0A)?;
        if frame_h == 0 {
            return Err(broken(
                code::LIMIT,
                format!("YDG 帧 {index} 高度为 0"),
            ));
        }
        let end = offset.checked_add(len).ok_or_else(|| {
            broken(code::LIMIT, format!("YDG 帧 {index} 长度溢出"))
        })?;
        if end > bytes.len() {
            return Err(broken(
                code::COUNT,
                format!(
                    "YDG 帧 {index} 越过文件尾: {offset:#x}+{len:#x} > {}",
                    bytes.len()
                ),
            ));
        }
        total_height = total_height.saturating_add(u32::from(frame_h));
        frames.push(YdgFrame {
            codec,
            height: frame_h,
            bytes: &bytes[offset..end],
        });
    }
    if total_height != u32::from(height) {
        return Err(broken(
            code::COUNT,
            format!("YDG 画布高 {height} != 帧高之和 {total_height}"),
        ));
    }
    Ok(Ydg {
        width,
        height,
        frames,
    })
}

/// QOI 解码结果(RGBA8,首行在上)。
pub struct QoiImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// 解码一帧 QOI。
///
/// `expect` 是 YDG 帧表声明的 (宽, 帧高);QOI 自带头必须与宽一致、
/// 与帧高一致(官方 `FUN_00449a08` 把帧高写入工作项,解码器按帧高
/// 推进目标行)。
pub fn decode_qoi(bytes: &[u8], expect: (u16, u16)) -> Result<QoiImage, Diag> {
    if bytes.len() < 22 || &bytes[..4] != b"qoif" {
        return Err(broken(code::MAGIC, "QOI 头魔数不匹配"));
    }
    let width = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    let height = u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
    let channels = bytes[12];
    let colorspace = bytes[13];
    if width == 0 || height == 0 || width > 0x2000 || height > 0x2000 {
        return Err(broken(code::LIMIT, format!("QOI 尺寸非法: {width}x{height}")));
    }
    if channels != 3 && channels != 4 {
        return Err(broken(code::LIMIT, format!("QOI channels 非法: {channels}")));
    }
    if colorspace > 1 {
        return Err(broken(code::LIMIT, format!("QOI colorspace 非法: {colorspace}")));
    }
    if u32::from(expect.0) != width || u32::from(expect.1) != height {
        return Err(broken(
            code::COUNT,
            format!(
                "QOI 尺寸 {width}x{height} 与 YDG 帧表 {}x{} 不一致",
                expect.0, expect.1
            ),
        ));
    }
    // 每像素至少 1 字节余量;官方 `FUN_0044c670` 的防溢出判据是
    // `height < 400000000 / width`(宽度/高度分开比,不是 w*h)。
    let pixels = u64::from(width) * u64::from(height);
    if u64::from(height) > 400_000_000 / u64::from(width) || pixels > 64 * 1024 * 1024 {
        return Err(broken(code::LIMIT, format!("QOI 像素数超限: {width}x{height}")));
    }

    let mut out = Vec::with_capacity((pixels * 4) as usize);
    let mut index = [[0u8; 4]; 64];
    let mut px = [0u8, 0, 0, 255];
    let mut p = 14usize;
    let mut produced = 0u64;
    while produced < pixels {
        let op = *bytes
            .get(p)
            .ok_or_else(|| broken(code::TRUNCATED, "QOI 数据在像素完成前截断"))?;
        p += 1;
        match op {
            0x00..=0x3f => px = index[op as usize],
            0x40..=0x7f => {
                px[0] = px[0].wrapping_add(((op >> 4) & 3).wrapping_sub(2));
                px[1] = px[1].wrapping_add(((op >> 2) & 3).wrapping_sub(2));
                px[2] = px[2].wrapping_add((op & 3).wrapping_sub(2));
            }
            0x80..=0xbf => {
                let b2 = *bytes
                    .get(p)
                    .ok_or_else(|| broken(code::TRUNCATED, "QOI LUMA 缺第二字节"))?;
                p += 1;
                let dg = (op & 0x3f).wrapping_sub(32);
                px[0] = px[0].wrapping_add(dg.wrapping_add((b2 >> 4).wrapping_sub(8)));
                px[1] = px[1].wrapping_add(dg);
                px[2] = px[2].wrapping_add(dg.wrapping_add((b2 & 0x0f).wrapping_sub(8)));
            }
            0xc0..=0xfd => {
                let run = usize::from(op & 0x3f) + 1;
                for _ in 0..run.min((pixels - produced) as usize) {
                    out.extend_from_slice(&px);
                }
                produced += run as u64;
                index[(usize::from(px[0]) * 3
                    + usize::from(px[1]) * 5
                    + usize::from(px[2]) * 7
                    + usize::from(px[3]) * 11)
                    % 64] = px;
                continue;
            }
            0xfe => {
                let rgb = bytes
                    .get(p..p + 3)
                    .ok_or_else(|| broken(code::TRUNCATED, "QOI RGB 载荷截断"))?;
                px[0] = rgb[0];
                px[1] = rgb[1];
                px[2] = rgb[2];
                p += 3;
            }
            0xff => {
                let rgba = bytes
                    .get(p..p + 4)
                    .ok_or_else(|| broken(code::TRUNCATED, "QOI RGBA 载荷截断"))?;
                px = [rgba[0], rgba[1], rgba[2], rgba[3]];
                p += 4;
            }
        }
        out.extend_from_slice(&px);
        produced += 1;
        index[(usize::from(px[0]) * 3
            + usize::from(px[1]) * 5
            + usize::from(px[2]) * 7
            + usize::from(px[3]) * 11)
            % 64] = px;
    }
    Ok(QoiImage {
        width,
        height,
        rgba: out,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 两像素 QOI:RGBA(10,20,30,40) + RUN 1 像素。
    fn qoi_2px() -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"qoif");
        out.extend_from_slice(&2u32.to_be_bytes());
        out.extend_from_slice(&1u32.to_be_bytes());
        out.push(4);
        out.push(0);
        out.push(0xff);
        out.extend_from_slice(&[10, 20, 30, 40]);
        out.push(0xc0);
        out.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 1]);
        out
    }

    #[test]
    fn qoi_decodes_rgba_pixels() {
        let img = decode_qoi(&qoi_2px(), (2, 1)).unwrap();
        assert_eq!(img.width, 2);
        assert_eq!(img.height, 1);
        assert_eq!(img.rgba, vec![10, 20, 30, 40, 10, 20, 30, 40]);
    }

    #[test]
    fn qoi_rejects_size_mismatch() {
        assert!(decode_qoi(&qoi_2px(), (3, 1)).is_err());
    }

    /// 官方防溢出判据是 `height < 400000000 / width`;早期实现误写成
    /// `w*h > 400000000 / w`,会把 1920×270 这类宽瓦片整片拒掉。
    #[test]
    fn qoi_accepts_wide_frame_beyond_naive_pixel_cap() {
        let w = 1920u32;
        let h = 270u32;
        let mut out = Vec::new();
        out.extend_from_slice(b"qoif");
        out.extend_from_slice(&w.to_be_bytes());
        out.extend_from_slice(&h.to_be_bytes());
        out.push(4);
        out.push(0);
        let mut left = (w * h) as usize;
        while left > 0 {
            let n = left.min(62);
            out.push(0xc0 | ((n - 1) as u8));
            left -= n;
        }
        out.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 1]);
        let img = decode_qoi(&out, (w as u16, h as u16)).unwrap();
        assert_eq!(img.width, w);
        assert_eq!(img.height, h);
        assert_eq!(img.rgba.len(), (w * h * 4) as usize);
    }

    #[test]
    fn ydg_parses_single_frame() {
        let frame = qoi_2px();
        let mut ydg = vec![0u8; FRAME_TABLE + 16];
        ydg[..12].copy_from_slice(&MAGIC);
        ydg[0x14..0x18].copy_from_slice(&((FRAME_TABLE + 16 + frame.len()) as u32).to_le_bytes());
        ydg[0x20..0x22].copy_from_slice(&2u16.to_le_bytes());
        ydg[0x22..0x24].copy_from_slice(&1u16.to_le_bytes());
        ydg[0x30..0x34].copy_from_slice(&1u32.to_le_bytes());
        let off = (FRAME_TABLE + 16) as u32;
        ydg[0x34..0x38].copy_from_slice(&off.to_le_bytes());
        ydg[0x38..0x3c].copy_from_slice(&(frame.len() as u32).to_le_bytes());
        ydg[0x3c] = 0;
        ydg[0x3e..0x40].copy_from_slice(&1u16.to_le_bytes());
        ydg.extend_from_slice(&frame);
        let parsed = parse(&ydg).unwrap();
        assert_eq!(parsed.width, 2);
        assert_eq!(parsed.height, 1);
        assert_eq!(parsed.frames.len(), 1);
        assert_eq!(parsed.frames[0].codec, YdgCodec::Qoi);
        let img = decode_qoi(parsed.frames[0].bytes, (2, 1)).unwrap();
        assert_eq!(img.rgba[0..4], [10, 20, 30, 40]);
    }
}
