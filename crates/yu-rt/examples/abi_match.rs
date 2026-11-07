//! 用一份 ABI TSV 校验工程 ysv.ybn,打印逐项差异。
//!
//! 用法:`abi_match <ysv.ybn> <tsv>`
use std::path::Path;

fn main() {
    let mut args = std::env::args().skip(1);
    let ysv_path = args.next().expect("ysv.ybn");
    let tsv_path = args.next().expect("tsv");

    let ysv_bytes = std::fs::read(&ysv_path).expect("read ysv");
    let decls = yu_format::ysvr::VarDecls::parse(&ysv_bytes, b"ysv.ybn").expect("parse ysv");
    let mut image = yu_rt::tables::VarImage {
        version: decls.version,
        records: Vec::new(),
    };
    for d in &decls.decls {
        image.records.push(yu_rt::tables::VarRecord {
            kind: d.kind,
            group: d.flag,
            script_id: d.file_id,
            var_id: d.slot,
            value_type: d.ty,
            dims: d.dims.clone(),
            init: yu_rt::tables::VarInit::None,
        });
    }

    let text = std::fs::read_to_string(&tsv_path).expect("read tsv");
    let table = yu_rt::sysvar_abi::SysVarTable::from_tsv_text(&text, "candidate");

    println!(
        "ysv: version={} records={} (kind1={})",
        decls.version,
        decls.decls.len(),
        decls.decls.iter().filter(|d| d.kind == 1).count()
    );
    println!(
        "table: {} 项 ({})",
        table.len(),
        Path::new(&tsv_path).file_name().unwrap().to_string_lossy()
    );
    println!("appended ids (>=表长): {:?}", table.appended_ids(&image));
    match table.verify_against_ysvr(&image) {
        Ok(()) => println!("MATCH"),
        Err(d) => println!("MISMATCH: {}", d.detail),
    }
}
