//! 倾倒官方 `.sd` 存档里指定变量 id 的块与值(取证用;DIV-RT-003)。
//!
//! 用法:`dump_yssd_var <var_id> [<file.sd>…]`
//! 省略文件时扫 `$YURIS_SAMPLE_ERIS_ROOT/save/` 的官方 5 文件。
use yu_format::yssd::{parse_multi_payload, YssdSave, YssdValue};

fn first_ints(v: &YssdValue, n: usize) -> String {
    let mut out = Vec::new();
    for i in 0..n {
        let off = i * 8;
        let Some(b) = v.data.get(off..off + 8) else { break };
        out.push(format!("{}", i64::from_le_bytes(b.try_into().unwrap())));
    }
    format!("[{}]", out.join(", "))
}

fn main() {
    let mut args = std::env::args().skip(1);
    let want: u16 = args.next().expect("var_id").parse().expect("u16");
    let files: Vec<String> = args.collect();
    let files = if files.is_empty() {
        let root = std::env::var("YURIS_SAMPLE_ERIS_ROOT").expect("YURIS_SAMPLE_ERIS_ROOT");
        ["extra.sd", "gsd.sd", "kidoku.sd", "save50001.sd", "seld.sd"]
            .iter()
            .map(|f| format!("{root}/save/{f}"))
            .collect()
    } else {
        files
    };

    for file in &files {
        let name = std::path::Path::new(file)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(file);
        let bytes = match std::fs::read(file) {
            Ok(b) => b,
            Err(e) => {
                println!("{name}: 读失败({e})");
                continue;
            }
        };
        let origin = name.as_bytes();
        let save = match YssdSave::parse(&bytes, origin) {
            Ok(s) => s,
            Err(d) => {
                println!("{name}: 解析失败({d})");
                continue;
            }
        };
        println!(
            "{name}: {} 块",
            save.entries.len(),
        );
        for entry in &save.entries {
            let hit = |vid: u16, v: &YssdValue, tag: &str| {
                if want == 0 || vid == want {
                    println!(
                        "{name}: dno={} {tag} var={vid} ty={:?} dims={:?} n={} head{}",
                        entry.dno,
                        v.ty,
                        v.dims,
                        v.element_count(),
                        first_ints(v, 8),
                    );
                }
            };
            match entry.btype {
                0 => {
                    if want != 0 && entry.var_id != want {
                        continue;
                    }
                    let Ok(raw) = entry.decompress(origin) else { continue };
                    if let Ok(v) = YssdValue::parse(&raw, origin, entry.dno) {
                        hit(entry.var_id, &v, "type0");
                    }
                }
                2 => {
                    let Ok(raw) = entry.decompress(origin) else { continue };
                    let Ok(subs) = parse_multi_payload(&raw, origin, entry.dno) else { continue };
                    for sub in &subs {
                        let Ok(sraw) = sub.decompress(origin, entry.dno) else { continue };
                        if let Ok(v) = YssdValue::parse(&sraw, origin, entry.dno) {
                            hit(sub.var_id, &v, "type2");
                        }
                    }
                }
                _ => {}
            }
        }
    }
}
