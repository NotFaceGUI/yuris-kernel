//! 取证探针:启动期关键命令流(DIALOG / MOVIE / YMV / WINDOW / TEXT / ERROR / EXEC)。
//!
//! 用法:`cargo run -p yu-rt --example probe_startup_cmds -- <game_root> [frames]`
use yu_rt::input::InputTimeline;
use yu_rt::{BootConfig, Vm, VmEvent};
use yu_vfs::{Codec, DirSource, YpfSource};

fn main() {
    let root = std::env::args().nth(1).expect("game root");
    let frames: u32 = std::env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
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

    let interesting = |name: &str| -> bool {
        matches!(
            name,
            "DIALOG" | "ERROR" | "MOVIE" | "MOVIEEND" | "YMV" | "YMVEND" | "YMVINFO"
                | "WINDOW" | "WINDOWEND" | "TEXT" | "FILEACT" | "FILEINFO" | "TRAY"
        ) || name.starts_with("MENU")
            || name.starts_with("SYS")
    };

    let tl = InputTimeline::new();
    for f in 0..frames {
        vm.tick_at(f * 16, &tl);
        let events = vm.take_events();
        if events.is_empty() {
            continue;
        }
        let mut printed = 0usize;
        for e in &events {
            if let VmEvent::Command {
                script, pc, name, args, ..
            } = e
            {
                if !interesting(name) {
                    continue;
                }
                printed += 1;
                let ra: Vec<String> = args
                    .iter()
                    .map(|a| match &a.value {
                        yu_rt::value::Value::Int(i) => format!("@{}={i}", a.slot),
                        yu_rt::value::Value::Float(x) => format!("@{}={x}", a.slot),
                        yu_rt::value::Value::Str(b) => {
                            format!("@{}={:?}", a.slot, String::from_utf8_lossy(b))
                        }
                    })
                    .collect();
                println!("f{f} s{script} pc{pc} {name} [{}]", ra.join(" "));
            }
        }
        if printed > 0 {
            println!("-- frame {f}: {printed} interesting --");
        }
    }
    println!("== pending dialogs ==");
    for d in vm.pending_dialogs() {
        println!(
            "  id={} caption={:?} text={:?} buttons={:?} input={}",
            d.id,
            d.caption_display(),
            d.text_display(),
            d.buttons,
            d.input
        );
    }
    println!("== unimplemented ==");
    for (name, n) in vm.unimplemented_cmds() {
        println!("  {name}: {n}");
    }
}
