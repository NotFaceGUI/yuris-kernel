//! 确定性 JSON 读写(本 crate 自有协议,不复用其它 crate 的内部序列化)。
//!
//! - 输出顺序 = 构造顺序;不经过任何无序容器。
//! - 整数分 `u64`/`i64` 两种,浮点用 Rust 的最短往返表示;非有限浮点写成
//!   字符串 `"NaN"` / `"Infinity"` / `"-Infinity"`(JSON 数字文法不接受它们)。
//! - 字节串按单字节 `\u00XX` 转义,保证 CP932 等非 UTF-8 字节逐字节往返。

use std::fmt::Write as _;

/// 解析后的 JSON 值(键序保留)。
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Json {
    Null,
    Bool(bool),
    UInt(u64),
    Int(i64),
    Float(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

/// 单行文本(对象/数组内部的空白由写入者控制)。
pub(crate) fn to_text(value: &Json) -> String {
    let mut out = String::new();
    write_value(&mut out, value);
    out
}

fn write_value(out: &mut String, value: &Json) {
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(true) => out.push_str("true"),
        Json::Bool(false) => out.push_str("false"),
        Json::UInt(n) => {
            let _ = write!(out, "{n}");
        }
        Json::Int(n) => {
            let _ = write!(out, "{n}");
        }
        Json::Float(f) => write_float(out, *f),
        Json::Str(s) => write_string(out, s),
        Json::Arr(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(out, item);
            }
            out.push(']');
        }
        Json::Obj(fields) => {
            out.push('{');
            for (i, (key, item)) in fields.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_string(out, key);
                out.push(':');
                write_value(out, item);
            }
            out.push('}');
        }
    }
}

fn write_float(out: &mut String, value: f64) {
    if value.is_nan() {
        out.push_str("\"NaN\"");
    } else if value == f64::INFINITY {
        out.push_str("\"Infinity\"");
    } else if value == f64::NEG_INFINITY {
        out.push_str("\"-Infinity\"");
    } else {
        let _ = write!(out, "{value:?}");
    }
}

pub(crate) fn write_string(out: &mut String, s: &str) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // 控制字符与 0x7f..=0xff 一律 `\u00XX`:字节串(path/文本载荷)
            // 在文件里保持纯 ASCII,可与真实 UTF-8 文本在书写形式上区分。
            c if (c as u32) < 0x20 || matches!(c as u32, 0x7f..=0xff) => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// [`write_bytes`] 的逆变换。
pub(crate) fn bytes_from_string(s: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(s.len());
    for ch in s.chars() {
        let code = ch as u32;
        if code > 0xFF {
            return Err(format!("byte-string contains char U+{code:04X}"));
        }
        out.push(code as u8);
    }
    Ok(out)
}

/// 嵌套深度上限:超出 = 显式错误,而不是递归到栈溢出。
///
/// trace 记录的合法嵌套(header/record/payload/args)远小于此值;
/// 损坏或恶意输入必须有界失败。
const MAX_DEPTH: usize = 128;

pub(crate) fn parse(input: &str) -> Result<Json, String> {
    let mut p = Parser {
        bytes: input.as_bytes(),
        src: input,
        pos: 0,
        depth: 0,
    };
    let value = p.value()?;
    p.ws();
    if p.pos != p.bytes.len() {
        return Err(format!("trailing data at byte {}", p.pos));
    }
    Ok(value)
}

pub(crate) fn as_obj(value: &Json) -> Result<&[(String, Json)], String> {
    match value {
        Json::Obj(fields) => Ok(fields),
        other => Err(format!("expected object, got {}", type_name(other))),
    }
}

pub(crate) fn as_arr(value: &Json) -> Result<&[Json], String> {
    match value {
        Json::Arr(items) => Ok(items),
        other => Err(format!("expected array, got {}", type_name(other))),
    }
}

pub(crate) fn as_str(value: &Json) -> Result<&str, String> {
    match value {
        Json::Str(s) => Ok(s),
        other => Err(format!("expected string, got {}", type_name(other))),
    }
}

pub(crate) fn as_bool(value: &Json) -> Result<bool, String> {
    match value {
        Json::Bool(b) => Ok(*b),
        other => Err(format!("expected bool, got {}", type_name(other))),
    }
}

pub(crate) fn as_u64(value: &Json) -> Result<u64, String> {
    match value {
        Json::UInt(n) => Ok(*n),
        other => Err(format!(
            "expected unsigned integer, got {}",
            type_name(other)
        )),
    }
}

pub(crate) fn as_i64(value: &Json) -> Result<i64, String> {
    match value {
        Json::Int(n) => Ok(*n),
        Json::UInt(n) => i64::try_from(*n).map_err(|_| format!("integer out of range: {n}")),
        other => Err(format!("expected integer, got {}", type_name(other))),
    }
}

/// 浮点:数字,或非有限值的三个约定字符串。
pub(crate) fn as_f64(value: &Json) -> Result<f64, String> {
    match value {
        Json::Float(f) => Ok(*f),
        Json::Str(s) => match s.as_str() {
            "NaN" => Ok(f64::NAN),
            "Infinity" => Ok(f64::INFINITY),
            "-Infinity" => Ok(f64::NEG_INFINITY),
            other => Err(format!("expected float, got string {other:?}")),
        },
        other => Err(format!("expected float, got {}", type_name(other))),
    }
}

fn type_name(value: &Json) -> &'static str {
    match value {
        Json::Null => "null",
        Json::Bool(_) => "bool",
        Json::UInt(_) => "unsigned integer",
        Json::Int(_) => "integer",
        Json::Float(_) => "float",
        Json::Str(_) => "string",
        Json::Arr(_) => "array",
        Json::Obj(_) => "object",
    }
}

/// 字段访问错误:缺字段与多余字段在协议里属于不同错误类。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FieldError {
    Missing(String),
    Unknown(String),
}

/// 严格字段游标:只允许 schema 声明的键,且不允许重复消费。
pub(crate) struct Fields<'a> {
    items: &'a [(String, Json)],
    used: Vec<bool>,
}

impl<'a> Fields<'a> {
    pub(crate) fn new(items: &'a [(String, Json)]) -> Self {
        Self {
            items,
            used: vec![false; items.len()],
        }
    }

    pub(crate) fn take(&mut self, key: &str) -> Result<&'a Json, FieldError> {
        self.take_opt(key)
            .ok_or_else(|| FieldError::Missing(key.to_string()))
    }

    pub(crate) fn take_opt(&mut self, key: &str) -> Option<&'a Json> {
        for (i, (name, value)) in self.items.iter().enumerate() {
            if name == key {
                self.used[i] = true;
                return Some(value);
            }
        }
        None
    }

    pub(crate) fn finish(&self) -> Result<(), FieldError> {
        for (i, (name, _)) in self.items.iter().enumerate() {
            if !self.used[i] {
                return Err(FieldError::Unknown(name.clone()));
            }
        }
        Ok(())
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    src: &'a str,
    pos: usize,
    depth: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while matches!(self.bytes.get(self.pos), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn expect(&mut self, byte: u8) -> Result<(), String> {
        if self.peek() == Some(byte) {
            self.pos += 1;
            Ok(())
        } else {
            Err(format!(
                "expected '{}' at byte {}, found {:?}",
                byte as char,
                self.pos,
                self.peek().map(|b| b as char)
            ))
        }
    }

    fn value(&mut self) -> Result<Json, String> {
        self.ws();
        let nested = matches!(self.peek(), Some(b'{' | b'['));
        if nested {
            if self.depth >= MAX_DEPTH {
                return Err(format!(
                    "nesting deeper than {MAX_DEPTH} levels at byte {}",
                    self.pos
                ));
            }
            self.depth += 1;
        }
        let result = match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => self.string().map(Json::Str),
            Some(b't') => {
                self.keyword("true")?;
                Ok(Json::Bool(true))
            }
            Some(b'f') => {
                self.keyword("false")?;
                Ok(Json::Bool(false))
            }
            Some(b'n') => {
                self.keyword("null")?;
                Ok(Json::Null)
            }
            Some(b'-' | b'0'..=b'9') => self.number(),
            other => Err(format!(
                "unexpected {:?} at byte {}",
                other.map(|b| b as char),
                self.pos
            )),
        };
        if nested {
            self.depth -= 1;
        }
        result
    }

    fn keyword(&mut self, word: &str) -> Result<(), String> {
        if self.bytes[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(())
        } else {
            Err(format!("expected `{word}` at byte {}", self.pos))
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.pos;
        let negative = self.peek() == Some(b'-');
        if negative {
            self.pos += 1;
        }

        let int_start = self.pos;
        match self.peek() {
            Some(b'0') => {
                self.pos += 1;
                if matches!(self.peek(), Some(b'0'..=b'9')) {
                    return Err(format!("leading zero in number at byte {start}"));
                }
            }
            Some(b'1'..=b'9') => {
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.pos += 1;
                }
            }
            _ => return Err(format!("expected digits at byte {start}")),
        }

        let mut is_float = false;
        if self.peek() == Some(b'.') {
            is_float = true;
            self.pos += 1;
            let frac_start = self.pos;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
            if self.pos == frac_start {
                return Err(format!("expected fraction digits at byte {start}"));
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            is_float = true;
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            let exp_start = self.pos;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
            if self.pos == exp_start {
                return Err(format!("expected exponent digits at byte {start}"));
            }
        }

        let text = &self.src[int_start..self.pos];
        if is_float {
            let value = if negative {
                format!("-{text}")
            } else {
                text.to_string()
            };
            value
                .parse::<f64>()
                .map(Json::Float)
                .map_err(|_| format!("invalid float at byte {start}: {value}"))
        } else if negative {
            format!("-{text}")
                .parse::<i64>()
                .map(Json::Int)
                .map_err(|_| format!("integer out of range at byte {start}"))
        } else {
            text.parse::<u64>()
                .map(Json::UInt)
                .map_err(|_| format!("integer out of range at byte {start}: {text}"))
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            let Some(byte) = self.peek() else {
                return Err("unterminated string".to_string());
            };
            match byte {
                b'"' => {
                    self.pos += 1;
                    return Ok(out);
                }
                b'\\' => {
                    self.pos += 1;
                    self.escape(&mut out)?;
                }
                0x00..=0x1f => {
                    return Err(format!("raw control byte {byte:#04x} in string"));
                }
                0x20..=0x7f => {
                    out.push(byte as char);
                    self.pos += 1;
                }
                _ => {
                    let ch = self.src[self.pos..]
                        .chars()
                        .next()
                        .ok_or_else(|| "invalid UTF-8 at string boundary".to_string())?;
                    out.push(ch);
                    self.pos += ch.len_utf8();
                }
            }
        }
    }

    fn escape(&mut self, out: &mut String) -> Result<(), String> {
        let Some(byte) = self.peek() else {
            return Err("unterminated escape".to_string());
        };
        self.pos += 1;
        match byte {
            b'"' => out.push('"'),
            b'\\' => out.push('\\'),
            b'/' => out.push('/'),
            b'b' => out.push('\u{08}'),
            b'f' => out.push('\u{0c}'),
            b'n' => out.push('\n'),
            b'r' => out.push('\r'),
            b't' => out.push('\t'),
            b'u' => {
                let hi = self.hex4()?;
                let code = if (0xD800..=0xDBFF).contains(&hi) {
                    if self.peek() != Some(b'\\') {
                        return Err("high surrogate without low surrogate".to_string());
                    }
                    self.pos += 1;
                    self.expect(b'u')?;
                    let lo = self.hex4()?;
                    if !(0xDC00..=0xDFFF).contains(&lo) {
                        return Err(format!("invalid low surrogate {lo:#06x}"));
                    }
                    0x10000 + (((hi as u32 - 0xD800) << 10) | (lo as u32 - 0xDC00))
                } else if (0xDC00..=0xDFFF).contains(&hi) {
                    return Err(format!("lone low surrogate {hi:#06x}"));
                } else {
                    hi as u32
                };
                let ch = char::from_u32(code)
                    .ok_or_else(|| format!("invalid unicode scalar {code:#08x}"))?;
                out.push(ch);
            }
            other => return Err(format!("unknown escape \\{}", other as char)),
        }
        Ok(())
    }

    /// 读 `\u` 后的 4 个十六进制字符。
    ///
    /// 按**字节**读取并只接受 ASCII 十六进制:直接用 `&self.src[pos..pos+4]`
    /// 在跨字符边界时会 panic(`\uあ` 的窗口落在 3 字节 UTF-8 字符中间)。
    fn hex4(&mut self) -> Result<u16, String> {
        let end = self
            .pos
            .checked_add(4)
            .ok_or_else(|| "\\u escape position overflow".to_string())?;
        if end > self.bytes.len() {
            return Err("truncated \\u escape".to_string());
        }
        let mut value = 0u16;
        for &byte in &self.bytes[self.pos..end] {
            let digit = match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                b'A'..=b'F' => byte - b'A' + 10,
                _ => {
                    return Err(format!(
                        "invalid hex byte {byte:#04x} in \\u escape at byte {}",
                        self.pos
                    ))
                }
            };
            value = (value << 4) | u16::from(digit);
        }
        self.pos = end;
        Ok(value)
    }

    fn array(&mut self) -> Result<Json, String> {
        self.expect(b'[')?;
        let mut items = Vec::new();
        self.ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Json::Arr(items));
        }
        loop {
            items.push(self.value()?);
            self.ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Json::Arr(items));
                }
                other => {
                    return Err(format!(
                        "expected ',' or ']' at byte {}, found {:?}",
                        self.pos,
                        other.map(|b| b as char)
                    ))
                }
            }
        }
    }

    fn object(&mut self) -> Result<Json, String> {
        self.expect(b'{')?;
        let mut items: Vec<(String, Json)> = Vec::new();
        self.ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Json::Obj(items));
        }
        loop {
            self.ws();
            let key = self.string()?;
            if items.iter().any(|(existing, _)| existing == &key) {
                return Err(format!("duplicate key `{key}`"));
            }
            self.ws();
            self.expect(b':')?;
            let value = self.value()?;
            items.push((key, value));
            self.ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Json::Obj(items));
                }
                other => {
                    return Err(format!(
                        "expected ',' or '}}' at byte {}, found {:?}",
                        self.pos,
                        other.map(|b| b as char)
                    ))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_string_roundtrip() {
        let bytes = vec![0x00, 0x1f, b'"', b'\\', 0x7f, 0x80, 0xe9, 0xff];
        let text = to_text(&Json::Str(
            bytes.iter().map(|&b| b as char).collect::<String>(),
        ));
        let parsed = parse(&text).unwrap();
        assert_eq!(bytes_from_string(as_str(&parsed).unwrap()).unwrap(), bytes);
    }

    #[test]
    fn unicode_and_errors_roundtrip() {
        for text in ["テスト \"quoted\"\n", "\u{1F600}", "a/b~c"] {
            let mut encoded = String::new();
            write_string(&mut encoded, text);
            let parsed = parse(&encoded).unwrap();
            assert_eq!(as_str(&parsed).unwrap(), text);
        }
    }

    #[test]
    fn float_forms_roundtrip() {
        for value in [
            0.0,
            -0.0,
            1.5,
            0.1,
            f64::MIN_POSITIVE,
            f64::MAX,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            let mut text = String::new();
            write_float(&mut text, value);
            let parsed = parse(&text).unwrap();
            let back = as_f64(&parsed).unwrap();
            assert_eq!(back.to_bits(), value.to_bits(), "{text}");
        }
    }

    #[test]
    fn hex4_never_panics_on_split_utf8_or_bad_hex() {
        // `\u` 窗口落在 3 字节 UTF-8 字符中间:旧实现 `&src[pos..pos+4]` 会 panic。
        assert!(parse("{\"a\":\"\\uあ\"}").is_err());
        assert!(parse("{\"a\":\"\\u12あ\"}").is_err());
        // 非法十六进制(含 ASCII 非 hex)
        assert!(parse("{\"a\":\"\\u12g4\"}").is_err());
        // 截断
        assert!(parse("{\"a\":\"\\u12\"}").is_err());
        // 正常转义仍可用
        assert_eq!(as_str(&parse("\"\\u0041\"").unwrap()).unwrap(), "A");
    }

    #[test]
    fn deep_nesting_fails_without_stack_overflow() {
        let deep = format!("{}{}", "[".repeat(MAX_DEPTH + 1), "]".repeat(MAX_DEPTH + 1));
        let err = parse(&deep).unwrap_err();
        assert!(err.contains("nesting deeper"), "{err}");
        let ok = format!("{}{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH));
        assert!(parse(&ok).is_ok());
    }

    #[test]
    fn rejects_duplicate_and_trailing() {
        assert!(parse(r#"{"a":1,"a":2}"#).is_err());
        assert!(parse("1 2").is_err());
        assert!(parse("").is_err());
        assert!(parse("01").is_err());
    }

    #[test]
    fn strict_fields_report_kind() {
        let value = parse(r#"{"a":1,"b":2}"#).unwrap();
        let obj = as_obj(&value).unwrap();
        let mut fields = Fields::new(obj);
        assert_eq!(as_u64(fields.take("a").unwrap()).unwrap(), 1);
        assert_eq!(fields.finish(), Err(FieldError::Unknown("b".to_string())));
    }
}
