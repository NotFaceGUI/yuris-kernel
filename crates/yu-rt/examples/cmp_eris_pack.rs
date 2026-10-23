//! 比较散盘 `data/ysbin` 与 `data.ypf` 内的 ysbin 条目(逐文件 SHA-256)。
use std::collections::BTreeMap;
use std::path::Path;
use yu_vfs::{Codec, DataSource, LookupKey, YpfSource};
fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let src = YpfSource::open(Path::new(&root).join("data.ypf"), Codec::Cp932).expect("open ypf");
    println!(
        "ypf: v{} entries={} dialect={:?}",
        src.header().version,
        src.header().entry_count,
        src.dialect()
    );
    let dir_key = LookupKey::new(b"ysbin", Codec::Cp932).unwrap();
    let mut pack: BTreeMap<String, String> = BTreeMap::new();
    for e in src.list(&dir_key).unwrap() {
        let name = String::from_utf8_lossy(&e.name).into_owned();
        if !name.ends_with(".ybn") {
            continue;
        }
        let k = LookupKey::new(format!("ysbin/{name}").as_bytes(), Codec::Cp932).unwrap();
        let b = src.open(&k).unwrap();
        pack.insert(name, yu_rt::sysvar_abi::sha256_hex(&b));
    }
    let loose_dir = Path::new(&root).join("data/ysbin");
    let mut loose: BTreeMap<String, String> = BTreeMap::new();
    for f in std::fs::read_dir(&loose_dir).unwrap() {
        let p = f.unwrap().path();
        if p.extension().and_then(|s| s.to_str()) != Some("ybn") {
            continue;
        }
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        let b = std::fs::read(&p).unwrap();
        loose.insert(name, yu_rt::sysvar_abi::sha256_hex(&b));
    }
    println!("pack .ybn = {}, loose .ybn = {}", pack.len(), loose.len());
    let mut same = 0;
    let mut diff = Vec::new();
    let mut only_pack = Vec::new();
    let mut only_loose = Vec::new();
    for (n, h) in &pack {
        match loose.get(n) {
            Some(l) if l == h => same += 1,
            Some(_) => diff.push(n.clone()),
            None => only_pack.push(n.clone()),
        }
    }
    for n in loose.keys() {
        if !pack.contains_key(n) {
            only_loose.push(n.clone());
        }
    }
    println!(
        "same={same} diff={} only_pack={} only_loose={}",
        diff.len(),
        only_pack.len(),
        only_loose.len()
    );
    println!("diff: {:?}", &diff[..diff.len().min(20)]);
    println!("only_pack: {:?}", &only_pack[..only_pack.len().min(10)]);
    println!("only_loose: {:?}", &only_loose[..only_loose.len().min(10)]);
}
