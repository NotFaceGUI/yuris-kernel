//! 取证探针:打印工程 ysc 表里 CGACT/CG/CGINFO 的参数名表(槽号→名字)。
//!
//! 用法:`cargo run -p yu-gfx --example probe_cgact_params -- <game_root>`
use yu_gfx::ParamTable;
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource, YpfSource};

fn main() {
    let root = std::env::args().nth(1).expect("game root");
    let ypf = std::path::PathBuf::from(&root).join("data.ypf");
    let src: Box<dyn yu_vfs::DataSource> = Box::new(
        YpfSource::open(&ypf, Codec::Cp932).expect("data.ypf"),
    );
    let aux = DirSource::open(&root, Codec::Cp932).expect("game root");
    let vm = Vm::boot_with_aux(
        src,
        Some(Box::new(aux)),
        BootConfig {
            entry_label: "SYSTEM_START".into(),
            ..BootConfig::default()
        },
    )
    .expect("boot");
    let table = ParamTable::from_tables(vm.tables());
    for cmd in ["CG", "CGACT", "CGINFO", "CGEND"] {
        println!("== {cmd} ==");
        let Some(params) = table.params(cmd) else {
            println!("  (表里没有该命令)");
            continue;
        };
        for (i, p) in params.iter().enumerate() {
            println!("  {i:3}  {}", p.name);
        }
    }
}
