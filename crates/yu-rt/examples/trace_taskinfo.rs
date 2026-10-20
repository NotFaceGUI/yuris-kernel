//! 追踪 TASKINFO 返回值与触发除零的变量。
use yu_rt::input::InputTimeline;
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource};
fn main() {
    let root = std::env::args().nth(1).unwrap();
    let src = DirSource::open(format!("{root}/data"), Codec::Cp932).unwrap();
    let aux = DirSource::open(&root, Codec::Cp932).unwrap();
    let cfg = BootConfig { entry_label: "SYSTEM_START".into(), ..BootConfig::default() };
    let mut vm = Vm::boot_with_aux(Box::new(src), Some(Box::new(aux)), cfg).unwrap();
    let tl = InputTimeline::new();
    for f in 0..2 {
        let r = vm.tick_at(f*16, &tl);
        println!("frame {f}: cmds={} err={:?}", r.commands_pumped, r.errors);
        if !r.errors.is_empty() { break; }
    }
    let evs = vm.events();
    let mut ti = 0;
    for (i, e) in evs.iter().enumerate() {
        if let yu_rt::VmEvent::Command { name, script, pc, args, .. } = e {
            if name == "TASKINFO" { ti += 1; if ti <= 6 { println!("TASKINFO s{script} pc{pc} {args:?}"); } }
        }
        if let yu_rt::VmEvent::Diag { detail, .. } = e {
            if detail.contains("除以 0") || detail.contains("div_zero") {
                println!("--- context at event {i} ---");
                for x in &evs[i.saturating_sub(8)..=i] { println!("{}", yu_rt::json::event_line(x)); }
                break;
            }
        }
    }
    println!("TASKINFO total={ti}");
}
