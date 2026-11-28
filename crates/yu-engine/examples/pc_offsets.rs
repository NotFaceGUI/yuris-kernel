//! 取证探针(临时):组下标 ↔ code 段字节偏移互查。
//!
//! 用法:`pc_offsets <project-root> <sid> <pc>...`

use std::path::Path;

use yu_engine::project::Project;
use yu_rt::tables::TableSet;

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let sid: u32 = std::env::args().nth(2).expect("sid").parse().expect("sid");
    let pcs: Vec<u32> = std::env::args()
        .skip(3)
        .filter(|s| !s.starts_with('@'))
        .map(|s| s.parse().expect("pc"))
        .collect();
    let project = Project::discover(Path::new(&root), None).expect("project");
    let key = project.resolve_script_key(None).expect("script key");
    let src = project.open_source().expect("source");
    let mut tables = TableSet::load_from(src.as_ref(), None, key.bytes).expect("tables");
    let path = tables.script_path(sid);
    let script = tables.load_script(src.as_ref(), sid).expect("script").clone();
    println!("s{sid} groups={} path={path}", script.group_count());
    for pc in pcs {
        println!(
            "pc={pc} code_offset={:?} line={:?}",
            script.code_offset_at(pc as usize),
            script.source_position(pc as usize)
        );
    }
    for off in std::env::args().filter_map(|a| a.strip_prefix('@').map(|v| v.parse().unwrap_or(0))) {
        println!("code_offset={off} -> pc={:?}", script.pc_for_code_offset(off));
    }
}
