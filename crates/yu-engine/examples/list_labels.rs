//! 探针:列出工程全部标签(可按子串过滤)。
//! 用法:`list_labels <project-root> [needle]`
use std::path::Path;
use yu_engine::project::Project;
use yu_rt::tables::TableSet;

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let needle = std::env::args().nth(2).unwrap_or_default().to_ascii_uppercase();
    let project = Project::discover(Path::new(&root), None).expect("project");
    let key = project.resolve_script_key(None).expect("script key");
    let src = project.open_source().expect("source");
    let tables = TableSet::load_from(src.as_ref(), None, key.bytes).expect("tables");
    let mut n = 0usize;
    for l in &tables.boot.labels.labels {
        let name = l.name.clone();
        if needle.is_empty() || name.to_ascii_uppercase().contains(&needle) {
            println!("s{} pc{} name={name}", l.script_id, l.pc);
            n += 1;
        }
    }
    println!("hits={n}");
}
