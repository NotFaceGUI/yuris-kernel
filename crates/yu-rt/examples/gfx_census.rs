//! 图形域 Census:`DomainCall` 的命令/参数分布(首屏语义实现前的取证工具)。
//!
//! 用法:
//!   YURIS_SAMPLE_ERIS_ROOT=<project-eris> cargo run -p yu-rt --example gfx_census -- 5
//!
//! 输出(每帧):
//! - 域命令计数;
//! - `CG` 参数的槽名频度(按工程 ysc 表解析,不硬编码槽号);
//! - `CG` 的 `MODE`/`A`/`T`/`TA`/`E` 取值分布;
//! - 前 N 条 `CG` 的样例(槽名=值)。
use std::collections::BTreeMap;

use yu_rt::event::{ArgValue, Domain, VmEvent};
use yu_rt::input::InputTimeline;
use yu_rt::value::Value;
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource, YpfSource};

fn eris_root() -> String {
    // 唯一约定:工程数据入口是 <工程根>/data;ysbin 位于 <工程根>/data/ysbin。
    std::env::var("YURIS_SAMPLE_ERIS_ROOT")
        .or_else(|_| {
            std::env::var("YURIS_SAMPLE_ERIS_YSBIN").map(|p| {
                std::path::Path::new(&p)
                    .ancestors()
                    .nth(2)
                    .expect("ysbin/../..")
                    .display()
                    .to_string()
            })
        })
        .expect("set YURIS_SAMPLE_ERIS_ROOT or YURIS_SAMPLE_ERIS_YSBIN")
}

fn value_text(v: &Value) -> String {
    match v {
        Value::Int(i) => i.to_string(),
        Value::Float(f) => format!("{f}"),
        Value::Str(b) => {
            let mut s = String::new();
            for &x in b {
                if (0x20..0x7f).contains(&x) {
                    s.push(x as char);
                } else {
                    s.push_str(&format!("\\x{x:02x}"));
                }
            }
            s
        }
    }
}

fn main() {
    let root = eris_root();
    let frames: u32 = std::env::args()
        .nth(1)
        .and_then(|v| v.parse().ok())
        .unwrap_or(3);
    let samples: usize = std::env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    // 数据入口两种合法形态:封包 data.ypf 优先,散盘 data/ 为备。
    let ypf = std::path::PathBuf::from(&root).join("data.ypf");
    let src: Box<dyn yu_vfs::DataSource> = if ypf.is_file() {
        Box::new(YpfSource::open(&ypf, Codec::Cp932).expect("data.ypf"))
    } else {
        Box::new(DirSource::open(format!("{root}/data"), Codec::Cp932).expect("data"))
    };
    let aux = DirSource::open(&root, Codec::Cp932).expect("project");
    let cfg = BootConfig {
        entry_label: "SYSTEM_START".to_string(),
        fps: 60,
        // 只保留域事件(Command 事件在首帧 130 万条,会挤爆有界队列)。
        record_events: false,
        ..BootConfig::default()
    };
    let mut vm = Vm::boot_with_aux(src, Some(Box::new(aux)), cfg).expect("boot");
    let timeline = InputTimeline::new();
    let yscm = vm.tables().boot.yscm.clone();

    for f in 0..frames {
        let r = vm.tick_at(f * 16, &timeline);
        let mut by_name: BTreeMap<String, usize> = BTreeMap::new();
        let mut cg_params: BTreeMap<String, usize> = BTreeMap::new();
        let mut act_params: BTreeMap<String, usize> = BTreeMap::new();
        let mut files: BTreeMap<String, usize> = BTreeMap::new();
        let mut act_samples: Vec<String> = Vec::new();
        let mut field_values: BTreeMap<&'static str, BTreeMap<String, usize>> = BTreeMap::new();
        let mut sample_lines: Vec<String> = Vec::new();
        let mut layer_ids: BTreeMap<String, usize> = BTreeMap::new();
        let evs = vm.events();
        for e in evs {
            let VmEvent::DomainCall {
                domain: Domain::Graphics,
                name,
                args,
                ..
            } = e
            else {
                continue;
            };
            *by_name.entry(name.clone()).or_insert(0) += 1;
            if name == "CGACT" {
                let Some(decl) = yscm.index_of("CGACT").and_then(|i| yscm.command(i as u8)) else {
                    continue;
                };
                let mut line = String::new();
                for ArgValue { slot, value } in args {
                    let pname = decl
                        .params
                        .get(usize::from(*slot))
                        .map(|p| p.name.as_str())
                        .unwrap_or("<out-of-range>");
                    *act_params.entry(pname.to_string()).or_insert(0) += 1;
                    if !line.is_empty() {
                        line.push(' ');
                    }
                    line.push_str(pname);
                    line.push('=');
                    line.push_str(&value_text(value));
                }
                if act_samples.len() < samples {
                    act_samples.push(line);
                }
                continue;
            }
            if name != "CG" {
                continue;
            }
            let Some(decl) = yscm.index_of("CG").and_then(|i| yscm.command(i as u8)) else {
                continue;
            };
            let mut line = String::new();
            for ArgValue { slot, value } in args {
                let pname = decl
                    .params
                    .get(usize::from(*slot))
                    .map(|p| p.name.as_str())
                    .unwrap_or("<out-of-range>");
                *cg_params.entry(pname.to_string()).or_insert(0) += 1;
                if matches!(pname, "A" | "T" | "TA" | "E" | "MODE" | "Z") {
                    *field_values
                        .entry(match pname {
                            "A" => "A",
                            "T" => "T",
                            "TA" => "TA",
                            "E" => "E",
                            "MODE" => "MODE",
                            _ => "Z",
                        })
                        .or_default()
                        .entry(value_text(value))
                        .or_insert(0) += 1;
                }
                if pname == "ID" {
                    *layer_ids.entry(value_text(value)).or_insert(0) += 1;
                }
                if pname == "FILE" {
                    let text = value_text(value);
                    if !text.is_empty() {
                        *files.entry(text).or_insert(0) += 1;
                    }
                }
                if !line.is_empty() {
                    line.push(' ');
                }
                line.push_str(pname);
                line.push('=');
                line.push_str(&value_text(value));
            }
            if sample_lines.len() < samples {
                sample_lines.push(line);
            }
        }
        println!(
            "== frame {f}: commands_pumped={} errors={} main_finished={} ==",
            r.commands_pumped,
            r.errors.len(),
            r.main_finished
        );
        println!("  graphics domain calls:");
        for (k, v) in &by_name {
            println!("    {v:8}  {k}");
        }
        println!("  CG params (slot name -> count):");
        for (k, v) in &cg_params {
            println!("    {v:8}  {k}");
        }
        for (field, vals) in &field_values {
            let mut rows: Vec<_> = vals.iter().collect();
            rows.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
            println!(
                "  CG {field} values (top 12): {}",
                rows.iter()
                    .take(12)
                    .map(|(k, v)| format!("{k}×{v}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
        println!("  distinct CG layer IDs: {}", layer_ids.len());
        let mut ids: Vec<_> = layer_ids.iter().collect();
        ids.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        println!(
            "  top IDs: {}",
            ids.iter()
                .take(15)
                .map(|(k, v)| format!("{k}×{v}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        let mut file_rows: Vec<_> = files.iter().collect();
        file_rows.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        println!(
            "  CG FILE values (top 10): {}",
            file_rows
                .iter()
                .take(10)
                .map(|(k, v)| format!("{k}×{v}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        println!("  CGACT params (slot name -> count):");
        for (k, v) in &act_params {
            println!("    {v:8}  {k}");
        }
        if !act_samples.is_empty() {
            println!("  CGACT samples:");
            for line in &act_samples {
                println!("    {line}");
            }
        }
        if !sample_lines.is_empty() {
            println!("  CG samples:");
            for line in &sample_lines {
                println!("    {line}");
            }
        }
        println!("  diags: unimplemented={} total_events={}", vm.unimplemented_cmds().len(), evs.len());
    }
}
