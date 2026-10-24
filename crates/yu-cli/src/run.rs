//! `yu-cli run`:桌面 headless 入口(与 `first_frame` 示例同一条链)。

use std::path::{Path, PathBuf};

use crate::args::ArgReader;
use crate::engine::{Backend, Engine, EngineOptions};
use crate::output;
use crate::project::Project;
use crate::{CliResult, Failure};

pub fn from_args(tokens: Vec<String>) -> CliResult<()> {
    let mut args = ArgReader::new(tokens);
    let root = args
        .positional(0)
        .ok_or_else(|| Failure::usage("用法: yu-cli run <project-root> [...]"))?;
    let backend = match args.flag("backend") {
        Some(value) => Backend::parse(&value)?,
        None => Backend::Soft,
    };
    let entry = args
        .flag("entry")
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "SYSTEM_START".to_string());
    let frames = match args.flag("frames") {
        Some(value) => Some(parse_u32("frames", &value)?),
        None => None,
    };
    let until_visible = args.switch("until-visible");
    let until_frame =
        match args.flag("until-frame") {
            Some(value) => Some(value.parse::<u64>().map_err(|_| {
                Failure::usage(format!("--until-frame 需要非负整数,收到 `{value}`"))
            })?),
            None => None,
        };
    let data = args.flag("data").map(PathBuf::from);
    let shot = args.flag("shot").map(PathBuf::from);
    let json_path = args.flag("json").map(PathBuf::from);
    let record_events = match args.flag("record-events") {
        Some(value) => ArgReader::on_off("record-events", &value)?,
        None => false,
    };
    // 存档根重定向(P4/B1):给显式目录 = 可写(存档真落盘);
    // 缺省 = 工程根 save/(只读保护,SAVE 刷盘被拒)。
    let save_root = args.flag("save-root").map(PathBuf::from);
    // debug 语料对拍口径(DIV-L-001):官方 debug 构建 `_DEBUGMODE=1`,
    // 分支语义(如 script 20 的 debug 任务)依赖该注入。
    let debug_mode = match args.flag("debug-mode") {
        Some(value) => ArgReader::on_off("debug-mode", &value)?,
        None => false,
    };
    args.finish()?;

    let project = Project::discover(Path::new(&root), data.as_deref())?;
    let opts = EngineOptions {
        backend,
        entry,
        record_events,
        debug_mode,
        save_root,
        ..EngineOptions::default()
    };
    let mut engine = Engine::new(project, opts)?;
    let limit = frames
        .unwrap_or(if until_frame.is_some() { 2000 } else { 1 })
        .max(1);
    let mut ticks = 0u32;
    let mut stop_reason = "frame limit";
    for _ in 0..limit {
        let rendered = engine.tick()?;
        ticks += 1;
        if let Some(target) = until_frame {
            if rendered.report.frame >= target {
                stop_reason = "until-frame";
                break;
            }
        }
        if until_visible && rendered.report.drawable {
            stop_reason = "first drawable frame";
            break;
        }
    }
    if until_visible && !engine.last_report().drawable {
        output::line(format!("warning: {limit} 帧内没有可绘制屏幕;渲染最后一帧"));
    }

    let report = engine.last_report().clone();
    output::line(format!(
        "engine: backend={} entry={} record_events={} boot_ms={:.2}",
        engine.backend(),
        engine.entry(),
        if record_events { "on" } else { "off" },
        engine.boot_ms()
    ));
    output::line(format!(
        "frames ticked: {ticks} (stop: {stop_reason}, limit {limit})"
    ));
    output::line(format!(
        "frame: {} viewport: {}x{}",
        report.frame,
        engine.viewport().width,
        engine.viewport().height
    ));
    output::line(format!(
        "layers: {} (visible {}), passes: {}",
        report.layers, report.visible, report.passes
    ));
    output::line(format!(
        "ops: {} total, {} drawn, {} skipped",
        report.ops, report.executed, report.skipped
    ));
    if let Some(surface) = engine.last_surface() {
        output::line(format!(
            "rgba sha256: {} ({} bytes)",
            crate::sha::sha256_hex(&surface.rgba),
            surface.rgba.len()
        ));
    }
    output::line(format!(
        "timing: tick={:.2} ms render={:.2} ms total={:.2} ms (avg tick {:.2} / render {:.2})",
        report.tick_ms,
        report.render_ms,
        report.ms,
        engine.tick_avg_ms(),
        engine.render_avg_ms()
    ));
    if let Some(report) = engine.render_report() {
        for skip in &report.skipped {
            output::line(format!(
                "  skip {:<10} x{:<4} {:<44} {}",
                skip.kind, skip.count, skip.reason, skip.target
            ));
        }
        output::line(format!(
            "missing assets: {}; text runs (not rasterized): {}",
            report.missing_assets.len(),
            report.skipped_text
        ));
    }
    if let Some(reconciled) = engine.wgpu_reconciled() {
        output::line(format!(
            "wgpu reconcile: {} (layers/visible/passes/offscreen/ops/skipped 与 soft 逐项一致)",
            if reconciled { "OK" } else { "PENDING" }
        ));
    }
    output::line(format!(
        "textures: decoded={} cache_entries={}",
        engine.textures_decoded(),
        engine.textures_cached()
    ));
    output::line(format!(
        "events: drained={} dropped={}",
        engine.events_drained(),
        engine.events_dropped()
    ));

    if let Some(path) = shot.as_deref() {
        let bytes = engine.write_png(path)?;
        output::line(format!("png: {} ({} bytes)", path.display(), bytes));
    }
    if let Some(path) = json_path.as_deref() {
        let json = engine
            .display_json()
            .ok_or_else(|| Failure::runtime("没有可写的 display JSON"))?;
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    Failure::runtime(format!("创建 {} 失败: {e}", parent.display()))
                })?;
            }
        }
        std::fs::write(path, json.as_bytes())
            .map_err(|e| Failure::runtime(format!("写 {} 失败: {e}", path.display())))?;
        output::line(format!(
            "display json: {} ({} bytes)",
            path.display(),
            json.len()
        ));
    }
    Ok(())
}

pub fn parse_u32(name: &str, value: &str) -> CliResult<u32> {
    value
        .parse::<u32>()
        .map_err(|_| Failure::usage(format!("--{name} 需要非负整数,收到 `{value}`")))
}
