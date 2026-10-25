//! 取证探针:商业包前 N 帧的域事件流(`CG`/`CGACT`/`MOVIE`/`DIALOG`/…)
//! + 待回答 DIALOG 队列。
//!
//! 用法:`cargo run -p yu-rt --example probe_domain_stream -- <game_root> [frames]`
use std::collections::BTreeMap;

use yu_rt::input::{InputFrame, InputTimeline};
use yu_rt::{BootConfig, Vm, VmEvent};
use yu_vfs::{Codec, DirSource, YpfSource};

fn main() {
    let root = std::env::args().nth(1).expect("game root");
    let frames: u32 = std::env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);
    let pack = YpfSource::open(format!("{root}/data.ypf"), Codec::Cp932).expect("data.ypf");
    let aux = DirSource::open(&root, Codec::Cp932).expect("game root");
    let vm_holder = Vm::boot_with_aux(
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
    );
    let mut vm = vm_holder.expect("boot");

    // 每 10 帧一次点击(推进 RETURNCODE 等待)。
    let mut tl = InputTimeline::new();
    for f in 0..u64::from(frames) {
        let st = if f % 10 == 5 {
            InputFrame::new(Vec::<String>::new()).with_pulse("MOUSE_L")
        } else {
            InputFrame::new(Vec::<String>::new())
        };
        tl.set(f, st);
    }

    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for f in 0..frames {
        vm.tick_at(f * 16, &tl);
        if f < 12 || f % 30 == 0 {
            println!("== frame {f} ==");
            for e in vm.events() {
                match e {
                    VmEvent::DomainCall {
                        domain,
                        name,
                        args,
                        script,
                        pc,
                        ..
                    } => {
                        *counts.entry(format!("{domain:?}/{name}")).or_insert(0) += 1;
                        let rendered: Vec<String> = args
                            .iter()
                            .map(|a| {
                                let v = match &a.value {
                                    yu_rt::value::Value::Int(i) => format!("{i}"),
                                    yu_rt::value::Value::Float(x) => format!("{x}"),
                                    yu_rt::value::Value::Str(b) => {
                                        String::from_utf8_lossy(b).into_owned()
                                    }
                                };
                                format!("@{}={v}", a.slot)
                            })
                            .collect();
                        println!(
                            "   DOMAIN {domain:?} {name} s{:?} pc{:?} args=[{}]",
                            script,
                            pc,
                            rendered.join(" ")
                        );
                    }
                    VmEvent::Diag { code, detail, .. } => {
                        println!("   DIAG {code}: {detail}");
                    }
                    _ => {}
                }
            }
        } else {
            // 只累计计数。
            for e in vm.events() {
                if let VmEvent::DomainCall { domain, name, .. } = e {
                    *counts.entry(format!("{domain:?}/{name}")).or_insert(0) += 1;
                }
            }
        }
    }

    println!("== 域事件计数(全 {frames} 帧) ==");
    for (k, v) in &counts {
        println!("  {v:6}  {k}");
    }
    println!("== pending dialogs ==");
    let dialogs = vm.pending_dialogs();
    if dialogs.is_empty() {
        println!("  (无)");
    }
    for d in dialogs {
        println!("  id={} caption={:?} text={:?}", d.id, d.caption_display(), d.text_display());
    }
}
