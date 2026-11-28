//! 取证探针:打印派生后的系统变量 ABI(id → 名字/类型/维数)。
//!
//! 用法:`dump_sysvars <project-root> [名字子串] [lo] [hi]`
//! 无子串则打印 lo..=hi(缺省 0..=140)全部;有子串则全表过滤。

use std::path::Path;

use yu_engine::project::Project;
use yu_rt::tables::TableSet;

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let needle = std::env::args().nth(2).unwrap_or_default();
    let lo: usize = std::env::args()
        .nth(3)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let hi: usize = std::env::args()
        .nth(4)
        .and_then(|v| v.parse().ok())
        .unwrap_or(140);
    let project = Project::discover(Path::new(&root), None).expect("project");
    let key = project.resolve_script_key(None).expect("script key");
    let src = project.open_source().expect("source");
    let tables = TableSet::load_from(src.as_ref(), None, key.bytes).expect("tables");
    let decls = &tables.boot.sysvars.decls;
    println!("total={}", decls.len());
    for (id, d) in decls.iter().enumerate() {
        let hit = needle.is_empty() || d.name.contains(&needle);
        if hit && (needle.is_empty() == (lo..=hi).contains(&id) || !needle.is_empty()) {
            println!("{id}\t{}\tty={}\tdims={:?}", d.name, d.value_type, d.dims);
        }
    }
}
