//! 取证探针:列脚本号与路径(只走表,不启动 VM)。
//!
//! 用法:`list_scripts <project-root> [路径子串]`
//! 无子串时列全部;匹配为大小写不敏感的子串。

use std::path::Path;

use yu_engine::project::Project;
use yu_rt::tables::TableSet;

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let needle = std::env::args().nth(2).unwrap_or_default().to_lowercase();
    let project = Project::discover(Path::new(&root), None).expect("project");
    let key = project.resolve_script_key(None).expect("script key");
    let src = project.open_source().expect("source");
    let tables = TableSet::load_from(src.as_ref(), None, key.bytes).expect("tables");
    for (sid, path) in &tables.script_paths {
        let path = String::from_utf8_lossy(path);
        if needle.is_empty() || path.to_lowercase().contains(&needle) {
            println!("s{sid}\t{path}");
        }
    }
}
