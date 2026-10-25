//! 取证探针:比较 `DirSource` 与 `YpfSource` 对同一逻辑目录的 `list()` 结果。
//!
//! 用法:`cargo run -p yu-rt --example probe_list_cmp -- <project_root> <dir>`
use std::collections::BTreeSet;

use yu_vfs::{Codec, DataSource, DirSource, LookupKey, YpfSource};

fn names(src: &dyn DataSource, dir: &str) -> Vec<String> {
    let key = LookupKey::new(dir.as_bytes(), Codec::Cp932).expect("key");
    match src.list(&key) {
        Ok(entries) => entries
            .into_iter()
            .map(|e| {
                format!(
                    "{}:{:?}",
                    String::from_utf8_lossy(&e.name),
                    e.kind
                )
            })
            .collect(),
        Err(d) => vec![format!("ERR {}", d.code)],
    }
}

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let dir = std::env::args().nth(2).unwrap_or_else(|| "ysbin/".into());
    let pack = YpfSource::open(format!("{root}/data.ypf"), Codec::Cp932).expect("ypf");
    let loose = DirSource::open(format!("{root}/data"), Codec::Cp932).expect("dir");
    let p = names(&pack, &dir);
    let l = names(&loose, &dir);
    println!("pack  n={} : {:?}", p.len(), p.iter().take(8).collect::<Vec<_>>());
    println!("loose n={} : {:?}", l.len(), l.iter().take(8).collect::<Vec<_>>());
    let ps: BTreeSet<_> = p.iter().cloned().collect();
    let ls: BTreeSet<_> = l.iter().cloned().collect();
    println!("only pack : {:?}", ps.difference(&ls).take(10).collect::<Vec<_>>());
    println!("only loose: {:?}", ls.difference(&ps).take(10).collect::<Vec<_>>());
}
