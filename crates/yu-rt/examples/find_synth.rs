//! 在编译语料里定位合成命令(源码 grep 不到的命令),并映射回源码行。
//! 依据: 源码(.yst) ↔ YSTB ↔ 源码行号(part4)三方对齐。
use std::collections::BTreeMap;
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
    let aux = DirSource::open(&root, Codec::Cp932).unwrap();
    let cfg = BootConfig { entry_label: "SYSTEM_START".into(), ..BootConfig::default() };
    let mut vm = Vm::boot_with_aux(Box::new(src), Some(Box::new(aux)), cfg).unwrap();
    // 预装载全部脚本(扫全表)。
    let ids: Vec<u32> = vm.tables().boot.scripts.entries.iter().map(|e| e.index).collect();
    let dat = DirSource::open(format!("{root}/data"), Codec::Cp932).unwrap();
    for id in &ids {
        let _ = vm.tables_mut().load_script(&dat, *id);
    }

    let targets: Vec<String> = std::env::args().skip(1).collect();
    let targets: Vec<String> = if targets.is_empty() {
        vec!["RETURNCODE".into(), "WORD".into()]
    } else {
        targets
    };

    let mut hits: BTreeMap<String, Vec<(u32, usize, u32, usize)>> = BTreeMap::new();
    for (&sid, sf) in vm.tables().scripts.iter() {
        for (gi, g) in sf.groups.iter().enumerate() {
            let name = vm
                .tables()
                .boot
                .yscm
                .command(g.opcode)
                .map(|c| c.name.clone())
                .unwrap_or_default();
            if targets.iter().any(|t| name.eq_ignore_ascii_case(t)) {
                let line = sf.source_position(gi).unwrap_or(0);
                hits.entry(name).or_default().push((sid, gi, line, g.window_count as usize));
            }
        }
    }

    for (cmd, rows) in &hits {
        println!("=== {cmd} : {} 处 ===", rows.len());
        let mut per_script: BTreeMap<u32, Vec<(usize, u32, usize)>> = BTreeMap::new();
        for (sid, gi, line, w) in rows {
            per_script.entry(*sid).or_default().push((*gi, *line, *w));
        }
        for (sid, rs) in &per_script {
            println!("  yst{:05} ({}) × {}", sid, vm.tables().script_path(*sid), rs.len());
            for (gi, line, w) in rs.iter().take(4) {
                println!("    group={:>4} line={} win={}", gi, line, w);
            }
        }
    }
}
