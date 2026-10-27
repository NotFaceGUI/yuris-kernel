//! `yu-cli case`:用例执行器。
//!
//! 用例文件 `cases/*.json` 的格式见 `crates/yu-cli/README.md`。比较项:
//! `display_json_sha256`、`png_sha256`、`ops_total`、`skipped_ops_total`;
//! 缺省字段不比较。`--update` 重生成期望并打印逐字段 diff,人工审后入库。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use yu_render_core::png;
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
    let debug_mode = match args.flag("debug-mode") {
        Some(value) => ArgReader::on_off("debug-mode", &value)?,
        None => false,
    };
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
        match run_case(&project, path, update, debug_mode) {
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
    /// 存档域用例(P4 M-C):save 根重定向到 `target/tmp/save-cases/<名>`
    /// (可写;每轮清空重建),并在 expect 里支持 `save_state_sha256`。
    save: bool,
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
    /// 存档域快照摘要(`SaveSnapshot` 的确定性文本;P4 M-C)。
    save_state_sha256: Option<String>,
    /// **Fatal 诊断码断言**(2026-10-23 审查 H1:门禁曾对 Fatal 全盲 ——
    /// 主任务被 Fatal 冻结后像素不变照样 PASS)。在场的期望 = 实际发生的
    /// Fatal 码集合(去重升序)必须逐项相等;`[]` = 断言无 Fatal。
    /// 数据源 = `Engine::take_diag_report()`(P1 H-P1-02)。
    fatal_codes: Option<Vec<String>>,
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
        if let Some(v) = object.get("save_state_sha256").and_then(|v| v.as_str()) {
            expect.save_state_sha256 = Some(v.to_string());
        }
        if let Some(v) = object.get("fatal_codes").and_then(|v| v.as_array()) {
            expect.fatal_codes = Some(
                v.iter()
                    .filter_map(|c| c.as_str().map(str::to_string))
                    .collect::<Vec<_>>(),
            );
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
        if let Some(v) = &self.save_state_sha256 {
            map.insert("save_state_sha256".to_string(), Value::String(v.clone()));
        }
        if let Some(v) = &self.fatal_codes {
            map.insert(
                "fatal_codes".to_string(),
                Value::Array(v.iter().map(|c| Value::String(c.clone())).collect()),
            );
        }
        Value::Object(map)
    }

    fn is_empty(&self) -> bool {
        self.display_json_sha256.is_none()
            && self.png_sha256.is_none()
            && self.ops_total.is_none()
            && self.skipped_ops_total.is_none()
            && self.save_state_sha256.is_none()
            && self.fatal_codes.is_none()
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
    let save = value.get("save").and_then(|v| v.as_bool()).unwrap_or(false);
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
        save,
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

fn run_case(project: &Project, path: &Path, update: bool, debug_mode: bool) -> CliResult<bool> {
    let spec = parse_case(path)?;
    // 存档域用例:save 根重定向到 target/tmp(产物只进 target/tmp,
    // 语料工程目录永不落盘;每轮清空重建保证确定性)。
    let save_root = spec.save.then(|| {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("crates/yu-cli -> workspace root")
            .join("target/tmp/save-cases")
            .join(&spec.name)
    });
    if let Some(dir) = &save_root {
        let _ = std::fs::remove_dir_all(dir);
        std::fs::create_dir_all(dir)
            .map_err(|e| Failure::runtime(format!("创建 save 根 {} 失败: {e}", dir.display())))?;
        // 播种:官方 .sd 快照(若已快照到 target/tmp/yssd-corpus)复制进
        // save 根,让槽表事实进入 save_state_sha256。快照缺失时留空
        // (摘要只覆盖事件计数,同样确定性)。
        let seed = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("crates/yu-cli -> workspace root")
            .join("target/tmp/yssd-corpus");
        if let Ok(entries) = std::fs::read_dir(&seed) {
            for entry in entries.flatten() {
                if entry.path().extension().and_then(|e| e.to_str()) == Some("sd") {
                    let target = dir.join(entry.file_name());
                    std::fs::copy(entry.path(), &target).map_err(|e| {
                        Failure::runtime(format!("播种 {} 失败: {e}", entry.path().display()))
                    })?;
                }
            }
        }
    }
    let opts = EngineOptions {
        backend: spec.backend,
        entry: spec.entry.clone(),
        record_events: spec.record_events,
        debug_mode,
        save_root: save_root.clone(),
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

    let save_state = match save_root.as_ref() {
        Some(_) => {
            let snapshot = engine
                .save_snapshot()
                .ok_or_else(|| Failure::runtime("存档域宿主未安装"))?;
            let text = save_snapshot_text(&snapshot);
            Some(sha256_hex(text.as_bytes()))
        }
        None => None,
    };

    // Fatal 诊断码集合(去重升序;severity = Fatal 的内核+域宿主诊断)。
    let diag_report = engine.take_diag_report();
    let fatal_codes: Vec<String> = {
        let mut set: BTreeSet<String> = diag_report
            .diags
            .iter()
            .filter(|d| matches!(d.severity, yu_log::Severity::Fatal))
            .map(|d| d.code.as_ref().to_string())
            .collect();
        // 执行器结束任务留下的脚本错误同样是 Fatal 级事实(任务终止)。
        set.extend(diag_report.script_errors.iter().map(|e| {
            let code = e.split(':').next().unwrap_or(e).trim();
            code.to_string()
        }));
        set.into_iter().collect()
    };

    let actual = Expect {
        display_json_sha256: Some(sha256_hex(display_json.as_bytes())),
        png_sha256: Some(sha256_hex(&png_bytes)),
        ops_total: Some(report.ops as u64),
        skipped_ops_total: Some(report.skipped as u64),
        save_state_sha256: save_state,
        fatal_codes: Some(fatal_codes),
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
    compare(
        "save_state_sha256",
        spec.expect.save_state_sha256.as_deref(),
        actual.save_state_sha256.as_deref(),
        &mut diffs,
    );
    if let Some(expected_codes) = &spec.expect.fatal_codes {
        // 两侧同为「去重升序」;期望在场才断言(旧用例缺字段 = 不检查)。
        compare(
            "fatal_codes",
            Some(expected_codes.join(",").as_str()),
            Some(
                actual
                    .fatal_codes
                    .clone()
                    .unwrap_or_default()
                    .join(",")
                    .as_str(),
            ),
            &mut diffs,
        );
    }
    if diffs.is_empty() {
        Ok(true)
    } else {
        output::error(format!(
            "[case] {}: frame={} ticks={} ops={} skipped={}",
            spec.name, report.frame, ticks, report.ops, report.skipped
        ));
        for diff in diffs {
            output::error(format!("[case]   {diff}"));
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

/// `SaveSnapshot` 的确定性单行文本(save_state_sha256 的输入)。
///
/// 只含稳定事实:槽文件(名/尺寸/块数/DNO 表)、预约(DNO/块类型/变量 id,
/// **不含块值**——数组快照动辄数千元素)、域事件计数。字段序固定。
fn save_snapshot_text(snapshot: &yu_save::SaveSnapshot) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "save_calls={} load_calls={} saveend_calls={} fileact_calls={}\n",
        snapshot.save_calls, snapshot.load_calls, snapshot.saveend_calls, snapshot.fileact_calls
    ));
    out.push_str("slots:\n");
    for slot in &snapshot.slots {
        let dnos: Vec<String> = slot
            .dnos
            .iter()
            .map(|(dno, var)| format!("{dno}:{var}"))
            .collect();
        out.push_str(&format!(
            "  {} size={} blocks={} dnos={{{}}}\n",
            slot.name,
            slot.size,
            slot.blocks,
            dnos.join(",")
        ));
    }
    out.push_str("reservations:\n");
    for fact in &snapshot.reservations {
        if fact.btype == 2 {
            let subs: Vec<String> = fact.subs.iter().map(|(id, _)| id.to_string()).collect();
            out.push_str(&format!(
                "  dno={} btype=2 var=0 subs=[{}]\n",
                fact.dno,
                subs.join(",")
            ));
        } else {
            out.push_str(&format!(
                "  dno={} btype={} var={}\n",
                fact.dno, fact.btype, fact.var_id
            ));
        }
    }
    out
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
