//! 静态命令普查:遍历工程全部脚本,统计命令/参数出现次数(只读)。
//!
//! 用法:`command_census <project-root> [--top N]`
//!
//! 输出:命令总数、逐命令频度、2xx 关注的动画/过渡参数族频度。
//! 用途:在跑帧前先确认真实脚本实际使用了哪些命令/参数,避免按版本猜。

use std::collections::BTreeMap;
use std::path::Path;

use yu_engine::project::Project;
use yu_rt::tables::TableSet;

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let top = std::env::args()
        .position(|a| a == "--top")
        .and_then(|i| std::env::args().nth(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(40usize);
    let project = Project::discover(Path::new(&root), None).expect("project");
    let key = project.resolve_script_key(None).expect("script key");
    let src = project.open_source().expect("source");
    let mut tables = TableSet::load_from(src.as_ref(), None, key.bytes).expect("tables");

    let mut cmd_count: BTreeMap<String, u64> = BTreeMap::new();
    let mut param_count: BTreeMap<(String, String), u64> = BTreeMap::new();
    let mut param_first: BTreeMap<(String, String), (u32, u32)> = BTreeMap::new();
    let mut scripts = 0u32;
    let mut groups = 0u64;
    let mut cg_no_id = 0u64;
    let mut cg_no_id_sample = String::new();
    let script_ids: Vec<u32> = tables
        .boot
        .scripts
        .entries
        .iter()
        .map(|entry| u32::from(entry.index))
        .collect();
    for sid in script_ids {
        let Ok(script) = tables.load_script(src.as_ref(), sid) else {
            continue;
        };
        scripts += 1;
        let script = script.clone();
        for pc in 0..script.group_count() {
            let Some(g) = script.group(pc) else { continue };
            groups += 1;
            let Some(c) = tables.boot.yscm.command(g.opcode) else {
                continue;
            };
            *cmd_count.entry(c.name.clone()).or_default() += 1;
            if c.name == "CG" {
                let has_id = script.group_windows(pc).iter().any(|w| {
                    c.params.get(w.kind as usize).is_some_and(|p| &*p.name == "ID")
                        && script.window_payload(w).is_some_and(|b| !b.is_empty())
                });
                if !has_id {
                    cg_no_id += 1;
                    if cg_no_id_sample.is_empty() {
                        cg_no_id_sample = format!("s{sid} pc{pc}");
                    }
                }
            }
            for w in script.group_windows(pc) {
                let Some(p) = c.params.get(w.kind as usize) else {
                    continue;
                };
                if script.window_payload(w).is_some_and(|b| !b.is_empty()) {
                    let key = (c.name.clone(), p.name.clone());
                    *param_count.entry(key.clone()).or_default() += 1;
                    param_first.entry(key).or_insert((sid, pc as u32));
                }
            }
        }
    }

    println!("scripts={scripts} groups={groups} key={:02X?}", key.bytes);
    println!("CG without non-empty ID: {cg_no_id} (first {cg_no_id_sample})");
    println!("--- commands (top {top}) ---");
    let mut cmds: Vec<_> = cmd_count.iter().collect();
    cmds.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (name, n) in cmds.iter().take(top) {
        println!("{n:8} {name}");
    }
    println!("--- animation/transition params ---");
    for (cmd, params) in [
        (
            "CG",
            &["T", "TID", "TNO", "TA", "FRAME", "MODE", "CASH", "MID", "MNO", "MFILE"][..],
        ),
        (
            "CGACT",
            &[
                "TRANSITION",
                "TRANSITION2",
                "ROTATION",
                "LEFT",
                "RIGHT",
                "TOP",
                "BOTTOM",
                "RESIZE",
                "SIZE5",
                "BLUR",
                "MOSAIC",
                "DIFFUSION",
            ][..],
        ),
        (
            "CGINFO",
            &["TOTALFRAME", "GIFLOOP", "GIFWAIT", "FRAME", "T", "TID", "TA"][..],
        ),
        ("FLASH", &["PLAY", "ENDFRAME"][..]),
        ("FLASHINFO", &["TOTALFRAME", "FRAME"][..]),
        ("MOVIE", &["PLAY", "METHOD"][..]),
        ("TEXT", &["WAIT", "PAGE", "CLICK"][..]),
    ] {
        for p in params {
            let n = param_count
                .get(&(cmd.to_string(), (*p).to_string()))
                .copied()
                .unwrap_or(0);
            if n > 0 {
                let first = param_first
                    .get(&(cmd.to_string(), (*p).to_string()))
                    .map(|(s, pc)| format!(" s{s} pc{pc}"))
                    .unwrap_or_default();
                println!("{n:8} {cmd}[{p}]{first}");
            }
        }
    }
    println!("--- commands with zero runtime handler candidates ---");
    for (name, n) in cmds.iter() {
        if matches!(
            name.as_str(),
            "FLASH" | "FLASHEND" | "FLASHINFO" | "MOVIE" | "MOVIEEND" | "MOVIEINFO"
        ) {
            println!("{n:8} {name}");
        }
    }
}
