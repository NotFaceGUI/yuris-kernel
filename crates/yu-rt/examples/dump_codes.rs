//! 找出 _CODE_* 的 sysvar id 与 ysv 初值(检验码值是否来自数据)。
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
    let sv = vm.sysvars();
    let mut shown = 0;
    for (i, d) in sv.decls.iter().enumerate() {
        if d.name.starts_with("_CODE_") {
            let init = vm
                .tables()
                .boot
                .vars
                .records
                .iter()
                .find(|r| r.var_id as usize == i)
                .map(|r| format!("ysv init {:?}", r.init));
            println!("id={i} {} dims={:?} {}", d.name, d.dims, init.unwrap_or("无 ysv 记录".into()));
            shown += 1;
            if shown >= 12 {
                break;
            }
        }
    }
    let total = sv.decls.iter().filter(|d| d.name.starts_with("_CODE_")).count();
    println!("... total _CODE_* = {total}");
}
