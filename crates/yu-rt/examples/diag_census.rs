//! 一帧内的诊断/未实现分布(定位 E2E 卡点)。
use std::collections::BTreeMap;
use yu_rt::input::InputTimeline;
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource};

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let frames: u32 = std::env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    let src = DirSource::open(format!("{root}/data"), Codec::Cp932).unwrap();
    let aux = DirSource::open(&root, Codec::Cp932).unwrap();
    let cfg = BootConfig {
        entry_label: "SYSTEM_START".into(),
        ..BootConfig::default()
    };
    let mut vm = Vm::boot_with_aux(Box::new(src), Some(Box::new(aux)), cfg).unwrap();
    for f in 0..frames {
        vm.tick_at(f * 16, &InputTimeline::new());
    }
    let mut by_code: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_detail: BTreeMap<String, usize> = BTreeMap::new();
    for e in vm.events() {
        if let yu_rt::VmEvent::Diag { code, detail, .. } = e {
            *by_code.entry(code.clone()).or_insert(0) += 1;
            let key = detail
                .split('(')
                .next()
                .unwrap_or(detail)
                .chars()
                .take(60)
                .collect::<String>();
            *by_detail.entry(format!("{code} | {key}")).or_insert(0) += 1;
        }
    }
    println!("diag by code:");
    for (k, v) in &by_code {
        println!("  {v:6}  {k}");
    }
    println!("diag by detail (top 25):");
    let mut rows: Vec<_> = by_detail.into_iter().collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.1));
    for (k, v) in rows.iter().take(25) {
        println!("  {v:6}  {k}");
    }
}
