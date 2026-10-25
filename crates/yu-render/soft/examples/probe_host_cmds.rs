//! 取证探针:**挂 GfxHost 的**命令级 trace(带脚本/pc/参数)。
//!
//! 用法:`cargo run -p yu-render-soft --example probe_host_cmds -- <root> <from> <to> [sid] [pc_from:pc_to]`
//!
//! 为什么不复用 `probe_trace_script`:`yu-rt` 里没挂 `GraphicsHost` 时,
//! `CGINFO/WINDOWINFO/FONTINFO` 的 `LET` 完全不写回(`op_info` 直接
//! `report_unimplemented` 返回),脚本读到的是旧值 0 ⇒ **控制流会走岔**。
//! 定位 `es_Sprite.yst` 这类"按 `CGINFO[EXIST]` 决定删不删层"的脚本时,
//! 必须挂宿主跑,否则 trace 是假的。
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use yu_gfx::{GfxConfig, GfxHost, GfxState, Viewport};
use yu_rt::event::VmEvent;
use yu_rt::input::InputTimeline;
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource, YpfSource};

fn cp932(b: &[u8]) -> String {
    yu_format::Codec::Cp932
        .decode(b, b)
        .unwrap_or_else(|_| String::from_utf8_lossy(b).into_owned())
}

fn main() {
    let mut args = std::env::args().skip(1);
    let root = args.next().expect("用法: <root> <from> <to> [sid] [pc_from:pc_to]");
    let from: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    let to: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(100);
    let sid: Option<u32> = args.next().and_then(|v| v.parse().ok());
    let range: Option<(u32, u32)> = args.next().and_then(|v| {
        let (a, b) = v.split_once(':')?;
        Some((a.parse().ok()?, b.parse().ok()?))
    });

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
        record_events: true,
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

    let timeline = InputTimeline::new();
    for f in 0..to {
        vm.tick_at(f * 16, &timeline);
        let mut out: Vec<String> = Vec::new();
        for ev in vm.take_events() {
            match ev {
                VmEvent::Command {
                    script,
                    pc,
                    name,
                    args,
                    ..
                } => {
                    if f < from {
                        continue;
                    }
                    if let Some(s) = sid {
                        if script != s {
                            continue;
                        }
                    }
                    if let Some((a, b)) = range {
                        if pc < a || pc > b {
                            continue;
                        }
                    }
                    let ra: Vec<String> = args
                        .iter()
                        .map(|x| match &x.value {
                            yu_rt::value::Value::Int(i) => format!("#{}={i}", x.slot),
                            yu_rt::value::Value::Float(v) => format!("#{}={v}", x.slot),
                            yu_rt::value::Value::Str(b) => format!("#{}={:?}", x.slot, cp932(b)),
                        })
                        .collect();
                    out.push(format!("f{f:<4} s{script:<4} pc{pc:<5} {name:<12} {}", ra.join(" ")));
                }
                VmEvent::Jump {
                    script,
                    pc,
                    to_pc,
                    to_script,
                    kind,
                    ..
                } => {
                    if f < from || sid.map(|s| s != script).unwrap_or(false) {
                        continue;
                    }
                    if let Some((a, b)) = range {
                        if pc < a || pc > b {
                            continue;
                        }
                    }
                    out.push(format!(
                        "f{f:<4} s{script:<4} pc{pc:<5} <JUMP {kind:?}> → s{to_script} pc{to_pc}"
                    ));
                }
                VmEvent::DomainCall {
                    frame,
                    domain,
                    name,
                    args,
                    script,
                    pc,
                } => {
                    if f < from {
                        continue;
                    }
                    let ra: Vec<String> = args
                        .iter()
                        .map(|x| match &x.value {
                            yu_rt::value::Value::Int(i) => format!("{}={i}", x.slot),
                            yu_rt::value::Value::Float(v) => format!("{}={v}", x.slot),
                            yu_rt::value::Value::Str(b) => format!("{}={:?}", x.slot, cp932(b)),
                        })
                        .collect();
                    if let Some(s) = sid {
                        if script != Some(s) {
                            continue;
                        }
                    }
                    out.push(format!(
                        "f{f:<4} s{:?} pc{:?} DOMAIN {domain:?} {name} (evframe {frame}) {}",
                        script,
                        pc,
                        ra.join(" ")
                    ));
                }
                VmEvent::Diag { code, detail, .. } => {
                    if f < from {
                        continue;
                    }
                    out.push(format!("f{f:<4} <DIAG {code}> {detail}"));
                }
                _ => {}
            }
        }
        for line in out {
            println!("{line}");
        }
    }
}
