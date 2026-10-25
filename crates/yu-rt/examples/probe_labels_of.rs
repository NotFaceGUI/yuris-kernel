//! 取证探针:列出某脚本号的全部标签 + 按关键词搜全表标签。
//!
//! 用法:`cargo run -p yu-rt --example probe_labels_of -- <game_root> <sid> [关键词...]`
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource, YpfSource};

fn cp932(b: &[u8]) -> String {
    yu_format::Codec::Cp932
        .decode(b, b)
        .unwrap_or_else(|_| String::from_utf8_lossy(b).into_owned())
}

fn main() {
    let root = std::env::args().nth(1).expect("game root");
    let sid: u32 = std::env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(142);
    let keywords: Vec<String> = std::env::args()
        .skip(3)
        .map(|s| s.to_ascii_uppercase())
        .collect();

    let pack = YpfSource::open(format!("{root}/data.ypf"), Codec::Cp932).expect("data.ypf");
    let aux = DirSource::open(&root, Codec::Cp932).expect("game root");
    let vm = Vm::boot_with_aux(
        Box::new(pack),
        Some(Box::new(aux)),
        BootConfig {
            entry_label: "SYSTEM_START".into(),
            ..BootConfig::default()
        },
    )
    .expect("boot");

    println!("== s{sid} 的全部标签(按 pc 排序)==");
    let mut rows: Vec<(u32, String)> = vm
        .tables()
        .boot
        .labels
        .iter()
        .filter(|l| u32::from(l.script_id) == sid)
        .map(|l| (l.pc, cp932(&l.name_bytes)))
        .collect();
    rows.sort();
    for (pc, name) in &rows {
        println!("  pc{pc:<6} {name}");
    }

    if !keywords.is_empty() {
        println!("== 全表标签命中 {keywords:?} ==");
        let mut n = 0usize;
        for l in vm.tables().boot.labels.iter() {
            let name = cp932(&l.name_bytes);
            let up = name.to_ascii_uppercase();
            if keywords.iter().any(|k| up.contains(k.as_str())) {
                n += 1;
                println!(
                    "  s{:<4} pc{:<6} {name}",
                    l.script_id,
                    l.pc
                );
            }
        }
        println!("  共 {n} 条");
    }

    // 顺带:s142 有多少标签?scenario_start 里 GO 的目标名集合。
    println!("== 全表:名以 SCENARIO 开头 ==");
    for l in vm.tables().boot.labels.iter() {
        let name = cp932(&l.name_bytes);
        if name.to_ascii_uppercase().starts_with("SCENARIO") {
            println!("  s{:<4} pc{:<6} {name}", l.script_id, l.pc);
        }
    }
}
