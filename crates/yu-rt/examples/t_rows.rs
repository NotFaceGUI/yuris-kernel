//! 打印 $T(id=1111) 的非空行,以及 ES.CONFIG.DEFINE 是否被调用。
use std::collections::BTreeSet;
use yu_rt::input::InputTimeline;
use yu_rt::value::{VarRef, VarSpace};
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource};
fn main() {
    let root = std::env::args().nth(1).unwrap();
    let src = DirSource::open(format!("{root}/data"), Codec::Cp932).unwrap();
    let aux = DirSource::open(&root, Codec::Cp932).unwrap();
    let cfg = BootConfig { entry_label: "SYSTEM_START".into(), ..BootConfig::default() };
    let mut vm = Vm::boot_with_aux(Box::new(src), Some(Box::new(aux)), cfg).unwrap();
    vm.tick_at(0, &InputTimeline::new());
    let r = VarRef::new(VarSpace::Dollar, 1111);
    if let Some(s) = vm.store().slot(r) {
        let rows: BTreeSet<usize> = s.data.iter().enumerate()
            .filter(|(_, v)| matches!(v, yu_rt::value::Value::Str(b) if !b.is_empty()))
            .map(|(i, _)| i / 128).collect();
        println!("$T nonempty rows: {rows:?}");
        println!("$T(18,1) = {:?}", s.data.get(18*128+1));
    }
    let gosubs: Vec<String> = vm.events().iter().filter_map(|e| match e {
        yu_rt::VmEvent::Command { name, script, pc, args, .. } if name == "GOSUB" => {
            args.first().map(|a| format!("s{script} pc{pc} -> {}", match &a.value { yu_rt::value::Value::Str(b) => String::from_utf8_lossy(b).into_owned(), _ => "?".into() }))
        }
        _ => None }).collect();
    println!("gosub count={}", gosubs.len());
    for g in gosubs.iter().filter(|g| g.contains("CONFIG") || g.contains("FONT")) { println!("  {g}"); }
}
