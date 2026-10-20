//! 定位未取证变量空间前缀(0x60/0x23)的现场。
use std::collections::BTreeMap;
use yu_rt::input::InputTimeline;
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource};
fn main() {
    let root = std::env::args().nth(1).unwrap();
    let src = DirSource::open(format!("{root}/data"), Codec::Cp932).unwrap();
    let aux = DirSource::open(&root, Codec::Cp932).unwrap();
    let cfg = BootConfig { entry_label: "SYSTEM_START".into(), ..BootConfig::default() };
    let mut vm = Vm::boot_with_aux(Box::new(src), Some(Box::new(aux)), cfg).unwrap();
    vm.tick_at(0, &InputTimeline::new());
    let mut last = (0u32, 0u32, String::new());
    let mut rows: BTreeMap<String, usize> = BTreeMap::new();
    for e in vm.events() {
        match e {
            yu_rt::VmEvent::Command { script, pc, name, .. } => last = (*script, *pc, name.clone()),
            yu_rt::VmEvent::Diag { detail, .. } if detail.contains("未取证") => {
                *rows.entry(format!("s{} pc{} {} | {}", last.0, last.1, last.2, detail.chars().take(45).collect::<String>())).or_insert(0) += 1;
            }
            _ => {}
        }
    }
    for (k, v) in &rows { println!("{v:4} {k}"); }
    let r = vm.tick_at(16, &InputTimeline::new());
    println!("frame1: cmds={} errors={:?} finished={}", r.commands_pumped, r.errors, r.main_finished);
}
