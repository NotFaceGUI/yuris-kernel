//! 取证探针:导出封包全部条目名(CP932 正确解码)到 stdout,便于 grep。
//!
//! 用法:`cargo run -p yu-rt --example probe_dump_names -- <game_root> [extension-filter]`
use yu_vfs::{Codec, DataSource, LookupKey, YpfSource};

fn cp932(b: &[u8]) -> String {
    yu_format::Codec::Cp932
        .decode(b, b)
        .unwrap_or_else(|_| String::from_utf8_lossy(b).into_owned())
}

fn walk(pack: &YpfSource, dir: &str, depth: usize, ext: Option<&str>, out: &mut Vec<String>) {
    if depth > 8 {
        return;
    }
    let Ok(k) = LookupKey::new(dir.as_bytes(), Codec::Cp932) else {
        return;
    };
    let Ok(entries) = pack.list(&k) else { return };
    for e in entries {
        let name = cp932(&e.name);
        let base = dir.trim_end_matches('/');
        let full = if base.is_empty() {
            name.clone()
        } else {
            format!("{base}/{name}")
        };
        if matches!(e.kind, yu_vfs::EntryKind::Dir) {
            walk(pack, &format!("{full}/"), depth + 1, ext, out);
            continue;
        }
        if let Some(want) = ext {
            if !full.to_ascii_lowercase().ends_with(&format!(".{want}")) {
                continue;
            }
        }
        out.push(format!("{}\t{:?}", full, e.size));
    }
}

fn main() {
    let root = std::env::args().nth(1).expect("game root");
    let ext = std::env::args().nth(2);
    let pack = YpfSource::open(format!("{root}/data.ypf"), Codec::Cp932).expect("data.ypf");
    let mut out = Vec::new();
    walk(&pack, "", 0, ext.as_deref(), &mut out);
    out.sort();
    for line in &out {
        println!("{line}");
    }
    eprintln!("total {}", out.len());
}
