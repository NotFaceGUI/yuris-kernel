//! 启动链事件观察器:打印 ERIS boot 的高信号事件,用于核对
//! `SYSTEM_START → es.ERIS → ES.FIRST → ES.GAMEMAIN → …` 的推进顺序。
//!
//! 用法:
//! ```text
//! cargo run -p yu-rt --example boot_events -- <project_root> [frames] [选项]
//! ```
//! - `<project_root>`:含 `data.ypf`(优先)或 `data/` 的工程根;
//!   也可用 `YURIS_SAMPLE_ERIS_ROOT` 环境变量。
//! - 选项:`--all-jumps` 打印全部跳转(默认只打跨脚本跳转);
//!   `--json` 改用稳定 JSONL;`--vars` 先倾倒非空字符串初值;
//!   `--domain-samples N` 每个 (domain,name) 每帧最多打印 N 条(0 = 不限);
//!   `--no-domain` 完全不打印域事件;`--chain` 只打印首次跨脚本跳转、
//!   首次域调用与去重诊断(启动链速览);`--all-diags` 关闭诊断去重;
//!   `--groups <sid>:<from>:<to>` 运行后倾倒该脚本的组窗口(定位越界 pc);
//!   `--label <text>` 打印标签表里名字包含该文本的条目(脚本号 + pc);
//!   `--diags` 打印 `Vm::diags()` 的结构化诊断(含 where=脚本/任务/pc/最后命令)。
//!
//! 说明:`record_events=false`(不记录通用 `Command` 事件)以避免首帧
//! 上百万条命令;任务/跳转/等待/域/系统/诊断事件仍然完整产出。

use std::path::Path;

use yu_format::codec::Codec as TextCodec;
use yu_rt::event::{ArgValue, JumpKind, VmEvent};
use yu_rt::input::InputTimeline;
use yu_rt::tables::{BootTables, ScriptList, VarInit};
use yu_rt::value::Value;
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec as VfsCodec, DataSource, DirSource, YpfSource};

fn root_arg(args: &[String]) -> String {
    args.get(1)
        .filter(|a| !a.starts_with("--"))
        .cloned()
        .or_else(|| std::env::var("YURIS_SAMPLE_ERIS_ROOT").ok())
        .or_else(|| {
            std::env::var("YURIS_SAMPLE_ERIS_YSBIN").ok().map(|p| {
                Path::new(&p)
                    .ancestors()
                    .nth(2)
                    .expect("ysbin/../..")
                    .display()
                    .to_string()
            })
        })
        .expect("用法: boot_events <project_root> [frames] [--all-jumps] [--json] [--vars]")
}

fn opt_u32(args: &[String], name: &str, default: u32) -> u32 {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn decode(bytes: &[u8]) -> String {
    TextCodec::Cp932
        .decode(bytes, b"event")
        .unwrap_or_else(|_| String::from_utf8_lossy(bytes).into_owned())
}

fn value_text(v: &Value) -> String {
    match v {
        Value::Int(i) => i.to_string(),
        Value::Float(f) => format!("{f}"),
        Value::Str(bytes) => format!("\"{}\"", decode(bytes)),
    }
}

fn args_text(tables: &BootTables, cmd: &str, args: &[ArgValue]) -> String {
    let decl = tables
        .yscm
        .index_of(cmd)
        .and_then(|i| tables.yscm.command(i as u8));
    let mut out = String::new();
    for ArgValue { slot, value } in args {
        let name = decl
            .and_then(|d| d.params.get(usize::from(*slot)))
            .map(|p| p.name.as_str())
            .unwrap_or("?");
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(name);
        out.push('=');
        out.push_str(&value_text(value));
    }
    out
}

fn script_name(list: &ScriptList, id: u32) -> String {
    list.path_of(id as u16)
        .map(decode)
        .unwrap_or_else(|| format!("<s{id}>"))
}

fn jump_is_cross_script(kind: JumpKind, script: u32, to_script: u32) -> bool {
    // GO/GOSUB/RETURN 跨文件才改变执行中的脚本;循环回边同脚本,忽略。
    matches!(
        kind,
        JumpKind::Go | JumpKind::Gosub | JumpKind::Return | JumpKind::TaskStart
    ) && script != to_script
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let root = root_arg(&args);
    let frames = args.get(2).and_then(|v| v.parse().ok()).unwrap_or(120u32);
    let all_jumps = args.iter().any(|a| a == "--all-jumps");
    let json = args.iter().any(|a| a == "--json");
    let dump_vars = args.iter().any(|a| a == "--vars");
    let no_domain = args.iter().any(|a| a == "--no-domain");
    let chain = args.iter().any(|a| a == "--chain");
    let all_diags = args.iter().any(|a| a == "--all-diags");
    let dump_diags = args.iter().any(|a| a == "--diags");
    let label_filter = args
        .iter()
        .position(|a| a == "--label")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let groups_spec = args
        .iter()
        .position(|a| a == "--groups")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| {
            let mut it = v.split(':');
            let sid = it.next()?.parse::<u32>().ok()?;
            let from = it.next().and_then(|v| v.parse().ok()).unwrap_or(0usize);
            let to = it.next().and_then(|v| v.parse().ok()).unwrap_or(usize::MAX);
            Some((sid, from, to))
        });
    let domain_samples = opt_u32(&args, "--domain-samples", 4) as usize;

    let ypf = Path::new(&root).join("data.ypf");
    let source_label;
    let src: Box<dyn DataSource> = if ypf.is_file() {
        source_label = format!("{}", ypf.display());
        Box::new(YpfSource::open(&ypf, VfsCodec::Cp932).expect("data.ypf"))
    } else {
        source_label = format!("{root}/data");
        Box::new(DirSource::open(format!("{root}/data"), VfsCodec::Cp932).expect("data/"))
    };
    let aux = DirSource::open(&root, VfsCodec::Cp932).expect("project root");

    let cfg = BootConfig {
        entry_label: "SYSTEM_START".to_string(),
        fps: 60,
        screen_color: 32,
        dx_mode: 0,
        media_readonly: false,
        os_sound: true,
        // 首帧 ~130 万条 Command;只保留高信号事件。
        record_events: false,
        paths: yu_rt::vm::HostPaths {
            system_path: Some(format!("{root}/").into_bytes()),
            save_path: Some(format!("{root}/save/").into_bytes()),
            appdata_path: Some(format!("{root}/").into_bytes()),
            personal_path: Some(format!("{root}/").into_bytes()),
            project_path: Some(format!("{root}/").into_bytes()),
        },
        ..BootConfig::default()
    };
    let mut vm = Vm::boot_with_aux(src, Some(Box::new(aux)), cfg).expect("boot");

    println!("== source ==");
    println!("  root={root}");
    println!("  data={source_label}");
    for line in vm.table_provenance() {
        println!("  {line}");
    }

    let scripts = vm.tables().boot.scripts.clone();
    if dump_vars {
        println!("== ysv string initial values (non-empty) ==");
        for r in &vm.tables().boot.vars.records {
            let VarInit::Str(bytes) = &r.init else {
                continue;
            };
            if bytes.is_empty() {
                continue;
            }
            let preview = if bytes.starts_with(b"M") && bytes.len() >= 4 {
                let len = u16::from_le_bytes([bytes[1], bytes[2]]) as usize;
                format!(
                    "raw[0..3]={:02x?} ystb_len={len} payload=\"{}\"",
                    &bytes[..3],
                    decode(&bytes[3..])
                )
            } else {
                format!("\"{}\"", decode(bytes))
            };
            println!(
                "  id={:<5} kind={} group={} script={:<4} dims={:?} {}",
                r.var_id, r.kind, r.group, r.script_id, r.dims, preview
            );
        }
    }

    let timeline = InputTimeline::new();
    let mut totals: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    let mut seen_jumps: std::collections::BTreeSet<(u32, u32, &'static str)> =
        std::collections::BTreeSet::new();
    let mut seen_domains: std::collections::BTreeSet<(String, String)> =
        std::collections::BTreeSet::new();
    let mut seen_diags: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    for tick in 0..frames {
        let before_dropped = vm.events_dropped();
        let report = vm.tick_at(tick * 16, &timeline);
        let events = vm.take_events();
        let dropped = vm.events_dropped() - before_dropped;

        if !json {
            println!(
                "== tick {} (frame {}): cmds={} errors={} events={}{} ==",
                tick,
                report.logical_frames,
                report.commands_pumped,
                report.errors.len(),
                events.len(),
                if dropped > 0 {
                    format!(" dropped={dropped}")
                } else {
                    String::new()
                }
            );
        }
        for error in &report.errors {
            println!("  ERR {error}");
        }

        let mut samples: std::collections::BTreeMap<(String, String), (usize, usize)> =
            std::collections::BTreeMap::new();
        for event in &events {
            *totals.entry(event.kind().to_string()).or_insert(0) += 1;
            if json {
                if let VmEvent::Jump {
                    kind,
                    script,
                    to_script,
                    ..
                } = event
                {
                    if !all_jumps && !jump_is_cross_script(*kind, *script, *to_script) {
                        continue;
                    }
                    if chain && !seen_jumps.insert((*script, *to_script, kind.as_str())) {
                        continue;
                    }
                }
                if chain {
                    if let VmEvent::DomainCall { domain, name, .. } = event {
                        if !seen_domains.insert((domain.as_str().to_string(), name.clone())) {
                            continue;
                        }
                    }
                }
                if no_domain && matches!(event, VmEvent::DomainCall { .. }) {
                    continue;
                }
                println!("{}", yu_rt::json::event_line(event));
                continue;
            }
            match event {
                VmEvent::FrameStart { .. } | VmEvent::FrameEnd { .. } => {}
                VmEvent::TaskSpawn {
                    id,
                    script,
                    pc,
                    enabled,
                    main,
                    ..
                } => println!(
                    "  TASK+ {id:<28} s{script}({}) pc{pc} enabled={enabled} main={main}",
                    script_name(&scripts, *script)
                ),
                VmEvent::TaskFinish { id, .. } => println!("  TASK- {id}"),
                VmEvent::Jump {
                    script,
                    pc,
                    to_pc,
                    to_script,
                    kind,
                    ..
                } => {
                    let cross = jump_is_cross_script(*kind, *script, *to_script);
                    if !all_jumps && !cross {
                        continue;
                    }
                    if chain && !seen_jumps.insert((*script, *to_script, kind.as_str())) {
                        continue;
                    }
                    println!(
                        "  JUMP {:<8} s{script} pc{pc} -> s{to_script}({}) pc{to_pc}{}{}",
                        kind.as_str(),
                        script_name(&scripts, *to_script),
                        if cross { "  [cross-script]" } else { "" },
                        if chain { "  [first]" } else { "" }
                    );
                }
                VmEvent::Wait {
                    task,
                    frames,
                    time_ms,
                    ..
                } => println!(
                    "  WAIT {task} frames={} time_ms={}",
                    frames.map_or("-".to_string(), |v| v.to_string()),
                    time_ms.map_or("-".to_string(), |v| v.to_string())
                ),
                VmEvent::Fps { fps, skip, .. } => {
                    println!("  FPS {fps} skip={skip}");
                }
                VmEvent::SystemCall { name, args, .. } => {
                    println!(
                        "  SYSTEM {name} {}",
                        args_text(&vm.tables().boot, name, args)
                    );
                }
                VmEvent::DomainCall {
                    domain, name, args, ..
                } => {
                    if no_domain {
                        continue;
                    }
                    if chain {
                        if !seen_domains.insert((domain.as_str().to_string(), name.clone())) {
                            continue;
                        }
                        println!(
                            "  DOMAIN {:<8} {:<10} {}",
                            domain.as_str(),
                            name,
                            args_text(&vm.tables().boot, name, args)
                        );
                        continue;
                    }
                    let key = (domain.as_str().to_string(), name.clone());
                    let entry = samples.entry(key).or_insert((0, 0));
                    entry.0 += 1;
                    if domain_samples == 0 || entry.1 < domain_samples {
                        entry.1 += 1;
                        println!(
                            "  DOMAIN {:<8} {:<10} {}",
                            domain.as_str(),
                            name,
                            args_text(&vm.tables().boot, name, args)
                        );
                    }
                }
                VmEvent::Text { id, bytes, .. } => {
                    println!("  TEXT id={id:?} \"{}\"", decode(bytes));
                }
                VmEvent::ReturnCode { line, .. } => println!("  RETURNCODE line={line}"),
                VmEvent::End { restart, .. } => println!("  END restart={restart}"),
                VmEvent::Diag { code, detail, .. } => {
                    if !all_diags {
                        let key = format!("{code}: {detail}");
                        let n = seen_diags.entry(key.clone()).or_insert(0);
                        *n += 1;
                        if *n > 1 {
                            continue;
                        }
                        println!("  DIAG {key}");
                    } else {
                        println!("  DIAG {code}: {detail}");
                    }
                }
                VmEvent::Command {
                    name,
                    script,
                    pc,
                    args,
                    ..
                } => {
                    println!(
                        "  CMD {name} s{script} pc{pc} {}",
                        args_text(&vm.tables().boot, name, args)
                    );
                }
            }
        }
        for ((domain, name), (seen, shown)) in &samples {
            if domain_samples > 0 && *seen > *shown {
                println!("  DOMAIN {domain:<8} {name:<10} … (+{} more)", seen - shown);
            }
        }
        if report.main_finished {
            println!("== main task finished at tick {tick} ==");
            break;
        }
    }

    if !all_diags && !seen_diags.is_empty() {
        println!("== diagnostics (deduplicated) ==");
        for (text, n) in &seen_diags {
            println!("  x{n:<6} {text}");
        }
    }
    if dump_diags {
        println!("== structured diagnostics (Vm::diags) ==");
        for d in vm.diags() {
            println!("{}", d.to_json_line());
        }
    }
    if let Some(text) = label_filter {
        println!("== labels containing {text:?} ==");
        for l in &vm.tables().boot.labels.labels {
            if l.name.contains(&text) {
                let path = vm
                    .tables()
                    .boot
                    .scripts
                    .path_of(l.script_id)
                    .map(decode)
                    .unwrap_or_else(|| "<no script>".to_string());
                println!(
                    "  {:<40} s{:<4} pc{:<5} base_a={} base_b={} ({path})",
                    l.name, l.script_id, l.pc, l.base_a, l.base_b
                );
            }
        }
    }
    if let Some((sid, from, to)) = groups_spec {
        println!("== script {sid} groups {from}..{to} ==");
        if let Some(sf) = vm.tables().scripts.get(&sid) {
            let pool = sf.pool();
            for gi in from..to.min(sf.groups.len()) {
                let g = &sf.groups[gi];
                let name = vm
                    .tables()
                    .boot
                    .yscm
                    .command(g.opcode)
                    .map(|c| c.name.as_str())
                    .unwrap_or("<unknown>");
                let line = sf.source_position(gi).unwrap_or(0);
                let mut windows = String::new();
                for w in sf.group_windows(gi) {
                    let payload = pool
                        .get(
                            w.offset as usize
                                ..(w.offset as usize + w.length as usize).min(pool.len()),
                        )
                        .unwrap_or(&[]);
                    let preview: String = payload
                        .iter()
                        .map(|&b| {
                            if (0x20..0x7f).contains(&b) {
                                b as char
                            } else {
                                '.'
                            }
                        })
                        .collect();
                    if !windows.is_empty() {
                        windows.push(' ');
                    }
                    windows.push_str(&format!(
                        "{{k{} s{} len{} off{} {preview:?}}}",
                        w.kind, w.slot, w.length, w.offset
                    ));
                }
                println!("  [{gi}] {name} param={} line={line} {windows}", g.param);
            }
        } else {
            println!("  (script not loaded)");
        }
    }
    println!("== event totals ==");
    for (kind, n) in &totals {
        println!("  {kind}: {n}");
    }
    println!("== unimplemented ==");
    let mut rows: Vec<_> = vm.unimplemented_cmds().iter().collect();
    rows.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
    for (name, n) in rows {
        println!("  {name}: {n}");
    }
}
