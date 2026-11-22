//! 取证探针:列出某脚本的标签(pc / base_a)。
//!
//! 用法:`dump_labels <project-root> <sid>`
use std::path::Path;
use yu_engine::project::Project;
use yu_rt::tables::TableSet;

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let sid: u16 = std::env::args().nth(2).expect("sid").parse().expect("u16");
    let project = Project::discover(Path::new(&root), None).expect("project");
    let key = project.resolve_script_key(None).expect("script key");
    let src = project.open_source().expect("source");
    let tables = TableSet::load_from(src.as_ref(), None, key.bytes).expect("tables");
    let mut rows: Vec<_> = tables
        .boot
        .labels
        .labels
        .iter()
        .filter(|l| l.script_id == sid)
        .collect();
    rows.sort_by_key(|l| (l.pc, l.name.clone()));
    println!("s{sid}: {} labels", rows.len());
    for l in rows {
        println!("pc={} base_a={} name={}", l.pc, l.base_a, l.name);
    }
}
