//! `YMV` 影片容器(魔数 `YSMV`)· 帧序列 + 双平面(颜色 / alpha)。
//!
//! 布局(0.495 官方解析器 `0x459094` 逐指令取证;SDK 样例
//! `data/pac/{rain,sample2,sample3}.ymv` 逐字节闭合):
//!
//! ```text
//! +0x00 char[4] "YSMV"
//! +0x04 u32     版本(样例恒 1;官方解码器不校验,本层只记录)
//! +0x08 u32     帧数
//! +0x0C u32     alpha 标志(1 = 第二平面存在且为 alpha;官方
//!               `0x45934c` 用 `== 1` 置图层 `+0x86`)
//! +0x10 u32     宽
//! +0x14 u32     高
//! +0x18 u32     fps(官方上限 30,由制作工具约束;引擎原样返回)
//! +0x1C u32     未用(样例恒 0;官方解析器不读)
//! +0x20 u32[帧数]  帧偏移(相对文件首)
//! +…    u32[帧数]  颜色段长度
//! +…    u32[帧数]  alpha 段长度
//! 之后逐帧排布 [颜色段][alpha 段]
//! ```
//!
//! **每段都是独立 XOR 混淆的基线 JPEG**:官方 `0x459094` 在挂接时把整段
//! 就地按 `byte[i] ^= (i & 0x0f) + 0x10` 解混淆(`0x4591d8..0x45923b`,
//! 两段各自从 0 起算);解后首字节 `FF D8 FF E0`(`JFIF`),颜色段为 RGB、
//! alpha 段为灰度(实测 `rain` 8 位灰度 0..70、`sample2` 恒 255)。
//!
//! 本模块**只做容器切分与解混淆**,不解 JPEG、不碰 IO;像素在渲染端资产
//! 管线物化(见 `yu-gfx::TextureRef::Movie`)。
//!
//! 安全边界:表/段越界返回类型化 [`Diag`](`fmt.magic` / `fmt.truncated` /
//! `fmt.count` / `fmt.limit`),不 panic、不静默截断(官方不校验,本层校验
//! 仅用于不越界读取,不改变可见语义)。

use crate::code;
use crate::container::{truncated, u32_at};
use yu_log::{Boundary, Diag};

/// 容器魔数。
pub const MAGIC: [u8; 4] = *b"YSMV";

/// 头部字节数(到 `+0x20` 为止)。
pub const HEADER_BYTES: usize = 0x20;

/// XOR 解混淆的基数:第 `i` 字节异或 `(i & 0x0f) + XOR_BASE`。
pub const XOR_BASE: u8 = 0x10;

/// 影片元数据(与官方图层影片结构 `+0..+0x10` 一一对应)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct YmvInfo {
    /// `+0x04` 版本(样例恒 1)。
    pub version: u32,
    /// `+0x08` 帧数。
    pub frames: u32,
    /// `+0x10` 宽。
    pub width: u32,
    /// `+0x14` 高。
    pub height: u32,
    /// `+0x18` fps。
    pub fps: u32,
    /// `+0x0C == 1`:第二平面存在且为 alpha。
    pub has_alpha: bool,
}

/// 一帧的两个平面(仍是 **XOR 混淆**后的原始字节;用 [`decrypt_plane`] 还原)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct YmvFrame<'a> {
    /// 颜色平面(JPEG 混淆段)。
    pub color: &'a [u8],
    /// alpha 平面(JPEG 灰度混淆段;`has_alpha == false` 时为 `None`)。
    pub alpha: Option<&'a [u8]>,
}

/// 已解析的 `YSMV` 容器(借用原始字节,不复制平面)。
#[derive(Clone, Debug)]
pub struct Ymv<'a> {
    buf: &'a [u8],
    info: YmvInfo,
    /// 三张表的起点(`+0x20`)。
    tables: usize,
    /// 颜色平面段长度表起点(`+0x20 + frames*4`)。
    color_lens: usize,
    /// alpha 平面段长度表起点(`+0x20 + frames*8`)。
    alpha_lens: usize,
}

impl<'a> Ymv<'a> {
    /// 解析头部与三张表,并校验所有帧段都落在文件内。
    ///
    /// `origin` 只用于诊断定位(资产路径)。
    pub fn parse(buf: &'a [u8], origin: &[u8]) -> Result<Self, Diag> {
        if buf.len() < HEADER_BYTES {
            return Err(truncated(origin, HEADER_BYTES, buf.len(), "YSMV 头"));
        }
        if buf[0..4] != MAGIC {
            return Err(Diag::fatal(code::MAGIC, Boundary::Format)
                .at_data(origin, 0)
                .with_detail(format!(
                    "魔数不符:期望 \"YSMV\",实际 {:?}",
                    String::from_utf8_lossy(&buf[0..4])
                )));
        }
        let version = u32_at(buf, 4);
        let frames = u32_at(buf, 8);
        let alpha_flag = u32_at(buf, 0x0C);
        if frames == 0 {
            return Err(Diag::fatal(code::COUNT, Boundary::Format)
                .at_data(origin, 8)
                .with_detail("帧数为 0(官方解析器会按 0 帧建结构,无法出图)".to_string()));
        }
        let frames_usize = frames as usize;
        let table_bytes = frames_usize
            .checked_mul(12)
            .ok_or_else(|| limit_diag(origin, 8, "帧数 × 12 溢出 32 位"))?;
        let tables_end = HEADER_BYTES
            .checked_add(table_bytes)
            .ok_or_else(|| limit_diag(origin, 8, "表区终点溢出"))?;
        if buf.len() < tables_end {
            return Err(truncated(origin, tables_end, buf.len(), "YSMV 三张表"));
        }
        let ymv = Self {
            buf,
            info: YmvInfo {
                version,
                frames,
                width: u32_at(buf, 0x10),
                height: u32_at(buf, 0x14),
                fps: u32_at(buf, 0x18),
                has_alpha: alpha_flag == 1,
            },
            tables: HEADER_BYTES,
            color_lens: HEADER_BYTES + frames_usize * 4,
            alpha_lens: HEADER_BYTES + frames_usize * 8,
        };
        for i in 0..frames {
            ymv.span(i).ok_or_else(|| {
                Diag::fatal(code::LIMIT, Boundary::Format)
                    .at_data(origin, tables_end as u64)
                    .with_detail(format!(
                        "第 {i} 帧的段区间越界(偏移/长度表与文件长度 {} 不自洽)",
                        buf.len()
                    ))
            })?;
        }
        Ok(ymv)
    }

    /// 元数据。
    #[must_use]
    pub fn info(&self) -> YmvInfo {
        self.info
    }

    /// 帧数。
    #[must_use]
    pub fn frames(&self) -> u32 {
        self.info.frames
    }

    /// 第 `index` 帧的两个平面(原始混淆字节;越界返回 `None`)。
    ///
    /// alpha 标志为 0 时第二段长度按 0 处理(样例中该表项也是 0)。
    #[must_use]
    pub fn frame_planes(&self, index: u32) -> Option<YmvFrame<'a>> {
        let (start, color_len, alpha_len) = self.span(index)?;
        let color_end = start.checked_add(color_len as usize)?;
        let color = self.buf.get(start..color_end)?;
        let alpha = if self.info.has_alpha && alpha_len > 0 {
            let alpha_end = color_end.checked_add(alpha_len as usize)?;
            Some(self.buf.get(color_end..alpha_end)?)
        } else {
            None
        };
        Some(YmvFrame { color, alpha })
    }

    /// 第 `index` 帧解混淆后的 JPEG 字节(颜色,`Some` = alpha)。
    ///
    /// 每次调用都会复制并解混淆该帧两段(`rain` 800×600 ≈ 56 KB/帧),
    /// 渲染端按帧缓存,不要每帧重建。
    #[must_use]
    pub fn frame_jpegs(&self, index: u32) -> Option<(Vec<u8>, Option<Vec<u8>>)> {
        let planes = self.frame_planes(index)?;
        let color = decrypt_plane(planes.color);
        let alpha = planes.alpha.map(decrypt_plane);
        Some((color, alpha))
    }

    /// 第 `index` 帧的 `(段起点, 颜色段长, alpha 段长)`;越界返回 `None`。
    ///
    /// 表项与段区间在 [`Ymv::parse`] 已校验,这里只做边界复核(公开接口
    /// 允许对任意下标调用)。
    #[must_use]
    pub fn frame_spans(&self, index: u32) -> Option<(usize, usize, usize)> {
        self.span(index)
            .map(|(start, color, alpha)| (start, color as usize, alpha as usize))
    }

    /// 第 `index` 帧的 `(段起点, 颜色段长, alpha 段长)`;越界返回 `None`。
    fn span(&self, index: u32) -> Option<(usize, u32, u32)> {
        let i = index as usize;
        if index >= self.info.frames {
            return None;
        }
        let start = u32_at(self.buf, self.tables + i * 4) as usize;
        let color_len = u32_at(self.buf, self.color_lens + i * 4);
        let alpha_len = if self.info.has_alpha {
            u32_at(self.buf, self.alpha_lens + i * 4)
        } else {
            0
        };
        let end = start
            .checked_add(color_len as usize)?
            .checked_add(alpha_len as usize)?;
        if end > self.buf.len() || start < self.alpha_lens + self.info.frames as usize * 4 {
            return None;
        }
        Some((start, color_len, alpha_len))
    }
}

/// 解混淆一段平面:第 `i` 字节异或 `(i & 0x0f) + [`XOR_BASE`]`。
#[must_use]
pub fn decrypt_plane(plane: &[u8]) -> Vec<u8> {
    let mut out = plane.to_vec();
    decrypt_in_place(&mut out);
    out
}

/// 就地解混淆(渲染端缓存已复制过的段时省一次分配)。
pub fn decrypt_in_place(plane: &mut [u8]) {
    for (i, byte) in plane.iter_mut().enumerate() {
        *byte ^= (i as u8 & 0x0f) + XOR_BASE;
    }
}

/// **常驻形态**的 `YSMV` 容器:整份字节持有、全部平面已解混淆。
///
/// 对应官方 `0x459094` 的产物 —— 它在 `YMV[FILE]` 命令内读入整份影片并把
/// 每帧两段就地解混淆(`0x4591d8..0x45923b` 的双重循环),之后 `YMV[FRAME]`
/// (`0x45934c`)只在这份缓冲上切段。播放期每帧的固定成本因此只有
/// **两张 JPEG 解码**,没有文件 IO、没有 XOR。
///
/// 借用形态的 [`Ymv`] 适合一次性检查(元数据/单帧);要反复换帧的渲染端用
/// 本类型。
#[derive(Clone, Debug)]
pub struct DecryptedYmv {
    buf: Vec<u8>,
    info: YmvInfo,
    spans: Vec<(usize, usize, usize)>,
}

impl DecryptedYmv {
    /// 读入整份字节并解混淆所有平面(官方 `0x459094` 的等价形态)。
    pub fn parse(bytes: &[u8], origin: &[u8]) -> Result<Self, Diag> {
        let borrowed = Ymv::parse(bytes, origin)?;
        let mut buf = bytes.to_vec();
        let mut spans = Vec::with_capacity(borrowed.frames() as usize);
        for i in 0..borrowed.frames() {
            let (start, color_len, alpha_len) = borrowed
                .frame_spans(i)
                .ok_or_else(|| limit_diag(origin, 0, format!("第 {i} 帧段区间不可读")))?;
            let color_end = start + color_len;
            decrypt_in_place(&mut buf[start..color_end]);
            if alpha_len > 0 {
                decrypt_in_place(&mut buf[color_end..color_end + alpha_len]);
            }
            spans.push((start, color_len, alpha_len));
        }
        Ok(Self {
            buf,
            info: borrowed.info(),
            spans,
        })
    }

    /// 元数据。
    #[must_use]
    pub fn info(&self) -> YmvInfo {
        self.info
    }

    /// 第 `index` 帧**已解混淆**的两段(零拷贝)。
    #[must_use]
    pub fn frame_planes(&self, index: u32) -> Option<(&[u8], Option<&[u8]>)> {
        let (start, color_len, alpha_len) = *self.spans.get(index as usize)?;
        let color = self.buf.get(start..start + color_len)?;
        let alpha = if alpha_len > 0 {
            Some(self.buf.get(start + color_len..start + color_len + alpha_len)?)
        } else {
            None
        };
        Some((color, alpha))
    }

    /// 常驻字节数(诊断/预算用)。
    #[must_use]
    pub fn resident_bytes(&self) -> usize {
        self.buf.len()
    }
}

fn limit_diag(origin: &[u8], at: u64, detail: impl Into<String>) -> Diag {
    Diag::fatal(code::LIMIT, Boundary::Format)
        .at_data(origin, at)
        .with_detail(detail.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一个 `frames` 帧的样例:段内容 = `(帧号, 平面号, 序号)` 的伪 JPEG,
    /// 混淆方式与官方一致。
    fn build(frames: u32, with_alpha: bool, w: u32, h: u32, fps: u32) -> Vec<u8> {
        let n = frames as usize;
        let mut color_planes: Vec<Vec<u8>> = Vec::new();
        let mut alpha_planes: Vec<Vec<u8>> = Vec::new();
        for i in 0..n {
            let mut c: Vec<u8> = (0..17)
                .map(|k| (i as u8).wrapping_mul(7).wrapping_add(k as u8))
                .collect();
            c[0] = 0xFF;
            c[1] = 0xD8;
            decrypt_in_place(&mut c);
            color_planes.push(c);
            let mut a: Vec<u8> = (0..9).map(|k| (k as u8).wrapping_add(0x30)).collect();
            a[0] = 0xFF;
            a[1] = 0xD8;
            decrypt_in_place(&mut a);
            alpha_planes.push(a);
        }
        let tables_end = HEADER_BYTES + n * 12;
        let mut out = Vec::new();
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&frames.to_le_bytes());
        out.extend_from_slice(&(u32::from(with_alpha)).to_le_bytes());
        out.extend_from_slice(&w.to_le_bytes());
        out.extend_from_slice(&h.to_le_bytes());
        out.extend_from_slice(&fps.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        let mut off = tables_end;
        for c in &color_planes {
            out.extend_from_slice(&(off as u32).to_le_bytes());
            off += c.len() + alpha_planes[0].len();
        }
        for c in &color_planes {
            out.extend_from_slice(&(c.len() as u32).to_le_bytes());
        }
        for a in &alpha_planes {
            out.extend_from_slice(&(a.len() as u32).to_le_bytes());
        }
        for (c, a) in color_planes.iter().zip(alpha_planes.iter()) {
            out.extend_from_slice(c);
            out.extend_from_slice(a);
        }
        out
    }

    #[test]
    fn parses_header_and_planes() {
        let bytes = build(3, true, 40, 30, 30);
        let ymv = Ymv::parse(&bytes, b"pac/test.ymv").expect("parse");
        let info = ymv.info();
        assert_eq!(info.version, 1);
        assert_eq!(info.frames, 3);
        assert_eq!((info.width, info.height, info.fps), (40, 30, 30));
        assert!(info.has_alpha);

        for i in 0..3u32 {
            let (color, alpha) = ymv.frame_jpegs(i).expect("frame");
            assert_eq!(&color[0..2], &[0xFF, 0xD8]);
            assert_eq!(color.len(), 17);
            let alpha = alpha.expect("alpha");
            assert_eq!(&alpha[0..2], &[0xFF, 0xD8]);
            assert_eq!(alpha.len(), 9);
        }
        assert!(ymv.frame_planes(3).is_none());
    }

    #[test]
    fn alpha_flag_zero_yields_no_second_plane() {
        let bytes = build(1, false, 1, 1, 24);
        let ymv = Ymv::parse(&bytes, b"x").expect("parse");
        assert!(!ymv.info().has_alpha);
        let frame = ymv.frame_planes(0).expect("frame");
        assert!(frame.alpha.is_none());
        assert!(ymv.frame_jpegs(0).expect("jpegs").1.is_none());
    }

    #[test]
    fn rejects_bad_magic_and_truncation() {
        let mut bytes = build(1, true, 1, 1, 24);
        bytes[0] = b'X';
        assert_eq!(
            Ymv::parse(&bytes, b"x").expect_err("magic").code,
            code::MAGIC
        );

        let bytes = build(1, true, 1, 1, 24);
        assert_eq!(
            Ymv::parse(&bytes[..10], b"x").expect_err("short").code,
            code::TRUNCATED
        );
        assert_eq!(
            Ymv::parse(&bytes[..HEADER_BYTES + 4], b"x")
                .expect_err("tables cut")
                .code,
            code::TRUNCATED
        );
    }

    #[test]
    fn rejects_frame_span_outside_file() {
        let mut bytes = build(2, true, 1, 1, 24);
        // 把第 0 帧的颜色段长度改成超大值。
        let at = HEADER_BYTES + 2 * 4;
        bytes[at..at + 4].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        assert_eq!(
            Ymv::parse(&bytes, b"x").expect_err("span").code,
            code::LIMIT
        );
    }

    #[test]
    fn rejects_zero_frames() {
        let bytes = build(1, true, 1, 1, 24);
        let mut b = bytes.clone();
        b[8..12].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(
            Ymv::parse(&b, b"x").expect_err("zero").code,
            code::COUNT
        );
    }

    /// 官方 SDK 样例(需要 `YURIS_SAMPLE_SDK0495`)。
    ///
    /// 缺样本时跳过;CI/取证用 `YURIS_REQUIRE_SAMPLES=1` 强制要求(禁止空跑通过)。
    #[test]
    fn sdk_samples_parse() {
        let Ok(root) = std::env::var("YURIS_SAMPLE_SDK0495") else {
            if std::env::var("YURIS_REQUIRE_SAMPLES").ok().as_deref() == Some("1") {
                panic!("YURIS_REQUIRE_SAMPLES=1 但 YURIS_SAMPLE_SDK0495 未设置");
            }
            return;
        };
        // 样本电影属于数据目录而非 `システム` 本体:两者都试。
        let dirs = [
            format!("{root}/data/pac"),
            format!("{root}/../data/pac"),
        ];
        let mut checked = 0usize;
        for dir in dirs {
            for (name, frames, w, h, fps) in [
                ("rain.ymv", 20u32, 800u32, 600u32, 24u32),
                ("sample2.ymv", 306, 400, 300, 30),
                ("sample3.ymv", 306, 400, 300, 30),
            ] {
                let path = format!("{dir}/{name}");
                let Ok(bytes) = std::fs::read(&path) else {
                    continue;
                };
                let ymv = Ymv::parse(&bytes, path.as_bytes()).expect("parse sample");
                let info = ymv.info();
                assert_eq!(info.frames, frames, "{name}");
                assert_eq!((info.width, info.height, info.fps), (w, h, fps), "{name}");
                assert!(info.has_alpha, "{name}");
                let last = ymv
                    .frame_jpegs(frames - 1)
                    .unwrap_or_else(|| panic!("{name} 末帧可解"));
                assert_eq!(&last.0[0..4], &[0xFF, 0xD8, 0xFF, 0xE0], "{name} 颜色段 JPEG");
                let alpha = last.1.expect("alpha");
                assert_eq!(&alpha[0..4], &[0xFF, 0xD8, 0xFF, 0xE0], "{name} alpha 段 JPEG");
                checked += 1;
            }
        }
        if checked == 0 && std::env::var("YURIS_REQUIRE_SAMPLES").ok().as_deref() == Some("1") {
            panic!("YURIS_REQUIRE_SAMPLES=1 但 {root} 下未找到 data/pac/*.ymv");
        }
    }
}
