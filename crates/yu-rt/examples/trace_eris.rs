//! 追踪:打印进入指定 (script, pc) 前后的命令序列(定位调用方)。
use yu_rt::input::InputTimeline;
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DataSource, DirSource, LookupKey, YpfSource};

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let use_pack = std::env::args().any(|a| a == "--pack");
    let want_s: u32 = std::env::args().nth(2).and_then(|v| v.parse().ok()).unwrap_or(64);
    let want_pc: u32 = std::env::args().nth(3).and_then(|v| v.parse().ok()).unwrap_or(46);
    let before: usize = std::env::args().nth(4).and_then(|v| v.parse().ok()).unwrap_or(40);

    let cfg = BootConfig { entry_label: "SYSTEM_START".to_string(), fps: 60, ..BootConfig::default() };
    let mut vm = if use_pack {
        let src = YpfSource::open(format!("{root}/data.ypf"), Codec::Cp932).expect("data.ypf");
        let aux = DirSource::open(&root, Codec::Cp932).expect("project");
        // YpfSource 不是 'static 问题:用 Box<dyn DataSource>
        let _ = LookupKey::root();
        Vm::boot_with_aux(Box::new(src), Some(Box::new(aux)), cfg).expect("boot")
    } else {
        let src = DirSource::open(format!("{root}/data"), Codec::Cp932).expect("data");
        let aux = DirSource::open(&root, Codec::Cp932).expect("project");
        Vm::boot_with_aux(Box::new(src), Some(Box::new(aux)), cfg).expect("boot")
    };
    let r = vm.tick_at(0, &InputTimeline::new());
    println!("frame0: cmds={} errors={} finished={}", r.commands_pumped, r.errors.len(), r.main_finished);
    let evs = vm.events();
    // 找到第一次出现目标命令的位置
    let mut hit = None;
    for (i, e) in evs.iter().enumerate() {
        if let yu_rt::VmEvent::Command { script, pc, .. } = e {
            if *script == want_s && *pc == want_pc { hit = Some(i); break; }
        }
    }
    match hit {
        None => println!("未命中 s{want_s} pc{want_pc}"),
        Some(i) => {
            let start = i.saturating_sub(before);
            println!("== {} events before/at s{want_s} pc{want_pc} ==", i - start + 1);
            for e in &evs[start..=i] {
                println!("{}", yu_rt::json::event_line(e));
            }
        }
    }
}
