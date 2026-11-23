//! 脚本文本常量(`0x4D` 记录)的运行时重写。
//!
//! YSTB 的字符串常量窗口形如 `M + u16 len + 内容`;内容可以是相邻的多个
//! 引号段(`"a" "b"`)。翻译表以 `(script_id, pc, window, segment)` 定位
//! 其中一段,这里按字节重写并更新每条 `M` 记录的 u16 长度。
//!
//! 与剧本 DSL 一样,引号/反斜杠的识别必须按 codec 前导表步进,否则双字节
//! 字符的尾字节会被误当语法;译文中的单字节引号/反斜杠要转义。

use crate::Codec;

/// 一条要替换的段(段号 = 该窗口 payload 内所有引号段的出现顺序)。
#[derive(Clone, Copy, Debug)]
pub struct Replacement<'a> {
    pub segment: u32,
    pub bytes: &'a [u8],
}

/// 重写结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rewrite {
    /// `None` = 没有任何段命中,调用方保留原载荷。
    pub payload: Option<Vec<u8>>,
    pub applied: usize,
    /// 请求里段号超过该窗口实际段数(坐标过期)的数量。
    pub missing: usize,
}

/// 重写失败原因(调用方保留原载荷并报告)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RewriteError {
    /// `M`/表达式记录头越界,或引号段未闭合。
    Malformed,
    /// 替换后的 `M` 内容超过 u16 长度上限。
    TooLong,
}

impl std::fmt::Display for RewriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed => write!(f, "字符串常量记录头/引号段不完整"),
            Self::TooLong => write!(f, "替换后字符串常量超过 u16 长度上限"),
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

/// 引号段(不含引号)的内部 span,按出现顺序。
fn quoted_spans(codec: Codec, content: &[u8]) -> Result<Vec<(usize, usize)>, RewriteError> {
    let mut spans = Vec::new();
    let mut i = 0usize;
    while i < content.len() {
        let byte = content[i];
        if !matches!(byte, b'"' | b'`' | b'\'') {
            i += step(codec, content, i);
            continue;
        }
        let start = i + 1;
        let mut j = start;
        let mut closed = None;
        while j < content.len() {
            if content[j] == b'\\' && j + 1 < content.len() {
                j += 2;
                continue;
            }
            if content[j] == byte {
                closed = Some(j);
                break;
            }
            j += step(codec, content, j);
        }
        let Some(end) = closed else {
            return Err(RewriteError::Malformed);
        };
        spans.push((start, end));
        i = end + 1;
    }
    Ok(spans)
}

/// 转义译文中的**单字节**引号/反斜杠;双字节字符整体保留。
fn escape(codec: Codec, bytes: &[u8], quote: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + 4);
    let mut at = 0usize;
    while at < bytes.len() {
        let width = step(codec, bytes, at).min(bytes.len() - at);
        if width == 1 && (bytes[at] == quote || bytes[at] == b'\\') {
            out.push(b'\\');
        }
        out.extend_from_slice(&bytes[at..at + width]);
        at += width;
    }
    out
}

/// 枚举一个窗口 payload 里的引号段(段号跨所有 `0x4D` 记录连续)。
///
/// 导出与运行时共用这一份记录解析:不把 `M` 头长度字节/非字符串记录里的
/// 引号字节误当段;引号内按 codec 前导表步进。
#[must_use]
pub fn segments(payload: &[u8], codec: Codec) -> Vec<(u32, &[u8])> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    let mut segment = 0u32;
    while pos + 3 <= payload.len() {
        let op = payload[pos];
        let declared = usize::from(u16::from_le_bytes([payload[pos + 1], payload[pos + 2]]));
        let end = pos + 3 + declared;
        if end > payload.len() {
            break;
        }
        if op == 0x4D {
            if let Ok(spans) = quoted_spans(codec, &payload[pos + 3..end]) {
                for (start, quote_end) in spans {
                    out.push((segment, &payload[pos + 3 + start..pos + 3 + quote_end]));
                    segment += 1;
                }
            }
        }
        pos = end;
    }
    out
}

/// 按 `(segment, bytes)` 重写一个窗口 payload。
///
/// 非 `0x4D` 记录按声明长度原样复制;`0x4D` 记录重写命中的引号段并更新
/// u16 长度。段号跨该 payload 内所有 `M` 记录连续计数,与导出侧一致。
pub fn rewrite_payload(
    codec: Codec,
    payload: &[u8],
    replacements: &[Replacement<'_>],
) -> Result<Rewrite, RewriteError> {
    let mut out = Vec::with_capacity(payload.len() + 16);
    let mut pos = 0usize;
    let mut segment = 0u32;
    let mut applied = 0usize;
    let mut used = vec![false; replacements.len()];
    while pos < payload.len() {
        if pos + 3 > payload.len() {
            return Err(RewriteError::Malformed);
        }
        let op = payload[pos];
        let declared = usize::from(u16::from_le_bytes([payload[pos + 1], payload[pos + 2]]));
        let end = pos + 3 + declared;
        if end > payload.len() {
            return Err(RewriteError::Malformed);
        }
        if op != 0x4D {
            out.extend_from_slice(&payload[pos..end]);
            pos = end;
            continue;
        }
        let content = &payload[pos + 3..end];
        let spans = quoted_spans(codec, content)?;
        let mut rewritten = Vec::with_capacity(content.len() + 16);
        let mut cursor = 0usize;
        for (start, quote_end) in spans {
            rewritten.extend_from_slice(&content[cursor..start]);
            let (quote, inner) = (content[start - 1], &content[start..quote_end]);
            let replacement = replacements
                .iter()
                .enumerate()
                .find(|(index, replacement)| !used[*index] && replacement.segment == segment);
            if let Some((index, replacement)) = replacement {
                used[index] = true;
                applied += 1;
                rewritten.extend_from_slice(&escape(codec, replacement.bytes, quote));
            } else {
                rewritten.extend_from_slice(inner);
            }
            cursor = quote_end;
            segment += 1;
        }
        rewritten.extend_from_slice(&content[cursor..]);
        if rewritten.len() > usize::from(u16::MAX) {
            return Err(RewriteError::TooLong);
        }
        out.push(0x4D);
        out.extend_from_slice(&(rewritten.len() as u16).to_le_bytes());
        out.extend_from_slice(&rewritten);
        pos = end;
    }
    let missing = replacements
        .iter()
        .enumerate()
        .filter(|(index, _)| !used[*index])
        .count();
    Ok(Rewrite {
        payload: (applied > 0).then_some(out),
        applied,
        missing,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m_record(content: &[u8]) -> Vec<u8> {
        let mut out = vec![0x4D];
        out.extend_from_slice(&(content.len() as u16).to_le_bytes());
        out.extend_from_slice(content);
        out
    }

    #[test]
    fn rewrites_segment_and_updates_length() {
        let payload = m_record(b"\"AA\"");
        let out = rewrite_payload(
            Codec::Cp932,
            &payload,
            &[Replacement {
                segment: 0,
                bytes: b"BB",
            }],
        )
        .unwrap();
        assert_eq!(out.applied, 1);
        assert_eq!(out.payload.as_deref(), Some(m_record(b"\"BB\"").as_slice()));
    }

    #[test]
    fn segments_ignore_header_and_non_string_records() {
        // M 头长度低字节恰为 0x22 时不能被当成引号。
        let payload = m_record(b"\"A\"");
        let got = segments(&payload, Codec::Cp932);
        assert_eq!(got, vec![(0, &b"A"[..])]);
        let payload = vec![0x42, 0x22, 0x00, 1, b'"', b'B', b'"'];
        assert!(segments(&payload, Codec::Cp932).is_empty());
    }

    #[test]
    fn keeps_other_records_and_quotes() {
        let mut payload = vec![0x42, 1, 0, 7];
        payload.extend_from_slice(&m_record(b"\"A\"\"B\""));
        let out = rewrite_payload(
            Codec::Cp932,
            &payload,
            &[Replacement {
                segment: 1,
                bytes: b"C",
            }],
        )
        .unwrap();
        assert_eq!(out.applied, 1);
        let mut want = vec![0x42, 1, 0, 7];
        want.extend_from_slice(&m_record(b"\"A\"\"C\""));
        assert_eq!(out.payload.unwrap(), want);
    }

    #[test]
    fn escapes_ascii_quotes_but_not_tunnel_trails() {
        let payload = m_record(b"\"A\"");
        // 隧道码 81 22:尾字节 22 不是引号,不能拆开;ASCII 引号要转义。
        let out = rewrite_payload(
            Codec::Cp932,
            &payload,
            &[Replacement {
                segment: 0,
                bytes: b"\x81\x22\x22",
            }],
        )
        .unwrap();
        assert_eq!(out.payload.unwrap(), m_record(b"\"\x81\x22\\\"\""));
    }

    #[test]
    fn missing_segment_keeps_payload() {
        let payload = m_record(b"\"A\"");
        let out = rewrite_payload(
            Codec::Cp932,
            &payload,
            &[Replacement {
                segment: 3,
                bytes: b"B",
            }],
        )
        .unwrap();
        assert_eq!(out.applied, 0);
        assert_eq!(out.missing, 1);
        assert!(out.payload.is_none());
    }
}
