//! 取证探针:打印指定脚本在前 N 帧执行的所有命令(含帧号/pc/命令名/参数)。
//!
//! 用法:`cargo run -p yu-rt --example probe_trace_script -- <game_root> <sid> [frames]`
use yu_rt::input::InputTimeline;
use yu_rt::{BootConfig, Vm, VmEvent};
use yu_vfs::{Codec, DirSource, YpfSource};

fn cp932(b: &[u8]) -> String {
    yu_format::Codec::Cp932
        .decode(b, b)
        .unwrap_or_else(|_| format!("<{:02x?}>", b))
}

fn main() {
    let root = std::env::args().nth(1).expect("game root");
    let sid: u32 = std::env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .expect("script id");
    let frames: u32 = std::env::args()
        .nth(3)
        .and_then(|v| v.parse().ok())
        .unwrap_or(300);
    let pack = YpfSource::open(format!("{root}/data.ypf"), Codec::Cp932).expect("data.ypf");
    let aux = DirSource::open(&root, Codec::Cp932).expect("game root");
    let mut vm = Vm::boot_with_aux(
        Box::new(pack),
        Some(Box::new(aux)),
        BootConfig {
            entry_label: "SYSTEM_START".into(),
            fps: 60,
            screen_color: 32,
            dx_mode: 0,
            media_readonly: false,
            os_sound: true,
            paths: yu_rt::vm::HostPaths {
                system_path: Some(format!("{root}/").into_bytes()),
                save_path: Some(format!("{root}/save/").into_bytes()),
                appdata_path: Some(format!("{root}/").into_bytes()),
                personal_path: Some(format!("{root}/").into_bytes()),
                project_path: Some(format!("{root}/").into_bytes()),
            },
            ..BootConfig::default()
        },
    )
    .expect("boot");
    let yscm_names: Vec<String> = (0..vm.tables().boot.yscm.len())
        .filter_map(|i| vm.tables().boot.yscm.command(i as u8).map(|c| c.name.clone()))
        .collect();
    let _ = yscm_names;

    let tl = InputTimeline::new();
    for f in 0..frames {
        vm.tick_at(f * 16, &tl);
        for e in vm.take_events() {
            match e {
                VmEvent::Command {
                    script, pc, name, args, ..
                } if script == sid => {
                    let ra: Vec<String> = args
                        .iter()
                        .map(|a| match &a.value {
                            yu_rt::value::Value::Int(i) => format!("#{}={i}", a.slot),
                            yu_rt::value::Value::Float(x) => format!("#{}={x}", a.slot),
                            yu_rt::value::Value::Str(b) => {
                                format!("#{}={:?}", a.slot, cp932(b))
                            }
                        })
                        .collect();
                    println!("f{f:<4} pc{pc:<5} {name:<12} {}", ra.join(" "));
                }
                VmEvent::Jump {
                    script,
                    pc,
                    to_pc,
                    to_script,
                    kind,
                    ..
                } if script == sid || to_script == sid => {
                    println!(
                        "f{f:<4} pc{pc:<5} <JUMP {kind:?}> → s{to_script} pc{to_pc} [{}]",
                        cp932(vm.script_name(to_script).as_bytes())
                    );
                }
                VmEvent::TaskSpawn { id, script, pc, enabled, .. } => {
                    println!("f{f:<4} <TASK {id}> s{script} pc{pc} enabled={enabled}");
                }
                VmEvent::Diag { code, detail, .. } => {
                    println!("f{f:<4} <DIAG {code}> {detail}");
                }
                _ => {}
            }
        }
    }
}
