//! 取证探针:列出一个 `.sd` 的块表;btype 1 打印配置段长度/头字节。
//!
//! 用法:`dump_sd <file.sd> [dno]`
use std::path::Path;

fn main() {
    let path = std::env::args().nth(1).expect("file.sd");
    let only: Option<u32> = std::env::args().nth(2).and_then(|v| v.parse().ok());
    let origin = Path::new(&path)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "sd".into());
    let bytes = std::fs::read(&path).expect("read sd");
    let save = yu_format::yssd::YssdSave::parse(&bytes, origin.as_bytes()).expect("parse ysd");
    println!(
        "{}: version={:#x} entries={}",
        origin,
        save.version,
        save.entries.len()
    );
    for entry in &save.entries {
        if let Some(dno) = only {
            if entry.dno != dno {
                continue;
            }
        }
        println!(
            "DNO={} btype={} var_id={} clen={} ulen={}",
            entry.dno,
            entry.btype,
            entry.var_id,
            entry.compressed.len(),
            entry.ulen
        );
        let Ok(raw) = entry.decompress(origin.as_bytes()) else {
            println!("  decompress failed");
            continue;
        };
        println!("  raw {} bytes", raw.len());
        match entry.btype {
            1 => match yu_format::yssd::YssdConfigPayload::parse(&raw, origin.as_bytes(), entry.dno) {
                Ok(cfg) => {
                    println!(
                        "  config A={} B={}",
                        cfg.section_a.len(),
                        cfg.section_b.len()
                    );
                    for (name, seg) in [("A", &cfg.section_a), ("B", &cfg.section_b)] {
                        let head: String = seg
                            .iter()
                            .take(64)
                            .map(|b| format!("{b:02x}"))
                            .collect::<Vec<_>>()
                            .join(" ");
                        println!("    {name}[0..{}] {head}", seg.len().min(64));
                    }
                }
                Err(d) => println!("  config parse error: {d:?}"),
            },
            0 => {
                let head: String = raw
                    .iter()
                    .take(64)
                    .map(|b| format!("{b:02x}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                println!("  var head {head}");
            }
            _ => {}
        }
    }
}
