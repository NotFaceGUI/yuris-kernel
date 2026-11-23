//! 临时探针:555 正文任务状态机逐帧取证(只读 Engine 公开 API)。
//!
//! 用法:`probe_text_click <project-root> [frames] [click_frame:x,y ...]`;
//! 存档根可用 `YURIS_SAVE_ROOT` 覆盖,缺省 `<cwd>/target/tmp/probe-text-click/save`。
use std::path::Path;

use yu_engine::engine::{Backend, Engine, EngineOptions};
use yu_engine::project::Project;
use yu_rt::value::{Value, VarRef, VarSpace};

fn ival(engine: &Engine, id: u16) -> i64 {
    engine.debug_var_int(id).unwrap_or(-9999)
}

fn arr(engine: &Engine, id: u16, ix: &[i64]) -> i64 {
    engine
        .read_variable(VarRef::new(VarSpace::At, id), ix)
        .and_then(|r| r.ok())
        .and_then(|v| v.as_int())
        .unwrap_or(-9999)
}

fn sval(engine: &Engine, id: u16) -> String {
    match engine.debug_var_value(id) {
        Some(Value::Str(bytes)) => {
            let decoded = yu_format::Codec::Gbk
                .decode(&bytes, b"probe")
                .unwrap_or_else(|_| String::from_utf8_lossy(&bytes).into_owned());
            let n = decoded.chars().count();
            if n > 32 {
                format!("{}…({n})", decoded.chars().take(32).collect::<String>())
            } else {
                decoded
            }
        }
        other => format!("{other:?}"),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = args.first().expect("project root");
    let frames: u32 = args.get(1).and_then(|v| v.parse().ok()).unwrap_or(1600);
    let mut clicks: Vec<(u32, i64, i64)> = Vec::new();
    for raw in args.iter().skip(2) {
        let (frame, coords) = raw.split_once(':').expect("click_frame:x,y");
        let (x, y) = coords.split_once(',').expect("click_frame:x,y");
        clicks.push((frame.parse().unwrap(), x.parse().unwrap(), y.parse().unwrap()));
    }

    let project = Project::discover(Path::new(&root), None).expect("project");
    let mut engine = Engine::new(
        project,
        EngineOptions {
            backend: Backend::Wgpu,
            entry: "SYSTEM_START".into(),
            save_root: Some(
                std::env::var_os("YURIS_SAVE_ROOT")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| {
                        std::path::PathBuf::from("target/tmp/probe-text-click/save")
                    }),
            ),
            ..EngineOptions::default()
        },
    )
    .expect("engine");

    let mut last = (i64::MIN, i64::MIN);
    let mut cursor = None;
    for frame in 1..=frames {
        if let Some((click_frame, x, y)) = clicks.iter().find(|(f, _, _)| *f == frame) {
            cursor = Some((*x, *y));
            let _ = click_frame;
        }
        if let Some((x, y)) = cursor {
            engine.set_cursor(x, y);
        }
        // 与 case 的 InputTimeline 同型:按下沿后保持若干帧,再松开。
        let pressed = clicks
            .iter()
            .any(|(f, _, _)| frame >= *f && frame < *f + 4);
        engine.set_input("MOUSE_L", pressed);
        if let Err(error) = engine.tick() {
            eprintln!("ERR f{frame}: {error:?}");
            break;
        }
        let Some(cf) = engine.last_frame() else { continue };
        if [600u32, 650, 680, 700, 704, 710, 760, 960, 1200, 1500].contains(&frame) {
            println!(
                "DBG f{frame} engine_frame={} viewport={}x{} layers={} passes={} live={} main_finished={} pos={:?} dialogs={}",
                cf.frame,
                cf.viewport.width,
                cf.viewport.height,
                cf.layers.len(),
                cf.passes.len(),
                engine.live_tasks(),
                engine.main_finished(),
                engine.current_position(),
                engine.pending_dialogs().len(),
            );
        }
        let draws = cf.text_draws.len();
        let state = ival(&engine, 6205);
        let cursor = ival(&engine, 6125);
        let len = ival(&engine, 6124);
        let key = (state, cursor);
        let changing = key != last;
        // 每个状态迁移都打;稳定期每 60 帧一行。
        if changing || frame % 60 == 0 {
            println!(
                "f{frame} state={state} cur={cursor}/{len} draws={draws} texts={} \
                 skip={} in1150={} 1151={} 1152={} 1153={} 1154={} \
                 line_time={}/{} voice={} done={} gap={} \
                 t81_1={} t81_2={} t81_4={} | name={:?} body={:?}",
                cf.text.len(),
                ival(&engine, 1030),
                ival(&engine, 1150),
                ival(&engine, 1151),
                ival(&engine, 1152),
                ival(&engine, 1153),
                ival(&engine, 1154),
                ival(&engine, 6238),
                ival(&engine, 6240),
                ival(&engine, 6239),
                ival(&engine, 6237),
                ival(&engine, 6146),
                arr(&engine, 1173, &[81, 1]),
                arr(&engine, 1173, &[81, 2]),
                arr(&engine, 1173, &[81, 4]),
                sval(&engine, 6120),
                sval(&engine, 6123),
            );
            last = key;
        }
        if draws > 0 && (changing || frame % 300 == 0) {
            for (i, d) in cf.text_draws.iter().take(3).enumerate() {
                let decoded = yu_format::Codec::Gbk
                    .decode(&d.bytes, b"draw")
                    .unwrap_or_else(|_| String::from_utf8_lossy(&d.bytes).into_owned());
                println!(
                    "   draw[{i}] target={} set={} x={} y={} text={decoded:?}",
                    d.target.display(),
                    d.set,
                    d.x,
                    d.y
                );
            }
        }
    }
    println!("done frames={frames}");
}
