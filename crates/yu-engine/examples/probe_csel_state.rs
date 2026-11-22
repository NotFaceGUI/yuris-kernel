//! 取证探针:引擎装配下跑到 CSEL 等待点,打印 BT 槽位与输入状态。
//! 用法:`probe_csel_state <project-root> [ticks]`
use std::path::Path;

use yu_engine::engine::{Backend, Engine, EngineOptions};
use yu_engine::patch::PatchSelection;
use yu_engine::project::Project;
use yu_rt::value::{VarRef, VarSpace};

fn show(engine: &Engine, space: VarSpace, id: u16, indices: &[i64]) {
    let val = engine.read_variable(VarRef::new(space, id), indices);
    println!("{:02x}{id:04x}[{indices:?}] = {val:?}", space.prefix());
}

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let ticks: u32 = std::env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(1500);
    let project = Project::discover(Path::new(&root), None).expect("project");
    let patch = std::env::var("PROBE_VM_PATCH")
        .ok()
        .and_then(|v| PatchSelection::parse(&v).ok())
        .unwrap_or_else(|| PatchSelection::parse("big5").expect("big5"));
    let mut engine = Engine::new(
        project,
        EngineOptions {
            backend: Backend::Soft,
            entry: std::env::var("PROBE_ENTRY").unwrap_or_else(|_| "SYSTEM_START".into()),
            record_events: false,
            patch,
            ..EngineOptions::default()
        },
    )
    .expect("engine");
    for f in 0..ticks {
        let _ = engine.tick().expect("tick");
        if std::env::var_os("PROBE_SPRITE_TRACE").is_some() && f % 20 == 0 {
            let rd = |id: u16| {
                engine
                    .read_variable(VarRef::new(VarSpace::At, id), &[])
                    .and_then(|r| r.ok())
                    .and_then(|v| v.as_int())
            };
            eprintln!(
                "[sprite] f={f} pos={:?} elapsed={:?} ostime={:?} time={:?} wait={:?}",
                engine.current_position(),
                rd(0x1735),
                engine.read_sysvar(1, &[1]),
                engine.vm_time(),
                rd(0x1566)
            );
        }
    }
    println!("ticks={ticks} live_tasks={}", engine.live_tasks());
    for field in 0..=9 {
        show(&engine, VarSpace::At, 0x49b, &[93, field]);
    }
    show(&engine, VarSpace::At, 0x4c6, &[]);
    show(&engine, VarSpace::At, 0x4cc, &[]);
    show(&engine, VarSpace::At, 0xb43, &[]);
    show(&engine, VarSpace::At, 0x405, &[]);
    show(&engine, VarSpace::At, 0x40d, &[]);
    show(&engine, VarSpace::At, 0x1566, &[]);
    show(&engine, VarSpace::At, 0x1735, &[]);
    show(&engine, VarSpace::At, 0x837, &[]);
    println!("pos={:?}", engine.current_position());
    println!("cursor={:?}", engine.input_cursor());
    for i in 1..=7 {
        println!("inp[{i}]={:?}", engine.input_inp(i));
    }
    let report = engine.take_diag_report();
    let mut undetermined = 0usize;
    let mut unimplemented = 0usize;
    for d in &report.diags {
        if d.code == "rt.undetermined" {
            undetermined += 1;
        }
        if d.code == "rt.cmd.unimplemented" {
            unimplemented += 1;
        }
    }
    println!("diags undetermined={undetermined} unimplemented={unimplemented}");
    for t in &report.waiting_tasks {
        println!(
            "wait id={} s{} pc{} frames={} text={} main={}",
            t.id, t.script, t.pc, t.wait_frames, t.text_rest, t.main
        );
    }
    for (x, y) in [(175, 150), (350, 300), (100, 100), (640, 360), (175, 300), (350, 150)] {
        engine.set_cursor(x, y);
        engine.set_input("MOUSE_L", true);
        for _ in 0..2 {
            let _ = engine.tick().expect("tick click");
        }
        engine.set_input("MOUSE_L", false);
        for _ in 0..2 {
            let _ = engine.tick().expect("tick release");
        }
        show(&engine, VarSpace::At, 0x49b, &[93, 2]);
        println!("clicked ({x},{y}) cursor={:?} pos={:?}", engine.cursor(), engine.current_position());
    }
    println!("after-click cursor={:?} pos={:?}", engine.cursor(), engine.current_position());
    for field in 0..=9 {
        show(&engine, VarSpace::At, 0x49b, &[93, field]);
    }
    show(&engine, VarSpace::At, 0x4c6, &[]);
    for d in &report.diags {
        if d.code == "rt.cmd.unimplemented" || d.code == "rt.undetermined" {
            println!("diag code={} detail={}", d.code, d.detail);
        }
    }
}
