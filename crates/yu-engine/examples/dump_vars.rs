//! 取证探针:打印指定全局变量槽的 YSVR 声明(type/dims/kind/init)。
//!
//! 用法:`dump_vars <project-root> <slot…>`
use std::path::Path;
use yu_engine::project::Project;
use yu_rt::tables::TableSet;

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let slots: Vec<u16> = std::env::args()
        .skip(2)
        .map(|s| {
            s.strip_prefix("0x")
                .map(|h| u16::from_str_radix(h, 16).expect("hex slot"))
                .unwrap_or_else(|| s.parse().expect("slot"))
        })
        .collect();
    let project = Project::discover(Path::new(&root), None).expect("project");
    let key = project.resolve_script_key(None).expect("script key");
    let src = project.open_source().expect("source");
    let tables = TableSet::load_from(src.as_ref(), None, key.bytes).expect("tables");
    for id in &slots {
        if let Some(d) = tables.boot.sysvars.get(*id) {
            println!("sysvar id={id} name={} ty={:?} dims={:?}", d.name, d.value_type, d.dims);
        }
    }
    for name in ["_INP", "_MOUSE_X", "_MOUSE_Y", "_MOUSELAST", "_CLICK"] {
        if let Some((id, d)) = tables
            .boot
            .sysvars
            .decls
            .iter()
            .enumerate()
            .find(|(_, d)| d.name == name)
        {
            println!("named name={name} id={id} ty={:?} dims={:?}", d.value_type, d.dims);
        }
    }
    for r in &tables.boot.vars.records {
        if slots.is_empty() || slots.contains(&r.var_id) {
            println!(
                "slot={} kind={} group={} script={} ty={} dims={:?} init={:?}",
                r.var_id, r.kind, r.group, r.script_id, r.value_type, r.dims, r.init
            );
        }
    }
}
