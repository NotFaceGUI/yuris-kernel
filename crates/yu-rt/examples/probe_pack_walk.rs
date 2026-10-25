//! 取证探针:递归遍历封包条目,按关键词筛资产(定位片头 logo 真名)。
//!
//! 用法:`cargo run -p yu-rt --example probe_pack_walk -- <game_root> [关键词...]`
use yu_vfs::{Codec, DataSource, LookupKey, YpfSource};

fn cp932(b: &[u8]) -> String {
    yu_format::Codec::Cp932
        .decode(b, b)
        .unwrap_or_else(|_| String::from_utf8_lossy(b).into_owned())
}

fn walk(
    pack: &YpfSource,
    dir: &str,
    depth: usize,
    visitor: &mut dyn FnMut(&str, bool, Option<u64>),
) {
    if depth > 6 {
        return;
    }
    let Ok(k) = LookupKey::new(dir.as_bytes(), Codec::Cp932) else {
        return;
    };
    let Ok(entries) = pack.list(&k) else {
        return;
    };
    for e in entries {
        let name = cp932(&e.name);
        let base = dir.trim_end_matches('/');
        let full = if base.is_empty() {
            name.clone()
        } else {
            format!("{base}/{name}")
        };
        let is_dir = matches!(e.kind, yu_vfs::EntryKind::Dir);
        visitor(&full, is_dir, e.size);
        if is_dir {
            walk(pack, &format!("{full}/"), depth + 1, visitor);
        }
    }
}

fn main() {
    let root = std::env::args().nth(1).expect("game root");
    let keywords: Vec<String> = std::env::args()
        .skip(2)
        .map(|s| s.to_ascii_lowercase())
        .collect();
    let pack = YpfSource::open(format!("{root}/data.ypf"), Codec::Cp932).expect("data.ypf");

    let mut total = 0usize;
    let mut files = 0usize;
    let mut hits: Vec<(String, Option<u64>)> = Vec::new();
    {
        let mut visitor = |full: &str, is_dir: bool, size: Option<u64>| {
            total += 1;
            if !is_dir {
                files += 1;
            }
            if keywords.is_empty() {
                return;
            }
            let low = full.to_ascii_lowercase();
            if keywords.iter().any(|k| low.contains(k.as_str())) {
                hits.push((if is_dir {
                    format!("{full}/ (dir)")
                } else {
                    full.to_string()
                }, size));
            }
        };
        walk(&pack, "", 0, &mut visitor);
    }
    println!("== 遍历完成:条目 {total},文件 {files} ==");
    if keywords.is_empty() {
        return;
    }
    println!("== 命中(关键词 {keywords:?})共 {} ==", hits.len());
    for (name, size) in hits.iter().take(80) {
        println!("  {name} {size:?}");
    }
}
