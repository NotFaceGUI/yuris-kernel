//! 排查示例(主人指示):封包内到底有没有 YSCom.ycd、在哪个路径。
//! 列出全部条目,按子串(ycd/yscom,ASCII 不分大小写)过滤;
//! 另给对照文件时报告字节是否一致。
use yu_vfs::{DataSource, YpfSource};

fn main() {
    let pack = std::env::args().nth(1).expect("data.ypf 路径");
    let ypf = YpfSource::open(&pack, yu_vfs::Codec::Cp932).expect("打开封包");
    let entries = ypf.entries();
    println!("{}: 共 {} 条目", pack, entries.len());
    let mut hits = 0;
    for e in entries {
        let lower: Vec<u8> = e.name.iter().map(|b| b.to_ascii_lowercase()).collect();
        let text = String::from_utf8_lossy(&lower);
        if text.contains("ycd") || text.contains("yscom") {
            let normalized = String::from_utf8_lossy(e.normalized.as_bytes());
            println!(
                "  命中: 原名 {:?} → 归一键 {} (hash_ok={})",
                String::from_utf8_lossy(&e.name),
                normalized,
                e.name_hash_ok
            );
            hits += 1;
        }
    }
    if hits == 0 {
        println!("  无 ycd/yscom 条目");
    }
    // 对照:可选第二参数 = 磁盘上的 ysc.ybn,与包内 ysbin/ysc.ybn 比字节。
    if let Some(file) = std::env::args().nth(2) {
        let key = yu_vfs::LookupKey::new(b"ysbin/ysc.ybn", yu_vfs::Codec::Cp932).unwrap();
        let in_pack = ypf.open(&key).expect("包内 ysbin/ysc.ybn");
        let on_disk = std::fs::read(&file).expect("读对照文件");
        println!(
            "对照 ysbin/ysc.ybn: 包内 {} 字节 vs {} {} 字节 → {}",
            in_pack.len(),
            file,
            on_disk.len(),
            if in_pack == on_disk {
                "逐字节一致"
            } else {
                "不一致"
            }
        );
    }
}
