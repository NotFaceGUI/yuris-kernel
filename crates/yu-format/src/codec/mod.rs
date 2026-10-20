//! 文本编解码器:CP932 与 GBK 的静态码表 + decode/encode。
//!
//! - 码表由 `crates/yu-format/gen_tables.py` 从 **Windows kernel32 码页**
//!   (932/936)生成(`MultiByteToWideChar` / `WideCharToMultiByte`),
//!   即引擎运行平台的系统映射;生成物入库,勿手改。
//! - 解码严格:非法/不完整序列返回 `fmt.codec.invalid`(带字节偏移),
//!   不做替换、不做猜测。
//! - 编码取系统首选形态:CP932 的 NEC/IBM 别名行(398 条"多码一符")
//!   编码时回落到系统首选码;`encode∘decode` 对别名码不保字节,但
//!   `decode∘encode` 恒等(全表往返测试守卫)。GBK 零别名,双向全等。
//! - auto 评分器见 [`auto`] 与 `docs/architecture.md` §7。

pub mod auto;

mod table_cp932;
mod table_gbk;

use std::collections::HashMap;
use std::sync::OnceLock;

use yu_log::{Boundary, Diag};

use crate::code;

/// 一张生成码表的静态访问面(两个码表共用同一形状)。
struct TableRef<const W: usize> {
    single_high: &'static [u16; 0x80],
    rows: &'static [[u16; W]],
    lead_lo: u8,
    lead_hi: u8,
    trail_lo: u8,
    trail_hi: u8,
}

const CP932_TABLE: TableRef<189> = TableRef {
    single_high: &table_cp932::SINGLE_HIGH,
    rows: &table_cp932::ROWS,
    lead_lo: table_cp932::LEAD_LO,
    lead_hi: table_cp932::LEAD_HI,
    trail_lo: table_cp932::TRAIL_LO,
    trail_hi: table_cp932::TRAIL_HI,
};

const GBK_TABLE: TableRef<191> = TableRef {
    single_high: &table_gbk::SINGLE_HIGH,
    rows: &table_gbk::ROWS,
    lead_lo: table_gbk::LEAD_LO,
    lead_hi: table_gbk::LEAD_HI,
    trail_lo: table_gbk::TRAIL_LO,
    trail_hi: table_gbk::TRAIL_HI,
};

impl<const W: usize> TableRef<W> {
    /// 从 `bytes[at]`(高位字节)起解码一步;
    /// `Some((码位, 消费字节数))`,`None` = 非法。
    ///
    /// 调用方约定:`bytes[at] >= 0x80`(ASCII 由主循环直通)。
    fn step(&self, bytes: &[u8], at: usize) -> Option<(u16, usize)> {
        let lead = bytes[at];
        let single = self.single_high[usize::from(lead) - 0x80];
        if single != 0 {
            return Some((single, 1));
        }
        if at + 1 < bytes.len() && (self.lead_lo..=self.lead_hi).contains(&lead) {
            let trail = bytes[at + 1];
            if (self.trail_lo..=self.trail_hi).contains(&trail) {
                let unit = self.rows[usize::from(lead - self.lead_lo)]
                    [usize::from(trail - self.trail_lo)];
                if unit != 0 {
                    return Some((unit, 2));
                }
            }
        }
        None
    }

    /// 字符 → 码位反查表(先到先得 + 系统首选覆盖;测试用)。
    fn build_map(&self, overrides: &[(u16, u16)]) -> HashMap<char, u16> {
        let mut map = HashMap::with_capacity(24_000);
        for (i, &unit) in self.single_high.iter().enumerate() {
            if unit != 0 {
                if let Some(ch) = char::from_u32(u32::from(unit)) {
                    map.insert(ch, 0x80 + i as u16);
                }
            }
        }
        for (r, row) in self.rows.iter().enumerate() {
            let lead = u16::from(self.lead_lo + r as u8);
            for (c, &unit) in row.iter().enumerate() {
                if unit != 0 {
                    if let Some(ch) = char::from_u32(u32::from(unit)) {
                        // 多码一符:先到先得,再用系统首选覆盖表校正,
                        // 最终编码与 Windows WideCharToMultiByte 逐字一致。
                        let trail = u16::from(self.trail_lo + c as u8);
                        map.entry(ch).or_insert(lead | (trail << 8));
                    }
                }
            }
        }
        for &(unit, code_point) in overrides {
            if let Some(ch) = char::from_u32(u32::from(unit)) {
                map.insert(ch, code_point);
            }
        }
        map
    }
}

/// 运行配置中的文本编解码器。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Codec {
    /// Windows CP932(日语;半角假名 = 0xA1..=0xDF 单字节)。
    Cp932,
    /// Windows CP936 / GBK(简中;0x80 → U+20AC)。
    Gbk,
}

impl Codec {
    /// 全部编解码器(顺序稳定,供 CLI 枚举)。
    pub const ALL: [Codec; 2] = [Codec::Cp932, Codec::Gbk];

    /// 稳定名(进 trace / golden 头部)。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Codec::Cp932 => "cp932",
            Codec::Gbk => "gbk",
        }
    }

    /// 按名取编解码器(未知名 = `fmt.codec.name`)。
    pub fn from_name(name: &str) -> Result<Self, Diag> {
        match name {
            "cp932" | "sjis" | "shift_jis" => Ok(Codec::Cp932),
            "gbk" | "cp936" | "gb2312" => Ok(Codec::Gbk),
            other => Err(Diag::fatal(code::CODEC_NAME, Boundary::Format).with_detail(format!(
                "未知编解码器 `{other}`;可用:{}",
                Codec::ALL
                    .iter()
                    .map(|c| c.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))),
        }
    }

    /// 字节 → Unicode(严格)。`origin` 用于诊断定位。
    pub fn decode(self, bytes: &[u8], origin: &[u8]) -> Result<String, Diag> {
        let mut out = String::with_capacity(bytes.len());
        let mut i = 0usize;
        while i < bytes.len() {
            let b = bytes[i];
            if b < 0x80 {
                out.push(b as char);
                i += 1;
                continue;
            }
            let step = match self {
                Codec::Cp932 => CP932_TABLE.step(bytes, i),
                Codec::Gbk => GBK_TABLE.step(bytes, i),
            };
            match step {
                Some((unit, width)) => {
                    // 生成表保证非零码位均为合法 BMP 标量
                    out.push(char::from_u32(u32::from(unit)).ok_or_else(|| {
                        invalid(origin, i, &format!("码位 U+{unit:04X} 非法"))
                    })?);
                    i += width;
                }
                None => {
                    return Err(invalid(
                        origin,
                        i,
                        &format!("非法/不完整字节序列,起点 0x{b:02X}"),
                    ));
                }
            }
        }
        Ok(out)
    }

    /// Unicode → 字节(严格;不可映射字符 = `fmt.codec.invalid`)。
    pub fn encode(self, text: &str, origin: &[u8]) -> Result<Vec<u8>, Diag> {
        let map = encode_map(self);
        let mut out = Vec::with_capacity(text.len());
        for (index, ch) in text.char_indices() {
            let u = u32::from(ch);
            if u < 0x80 {
                out.push(u as u8);
                continue;
            }
            match map.get(&ch) {
                Some(&code_point) => {
                    if code_point < 0x100 {
                        out.push(code_point as u8);
                    } else {
                        out.push((code_point & 0xFF) as u8);
                        out.push((code_point >> 8) as u8);
                    }
                }
                None => {
                    return Err(Diag::fatal(code::CODEC_INVALID, Boundary::Format)
                        .at_data(origin, index as u64)
                        .with_detail(format!(
                            "字符 U+{u:04X}({ch:?}) 在 {} 中不可映射",
                            self.as_str()
                        )));
                }
            }
        }
        Ok(out)
    }

    /// 生成器审计常量((双字节条目数, 编码别名数));测试与文档核对用。
    #[doc(hidden)]
    #[must_use]
    pub fn table_audit(self) -> (usize, usize) {
        match self {
            Codec::Cp932 => (table_cp932::DOUBLE_ENTRIES, table_cp932::ALIASES),
            Codec::Gbk => (table_gbk::DOUBLE_ENTRIES, table_gbk::ALIASES),
        }
    }
}

fn invalid(origin: &[u8], at: usize, detail: &str) -> Diag {
    Diag::fatal(code::CODEC_INVALID, Boundary::Format)
        .at_data(origin, at as u64)
        .with_detail(detail.to_string())
}

/// 字符 → 码位的静态反查表(首次编码时构建;生成表零别名时即双射)。
fn encode_map(codec: Codec) -> &'static HashMap<char, u16> {
    static CP932: OnceLock<HashMap<char, u16>> = OnceLock::new();
    static GBK: OnceLock<HashMap<char, u16>> = OnceLock::new();
    match codec {
        Codec::Cp932 => {
            CP932.get_or_init(|| CP932_TABLE.build_map(&table_cp932::ALIAS_OVERRIDES))
        }
        Codec::Gbk => GBK.get_or_init(|| GBK_TABLE.build_map(&table_gbk::ALIAS_OVERRIDES)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 全表遍历(单码表):每格 decode/encode 往返 + 条目数核对。
    /// CP932 的 398 条别名允许 encode 换码(Windows 首选),但
    /// `decode∘encode` 恒等必须全部成立;GBK 零别名,字节级双向全等。
    fn roundtrip_table<const W: usize>(
        codec: Codec,
        table: &TableRef<W>,
        expect_entries: usize,
        aliases: usize,
    ) {
        let mut checked = 0usize;
        for (r, row) in table.rows.iter().enumerate() {
            let lead = table.lead_lo + r as u8;
            for (c, &unit) in row.iter().enumerate() {
                if unit == 0 {
                    continue;
                }
                checked += 1;
                let bytes = [lead, table.trail_lo + c as u8];
                let ch = char::from_u32(u32::from(unit)).unwrap();
                assert_eq!(
                    codec.decode(&bytes, b"t").unwrap(),
                    ch.to_string(),
                    "{bytes:?}"
                );
                let re = codec.encode(ch.to_string().as_str(), b"t").unwrap();
                assert_eq!(
                    codec.decode(&re, b"t").unwrap(),
                    ch.to_string(),
                    "往返 {bytes:?}"
                );
                if aliases == 0 || re == bytes {
                    assert_eq!(re, bytes, "字节往返 {bytes:?}");
                }
            }
        }
        assert_eq!(checked, expect_entries, "条目数应与生成器审计常量一致");
    }

    #[test]
    fn full_table_roundtrip_cp932() {
        let (entries, aliases) = Codec::Cp932.table_audit();
        roundtrip_table(Codec::Cp932, &CP932_TABLE, entries, aliases);
    }

    #[test]
    fn full_table_roundtrip_gbk() {
        let (entries, aliases) = Codec::Gbk.table_audit();
        roundtrip_table(Codec::Gbk, &GBK_TABLE, entries, aliases);
    }

    /// 单字节区遍历:合法字节 decode/encode 往返,非法字节必须报错。
    fn single_byte_roundtrip(codec: Codec) {
        let high: &[u16; 0x80] = match codec {
            Codec::Cp932 => CP932_TABLE.single_high,
            Codec::Gbk => GBK_TABLE.single_high,
        };
        for b in 0x00u16..0x100 {
            let bytes = [b as u8];
            let decoded = codec.decode(&bytes, b"t");
            if b < 0x80 {
                assert_eq!(decoded.unwrap(), (b as u8 as char).to_string());
                continue;
            }
            let unit = high[usize::from(b) - 0x80];
            if unit == 0 {
                assert!(decoded.is_err(), "0x{b:02X} 应为非法");
            } else {
                let ch = char::from_u32(u32::from(unit)).unwrap();
                assert_eq!(decoded.unwrap(), ch.to_string());
                assert_eq!(codec.encode(ch.to_string().as_str(), b"t").unwrap(), bytes);
            }
        }
    }

    #[test]
    fn single_byte_roundtrip_cp932() {
        single_byte_roundtrip(Codec::Cp932);
    }

    #[test]
    fn single_byte_roundtrip_gbk() {
        single_byte_roundtrip(Codec::Gbk);
    }
}
