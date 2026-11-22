//! v555 语料静态普查:`PRODUCT[COM=...]` 的常量使用分布。
//!
//! 用法:`v555_product_census <project-root> [out-tsv]`
//!
//! 只统计 `COM` 窗口能折叠成整数常量的调用;变量/表达式形态记 `dynamic`。

use std::collections::BTreeMap;
use std::path::Path;

use yu_engine::project::Project;
use yu_rt::expr;
use yu_rt::tables::TableSet;

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let out = std::env::args().nth(2);
    let project = Project::discover(Path::new(&root), None).expect("project");
    let key = project.resolve_script_key(None).expect("script key");
    let src = project.open_source().expect("source");
    let mut tables = TableSet::load_from(src.as_ref(), None, key.bytes).expect("tables");
    let ids: Vec<u32> = tables.boot.scripts.entries.iter().map(|e| e.index).collect();

    let mut consts: BTreeMap<i64, u64> = BTreeMap::new();
    let mut dynamic = 0u64;
    let mut calls = 0u64;
    for id in ids {
        let Ok(script) = tables.load_script(src.as_ref(), id) else {
            continue;
        };
        let script = script.clone();
        for pc in 0..script.group_count() {
            let Some(g) = script.group(pc) else { continue };
            let Some(cmd) = tables.boot.yscm.command(g.opcode) else {
                continue;
            };
            if cmd.name != "PRODUCT" {
                continue;
            }
            calls += 1;
            for w in script.group_windows(pc) {
                let Some(p) = cmd.params.get(w.kind as usize) else {
                    continue;
                };
                if p.name != "COM" {
                    continue;
                }
                let payload = script.window_payload(w).unwrap_or(&[]);
                let value = expr::decode(payload)
                    .ok()
                    .and_then(|instrs| expr::simple_expr(&instrs))
                    .and_then(|e| match e {
                        expr::SimpleExpr::Const(v) => Some(v),
                        _ => None,
                    });
                match value {
                    Some(v) => {
                        *consts.entry(v).or_default() += 1;
                        if std::env::var("COM_FILTER")
                            .ok()
                            .and_then(|s| s.parse::<i64>().ok())
                            == Some(v)
                        {
                            println!("s{id} pc{pc} COM={v}");
                        }
                    }
                    None => dynamic += 1,
                }
            }
        }
    }
    let mut lines = vec![
        format!("# root={root}"),
        format!("# calls={calls} dynamic_com={dynamic}"),
        "com\tcount".to_string(),
    ];
    for (com, n) in &consts {
        lines.push(format!("{com}\t{n}"));
    }
    let text = lines.join("\n") + "\n";
    if let Some(path) = out {
        std::fs::write(&path, text).expect("write");
        println!("wrote {path}");
    } else {
        print!("{text}");
    }
}
