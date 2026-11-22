//! 取证探针:按路径子串找脚本 id。
use std::path::Path;
use yu_engine::project::Project;
use yu_rt::tables::TableSet;

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let needle = std::env::args().nth(2).expect("needle").to_ascii_lowercase();
    let project = Project::discover(Path::new(&root), None).expect("project");
    let key = project.resolve_script_key(None).expect("script key");
    let src = project.open_source().expect("source");
    let tables = TableSet::load_from(src.as_ref(), None, key.bytes).expect("tables");
    let mut hits = 0;
    for e in &tables.boot.scripts.entries {
        let sid = u32::from(e.index);
        let path = tables.script_path(sid);
        if path.to_ascii_lowercase().contains(&needle) {
            println!("sid={sid} path={path}");
            hits += 1;
        }
    }
    println!("hits={hits}");
}
