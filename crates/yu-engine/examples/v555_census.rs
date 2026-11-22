//! v555 语料静态普查:命令/参数使用次数(只读)。
//!
//! 用法:`v555_census <project-root> [out-tsv]`
//!
//! 输出(TSV):
//!   command  groups  params
//!   `params` = `NAME=count;...`(按次数降序,只统计组里实际出现的参数槽)
//!
//! 口径:组的参数槽名,不是运行期执行次数;同一组循环只算一次。
//! 缺工程/表 → 显式错误,不空跑。

use std::collections::BTreeMap;
use std::path::Path;

use yu_engine::project::Project;
use yu_rt::tables::TableSet;

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let out = std::env::args().nth(2);
    let project = Project::discover(Path::new(&root), None).expect("project");
    let key = project.resolve_script_key(None).expect("script key");
    let src = project.open_source().expect("data source");
    let mut tables = TableSet::load_from(src.as_ref(), None, key.bytes).expect("tables");

    let ids: Vec<u32> = tables.boot.scripts.entries.iter().map(|e| e.index).collect();
    let mut command_groups: BTreeMap<String, u64> = BTreeMap::new();
    let mut command_params: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    let mut unknown_opcodes: BTreeMap<u8, u64> = BTreeMap::new();
    let mut groups_total = 0u64;
    let mut scripts_ok = 0u64;
    let mut scripts_failed = 0u64;
    let mut failed: Vec<(u32, String)> = Vec::new();

    for id in ids {
        let script = match tables.load_script(src.as_ref(), id) {
            Ok(s) => s.clone(),
            Err(e) => {
                scripts_failed += 1;
                failed.push((id, format!("{e:?}")));
                continue;
            }
        };
        scripts_ok += 1;
        for pc in 0..script.group_count() {
            let Some(g) = script.group(pc) else { continue };
            groups_total += 1;
            let Some(cmd) = tables.boot.yscm.command(g.opcode) else {
                *unknown_opcodes.entry(g.opcode).or_default() += 1;
                continue;
            };
            *command_groups.entry(cmd.name.to_string()).or_default() += 1;
            let params = command_params.entry(cmd.name.to_string()).or_default();
            for w in script.group_windows(pc) {
                if let Some(p) = cmd.params.get(w.kind as usize) {
                    *params.entry(p.name.to_string()).or_default() += 1;
                }
            }
        }
    }

    let mut lines = vec![
        format!("# root={root}"),
        format!(
            "# scripts_ok={scripts_ok} scripts_failed={scripts_failed} groups={groups_total} unknown_opcodes={}",
            unknown_opcodes.len()
        ),
        "command\tgroups\tparams".to_string(),
    ];
    let mut rows: Vec<_> = command_groups.into_iter().collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    for (name, groups) in rows {
        let mut params: Vec<_> = command_params
            .remove(&name)
            .unwrap_or_default()
            .into_iter()
            .collect();
        params.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let p = params
            .into_iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(";");
        lines.push(format!("{name}\t{groups}\t{p}"));
    }
    for (id, msg) in failed {
        lines.push(format!("# failed s{id}: {msg}"));
    }
    if !unknown_opcodes.is_empty() {
        lines.push("# unknown_opcodes".to_string());
        for (op, n) in unknown_opcodes {
            lines.push(format!("# opcode_{op}\t{n}"));
        }
    }
    let text = lines.join("\n") + "\n";
    if let Some(path) = out {
        std::fs::write(&path, text).expect("write census");
        println!("wrote {path}");
    } else {
        print!("{text}");
    }
}
