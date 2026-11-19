//! 临时探针:经完整 Engine(装 GraphicsHost/MENU 域)注入鼠标点击,
//! 捕获 DIALOG 呈现瞬间与诊断。
//! 用法:`probe_dialog_engine <project-root> [frames]`
//! 环境:CLICK_EVERY / CLICK_X / CLICK_Y / CLICK_X2 / CLICK_Y2 / CLICK2_FROM
use std::path::Path;

use yu_engine::engine::{Backend, Engine, EngineOptions};
use yu_engine::project::Project;

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let frames: u32 = std::env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(1200);
    let env_i = |k: &str, d: i64| -> i64 {
        std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d)
    };
    let click_every = env_i("CLICK_EVERY", 60) as u32;
    let x1 = env_i("CLICK_X", 100);
    let y1 = env_i("CLICK_Y", 40);
    let x2 = env_i("CLICK_X2", x1);
    let y2 = env_i("CLICK_Y2", y1);
    let switch_from = env_i("CLICK2_FROM", i64::MAX) as u32;

    let project = Project::discover(Path::new(&root), None).expect("project");
    let mut engine = Engine::new(
        project,
        EngineOptions {
            backend: Backend::Wgpu,
            entry: "SYSTEM_START".into(),
            record_events: false,
            ..EngineOptions::default()
        },
    )
    .expect("engine");

    let mut seen_dialogs = std::collections::BTreeSet::new();
    let mut all_diags: Vec<String> = Vec::new();
    let mut last_pos = String::new();
    for frame in 1..=frames {
        let (x, y) = if frame >= switch_from { (x2, y2) } else { (x1, y1) };
        engine.set_cursor(x, y);
        // 点击:偶数帧按下,奇数帧松开(每 click_every 一次)。
        let press = click_every > 0 && frame % click_every == 0;
        engine.set_input("MOUSE_L", press);
        let _ = engine.tick().expect("tick");
        for d in engine.take_diags() {
            all_diags.push(format!("f{frame} {} {:?}", d.code, d.detail));
        }
        if frame % 60 == 0 {
            let pos = engine.current_position();
            let p = format!("{pos:?}");
            if p != last_pos {
                println!(
                    "f{frame} pos={p} text={:?}",
                    engine
                        .pending_dialogs()
                        .iter()
                        .map(|d| String::from_utf8_lossy(&d.text).into_owned())
                        .collect::<Vec<_>>()
                );
                last_pos = p;
            }
        }
        for dialog in engine.pending_dialogs() {
            if seen_dialogs.insert(dialog.id) {
                println!(
                    "=== DIALOG#{} @f{frame} buttons={:?} text={:?}",
                    dialog.id,
                    dialog.buttons,
                    String::from_utf8_lossy(&dialog.text)
                );
                if let Some((bytes, flags)) = engine.debug_text_state() {
                    println!(
                        "TEXT({}) cp932={:?} hex={:02x?}",
                        bytes.len(),
                        yu_format::Codec::Cp932.decode(&bytes, b"text"),
                        bytes
                    );
                    println!(
                        "FLAGS({}) ascii={:?} hex={:02x?}",
                        flags.len(),
                        String::from_utf8_lossy(&flags),
                        flags
                    );
                }
                println!(
                    "VARS 0f6d={:?} 0f6e={:?} 0f6f={:?} 0f70={:?} 0eed={:?} 0eee={:?} 0eef={:?} 0ef0={:?} 0ef1={:?}",
                    engine.debug_var_value(0x0f6d),
                    engine.debug_var_int(0x0f6e),
                    engine.debug_var_int(0x0f6f),
                    engine.debug_var_int(0x0f70),
                    engine.debug_var_int(0x0eed),
                    engine.debug_var_int(0x0eee),
                    engine.debug_var_int(0x0eef),
                    engine.debug_var_int(0x0ef0),
                    engine.debug_var_int(0x0ef1),
                );
                for d in &all_diags {
                    println!("diag {d}");
                }
                println!("--- end dialog");
            }
        }
        if !engine.pending_dialogs().is_empty() {
            // 自动按 OK,让流程继续。
            let ids: Vec<u64> = engine.pending_dialogs().iter().map(|d| d.id).collect();
            for id in ids {
                let _ = engine.answer_dialog(id, 1, None);
            }
        }
        if (640..=735).contains(&frame) {
            if let Some((bytes, flags)) = engine.debug_text_state() {
                println!(
                    "f{frame} TEXT({})={:?} FLAGS({})={:?} f6d={:?} f6e={:?} f6f={:?} f70={:?} e2={:?} st={:?}",
                    bytes.len(),
                    yu_format::Codec::Cp932.decode(&bytes, b"text"),
                    flags.len(),
                    String::from_utf8_lossy(&flags),
                    engine.debug_var_value(0x0f6d),
                    engine.debug_var_int(0x0f6e),
                    engine.debug_var_int(0x0f6f),
                    engine.debug_var_int(0x0f70),
                    engine.debug_var_int(0x0ee2),
                    engine.debug_var_int(0x0f29),
                );
            }
        }
    }
    println!("done frames={frames}");
}
