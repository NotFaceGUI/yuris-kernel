//! 临时取证:列出/导出工程数据源里的文件(用完即删)。
//!
//! `cargo run -p yu-engine --example asset-walk -- <root> [filter] [--dump REL]`

use std::path::Path;

use yu_engine::project::{Project, CODEC};
use yu_vfs::{DataSource, EntryKind, LookupKey};

fn walk(source: &dyn DataSource, dir: &[u8], filter: &str) {
    let Ok(key) = LookupKey::new(dir, CODEC) else {
        return;
    };
    let Ok(entries) = source.list(&key) else {
        return;
    };
    for entry in entries {
        let mut child = dir.to_vec();
        if !child.is_empty() {
            child.push(b'/');
        }
        child.extend_from_slice(&entry.name);
        let name = String::from_utf8_lossy(&entry.name);
        match entry.kind {
            EntryKind::Dir => walk(source, &child, filter),
            EntryKind::File => {
                let path = String::from_utf8_lossy(&child);
                if filter.is_empty() || path.contains(filter) {
                    println!("{} {} {}", path, entry.size.unwrap_or(0), name);
                }
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let root = args.get(1).expect("project root");
    let filter = args.get(2).cloned().unwrap_or_default();
    let project = Project::discover(Path::new(root), None).expect("open project");
    let source = project.open_source().expect("open source");
    walk(source.as_ref(), b"", &filter);
    if let Some(pos) = args.iter().position(|a| a == "--dump") {
        let rel = args.get(pos + 1).expect("relative path");
        let key = LookupKey::new(rel.as_bytes(), CODEC).expect("key");
        let bytes = source.open(&key).expect("open file");
        eprintln!("dump: {rel} bytes={}", bytes.len());
        // 原始字节写 stdout(便于 | xxd / python 读)。
        use std::io::Write;
        std::io::stdout().write_all(&bytes).expect("write stdout");
    }
}
