//! 取证探针:按工程数据入口打印一个资产的原始字节/文本。
//!
//! 用法:`dump_asset <project-root> <key|hex:xx..> [text|hex|probe]`
//!
//! `probe` = 只报告候选键的 open 结果(键本身与 .png/.bmp/.jpg/.gif/.webp
//! 扩展),用于对齐 `resolve_asset_size` 的探测链。
use std::path::Path;
use yu_engine::project::Project;
use yu_vfs::{Codec, LookupKey};

fn decode_key(raw: &str) -> Vec<u8> {
    if let Some(hex) = raw.strip_prefix("hex:") {
        let digits: Vec<u8> = hex
            .bytes()
            .filter(|b| !b.is_ascii_whitespace() && *b != b'_')
            .collect();
        let mut out = Vec::with_capacity(digits.len() / 2);
        for pair in digits.chunks(2) {
            let s = std::str::from_utf8(pair).expect("hex ascii");
            out.push(u8::from_str_radix(s, 16).expect("hex pair"));
        }
        out
    } else {
        raw.as_bytes().to_vec()
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let root = args.next().expect("project root");
    let key = args.next().expect("asset key");
    let mode = args.next().unwrap_or_else(|| "text".into());
    let project = Project::discover(Path::new(&root), None).expect("project");
    let src = project.open_source().expect("source");
    let key_bytes = decode_key(&key);
    let lookup = LookupKey::new(&key_bytes, Codec::Cp932).expect("key");
    if let Some(needle) = mode.strip_prefix("find:") {
        // 递归列出含 needle(按字节,大小写敏感)的资产路径。
        let needle = needle.as_bytes();
        let mut stack = vec![LookupKey::root()];
        let mut hits = 0usize;
        while let Some(dir) = stack.pop() {
            let Ok(entries) = src.list(&dir) else { continue };
            for e in entries {
                let mut path = dir.as_bytes().to_vec();
                if !path.is_empty() {
                    path.push(b'/');
                }
                path.extend_from_slice(&e.name);
                match e.kind {
                    yu_vfs::EntryKind::Dir => {
                        if let Ok(k) = LookupKey::new(&path, Codec::Cp932) {
                            stack.push(k);
                        }
                    }
                    _ => {
                        if e.name.windows(needle.len()).any(|w| w == needle) {
                            println!("{}", LookupKey::new(&path, Codec::Cp932).unwrap().display());
                            hits += 1;
                        }
                    }
                }
            }
        }
        println!("hits={hits}");
        return;
    }
    if mode == "probe" {
        println!("normalized: {}", lookup.display());
        let mut candidates = vec![key_bytes.clone()];
        for ext in [".png", ".bmp", ".jpg", ".gif", ".webp"] {
            let mut p = key_bytes.clone();
            p.extend_from_slice(ext.as_bytes());
            candidates.push(p);
        }
        for c in candidates {
            let k = LookupKey::new(&c, Codec::Cp932).expect("candidate");
            match src.open_prefix(&k, 16) {
                Ok(prefix) => println!(
                    "  open {} -> prefix {} bytes {:02x?}",
                    k.display(),
                    prefix.len(),
                    prefix
                ),
                Err(d) => println!("  miss {} -> {}", k.display(), d.code),
            }
        }
        return;
    }
    let bytes = match src.open(&lookup) {
        Ok(bytes) => bytes,
        Err(d) => {
            eprintln!("open {}: {d:?}", lookup.display());
            std::process::exit(2);
        }
    };
    println!("{}: {} bytes", lookup.display(), bytes.len());
    if let Some(out) = mode.strip_prefix("bin:") {
        std::fs::write(out, &bytes).expect("write bin");
        return;
    }
    if mode == "hex" {
        for (i, chunk) in bytes.chunks(32).enumerate() {
            print!("{i:08x} ");
            for b in chunk {
                print!("{b:02x} ");
            }
            println!();
        }
    } else {
        for line in bytes.split(|&b| b == b'\n') {
            println!("{}", String::from_utf8_lossy(line));
        }
    }
}
