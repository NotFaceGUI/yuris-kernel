//! 最小 JSON 读写(仅覆盖 `yu-log` 自有 schema)。
//!
//! 不引入第三方依赖是任务书 A 的硬约束;这里只实现确定性输出与严格解析,
//! 不支持浮点/指数/重复键。字节串(path)按单字节 `\u00XX` 转义,
//! 从而在 JSONL 内做到逐字节往返。

use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Json {
    Null,
    Bool(bool),
    Int(i64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
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
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// 把任意字节序列写成 JSON 字符串(每字节一个 `\u00XX` 或可打印 ASCII)。
pub(crate) fn write_bytes(out: &mut String, bytes: &[u8]) {
    out.push('"');
    for &b in bytes {
        match b {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            0x20..=0x7e => out.push(b as char),
            _ => {
                let _ = write!(out, "\\u{b:04x}");
            }
        }
    }
    out.push('"');
}

/// 把 [`write_bytes`] 产生的字符串还原为字节。
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

pub(crate) fn parse(input: &str) -> Result<Json, String> {
    let mut p = Parser {
        bytes: input.as_bytes(),
        src: input,
        pos: 0,
    };
    let v = p.value()?;
    p.ws();
    if p.pos != p.bytes.len() {
        return Err(format!("trailing data at byte {}", p.pos));
    }
    Ok(v)
}

pub(crate) fn get<'a>(obj: &'a [(String, Json)], key: &str) -> Option<&'a Json> {
    obj.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

pub(crate) fn as_obj(v: &Json) -> Result<&[(String, Json)], String> {
    match v {
        Json::Obj(o) => Ok(o),
        other => Err(format!("expected object, got {other:?}")),
    }
}

pub(crate) fn as_arr(v: &Json) -> Result<&[Json], String> {
    match v {
        Json::Arr(a) => Ok(a),
        other => Err(format!("expected array, got {other:?}")),
    }
}

pub(crate) fn as_str(v: &Json) -> Result<&str, String> {
    match v {
        Json::Str(s) => Ok(s),
        other => Err(format!("expected string, got {other:?}")),
    }
}

pub(crate) fn as_i64(v: &Json) -> Result<i64, String> {
    match v {
        Json::Int(n) => Ok(*n),
        other => Err(format!("expected integer, got {other:?}")),
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    src: &'a str,
    pos: usize,
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

    fn expect(&mut self, b: u8) -> Result<(), String> {
        if self.peek() == Some(b) {
            self.pos += 1;
            Ok(())
        } else {
            Err(format!(
                "expected '{}' at byte {}, found {:?}",
                b as char,
                self.pos,
                self.peek().map(|x| x as char)
            ))
        }
    }

    fn value(&mut self) -> Result<Json, String> {
        self.ws();
        match self.peek() {
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
            Some(b'-' | b'0'..=b'9') => self.number().map(Json::Int),
            other => Err(format!(
                "unexpected {:?} at byte {}",
                other.map(|x| x as char),
                self.pos
            )),
        }
    }

    fn keyword(&mut self, word: &str) -> Result<(), String> {
        if self.bytes[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(())
        } else {
            Err(format!("expected `{word}` at byte {}", self.pos))
        }
    }

    fn number(&mut self) -> Result<i64, String> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        let digits_start = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        if self.pos == digits_start {
            return Err(format!("expected digits at byte {start}"));
        }
        let text = &self.src[start..self.pos];
        if text.len() > 1 && text.starts_with('0') {
            return Err(format!("leading zero in number at byte {start}"));
        }
        text.parse::<i64>()
            .map_err(|_| format!("integer out of range at byte {start}: {text}"))
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            let Some(b) = self.peek() else {
                return Err("unterminated string".to_string());
            };
            match b {
                b'"' => {
                    self.pos += 1;
                    return Ok(out);
                }
                b'\\' => {
                    self.pos += 1;
                    self.escape(&mut out)?;
                }
                0x00..=0x1f => {
                    return Err(format!("raw control byte {b:#04x} in string"));
                }
                0x20..=0x7f => {
                    out.push(b as char);
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
        let Some(b) = self.peek() else {
            return Err("unterminated escape".to_string());
        };
        self.pos += 1;
        match b {
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

    fn hex4(&mut self) -> Result<u16, String> {
        if self.pos + 4 > self.bytes.len() {
            return Err("truncated \\u escape".to_string());
        }
        let text = &self.src[self.pos..self.pos + 4];
        let value = u16::from_str_radix(text, 16)
            .map_err(|_| format!("invalid hex in \\u escape: {text}"))?;
        self.pos += 4;
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
                        other.map(|x| x as char)
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
            if items.iter().any(|(k, _)| k == &key) {
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
                        other.map(|x| x as char)
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
        let mut s = String::new();
        write_bytes(&mut s, &bytes);
        let parsed = parse(&s).unwrap();
        let text = as_str(&parsed).unwrap();
        assert_eq!(bytes_from_string(text).unwrap(), bytes);
    }

    #[test]
    fn unicode_string_roundtrip() {
        let mut s = String::new();
        write_string(&mut s, "テスト \"quoted\"\n");
        let parsed = parse(&s).unwrap();
        assert_eq!(as_str(&parsed).unwrap(), "テスト \"quoted\"\n");
    }

    #[test]
    fn rejects_trailing_data() {
        assert!(parse("1 2").is_err());
    }

    #[test]
    fn rejects_duplicate_keys() {
        assert!(parse(r#"{"a":1,"a":2}"#).is_err());
    }
}
