//! 在所有字符串变量里搜索指定子串。
use yu_rt::input::InputTimeline;
use yu_rt::value::Value;
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource};
fn main() {
    let root = std::env::args().nth(1).unwrap();
    let needle = std::env::args().nth(2).unwrap_or_else(|| "backlog".to_string());
    let src = DirSource::open(format!("{root}/data"), Codec::Cp932).unwrap();
    let aux = DirSource::open(&root, Codec::Cp932).unwrap();
    let cfg = BootConfig { entry_label: "SYSTEM_START".into(), ..BootConfig::default() };
    let mut vm = Vm::boot_with_aux(Box::new(src), Some(Box::new(aux)), cfg).unwrap();
    vm.tick_at(0, &InputTimeline::new());
    let mut found = 0;
    for (r, slot) in vm.store().iter() {
        if slot.ty != yu_rt::value::ElemType::Str { continue; }
        for (i, v) in slot.data.iter().enumerate() {
            if let Value::Str(b) = v {
                let s = String::from_utf8_lossy(b);
                if s.to_ascii_lowercase().contains(&needle.to_ascii_lowercase()) && !s.is_empty() {
                    let idx: Vec<usize> = if slot.dims.len() == 2 { vec![i / slot.dims[1], i % slot.dims[1]] } else { vec![i] };
                    println!("{} dims={:?} idx={:?} = {:?}", r.display(), slot.dims, idx, s);
                    found += 1;
                    if found > 12 { return; }
                }
            }
        }
    }
    println!("found {found}");
}
