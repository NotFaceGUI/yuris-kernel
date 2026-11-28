//! 临时取证:把 `cgsys/font/**` 的字形 PNG 全部导出,并写 manifest。
//!
//! `cargo run -p yu-engine --example export-font -- <root> <out-dir>`

use std::path::{Path, PathBuf};

use yu_engine::project::{Project, CODEC};
use yu_format::Codec;
use yu_vfs::{DataSource, EntryKind, LookupKey};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

fn safe_name(bytes: &[u8]) -> String {
    let name = Codec::Cp932
        .decode(bytes, b"font-export")
        .unwrap_or_else(|_| hex(bytes));
    name.chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect()
}

fn walk(
    source: &dyn DataSource,
    dir: &[u8],
    out: &Path,
    manifest: &mut Vec<String>,
    count: &mut usize,
) {
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
        match entry.kind {
            EntryKind::Dir => walk(source, &child, out, manifest, count),
            EntryKind::File => {
                let dir_text = String::from_utf8_lossy(dir);
                let Some(folder) = dir_text.strip_prefix("cgsys/font/") else {
                    continue;
                };
                let folder = folder.replace('/', "_");
                let file = safe_name(&entry.name);
                let target_dir = out.join(&folder);
                if std::fs::create_dir_all(&target_dir).is_err() {
                    continue;
                }
                let Ok(path) = LookupKey::new(&child, CODEC) else {
                    continue;
                };
                let Ok(bytes) = source.open(&path) else {
                    continue;
                };
                let target = target_dir.join(&file);
                if std::fs::write(&target, &bytes).is_err() {
                    continue;
                }
                manifest.push(format!(
                    "{folder}\t{}\t{}\t{}\t{}",
                    hex(&entry.name),
                    file,
                    target.display(),
                    bytes.len()
                ));
                *count += 1;
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let root = args.get(1).expect("project root");
    let out = PathBuf::from(args.get(2).expect("out dir"));
    std::fs::create_dir_all(&out).expect("create out");
    let project = Project::discover(Path::new(root), None).expect("open project");
    let source = project.open_source().expect("open source");
    let mut manifest = vec!["folder\tname_hex\tname_decoded\tpath\tbytes".to_string()];
    let mut count = 0usize;
    walk(source.as_ref(), b"cgsys/font", &out, &mut manifest, &mut count);
    std::fs::write(out.join("manifest.tsv"), manifest.join("\n")).expect("manifest");
    println!("exported {count} glyphs -> {}", out.display());
}
