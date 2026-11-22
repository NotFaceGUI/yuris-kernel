//! 取证探针:boot 后跑 N tick,按槽号 dump 运行时变量值。
//!
//! 用法:`dump_runtime_vars <project-root> <ticks> <slot…>`
//!
//! 槽号用 `0x` 前缀按十六进制解析;输出所有匹配槽号的存储项
//! (`@`/`$`/旧式前缀都会命中)。fatal 后仍打印已到达的状态。
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
        .unwrap_or(200);
    let slots: Vec<u16> = std::env::args()
        .skip(3)
        .map(|s| {
            s.strip_prefix("0x")
                .map(|h| u16::from_str_radix(h, 16).expect("hex slot"))
                .unwrap_or_else(|| s.parse().expect("slot"))
        })
        .collect();

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
        if slots.is_empty() || slots.contains(&r.id) {
            let vals: Vec<String> = slot
                .data
                .iter()
                .take(16)
                .map(|v| match v {
                    Value::Int(i) => i.to_string(),
                    Value::Float(f) => f.to_string(),
                    Value::Str(b) => format!("{:?}", String::from_utf8_lossy(b)),
                })
                .collect();
            println!(
                "{} id={} ty={} dims={:?} n={} [{}]",
                r.display(),
                r.id,
                slot.ty.as_str(),
                slot.dims,
                slot.data.len(),
                vals.join(", ")
            );
        }
    }
}
