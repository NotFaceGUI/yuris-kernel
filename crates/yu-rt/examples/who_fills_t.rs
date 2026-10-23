//! 找出 $T(18,01) 何时被写入(定位 DIAG 依赖)。
use yu_rt::input::InputTimeline;
use yu_rt::value::{VarRef, VarSpace};
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource};
fn main() {
    let root = std::env::args().nth(1).unwrap();
    let src = DirSource::open(format!("{root}/data"), Codec::Cp932).unwrap();
    let aux = DirSource::open(&root, Codec::Cp932).unwrap();
    let cfg = BootConfig {
        entry_label: "SYSTEM_START".into(),
        ..BootConfig::default()
    };
    let mut vm = Vm::boot_with_aux(Box::new(src), Some(Box::new(aux)), cfg).unwrap();
    // $T id 从 YSVR 找:value_type=3 且 dims 以 3 维开头的大数组
    let cands: Vec<_> = vm
        .tables()
        .boot
        .vars
        .records
        .iter()
        .filter(|r| r.value_type == 3 && r.dims.len() >= 2)
        .map(|r| (r.var_id, r.dims.clone()))
        .collect();
    println!("str arrays: {:?}", &cands[..cands.len().min(10)]);
    vm.tick_at(0, &InputTimeline::new());
    for (id, dims) in cands.iter().take(6) {
        let r = VarRef::new(VarSpace::Dollar, *id);
        if let Some(slot) = vm.store().slot(r) {
            let nz = slot
                .data
                .iter()
                .filter(|v| matches!(v, yu_rt::value::Value::Str(b) if !b.is_empty()))
                .count();
            println!("${id} dims={dims:?} nonempty={nz}/{}", slot.data.len());
        }
    }
    let hits: Vec<_> = vm
        .events()
        .iter()
        .filter_map(|e| match e {
            yu_rt::VmEvent::Command {
                name, script, pc, ..
            } if name == "CSV" || name == "FILEACT" || name == "DIALOG" => {
                Some(format!("{name} s{script} pc{pc}"))
            }
            _ => None,
        })
        .collect();
    println!("io commands: {hits:?}");
}
