//! `scenario_probe` · 症状复现探针(证据工具,不参与产品路径)。
//!
//! 用与 `cases/*.json` **同形**的场景文件驱动 `yu-cli::Engine`,逐帧记录:
//!
//! - 每帧的 `drawable / ops / skipped` 与屏幕 RGBA SHA-256;
//! - 本帧新产生的诊断(码 → 次数/首末帧/样例),含 sound 域;
//! - 指定帧的 PNG + display JSON(黑屏/无字/未响应的视觉证据);
//! - 结束时的 skipped top-N、`main_finished`、脚本错误。
//!
//! 用途:P1 台账的“稳定复现步骤 + 证据”一栏。**本工具不改变引擎语义**,
//! 只读 `Engine` 的公开 API。
//!
//! ```text
//! cargo run -p yu-host-desktop --example scenario_probe -- \
//!     <project-root> cases/symptom-enter-story.json \
//!     [--backend soft|wgpu] [--shots 300,360,420] [--out target/tmp/p1]
//! ```
//!
//! 场景文件字段(`cases/*.json` 的子集,多一个 `shots`):
//!
//! ```json
//! {
//!   "name": "enter-story",
//!   "entry": "SYSTEM_START",
//!   "frames": 420,
//!   "backend": "wgpu",
//!   "inputs": [{ "frame": 300, "button": "MOUSE_L", "x": 217, "y": 281 }],
//!   "shots": [300, 330, 420]
//! }
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use yu_cli::engine::{Backend, Engine, EngineOptions};
use yu_cli::json::{self, Value};
use yu_cli::project::Project;
use yu_rt::input::{InputFrame, InputTimeline};

fn main() {
    if let Err(error) = run() {
        eprintln!("scenario_probe: {error}");
        std::process::exit(2);
    }
}

struct Scenario {
    name: String,
    entry: String,
    frames: u32,
    backend: Backend,
    inputs: Vec<(u64, String, i64, i64)>,
    /// `actions:[{"frame":N,"action":"close"}]`:窗口关闭请求(宿主路径)。
    closes: Vec<u64>,
    shots: BTreeSet<u64>,
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = args.first().ok_or_else(usage)?;
    let scenario_path = args.get(1).ok_or_else(usage)?;
    let mut backend = None;
    let mut shots_override: Option<BTreeSet<u64>> = None;
    let mut out_dir = PathBuf::from("target/tmp/p1");
    let mut hash_csv: Option<PathBuf> = None;
    let mut index = 2;
    while index < args.len() {
        match args[index].as_str() {
            "--backend" => {
                let value = args.get(index + 1).ok_or_else(usage)?;
                backend = Some(match value.as_str() {
                    "soft" => Backend::Soft,
                    "wgpu" => Backend::Wgpu,
                    other => return Err(format!("--backend 只接受 soft|wgpu,收到 `{other}`")),
                });
                index += 2;
            }
            "--shots" => {
                let value = args.get(index + 1).ok_or_else(usage)?;
                let mut set = BTreeSet::new();
                for part in value.split(',') {
                    let frame: u64 = part
                        .trim()
                        .parse()
                        .map_err(|_| format!("--shots 需要逗号分隔的帧号,收到 `{part}`"))?;
                    set.insert(frame.max(1));
                }
                shots_override = Some(set);
                index += 2;
            }
            "--out" => {
                out_dir = PathBuf::from(args.get(index + 1).ok_or_else(usage)?);
                index += 2;
            }
            // 逐帧像素 SHA(性能改动不得改像素;soft/wgpu 各跑一份对比)。
            "--hash-csv" => {
                hash_csv = Some(PathBuf::from(args.get(index + 1).ok_or_else(usage)?));
                index += 2;
            }
            other => return Err(format!("未知参数 `{other}`\n{}", usage())),
        }
    }

    let scenario = parse_scenario(Path::new(scenario_path))?;
    let shots = shots_override.unwrap_or_else(|| scenario.shots.clone());
    let backend = backend.unwrap_or(scenario.backend);
    std::fs::create_dir_all(&out_dir)
        .map_err(|e| format!("创建 {} 失败: {e}", out_dir.display()))?;

    let project = Project::discover(Path::new(root), None).map_err(|e| e.message)?;
    let options = EngineOptions {
        backend,
        entry: scenario.entry.clone(),
        fps: 60,
        record_events: false,
        ..EngineOptions::default()
    };
    let mut engine = Engine::new(project, options).map_err(|e| e.message)?;
    engine.set_timeline(build_timeline(&scenario.inputs));

    let mut ledger = Ledger::default();
    let mut shot_notes: Vec<ShotNote> = Vec::new();
    let mut previous_sha: Option<String> = None;
    let mut hash_lines = String::from("host_frame,engine_frame,sha256\n");
    let mut closes = scenario.closes.clone();
    closes.sort_unstable();
    for host_frame in 1..=u64::from(scenario.frames) {
        while closes.first().is_some_and(|frame| *frame <= host_frame) {
            closes.remove(0);
            engine.request_window_close();
            ledger.closes += 1;
        }
        let rendered = engine.tick().map_err(|e| e.message)?;
        let sha = yu_cli::sha::sha256_hex(&rendered.surface.rgba);
        let report = &rendered.report;
        ledger.observe(
            report.frame,
            report.drawable,
            report.ops,
            report.skipped,
            engine.take_diags(),
            engine.take_sound_diags(),
            &sha,
            previous_sha.as_deref(),
            host_frame,
        );
        previous_sha = Some(sha.clone());
        {
            use std::fmt::Write as _;
            let _ = writeln!(hash_lines, "{host_frame},{},{sha}", report.frame);
        }

        if shots.contains(&host_frame) {
            let png_path = out_dir.join(format!("{}-{:04}.png", scenario.name, host_frame));
            let json_path =
                out_dir.join(format!("{}-{:04}.display.json", scenario.name, host_frame));
            let bytes = engine.write_png(&png_path).map_err(|e| e.message)?;
            if let Some(display) = engine.display_json() {
                let _ = std::fs::write(&json_path, display.as_bytes());
            }
            let skipped = skip_summary(engine.render_report());
            let (missing_assets, skipped_text) = asset_summary(engine.render_report());
            let text_runs = engine
                .display_json()
                .as_deref()
                .and_then(|text| json::parse(text).ok())
                .and_then(|value| {
                    value
                        .get("text")
                        .and_then(Value::as_array)
                        .map(<[Value]>::len)
                })
                .unwrap_or(0);
            shot_notes.push(ShotNote {
                host_frame,
                engine_frame: report.frame,
                sha256: sha,
                ops: report.ops,
                skipped: report.skipped,
                drawable: report.drawable,
                png: png_path.display().to_string(),
                png_bytes: bytes,
                skip_top: skipped,
                missing_assets,
                skipped_text,
                text_runs,
            });
        }
        if rendered.report.frame == 0 && host_frame > 4 {
            // 引擎从未推进逻辑帧(不可能在正常路径出现)—— 直接报错,不静默。
            return Err("引擎逻辑帧未推进".to_string());
        }
    }

    let final_report = engine.last_report().clone();
    let rendered_sha = previous_sha.unwrap_or_default();
    let summary = summarize(
        &scenario.name,
        backend.as_str(),
        &scenario,
        &ledger,
        &shot_notes,
        &final_report,
        rendered_sha,
        engine.main_finished(),
        &engine.script_errors(),
    );
    let summary_path = out_dir.join(format!("{}.summary.json", scenario.name));
    std::fs::write(&summary_path, summary.as_bytes())
        .map_err(|e| format!("写 {} 失败: {e}", summary_path.display()))?;
    println!("{summary}");
    println!("summary: {}", summary_path.display());
    if let Some(path) = hash_csv {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("创建 {} 失败: {e}", parent.display()))?;
            }
        }
        std::fs::write(&path, hash_lines)
            .map_err(|e| format!("写 {} 失败: {e}", path.display()))?;
        println!("frame hashes: {}", path.display());
    }
    Ok(())
}

fn usage() -> String {
    "用法: scenario_probe <project-root> <scenario.json> \
     [--backend soft|wgpu] [--shots 300,360] [--out DIR] [--hash-csv PATH]"
        .to_string()
}

fn parse_scenario(path: &Path) -> Result<Scenario, String> {
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("读 {} 失败: {e}", path.display()))?;
    let value = json::parse(&text).map_err(|e| format!("解析 {} 失败: {e}", path.display()))?;
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "scenario".to_string());
    let entry = value
        .get("entry")
        .and_then(Value::as_str)
        .unwrap_or("SYSTEM_START")
        .to_string();
    let frames = value
        .get("frames")
        .and_then(Value::as_u64)
        .unwrap_or(1)
        .max(1) as u32;
    let backend = match value.get("backend").and_then(Value::as_str) {
        Some("wgpu") => Backend::Wgpu,
        _ => Backend::Soft,
    };
    let mut inputs = Vec::new();
    if let Some(list) = value.get("inputs").and_then(Value::as_array) {
        for item in list {
            let frame = item
                .get("frame")
                .and_then(Value::as_u64)
                .ok_or_else(|| "inputs[].frame 缺失".to_string())?;
            let raw = item
                .get("button")
                .and_then(Value::as_str)
                .ok_or_else(|| "inputs[].button 缺失".to_string())?;
            let button = if raw.eq_ignore_ascii_case("CLICK") {
                "MOUSE_L".to_string()
            } else {
                raw.to_string()
            };
            let x = item.get("x").and_then(Value::as_f64).unwrap_or(0.0) as i64;
            let y = item.get("y").and_then(Value::as_f64).unwrap_or(0.0) as i64;
            inputs.push((frame.max(1), button, x, y));
        }
    }
    let mut closes = Vec::new();
    if let Some(list) = value.get("actions").and_then(Value::as_array) {
        for item in list {
            let frame = item
                .get("frame")
                .and_then(Value::as_u64)
                .ok_or_else(|| "actions[].frame 缺失".to_string())?;
            match item.get("action").and_then(Value::as_str) {
                Some("close") => closes.push(frame.max(1)),
                Some(other) => return Err(format!("scenario_probe: 未知 action `{other}`")),
                None => return Err("actions[].action 缺失".to_string()),
            }
        }
    }
    let mut shots = BTreeSet::new();
    if let Some(list) = value.get("shots").and_then(Value::as_array) {
        for item in list {
            if let Some(frame) = item.as_u64() {
                shots.insert(frame.max(1));
            }
        }
    }
    Ok(Scenario {
        name,
        entry,
        frames,
        backend,
        inputs,
        closes,
        shots,
    })
}

/// 按下发生在 `frame`,松开在 `frame+1`(与 `cases/*.json` 的语义一致)。
fn build_timeline(inputs: &[(u64, String, i64, i64)]) -> InputTimeline {
    let mut presses: BTreeMap<u64, BTreeSet<String>> = BTreeMap::new();
    let mut cursors: BTreeMap<u64, (i64, i64)> = BTreeMap::new();
    for (frame, button, x, y) in inputs {
        presses.entry(*frame).or_default().insert(button.clone());
        presses.entry(frame + 1).or_default().remove(button);
        cursors.insert(*frame, (*x, *y));
    }
    let mut frames: BTreeSet<u64> = presses.keys().copied().collect();
    frames.extend(cursors.keys().copied());
    let mut timeline = InputTimeline::new();
    let mut held = BTreeSet::new();
    let mut cursor = (0i64, 0i64);
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

// ---------------------------------------------------------------- ledger

#[derive(Default)]
struct DiagEntry {
    count: u64,
    first_frame: u64,
    last_frame: u64,
    severity: String,
    sample: String,
    /// 前 3 条完整诊断 JSON(含 `where`:脚本/表/数据位置)。
    raw: Vec<String>,
}

#[derive(Default)]
struct Ledger {
    frames: u64,
    drawable_frames: u64,
    first_drawable: Option<u64>,
    last_drawable: Option<u64>,
    static_run_max: u64,
    static_run: u64,
    /// 画面 SHA 变化的帧(无变化的连续区间 = “画面停住”的证据)。
    changes: Vec<u64>,
    diags: BTreeMap<String, DiagEntry>,
    unimplemented: BTreeMap<String, u64>,
    skipped_total: u64,
    closes: u64,
}

impl Ledger {
    #[allow(clippy::too_many_arguments)]
    fn observe(
        &mut self,
        engine_frame: u64,
        drawable: bool,
        ops: u32,
        skipped: u32,
        diags: Vec<yu_log::Diag>,
        sound_diags: Vec<yu_log::Diag>,
        sha: &str,
        previous: Option<&str>,
        host_frame: u64,
    ) {
        self.frames += 1;
        self.skipped_total += u64::from(skipped);
        if drawable {
            self.drawable_frames += 1;
            self.first_drawable.get_or_insert(engine_frame);
            self.last_drawable = Some(engine_frame);
        }
        if previous == Some(sha) {
            self.static_run += 1;
            self.static_run_max = self.static_run_max.max(self.static_run);
        } else {
            self.static_run = 0;
            self.changes.push(engine_frame);
        }
        let _ = ops;
        for diag in diags.into_iter().chain(sound_diags) {
            let code = diag.code.to_string();
            let entry = self.diags.entry(code.clone()).or_insert_with(|| DiagEntry {
                count: 0,
                first_frame: engine_frame,
                last_frame: engine_frame,
                severity: diag.severity.as_str().to_string(),
                sample: diag.detail.clone(),
                raw: Vec::new(),
            });
            entry.count += 1;
            entry.last_frame = engine_frame;
            if entry.raw.len() < 3 {
                entry.raw.push(diag.to_json_line());
            }
            if yu_log::Severity::Unimplemented == diag.severity {
                *self.unimplemented.entry(code).or_insert(0) += 1;
            }
        }
        let _ = host_frame;
    }
}

struct ShotNote {
    host_frame: u64,
    engine_frame: u64,
    sha256: String,
    ops: u32,
    skipped: u32,
    drawable: bool,
    png: String,
    png_bytes: usize,
    skip_top: Vec<(String, u32)>,
    missing_assets: Vec<String>,
    skipped_text: usize,
    text_runs: usize,
}

fn skip_summary(report: Option<&yu_backend_soft::RenderReport>) -> Vec<(String, u32)> {
    let Some(report) = report else {
        return Vec::new();
    };
    let mut map: BTreeMap<String, u32> = BTreeMap::new();
    for skip in &report.skipped {
        *map.entry(format!("{}:{}", skip.kind, skip.reason))
            .or_insert(0) += skip.count;
    }
    let mut list: Vec<(String, u32)> = map.into_iter().collect();
    list.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    list.truncate(8);
    list
}

fn asset_summary(report: Option<&yu_backend_soft::RenderReport>) -> (Vec<String>, usize) {
    let Some(report) = report else {
        return (Vec::new(), 0);
    };
    let missing = report
        .missing_assets
        .iter()
        .take(8)
        .map(|issue| format!("{}:{}", issue.code, issue.key_display()))
        .collect();
    (missing, report.skipped_text)
}

#[allow(clippy::too_many_arguments)]
fn summarize(
    name: &str,
    backend: &str,
    scenario: &Scenario,
    ledger: &Ledger,
    shots: &[ShotNote],
    report: &yu_cli::engine::FrameReport,
    final_sha: String,
    main_finished: bool,
    script_errors: &[String],
) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str(&format!(
        "  \"name\": {},\n  \"backend\": {},\n  \"entry\": {},\n  \"frames\": {},\n",
        json_string(name),
        json_string(backend),
        json_string(&scenario.entry),
        ledger.frames
    ));
    out.push_str(&format!(
        "  \"engine_frame\": {},\n  \"main_finished\": {},\n",
        report.frame, main_finished
    ));
    out.push_str(&format!(
        "  \"drawable_frames\": {},\n  \"first_drawable_frame\": {},\n  \"last_drawable_frame\": {},\n",
        ledger.drawable_frames,
        opt(ledger.first_drawable),
        opt(ledger.last_drawable)
    ));
    out.push_str(&format!(
        "  \"final_ops\": {},\n  \"final_skipped\": {},\n  \"skipped_total\": {},\n",
        report.ops, report.skipped, ledger.skipped_total
    ));
    out.push_str(&format!(
        "  \"final_sha256\": {},\n  \"static_run_max_frames\": {},\n  \"close_requests\": {},\n",
        json_string(&final_sha),
        ledger.static_run_max,
        ledger.closes
    ));
    out.push_str("  \"sha_change_frames\": [");
    for (index, frame) in ledger.changes.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        out.push_str(&frame.to_string());
    }
    out.push_str("],\n");

    out.push_str("  \"diags\": [\n");
    for (index, (code, entry)) in ledger.diags.iter().enumerate() {
        out.push_str(&format!(
            "    {{\"code\": {}, \"severity\": {}, \"count\": {}, \
             \"first_frame\": {}, \"last_frame\": {}, \"sample\": {}, \"raw\": [{}]}}{}\n",
            json_string(code),
            json_string(&entry.severity),
            entry.count,
            entry.first_frame,
            entry.last_frame,
            json_string(&entry.sample),
            entry
                .raw
                .iter()
                .map(|line| json_string(line))
                .collect::<Vec<_>>()
                .join(", "),
            if index + 1 == ledger.diags.len() {
                ""
            } else {
                ","
            }
        ));
    }
    out.push_str("  ],\n");

    out.push_str("  \"unimplemented\": [");
    let mut unimplemented: Vec<(&String, &u64)> = ledger.unimplemented.iter().collect();
    unimplemented.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    for (index, (code, count)) in unimplemented.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        out.push_str(&format!(
            "{{\"code\": {}, \"count\": {count}}}",
            json_string(code)
        ));
    }
    out.push_str("],\n");

    out.push_str("  \"shots\": [\n");
    for (index, shot) in shots.iter().enumerate() {
        let skip_top = shot
            .skip_top
            .iter()
            .map(|(key, count)| format!("{{\"key\": {}, \"count\": {count}}}", json_string(key)))
            .collect::<Vec<_>>()
            .join(", ");
        let missing = shot
            .missing_assets
            .iter()
            .map(|key| json_string(key))
            .collect::<Vec<_>>()
            .join(", ");
        let comma = if index + 1 == shots.len() { "" } else { "," };
        out.push_str(&format!(
            "    {{\"host_frame\": {}, \"engine_frame\": {}, \"drawable\": {}, \
             \"ops\": {}, \"skipped\": {}, \"sha256\": {}, \"png\": {}, \
             \"png_bytes\": {}, \"skip_top\": [{}], \"missing_assets\": [{}], \
             \"skipped_text\": {}, \"text_runs\": {}}}{}\n",
            shot.host_frame,
            shot.engine_frame,
            shot.drawable,
            shot.ops,
            shot.skipped,
            json_string(&shot.sha256),
            json_string(&shot.png),
            shot.png_bytes,
            skip_top,
            missing,
            shot.skipped_text,
            shot.text_runs,
            comma
        ));
    }
    out.push_str("  ],\n");

    out.push_str("  \"script_errors\": [");
    for (index, error) in script_errors.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        out.push_str(&json_string(error));
    }
    out.push_str("]\n}\n");
    out
}

fn opt(value: Option<u64>) -> String {
    value
        .map(|v| v.to_string())
        .unwrap_or_else(|| "null".to_string())
}

fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
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
    out
}
