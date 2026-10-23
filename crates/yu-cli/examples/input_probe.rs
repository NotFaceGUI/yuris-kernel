//! 输入/按钮回归探针：在真实语料上注入鼠标点击并落帧。
//!
//! 用法：
//!
//! ```text
//! cargo run -p yu-cli --example input_probe -- <project-root> [--frames N] \
//!     [--click FRAME,X,Y] [--shot FRAME=PATH] [--json FRAME=PATH]
//! ```
//!
//! 点击语义：`FRAME` 帧按下并松开一帧（`frame+1` 抬起）；光标从按下前一帧
//! 起保持在 `(X,Y)`。输出用于人工比对标题按钮、弹窗关闭与场景返回。
//!
//! 该示例只做诊断，不参与内核语义。

use std::path::{Path, PathBuf};

use yu_backend_soft::png;
use yu_cli::args::ArgReader;
use yu_cli::engine::{Backend, Engine, EngineOptions};
use yu_cli::project::Project;
use yu_rt::input::{InputFrame, InputTimeline};

fn main() {
    if let Err(message) = run() {
        eprintln!("input_probe: {message}");
        std::process::exit(2);
    }
}

fn to_u64(value: &str, what: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .map_err(|_| format!("{what} 需要非负整数，收到 `{value}`"))
}

fn split3(value: &str) -> Result<(u64, i64, i64), String> {
    let mut parts = value.split(',');
    let frame = parts.next().ok_or_else(|| "缺少 frame".to_string())?;
    let x = parts.next().ok_or_else(|| "缺少 x".to_string())?;
    let y = parts.next().ok_or_else(|| "缺少 y".to_string())?;
    if parts.next().is_some() {
        return Err(format!("点击格式为 FRAME,X,Y，收到 `{value}`"));
    }
    Ok((
        to_u64(frame, "frame")?,
        x.parse::<i64>()
            .map_err(|_| format!("x 需要整数，收到 `{x}`"))?,
        y.parse::<i64>()
            .map_err(|_| format!("y 需要整数，收到 `{y}`"))?,
    ))
}

fn split_at(value: &str) -> Result<(u64, PathBuf), String> {
    let (frame, path) = value
        .split_once('=')
        .ok_or_else(|| format!("格式为 FRAME=PATH，收到 `{value}`"))?;
    Ok((to_u64(frame, "frame")?, PathBuf::from(path)))
}

struct Probe {
    frames: u32,
    clicks: Vec<(u64, i64, i64)>,
    shots: Vec<(u64, PathBuf)>,
    jsons: Vec<(u64, PathBuf)>,
    fulls: Vec<(u64, PathBuf)>,
}

impl Probe {
    fn parse(tokens: Vec<String>) -> Result<(PathBuf, Probe), String> {
        let mut args = ArgReader::new(tokens);
        // 先消费带值开关，避免它们的值被当成位置参数。
        let mut clicks = Vec::new();
        while let Some(value) = args.flag("click") {
            if value.is_empty() {
                return Err("--click 需要 FRAME,X,Y".to_string());
            }
            clicks.push(split3(&value)?);
        }
        let mut shots = Vec::new();
        while let Some(value) = args.flag("shot") {
            if value.is_empty() {
                return Err("--shot 需要 FRAME=PATH".to_string());
            }
            shots.push(split_at(&value)?);
        }
        let mut jsons = Vec::new();
        while let Some(value) = args.flag("json") {
            if value.is_empty() {
                return Err("--json 需要 FRAME=PATH".to_string());
            }
            jsons.push(split_at(&value)?);
        }
        let mut fulls = Vec::new();
        while let Some(value) = args.flag("full") {
            if value.is_empty() {
                return Err("--full 需要 FRAME=PATH".to_string());
            }
            fulls.push(split_at(&value)?);
        }
        let root = args.positional(0).ok_or_else(|| {
            "用法: input_probe <project-root> [--frames N] [--click F,X,Y]".to_string()
        })?;
        let frames = match args.flag("frames") {
            Some(value) => to_u64(&value, "frames")? as u32,
            None => 400,
        };
        args.finish().map_err(|error| error.to_string())?;
        Ok((
            PathBuf::from(root),
            Probe {
                frames: frames.max(1),
                clicks,
                shots,
                jsons,
                fulls,
            },
        ))
    }

    fn timeline(&self) -> InputTimeline {
        let mut timeline = InputTimeline::new();
        for &(frame, x, y) in &self.clicks {
            // 光标先于按下帧生效：按钮判定（`@_MOUSE_X/Y`）需要先到位。
            if frame > 0 {
                timeline.set(
                    frame - 1,
                    InputFrame::new(std::iter::empty::<&str>()).with_cursor(x, y),
                );
            }
            timeline.set(frame, InputFrame::new(["MOUSE_L"]).with_cursor(x, y));
            timeline.set(
                frame + 1,
                InputFrame::new(std::iter::empty::<&str>()).with_cursor(x, y),
            );
        }
        timeline
    }
}

fn run() -> Result<(), String> {
    let (root, probe) = Probe::parse(std::env::args().skip(1).collect())?;
    println!(
        "probe frames={} clicks={}",
        probe.frames,
        probe.clicks.len()
    );
    let project = Project::discover(Path::new(&root), None).map_err(|error| error.to_string())?;
    let mut engine = Engine::new(
        project,
        EngineOptions {
            backend: Backend::Soft,
            record_events: false,
            ..EngineOptions::default()
        },
    )
    .map_err(|error| error.to_string())?;
    engine.set_timeline(probe.timeline());

    let mut ticks = 0u32;
    for _ in 0..probe.frames {
        let rendered = engine.tick().map_err(|error| error.to_string())?;
        ticks += 1;
        let frame = rendered.report.frame;
        for (at, path) in &probe.shots {
            if *at == frame {
                let surface = engine
                    .last_surface()
                    .ok_or_else(|| "没有像素输出".to_string())?;
                let bytes =
                    png::png_bytes(surface).map_err(|error| format!("PNG 编码失败: {error}"))?;
                std::fs::write(path, bytes)
                    .map_err(|error| format!("写 {} 失败: {error}", path.display()))?;
                println!("shot frame={frame} -> {}", path.display());
            }
        }
        for (at, path) in &probe.jsons {
            if *at == frame {
                let json = engine
                    .display_json()
                    .ok_or_else(|| "没有 display_json".to_string())?;
                std::fs::write(path, json)
                    .map_err(|error| format!("写 {} 失败: {error}", path.display()))?;
                println!("json frame={frame} -> {}", path.display());
            }
        }
        for (at, path) in &probe.fulls {
            if *at == frame {
                let frame_json = engine
                    .last_frame()
                    .map(yu_gfx::json::to_json)
                    .ok_or_else(|| "没有 full frame".to_string())?;
                std::fs::write(path, frame_json)
                    .map_err(|error| format!("写 {} 失败: {error}", path.display()))?;
                println!("full frame={frame} -> {}", path.display());
            }
        }
        if frame % 60 == 0 {
            println!(
                "frame={frame} layers={} visible={} ops={} skipped={} drawable={} tick={:.2}ms render={:.2}ms",
                rendered.report.layers,
                rendered.report.visible,
                rendered.report.ops,
                rendered.report.skipped,
                rendered.report.drawable,
                rendered.report.tick_ms,
                rendered.report.render_ms
            );
        }
    }
    let report = engine.last_report().clone();
    println!(
        "final ticks={ticks} engine_frame={} frame={} layers={} visible={} ops={} skipped={} drawable={} main_finished={} tick_avg={:.2}ms render_avg={:.2}ms",
        engine.frame(),
        report.frame,
        report.layers,
        report.visible,
        report.ops,
        report.skipped,
        report.drawable,
        engine.main_finished(),
        engine.tick_avg_ms(),
        engine.render_avg_ms()
    );
    for error in engine.script_errors() {
        println!("  script error: {error}");
    }
    let diags = engine.take_diags();
    for diag in diags.iter().take(40) {
        println!("  diag: {diag}");
    }
    if diags.len() > 40 {
        println!("  ... {} more diags", diags.len() - 40);
    }
    for (name, hits) in engine.top_commands(16) {
        println!("  top {name}: {hits}");
    }
    Ok(())
}
