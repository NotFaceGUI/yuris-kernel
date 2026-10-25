//! 取证探针:商业包 ysbin 布局 / yst_list 索引↔文件号一致性 /
//! 与 demo 工程的核心脚本逐字节对比 / CP932 字符串解码。
//!
//! 用法:`cargo run -p yu-rt --example probe_pack_layout -- <game_root> [peer_root]`
use std::collections::BTreeSet;

use yu_vfs::{Codec, DataSource, LookupKey, YpfSource};

fn sha256_hex(bytes: &[u8]) -> String {
    // 自带最小 SHA-256(避免新增依赖):用 yu-cli 的?此处只做对比 → 用 FNV 不够,
    // 直接用长度 + 前 32 字节十六进制 + 简单校验和,足以判断"是否同一文件"。
    let mut sum: u64 = 0xcbf29ce484222325;
    for b in bytes {
        sum ^= u64::from(*b);
        sum = sum.wrapping_mul(0x100000001b3);
    }
    let head: String = bytes.iter().take(16).map(|b| format!("{b:02x}")).collect();
    format!("fnv={sum:016x} len={} head={head}", bytes.len())
}

fn cp932(bytes: &[u8]) -> String {
    yu_format::Codec::Cp932
        .decode(bytes, bytes)
        .unwrap_or_else(|_| format!("<坏字节 {:02x?}>", bytes))
}

fn main() {
    let root = std::env::args().nth(1).expect("game root");
    let peer = std::env::args().nth(2);
    let pack = YpfSource::open(format!("{root}/data.ypf"), Codec::Cp932).expect("data.ypf");

    // 1. 包内 ystNNNNN.ybn 清单。
    let list_key = LookupKey::new(b"ysbin/", Codec::Cp932).expect("list key");
    let entries = pack.list(&list_key).expect("list ysbin");
    let mut ybn_ids: BTreeSet<u32> = BTreeSet::new();
    let mut other = 0usize;
    for e in &entries {
        let name = String::from_utf8_lossy(&e.name);
        // list 返回的条目名相对于查询路径(不含 "ysbin/" 前缀)。
        let base = name.strip_prefix("ysbin/").unwrap_or(&name);
        if let Some(rest) = base.strip_prefix("yst") {
            if let Some(num) = rest.strip_suffix(".ybn") {
                if let Ok(id) = num.parse::<u32>() {
                    ybn_ids.insert(id);
                } else {
                    other += 1;
                }
            } else {
                other += 1;
            }
        } else {
            other += 1;
        }
    }
    println!("== 包内 ysbin/ 条目(前 15 名)==");
    for e in entries.iter().take(15) {
        println!("    kind={:?} size={:?} name={}", e.kind, e.size, cp932(&e.name));
    }
    println!("== 包内 ysbin/ 条目 ==");
    println!(
        "  yst*.ybn: {} 个(id {}..{}),其它: {}",
        ybn_ids.len(),
        ybn_ids.iter().next().copied().unwrap_or(0),
        ybn_ids.iter().next_back().copied().unwrap_or(0),
        other
    );
    let missing: Vec<u32> = (0..=*ybn_ids.iter().next_back().unwrap_or(&0))
        .filter(|i| !ybn_ids.contains(i))
        .collect();
    println!("  缺号(0..max): {:?}", &missing[..missing.len().min(40)]);

    println!("== 包内 movie / logo 类资产 ==");
    for prefix in ["movie/", "mov/", "data/movie"] {
        if let Ok(k) = LookupKey::new(prefix.as_bytes(), Codec::Cp932) {
            if let Ok(es) = pack.list(&k) {
                println!("  {prefix} → {} 条", es.len());
                for e in es.iter().take(10) {
                    println!("     {} ({:?})", cp932(&e.name), e.size);
                }
            }
        }
    }

    println!("== 包内条目名含「人生通行止め」(CP932)的资产 ==");
    {
        let needle: &[u8] = &[0x90, 0x6c, 0x90, 0xb6, 0x92, 0xca, 0x8d, 0x73];
        for prefix in ["", "data/", "cg/", "cgsys/", "movie/", "sound/"] {
            let Ok(k) = LookupKey::new(prefix.as_bytes(), Codec::Cp932) else { continue };
            let Ok(es) = pack.list(&k) else { continue };
            for e in &es {
                if e.name.windows(needle.len()).any(|w| w == needle) {
                    println!("  [{prefix}] {} ({:?})", cp932(&e.name), e.size);
                }
            }
        }
    }

    println!("== 包内条目名含 logo / title / open 的资产 ==");
    for prefix in ["", "data/", "cg/", "cgsys/", "system/", "movie/"] {
        let Ok(k) = LookupKey::new(prefix.as_bytes(), Codec::Cp932) else { continue };
        let Ok(es) = pack.list(&k) else { continue };
        for e in &es {
            let name = cp932(&e.name);
            let low = name.to_ascii_lowercase();
            if low.contains("logo") || low.contains("title") || low.contains("open") || low.contains("splash") {
                println!("  [{prefix}] {name} ({:?})", e.size);
            }
        }
    }

    // 2. yst_list 索引完整性 + 是否与文件号一一对应。
    let list_bytes = {
        let key = LookupKey::new(b"ysbin/yst_list.ybn", Codec::Cp932).expect("key");
        pack.open(&key).expect("yst_list")
    };
    let list = yu_format::ystl::ScriptList::parse(&list_bytes, b"ysbin/yst_list.ybn").expect("ystl");
    println!("== yst_list ==");
    println!("  条目 {}", list.entries.len());
    let list_ids: BTreeSet<u32> = list.entries.iter().map(|e| e.index).collect();
    let no_file: Vec<u32> = list_ids.iter().copied().filter(|i| !ybn_ids.contains(i)).collect();
    let no_entry: Vec<u32> = ybn_ids.iter().copied().filter(|i| !list_ids.contains(i)).collect();
    println!("  list 有但包内无 .ybn: {:?}", &no_file[..no_file.len().min(30)]);
    println!("  包内有 .ybn 但 list 无条目: {:?}", &no_entry[..no_entry.len().min(30)]);

    // 3. 关键脚本与 demo 逐字节对比。
    if let Some(peer) = peer {
        let ppack = YpfSource::open(format!("{peer}/data.ypf"), Codec::Cp932).expect("peer data.ypf");
        println!("== 与 peer 的脚本字节对比 ==");
        for sid in [3u32, 8, 10, 73, 84] {
            let key = LookupKey::new(format!("ysbin/yst{sid:05}.ybn").as_bytes(), Codec::Cp932)
                .expect("key");
            let a = pack.open(&key).map(|b| sha256_hex(&b)).unwrap_or_else(|e| format!("<{e}>"));
            let b = ppack.open(&key).map(|x| sha256_hex(&x)).unwrap_or_else(|e| format!("<{e}>"));
            println!("  s{sid:<4} game {a}");
            println!("        peer {b}   {}", if a == b { "同" } else { "**不同**" });
        }
        println!("== es_first / es_eris / es_charname 源码是否在包内 ==");
        for p in [
            "data/script/eris/es_first.yst",
            "data/script/eris/es_eris.yst",
            "data/script/eris/es_charname.yst",
            "data/script/userdefine/キャラ名定義.txt",
            "data/script/eris/es_define.txt",
        ] {
            let opened = LookupKey::new(p.as_bytes(), Codec::Cp932).and_then(|k| pack.open(&k));
            match opened {
                Ok(bytes) => println!("  有 {p}({} B)", bytes.len()),
                Err(e) => println!("  无 {p}({e})"),
            }
        }
    }

    // 4. 框架脚本 opcode 序列对比(证明"框架同源、内容各异")。
    if let Some(peer2) = std::env::args().nth(2) {
        let ppack2 =
            YpfSource::open(format!("{peer2}/data.ypf"), Codec::Cp932).expect("peer pack");
        println!("== 框架脚本 opcode 序列对比 ==");
        for sid in [3u32, 8, 10, 73] {
            let key = LookupKey::new(format!("ysbin/yst{sid:05}.ybn").as_bytes(), Codec::Cp932)
                .expect("key");
            let dump = |src: &YpfSource| -> Option<Vec<u8>> {
                let b = src.open(&key).ok()?;
                let sc = yu_rt::tables::ScriptFile::parse(sid, &b, yu_rt::tables::DEFAULT_SCRIPT_KEY)
                    .ok()?;
                Some((0..sc.group_count()).filter_map(|pc| sc.group(pc).map(|g| g.opcode)).collect())
            };
            let a = dump(&pack);
            let b = dump(&ppack2);
            match (a, b) {
                (Some(a), Some(b)) => println!(
                    "  s{sid:<4} opcode 序列: {} 条 vs {} 条 → {}",
                    a.len(),
                    b.len(),
                    if a == b { "完全相同" } else { "不同" }
                ),
                _ => println!("  s{sid:<4} 解析失败"),
            }
        }
    }

    // 5. s84 的字符串参数 CP932 解码(判断是不是真实角色名表)。
    let bytes84 = {
        let key = LookupKey::new(b"ysbin/yst00084.ybn", Codec::Cp932).expect("key");
        pack.open(&key).expect("yst00084")
    };
    let script =
        yu_rt::tables::ScriptFile::parse(84, &bytes84, yu_rt::tables::DEFAULT_SCRIPT_KEY)
            .expect("parse s84");
    println!("== s84 前 8 条的字符串载荷(CP932 解码)==");
    for pc in 0..8u32 {
        let Some(g) = script.group(pc as usize) else { break };
        let line = script.source_position(pc as usize).unwrap_or(0);
        print!("  pc{pc} L{line} op{} ", g.opcode);
        for w in script.group_windows(pc as usize) {
            let Some(payload) = script.window_payload(w) else { continue };
            // 字符串窗口:0x22 引号后的字节。
            if let Some(pos) = payload.iter().position(|b| *b == 0x22) {
                let raw: Vec<u8> = payload[pos + 1..]
                    .iter()
                    .copied()
                    .take_while(|b| *b != 0x22)
                    .collect();
                if !raw.is_empty() {
                    print!(" | {}", cp932(&raw));
                }
            }
            // 词文本(WORD 形态:0x27 引号)。
            if let Some(pos) = payload.iter().position(|b| *b == 0x27) {
                let raw: Vec<u8> = payload[pos + 1..]
                    .iter()
                    .copied()
                    .take_while(|b| *b != 0x27)
                    .collect();
                if !raw.is_empty() {
                    print!(" | WORD={:?}", cp932(&raw));
                }
            }
        }
        println!();
    }
}
