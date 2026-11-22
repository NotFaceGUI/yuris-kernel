//! 取证探针:按原始字节子串搜索工程资产名(不按 UTF-8 解码)。
//! 用法:`find_asset_name <project-root> <hex-needle>`

use std::path::Path;

use yu_engine::project::{DataEntry, Project};

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let hex = std::env::args().nth(2).expect("hex needle");
    let needle: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex"))
        .collect();
    let project = Project::discover(Path::new(&root), None).expect("project");
    let mut hits = 0usize;
    if let DataEntry::Packed(set) = project.entry() {
        for pack in set.base.iter().chain(set.updates.iter()) {
            let Ok(src) = yu_vfs::YpfSource::open(pack, yu_vfs::Codec::Cp932) else {
                continue;
            };
            let label = pack.file_name().map(|s| s.to_string_lossy().into_owned());
            for e in src.entries() {
                if e.name.windows(needle.len()).any(|x| x == needle.as_slice()) {
                    println!(
                        "hex={} (lossy: {}) <- {}",
                        e.name.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(""),
                        String::from_utf8_lossy(&e.name),
                        label.as_deref().unwrap_or("?")
                    );
                    hits += 1;
                }
            }
        }
    }
    println!("hits={hits}");
}
