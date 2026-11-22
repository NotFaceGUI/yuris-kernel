//! 取证探针:列出所有 TASK 命令的 ID 窗口(字符串表达式原始字节)。
use std::path::Path;
use yu_engine::project::Project;
use yu_rt::tables::TableSet;

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let project = Project::discover(Path::new(&root), None).expect("project");
    let key = project.resolve_script_key(None).expect("script key");
    let src = project.open_source().expect("source");
    let mut tables = TableSet::load_from(src.as_ref(), None, key.bytes).expect("tables");
    let Some(task_op) = tables.boot.yscm.index_of("TASK") else {
        eprintln!("no TASK");
        return;
    };
    let mut hits = 0;
    for sid in 0..tables.boot.scripts.entries.len() as u32 {
        let Ok(script) = tables.load_script(src.as_ref(), sid).map(|s| s.clone()) else {
            continue;
        };
        for pc in 0..script.group_count() {
            let Some(g) = script.group(pc) else { continue };
            if usize::from(g.opcode) != task_op {
                continue;
            }
            for w in script.group_windows(pc) {
                if w.kind != 0 {
                    continue;
                }
                let payload = script.window_payload(w).unwrap_or(&[]);
                println!(
                    "s{sid} pc{pc} len={} hex={} text={:?}",
                    payload.len(),
                    payload.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(""),
                    String::from_utf8_lossy(payload)
                );
                hits += 1;
            }
        }
    }
    println!("hits={hits}");
}
