//! 导出一张工程的实际 handler 分类表(按当前 `build_handler_table` 结果)。
//!
//! 用法:`handler_audit <project-root> [out-tsv]`
//!
//! 输出:opcode, name, argc, handler_kind
//! 用途:与 555 exe 的分发表(官方真实 handler 地址)对拍,找出误分类。

use std::path::Path;

use yu_engine::project::Project;
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource};

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let out = std::env::args().nth(2);
    let project = Project::discover(Path::new(&root), None).expect("project");
    let key = project.resolve_script_key(None).expect("script key");
    eprintln!("script key = {:02X?} (source={:?})", key.bytes, key.source);
    let src = project.open_source().expect("source");
    let aux = DirSource::open(&root, Codec::Cp932).expect("aux");
    let vm = Vm::boot_with_aux(
        src,
        Some(Box::new(aux)),
        BootConfig {
            entry_label: "SYSTEM_START".into(),
            script_key: key.bytes,
            ..BootConfig::default()
        },
    )
    .expect("boot");

    let yscm = &vm.tables().boot.yscm;
    let mut lines = vec!["opcode\tname\targc\thandler_kind".to_string()];
    for i in 0..yscm.len() {
        let opcode = i as u8;
        let (name, argc) = yscm
            .command(opcode)
            .map(|c| (c.name.clone(), c.params.len()))
            .unwrap_or_default();
        let kind = vm
            .handler_kind(opcode)
            .map(|k| format!("{k:?}"))
            .unwrap_or_else(|| "NONE".into());
        lines.push(format!("{i}\t{name}\t{argc}\t{kind}"));
    }
    let text = lines.join("\n") + "\n";
    if let Some(path) = out {
        std::fs::write(&path, text).expect("write audit");
        println!("wrote {path}");
    } else {
        print!("{text}");
    }
}
