//! 倾倒 YSCom.ycd 命令段:每个命令的参数声明(名字/类型)。
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
    let ycd = vm.tables().boot.ycd.as_ref().expect("no ycd");
    let filter = std::env::args().nth(1).unwrap_or_default();
    for (i, c) in ycd.commands.iter().enumerate() {
        if !filter.is_empty() && !c.name.to_uppercase().contains(&filter.to_uppercase()) {
            continue;
        }
        print!("[{i}] {}", c.name);
        for p in &c.params {
            print!("  {}(k{} b1={} b2={})", p.name, p.kind, p.b1, p.b2);
        }
        println!();
    }
}
