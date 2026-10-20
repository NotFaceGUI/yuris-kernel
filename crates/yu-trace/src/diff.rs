//! 严格逐行 diff:首个分歧定位到 JSON Pointer。
//!
//! 规则(任务书 §2.4):
//! - 按 `seq` / 物理行逐行比较,**不做容差、归一化或模糊匹配**;
//! - 行数不等、末尾截断、非法 JSON 分别给出独立错误码;
//! - 首个内容分歧报告 `{line, path, expected, actual}`。
//!
//! `line` 是 1 起算的物理行号(header 在第 1 行,`seq = n` 的记录在第 `n + 1` 行)。

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use yu_log::{Boundary, Diag};

use crate::error;
use crate::json::{self, Json};
use crate::record::{self, HeaderPayload, Kind, Payload, Record, Sub, TraceLog};

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
            if self.path.is_empty() {
                "/"
            } else {
                &self.path
            },
            self.expected,
            self.actual
        ))
    }
}

/// diff 失败。`diag` 带稳定错误码;`report` 仅在原始逐行分歧时存在,
/// `aligned` 仅在 [`diff_aligned`] 的逻辑对齐分歧时存在。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffFailure {
    pub diag: Diag,
    pub report: Option<DiffReport>,
    pub aligned: Option<AlignedDiffReport>,
}

impl DiffFailure {
    pub fn code(&self) -> &str {
        self.diag.code.as_ref()
    }

    pub fn report(&self) -> Option<&DiffReport> {
        self.report.as_ref()
    }

    pub fn aligned_report(&self) -> Option<&AlignedDiffReport> {
        self.aligned.as_ref()
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
        aligned: None,
    })?;
    let actual_log = TraceLog::parse(actual).map_err(|diag| DiffFailure {
        diag,
        report: None,
        aligned: None,
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

// ---------------------------------------------------------------- 投影/对齐
//
// 跨 producer 的 trace 不能按物理行号 `seq` 对齐:两边的 header 元数据、
// 事件密度都不同。这里用 `(sub, kind, frame, ordinal)` 作为逻辑键:
// `ordinal` 是同一 `(sub, kind, frame)` 在该文件(应用投影后)中的第几次出现。
// 比较语义仍是**精确匹配**:不排序、不去重、不做容差。

/// 记录投影(空列表 = 不限制)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Projection {
    pub subs: Vec<Sub>,
    pub kinds: Vec<Kind>,
    /// 帧号下限(含)。
    pub frame_first: Option<u64>,
    /// 帧号上限(含)。
    pub frame_last: Option<u64>,
    /// 无帧号记录(vfs 启动链)是否参与;设置帧范围时恒为 false。
    pub include_null_frames: bool,
}

impl Default for Projection {
    fn default() -> Self {
        Self::all()
    }
}

impl Projection {
    pub fn all() -> Self {
        Self {
            subs: Vec::new(),
            kinds: Vec::new(),
            frame_first: None,
            frame_last: None,
            include_null_frames: true,
        }
    }

    pub fn with_subs(subs: &[Sub]) -> Self {
        Self {
            subs: subs.to_vec(),
            ..Self::all()
        }
    }

    pub fn with_kinds(kinds: &[Kind]) -> Self {
        Self {
            kinds: kinds.to_vec(),
            ..Self::all()
        }
    }

    /// 帧范围(含两端);无帧号记录不再参与。
    pub fn frames(mut self, first: u64, last: u64) -> Self {
        self.frame_first = Some(first);
        self.frame_last = Some(last);
        self
    }

    pub fn matches(&self, record: &Record) -> bool {
        if !self.subs.is_empty() && !self.subs.contains(&record.sub()) {
            return false;
        }
        if !self.kinds.is_empty() && !self.kinds.contains(&record.kind()) {
            return false;
        }
        match record.frame() {
            Some(frame) => {
                if let Some(first) = self.frame_first {
                    if frame < first {
                        return false;
                    }
                }
                if let Some(last) = self.frame_last {
                    if frame > last {
                        return false;
                    }
                }
                true
            }
            None => {
                self.include_null_frames && self.frame_first.is_none() && self.frame_last.is_none()
            }
        }
    }
}

/// header 比较策略。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderCompare {
    /// 不比较 header。
    Ignore,
    /// 只比较 `codec` 与双方都存在的 `producer.sample_sha256`。
    CodecSample,
    /// 跨 producer 语义投影:`codec` + `boot` + `input` + 双方都存在的样本摘要。
    /// 不比较 `kernel` / `tables` / producer 名称版本,也不比较 `clock`。
    Semantic,
    /// 完整 header(v3 的全部字段)。
    Strict,
}

/// [`diff_aligned`] 的选项。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlignedDiffOptions {
    pub header: HeaderCompare,
    pub projection: Projection,
    /// 只比较 payload 中这些顶层键(空 = 全部)。用于两边可观测字段不同的
    /// producer(例如 official 事件采集没有命令参数):这是**显式投影**,
    /// 未列出的字段明确不参与比较,但已列出的字段仍精确比较。
    pub payload_fields: Vec<String>,
    /// 首分歧报告附带的上下文条数(前后各 N 条逻辑记录)。
    pub context: usize,
}

impl Default for AlignedDiffOptions {
    fn default() -> Self {
        Self {
            header: HeaderCompare::Semantic,
            projection: Projection::all(),
            payload_fields: Vec::new(),
            context: 2,
        }
    }
}

impl AlignedDiffOptions {
    /// 跨 producer 对齐:语义 header + 全记录 + 3 条上下文。
    pub fn cross_producer() -> Self {
        Self {
            header: HeaderCompare::Semantic,
            projection: Projection::all(),
            payload_fields: Vec::new(),
            context: 3,
        }
    }

    /// 同 producer 对齐:完整 header(用于回放闭合时检查 clock 等)。
    pub fn same_producer() -> Self {
        Self {
            header: HeaderCompare::Strict,
            projection: Projection::all(),
            payload_fields: Vec::new(),
            context: 2,
        }
    }

    /// 只比较 payload 的指定顶层键(两边可观测字段不同时使用)。
    pub fn with_payload_fields(mut self, fields: &[&str]) -> Self {
        self.payload_fields = fields.iter().map(|field| field.to_string()).collect();
        self
    }
}

/// 逻辑记录键。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecordKey {
    pub sub: Sub,
    pub kind: Kind,
    pub frame: Option<u64>,
    /// 同一 `(sub, kind, frame)` 的第几次出现(0 起)。
    pub ordinal: u64,
}

impl fmt::Display for RecordKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.frame {
            Some(frame) => write!(
                f,
                "{}/{} frame={frame} ordinal={}",
                self.sub.as_str(),
                self.kind.as_str(),
                self.ordinal
            ),
            None => write!(
                f,
                "{}/{} frame=null ordinal={}",
                self.sub.as_str(),
                self.kind.as_str(),
                self.ordinal
            ),
        }
    }
}

/// 逻辑对齐后的首分歧报告。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlignedDiffReport {
    pub key: RecordKey,
    pub expected_line: Option<u64>,
    pub actual_line: Option<u64>,
    /// JSON Pointer;header 路径以 `/header` 开头,payload 以 `/payload` 开头。
    pub path: String,
    pub expected: String,
    pub actual: String,
    /// 期望侧的相邻逻辑记录及两侧行号(首分歧前后各 N 条)。
    pub context: Vec<String>,
}

impl AlignedDiffReport {
    pub fn to_diag(&self) -> Diag {
        let expected_line = self
            .expected_line
            .map(|line| line.to_string())
            .unwrap_or_else(|| "missing".to_string());
        let actual_line = self
            .actual_line
            .map(|line| line.to_string())
            .unwrap_or_else(|| "missing".to_string());
        let mut detail = format!(
            "first aligned divergence at {} (expected line {}, actual line {}) path {} expected {} actual {}",
            self.key,
            expected_line,
            actual_line,
            if self.path.is_empty() { "/" } else { &self.path },
            self.expected,
            self.actual
        );
        if !self.context.is_empty() {
            detail.push_str(" | context: ");
            detail.push_str(&self.context.join(" || "));
        }
        Diag::fatal(error::code::DIFF, Boundary::Format).with_detail(detail)
    }
}

fn aligned_failure(report: AlignedDiffReport) -> DiffFailure {
    DiffFailure {
        diag: report.to_diag(),
        report: None,
        aligned: Some(report),
    }
}

fn projection_failure(detail: impl Into<String>) -> DiffFailure {
    DiffFailure {
        diag: error::schema(0, detail.into()),
        report: None,
        aligned: None,
    }
}

/// 比较两份 JSONL 文本,按 `(sub, kind, frame, ordinal)` 对齐。
pub fn diff_aligned(
    expected: &str,
    actual: &str,
    options: &AlignedDiffOptions,
) -> Result<(), DiffFailure> {
    let expected_log = TraceLog::parse(expected).map_err(|diag| DiffFailure {
        diag,
        report: None,
        aligned: None,
    })?;
    let actual_log = TraceLog::parse(actual).map_err(|diag| DiffFailure {
        diag,
        report: None,
        aligned: None,
    })?;
    diff_aligned_logs(&expected_log, &actual_log, options)
}

/// 比较两份已解析日志,按逻辑键对齐。
pub fn diff_aligned_logs(
    expected: &TraceLog,
    actual: &TraceLog,
    options: &AlignedDiffOptions,
) -> Result<(), DiffFailure> {
    if let (Some(first), Some(last)) = (
        options.projection.frame_first,
        options.projection.frame_last,
    ) {
        if first > last {
            return Err(projection_failure(format!(
                "projection frame_first {first} is greater than frame_last {last}"
            )));
        }
    }

    if let Some(report) = compare_headers(expected, actual, options.header) {
        return Err(aligned_failure(report));
    }

    let expected_entries = aligned_entries(expected, &options.projection);
    let actual_entries = aligned_entries(actual, &options.projection);
    let actual_index: BTreeMap<RecordKey, usize> = actual_entries
        .iter()
        .enumerate()
        .map(|(index, entry)| (entry.key.clone(), index))
        .collect();
    let expected_keys: BTreeSet<RecordKey> = expected_entries
        .iter()
        .map(|entry| entry.key.clone())
        .collect();

    for (position, entry) in expected_entries.iter().enumerate() {
        match actual_index.get(&entry.key) {
            Some(&actual_position) => {
                let actual_entry = &actual_entries[actual_position];
                let expected_payload = project_payload(&entry.payload, &options.payload_fields);
                let actual_payload =
                    project_payload(&actual_entry.payload, &options.payload_fields);
                if let Some(site) = compare(&expected_payload, &actual_payload, "/payload") {
                    let report = AlignedDiffReport {
                        key: entry.key.clone(),
                        expected_line: Some(entry.line),
                        actual_line: Some(actual_entry.line),
                        path: site.path,
                        expected: site.expected,
                        actual: site.actual,
                        context: context_for(
                            &expected_entries,
                            &actual_index,
                            &actual_entries,
                            position,
                            options.context,
                        ),
                    };
                    return Err(aligned_failure(report));
                }
            }
            None => {
                let report = AlignedDiffReport {
                    key: entry.key.clone(),
                    expected_line: Some(entry.line),
                    actual_line: None,
                    path: String::new(),
                    expected: json::to_text(&project_payload(
                        &entry.payload,
                        &options.payload_fields,
                    )),
                    actual: "null".to_string(),
                    context: context_for(
                        &expected_entries,
                        &actual_index,
                        &actual_entries,
                        position,
                        options.context,
                    ),
                };
                return Err(aligned_failure(report));
            }
        }
    }

    if let Some(entry) = actual_entries
        .iter()
        .find(|entry| !expected_keys.contains(&entry.key))
    {
        let start = expected_entries.len().saturating_sub(options.context);
        let context: Vec<String> = expected_entries[start..]
            .iter()
            .map(|entry| format!("  {} (expected line {})", entry.key, entry.line))
            .collect();
        let report = AlignedDiffReport {
            key: entry.key.clone(),
            expected_line: None,
            actual_line: Some(entry.line),
            path: String::new(),
            expected: "null".to_string(),
            actual: json::to_text(&project_payload(&entry.payload, &options.payload_fields)),
            context,
        };
        return Err(aligned_failure(report));
    }

    Ok(())
}

struct AlignedEntry {
    key: RecordKey,
    payload: Json,
    line: u64,
}

fn aligned_entries(log: &TraceLog, projection: &Projection) -> Vec<AlignedEntry> {
    let mut ordinals: BTreeMap<(Sub, Kind, Option<u64>), u64> = BTreeMap::new();
    let mut entries = Vec::new();
    for record in log.records().iter().skip(1) {
        if !projection.matches(record) {
            continue;
        }
        let key_part = (record.sub(), record.kind(), record.frame());
        let ordinal = ordinals.entry(key_part).or_insert(0);
        entries.push(AlignedEntry {
            key: RecordKey {
                sub: record.sub(),
                kind: record.kind(),
                frame: record.frame(),
                ordinal: *ordinal,
            },
            payload: record::payload_to_json(record.payload()),
            line: record.seq() + 1,
        });
        *ordinal += 1;
    }
    entries
}

fn context_for(
    expected: &[AlignedEntry],
    actual_index: &BTreeMap<RecordKey, usize>,
    actual_entries: &[AlignedEntry],
    position: usize,
    context: usize,
) -> Vec<String> {
    if context == 0 {
        return Vec::new();
    }
    let start = position.saturating_sub(context);
    let end = (position + context + 1).min(expected.len());
    let mut out = Vec::with_capacity(end - start);
    for (index, entry) in expected.iter().enumerate().take(end).skip(start) {
        let marker = if index == position { ">>" } else { "  " };
        let actual = actual_index
            .get(&entry.key)
            .map(|&actual_position| format!("actual line {}", actual_entries[actual_position].line))
            .unwrap_or_else(|| "actual missing".to_string());
        out.push(format!(
            "{marker} {} (expected line {}, {})",
            entry.key, entry.line, actual
        ));
    }
    out
}

fn project_payload(payload: &Json, fields: &[String]) -> Json {
    if fields.is_empty() {
        return payload.clone();
    }
    match payload {
        Json::Obj(items) => Json::Obj(
            items
                .iter()
                .filter(|(key, _)| fields.iter().any(|field| field == key))
                .cloned()
                .collect(),
        ),
        other => other.clone(),
    }
}

fn compare_headers(
    expected: &TraceLog,
    actual: &TraceLog,
    mode: HeaderCompare,
) -> Option<AlignedDiffReport> {
    if mode == HeaderCompare::Ignore {
        return None;
    }
    let key = RecordKey {
        sub: Sub::Meta,
        kind: Kind::Header,
        frame: None,
        ordinal: 0,
    };
    match (expected.header(), actual.header()) {
        (Some(expected_header), Some(actual_header)) => {
            let include_sample = expected_header.producer.sample_sha256.is_some()
                && actual_header.producer.sample_sha256.is_some();
            let (expected_json, actual_json) = match mode {
                HeaderCompare::Strict => (
                    record::payload_to_json(&Payload::Header(expected_header.clone())),
                    record::payload_to_json(&Payload::Header(actual_header.clone())),
                ),
                HeaderCompare::Semantic => (
                    semantic_header(expected_header, include_sample),
                    semantic_header(actual_header, include_sample),
                ),
                HeaderCompare::CodecSample => (
                    codec_sample_header(expected_header, include_sample),
                    codec_sample_header(actual_header, include_sample),
                ),
                HeaderCompare::Ignore => unreachable!("handled above"),
            };
            let site = compare(&expected_json, &actual_json, "/header")?;
            Some(AlignedDiffReport {
                key,
                expected_line: Some(1),
                actual_line: Some(1),
                path: site.path,
                expected: site.expected,
                actual: site.actual,
                context: Vec::new(),
            })
        }
        (expected_header, actual_header) => Some(AlignedDiffReport {
            key,
            expected_line: expected_header.map(|_| 1),
            actual_line: actual_header.map(|_| 1),
            path: String::new(),
            expected: if expected_header.is_some() {
                "header".to_string()
            } else {
                "null".to_string()
            },
            actual: if actual_header.is_some() {
                "header".to_string()
            } else {
                "null".to_string()
            },
            context: Vec::new(),
        }),
    }
}

fn semantic_header(header: &HeaderPayload, include_sample: bool) -> Json {
    let full = record::payload_to_json(&Payload::Header(header.clone()));
    let Json::Obj(fields) = full else {
        unreachable!("header payload is an object");
    };
    let mut out = Vec::new();
    for (key, value) in fields {
        match key.as_str() {
            "codec" | "boot" | "input" => out.push((key, value)),
            "producer" if include_sample => {
                if let Some(sample) = producer_sample(value) {
                    out.push(("sample_sha256".to_string(), sample));
                }
            }
            _ => {}
        }
    }
    Json::Obj(out)
}

fn codec_sample_header(header: &HeaderPayload, include_sample: bool) -> Json {
    let full = record::payload_to_json(&Payload::Header(header.clone()));
    let Json::Obj(fields) = full else {
        unreachable!("header payload is an object");
    };
    let mut out = Vec::new();
    for (key, value) in fields {
        match key.as_str() {
            "codec" => out.push((key, value)),
            "producer" if include_sample => {
                if let Some(sample) = producer_sample(value) {
                    out.push(("sample_sha256".to_string(), sample));
                }
            }
            _ => {}
        }
    }
    Json::Obj(out)
}

fn producer_sample(producer: Json) -> Option<Json> {
    let Json::Obj(fields) = producer else {
        return None;
    };
    fields
        .into_iter()
        .find(|(key, _)| key == "sample_sha256")
        .map(|(_, value)| value)
        .filter(|value| !matches!(value, Json::Null))
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
            aligned: None,
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
        aligned: None,
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
            let expected_order: Vec<&str> = expected_fields
                .iter()
                .map(|(key, _)| key.as_str())
                .collect();
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
                if let Some(site) =
                    compare(&expected_items[index], &actual_items[index], &item_path)
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
    use crate::record::{
        BootSummary, ClockSummary, HeaderPayload, InputSummary, Payload, ProducerInfo,
    };

    fn header() -> HeaderPayload {
        HeaderPayload {
            codec: "cp932".to_string(),
            producer: ProducerInfo::kernel(),
            tables: vec![],
            boot: BootSummary::from_config(&yu_rt::BootConfig::default()),
            input: InputSummary {
                events: 0,
                sha256: "0".repeat(64),
            },
            clock: ClockSummary::none(),
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
        let shortened = format!(
            "{}
",
            lines.join(
                "
"
            )
        );
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
        let mutated = text.replacen("\"fps\":60,\"skip\":false", "\"skip\":false,\"fps\":60", 1);
        let failure = diff_text(&text, &mutated).unwrap_err();
        assert_eq!(failure.code(), error::code::DIFF);
        assert_eq!(failure.report().unwrap().line, 3);
    }

    #[test]
    fn whitespace_change_is_a_divergence() {
        // 不做任何归一化:纯空白差异也是分歧,报告整条记录。
        let text = log().to_jsonl();
        let mutated = text.replacen("\"fps\":60,\"skip\":false", "\"fps\": 60,\"skip\":false", 1);
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

    // ------------------------------------------------------------ aligned diff

    use crate::record::{FrameDescriptionInput, FrameLevel, FrameSummary};

    fn header_with(producer: ProducerInfo, kernel: &str, tables: &[&str]) -> HeaderPayload {
        HeaderPayload {
            codec: "cp932".to_string(),
            producer,
            tables: tables.iter().map(|table| table.to_string()).collect(),
            boot: BootSummary::from_config(&yu_rt::BootConfig::default()),
            input: InputSummary {
                events: 0,
                sha256: "0".repeat(64),
            },
            clock: ClockSummary::none(),
            kernel: kernel.to_string(),
        }
    }

    fn command_log(header: HeaderPayload, names: &[&str]) -> TraceLog {
        let mut log = TraceLog::new();
        log.push_header(header);
        for name in names {
            log.push(
                Some(1),
                Payload::VmCommand {
                    script: 1,
                    pc: 0,
                    name: name.to_string(),
                    args: Vec::new(),
                },
            );
        }
        log
    }

    fn frame_summary(
        frame: u64,
        op_count: u64,
        executable_ops: u64,
        skipped_ops: u64,
    ) -> FrameSummary {
        FrameSummary::from_input(FrameDescriptionInput {
            frame,
            level: FrameLevel::Event,
            pass_count: 1,
            op_count,
            executable_ops,
            skipped_ops,
            display: Some(br#"{"ops":[]}"#.to_vec()),
            display_sha256: None,
        })
        .unwrap()
    }

    #[test]
    fn aligned_diff_is_producer_neutral() {
        let sample = "a".repeat(64);
        let kernel_header = header_with(
            ProducerInfo::kernel().with_sample_sha256(sample.clone()),
            "yu-trace 0.1.0",
            &["ysc: v495 (105 命令)"],
        );
        let official_header = header_with(
            ProducerInfo::official("official-eris", "0.1").with_sample_sha256(sample),
            "official engine trace",
            &[],
        );
        let expected = command_log(kernel_header.clone(), &["A", "B"]);
        let actual = command_log(official_header, &["A", "B"]);

        // 跨 producer:kernel/tables/producer 名称版本不参与比较。
        assert!(
            diff_aligned_logs(&expected, &actual, &AlignedDiffOptions::cross_producer()).is_ok()
        );

        // 同 producer 严格模式会报 header 差异。
        let failure = diff_aligned_logs(&expected, &actual, &AlignedDiffOptions::same_producer())
            .unwrap_err();
        assert_eq!(failure.code(), error::code::DIFF);
        let report = failure.aligned_report().unwrap();
        assert_eq!(report.key.kind, Kind::Header);
        assert!(report.path.starts_with("/header/"), "{}", report.path);

        // 样本摘要不同:即使跨 producer 也必须报出来(当两侧都存在时)。
        let different_sample = header_with(
            ProducerInfo::official("official-eris", "0.1").with_sample_sha256("b".repeat(64)),
            "official engine trace",
            &[],
        );
        let actual = command_log(different_sample, &["A", "B"]);
        let failure = diff_aligned_logs(&expected, &actual, &AlignedDiffOptions::cross_producer())
            .unwrap_err();
        assert_eq!(
            failure.aligned_report().unwrap().path,
            "/header/sample_sha256"
        );

        // codec 永远参与语义比较。
        let mut wrong_codec = header_with(
            ProducerInfo::official("official-eris", "0.1"),
            "official engine trace",
            &[],
        );
        wrong_codec.codec = "gbk".to_string();
        let actual = command_log(wrong_codec, &["A", "B"]);
        let failure = diff_aligned_logs(&expected, &actual, &AlignedDiffOptions::cross_producer())
            .unwrap_err();
        assert_eq!(failure.aligned_report().unwrap().path, "/header/codec");
    }

    #[test]
    fn aligned_diff_reports_logical_key_line_and_context() {
        let header = header_with(ProducerInfo::kernel(), "yu-trace test", &[]);
        let expected = command_log(header.clone(), &["A", "B", "C"]);
        let actual = command_log(header, &["A", "C"]);
        let options = AlignedDiffOptions {
            header: HeaderCompare::Ignore,
            projection: Projection::all(),
            payload_fields: Vec::new(),
            context: 2,
        };
        // 逻辑键按 (sub,kind,frame,ordinal):B/C 都落在 ordinal=1,
        // 因此这是 payload 分歧(不是"少了一条记录")。
        let failure = diff_aligned_logs(&expected, &actual, &options).unwrap_err();
        let report = failure.aligned_report().unwrap();
        assert_eq!(report.key.sub, Sub::Vm);
        assert_eq!(report.key.kind, Kind::VmCommand);
        assert_eq!(report.key.frame, Some(1));
        assert_eq!(report.key.ordinal, 1);
        assert_eq!(report.expected_line, Some(3));
        assert_eq!(report.actual_line, Some(3));
        assert_eq!(report.path, "/payload/name");
        assert_eq!(report.expected, "\"B\"");
        assert_eq!(report.actual, "\"C\"");
        assert!(!report.context.is_empty());
        assert!(report.context.iter().any(|line| line.contains(">>")));
    }

    #[test]
    fn aligned_diff_reports_missing_and_extra_records() {
        let header = header_with(ProducerInfo::kernel(), "yu-trace test", &[]);
        let options = AlignedDiffOptions {
            header: HeaderCompare::Ignore,
            projection: Projection::all(),
            payload_fields: Vec::new(),
            context: 1,
        };

        let expected = command_log(header.clone(), &["A", "B"]);
        let actual = command_log(header.clone(), &["A"]);
        let failure = diff_aligned_logs(&expected, &actual, &options).unwrap_err();
        let report = failure.aligned_report().unwrap();
        assert_eq!(report.key.ordinal, 1);
        assert_eq!(report.expected_line, Some(3));
        assert_eq!(report.actual_line, None);
        assert_eq!(report.path, "");
        assert_eq!(report.actual, "null");

        let expected = command_log(header.clone(), &["A"]);
        let actual = command_log(header, &["A", "D"]);
        let failure = diff_aligned_logs(&expected, &actual, &options).unwrap_err();
        let report = failure.aligned_report().unwrap();
        assert_eq!(report.key.ordinal, 1);
        assert_eq!(report.expected_line, None);
        assert_eq!(report.actual_line, Some(3));
        assert_eq!(report.expected, "null");
        assert!(report.actual.contains("\"D\""));
    }

    #[test]
    fn aligned_projection_filters_sub_kind_and_frame() {
        let header = header_with(ProducerInfo::kernel(), "yu-trace test", &[]);
        let mut expected = TraceLog::new();
        expected.push_header(header.clone());
        expected.push(
            Some(1),
            Payload::VmCommand {
                script: 1,
                pc: 0,
                name: "A".to_string(),
                args: Vec::new(),
            },
        );
        expected.push(
            Some(2),
            Payload::VmCommand {
                script: 1,
                pc: 0,
                name: "B".to_string(),
                args: Vec::new(),
            },
        );
        let mut actual = TraceLog::new();
        actual.push_header(header);
        actual.push(
            Some(1),
            Payload::VmCommand {
                script: 1,
                pc: 0,
                name: "A".to_string(),
                args: Vec::new(),
            },
        );
        actual.push(
            Some(2),
            Payload::VmCommand {
                script: 1,
                pc: 0,
                name: "C".to_string(),
                args: Vec::new(),
            },
        );

        let frame_one = AlignedDiffOptions {
            header: HeaderCompare::Ignore,
            projection: Projection::all().frames(1, 1),
            payload_fields: Vec::new(),
            context: 0,
        };
        assert!(diff_aligned_logs(&expected, &actual, &frame_one).is_ok());

        let all = AlignedDiffOptions {
            header: HeaderCompare::Ignore,
            projection: Projection::all(),
            payload_fields: Vec::new(),
            context: 0,
        };
        let failure = diff_aligned_logs(&expected, &actual, &all).unwrap_err();
        assert_eq!(failure.aligned_report().unwrap().key.frame, Some(2));

        let only_frames = AlignedDiffOptions {
            header: HeaderCompare::Ignore,
            projection: Projection::with_subs(&[Sub::Frame]),
            payload_fields: Vec::new(),
            context: 0,
        };
        assert!(diff_aligned_logs(&expected, &actual, &only_frames).is_ok());

        let invalid = AlignedDiffOptions {
            header: HeaderCompare::Ignore,
            projection: Projection::all().frames(3, 1),
            payload_fields: Vec::new(),
            context: 0,
        };
        let failure = diff_aligned_logs(&expected, &actual, &invalid).unwrap_err();
        assert_eq!(failure.code(), error::code::SCHEMA);
    }

    #[test]
    fn aligned_payload_field_projection_compares_only_observed_fields() {
        use crate::record::{TraceArg, TraceValue};

        let header = header_with(ProducerInfo::kernel(), "yu-trace test", &[]);
        let mut expected = TraceLog::new();
        expected.push_header(header.clone());
        expected.push(
            Some(1),
            Payload::VmCommand {
                script: 3,
                pc: 7,
                name: "CG".to_string(),
                args: vec![TraceArg {
                    slot: 0,
                    value: TraceValue::Str(b"ev01".to_vec()),
                }],
            },
        );
        let mut actual = TraceLog::new();
        actual.push_header(header);
        actual.push(
            Some(1),
            Payload::VmCommand {
                script: 3,
                pc: 7,
                name: "CG".to_string(),
                // official event capture cannot observe evaluated args
                args: Vec::new(),
            },
        );

        let full = AlignedDiffOptions {
            header: HeaderCompare::Ignore,
            projection: Projection::all(),
            payload_fields: Vec::new(),
            context: 0,
        };
        let failure = diff_aligned_logs(&expected, &actual, &full).unwrap_err();
        assert_eq!(failure.aligned_report().unwrap().path, "/payload/args/0");

        // 显式只比较 official 能观测到的字段:不是容差,其余字段不声称相等。
        let observed = full.with_payload_fields(&["script", "pc", "name"]);
        assert!(diff_aligned_logs(&expected, &actual, &observed).is_ok());
    }

    #[test]
    fn aligned_diff_covers_frame_description_payloads() {
        let header = header_with(ProducerInfo::kernel(), "yu-trace test", &[]);
        let mut expected = TraceLog::new();
        expected.push_header(header.clone());
        expected.push_frame(frame_summary(5, 10, 9, 1));
        let mut actual = TraceLog::new();
        actual.push_header(header);
        actual.push_frame(frame_summary(5, 10, 9, 2));
        let options = AlignedDiffOptions {
            header: HeaderCompare::Ignore,
            projection: Projection::with_subs(&[Sub::Frame]),
            payload_fields: Vec::new(),
            context: 1,
        };
        let failure = diff_aligned_logs(&expected, &actual, &options).unwrap_err();
        let report = failure.aligned_report().unwrap();
        assert_eq!(report.key.sub, Sub::Frame);
        assert_eq!(report.key.kind, Kind::FrameDescription);
        assert_eq!(report.key.frame, Some(5));
        assert_eq!(report.path, "/payload/skipped_ops");
        assert_eq!(report.expected, "1");
        assert_eq!(report.actual, "2");
    }
}
