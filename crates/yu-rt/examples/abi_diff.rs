//! 取证探针:列出工程 ysv 与一份 ABI TSV 的全部 (type,dims) 差异。
//!
//! 用法:`abi_diff <ysv.ybn> <tsv>`
//! 只读;用于判断某版本与现有 ABI 族的真实差异范围(选表/建表前取证)。

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

    let mut kind1: Vec<&yu_rt::tables::VarRecord> =
        image.records.iter().filter(|r| r.kind == 1).collect();
    kind1.sort_by_key(|r| r.var_id);
    if let Ok(range) = std::env::var("ABI_DIFF_RANGE") {
        let (lo, hi) = range.split_once("..").unwrap_or((range.as_str(), range.as_str()));
        let lo: u32 = lo.parse().unwrap_or(0);
        let hi: u32 = hi.parse().unwrap_or(lo);
        for rec in &kind1 {
            if u32::from(rec.var_id) >= lo && u32::from(rec.var_id) <= hi {
                let init = decls
                    .decls
                    .iter()
                    .find(|d| u32::from(d.slot) == u32::from(rec.var_id) && d.kind == 1)
                    .map(|d| format!("{:?}", d.init))
                    .unwrap_or_default();
                println!(
                    "id={}: type={} dims={:?} group={} file={} init={}",
                    rec.var_id, rec.value_type, rec.dims, rec.group, rec.script_id, init
                );
            }
        }
    }
    let mut wrong = 0usize;
    let mut matched = 0usize;
    for rec in &kind1 {
        let id = rec.var_id as usize;
        if id >= table.decls.len() {
            continue;
        }
        let decl = &table.decls[id];
        if rec.value_type != decl.value_type || rec.dims != decl.dims {
            wrong += 1;
            println!(
                "id={}: ysv type={} dims={:?} vs table {} type={} dims={:?}",
                rec.var_id, rec.value_type, rec.dims, decl.name, decl.value_type, decl.dims
            );
        } else {
            matched += 1;
        }
    }
    println!(
        "ysv={} kind1={} table={} matched={} wrong={} tail={}",
        decls.version,
        kind1.len(),
        table.decls.len(),
        matched,
        wrong,
        table.diff_counts(&image).1
    );
    let _ = Path::new(&tsv_path);
}
