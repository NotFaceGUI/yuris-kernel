//! 最小 JSON(用例文件 + WebSocket 控制消息 + `--update` 输出)。
//!
//! 只支持标准 JSON;对象用 `BTreeMap`(键序稳定,`--update` 输出可复现)。
//! 数值统一 `f64`(用例里的整数都远小于 2^53)。

use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Value>),
    Object(BTreeMap<String, Value>),
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(map) => map.get(key),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        let value = self.as_f64()?;
        if value.is_finite() && value >= 0.0 {
            Some(value as u64)
        } else {
            None
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&BTreeMap<String, Value>> {
        match self {
            Value::Object(map) => Some(map),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }
}

pub fn parse(text: &str) -> Result<Value, String> {
    let mut parser = Parser {
        bytes: text.as_bytes(),
        at: 0,
    };
    parser.skip_ws();
    let value = parser.value()?;
    parser.skip_ws();
    if parser.at != parser.bytes.len() {
        return Err(format!("json: {} trailing bytes", parser.bytes.len() - parser.at));
    }
    Ok(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Parser<'a> {
    fn skip_ws(&mut self) {
        while let Some(byte) = self.bytes.get(self.at) {
            if matches!(byte, b' ' | b'\t' | b'\r' | b'\n') {
                self.at += 1;
            } else {
                break;
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn expect(&mut self, byte: u8) -> Result<(), String> {
        if self.peek() == Some(byte) {
            self.at += 1;
            Ok(())
        } else {
            Err(format!(
                "json: expected `{}` at offset {}",
                byte as char, self.at
            ))
        }
    }

    fn value(&mut self) -> Result<Value, String> {
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => Ok(Value::String(self.string()?)),
            Some(b't') => self.literal(b"true", Value::Bool(true)),
            Some(b'f') => self.literal(b"false", Value::Bool(false)),
            Some(b'n') => self.literal(b"null", Value::Null),
            Some(byte) if byte == b'-' || byte.is_ascii_digit() => self.number(),
            other => Err(format!("json: unexpected {:?} at offset {}", other, self.at)),
        }
    }

    fn literal(&mut self, text: &[u8], value: Value) -> Result<Value, String> {
        if self.bytes[self.at..].starts_with(text) {
            self.at += text.len();
            Ok(value)
        } else {
            Err(format!("json: bad literal at offset {}", self.at))
        }
    }

    fn object(&mut self) -> Result<Value, String> {
        self.expect(b'{')?;
        let mut map = BTreeMap::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.at += 1;
            return Ok(Value::Object(map));
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.skip_ws();
            self.expect(b':')?;
            self.skip_ws();
            let value = self.value()?;
            map.insert(key, value);
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.at += 1;
                }
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Value::Object(map));
                }
                _ => return Err(format!("json: expected , or }} at offset {}", self.at)),
            }
        }
    }

    fn array(&mut self) -> Result<Value, String> {
        self.expect(b'[')?;
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.at += 1;
            return Ok(Value::Array(items));
        }
        loop {
            self.skip_ws();
            items.push(self.value()?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.at += 1;
                }
                Some(b']') => {
                    self.at += 1;
                    return Ok(Value::Array(items));
                }
                _ => return Err(format!("json: expected , or ] at offset {}", self.at)),
            }
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            let byte = self
                .peek()
                .ok_or_else(|| "json: unterminated string".to_string())?;
            self.at += 1;
            match byte {
                b'"' => return Ok(out),
                b'\\' => {
                    let escape = self
                        .peek()
                        .ok_or_else(|| "json: unterminated escape".to_string())?;
                    self.at += 1;
                    match escape {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let code = self.hex4()?;
                            if (0xD800..=0xDBFF).contains(&code) {
                                // 代理对。
                                if self.peek() == Some(b'\\') {
                                    self.at += 1;
                                    self.expect(b'u')?;
                                    let low = self.hex4()?;
                                    if (0xDC00..=0xDFFF).contains(&low) {
                                        let combined = 0x10000
                                            + (((code as u32 - 0xD800) << 10)
                                                | (low as u32 - 0xDC00));
                                        out.push(
                                            char::from_u32(combined).unwrap_or('\u{FFFD}'),
                                        );
                                    } else {
                                        out.push('\u{FFFD}');
                                    }
                                } else {
                                    out.push('\u{FFFD}');
                                }
                            } else {
                                out.push(char::from_u32(code as u32).unwrap_or('\u{FFFD}'));
                            }
                        }
                        other => {
                            return Err(format!("json: bad escape `\\{}`", other as char))
                        }
                    }
                }
                byte if byte < 0x20 => {
                    return Err(format!("json: control byte {byte:#x} in string"))
                }
                _ => {
                    // UTF-8 按原字节推进:从当前字节重建字符。
                    let start = self.at - 1;
                    let width = utf8_width(byte);
                    let end = (start + width).min(self.bytes.len());
                    let text = std::str::from_utf8(&self.bytes[start..end])
                        .map_err(|_| "json: invalid UTF-8".to_string())?;
                    out.push_str(text);
                    self.at = end;
                }
            }
        }
    }

    fn hex4(&mut self) -> Result<u16, String> {
        if self.at + 4 > self.bytes.len() {
            return Err("json: truncated \\u escape".to_string());
        }
        let text = std::str::from_utf8(&self.bytes[self.at..self.at + 4])
            .map_err(|_| "json: bad \\u escape".to_string())?;
        let value = u16::from_str_radix(text, 16)
            .map_err(|_| "json: bad \\u escape".to_string())?;
        self.at += 4;
        Ok(value)
    }

    fn number(&mut self) -> Result<Value, String> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        while self.peek().map(|b| b.is_ascii_digit()).unwrap_or(false) {
            self.at += 1;
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            while self.peek().map(|b| b.is_ascii_digit()).unwrap_or(false) {
                self.at += 1;
            }
        }
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                self.at += 1;
            }
            while self.peek().map(|b| b.is_ascii_digit()).unwrap_or(false) {
                self.at += 1;
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.at])
            .map_err(|_| "json: bad number".to_string())?;
        text.parse::<f64>()
            .map(Value::Number)
            .map_err(|_| format!("json: bad number `{text}`"))
    }
}

fn utf8_width(byte: u8) -> usize {
    if byte < 0x80 {
        1
    } else if byte >> 5 == 0b110 {
        2
    } else if byte >> 4 == 0b1110 {
        3
    } else {
        4
    }
}

/// 稳定缩进输出(BTreeMap 键序;用于 `--update` 写入用例)。
pub fn to_pretty(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value, 0);
    out.push('\n');
    out
}

fn write_value(out: &mut String, value: &Value, indent: usize) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        Value::Number(value) => {
            if value.fract() == 0.0 && value.abs() < 9e15 {
                out.push_str(&format!("{}", *value as i64));
            } else {
                out.push_str(&format!("{value}"));
            }
        }
        Value::String(text) => write_string(out, text),
        Value::Array(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push_str("[\n");
            for (index, item) in items.iter().enumerate() {
                push_indent(out, indent + 1);
                write_value(out, item, indent + 1);
                if index + 1 < items.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            push_indent(out, indent);
            out.push(']');
        }
        Value::Object(map) => {
            if map.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push_str("{\n");
            let count = map.len();
            for (index, (key, value)) in map.iter().enumerate() {
                push_indent(out, indent + 1);
                write_string(out, key);
                out.push_str(": ");
                write_value(out, value, indent + 1);
                if index + 1 < count {
                    out.push(',');
                }
                out.push('\n');
            }
            push_indent(out, indent);
            out.push('}');
        }
    }
}

fn push_indent(out: &mut String, indent: usize) {
    for _ in 0..indent {
        out.push_str("  ");
    }
}

fn write_string(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_objects_arrays_and_escapes() {
        let value = parse(
            r#"{"name":"a\u3042","n":3,"flag":true,"list":[1,2.5,null,"x\"y"],"nested":{"k":-1e3}}"#,
        )
        .unwrap();
        assert_eq!(value.get("name").unwrap().as_str().unwrap(), "aあ");
        assert_eq!(value.get("n").unwrap().as_u64(), Some(3));
        assert_eq!(value.get("flag").unwrap().as_bool(), Some(true));
        assert_eq!(value.get("list").unwrap().as_array().unwrap().len(), 4);
        assert_eq!(
            value.get("nested").unwrap().get("k").unwrap().as_f64(),
            Some(-1000.0)
        );
    }

    #[test]
    fn rejects_trailing_and_unterminated() {
        assert!(parse("{\"a\":1} junk").is_err());
        assert!(parse("{\"a\":1").is_err());
        assert!(parse("\"unterminated").is_err());
    }

    #[test]
    fn pretty_roundtrip_is_stable() {
        let text = "{\"b\":[1,2],\"a\":{\"x\":true}}";
        let value = parse(text).unwrap();
        let pretty = to_pretty(&value);
        let again = parse(&pretty).unwrap();
        assert_eq!(value, again);
        assert!(pretty.contains("\"a\": {"));
    }
}
