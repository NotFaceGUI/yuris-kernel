//! `yu-cli run`:跑工程 —— 缺省 headless;`--mode desktop` 开原生窗口。

use std::path::{Path, PathBuf};

use crate::args::ArgReader;
use crate::engine::{Backend, Engine, EngineOptions};
use crate::output;
use crate::project::Project;
use crate::{CliResult, Failure};

pub fn from_args(tokens: Vec<String>) -> CliResult<()> {
    // 模式先行(2026-10-25 架构 v2,H3):--mode desktop 时整条命令行交给
    // 桌面宿主解析。参数按模式解耦:桌面旗标在 headless 下会被
    // `finish()` 显式报「未知参数」,反之亦然 —— 写错边不会静默生效。
    let mut probe = ArgReader::new(tokens.clone());
    if let Some(mode) = probe.flag("mode") {
        match mode.as_str() {
            "headless" | "" => {}
            "desktop" => {
                #[cfg(feature = "desktop")]
                {
                    return crate::desktop::from_args(tokens);
                }
                #[cfg(not(feature = "desktop"))]
                {
                    return Err(Failure::usage(
                        "本构建未启用 desktop feature(--no-default-features);\
                         桌面窗口请用默认 feature 构建",
                    ));
                }
            }
            other => {
                return Err(Failure::usage(format!(
                    "--mode 只接受 headless|desktop,收到 `{other}`"
                )))
            }
        }
    }
    // headless 路径:probe 已确认 `--mode`(显式 headless 或空值),这里
    // 在正式解析器里同样消费掉,避免 `finish()` 把它当未知旗标报错。
    let mut args = ArgReader::new(tokens);
    let _ = args.flag("mode");
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
    // 强制字体(诊断/换字体):忽略脚本的 NAME/NO/BNO。
    let force_font = args.flag("font").filter(|value| !value.trim().is_empty());
    // 强制存档块压缩层,覆盖自动判定(盘上既有存档 → 引擎代号 → zlib)。
    // 只影响**写盘**;读侧仍按实际流分流。
    let save_codec = match args.flag("save-codec") {
        Some(value) => Some(match value.trim().to_ascii_lowercase().as_str() {
            "zlib" => yu_engine::engine::BlockCodec::Zlib,
            "snappy" => yu_engine::engine::BlockCodec::Snappy,
            other => {
                return Err(Failure::usage(format!(
                    "--save-codec 只接受 zlib|snappy,收到 `{other}`"
                )))
            }
        }),
        None => None,
    };
    // 显式脚本密钥(换钥作品的兜底;缺省 = 启动时自动解析)。
    let script_key = match args.flag("script-key") {
        Some(value) => Some(parse_script_key(&value)?),
        None => None,
    };
    // Q1 日志收口:`--log-file` 落 JSONL(内核 Diag + 门面行,每轮截断);
    // `--diag-summary off` 关闭末尾诊断汇总(用例流水线不想看时用)。
    let log_file = args.flag("log-file").map(PathBuf::from);
    let diag_summary = match args.flag("diag-summary") {
        Some(value) => crate::logging::parse_on_off("diag-summary", &value)?,
        None => true,
    };
    args.finish()?;

    // 统一控制台(Q1):headless 聚合不即印,末尾汇总;JSONL 可选。
    let jsonl = match log_file.as_deref() {
        Some(path) => Some(crate::logging::open_jsonl_truncating(path)?),
        None => None,
    };
    if let Some(file) = &jsonl {
        crate::logging::write_color_notice(file);
    }
    let mut console = yu_log::console::Console::stderr_aggregate(crate::logging::min_level(), jsonl);

    let project = Project::discover(Path::new(&root), data.as_deref())?;
    // 数据入口决策留痕(stdout 报告行扩展;排查「行为为何不同」的第一事实)。
    let dataset = project.describe();
    let opts = EngineOptions {
        backend,
        entry,
        record_events,
        debug_mode,
        save_root,
        force_font,
        script_key,
        save_codec,
        // 真实运行:`@_TIME` 注入系统本地时间(存档菜单的现实日期)。
        local_time: crate::clock::local_now(),
        ..EngineOptions::default()
    };
    let mut engine = Engine::new(project, opts)?;
    // 字体枚举是懒加载(默认 warm_text=false),首帧前才预热的话启动摘要就
    // 是空的。这里显式预热一次,让 `--log-level info` 能看到字体事实。
    if engine.font_summary().is_none() {
        engine.warm_text();
    }
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
        // 降级必须留痕(诊断/日志走 stderr,频道化前缀)。
        output::error(format!(
            "[run] {limit} 帧内没有可绘制屏幕;渲染最后一帧"
        ));
    }

    let report = engine.last_report().clone();
    output::line(format!(
        "engine: backend={} entry={} record_events={} debug_mode={} data={} boot_ms={:.2}",
        engine.backend(),
        engine.entry(),
        if record_events { "on" } else { "off" },
        if debug_mode { "on" } else { "off" },
        dataset.summary(),
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

    // Q1 R2:末尾诊断汇总(挂号行带脚本 pc;频次降序;--diag-summary off 可关)。
    if diag_summary {
        crate::logging::print_diag_summary(&mut console, &mut engine);
    }
    Ok(())
}

pub fn parse_u32(name: &str, value: &str) -> CliResult<u32> {
    value
        .parse::<u32>()
        .map_err(|_| Failure::usage(format!("--{name} 需要非负整数,收到 `{value}`")))
}

/// `--script-key`:8 位十六进制(允许空格/下划线/冒号分隔与 `0x` 前缀)。
pub fn parse_script_key(value: &str) -> CliResult<[u8; 4]> {
    let cleaned: String = value
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '_' && *c != ':')
        .collect();
    let hex = cleaned
        .strip_prefix("0x")
        .or_else(|| cleaned.strip_prefix("0X"))
        .unwrap_or(&cleaned);
    if hex.len() != 8 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Failure::usage(format!(
            "--script-key 需要 8 位十六进制(4 字节),收到 `{value}`"
        )));
    }
    let mut key = [0u8; 4];
    for (i, slot) in key.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .map_err(|_| Failure::usage(format!("--script-key 解析失败:`{value}`")))?;
    }
    Ok(key)
}
