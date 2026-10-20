//! 在指定 (script,pc) 前后打印相关变量的形状与值(定位 VARINFO/DIMSIZE)。
use yu_rt::input::InputTimeline;
use yu_rt::value::{VarRef, VarSpace};
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource};
fn main() {
    let root = std::env::args().nth(1).unwrap();
    let want_s: u32 = std::env::args().nth(2).and_then(|v| v.parse().ok()).unwrap_or(2);
    let want_pc: u32 = std::env::args().nth(3).and_then(|v| v.parse().ok()).unwrap_or(16);
    let ids: Vec<u16> = std::env::args().skip(4).filter_map(|v| u16::from_str_radix(v.trim_start_matches("0x"), 16).ok()).collect();
    let src = DirSource::open(format!("{root}/data"), Codec::Cp932).unwrap();
    let aux = DirSource::open(&root, Codec::Cp932).unwrap();
    let cfg = BootConfig { entry_label: "SYSTEM_START".into(), ..BootConfig::default() };
    let mut vm = Vm::boot_with_aux(Box::new(src), Some(Box::new(aux)), cfg).unwrap();
    vm.tick_at(0, &InputTimeline::new());
    let mut last = (0u32, 0u32, String::new());
    let mut hit = false;
    for e in vm.events() {
        if let yu_rt::VmEvent::Command { script, pc, name, .. } = e {
            if *script == want_s && *pc == want_pc { hit = true; break; }
            last = (*script, *pc, name.clone());
        }
    }
    println!("hit={hit} last={:?}", last);
    for id in ids {
        for space in [VarSpace::At, VarSpace::Dollar] {
            let r = VarRef::new(space, id);
            if let Some(slot) = vm.store().slot(r) {
                println!("{:?}{} dims={:?} data[0..4]={:?}", space, id, slot.dims, &slot.data[..slot.data.len().min(4)]);
            }
        }
        if let Some(d) = vm.sysvars().get(id) { println!("sysvar {id} = {}", d.name); }
    }
}
