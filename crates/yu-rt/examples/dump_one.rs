//! 打印单个字符串数组的全部元素。
use yu_rt::input::InputTimeline;
use yu_rt::value::{VarRef, VarSpace};
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource};
fn main() {
    let root = std::env::args().nth(1).unwrap();
    let id: u16 = u16::from_str_radix(
        std::env::args().nth(2).unwrap().trim_start_matches("0x"),
        16,
    )
    .unwrap();
    let src = DirSource::open(format!("{root}/data"), Codec::Cp932).unwrap();
    let aux = DirSource::open(&root, Codec::Cp932).unwrap();
    let cfg = BootConfig {
        entry_label: "SYSTEM_START".into(),
        ..BootConfig::default()
    };
    let mut vm = Vm::boot_with_aux(Box::new(src), Some(Box::new(aux)), cfg).unwrap();
    vm.tick_at(0, &InputTimeline::new());
    if let Some(slot) = vm.store().slot(VarRef::new(VarSpace::Dollar, id)) {
        println!("dims={:?}", slot.dims);
        for (i, v) in slot.data.iter().enumerate() {
            println!("  [{i}] = {}", v.to_event_repr());
        }
    } else {
        println!("no such var");
    }
    let _ = vm.store().read(VarRef::new(VarSpace::At, id), &[0]);
}
