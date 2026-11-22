//! 取证探针:在真实工程上 boot 后 dump 所有字符串数组(标签表/文本表)。
//!
//! 用法:`probe_game_arrays <project-root> [ticks] [needle]`
use std::path::Path;
use yu_engine::project::Project;
use yu_rt::input::InputTimeline;
use yu_rt::value::Value;
use yu_rt::{BootConfig, Vm};

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let ticks: u32 = std::env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);
    let needle = std::env::args().nth(3).unwrap_or_default();
    let project = Project::discover(Path::new(&root), None).expect("project");
    let key = project.resolve_script_key(None).expect("script key");
    let src = project.open_source().expect("source");
    let aux = project.open_aux().expect("aux");
    let mut vm = Vm::boot_with_aux(
        src,
        Some(aux),
        BootConfig {
            entry_label: "SYSTEM_START".into(),
            script_key: key.bytes,
            codecs: yu_rt::CodecConfig {
                script: yu_format::Codec::Gbk,
                text: yu_format::Codec::Gbk,
                name: yu_format::Codec::Cp932,
            },
            ..BootConfig::default()
        },
    )
    .expect("boot");
    for f in 0..ticks {
        let _ = vm.tick_at(f, &InputTimeline::new());
    }
    for (r, slot) in vm.store().iter() {
        if slot.ty != yu_rt::value::ElemType::Str || slot.dims.is_empty() {
            continue;
        }
        let items: Vec<(usize, String)> = slot
            .data
            .iter()
            .enumerate()
            .filter_map(|(i, v)| match v {
                Value::Str(b) if !b.is_empty() => {
                    Some((i, String::from_utf8_lossy(b).into_owned()))
                }
                _ => None,
            })
            .collect();
        if !needle.is_empty() && !items.iter().any(|(_, s)| s.contains(&needle)) {
            continue;
        }
        println!(
            "{} dims={:?} n={} nonempty={}",
            r.display(),
            slot.dims,
            slot.data.len(),
            items.len()
        );
        for (i, s) in items.iter().take(200) {
            println!("  [{i}]={s}");
        }
    }
}
