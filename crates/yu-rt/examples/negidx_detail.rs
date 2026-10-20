//! 打印负下标的具体变量 id 与维长分布。
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
    let mut rows: BTreeMap<String, usize> = BTreeMap::new();
    for e in vm.events() {
        if let yu_rt::VmEvent::Diag { detail, .. } = e {
            if detail.contains("为负") {
                let head: String = detail.chars().take(70).collect();
                *rows.entry(head).or_insert(0) += 1;
            }
        }
    }
    println!("distinct negative-index patients: {}", rows.len());
    for (k, v) in rows.iter().take(15) { println!("  {v:5}  {k}"); }
}
