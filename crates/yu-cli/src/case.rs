//! `yu-cli case`:用例执行器。
//!
//! 用例文件 `cases/*.json` 的格式见 `crates/yu-cli/README.md`。比较项:
//! `display_json_sha256`、`png_sha256`、`ops_total`、`skipped_ops_total`;
//! 缺省字段不比较。`--update` 重生成期望并打印逐字段 diff,人工审后入库。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use yu_backend_soft::png;
use yu_rt::input::{InputFrame, InputTimeline};

use crate::args::ArgReader;
use crate::engine::{Backend, Engine, EngineOptions};
use crate::json::{self, Value};
use crate::output;
use crate::project::Project;
use crate::sha::sha256_hex;
use crate::{CliResult, Failure};

pub fn from_args(tokens: Vec<String>) -> CliResult<()> {
    let mut args = ArgReader::new(tokens);
    let root = args
        .positional(0)
        .ok_or_else(|| Failure::usage("用法: yu-cli case <project-root> --all | --case NAME"))?;
    let all = args.switch("all");
    let only = args.flag("case");
    let update = args.switch("update");
    let data = args.flag("data").map(PathBuf::from);
    let cases_dir = args
        .flag("cases")
        .map(PathBuf::from)
        .unwrap_or_else(default_cases_dir);
    args.finish()?;
    if !all && only.is_none() {
        return Err(Failure::usage(
            "需要 --all 或 --case NAME(二选一;可以同时给 --update)",
        ));
    }

    let project = Project::discover(Path::new(&root), data.as_deref())?;
    let mut files = list_cases(&cases_dir)?;
    if let Some(name) = &only {
        files.retain(|path| case_name(path) == *name);
        if files.is_empty() {
            return Err(Failure::runtime(format!(
                "找不到用例 `{name}`(目录 {})",
                cases_dir.display()
            )));
        }
    }

    let mut passed = 0usize;
    let mut failed = 0usize;
    for path in &files {
        match run_case(&project, path, update) {
            Ok(true) => {
                passed += 1;
                output::line(format!("case {}: PASS", case_name(path)));
            }
            Ok(false) => {
                failed += 1;
                output::line(format!("case {}: FAIL", case_name(path)));
            }
            Err(failure) => {
                failed += 1;
                output::error(format!(
                    "case {}: ERROR {}",
                    case_name(path),
                    failure.message
                ));
            }
        }
    }
    output::line(format!(
        "{passed} passed; {failed} failed; {} total (cases: {})",
        files.len(),
        cases_dir.display()
    ));
    if failed > 0 {
        Err(Failure::runtime(format!("{failed} 条用例失败")))
    } else {
        Ok(())
    }
}

fn default_cases_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/yu-cli -> workspace root")
        .join("cases")
}

fn list_cases(dir: &Path) -> CliResult<Vec<PathBuf>> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| Failure::runtime(format!("读取用例目录 {} 失败: {e}", dir.display())))?;
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("json"))
        .collect();
    files.sort();
    if files.is_empty() {
        return Err(Failure::runtime(format!(
            "用例目录里没有 *.json:{}",
            dir.display()
        )));
    }
    Ok(files)
}

fn case_name(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("<unnamed>")
        .to_string()
}

struct CaseSpec {
    name: String,
    entry: String,
    frames: u32,
    until_visible: bool,
    record_events: bool,
    backend: Backend,
    inputs: Vec<CaseInput>,
    expect: Expect,
}

#[derive(Clone)]
struct CaseInput {
    frame: u64,
    button: String,
    x: i64,
    y: i64,
}

#[derive(Default, Clone)]
struct Expect {
    display_json_sha256: Option<String>,
    png_sha256: Option<String>,
    ops_total: Option<u64>,
    skipped_ops_total: Option<u64>,
}

impl Expect {
    fn from_value(value: Option<&Value>) -> Expect {
        let mut expect = Self::default();
        let Some(value) = value else {
            return expect;
        };
        let Some(object) = value.as_object() else {
            return expect;
        };
        if let Some(v) = object.get("display_json_sha256").and_then(|v| v.as_str()) {
            expect.display_json_sha256 = Some(v.to_string());
        }
        if let Some(v) = object.get("png_sha256").and_then(|v| v.as_str()) {
            expect.png_sha256 = Some(v.to_string());
        }
        if let Some(v) = object.get("ops_total").and_then(|v| v.as_u64()) {
            expect.ops_total = Some(v);
        }
        if let Some(v) = object.get("skipped_ops_total").and_then(|v| v.as_u64()) {
            expect.skipped_ops_total = Some(v);
        }
        expect
    }

    fn to_value(&self) -> Value {
        let mut map = BTreeMap::new();
        if let Some(v) = &self.display_json_sha256 {
            map.insert("display_json_sha256".to_string(), Value::String(v.clone()));
        }
        if let Some(v) = &self.png_sha256 {
            map.insert("png_sha256".to_string(), Value::String(v.clone()));
        }
        if let Some(v) = self.ops_total {
            map.insert("ops_total".to_string(), Value::Number(v as f64));
        }
        if let Some(v) = self.skipped_ops_total {
            map.insert("skipped_ops_total".to_string(), Value::Number(v as f64));
        }
        Value::Object(map)
    }

    fn is_empty(&self) -> bool {
        self.display_json_sha256.is_none()
            && self.png_sha256.is_none()
            && self.ops_total.is_none()
            && self.skipped_ops_total.is_none()
    }
}

fn parse_case(path: &Path) -> CliResult<CaseSpec> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| Failure::runtime(format!("读 {} 失败: {e}", path.display())))?;
    let value = json::parse(&text)
        .map_err(|e| Failure::runtime(format!("解析 {} 失败: {e}", path.display())))?;
    let name = value
        .get("name")
        .and_then(|v| v.as_str())
        .map(|v| v.to_string())
        .unwrap_or_else(|| case_name(path));
    let entry = value
        .get("entry")
        .and_then(|v| v.as_str())
        .unwrap_or("SYSTEM_START")
        .to_string();
    let frames = value
        .get("frames")
        .and_then(|v| v.as_u64())
        .unwrap_or(1)
        .max(1) as u32;
    let until_visible = value
        .get("until_visible")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let record_events = value
        .get("record_events")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let backend = match value.get("backend").and_then(|v| v.as_str()) {
        Some("wgpu") => Backend::Wgpu,
        _ => Backend::Soft,
    };
    let mut inputs = Vec::new();
    if let Some(list) = value.get("inputs").and_then(|v| v.as_array()) {
        for item in list {
            let frame = item
                .get("frame")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| Failure::runtime("inputs[].frame 缺失"))?;
            let raw = item
                .get("button")
                .and_then(|v| v.as_str())
                .ok_or_else(|| Failure::runtime("inputs[].button 缺失"))?;
            let button = if raw.eq_ignore_ascii_case("CLICK") {
                "MOUSE_L".to_string()
            } else {
                raw.to_string()
            };
            let x = item.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0) as i64;
            let y = item.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0) as i64;
            inputs.push(CaseInput {
                frame: frame.max(1),
                button,
                x,
                y,
            });
        }
    }
    Ok(CaseSpec {
        name,
        entry,
        frames,
        until_visible,
        record_events,
        backend,
        inputs,
        expect: Expect::from_value(value.get("expect")),
    })
}

fn build_timeline(inputs: &[CaseInput]) -> InputTimeline {
    // frame → 按下集合;按下发生在 frame,frame+1 松开。
    // 光标从该帧起保持(InputTimeline::effective 的语义)。
    let mut presses: BTreeMap<u64, BTreeSet<String>> = BTreeMap::new();
    let mut cursors: BTreeMap<u64, (i64, i64)> = BTreeMap::new();
    for input in inputs {
        presses
            .entry(input.frame)
            .or_default()
            .insert(input.button.clone());
        presses
            .entry(input.frame + 1)
            .or_default()
            .remove(&input.button);
        cursors.insert(input.frame, (input.x, input.y));
    }
    let mut frames: BTreeSet<u64> = presses.keys().copied().collect();
    frames.extend(cursors.keys().copied());
    let mut timeline = InputTimeline::new();
    let mut held = BTreeSet::new();
    let mut cursor = (0i64, 0i64);
    // 按帧序展开成完整快照:显式空集 = 松开。
    for frame in frames {
        if let Some(set) = presses.get(&frame) {
            held = set.clone();
        }
        if let Some(&(x, y)) = cursors.get(&frame) {
            cursor = (x, y);
        }
        timeline.set(
            frame,
            InputFrame::new(held.iter().cloned()).with_cursor(cursor.0, cursor.1),
        );
    }
    timeline
}

fn run_case(project: &Project, path: &Path, update: bool) -> CliResult<bool> {
    let spec = parse_case(path)?;
    let opts = EngineOptions {
        backend: spec.backend,
        entry: spec.entry.clone(),
        record_events: spec.record_events,
        ..EngineOptions::default()
    };
    let mut engine = Engine::new(project.clone(), opts)?;
    engine.set_timeline(build_timeline(&spec.inputs));
    let mut ticks = 0u32;
    for _ in 0..spec.frames {
        let rendered = engine.tick()?;
        ticks += 1;
        if spec.until_visible && rendered.report.drawable {
            break;
        }
    }
    let report = engine.last_report().clone();
    let display_json = engine
        .display_json()
        .ok_or_else(|| Failure::runtime("用例没有产出帧"))?;
    let surface = engine
        .last_surface()
        .ok_or_else(|| Failure::runtime("用例没有产出像素"))?;
    let png_bytes =
        png::png_bytes(surface).map_err(|e| Failure::runtime(format!("PNG 编码失败: {e}")))?;

    let actual = Expect {
        display_json_sha256: Some(sha256_hex(display_json.as_bytes())),
        png_sha256: Some(sha256_hex(&png_bytes)),
        ops_total: Some(report.ops as u64),
        skipped_ops_total: Some(report.skipped as u64),
    };

    if update {
        write_expect(path, &actual)?;
        output::line(format!(
            "case {}: updated (frame={} ticks={} ops={} skipped={})",
            spec.name, report.frame, ticks, report.ops, report.skipped
        ));
        return Ok(true);
    }

    if spec.expect.is_empty() {
        return Err(Failure::runtime(format!(
            "用例 {} 没有 expect;先跑 --update 生成",
            spec.name
        )));
    }

    let mut diffs = Vec::new();
    compare(
        "display_json_sha256",
        spec.expect.display_json_sha256.as_deref(),
        actual.display_json_sha256.as_deref(),
        &mut diffs,
    );
    compare(
        "png_sha256",
        spec.expect.png_sha256.as_deref(),
        actual.png_sha256.as_deref(),
        &mut diffs,
    );
    compare(
        "ops_total",
        spec.expect.ops_total.map(|v| v.to_string()).as_deref(),
        actual.ops_total.map(|v| v.to_string()).as_deref(),
        &mut diffs,
    );
    compare(
        "skipped_ops_total",
        spec.expect
            .skipped_ops_total
            .map(|v| v.to_string())
            .as_deref(),
        actual.skipped_ops_total.map(|v| v.to_string()).as_deref(),
        &mut diffs,
    );
    if diffs.is_empty() {
        Ok(true)
    } else {
        output::error(format!(
            "case {}: frame={} ticks={} ops={} skipped={}",
            spec.name, report.frame, ticks, report.ops, report.skipped
        ));
        for diff in diffs {
            output::error(format!("  {diff}"));
        }
        Ok(false)
    }
}

fn compare(name: &str, expected: Option<&str>, actual: Option<&str>, diffs: &mut Vec<String>) {
    if let (Some(expected), Some(actual)) = (expected, actual) {
        if expected != actual {
            diffs.push(format!("{name}: expected={expected} actual={actual}"));
        }
    }
}

fn write_expect(path: &Path, actual: &Expect) -> CliResult<()> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| Failure::runtime(format!("读 {} 失败: {e}", path.display())))?;
    let mut value = json::parse(&text)
        .map_err(|e| Failure::runtime(format!("解析 {} 失败: {e}", path.display())))?;
    let Value::Object(ref mut map) = value else {
        return Err(Failure::runtime("用例根必须是对象"));
    };
    // 打印逐字段 diff(人工审阅用)。
    if let Some(Value::Object(old)) = map.get("expect") {
        if let Value::Object(new) = actual.to_value() {
            for (key, new_value) in new {
                let old_text = old
                    .get(&key)
                    .map(short)
                    .unwrap_or_else(|| "<none>".to_string());
                let new_text = short(&new_value);
                if old_text != new_text {
                    output::line(format!("  diff {key}: {old_text} -> {new_text}"));
                }
            }
        }
    }
    map.insert("expect".to_string(), actual.to_value());
    let pretty = json::to_pretty(&value);
    std::fs::write(path, pretty)
        .map_err(|e| Failure::runtime(format!("写 {} 失败: {e}", path.display())))?;
    Ok(())
}

fn short(value: &Value) -> String {
    match value {
        Value::String(text) => {
            if text.chars().count() > 16 {
                format!("{}…", text.chars().take(16).collect::<String>())
            } else {
                text.clone()
            }
        }
        Value::Number(number) => format!("{number}"),
        Value::Bool(flag) => flag.to_string(),
        Value::Null => "null".to_string(),
        Value::Array(_) => "[…]".to_string(),
        Value::Object(_) => "{…}".to_string(),
    }
}

/// 用例 `inputs` 的时间线展开(单测用)。
pub fn timeline_for_test(inputs: &[(u64, &str)]) -> InputTimeline {
    let inputs: Vec<CaseInput> = inputs
        .iter()
        .map(|(frame, button)| CaseInput {
            frame: *frame,
            button: (*button).to_string(),
            x: 0,
            y: 0,
        })
        .collect();
    build_timeline(&inputs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeline_presses_for_one_frame_then_releases() {
        let timeline = timeline_for_test(&[(2, "MOUSE_L")]);
        assert!(timeline.at(1).is_none());
        assert!(timeline.at(2).unwrap().is_pressed("MOUSE_L"));
        assert!(!timeline.at(3).unwrap().is_pressed("MOUSE_L"));
        // 未显式设置的后续帧继承最后状态(已松开)。
        assert!(!timeline.effective(10).is_pressed("MOUSE_L"));
    }

    #[test]
    fn two_buttons_on_different_frames_keep_independent_state() {
        // 每个输入都是**一帧按下**:frame 按下,frame+1 松开。
        let timeline = timeline_for_test(&[(1, "KEY_ENTER"), (3, "MOUSE_L")]);
        assert!(timeline.effective(1).is_pressed("KEY_ENTER"));
        assert!(!timeline.effective(2).is_pressed("KEY_ENTER"));
        assert!(!timeline.effective(2).is_pressed("MOUSE_L"));
        assert!(timeline.effective(3).is_pressed("MOUSE_L"));
        assert!(!timeline.effective(4).is_pressed("MOUSE_L"));
    }
}
