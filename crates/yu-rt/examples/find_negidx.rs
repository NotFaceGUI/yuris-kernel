//! 定位触发负下标的命令现场(script/pc/命令名/窗口)。
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
    let mut sites: BTreeMap<String, usize> = BTreeMap::new();
    let mut last_cmd = (0u32, 0u32, String::new());
    for e in vm.events() {
        match e {
            yu_rt::VmEvent::Command { script, pc, name, .. } => { last_cmd = (*script, *pc, name.clone()); }
            yu_rt::VmEvent::Diag { detail, .. } if detail.contains("为负:-1") => {
                *sites.entry(format!("s{} pc{} {}", last_cmd.0, last_cmd.1, last_cmd.2)).or_insert(0) += 1;
            }
            _ => {}
        }
    }
    println!("negative-index sites: {}", sites.len());
    let mut rows: Vec<_> = sites.into_iter().collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1));
    for (k, v) in rows.iter().take(20) { println!("  {v:5}  {k}"); }
}
