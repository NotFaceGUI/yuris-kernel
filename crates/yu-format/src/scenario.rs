//! 剧本行扫描/重写(字节级,codec-aware)。
//!
//! 555/500 的外置剧本是「ASCII 语法 + 双字节正文」的 DSL:引号、反斜杠、
//! 括号是 ASCII,正文按构建 codec(CP932/GBK/Big5)的双字节步进。运行时
//! 替换必须在**字节层**定位正文 span,不能用 UTF-8 字符串下标;否则 GBK/
//! Big5 的尾字节落在 ASCII 语法区间时会把正文误当命令。
//!
//! 本模块供两处复用:
//! - 导出侧提取台词/裸文本/行内文本;
//! - 运行时 `TEXT[FILE]` 在写 LET 数组之前替换行内 span。

use crate::Codec;

/// 一条可替换单元的类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum UnitKind {
    /// `\LC("…")` / `\LE("…")` / `\LT("…")` 等台词宏。
    Macro,
    /// 裸文本行,或命令链之后的行内文本。
    Text,
}

/// 行内一个可替换单元。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitSpan {
    /// 同一条行内的出现序号(从 0 开始)。
    pub seq: u32,
    pub kind: UnitKind,
    /// 宏名(大写;`Text` 类为前导命令名,裸文本为空)。
    pub name: Vec<u8>,
    /// 正文起始字节(不含引号/命令)。
    pub text_start: usize,
    /// 正文结束字节(不含引号/命令)。
    pub text_end: usize,
    /// 宏的引号定界符(`"`/`` ` ``/`'`);`Text` 类为 `None`。
    pub quote: Option<u8>,
}

/// 跨行的块注释状态。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LineState {
    pub in_block_comment: bool,
}

/// 一条替换请求。
#[derive(Clone, Copy, Debug)]
pub struct Replacement<'a> {
    pub seq: u32,
    pub kind: UnitKind,
    pub name: &'a [u8],
    pub bytes: &'a [u8],
}

/// 行重写失败原因(调用方保留原行并计数)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RewriteError {
    /// 找不到对应 seq/类别/宏名的 span。
    MissingSpan { seq: u32 },
    /// 译文含换行(一行一条的 DSL 不允许)。
    LineBreak { seq: u32 },
    /// 行内文本译文会改变 DSL 结构(`//`/`/*` 注释、行首 `#`/`\`)。
    Syntax { seq: u32 },
}

impl std::fmt::Display for RewriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingSpan { seq } => write!(f, "第 {seq} 个替换单元在行内找不到对应 span"),
            Self::LineBreak { seq } => write!(f, "第 {seq} 个译文中含换行"),
            Self::Syntax { seq } => {
                write!(f, "第 {seq} 个行内文本译文会引入注释/场景标记/命令前缀")
            }
        }
    }
}

fn step(codec: Codec, bytes: &[u8], at: usize) -> usize {
    if at >= bytes.len() {
        return 1;
    }
    if bytes[at] < 0x80 {
        1
    } else {
        codec.char_len_at(bytes, at).max(1)
    }
}

fn skip_ascii_space(bytes: &[u8], mut at: usize) -> usize {
    while at < bytes.len() && bytes[at].is_ascii_whitespace() {
        at += 1;
    }
    at
}

/// 解析 `"…"` / `` `…` `` / `'…'`;返回 `(inner_start, inner_end, after_quote)`。
///
/// 按 codec 前导表步进:双字节字符的尾字节即使等于引号/反斜杠,也不是
/// 语法(隧道码尤其依赖这一点,其尾字节落在 `0x01..=0x3F`)。
fn quoted(codec: Codec, bytes: &[u8], at: usize) -> Option<(usize, usize, usize)> {
    let quote = *bytes.get(at)?;
    if !matches!(quote, b'"' | b'`' | b'\'') {
        return None;
    }
    let mut i = at + 1;
    while i < bytes.len() {
        // 单字节反斜杠才是转义;双字节字符由下面的 step 整体跳过。
        if bytes[i] == b'\\' && i + 1 < bytes.len() && bytes[i] < 0x80 {
            i += 2;
            continue;
        }
        if bytes[i] == quote {
            return Some((at + 1, i, i + 1));
        }
        i += step(codec, bytes, i);
    }
    None
}

struct MacroCall {
    name: Vec<u8>,
    end: usize,
    /// 第一个参数若是引号串,返回其 inner span 与引号。
    first_quote: Option<(usize, usize, u8)>,
}

fn macro_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'.'
}

/// 从 `\` 起解析一个宏调用(允许无括号)。
fn macro_call(codec: Codec, bytes: &[u8], at: usize) -> Option<MacroCall> {
    if bytes.get(at) != Some(&b'\\') {
        return None;
    }
    let mut i = at + 1;
    let name_start = i;
    while i < bytes.len() && macro_name_byte(bytes[i]) {
        i += 1;
    }
    if i == name_start {
        return None;
    }
    let name = bytes[name_start..i].to_vec();
    let after_name = i;
    i = skip_ascii_space(bytes, i);
    if bytes.get(i) != Some(&b'(') {
        return Some(MacroCall {
            name,
            end: after_name,
            first_quote: None,
        });
    }
    let open = i;
    let mut depth = 0i32;
    let mut first_quote = None;
    let arg_start = skip_ascii_space(bytes, open + 1);
    while i < bytes.len() {
        let byte = bytes[i];
        if byte == b'"' || byte == b'`' || byte == b'\'' {
            let Some((inner_start, inner_end, after)) = quoted(codec, bytes, i) else {
                return Some(MacroCall {
                    name,
                    end: bytes.len(),
                    first_quote,
                });
            };
            if first_quote.is_none() && i == arg_start {
                first_quote = Some((inner_start, inner_end, byte));
            }
            i = after;
            continue;
        }
        match byte {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(MacroCall {
                        name,
                        end: i + 1,
                        first_quote,
                    });
                }
            }
            _ => {}
        }
        i += step(codec, bytes, i);
    }
    Some(MacroCall {
        name,
        end: bytes.len(),
        first_quote,
    })
}

fn is_dialogue_macro(name: &[u8]) -> bool {
    (1..=3).contains(&name.len())
        && name.iter().all(u8::is_ascii_alphabetic)
        && name.first().is_some_and(|b| b.eq_ignore_ascii_case(&b'L'))
}

fn uppercase_name(name: &[u8]) -> Vec<u8> {
    name.iter().map(u8::to_ascii_uppercase).collect()
}

/// 扫描一行,返回可替换 span(按出现顺序,`seq` 0 起)。
///
/// `state` 跨行维护块注释;调用方每条文件从 [`LineState::default`] 开始。
pub fn scan_units(codec: Codec, line: &[u8], state: &mut LineState) -> Vec<UnitSpan> {
    let mut units = Vec::new();
    let mut i = 0usize;
    let mut command_end: Option<usize> = None;
    let mut saw_command = false;
    // 第一个注释之外的非空白字节;块注释内部不设置。
    let mut content_start: Option<usize> = None;
    // 正文之后遇到的第一个注释位置(行尾注释从这里截断)。
    let mut comment_at: Option<usize> = None;
    while i < line.len() {
        if state.in_block_comment {
            if line[i] == b'*' && line.get(i + 1) == Some(&b'/') {
                state.in_block_comment = false;
                i += 2;
            } else {
                i += step(codec, line, i);
            }
            continue;
        }
        if line[i] == b'/' && line.get(i + 1) == Some(&b'*') {
            if content_start.is_some() && comment_at.is_none() {
                comment_at = Some(i);
            }
            state.in_block_comment = true;
            i += 2;
            continue;
        }
        if line[i] == b'/' && line.get(i + 1) == Some(&b'/') {
            if content_start.is_some() && comment_at.is_none() {
                comment_at = Some(i);
            }
            break;
        }
        if content_start.is_none() && !line[i].is_ascii_whitespace() {
            content_start = Some(i);
        }
        if line[i] == b'\\' {
            if let Some(call) = macro_call(codec, line, i) {
                saw_command = true;
                command_end = Some(call.end);
                if is_dialogue_macro(&call.name) {
                    if let Some((start, end, quote)) = call.first_quote {
                        units.push(UnitSpan {
                            seq: units.len() as u32,
                            kind: UnitKind::Macro,
                            name: uppercase_name(&call.name),
                            text_start: start,
                            text_end: end,
                            quote: Some(quote),
                        });
                    }
                }
                i = call.end.max(i + 1);
                continue;
            }
        }
        i += step(codec, line, i);
    }

    if !units.is_empty() {
        return units;
    }
    let Some(first) = content_start else {
        return units;
    };
    if line[first] == b'#' {
        return units;
    }

    // 行内文本:有前导命令链时从链尾之后开始;否则从行首。
    let text_start = if saw_command {
        let mut at = command_end.unwrap_or(first);
        // 继续吃掉后续命令链(例如 `\T(...)\VO(...)`)。
        loop {
            let next = skip_ascii_space(line, at);
            if line.get(next) == Some(&b'\\') {
                if let Some(call) = macro_call(codec, line, next) {
                    at = call.end;
                    continue;
                }
            }
            at = next;
            break;
        }
        skip_ascii_space(line, at)
    } else {
        first
    };
    if text_start >= line.len() {
        return units;
    }

    // 行尾注释/空白/CR 都截掉;`state.in_block_comment` 已由主循环维护。
    let mut end = comment_at.unwrap_or(line.len());
    if end <= text_start {
        return units;
    }
    while end > text_start && line[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    if end > text_start {
        units.push(UnitSpan {
            seq: 0,
            kind: UnitKind::Text,
            name: Vec::new(),
            text_start,
            text_end: end,
            quote: None,
        });
    }
    units
}

/// 按 `replacements` 重写一行;任何一个替换找不到 span 就整行失败。
pub fn rewrite_line(
    codec: Codec,
    line: &[u8],
    state: &mut LineState,
    replacements: &[Replacement<'_>],
) -> Result<Vec<u8>, RewriteError> {
    let units = scan_units(codec, line, state);
    let mut edits: Vec<(usize, usize, Vec<u8>)> = Vec::new();
    for replacement in replacements {
        if replacement.bytes.is_empty() {
            continue;
        }
        if replacement.bytes.iter().any(|b| *b == b'\r' || *b == b'\n') {
            return Err(RewriteError::LineBreak {
                seq: replacement.seq,
            });
        }
        let Some(unit) = units.iter().find(|unit| {
            unit.seq == replacement.seq
                && unit.kind == replacement.kind
                && (unit.kind == UnitKind::Text || unit.name == uppercase_name(replacement.name))
        }) else {
            return Err(RewriteError::MissingSpan {
                seq: replacement.seq,
            });
        };
        if unit.kind == UnitKind::Text {
            // 行内/裸文本没有引号保护:注释、场景标记、命令前缀都会改变
            // DSL 结构。含这些序列的译文保留原文并报告,不静默截断。
            let trimmed = replacement
                .bytes
                .iter()
                .position(|b| !b.is_ascii_whitespace())
                .map_or(replacement.bytes, |at| &replacement.bytes[at..]);
            let comment = replacement
                .bytes
                .windows(2)
                .any(|pair| pair == b"//" || pair == b"/*");
            if comment || matches!(trimmed.first(), Some(b'#') | Some(b'\\')) {
                return Err(RewriteError::Syntax {
                    seq: replacement.seq,
                });
            }
        }
        let mut bytes = replacement.bytes.to_vec();
        if let Some(quote) = unit.quote {
            // 宏参数里的**单字节**引号/反斜杠要转义;双字节字符的尾字节
            // 即使等于引号/反斜杠也不能拆开(与上面 quoted 的步进一致)。
            let mut escaped = Vec::with_capacity(bytes.len() + 4);
            let mut at = 0usize;
            while at < bytes.len() {
                let width = step(codec, &bytes, at).min(bytes.len() - at);
                if width == 1 && (bytes[at] == quote || bytes[at] == b'\\') {
                    escaped.push(b'\\');
                }
                escaped.extend_from_slice(&bytes[at..at + width]);
                at += width;
            }
            bytes = escaped;
        }
        edits.push((unit.text_start, unit.text_end, bytes));
    }
    edits.sort_by_key(|(start, _, _)| *start);
    let mut out = Vec::with_capacity(line.len());
    let mut cursor = 0usize;
    for (start, end, bytes) in edits {
        if start < cursor || end > line.len() {
            continue;
        }
        out.extend_from_slice(&line[cursor..start]);
        out.extend_from_slice(&bytes);
        cursor = end;
    }
    out.extend_from_slice(&line[cursor..]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_dialogue_macro_and_comment() {
        let mut state = LineState::default();
        let line = b"\\LC(\"\x82\xa0\")//comment\r";
        let units = scan_units(Codec::Cp932, line, &mut state);
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].kind, UnitKind::Macro);
        assert_eq!(units[0].name, b"LC");
        assert_eq!(&line[units[0].text_start..units[0].text_end], b"\x82\xa0");
    }

    #[test]
    fn scans_bare_text_and_strips_comment() {
        let mut state = LineState::default();
        let line = b"  \x96\x7b\x95\xb6  // tail\r";
        let units = scan_units(Codec::Cp932, line, &mut state);
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].kind, UnitKind::Text);
        assert_eq!(
            &line[units[0].text_start..units[0].text_end],
            b"\x96\x7b\x95\xb6"
        );
    }

    #[test]
    fn scans_inline_text_after_command_chain() {
        let mut state = LineState::default();
        let line = b"\\VO(x)\\T(y)\x81\x79name\x81\x7a\r";
        let units = scan_units(Codec::Cp932, line, &mut state);
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].kind, UnitKind::Text);
        assert_eq!(units[0].name, b"");
        assert_eq!(
            &line[units[0].text_start..units[0].text_end],
            b"\x81\x79name\x81\x7a"
        );
    }

    #[test]
    fn scans_multiple_dialogue_macros_in_order() {
        let mut state = LineState::default();
        let line = b"\\LC(\"a\")\\LC(\"b\")";
        let units = scan_units(Codec::Cp932, line, &mut state);
        assert_eq!(units.len(), 2);
        assert_eq!(units[0].seq, 0);
        assert_eq!(units[1].seq, 1);
    }

    #[test]
    fn gbk_trail_backslash_is_not_syntax() {
        // GBK 双字节 81 5C:5C 是尾字节,不能被当成命令前缀。
        let mut state = LineState::default();
        let line = b"\x81\x5c\r";
        let units = scan_units(Codec::Gbk, line, &mut state);
        assert_eq!(units.len(), 1);
        assert_eq!(&line[units[0].text_start..units[0].text_end], b"\x81\x5c");
    }

    #[test]
    fn block_comment_spans_lines() {
        let mut state = LineState::default();
        assert!(scan_units(Codec::Cp932, b"/* open", &mut state).is_empty());
        assert!(state.in_block_comment);
        assert!(scan_units(Codec::Cp932, b"still", &mut state).is_empty());
        assert!(scan_units(Codec::Cp932, b"close */", &mut state).is_empty());
        assert!(!state.in_block_comment);
        assert_eq!(scan_units(Codec::Cp932, b"text", &mut state).len(), 1);
    }

    #[test]
    fn tunnel_quote_trail_does_not_close_string() {
        // 隧道码 81 22:0x22 是尾字节,不是字符串结束符。
        let mut state = LineState::default();
        let line = b"\\LC(\"\x81\x22x\")\r";
        let units = scan_units(Codec::Cp932, line, &mut state);
        assert_eq!(units.len(), 1);
        assert_eq!(&line[units[0].text_start..units[0].text_end], b"\x81\x22x");
        let out = rewrite_line(
            Codec::Cp932,
            line,
            &mut LineState::default(),
            &[Replacement {
                seq: 0,
                kind: UnitKind::Macro,
                name: b"LC",
                bytes: b"\x81\x5cy",
            }],
        )
        .unwrap();
        // 尾字节 5C 不能被当成反斜杠再转义一次。
        assert_eq!(out, b"\\LC(\"\x81\x5cy\")\r");
    }

    #[test]
    fn rewrite_macro_and_text() {
        let mut state = LineState::default();
        let line = b"\\LC(\"a\")\r";
        let out = rewrite_line(
            Codec::Cp932,
            line,
            &mut state,
            &[Replacement {
                seq: 0,
                kind: UnitKind::Macro,
                name: b"LC",
                bytes: b"b",
            }],
        )
        .unwrap();
        assert_eq!(out, b"\\LC(\"b\")\r");

        let mut state = LineState::default();
        let line = b"\\VO(x)hello\r";
        let out = rewrite_line(
            Codec::Cp932,
            line,
            &mut state,
            &[Replacement {
                seq: 0,
                kind: UnitKind::Text,
                name: b"",
                bytes: b"world",
            }],
        )
        .unwrap();
        assert_eq!(out, b"\\VO(x)world\r");
    }

    #[test]
    fn rewrite_rejects_bare_text_with_comment_syntax() {
        let mut state = LineState::default();
        let err = rewrite_line(
            Codec::Cp932,
            b"hello\r",
            &mut state,
            &[Replacement {
                seq: 0,
                kind: UnitKind::Text,
                name: b"",
                bytes: b"a//b",
            }],
        )
        .unwrap_err();
        assert_eq!(err, RewriteError::Syntax { seq: 0 });
    }

    #[test]
    fn rewrite_fails_when_span_missing() {
        let mut state = LineState::default();
        let err = rewrite_line(
            Codec::Cp932,
            b"\\LC(\"a\")",
            &mut state,
            &[Replacement {
                seq: 3,
                kind: UnitKind::Macro,
                name: b"LC",
                bytes: b"b",
            }],
        )
        .unwrap_err();
        assert_eq!(err, RewriteError::MissingSpan { seq: 3 });
    }
}
