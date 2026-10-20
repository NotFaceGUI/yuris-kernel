//! 端到端:按官方入口链 `SYSTEM_START → es.ERIS` 跑 N 帧,可打印事件尾部。
use yu_rt::input::InputTimeline;
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource};

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let frames: u32 = std::env::args().nth(2).and_then(|v| v.parse().ok()).unwrap_or(5);
    let tail: usize = std::env::args().nth(3).and_then(|v| v.parse().ok()).unwrap_or(0);
    let src = DirSource::open(format!("{root}/data"), Codec::Cp932).expect("data");
    let aux = DirSource::open(&root, Codec::Cp932).expect("project");
    // 宿主注入(色深 + 路径);A3:内核不读真实显示/文件系统布局。
    let cfg = BootConfig {
        entry_label: "SYSTEM_START".to_string(),
        fps: 60,
        screen_color: 32,
        paths: yu_rt::vm::HostPaths {
            system_path: Some(format!("{root}/").into_bytes()),
            save_path: Some(format!("{root}/data/save/").into_bytes()),
            appdata_path: Some(format!("{root}/").into_bytes()),
            personal_path: Some(format!("{root}/").into_bytes()),
            project_path: Some(format!("{root}/").into_bytes()),
        },
        ..BootConfig::default()
    };
    let mut vm = Vm::boot_with_aux(Box::new(src), Some(Box::new(aux)), cfg).expect("boot");
    let timeline = InputTimeline::new();
    for f in 0..frames {
        let r = vm.tick_at(f * 16, &timeline);
        println!("frame {f}: cmds={} errors={} main_finished={}", r.commands_pumped, r.errors.len(), r.main_finished);
        for e in &r.errors { println!("   ERR {e}"); }
        if r.main_finished { break; }
    }
    if tail > 0 {
        let evs = vm.events();
        let start = evs.len().saturating_sub(tail);
        println!("== last {} events (of {}) ==", evs.len() - start, evs.len());
        for e in &evs[start..] {
            println!("{}", yu_rt::json::event_line(e));
        }
    }
    println!("== unimplemented ==");
    for (name, n) in vm.unimplemented_cmds() { println!("  {name}: {n}"); }
}
