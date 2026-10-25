//! 取证探针:列出封包里所有**非 ASCII 文件名**的资产(CP932 名),
//! 并给出 UTF-8 lossy 与 CP932 正确解码的对照。
//!
//! 用法:`cargo run -p yu-rt --example probe_nonascii_assets -- <game_root> [关键字]`
use yu_vfs::{Codec, DataSource, LookupKey, YpfSource};

fn cp932(b: &[u8]) -> String {
    yu_format::Codec::Cp932
        .decode(b, b)
        .unwrap_or_else(|e| format!("<CP932解码失败:{e}>"))
}

fn walk(
    pack: &YpfSource,
    dir: &str,
    depth: usize,
    keyword: Option<&str>,
    out: &mut Vec<(String, Vec<u8>, Option<u64>)>,
) {
    if depth > 6 {
        return;
    }
    let Ok(k) = LookupKey::new(dir.as_bytes(), Codec::Cp932) else {
        return;
    };
    let Ok(entries) = pack.list(&k) else { return };
    for e in entries {
        let base = dir.trim_end_matches('/');
        // 目录条目本身也是 CP932 名;这里先把**原始字节**拼出来。
        let mut full_bytes = Vec::new();
        if !base.is_empty() {
            full_bytes.extend_from_slice(base.as_bytes());
            full_bytes.push(b'/');
        }
        full_bytes.extend_from_slice(&e.name);
        let is_dir = matches!(e.kind, yu_vfs::EntryKind::Dir);
        if is_dir {
            // 目录名按 CP932 解码后再作为下一跳键(DataSource 内部按 CP932 归一)。
            let sub = cp932(&full_bytes);
            walk(pack, &format!("{sub}/"), depth + 1, keyword, out);
            continue;
        }
        if !e.name.iter().any(|b| *b >= 0x80) {
            continue;
        }
        let decoded = cp932(&full_bytes);
        if let Some(kw) = keyword {
            if !decoded.contains(kw) {
                continue;
            }
        }
        out.push((decoded, full_bytes, e.size));
    }
}

fn main() {
    let root = std::env::args().nth(1).expect("game root");
    let keyword = std::env::args().nth(2);
    let pack = YpfSource::open(format!("{root}/data.ypf"), Codec::Cp932).expect("data.ypf");

    let mut out: Vec<(String, Vec<u8>, Option<u64>)> = Vec::new();
    walk(&pack, "", 0, keyword.as_deref(), &mut out);
    println!("== 非 ASCII 名资产 {} 个 ==", out.len());
    for (decoded, raw, size) in out.iter().take(120) {
        let hex_head: String = raw.iter().take(24).map(|b| format!("{b:02x}")).collect();
        println!(
            "  {decoded}\n      bytes[{hex_head}...] size={size:?} lossy={}",
            String::from_utf8_lossy(raw)
        );
    }
}
