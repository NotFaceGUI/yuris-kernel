//! `state_audit` · gfx 状态增长审计探针(证据工具,不参与产品路径)。
//!
//! 驱动 `yu_engine::Engine`,在指定帧上打印 gfx 状态规模快照:图层表、
//! 复制日志、图像加工日志、后端纹理缓存。用途是回答「渲染耗时为什么随
//! 游玩时长增长」——单调增长的容器就是嫌疑。
//!
//! ```text
//! cargo run -p yu-host-desktop --example state_audit -- \
//!     <project-root> <scenario.json> --frames 1,600,1200,2800
//! ```
//!
//! 场景文件与 `cases/*.json` 同形(`inputs` 驱动点按),只读 `frame`/`inputs`。

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use yu_engine::engine::{Backend, Engine, EngineOptions};
use yu_engine::json::{self, Value};
use yu_engine::project::Project;
use yu_rt::input::{InputFrame, InputTimeline};

fn main() {
    if let Err(error) = run() {
        eprintln!("state_audit: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = args.first().ok_or_else(usage)?;
    let scenario_path = args.get(1).ok_or_else(usage)?;
    let mut shots: BTreeSet<u64> = BTreeSet::new();
    let mut frames: u32 = 0;
    let mut backend = Backend::Wgpu;
    let mut index = 2;
    while index < args.len() {
        match args[index].as_str() {
            "--frames" => {
                let value = args.get(index + 1).ok_or_else(usage)?;
                for part in value.split(',') {
                    let frame: u64 = part
                        .trim()
                        .parse()
                        .map_err(|_| format!("--frames 需要逗号分隔的帧号,收到 `{part}`"))?;
                    shots.insert(frame.max(1));
                    frames = frames.max(frame as u32);
                }
                index += 2;
            }
            "--backend" => {
                backend = match args.get(index + 1).ok_or_else(usage)?.as_str() {
                    "soft" => Backend::Soft,
                    "wgpu" => Backend::Wgpu,
                    other => return Err(format!("--backend 只接受 soft|wgpu,收到 `{other}`")),
                };
                index += 2;
            }
            other => return Err(format!("未知参数 `{other}`\n{}", usage())),
        }
    }
    if shots.is_empty() {
        return Err(usage());
    }

    let project = Project::discover(Path::new(root), None).map_err(|e| e.message)?;
    let options = EngineOptions {
        backend,
        entry: "SYSTEM_START".to_string(),
        fps: 60,
        reconcile: false,
        ..EngineOptions::default()
    };
    let mut engine = Engine::new(project, options).map_err(|e| e.message)?;
    let inputs = read_inputs(Path::new(scenario_path))?;
    engine.set_timeline(build_timeline(&inputs));

    println!(
        "{:>6} {:>7} {:>7} {:>7} {:>7} {:>8} {:>7} {:>8}",
        "frame", "layers", "visible", "copies", "images", "tex_cache", "passes", "render"
    );
    let mut dumped = false;
    for host_frame in 1..=u64::from(frames) {
        engine.tick().map_err(|e| e.message)?;
        if shots.contains(&host_frame) && !dumped {
            dumped = true;
            if let Some(state) = engine.gfx_state() {
                // 谁在读被缩放出来的层?COPY 源与转场配对的层名是判定
                // 「UV 化是否够用」的关键事实。
                println!("--- copies on frame {host_frame}:");
                let mut spl_copies = 0;
                for copy in state.copies() {
                    let source = match &copy.source {
                        yu_gfx::op::CopySource::Layer(id) => id.display(),
                        yu_gfx::op::CopySource::Group(id) => format!("{}(group)", id.display()),
                    };
                    if source.contains("SPL") {
                        println!("    COPY(SPL) {source} -> {}", copy.dest.display());
                        spl_copies += 1;
                    }
                }
                println!("    SPL copies: {spl_copies} / {}", state.copies().len());
                println!("--- image ops on frame {host_frame}:");
                for image in state.images() {
                    let target = image
                        .target
                        .as_ref()
                        .map(|t| t.display())
                        .unwrap_or_else(|| image.source.display());
                    println!(
                        "    IMAGE {} src={} ->{} region=({:?},{:?},{:?},{:?}) size=({:?},{:?})",
                        image.action.name(),
                        image.source.display(),
                        target,
                        image.x,
                        image.y,
                        image.x2,
                        image.y2,
                        image.sx,
                        image.sy
                    );
                }
                println!("--- SPL layer states on frame {host_frame}:");
                for id in [
                    ":/W0/SPL101",
                    ":/W0/SPL201",
                    ":/W0/SPL401",
                    ":/W0/SPLBUF101",
                ] {
                    let lid = yu_gfx::LayerId::new(id.as_bytes().to_vec());
                    if let Some(layer) = state.layer(&lid) {
                        let tid = layer
                            .transition
                            .as_ref()
                            .and_then(|t| t.tid.as_ref())
                            .map(|t| t.display());
                        println!(
                            "    {id}: visible={} tex={:?} size={:?} xy=({},{}) tid={:?}",
                            layer.visible,
                            layer.texture,
                            layer.image_size().map(|s| (s.width, s.height)),
                            layer.transform.x,
                            layer.transform.y,
                            tid
                        );
                    } else {
                        println!("    {id}: <absent>");
                    }
                }
            }
        }
        if !shots.contains(&host_frame) {
            continue;
        }
        let report = engine.last_report().clone();
        let (copies, images) = match engine.gfx_state() {
            Some(state) => (state.copies().len(), state.images().len()),
            None => (0, 0),
        };
        println!(
            "{:>6} {:>7} {:>7} {:>7} {:>7} {:>8} {:>7} {:>8.2}",
            host_frame,
            report.layers,
            report.visible,
            copies,
            images,
            engine.textures_cached(),
            report.passes,
            report.render_ms
        );
    }
    Ok(())
}

fn usage() -> String {
    "用法: state_audit <project-root> <scenario.json> --frames 1,600,1200 [--backend soft|wgpu]"
        .to_string()
}

fn read_inputs(path: &Path) -> Result<Vec<(u64, String, i64, i64)>, String> {
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("读 {} 失败: {e}", path.display()))?;
    let value = json::parse(&text).map_err(|e| format!("解析 {} 失败: {e}", path.display()))?;
    let mut inputs = Vec::new();
    if let Some(list) = value.get("inputs").and_then(Value::as_array) {
        for item in list {
            let frame = item
                .get("frame")
                .and_then(Value::as_u64)
                .unwrap_or(1)
                .max(1);
            let raw = item
                .get("button")
                .and_then(Value::as_str)
                .unwrap_or("MOUSE_L");
            let x = item.get("x").and_then(Value::as_f64).unwrap_or(0.0) as i64;
            let y = item.get("y").and_then(Value::as_f64).unwrap_or(0.0) as i64;
            inputs.push((frame, raw.to_string(), x, y));
        }
    }
    Ok(inputs)
}

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
