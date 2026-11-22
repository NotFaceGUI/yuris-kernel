//! 探针:在工程全部资产字节里按子串搜索(定位二进制表里的资源名)。
//! 用法:`find_asset_bytes <project-root> <needle> [max-bytes]`
use std::path::Path;
use yu_engine::project::{DataEntry, Project};
use yu_vfs::{Codec, DataSource, LookupKey, YpfSource};

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let needle = std::env::args().nth(2).expect("needle");
    let max: usize = std::env::args()
        .nth(3)
        .and_then(|v| v.parse().ok())
        .unwrap_or(4 * 1024 * 1024);
    let project = Project::discover(Path::new(&root), None).expect("project");
    let needles = [needle.as_bytes().to_vec(), needle.encode_utf16().flat_map(|u| u.to_le_bytes()).collect()];
    let mut hits = 0u32;
    match project.entry() {
        DataEntry::Packed(set) => {
            for pack in set.base.iter().chain(set.updates.iter()) {
                let Ok(src) = YpfSource::open(pack, Codec::Cp932) else { continue };
                let label = pack.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                for e in src.entries() {
                    if e.uncompressed_size as usize > max { continue; }
                    let name = String::from_utf8_lossy(&e.name).replace("\\", "/");
                    let Ok(key) = LookupKey::new(&e.name, Codec::Cp932) else { continue };
                    let Ok(bytes) = src.open(&key) else { continue };
                    for (i, n) in needles.iter().enumerate() {
                        if n.is_empty() { continue; }
                        let mut at = 0usize;
                        while let Some(pos) = bytes[at..].windows(n.len()).position(|w| w == n.as_slice()) {
                            let abs = at + pos;
                            println!("{label}:{name} {:?} off=0x{abs:X}", if i == 0 { "bytes" } else { "utf16" });
                            hits += 1;
                            at = abs + 1;
                            if hits > 200 { return; }
                        }
                    }
                }
            }
        }
        DataEntry::Loose(_) => println!("loose data 入口不支持全量扫描"),
    }
    println!("hits={hits}");
}
