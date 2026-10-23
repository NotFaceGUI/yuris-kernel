//! 列出所有字符串数组(维数非空)的形状与非空项样本。
use yu_rt::input::InputTimeline;
use yu_rt::value::Value;
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
    vm.tick_at(0, &InputTimeline::new());
    for (r, slot) in vm.store().iter() {
        if slot.ty != yu_rt::value::ElemType::Str || slot.dims.is_empty() {
            continue;
        }
        let nz: Vec<String> = slot
            .data
            .iter()
            .enumerate()
            .filter_map(|(i, v)| match v {
                Value::Str(b) if !b.is_empty() => {
                    Some(format!("[{i}]={}", String::from_utf8_lossy(b)))
                }
                _ => None,
            })
            .take(4)
            .collect();
        if !nz.is_empty() {
            println!(
                "{} dims={:?} n={} {:?}",
                r.display(),
                slot.dims,
                slot.data.len(),
                nz
            );
        }
    }
}
