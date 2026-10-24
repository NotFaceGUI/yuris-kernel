//! D4 验收:严格 diff 的全部失败模式。
//!
//! 每条用例都从同一份规范日志出发,只改一处,断言:
//! 错误码正确(内容分歧 / 行数不等 / 非法 JSON / schema)、
//! 首分歧行号与 JSON Pointer 正确。

mod support;

use support::header_for;
use yu_trace::record::{Payload, TraceArg, TraceLog, TraceValue};
use yu_trace::{diff_logs, diff_text, error, DiffFailure};

fn log() -> TraceLog {
    let mut log = TraceLog::new();
    log.push_header(header_for(vec!["ysc: v495 (105 命令)".to_string()]));
    log.push(
        None,
        Payload::VfsOpen {
            source: "ypf".to_string(),
            key: b"ysbin/ysc.ybn".to_vec(),
            bytes: Some(11156),
            sha256: Some("aa".repeat(32)),
            diag: None,
        },
    );
    log.push(
        Some(1),
        Payload::VmCommand {
            script: 29,
            pc: 107,
            name: "TEXT".to_string(),
            args: vec![TraceArg {
                slot: 0,
                value: TraceValue::Str(vec![b'a', b'b']),
                reference: None,
            }],
        },
    );
    log.push(
        Some(1),
        Payload::VmWait {
            task: "SYSTEM".to_string(),
            frames: Some(1),
            time_ms: None,
        },
    );
    log
}

fn replace_line(text: &str, line_index: usize, new_line: String) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    lines[line_index] = new_line;
    format!("{}\n", lines.join("\n"))
}

fn expect_failure(result: Result<(), DiffFailure>) -> DiffFailure {
    result.expect_err("diff must fail")
}

#[test]
fn same_text_is_zero_diff() {
    let text = log().to_jsonl();
    assert_eq!(diff_text(&text, &text), Ok(()));
    let other = log();
    assert_eq!(diff_logs(&log(), &other), Ok(()));
}

#[test]
fn first_divergence_is_the_smallest_line_number() {
    let text = log().to_jsonl();
    // 同时改第 2 行与第 4 行:必须报第 2 行。
    let mut mutated = replace_line(
        &text,
        1,
        text.lines()
            .nth(1)
            .unwrap()
            .replacen("\"bytes\":11156", "\"bytes\":11157", 1),
    );
    mutated = replace_line(
        &mutated,
        3,
        mutated
            .lines()
            .nth(3)
            .unwrap()
            .replacen("\"frames\":1", "\"frames\":2", 1),
    );
    let failure = expect_failure(diff_text(&text, &mutated));
    assert_eq!(failure.code(), error::code::DIFF);
    let report = failure.report().unwrap();
    assert_eq!(report.line, 2);
    assert_eq!(report.path, "/payload/bytes");
}

#[test]
fn divergence_in_an_array_element_points_into_the_array() {
    let text = log().to_jsonl();
    let mutated = replace_line(
        &text,
        2,
        text.lines()
            .nth(2)
            .unwrap()
            .replacen("\"v\":\"ab\"", "\"v\":\"ac\"", 1),
    );
    let failure = expect_failure(diff_text(&text, &mutated));
    let report = failure.report().unwrap();
    assert_eq!(report.line, 3);
    assert_eq!(report.path, "/payload/args/0/value/v");
    assert_eq!(report.expected, "\"ab\"");
    assert_eq!(report.actual, "\"ac\"");
}

#[test]
fn divergence_in_header_is_reported_at_line_one() {
    let text = log().to_jsonl();
    let mutated = text.replacen("\"fps\":60", "\"fps\":59", 1);
    let failure = expect_failure(diff_text(&text, &mutated));
    let report = failure.report().unwrap();
    assert_eq!(report.line, 1);
    assert_eq!(report.path, "/payload/boot/fps");
}

#[test]
fn extra_or_missing_fields_are_schema_errors() {
    let text = log().to_jsonl();
    let extra = text.replacen("\"bytes\":11156,", "\"bytes\":11156,\"extra\":1,", 1);
    let failure = expect_failure(diff_text(&text, &extra));
    assert_eq!(failure.code(), error::code::SCHEMA);

    let missing = text.replacen("\"bytes\":11156,", "", 1);
    let failure = expect_failure(diff_text(&text, &missing));
    assert_eq!(failure.code(), error::code::PARSE);
}

#[test]
fn line_count_mismatch_is_length_not_equal_prefix() {
    let text = log().to_jsonl();
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    lines.pop();
    let shortened = format!("{}\n", lines.join("\n"));
    let failure = expect_failure(diff_text(&text, &shortened));
    assert_eq!(failure.code(), error::code::LENGTH);
    assert!(failure.report().is_none());

    // 多一条合法记录 = 条数不符(不是截断,也不是 schema 错)。
    let last_seq = (log().len() - 1) as u64;
    let extra = format!(
        "{}{}\n",
        text,
        text.lines().last().unwrap().replacen(
            &format!("\"seq\":{last_seq}"),
            &format!("\"seq\":{}", last_seq + 1),
            1
        )
    );
    let failure = expect_failure(diff_text(&text, &extra));
    assert_eq!(failure.code(), error::code::LENGTH);
    assert!(failure.report().is_none());

    // 重复一条记录 ⇒ seq 不连续,先被 schema 拦下。
    let duplicate = format!("{text}{}\n", text.lines().last().unwrap());
    let failure = expect_failure(diff_text(&text, &duplicate));
    assert_eq!(failure.code(), error::code::SCHEMA);
}

#[test]
fn missing_final_newline_is_truncation() {
    let text = log().to_jsonl();
    let no_newline = text.trim_end_matches('\n').to_string();
    let failure = expect_failure(diff_text(&text, &no_newline));
    assert_eq!(failure.code(), error::code::TRUNCATED);
}

#[test]
fn invalid_json_is_parse_error_with_line() {
    let text = log().to_jsonl();
    let broken = replace_line(&text, 1, "{not json".to_string());
    let failure = expect_failure(diff_text(&text, &broken));
    assert_eq!(failure.code(), error::code::PARSE);
    assert!(failure.diag.detail.starts_with("line 2:"));
}

#[test]
fn unknown_kind_is_schema_error() {
    let text = log().to_jsonl();
    let broken = text.replacen("\"kind\":\"wait\"", "\"kind\":\"sleep\"", 1);
    let failure = expect_failure(diff_text(&text, &broken));
    assert_eq!(failure.code(), error::code::SCHEMA);
    assert!(failure.diag.detail.contains("unknown kind `sleep`"));
}

#[test]
fn unknown_schema_version_is_refused() {
    let text = log().to_jsonl();
    let broken = text.replacen(
        &format!("\"schema\":{}", yu_trace::SCHEMA_VERSION),
        "\"schema\":9",
        1,
    );
    let failure = expect_failure(diff_text(&text, &broken));
    assert_eq!(failure.code(), error::code::SCHEMA);
    assert!(failure.diag.detail.contains("unsupported schema 9"));
}

#[test]
fn whitespace_and_key_order_are_not_normalized() {
    let text = log().to_jsonl();
    let spaced = text.replacen("\"frames\":1", "\"frames\": 1", 1);
    let failure = expect_failure(diff_text(&text, &spaced));
    assert_eq!(failure.code(), error::code::DIFF);
    // 空白差异在语义上无法定位到某个键 ⇒ 报告整条记录。
    assert_eq!(failure.report().unwrap().path, "");
}

#[test]
fn empty_trace_is_truncated() {
    let text = log().to_jsonl();
    let failure = expect_failure(diff_text(&text, ""));
    assert_eq!(failure.code(), error::code::TRUNCATED);
}

#[test]
fn cross_platform_line_endings_are_rejected() {
    let text = log().to_jsonl();
    let crlf = text.replace('\n', "\r\n");
    let failure = expect_failure(diff_text(&text, &crlf));
    assert_eq!(failure.code(), error::code::PARSE);
}
