//! 探针:boot 后查看场景/画面状态矩阵 @1177(0x499) 的非零单元。
//! 用法:`probe_scenario_state <project-root> <ticks>`
use std::path::Path;
use yu_engine::project::Project;
use yu_rt::input::InputTimeline;
use yu_rt::value::{Value, VarRef, VarSpace};
use yu_rt::{BootConfig, Vm};

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let ticks: u32 = std::env::args().nth(2).and_then(|v| v.parse().ok()).unwrap_or(300);
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
    let store = vm.store();
    for (slot, name) in [(1177u16, "0x499"), (1217, "0x4c1"), (1219, "0x4c3"), (1222, "0x4c6"), (1146, "0x47a")] {
        let r = VarRef::new(VarSpace::At, slot);
        let v = store.scalar_int(r);
        println!("{name} (@{slot}) = {v:?}");
    }
    if let Some((_, arr)) = store.iter().find(|(r, slot)| {
        r.space == VarSpace::At && r.id == 1177 && !slot.dims.is_empty()
    }) {
        let mut shown = 0usize;
        for (i, v) in arr.data.iter().enumerate() {
            if let Value::Int(n) = v {
                if n != &0 {
                    println!("@1177[{i}] = {n}");
                    shown += 1;
                    if shown >= 40 {
                        break;
                    }
                }
            }
        }
        println!("@1177 nonzero shown={shown}");
        for idx in [141usize * 256 + 44, 44 * 256 + 141, 141 * 256 + 45, 142 * 256 + 44, 141 * 256 + 0, 0] {
            let v = arr.data.get(idx).map(|v| format!("{v:?}")).unwrap_or_default();
            println!("@1177[{idx}] = {v}");
        }
    }
    println!("live_tasks={}", vm.live_tasks());
}
