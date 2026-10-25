//! 取证探针:打印所有 `CG` 事件的 FILE/ID/Z/E/A 参数(CP932 解码),
//! 定位片头 logo 图层的资产名与淡入序列。
//!
//! 用法:`cargo run -p yu-rt --example probe_cg_files -- <game_root> [frames]`
use yu_rt::input::InputTimeline;
use yu_rt::{BootConfig, Vm, VmEvent};
use yu_vfs::{Codec, DirSource, YpfSource};

fn cp932(b: &[u8]) -> String {
    yu_format::Codec::Cp932
        .decode(b, b)
        .unwrap_or_else(|_| {
            format!(
                "<坏>{}",
                b.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join("")
            )
        })
}

fn main() {
    let root = std::env::args().nth(1).expect("game root");
    let frames: u32 = std::env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(250);
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
            let VmEvent::DomainCall {
                domain,
                name,
                args,
                script,
                pc,
                ..
            } = e
            else {
                continue;
            };
            if !matches!(domain, yu_rt::event::Domain::Graphics) {
                continue;
            }
            if name != "CG" && name != "CGEND" && name != "CGACT" {
                continue;
            }
            let mut parts: Vec<String> = Vec::new();
            let yscm = &vm.tables().boot.yscm;
            let opcode_of = |cmd: &str| -> Option<u8> {
                (0..yscm.len()).find_map(|i| {
                    yscm.command(i as u8)
                        .filter(|c| c.name == cmd)
                        .map(|_| i as u8)
                })
            };
            let op = opcode_of(&name);
            for a in &args {
                let slot = a.slot;
                let pname = op
                    .and_then(|o| yscm.param(o, slot).map(|p| p.name.clone()))
                    .unwrap_or_else(|| format!("#{}", a.slot));
                let v = match &a.value {
                    yu_rt::value::Value::Int(i) => format!("{i}"),
                    yu_rt::value::Value::Float(x) => format!("{x}"),
                    yu_rt::value::Value::Str(b) => format!("{:?}", cp932(b)),
                };
                let _ = slot;
                parts.push(format!("{pname}={v}"));
            }
            println!("f{f:<4} s{script:?} pc{pc:?} {name} [{}]", parts.join(" "));
        }
    }
}
