//! 取证探针:解压一个 `.sd` 的 btype=0 变量块并按字符串/整数初值打印。
//!
//! 用法:`dump_sd_raw <file.sd> [dno]`
use std::path::Path;

fn main() {
    let path = std::env::args().nth(1).expect("file.sd");
    let only: Option<u32> = std::env::args().nth(2).and_then(|v| v.parse().ok());
    let raw_out: Option<String> = std::env::args().nth(3);
    let origin = Path::new(&path)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "sd".into());
    let bytes = std::fs::read(&path).expect("read sd");
    let save = yu_format::yssd::YssdSave::parse(&bytes, origin.as_bytes()).expect("parse ysd");
    println!("{}: version={:#x} entries={}", origin, save.version, save.entries.len());
    for entry in &save.entries {
        if let Some(dno) = only {
            if entry.dno != dno {
                continue;
            }
        }
        let Ok(raw) = entry.decompress(origin.as_bytes()) else {
            println!("DNO={} decompress failed", entry.dno);
            continue;
        };
        println!(
            "DNO={} btype={} var_id={} ulen={} raw={}",
            entry.dno, entry.btype, entry.var_id, entry.ulen, raw.len()
        );
        if let Some(out) = &raw_out {
            std::fs::write(out, &raw).expect("write raw");
            println!("  raw written to {out}");
        }
        let head: String = raw.iter().take(64).map(|b| format!("{b:02x}")).collect();
        println!("  head {head}");
        // 变量块:按 u32 打印前 16 个值,并按 4 字节对齐找可打印串。
        let words: Vec<u32> = raw
            .chunks_exact(4)
            .take(16)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        println!("  u32 {words:?}");
        let mut i = 0usize;
        while i + 4 <= raw.len() {
            let len = u32::from_le_bytes([raw[i], raw[i + 1], raw[i + 2], raw[i + 3]]) as usize;
            if (1..=256).contains(&len) && i + 4 + len <= raw.len() {
                let s = &raw[i + 4..i + 4 + len];
                if s.iter().all(|&b| b == 0 || (0x20..0x7f).contains(&b)) && s.iter().any(|&b| b != 0)
                {
                    println!("  str@{i} len={len} {:?}", String::from_utf8_lossy(s));
                }
            }
            i += 4;
        }
    }
}
