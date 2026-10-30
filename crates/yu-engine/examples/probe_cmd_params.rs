//! 取证探针:打印任意命令的参数声明表(下标 → 名字/种类/检查器)。
//!
//! 引擎按**参数声明下标**取槽值,把处理体里的读取点还原成参数名需要
//! 「下标 → 名字」表;本探针用工程自己的 `ysc.ybn` 给出该表,不依赖
//! 内核里任何手写表。
//!
//! 用法:`probe_cmd_params <project-root> <命令名…>`
use yu_engine::project::Project;
use yu_rt::tables::TableSet;

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let names: Vec<String> = std::env::args().skip(2).collect();
    assert!(!names.is_empty(), "至少要给一个命令名");

    let project = Project::discover(std::path::Path::new(&root), None).expect("project");
    let key = project.resolve_script_key(None).expect("script key");
    let src = project.open_source().expect("data source");
    let tables = TableSet::load_from(src.as_ref(), None, key.bytes).expect("tables");

    for name in names {
        let Some(c) = tables.boot.yscm.commands.iter().find(|c| c.name == name) else {
            println!("== {name}: 表里没有 ==");
            continue;
        };
        println!("== {name} ==");
        for (i, p) in c.params.iter().enumerate() {
            println!("  {i:3} {:10} kind={:?} checker={}", p.name, p.kind, p.checker);
        }
    }
}
