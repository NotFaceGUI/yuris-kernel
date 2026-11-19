//! 临时探针:注入鼠标点击,捕获 DIALOG 呈现瞬间的文本状态与最近命令。
//! 用法:`probe_dialog <project-root> [frames] [--click-every N] [--click-x X --click-y Y]`
use std::collections::VecDeque;
use std::path::Path;

use yu_engine::project::Project;
use yu_format::Codec;
use yu_rt::event::VmEvent;
use yu_rt::input::{InputFrame, InputTimeline};
use yu_rt::{BootConfig, Vm};

fn main() {
    let mut args = std::env::args().skip(1);
    let root = args.next().expect("project root");
    let frames: u32 = args
        .next()
        .and_then(|v| v.parse().ok())
        .unwrap_or(900);
    let click_every: u32 = std::env::var("CLICK_EVERY")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);
    let click_x: i64 = std::env::var("CLICK_X")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(576);
    let click_y: i64 = std::env::var("CLICK_Y")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(360);
    let button = std::env::var("CLICK_BUTTON").unwrap_or_else(|_| "MOUSE_L".to_string());

    let project = Project::discover(Path::new(&root), None).expect("project");
    let key = project.resolve_script_key(None).expect("script key");
    let src = project.open_source().expect("source");
    let aux = project.open_aux().expect("aux");
    let save_root = project.root().join("save");
    let cfg = BootConfig {
        entry_label: "SYSTEM_START".into(),
        fps: 60,
        screen_color: 32,
        dx_mode: 1,
        os_sound: true,
        media_readonly: false,
        record_events: true,
        frame_ms: 16,
        script_key: key.bytes,
        paths: yu_rt::vm::HostPaths {
            system_path: Some(format!("{}/", project.root().display()).into_bytes()),
            save_path: Some(format!("{}/", save_root.display()).into_bytes()),
            appdata_path: Some(format!("{}/", project.root().display()).into_bytes()),
            personal_path: Some(format!("{}/", project.root().display()).into_bytes()),
            project_path: Some(format!("{}/", project.root().display()).into_bytes()),
        },
        ..BootConfig::default()
    };
    let mut vm = Vm::boot_with_aux(src, Some(aux), cfg).expect("boot");
    let scripts = vm.tables().boot.scripts.clone();
    let name = |sid: u32| -> String {
        scripts
            .entries
            .iter()
            .find(|e| e.index == sid)
            .map(|e| Codec::Cp932.decode(&e.path, b"path").unwrap_or_default())
            .unwrap_or_default()
    };

    // 点击在相邻两帧:按下→抬起,模拟真实点击沿。
    let mut timeline = InputTimeline::new();
    // 光标逐帧保持:命中测试看按下沿当帧/上一帧的坐标,只在点击帧
    // 写 cursor 会让按下前的位置退回 (0,0)。
    for tick in 0..frames {
        timeline.set(
            u64::from(tick),
            InputFrame::new(Vec::<String>::new()).with_cursor(click_x, click_y),
        );
    }
    if click_every > 0 {
        let mut tick = click_every;
        while tick < frames {
            timeline.set(
                u64::from(tick),
                InputFrame::new([button.as_str()]).with_cursor(click_x, click_y),
            );
            tick += click_every;
        }
    }

    let mut recent: VecDeque<String> = VecDeque::with_capacity(48);
    for tick in 0..frames {
        let report = vm.tick_at(tick * 16, &timeline);
        let events = vm.take_events();
        for e in &report.errors {
            println!("ERR f{tick}: {e}");
        }
        for event in &events {
            match event {
                VmEvent::Command {
                    script,
                    pc,
                    name: cmd,
                    args,
                    ..
                } => {
                    let arg_repr: Vec<String> = args
                        .iter()
                        .map(|a| format!("{:?}", a.value))
                        .collect();
                    recent.push_back(format!(
                        "f{tick} s{script} pc{pc} {cmd} [{}]",
                        arg_repr.join(", ")
                    ));
                    if recent.len() > 48 {
                        recent.pop_front();
                    }
                }
                VmEvent::DomainCall {
                    name: cmd, args, ..
                } if cmd == "DIALOG" => {
                    let flags = vm.text_flags();
                    let bytes = vm.text_bytes().to_vec();
                    println!("=== DIALOG @f{tick} args={args:?}");
                    println!(
                        "TEXT_FLAG len={} hex={:02x?} ascii={:?}",
                        flags.len(),
                        flags,
                        String::from_utf8_lossy(&flags)
                    );
                    println!(
                        "TEXT len={} hex={:02x?} cp932={:?}",
                        bytes.len(),
                        bytes,
                        Codec::Cp932.decode(&bytes, b"text")
                    );
                    println!("--- last commands ---");
                    for line in &recent {
                        println!("{line}");
                    }
                    println!("=== end DIALOG");
                }
                VmEvent::Diag {
                    code, detail, ..
                } if code.contains("text") => {
                    println!("DIAG f{tick} {code}: {detail}");
                }
                VmEvent::TaskSpawn {
                    id,
                    script,
                    pc,
                    main,
                    ..
                } if tick < 30 => {
                    println!("TASK+ f{tick} {id} s{script}({}) pc{pc} main={main}", name(*script))
                }
                _ => {}
            }
        }
        if std::env::var_os("DEBUG_INPUT").is_some() && tick >= 55 && tick <= 65 {
            let m = |id: u16| {
                vm.store()
                    .scalar_int(yu_rt::value::VarRef::new(yu_rt::value::VarSpace::At, id))
            };
            println!(
                "f{tick} INPUT _MOUSE_L={:?} X={:?} Y={:?}",
                m(147),
                m(133),
                m(138)
            );
        }
        if tick % 120 == 0 {
            let flags = vm.text_flags();
            let bytes = vm.text_bytes();
            println!(
                "f{tick} text={:?} flags={:?}",
                Codec::Cp932.decode(bytes, b"text"),
                String::from_utf8_lossy(&flags)
            );
        }
    }
    println!("done frames={frames}");
}
