//! 检查 VARACT[DIMSIZE+=1] 是否真的改变了数组长度,并打印负下标现场的数组形状。
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
    // 打印所有数组变量的形状(维数非空),按 id 排序
    let mut rows: Vec<(u16, String, Vec<usize>)> = vm
        .store()
        .iter()
        .filter(|(_, s)| !s.dims.is_empty())
        .map(|(r, s)| (r.id, format!("{:?}{}", r.space, r.id), s.dims.clone()))
        .collect();
    rows.sort();
    println!("array vars: {}", rows.len());
    for (_, name, dims) in rows.iter().take(25) {
        println!("  {name} dims={dims:?}");
    }
    // 关键:ERIS 中 @SP.DEF.NUM 这类计数是否增长
    println!("--- dims stats ---");
    let mut sizes: std::collections::BTreeMap<usize, usize> = std::collections::BTreeMap::new();
    for (_, _, d) in &rows { *sizes.entry(*d.first().unwrap_or(&0)).or_insert(0) += 1; }
    println!("{sizes:?}");
}
