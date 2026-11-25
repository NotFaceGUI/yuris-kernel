//! 文本编解码器:CP932 / GBK / Big5 的静态码表 + decode/encode。
//!
//! - 码表由 `crates/yu-format/gen_tables.py` 从 **Windows kernel32 码页**
//!   (932/936/950)生成(`MultiByteToWideChar` / `WideCharToMultiByte`),
//!   即引擎运行平台的系统映射;生成物入库,勿手改。
//! - 解码严格:非法/不完整序列返回 `fmt.codec.invalid`(带字节偏移),
//!   不做替换、不做猜测。
//! - 编码取系统首选形态:CP932 的 NEC/IBM 别名行(398 条"多码一符")
//!   编码时回落到系统首选码;`encode∘decode` 对别名码不保字节,但
//!   `decode∘encode` 恒等(全表往返测试守卫)。GBK 零别名,双向全等。
//! - auto 评分器见 [`auto`] 与 `docs/architecture.md` §7。

pub mod auto;

mod table_big5;
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

const BIG5_TABLE: TableRef<191> = TableRef {
    single_high: &table_big5::SINGLE_HIGH,
    rows: &table_big5::ROWS,
    lead_lo: table_big5::LEAD_LO,
    lead_hi: table_big5::LEAD_HI,
    trail_lo: table_big5::TRAIL_LO,
    trail_hi: table_big5::TRAIL_HI,
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
    /// Windows CP950 / Big5(繁中;港台旧作与部分汉化补丁用)。
    Big5,
    /// UTF-8(变长 1..=4 字节;2025 年本地化发行版实见)。
    ///
    /// 证据:Kagura 本地化版脚本字面量是 UTF-8(如 `終了確認`、
    /// `ルビ設定開始文字が不正です。`);按 CP932/GBK 步进会把多字节字符
    /// 切错,游戏自身的 ruby 解析器随即拒绝剧本。
    Utf8,
}

impl Codec {
    /// 全部编解码器(顺序稳定,供 CLI 枚举)。
    pub const ALL: [Codec; 4] = [Codec::Cp932, Codec::Gbk, Codec::Big5, Codec::Utf8];

    /// 稳定名(进 trace / golden 头部)。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Codec::Cp932 => "cp932",
            Codec::Gbk => "gbk",
            Codec::Big5 => "big5",
            Codec::Utf8 => "utf8",
        }
    }

    /// 该编解码器下是否为多字节字符的首字节。
    #[must_use]
    pub const fn is_lead(self, byte: u8) -> bool {
        match self {
            Codec::Cp932 => matches!(byte, 0x81..=0x9F | 0xE0..=0xFC),
            // GBK 与 Big5 的前导区间相同(0x81..=0xFE);差别在合法 trail
            // (Big5 的 0x40..=0x7E 段 GBK 也有,但码位表不同)。
            Codec::Gbk | Codec::Big5 => matches!(byte, 0x81..=0xFE),
            // UTF-8 多字节首字节;0xC0/0xC1(overlong)与 0xF5..=0xFF 非法。
            Codec::Utf8 => matches!(byte, 0xC2..=0xF4),
        }
    }

    /// 该编解码器下多字节字符的合法尾字节区间。
    ///
    /// 三种双字节编码的合法 trail 都不含 `0x22`(界定符 `"`)与其它 ASCII
    /// 标点:CP932 = `0x40..=0xFC` 去 `0x7F`;GBK = `0x40..=0x7E` +
    /// `0x80..=0xFE`;Big5 = `0x40..=0x7E` + `0xA1..=0xFE`。UTF-8 的
    /// continuation 恒为 `0x80..=0xBF`,同样不含 `0x22`。
    #[must_use]
    pub const fn is_trail(self, byte: u8) -> bool {
        match self {
            Codec::Cp932 => matches!(byte, 0x40..=0xFC) && byte != 0x7F,
            Codec::Gbk => matches!(byte, 0x40..=0x7E | 0x80..=0xFE),
            Codec::Big5 => matches!(byte, 0x40..=0x7E | 0xA1..=0xFE),
            Codec::Utf8 => matches!(byte, 0x80..=0xBF),
        }
    }

    /// `(lead, next)` 是否构成该编解码器下完整的双字节前缀。
    ///
    /// 字面量/文本扫描用这个而不是只看 [`Codec::is_lead`]:lead 后跟的字节
    /// 不是合法 trail 时按单字节处理,否则会把 ASCII 界定符吞进「双字节」
    /// (`<v555-kuro>` 实测:exe 静态表 = Big5,SJIS 半角假名 `0xA1..=0xDF`
    /// 在 Big5 下是 lead,后面紧跟闭合引号即报「字面量未闭合」)。
    ///
    /// UTF-8 下这是「多字节前缀」:3/4 字节字符的第 2 字节同样落在
    /// `0x80..=0xBF`,判定仍成立;真正步进看 [`Self::char_len_at`]。
    #[must_use]
    pub const fn forms_two_byte(self, lead: u8, next: u8) -> bool {
        self.is_lead(lead) && self.is_trail(next)
    }

    /// 从 `bytes[i]` 起的字符宽度(CP932/GBK/Big5 = 1..=2,UTF-8 = 1..=4;
    /// 非法/截断按 1)。
    #[must_use]
    pub fn char_len_at(self, bytes: &[u8], i: usize) -> usize {
        let Some(&byte) = bytes.get(i) else {
            return 1;
        };
        match self {
            Codec::Utf8 => utf8_width(byte).unwrap_or(1),
            _ => {
                if self.is_lead(byte) && i + 1 < bytes.len() {
                    2
                } else {
                    1
                }
            }
        }
    }

    /// 解一个字符(无分配)。非法/截断返回 `None`。
    #[must_use]
    pub fn next_char(self, bytes: &[u8], i: usize) -> Option<(char, usize)> {
        let byte = *bytes.get(i)?;
        if byte < 0x80 {
            return Some((byte as char, 1));
        }
        if self == Codec::Utf8 {
            let width = utf8_width(byte)?;
            let text = std::str::from_utf8(bytes.get(i..i + width)?).ok()?;
            return Some((text.chars().next()?, width));
        }
        let (unit, width) = match self {
            Codec::Cp932 => CP932_TABLE.step(bytes, i),
            Codec::Gbk => GBK_TABLE.step(bytes, i),
            Codec::Big5 => BIG5_TABLE.step(bytes, i),
            Codec::Utf8 => unreachable!("UTF-8 已在上面单独处理"),
        }?;
        Some((char::from_u32(u32::from(unit))?, width))
    }

    /// 宽容解码:非法/截断记一个 U+FFFD 并计数。
    #[must_use]
    pub fn decode_lossy(self, bytes: &[u8]) -> (Vec<char>, usize) {
        let mut chars = Vec::with_capacity(bytes.len());
        let mut invalid = 0usize;
        let mut i = 0usize;
        while i < bytes.len() {
            if let Some((ch, width)) = self.next_char(bytes, i) {
                chars.push(ch);
                i += width;
            } else {
                chars.push(char::REPLACEMENT_CHARACTER);
                invalid += 1;
                i += self.char_len_at(bytes, i).min(bytes.len() - i);
            }
        }
        (chars, invalid)
    }

    /// 单字符 → 码位(ASCII 恒真;不可映射返回 `None`)。
    ///
    /// 与 [`Self::encode`] 共用同一张反查表,但不构造中间字符串;翻译加载
    /// 时逐字编码/隧道回退用。UTF-8 是变长编码,没有单码位表示:
    /// 只有 ASCII 返回码位,非 ASCII 返回 `None`(翻译路线暂不支持 UTF-8;
    /// 整串往返请用 [`Self::encode`])。
    #[must_use]
    pub fn encode_char(self, ch: char) -> Option<u16> {
        let u = u32::from(ch);
        if u < 0x80 {
            return Some(u as u16);
        }
        if self == Codec::Utf8 {
            return None;
        }
        encode_map(self).get(&ch).copied()
    }

    /// 该字符能否用本 codec 表示(ASCII 恒真)。
    #[must_use]
    pub fn can_encode(self, ch: char) -> bool {
        if self == Codec::Utf8 {
            return true;
        }
        ch.is_ascii() || encode_map(self).contains_key(&ch)
    }

    /// 按名取编解码器(未知名 = `fmt.codec.name`)。
    pub fn from_name(name: &str) -> Result<Self, Diag> {
        match name {
            "cp932" | "sjis" | "shift_jis" => Ok(Codec::Cp932),
            "gbk" | "cp936" | "gb2312" => Ok(Codec::Gbk),
            "big5" | "cp950" | "big-5" => Ok(Codec::Big5),
            "utf8" | "utf-8" => Ok(Codec::Utf8),
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
        if self == Codec::Utf8 {
            return match std::str::from_utf8(bytes) {
                Ok(text) => Ok(text.to_string()),
                Err(error) => {
                    let at = error.valid_up_to();
                    Err(invalid(
                        origin,
                        at,
                        &format!(
                            "非法/不完整 UTF-8 序列,起点 0x{:02X}",
                            bytes.get(at).copied().unwrap_or(0)
                        ),
                    ))
                }
            };
        }
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
                Codec::Big5 => BIG5_TABLE.step(bytes, i),
                Codec::Utf8 => unreachable!("UTF-8 已在函数开头处理"),
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
        if self == Codec::Utf8 {
            return Ok(text.as_bytes().to_vec());
        }
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

    /// Unicode → 字节(**宽松**;不可映射字符按 Windows ANSI 口径写 `?`)。
    ///
    /// 只给「宿主 → 脚本」的路径注入用:官方是 Win32 ANSI 程序,
    /// `GetModuleFileNameA` 对系统代码页不可表示的字符就写 `?`。内核的
    /// `$_SAVE_PATH` 等路径变量必须与脚本/存档层看到同一个投影,否则
    /// 含中文/韩文等的工程根会在 `sv.path.escape` 永远对不上
    /// (2026-11-25 `<v555-2>` 实测:根目录的中文被写成 `?`)。
    ///
    /// 与 `yu-engine::host_paths::script_text_bytes` 同口径;两处改动必须
    /// 同步(后续可收敛到本函数)。
    #[must_use]
    pub fn encode_lossy(self, text: &str, origin: &[u8]) -> Vec<u8> {
        if self == Codec::Utf8 {
            return text.as_bytes().to_vec();
        }
        match self.encode(text, origin) {
            Ok(bytes) => bytes,
            Err(_) => {
                let map = encode_map(self);
                let mut out = Vec::with_capacity(text.len());
                for ch in text.chars() {
                    let u = u32::from(ch);
                    if u < 0x80 {
                        out.push(u as u8);
                        continue;
                    }
                    match map.get(&ch) {
                        Some(&code_point) if code_point < 0x100 => out.push(code_point as u8),
                        Some(&code_point) => {
                            out.push((code_point & 0xFF) as u8);
                            out.push((code_point >> 8) as u8);
                        }
                        None => out.push(b'?'),
                    }
                }
                out
            }
        }
    }

    /// 生成器审计常量((双字节条目数, 编码别名数));测试与文档核对用。
    #[doc(hidden)]
    #[must_use]
    pub fn table_audit(self) -> (usize, usize) {
        match self {
            Codec::Cp932 => (table_cp932::DOUBLE_ENTRIES, table_cp932::ALIASES),
            Codec::Gbk => (table_gbk::DOUBLE_ENTRIES, table_gbk::ALIASES),
            Codec::Big5 => (table_big5::DOUBLE_ENTRIES, table_big5::ALIASES),
            Codec::Utf8 => (0, 0),
        }
    }
}

/// UTF-8 多字节首字节对应的字符宽度(`None` = ASCII 或非法首字节)。
///
/// 只做宽度判定;overlong/代理区/超范围由 `std::str::from_utf8` 严格拒绝。
const fn utf8_width(byte: u8) -> Option<usize> {
    match byte {
        0xC2..=0xDF => Some(2),
        0xE0..=0xEF => Some(3),
        0xF0..=0xF4 => Some(4),
        _ => None,
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
    static BIG5: OnceLock<HashMap<char, u16>> = OnceLock::new();
    static EMPTY: OnceLock<HashMap<char, u16>> = OnceLock::new();
    match codec {
        Codec::Cp932 => {
            CP932.get_or_init(|| CP932_TABLE.build_map(&table_cp932::ALIAS_OVERRIDES))
        }
        Codec::Gbk => GBK.get_or_init(|| GBK_TABLE.build_map(&table_gbk::ALIAS_OVERRIDES)),
        Codec::Big5 => BIG5.get_or_init(|| BIG5_TABLE.build_map(&table_big5::ALIAS_OVERRIDES)),
        // UTF-8 没有反查表;所有调用点都应提前分支。这里给空表兜底,
        // 避免新增编码时把 panic 带进生产路径。
        Codec::Utf8 => EMPTY.get_or_init(HashMap::new),
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

    #[test]
    fn full_table_roundtrip_big5() {
        let (entries, aliases) = Codec::Big5.table_audit();
        roundtrip_table(Codec::Big5, &BIG5_TABLE, entries, aliases);
    }

    /// UTF-8:1..=4 字节步进、严格解码、变长字符宽度。
    #[test]
    fn utf8_roundtrip_and_stepping() {
        let codec = Codec::Utf8;
        let text = "終了確認|ルビ設定開始文字が不正です。abc｜😀";
        let bytes = codec.encode(text, b"t").unwrap();
        assert_eq!(codec.decode(&bytes, b"t").unwrap(), text);
        let mut offset = 0usize;
        let mut chars = 0usize;
        while offset < bytes.len() {
            let (ch, width) = codec
                .next_char(&bytes, offset)
                .unwrap_or_else(|| panic!("offset {offset} 应可解码"));
            assert_eq!(width, ch.len_utf8(), "字符 {ch:?} 宽度");
            assert_eq!(codec.char_len_at(&bytes, offset), width);
            offset += width;
            chars += 1;
        }
        assert_eq!(chars, text.chars().count());
        assert_eq!(codec.char_len_at("｜".as_bytes(), 0), 3);
        assert_eq!(codec.char_len_at("😀".as_bytes(), 0), 4);
        assert_eq!(codec.char_len_at(b"a", 0), 1);
        // 不完整/非法序列必须严格报错,不能静默替换
        assert!(codec.decode(&[0xE3, 0x81], b"t").is_err());
        assert!(codec.decode(&[0x80], b"t").is_err());
        assert!(codec.next_char(&[0xE3, 0x81], 0).is_none());
        // 单码位接口对非 ASCII 返回 None(UTF-8 是变长编码)
        assert_eq!(codec.encode_char('A'), Some(0x41));
        assert_eq!(codec.encode_char('終'), None);
        assert!(codec.can_encode('終'));
    }

    /// 自动识别:整池严格合法 UTF-8 + 多字节 → 判 UTF-8;ASCII 仍 CP932。
    #[test]
    fn auto_detects_utf8() {
        let jp = "ゲームコンテンツを終了します。".as_bytes();
        assert_eq!(auto::detect(&[jp]).codec, Codec::Utf8);
        assert_eq!(auto::detect(&[b"SCORE=100"]).codec, Codec::Cp932);
        // 混入一池非法 UTF-8 就回落到 CP932/GBK 裁决
        let bad: &[u8] = &[0x83, 0x65, 0x83, 0x58, 0x83, 0x67];
        assert_ne!(auto::detect(&[bad]).codec, Codec::Utf8);
    }

    /// 单字节区遍历:合法字节 decode/encode 往返,非法字节必须报错。
    fn single_byte_roundtrip(codec: Codec) {
        let high: &[u16; 0x80] = match codec {
            Codec::Cp932 => CP932_TABLE.single_high,
            Codec::Gbk => GBK_TABLE.single_high,
            Codec::Big5 => BIG5_TABLE.single_high,
            Codec::Utf8 => panic!("UTF-8 没有单字节表;本用例只覆盖三张双字节码表"),
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

    #[test]
    fn single_byte_roundtrip_big5() {
        single_byte_roundtrip(Codec::Big5);
    }
}
