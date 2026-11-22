//! 取证探针:按工程数据入口打印一个资产的原始字节/文本。
//!
//! 用法:`dump_asset <project-root> <key> [text|hex]`
use std::path::Path;
use yu_engine::project::Project;
use yu_vfs::{Codec, LookupKey};

fn main() {
    let mut args = std::env::args().skip(1);
    let root = args.next().expect("project root");
    let key = args.next().expect("asset key");
    let mode = args.next().unwrap_or_else(|| "text".into());
    let project = Project::discover(Path::new(&root), None).expect("project");
    let src = project.open_source().expect("source");
    let lookup = LookupKey::new(key.as_bytes(), Codec::Cp932).expect("key");
    let bytes = match src.open(&lookup) {
        Ok(bytes) => bytes,
        Err(d) => {
            eprintln!("open {key}: {d:?}");
            std::process::exit(2);
        }
    };
    println!("{key}: {} bytes", bytes.len());
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
