//! 取证探针:静态普查全部脚本里某个命令的**参数名使用次数与位置**。
//!
//! 用途:判断某个 `CGACT` 图像动作 / `WINDOW` 字段到底有没有语料在用 ——
//! 「手册无页」或「未实现」的条目先看这里,再决定要不要花力气取证实现。
//! 统计口径是**组参数槽名**,不是运行期执行次数(同一组在循环里只算一次)。
//!
//! 用法:`census_params <project-root> <命令名[:参数名]>…`
//! (带 `:参数名` 时只统计/定位含该参数槽的组)
use std::collections::BTreeMap;

use yu_engine::project::Project;
use yu_rt::tables::TableSet;

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let names: Vec<String> = std::env::args().skip(2).collect();
    assert!(!names.is_empty(), "至少要给一个命令名");

    let project = Project::discover(std::path::Path::new(&root), None).expect("project");
    let key = project.resolve_script_key(None).expect("script key");
    let src = project.open_source().expect("data source");
    let mut tables = TableSet::load_from(src.as_ref(), None, key.bytes).expect("tables");

    let ids: Vec<u32> = tables.boot.scripts.entries.iter().map(|e| e.index).collect();
    println!("脚本数 = {}", ids.len());

    for spec in names {
        let (name, only) = match spec.split_once(':') {
            Some((c, p)) => (c.to_string(), Some(p.to_string())),
            None => (spec.clone(), None),
        };
        let Some(cmd) = tables.boot.yscm.commands.iter().find(|c| c.name == name) else {
            println!("== {spec}: 表里没有 ==");
            continue;
        };
        // 槽号 → 参数名;槽号即 yscm 声明下标,与运行期取槽一致。
        // 先拷成 owned,避免与 `load_script` 的可变借用冲突。
        let slot_name: Vec<String> = cmd.params.iter().map(|p| p.name.clone()).collect();
        let mut hits: BTreeMap<String, u64> = BTreeMap::new();
        let mut locs: Vec<(u32, u32, u32)> = Vec::new();
        let mut groups = 0u64;
        for id in &ids {
            let Ok(script) = tables.load_script(src.as_ref(), *id) else {
                continue;
            };
            let script = script.clone();
            for pc in 0..script.group_count() {
                let Some(g) = script.group(pc) else { continue };
                let Some(c) = tables.boot.yscm.command(g.opcode) else {
                    continue;
                };
                if c.name != name {
                    continue;
                }
                // 收集本组的槽名(去重)。
                let mut seen: Vec<&str> = Vec::new();
                for w in script.group_windows(pc) {
                    let Some(n) = slot_name.get(w.kind as usize) else {
                        continue;
                    };
                    if seen.contains(&n.as_str()) {
                        continue;
                    }
                    seen.push(n);
                }
                if let Some(only) = only.as_deref() {
                    if !seen.contains(&only) {
                        continue;
                    }
                }
                groups += 1;
                locs.push((*id, pc as u32, script.source_position(pc).unwrap_or(0)));
                for n in seen {
                    *hits.entry(n.to_string()).or_default() += 1;
                }
            }
        }
        println!("== {spec}: {groups} 组 ==");
        let mut rows: Vec<(String, u64)> = hits.into_iter().collect();
        rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        for (n, count) in rows {
            println!("  {n:16} {count}");
        }
        // 位置(脚本/组号/源码行),最多 40 条 —— 取证时直接跳过去看。
        for (sid, pc, line) in locs.iter().take(40) {
            println!("  @ s{sid} pc{pc} line{line}");
        }
    }
}
