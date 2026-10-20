//! 严格逐行 diff:首个分歧定位到 JSON Pointer。
//!
//! 规则(任务书 §2.4):
//! - 按 `seq` / 物理行逐行比较,**不做容差、归一化或模糊匹配**;
//! - 行数不等、末尾截断、非法 JSON 分别给出独立错误码;
//! - 首个内容分歧报告 `{line, path, expected, actual}`。
//!
//! `line` 是 1 起算的物理行号(header 在第 1 行,`seq = n` 的记录在第 `n + 1` 行)。

use yu_log::{Boundary, Diag};

use crate::error;
use crate::json::{self, Json};
use crate::record::TraceLog;

/// 首个分歧。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffReport {
    pub line: u64,
    /// JSON Pointer(`""` = 整条记录;`~`/`/` 已按 RFC 6901 转义)。
    pub path: String,
    pub expected: String,
    pub actual: String,
}

impl DiffReport {
    pub fn to_diag(&self) -> Diag {
        Diag::fatal(error::code::DIFF, Boundary::Format).with_detail(format!(
            "first divergence at line {} ({}) expected {} actual {}",
            self.line,
            if self.path.is_empty() { "/" } else { &self.path },
            self.expected,
            self.actual
        ))
    }
}

/// diff 失败。`diag` 带稳定错误码;`report` 仅在内容分歧时存在。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffFailure {
    pub diag: Diag,
    pub report: Option<DiffReport>,
}

impl DiffFailure {
    pub fn code(&self) -> &str {
        self.diag.code.as_ref()
    }

    pub fn report(&self) -> Option<&DiffReport> {
        self.report.as_ref()
    }
}

/// 比较两份 JSONL 文本。
///
/// 先做协议校验(非法 JSON / 未知 schema / 缺字段 → 各自的错误码),
/// 然后**逐物理行比较原始字节**;某行字节不同时,再在该行内定位
/// 首个语义分歧的 JSON Pointer。这样空白与键序变化也算分歧,不会被
/// "先解析再重序列化"悄悄抹平。
pub fn diff_text(expected: &str, actual: &str) -> Result<(), DiffFailure> {
    let expected_log = TraceLog::parse(expected).map_err(|diag| DiffFailure {
        diag,
        report: None,
    })?;
    let actual_log = TraceLog::parse(actual).map_err(|diag| DiffFailure {
        diag,
        report: None,
    })?;
    diff_validated(expected, actual, &expected_log, &actual_log)
}

/// 比较两份已解析日志。
///
/// 已解析日志不带原始字节,因此这里只能按规范行比较;需要严格字节比较
/// 请用 [`diff_text`]。
pub fn diff_logs(expected: &TraceLog, actual: &TraceLog) -> Result<(), DiffFailure> {
    let expected_text = expected.to_jsonl();
    let actual_text = actual.to_jsonl();
    diff_validated(&expected_text, &actual_text, expected, actual)
}

fn diff_validated(
    expected_text: &str,
    actual_text: &str,
    expected: &TraceLog,
    actual: &TraceLog,
) -> Result<(), DiffFailure> {
    let expected_lines: Vec<&str> = expected_text.lines().collect();
    let actual_lines: Vec<&str> = actual_text.lines().collect();
    let common = expected_lines.len().min(actual_lines.len());

    for index in 0..common {
        let expected_line = expected_lines[index];
        let actual_line = actual_lines[index];
        if expected_line == actual_line {
            continue;
        }
        let site = first_difference(expected_line, actual_line);
        let report = DiffReport {
            line: index as u64 + 1,
            path: site.path,
            expected: site.expected,
            actual: site.actual,
        };
        return Err(DiffFailure {
            diag: report.to_diag(),
            report: Some(report),
        });
    }

    if expected_lines.len() == actual_lines.len() {
        return Ok(());
    }

    let detail = if actual_lines.len() < expected_lines.len() {
        format!(
            "actual ends after {} records, expected {} (first missing line {})",
            actual_lines.len(),
            expected_lines.len(),
            actual_lines.len() + 1
        )
    } else {
        format!(
            "actual has {} records, expected {} ({} extra lines starting at line {})",
            actual_lines.len(),
            expected_lines.len(),
            actual_lines.len() - expected_lines.len(),
            expected_lines.len() + 1
        )
    };
    // 已解析日志的行数必须与原始文本一致;不一致说明调用方传了错配的文本。
    debug_assert_eq!(expected_lines.len(), expected.len());
    debug_assert_eq!(actual_lines.len(), actual.len());
    Err(DiffFailure {
        diag: error::length(detail),
        report: None,
    })
}

struct Site {
    path: String,
    expected: String,
    actual: String,
}

fn first_difference(expected_line: &str, actual_line: &str) -> Site {
    let expected_value = json::parse(expected_line).expect("canonical line is valid JSON");
    let actual_value = json::parse(actual_line).expect("canonical line is valid JSON");
    if let Some(site) = compare(&expected_value, &actual_value, "") {
        return site;
    }
    // 语义相同但字节不同(空白、转义形式等):仍是分歧,按整条记录报告。
    Site {
        path: String::new(),
        expected: expected_line.to_string(),
        actual: actual_line.to_string(),
    }
}

fn compare(expected: &Json, actual: &Json, path: &str) -> Option<Site> {
    match (expected, actual) {
        (Json::Obj(expected_fields), Json::Obj(actual_fields)) => {
            // 先报缺失/多余字段(确定顺序:expected 先,actual 后)。
            for (key, value) in expected_fields {
                if !actual_fields.iter().any(|(other, _)| other == key) {
                    return Some(Site {
                        path: child_path(path, key),
                        expected: json::to_text(value),
                        actual: "null".to_string(),
                    });
                }
            }
            for (key, value) in actual_fields {
                if !expected_fields.iter().any(|(other, _)| other == key) {
                    return Some(Site {
                        path: child_path(path, key),
                        expected: "null".to_string(),
                        actual: json::to_text(value),
                    });
                }
            }
            // 字段集合相同但顺序不同:协议要求固定顺序,直接报整对象。
            let expected_order: Vec<&str> =
                expected_fields.iter().map(|(key, _)| key.as_str()).collect();
            let actual_order: Vec<&str> =
                actual_fields.iter().map(|(key, _)| key.as_str()).collect();
            if expected_order != actual_order {
                return Some(Site {
                    path: path.to_string(),
                    expected: json::to_text(expected),
                    actual: json::to_text(actual),
                });
            }
            for (key, value) in expected_fields {
                let (_, actual_value) = actual_fields
                    .iter()
                    .find(|(other, _)| other == key)
                    .expect("字段集合已确认相同");
                if let Some(site) = compare(value, actual_value, &child_path(path, key)) {
                    return Some(site);
                }
            }
            None
        }
        (Json::Arr(expected_items), Json::Arr(actual_items)) => {
            let common = expected_items.len().min(actual_items.len());
            for index in 0..common {
                let item_path = child_index(path, index);
                if let Some(site) = compare(&expected_items[index], &actual_items[index], &item_path)
                {
                    return Some(site);
                }
            }
            if expected_items.len() != actual_items.len() {
                let index = common;
                return Some(Site {
                    path: child_index(path, index),
                    expected: expected_items
                        .get(index)
                        .map(json::to_text)
                        .unwrap_or_else(|| "null".to_string()),
                    actual: actual_items
                        .get(index)
                        .map(json::to_text)
                        .unwrap_or_else(|| "null".to_string()),
                });
            }
            None
        }
        _ => {
            if expected == actual {
                None
            } else {
                Some(Site {
                    path: path.to_string(),
                    expected: json::to_text(expected),
                    actual: json::to_text(actual),
                })
            }
        }
    }
}

fn child_path(parent: &str, key: &str) -> String {
    let escaped = key.replace('~', "~0").replace('/', "~1");
    if parent.is_empty() {
        format!("/{escaped}")
    } else {
        format!("{parent}/{escaped}")
    }
}

fn child_index(parent: &str, index: usize) -> String {
    if parent.is_empty() {
        format!("/{index}")
    } else {
        format!("{parent}/{index}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::{BootSummary, HeaderPayload, InputSummary, Payload};

    fn header() -> HeaderPayload {
        HeaderPayload {
            codec: "cp932".to_string(),
            tables: vec![],
            boot: BootSummary::from_config(&yu_rt::BootConfig::default()),
            input: InputSummary {
                events: 0,
                sha256: "0".repeat(64),
            },
            kernel: "test".to_string(),
        }
    }

    fn log() -> TraceLog {
        let mut log = TraceLog::new();
        log.push_header(header());
        log.push(
            Some(1),
            Payload::VmWait {
                task: "SYSTEM".to_string(),
                frames: Some(1),
                time_ms: None,
            },
        );
        log.push(
            Some(2),
            Payload::VmFps {
                fps: 60,
                skip: false,
            },
        );
        log
    }

    #[test]
    fn identical_is_zero_diff() {
        let log = log();
        assert_eq!(diff_text(&log.to_jsonl(), &log.to_jsonl()), Ok(()));
    }

    #[test]
    fn one_byte_change_reports_that_line_and_path() {
        let text = log().to_jsonl();
        let mutated = text.replacen("\"fps\":60,\"skip\":false", "\"fps\":61,\"skip\":false", 1);
        let failure = diff_text(&text, &mutated).unwrap_err();
        assert_eq!(failure.code(), error::code::DIFF);
        let report = failure.report().expect("content divergence has a report");
        assert_eq!(report.line, 3);
        assert_eq!(report.path, "/payload/fps");
        assert_eq!(report.expected, "60");
        assert_eq!(report.actual, "61");
    }

        #[test]
    fn shortened_actual_has_length_code() {
        let text = log().to_jsonl();
        let mut lines: Vec<&str> = text.lines().collect();
        lines.pop();
        let shortened = format!("{}
", lines.join("
"));
        let failure = diff_text(&text, &shortened).unwrap_err();
        assert_eq!(failure.code(), error::code::LENGTH);
    }

    #[test]
    fn structurally_truncated_actual_has_truncated_code() {
        let text = log().to_jsonl();
        let cut = text[..text.len() - 5].to_string();
        let failure = diff_text(&text, &cut).unwrap_err();
        assert_eq!(failure.code(), error::code::TRUNCATED);
    }

    #[test]
    fn invalid_json_has_parse_code() {
        let text = log().to_jsonl();
        let mutated = text.replacen("\"fps\":60,\"skip\":false", "\"fps\":,\"skip\":false", 1);
        let failure = diff_text(&text, &mutated).unwrap_err();
        assert_eq!(failure.code(), error::code::PARSE);
    }

    #[test]
    fn key_order_change_is_a_divergence() {
        let text = log().to_jsonl();
        let mutated = text.replacen(
            "\"fps\":60,\"skip\":false",
            "\"skip\":false,\"fps\":60",
            1,
        );
        let failure = diff_text(&text, &mutated).unwrap_err();
        assert_eq!(failure.code(), error::code::DIFF);
        assert_eq!(failure.report().unwrap().line, 3);
    }

    #[test]
    fn whitespace_change_is_a_divergence() {
        // 不做任何归一化:纯空白差异也是分歧,报告整条记录。
        let text = log().to_jsonl();
        let mutated = text.replacen(
            "\"fps\":60,\"skip\":false",
            "\"fps\": 60,\"skip\":false",
            1,
        );
        let failure = diff_text(&text, &mutated).unwrap_err();
        assert_eq!(failure.code(), error::code::DIFF);
        let report = failure.report().unwrap();
        assert_eq!(report.line, 3);
        assert_eq!(report.path, "");
    }

    #[test]
    fn header_difference_is_reported_at_line_one() {
        let text = log().to_jsonl();
        let mutated = text.replacen("\"codec\":\"cp932\"", "\"codec\":\"gbk\"", 1);
        let failure = diff_text(&text, &mutated).unwrap_err();
        let report = failure.report().unwrap();
        assert_eq!(report.line, 1);
        assert_eq!(report.path, "/payload/codec");
    }
}
