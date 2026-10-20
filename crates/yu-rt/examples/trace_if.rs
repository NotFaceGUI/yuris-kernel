//! 打印 s10 pc70..95 的事件序列(含多次进入),定位 IF 帧失配。
use yu_rt::input::InputTimeline;
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource};
fn main() {
    let root = std::env::args().nth(1).unwrap();
    let src = DirSource::open(format!("{root}/data"), Codec::Cp932).unwrap();
    let aux = DirSource::open(&root, Codec::Cp932).unwrap();
    let cfg = BootConfig { entry_label: "SYSTEM_START".into(), ..BootConfig::default() };
    let mut vm = Vm::boot_with_aux(Box::new(src), Some(Box::new(aux)), cfg).unwrap();
    let r = vm.tick_at(0, &InputTimeline::new());
    println!("cmds={} err={:?}", r.commands_pumped, r.errors);
    let evs = vm.events();
    println!("events={}", evs.len());
    // 最后 40 条 s10 相关事件
    let tail: Vec<String> = evs.iter().filter_map(|e| match e {
        yu_rt::VmEvent::Command { script, pc, name, .. } if [10u32,11,5,66,60].contains(script) => Some(format!("CMD s{script} pc{pc} {name}")),
        yu_rt::VmEvent::Jump { script, pc, to_pc, to_script, kind, .. } if *script == 10 => Some(format!("JMP s{script} pc{pc} -> s{to_script} pc{to_pc} {:?}", kind)),
        yu_rt::VmEvent::Diag { detail, .. } => Some(format!("DIAG {detail}")),
        _ => None,
    }).rev().take(40).collect::<Vec<_>>();
    for l in tail.iter().rev() { println!("{l}"); }
}
