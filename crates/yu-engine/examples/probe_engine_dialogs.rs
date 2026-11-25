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
    // `--spl-log`:逐帧跟踪 `SPL*` 精灵层状态,只在任一字段变化时打印。
    // 用于标题 build 动画/淡出时序:alpha、xy、size、纹理一变就能定位到帧。
    let spl_log = args.iter().any(|a| a == "--spl-log");
    args.retain(|a| a != "--spl-log");
    // `--no-render`:只跑 VM/gfx 命令流,跳过合成上屏。时间线取证要跑数千帧,
    // 软后端逐帧上屏太慢;图层状态/alpha/尺寸不受渲染影响。
    let no_render = args.iter().any(|a| a == "--no-render");
    args.retain(|a| a != "--no-render");
    // `--wgpu`:headless 走 wgpu 后端(与桌面同一条渲染路径)。
    let wgpu = args.iter().any(|a| a == "--wgpu");
    args.retain(|a| a != "--wgpu");
    // `--flare-trace`:逐帧打印 SPL21/22 光照层新出现的图像加工项(含烘焙前)。
    let flare_trace = args.iter().any(|a| a == "--flare-trace");
    args.retain(|a| a != "--flare-trace");
    // `--screen-at 775,785,...`:在指定帧打印屏幕层清单与图像加工链。
    let screen_at: std::collections::BTreeSet<u32> = take_flag(&mut args, "--screen-at")
        .map(|v| {
            v.split(',')
                .filter_map(|s| s.trim().parse::<u32>().ok())
                .collect()
        })
        .unwrap_or_default();
    // `--cgs-at 1101,...`:打印窗口表、CGS 层及其屏幕快照的成员层。
    let cgs_at: std::collections::BTreeSet<u32> = take_flag(&mut args, "--cgs-at")
        .map(|v| {
            v.split(',')
                .filter_map(|s| s.trim().parse::<u32>().ok())
                .collect()
        })
        .unwrap_or_default();
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
        render: !no_render,
        backend: if wgpu {
            yu_engine::engine::Backend::Wgpu
        } else {
            yu_engine::engine::Backend::Soft
        },
        ..EngineOptions::default()
    };
    let mut engine = Engine::new(project, opts)?;
    let mut answered = 0u32;
    let mut spl_seen: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut spl_names: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut flare_seen: std::collections::BTreeSet<u64> = std::collections::BTreeSet::new();
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
        if no_render {
            // 只给 `--shot-at` 指定的帧打开渲染;其余帧复用上一呈现面。
            engine.set_render_enabled(shot_at.iter().any(|(shot_frame, _)| *shot_frame == frame));
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
        if cgs_at.contains(&frame) {
            use std::io::Write as _;
            if let Some(state) = engine.gfx_state() {
                let f = state.frame();
                println!("WINDOWS f{frame} count={}", f.windows.len());
                for w in &f.windows {
                    println!(
                        "  W no={} set={:?} vis={} a={} xy=({},{})",
                        w.number,
                        w.layer_set.as_ref().map(|l| l.display()),
                        w.visible,
                        w.alpha,
                        w.x,
                        w.y
                    );
                }
                for layer in &f.layers {
                    let name = layer.id.display();
                    if !name.contains("CGS") {
                        continue;
                    }
                    println!(
                        "  CGSL {name} z={} a={} vis={} tex={:?}",
                        layer.z, layer.alpha, layer.visible, layer.texture
                    );
                    if let yu_gfx::TextureRef::Snapshot(id) = &layer.texture {
                        if let Some(snap) = f.screen_snapshots.get(id) {
                            println!(
                                "    SNAP id={} frame={} layers={} passes={} texts={}",
                                id.0,
                                snap.frame,
                                snap.layer_ids.len(),
                                snap.passes.len(),
                                snap.text_draws.len()
                            );
                            for lid in &snap.layer_ids {
                                println!("      SL-{}", lid.display());
                            }
                        } else {
                            println!("    SNAP id={} (not in frame table)", id.0);
                        }
                    }
                }
            }
            let _ = std::io::stdout().flush();
        }
        if screen_at.contains(&frame) {
            use std::io::Write as _;
            if let Some(summary) = engine.frame_op_summary(0) {
                println!(
                    "SCREEN f{frame} passes={} screen_passes={} texture_passes={} \
                     layer_ops={} image_ops={} copy_ops={}",
                    summary.screen_passes + summary.offscreen_passes + summary.texture_passes,
                    summary.screen_passes,
                    summary.texture_passes,
                    summary.layer_ops,
                    summary.image_ops,
                    summary.copy_ops
                );
                for row in &summary.screen_layers {
                    println!("  SL {row}");
                }
                for row in &summary.image_chain {
                    println!("  IM {row}");
                }
            }
            // 烘焙/旧加工项不会出现在 `image_chain`(只列已发射项);这里把
            // 图像日志里所有命中 SPL21/22 的项原样列出(含 SET/区域/状态),
            // 用于定位光照层表面是怎么被写成「白色 RGB + alpha 形状」的。
            if let Some(state) = engine.gfx_state() {
                for op in state.images() {
                    let src = op.source.display();
                    let tgt = op
                        .target
                        .as_ref()
                        .map(|t| t.display())
                        .unwrap_or_else(|| "(in-place)".into());
                    if !(src.contains("SPL21") || tgt.contains("SPL21")) {
                        continue;
                    }
                    println!(
                        "  IALL seq={} {}->{} action={} set={:?} set2={:?} sx={:?} sy={:?} region=({:?},{:?},{:?},{:?}) status={:?}",
                        op.seq,
                        src,
                        tgt,
                        op.action.name(),
                        op.set.as_ref().and_then(|v| v.as_int()),
                        op.set2.as_ref().and_then(|v| v.as_int()),
                        op.sx,
                        op.sy,
                        op.x,
                        op.y,
                        op.x2,
                        op.y2,
                        op.status
                    );
                }
            }
            let _ = std::io::stdout().flush();
        }
        if flare_trace {
            use std::io::Write as _;
            if let Some(state) = engine.gfx_state() {
                for op in state.images() {
                    if flare_seen.contains(&op.seq) {
                        continue;
                    }
                    let src = op.source.display();
                    let tgt = op
                        .target
                        .as_ref()
                        .map(|t| t.display())
                        .unwrap_or_else(|| "(in-place)".into());
                    if !(src.contains("SPL21") || src.contains("SPL22") || tgt.contains("SPL21") || tgt.contains("SPL22")) {
                        continue;
                    }
                    flare_seen.insert(op.seq);
                    println!(
                        "FLARE f{frame} seq={} {}->{} {} set={:?} set2={:?} sx={:?} sy={:?} region=({:?},{:?},{:?},{:?}) status={:?}",
                        op.seq,
                        src,
                        tgt,
                        op.action.name(),
                        op.set.as_ref().and_then(|v| v.as_int()),
                        op.set2.as_ref().and_then(|v| v.as_int()),
                        op.sx,
                        op.sy,
                        op.x,
                        op.y,
                        op.x2,
                        op.y2,
                        op.status
                    );
                }
            }
            let _ = std::io::stdout().flush();
        }
        if spl_log {
            use std::io::Write as _;
            if let Some(summary) = engine.frame_op_summary(0) {
                let mut now_names: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
                for row in &summary.sprite_layers {
                    if !row.contains(":/W0/SPL") {
                        continue;
                    }
                    let name = row.split(" a=").next().unwrap_or(row);
                    now_names.insert(name.to_string());
                    if spl_seen.get(name) != Some(row) {
                        println!("SPL f{frame} {row}");
                        spl_seen.insert(name.to_string(), row.clone());
                        let _ = std::io::stdout().flush();
                    }
                }
                if now_names != spl_names {
                    let gone: Vec<&String> = spl_names.difference(&now_names).collect();
                    let added: Vec<&String> = now_names.difference(&spl_names).collect();
                    if !gone.is_empty() || !added.is_empty() {
                        println!(
                            "SPLSET f{frame} gone={:?} added={:?}",
                            gone.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
                            added.iter().map(|s| s.as_str()).collect::<Vec<_>>()
                        );
                        let _ = std::io::stdout().flush();
                    }
                    spl_names = now_names;
                }
            }
            // CGS 淡入淡出层与 `_tmptr` 遮罩:z 决定它们是否盖住 SPL 精灵。
            if let Some(state) = engine.gfx_state() {
                for (id, z, alpha, visible, x, y) in state.layer_snapshot() {
                    if !(id.contains("CGS") || id.contains("_tmptr")) {
                        continue;
                    }
                    let row = format!("{id} z={z} a={alpha} vis={visible} xy=({x:.1},{y:.1})");
                    let key = format!("CGS:{id}");
                    if spl_seen.get(&key) != Some(&row) {
                        println!("CGS f{frame} {row}");
                        spl_seen.insert(key, row);
                        let _ = std::io::stdout().flush();
                    }
                }
            }
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
                        for (id, z, alpha, visible, x, y) in
                            snap.iter().filter(|(_, z, _, _, _, _)| *z >= 150)
                        {
                            println!("  L z={z} a={alpha} vis={visible} xy=({x:.2},{y:.2}) {id}");
                        }
                        for (name, id) in [("@5398", 5398u16), ("@5399", 5399), ("@5400", 5400)] {
                            let mut vals = Vec::new();
                            for idx in 0..8i64 {
                                if let Some(Ok(v)) = engine.read_variable(
                                    yu_rt::value::VarRef::new(yu_rt::value::VarSpace::At, id),
                                    &[idx],
                                ) {
                                    vals.push(format!("{v:?}"));
                                }
                            }
                            println!("  V {name}[0..8] {vals:?}");
                        }
                        for op in state.images().iter().rev().take(12) {
                            println!(
                                "  I seq={} action={} src={} target={:?} x={:?} y={:?} sx={:?} sy={:?}",
                                op.seq,
                                op.action.name(),
                                op.source.display(),
                                op.target.as_ref().map(|t| t.display()),
                                op.x,
                                op.y,
                                op.sx,
                                op.sy
                            );
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
