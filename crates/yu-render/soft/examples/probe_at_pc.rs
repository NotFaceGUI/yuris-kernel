//! 取证探针:在指定 `(sid, pc)` 命令**执行后**立刻打印变量切片。
//!
//! 用法:
//!   `cargo run -p yu-backend-soft --example probe_at_pc -- <root> <from> <to> <sid> <pc,pc,...> <var,...>`
//!
//! 用途(DIV-GFX-046):`@3394` 在同一帧里被写又被清零,只有命令边界采样
//! 才能看出「谁先谁后」;帧末快照会把两者都掩盖掉。
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use yu_gfx::{GfxConfig, GfxHost, GfxState, Viewport};
use yu_rt::input::InputTimeline;
use yu_rt::value::Value;
use yu_rt::{BootConfig, Vm, VmEvent};
use yu_vfs::{Codec, DirSource, YpfSource};

fn main() {
    let mut args = std::env::args().skip(1);
    let root = args.next().expect("用法: <root> <from> <to> <sid> <pcs> <vars>");
    let from: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    let to: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(1);
    let sid: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(66);
    let pcs: Vec<u32> = args
        .next()
        .map(|s| s.split(',').filter_map(|x| x.trim().parse().ok()).collect())
        .unwrap_or_default();
    let vars: Vec<u16> = args
        .next()
        .map(|s| s.split(',').filter_map(|x| x.trim().parse().ok()).collect())
        .unwrap_or_default();

    let root_path = PathBuf::from(&root);
    let ypf = root_path.join("data.ypf");
    let src: Box<dyn yu_vfs::DataSource> = if ypf.is_file() {
        Box::new(YpfSource::open(&ypf, Codec::Cp932).unwrap())
    } else {
        Box::new(DirSource::open(root_path.join("data"), Codec::Cp932).unwrap())
    };
    let aux = DirSource::open(&root_path, Codec::Cp932).unwrap();
    let cfg = BootConfig {
        entry_label: "SYSTEM_START".into(),
        fps: 60,
        screen_color: 32,
        dx_mode: 0,
        paths: yu_rt::vm::HostPaths {
            system_path: Some(format!("{root}/").into_bytes()),
            save_path: Some(format!("{root}/save/").into_bytes()),
            appdata_path: Some(format!("{root}/").into_bytes()),
            personal_path: Some(format!("{root}/").into_bytes()),
            project_path: Some(format!("{root}/").into_bytes()),
        },
        ..BootConfig::default()
    };
    let mut vm = Vm::boot_with_aux(src, Some(Box::new(aux)), cfg).unwrap();
    let viewport = Viewport::new(
        vm.tables().boot.config.width,
        vm.tables().boot.config.height,
    );
    let state = Rc::new(RefCell::new(
        GfxState::from_tables(vm.tables(), GfxConfig::new(viewport)).unwrap(),
    ));
    vm.set_graphics_host(Box::new(GfxHost::new(state.clone())));

    let mut slice = |vm: &Vm, tag: &str| {
        let mut out = Vec::new();
        for (r, slot) in vm.store().iter() {
            if !vars.contains(&r.id) {
                continue;
            }
            let vals: Vec<String> = slot
                .data
                .iter()
                .take(12)
                .map(|v| match v {
                    Value::Int(x) => format!("{x}"),
                    Value::Float(x) => format!("{x}"),
                    Value::Str(b) => format!("{:?}", String::from_utf8_lossy(b)),
                })
                .collect();
            let shape = vm
                .store()
                .shape(r)
                .map(|(ty, dims)| format!("{ty:?}{dims:?}"))
                .unwrap_or_else(|| "?".into());
            out.push(format!("{}[{}] shape={shape}", r.display(), vals.join(",")));
        }
        if !out.is_empty() {
            println!("      {tag} {}", out.join(" "));
        }
    };

    let tl = InputTimeline::new();
    for f in 0..to {
        vm.tick_at(f * 16, &tl);
        let events = vm.take_events();
        for ev in &events {
            if let VmEvent::Command { script, pc, name, .. } = ev {
                if *script == sid && pcs.contains(pc) && f >= from {
                    println!("f{} pc{pc} {name}", f + 1);
                    slice(&vm, "after");
                }
            }
        }
    }
}
