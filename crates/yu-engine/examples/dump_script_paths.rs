//! 临时探针:列出工程全部脚本 (sid, path),可按子串过滤。
//! 用法:`dump_script_paths <project-root> [needle]`
use std::path::Path;

use yu_engine::project::Project;
use yu_format::Codec;
use yu_rt::tables::TableSet;

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let needle = std::env::args().nth(2).unwrap_or_default();
    let project = Project::discover(Path::new(&root), None).expect("project");
    let key = project.resolve_script_key(None).expect("script key");
    let src = project.open_source().expect("source");
    let tables = TableSet::load_from(src.as_ref(), None, key.bytes).expect("tables");
    for e in &tables.boot.scripts.entries {
        let path = Codec::Cp932.decode(&e.path, b"path").unwrap_or_default();
        if needle.is_empty() || path.contains(&needle) {
            println!("s{} {path}", e.index);
        }
    }
}
