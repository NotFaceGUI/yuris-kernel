//! 临时探针:555 正文任务状态机逐帧取证。
//!
//! 用法:`probe_text_state <project-root> [frames] [click_frame:x,y ...]`
//! 存档根可用 `YURIS_SAVE_ROOT` 覆盖;缺省写 `target/tmp/probe-text-state/save`。
//!
//! 只读 `Engine` 公开 API:脚本变量、`CompositorFrame.text_draws`、
//! `GfxState::text_state`。用于定位「正文自动推进 / 文本框不出现」时,
//! 文本任务状态(@6205)、字符游标(@6125)与绘制项数量之间的时序关系。
use std::path::{Path, PathBuf};

use yu_engine::engine::{Backend, Engine, EngineOptions};
use yu_engine::project::Project;
use yu_rt::value::Value;

fn read_int(engine: &Engine, id: u16) -> i64 {
    engine.debug_var_int(id).unwrap_or(-9999)
}

fn read_str(engine: &Engine, id: u16) -> String {
    match engine.debug_var_value(id) {
        Some(Value::Str(bytes)) => {
            let lossy = String::from_utf8_lossy(&bytes);
            let decoded = yu_format::Codec::Gbk
                .decode(&bytes, b"probe")
                .unwrap_or(lossy.into_owned());
            let n = decoded.chars().count();
            if n > 40 {
                format!("{}…({n})", decoded.chars().take(40).collect::<String>())
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
        clicks.push((
            frame.parse().expect("click frame"),
            x.parse().expect("click x"),
            y.parse().expect("click y"),
        ));
    }

    let save_root = std::env::var_os("YURIS_SAVE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/tmp/probe-text-state/save"));
    let project = Project::discover(Path::new(&root), None).expect("project");
    let mut engine = Engine::new(
        project,
        EngineOptions {
            backend: Backend::Wgpu,
            entry: "SYSTEM_START".into(),
            save_root: Some(save_root),
            ..EngineOptions::default()
        },
    )
    .expect("engine");

    let mut last = (i64::MIN, i64::MIN, usize::MAX, usize::MAX);
    for frame in 1..=frames {
        if let Some((_, x, y)) = clicks.iter().find(|(f, _, _)| *f == frame) {
            engine.set_cursor(*x, *y);
            engine.pulse_input("MOUSE_L");
        }
        if let Err(error) = engine.tick() {
            eprintln!("ERR f{frame}: {error:?}");
            break;
        }
        let Some(cf) = engine.last_frame() else { continue };
        let draws = cf.text_draws.len();
        let texts = cf.text.len();
        let layout = cf.text_layout.as_ref().map(|l| l.chars).unwrap_or(0);
        let state = read_int(&engine, 6205);
        let cursor = read_int(&engine, 6125);
        let len = read_int(&engine, 6124);
        let (buf_len, flags_len) = engine
            .gfx_state()
            .map(|g| {
                let (b, fl, _, _) = g.text_state();
                (b.len(), fl.map(<[u8]>::len))
            })
            .unwrap_or((0, None));
        let key = (state, cursor, draws, texts);
        let changing = key != last;
        if changing || frame % 60 == 0 {
            println!(
                "f{frame} state={state} cur={cursor}/{len} draws={draws} texts={texts} layout={layout} buf={buf_len} flags={flags_len:?} skip={} in1150..={},{},{},{} auto={} | name={:?} body={:?}",
                read_int(&engine, 1030),
                read_int(&engine, 1150),
                read_int(&engine, 1151),
                read_int(&engine, 1152),
                read_int(&engine, 1153),
                read_int(&engine, 1154),
                read_str(&engine, 6120),
                read_str(&engine, 6123),
            );
            last = key;
        }
        if draws > 0 && (changing || frame % 300 == 0) {
            for (i, d) in cf.text_draws.iter().take(4).enumerate() {
                let text = String::from_utf8_lossy(&d.bytes);
                let decoded = yu_format::Codec::Gbk
                    .decode(&d.bytes, b"draw")
                    .unwrap_or_else(|_| text.into_owned());
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
