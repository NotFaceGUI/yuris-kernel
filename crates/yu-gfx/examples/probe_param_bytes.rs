//! 取证探针:打印 CG/CGACT 每个参数在 yscm 里的种类与范围检查器编号。
//!
//! 用途:引擎按**参数声明下标**取槽值(`[0x684160 + 下标] → slot →
//! [slot*8 + 0x683740]`),所以把处理体里的读取点还原成参数名需要
//! 「下标 → 名字」表。本探针直接给出该表(`CG`/`CGACT` 的声明顺序)。
//!
//! 用法:`cargo run -p yu-gfx --example probe_param_bytes -- <game_root>`
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource, YpfSource};

fn main() {
    let root = std::env::args().nth(1).expect("game root");
    let ypf = std::path::PathBuf::from(&root).join("data.ypf");
    let src: Box<dyn yu_vfs::DataSource> =
        Box::new(YpfSource::open(&ypf, Codec::Cp932).expect("data.ypf"));
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
    for cmd in ["CG", "CGACT"] {
        let Some(c) = vm
            .tables()
            .boot
            .yscm
            .commands
            .iter()
            .find(|c| c.name == cmd)
        else {
            println!("== {cmd}: 表里没有 ==");
            continue;
        };
        println!("== {cmd} ==");
        for (i, p) in c.params.iter().enumerate() {
            println!(
                "  {i:3} {:6} kind={:?} checker={}",
                p.name, p.kind, p.checker
            );
        }
    }
}
