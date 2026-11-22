//! 取证探针:在所有已装载脚本的命令窗口里按字节子串搜索。
//!
//! 用法:`find_script_bytes <project-root> <needle>`
//! 只走 TableSet 解码,不启动 VM、不读游戏资产内容。

use std::path::Path;

use yu_engine::project::Project;
use yu_rt::tables::TableSet;

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let raw = std::env::args().nth(2).expect("needle");
    let needle: Vec<u8> = if let Some(hex) = raw.strip_prefix("hex:") {
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex"))
            .collect()
    } else {
        raw.into_bytes()
    };
    let project = Project::discover(Path::new(&root), None).expect("project");
    let key = project.resolve_script_key(None).expect("script key");
    let src = project.open_source().expect("source");
    let mut tables = TableSet::load_from(src.as_ref(), None, key.bytes).expect("tables");
    let only: Option<u32> = std::env::args().nth(3).and_then(|v| v.parse().ok());
    let ids: Vec<u32> = match only {
        Some(sid) => vec![sid],
        None => (0..tables.boot.scripts.entries.len() as u32).collect(),
    };
    let mut hits = 0usize;
    for sid in ids {
        let Ok(script) = tables.load_script(src.as_ref(), sid).map(|s| s.clone()) else {
            eprintln!("sid {sid}: load failed");
            continue;
        };
        for pc in 0..script.group_count() {
            let Some(group) = script.group(pc) else { continue };
            let Some(cmd) = tables.boot.yscm.command(group.opcode) else {
                continue;
            };
            for w in script.group_windows(pc) {
                let Some(payload) = script.window_payload(w) else { continue };
                if payload.windows(needle.len()).any(|x| x == needle.as_slice()) {
                    println!(
                        "s{sid} pc{pc} {} slot{} payload={:?} hex={}",
                        cmd.name,
                        w.kind,
                        String::from_utf8_lossy(payload),
                        payload.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join("")
                    );
                    hits += 1;
                }
            }
        }
    }
    println!("hits={hits}");
}
