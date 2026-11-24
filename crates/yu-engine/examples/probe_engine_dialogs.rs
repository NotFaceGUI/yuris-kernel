//! 通用探针:引擎级 tick + 自动回答 DIALOG,末帧写 PNG/display JSON。
//!
//! 用法:
//!   `probe_engine_dialogs <project-root> [frames] [--answer N]
//!    [--shot PATH] [--json PATH] [--save-root DIR]`
//!
//! 为什么需要它:headless 默认没有宿主 UI,启动期的更新检查/确认类
//! `DIALOG` 会让主任务永久挂起,画面停在清屏色。本探针按引擎公开的
//! `pending_dialogs` / `answer_dialog` 回答,让后续脚本(标题、正文)真正
//! 跑起来,再落最后一帧取证。回答按钮号是显式参数,不对任何游戏特化。

use std::path::PathBuf;

use yu_engine::engine::{Engine, EngineOptions};
use yu_engine::project::Project;
use yu_engine::Failure;

fn main() -> Result<(), Failure> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let take_flag = |args: &mut Vec<String>, name: &str| -> Option<String> {
        let pos = args.iter().position(|a| a == name)?;
        let value = args.get(pos + 1).cloned().unwrap_or_default();
        args.drain(pos..=pos + 1);
        Some(value)
    };
    let answer: u8 = take_flag(&mut args, "--answer")
        .map(|v| v.parse().expect("--answer 需要按钮序号(1 基)"))
        .unwrap_or(2);
    let shot = take_flag(&mut args, "--shot").map(PathBuf::from);
    let json = take_flag(&mut args, "--json").map(PathBuf::from);
    let save_root = take_flag(&mut args, "--save-root").map(PathBuf::from);
    // `--click FRAME:X:Y` 可重复:在指定逻辑帧把光标移到坐标并单帧按下左键。
    let mut clicks: Vec<(u32, i64, i64)> = Vec::new();
    while let Some(pos) = args.iter().position(|a| a == "--click") {
        let value = args.get(pos + 1).cloned().unwrap_or_default();
        args.drain(pos..=pos + 1);
        let mut parts = value.split(':');
        let frame: u32 = parts
            .next()
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| panic!("--click 需要 FRAME:X:Y,收到 `{value}`"));
        let x: i64 = parts
            .next()
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| panic!("--click 需要 FRAME:X:Y,收到 `{value}`"));
        let y: i64 = parts
            .next()
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| panic!("--click 需要 FRAME:X:Y,收到 `{value}`"));
        clicks.push((frame, x, y));
    }
    // `--move FRAME:X:Y` 可重复:只移动光标(不回按),用于验证 hover。
    let mut moves: Vec<(u32, i64, i64)> = Vec::new();
    while let Some(pos) = args.iter().position(|a| a == "--move") {
        let value = args.get(pos + 1).cloned().unwrap_or_default();
        args.drain(pos..=pos + 1);
        let mut parts = value.split(':');
        let frame: u32 = parts
            .next()
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| panic!("--move 需要 FRAME:X:Y,收到 `{value}`"));
        let x: i64 = parts
            .next()
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| panic!("--move 需要 FRAME:X:Y,收到 `{value}`"));
        let y: i64 = parts
            .next()
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| panic!("--move 需要 FRAME:X:Y,收到 `{value}`"));
        moves.push((frame, x, y));
    }
    // `--shot-at FRAME:PATH` 可重复:在指定帧落 PNG(过渡/警告页取证)。
    let mut shot_at: Vec<(u32, PathBuf)> = Vec::new();
    while let Some(pos) = args.iter().position(|a| a == "--shot-at") {
        let value = args.get(pos + 1).cloned().unwrap_or_default();
        args.drain(pos..=pos + 1);
        let (frame, path) = value
            .split_once(':')
            .unwrap_or_else(|| panic!("--shot-at 需要 FRAME:PATH,收到 `{value}`"));
        shot_at.push((
            frame
                .parse()
                .unwrap_or_else(|_| panic!("--shot-at 帧号非法:`{value}`")),
            PathBuf::from(path),
        ));
    }
    let font_bytes = take_flag(&mut args, "--font")
        .map(|path| {
            std::fs::read(&path)
                .unwrap_or_else(|e| panic!("--font 读取 {path} 失败: {e}"))
        })
        .into_iter()
        .collect::<Vec<_>>();
    let mut args = args.into_iter();
    let root = args.next().expect("project root");
    let frames: u32 = args
        .next()
        .and_then(|v| v.parse().ok())
        .unwrap_or(600);

    let project = Project::discover(std::path::Path::new(&root), None)?;
    let opts = EngineOptions {
        save_root,
        font_bytes,
        // 探针只取画面证据;音频设备在无声/无设备环境会让 tick 阻塞。
        os_sound: false,
        ..EngineOptions::default()
    };
    let mut engine = Engine::new(project, opts)?;
    let mut answered = 0u32;
    for frame in 0..frames {
        for &(click_frame, x, y) in &clicks {
            if click_frame == frame {
                engine.set_cursor(x, y);
                engine.pulse_input("MOUSE_L");
                println!("f{frame} click ({x},{y})");
                use std::io::Write as _;
                let _ = std::io::stdout().flush();
            }
        }
        for &(move_frame, x, y) in &moves {
            if move_frame == frame {
                engine.set_cursor(x, y);
                println!("f{frame} move ({x},{y})");
                use std::io::Write as _;
                let _ = std::io::stdout().flush();
            }
        }
        engine.tick()?;
        let pending: Vec<(u64, u8, String, String)> = engine
            .pending_dialogs()
            .iter()
            .map(|d| {
                (
                    d.id,
                    d.buttons.count(),
                    d.caption_display(),
                    d.text_display(),
                )
            })
            .collect();
        for (id, buttons, caption, text) in pending {
            let choice = answer.clamp(1, buttons.max(1));
            println!(
                "f{frame} dialog#{id} [{}/{}] caption={caption:?} text={:?} -> button {choice}",
                answered + 1,
                "pending",
                text.chars().take(60).collect::<String>()
            );
            engine
                .answer_dialog(id, choice, None)
                .map_err(|d| Failure::runtime(format!("DIALOG#{id}: {}: {}", d.code, d.detail)))?;
            answered += 1;
        }
        if frame % 120 == 0 || frame + 1 == frames {
            println!(
                "f{frame} lifecycle={:?} main_finished={} live_tasks={}",
                engine.lifecycle(),
                engine.main_finished(),
                engine.live_tasks()
            );
            use std::io::Write as _;
            let _ = std::io::stdout().flush();
        }
        for (shot_frame, path) in &shot_at {
            if *shot_frame == frame {
                let bytes = engine.write_png(path)?;
                println!("f{frame} png: {} ({bytes} bytes)", path.display());
                if std::env::var_os("YU_DUMP_LAYERS").is_some() {
                    if let Some(state) = engine.gfx_state() {
                        let snap = state.layer_snapshot();
                        for (id, z, alpha, visible) in snap.iter().filter(|(_, z, _, _)| *z >= 150)
                        {
                            println!("  L z={z} a={alpha} vis={visible} {id}");
                        }
                    }
                }
                if std::env::var_os("YU_TEXT_TRACE").is_some() {
                    if let Some(state) = engine.gfx_state() {
                        let (bytes, _flags, style, _origin) = state.text_state();
                        println!(
                            "  T pending={:?} size=({},{}) name={:?}",
                            String::from_utf8_lossy(bytes),
                            style.size_x,
                            style.size_y,
                            style
                                .name
                                .as_deref()
                                .map(|b| String::from_utf8_lossy(b).into_owned())
                        );
                    }
                    if let Some(frame) = engine.last_frame() {
                        if let Some(layout) = &frame.text_layout {
                            println!("  T layout chars={}", layout.chars);
                        }
                        for draw in frame.text_draws.iter().take(8) {
                            println!(
                                "  T draw x={} y={} set={} bytes={:?} size=({},{})",
                                draw.x,
                                draw.y,
                                draw.set,
                                String::from_utf8_lossy(&draw.bytes),
                                draw.style.size_x,
                                draw.style.size_y
                            );
                        }
                    }
                }
                use std::io::Write as _;
                let _ = std::io::stdout().flush();
            }
        }
    }
    if let Some(path) = shot {
        let bytes = engine.write_png(&path)?;
        println!("png: {} ({bytes} bytes)", path.display());
    }
    println!(
        "textures: decoded={} cached={}",
        engine.textures_decoded(),
        engine.textures_cached()
    );
    if let Some(report) = engine.render_report() {
        println!(
            "render: layers={} visible={} ops={} drawn={} skipped={} missing_assets={}",
            report.layers,
            report.visible_layers,
            report.ops_total,
            report.ops_drawn,
            report.skipped_ops_total,
            report.missing_assets.len()
        );
        for issue in &report.missing_assets {
            println!(
                "  missing {} {}: {}",
                issue.key_display(),
                issue.code,
                issue.detail.chars().take(140).collect::<String>()
            );
        }
    }
    let diags = engine.take_diags();
    let mut asset_codes: std::collections::BTreeMap<String, (usize, String)> =
        Default::default();
    for diag in &diags {
        if diag.code.starts_with("asset.")
            || diag.code.starts_with("gfx.")
            || diag.code.contains("unimplemented")
        {
            let entry = asset_codes
                .entry(diag.code.to_string())
                .or_insert_with(|| (0, diag.detail.clone()));
            entry.0 += 1;
        }
    }
    for (code, (count, sample)) in asset_codes {
        println!("diag {code} x{count}: {}", sample.chars().take(120).collect::<String>());
    }
    if let Some(path) = json {
        if let Some(text) = engine.display_json() {
            std::fs::write(&path, text.as_bytes())
                .map_err(|e| Failure::runtime(format!("写 {}: {e}", path.display())))?;
            println!("json: {}", path.display());
        }
    }
    Ok(())
}
