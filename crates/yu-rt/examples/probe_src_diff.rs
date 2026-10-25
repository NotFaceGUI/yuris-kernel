//! 取证探针:比较散盘 `data/` 与 `data.ypf` 的条目字节是否一致。
//!
//! 用法:`cargo run -p yu-rt --example probe_src_diff -- <project_root> [prefix]`
use std::collections::BTreeMap;

use yu_vfs::{Codec, DataSource, DirSource, YpfSource};

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let prefix = std::env::args().nth(2).unwrap_or_else(|| "ysbin/".into());
    let pack = YpfSource::open(format!("{root}/data.ypf"), Codec::Cp932).expect("data.ypf");
    let dir = DirSource::open(format!("{root}/data"), Codec::Cp932).expect("data dir");

    let mut packed: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
    for e in pack.entries() {
        let mut name = e.name.clone();
        // ypf 条目名以反斜杠分隔(引擎侧 Windows 口径)。
        for b in name.iter_mut() {
            if *b == 0x5c {
                *b = b'/';
            }
        }
        if !name.starts_with(prefix.as_bytes()) {
            continue;
        }
        if let Ok(bytes) = pack.open(&e.normalized) {
            packed.insert(name, bytes);
        }
    }
    println!("packed {} entries under {prefix}", packed.len());

    let mut same = 0usize;
    let mut diff = 0usize;
    let mut missing = 0usize;
    for (name, pbytes) in &packed {
        let key = match yu_vfs::LookupKey::new(name, Codec::Cp932) {
            Ok(k) => k,
            Err(_) => continue,
        };
        match dir.open(&key) {
            Ok(dbytes) => {
                if dbytes == *pbytes {
                    same += 1;
                } else {
                    diff += 1;
                    if diff <= 10 {
                        println!(
                            "  DIFF {}: pack={} dir={}",
                            String::from_utf8_lossy(name),
                            pbytes.len(),
                            dbytes.len()
                        );
                    }
                }
            }
            Err(_) => {
                missing += 1;
                if missing <= 10 {
                    println!("  MISSING in data/: {}", String::from_utf8_lossy(name));
                }
            }
        }
    }
    println!("same={same} diff={diff} missing={missing}");
}
