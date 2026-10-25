//! 取证探针:打印 ysc 里某命令的参数声明(名字/槽位/类型)。
//!
//! 用法:`cargo run -p yu-rt --example probe_ysc_params -- <game_root> CG,CGACT,CGEND`
use yu_rt::BootConfig;
use yu_vfs::{Codec, DirSource, YpfSource};

fn main() {
    let root = std::env::args().nth(1).expect("game root");
    let wanted: Vec<String> = std::env::args()
        .nth(2)
        .map(|s| s.split(',').map(|x| x.trim().to_ascii_uppercase()).collect())
        .unwrap_or_else(|| vec!["CG".into()]);
    let pack = YpfSource::open(format!("{root}/data.ypf"), Codec::Cp932).expect("data.ypf");
    let aux = DirSource::open(&root, Codec::Cp932).expect("game root");
    let vm = yu_rt::Vm::boot_with_aux(
        Box::new(pack),
        Some(Box::new(aux)),
        BootConfig {
            entry_label: "SYSTEM_START".into(),
            ..BootConfig::default()
        },
    )
    .expect("boot");
    let yscm = &vm.tables().boot.yscm;
    for i in 0..yscm.len() {
        let Some(cmd) = yscm.command(i as u8) else { continue };
        let up = cmd.name.to_ascii_uppercase();
        if !wanted.iter().any(|w| w == &up) {
            continue;
        }
        println!("== opcode {i} {} ==", cmd.name);
        for (n, p) in cmd.params.iter().enumerate() {
            println!("  #{n:<3} {} kind={:?} checker={}", p.name, p.kind, p.checker);
        }
    }
}
