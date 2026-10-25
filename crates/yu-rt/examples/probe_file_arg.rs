//! 取证探针:跟踪指定 (sid, pc) 处每条命令的 `#33`(FILE)槽实参,
//! 以及该层建立时的可打印字节。
//!
//! 用法:`cargo run -p yu-rt --example probe_file_arg -- <game_root> <sid> <pc> [frames]`
use yu_rt::input::InputTimeline;
use yu_rt::{BootConfig, Vm, VmEvent};
use yu_vfs::{Codec, DirSource, YpfSource};

fn show(b: &[u8]) -> String {
    let dec = yu_format::Codec::Cp932
        .decode(b, b)
        .unwrap_or_else(|_| "<decode-fail>".into());
    let hex: String = b.iter().take(40).map(|x| format!("{x:02x}")).collect();
    format!("{dec:?}  hex={hex}")
}

fn main() {
    let root = std::env::args().nth(1).expect("game root");
    let sid: u32 = std::env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .expect("sid");
    let target_pc: u32 = std::env::args()
        .nth(3)
        .and_then(|v| v.parse().ok())
        .expect("pc");
    let frames: u32 = std::env::args()
        .nth(4)
        .and_then(|v| v.parse().ok())
        .unwrap_or(100);

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

    let tl = InputTimeline::new();
    for f in 0..frames {
        vm.tick_at(f * 16, &tl);
        for e in vm.take_events() {
            match e {
                VmEvent::Command {
                    script,
                    pc,
                    name,
                    args,
                    ..
                } if script == sid && pc == target_pc => {
                    println!("f{f} s{sid} pc{pc} {name}");
                    for a in &args {
                        if let yu_rt::value::Value::Str(b) = &a.value {
                            println!("   slot{} = {}", a.slot, show(b));
                        } else {
                            println!("   slot{} = {:?}", a.slot, a.value);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}
