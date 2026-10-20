//! 按 id 区间倾倒系统变量 ABI 名字(核对 REA id 段假设)。
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource};

fn eris_root() -> String {
    std::env::var("YURIS_SAMPLE_ERIS_ROOT")
        .or_else(|_| {
            std::env::var("YURIS_SAMPLE_ERIS_YSBIN").map(|p| {
                std::path::Path::new(&p).ancestors().nth(2).unwrap().display().to_string()
            })
        })
        .expect("set YURIS_SAMPLE_ERIS_ROOT or YURIS_SAMPLE_ERIS_YSBIN")
}

fn main() {
    let root = eris_root();
    let src = DirSource::open(format!("{root}/data"), Codec::Cp932).unwrap();
    let aux = DirSource::open(root, Codec::Cp932).unwrap();
    let cfg = BootConfig { entry_label: "SYSTEM_START".into(), ..BootConfig::default() };
    let vm = Vm::boot_with_aux(Box::new(src), Some(Box::new(aux)), cfg).unwrap();
    let decls = &vm.sysvars().decls;
    for lo in [133usize, 144, 147, 166, 167, 274, 275, 276, 279, 280, 281, 300, 301, 408, 409, 414, 415, 419, 435, 444, 525, 541, 542, 543, 546, 547] {
        let d = &decls[lo];
        println!("id={lo} {} dims={:?}", d.name, d.dims);
    }
    println!("total={}", decls.len());
}
