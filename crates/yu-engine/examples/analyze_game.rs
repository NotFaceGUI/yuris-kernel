//! 只读游戏分析探针:
//! - 打印 yscfg 版本/宽高/7 个文件优先级原始值;
//! - 从当前挂载栈读 ysc/yst_list,遍历脚本的 WORD 文本;
//! - 每段分别按 CP932/GBK 严格解码,统计成功/失败并打印样本。
//!
//! 用法:`analyze_game <project-root> [max-scripts]`

use std::path::Path;

use yu_engine::project::Project;
use yu_format::codec::Codec;
use yu_format::{yscf, yscm, ystb, ystl};
use yu_vfs::LookupKey;

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let max_scripts: usize = std::env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(40);
    let project = Project::discover(Path::new(&root), None).expect("project");
    let key = project.resolve_script_key(None).expect("script key");
    let source = project.open_source().expect("source");

    let read = |name: &str| -> Option<Vec<u8>> {
        let k = LookupKey::new(name.as_bytes(), yu_engine::project::CODEC).ok()?;
        source.open(&k).ok()
    };

    if let Some(bytes) = read("ysbin/yscfg.ybn") {
        match yscf::Yscf::parse(&bytes, b"yscfg") {
            Ok(cfg) => println!(
                "yscfg: version={} {}x{} flag28={} image_order={:?} sound_order={:?} priority_raw={:?} title={:?}",
                cfg.version,
                cfg.width,
                cfg.height,
                cfg.flag_28,
                cfg.image_ext_order,
                cfg.sound_ext_order,
                cfg.file_priority_raw,
                Codec::Cp932.decode(&cfg.title, b"title").unwrap_or_default()
            ),
            Err(d) => println!("yscfg parse failed: {d}"),
        }
    }

    let table = read("ysbin/ysc.ybn").expect("ysc.ybn");
    let table = yscm::CommandTable::parse(&table, b"ysc.ybn").expect("ysc parse");
    let list = read("ysbin/yst_list.ybn").expect("yst_list.ybn");
    let list = ystl::ScriptList::parse(&list, b"yst_list.ybn").expect("yst_list parse");
    println!("ysc.version={} scripts={}", table.version, list.entries.len());
    for e in list.entries.iter().take(12) {
        println!(
            "  s{:>5} words={:>5} path_cp932={:?} path_gbk={:?}",
            e.index,
            e.text_count,
            Codec::Cp932.decode(&e.name, b"path"),
            Codec::Gbk.decode(&e.name, b"path")
        );
    }

    let dir = LookupKey::new(b"ysbin", yu_engine::project::CODEC).expect("ysbin");
    let mut names: Vec<Vec<u8>> = source
        .list(&dir)
        .unwrap_or_default()
        .into_iter()
        .filter(|e| {
            let n = &e.name;
            n.len() == 12
                && n.starts_with(b"yst")
                && n.ends_with(b".ybn")
                && n[3..8].iter().all(u8::is_ascii_digit)
        })
        .map(|e| e.name)
        .collect();
    names.sort();
    names.truncate(max_scripts);

    let (mut cp_ok, mut cp_bad, mut gb_ok, mut gb_bad) = (0usize, 0usize, 0usize, 0usize);
    let mut printed = 0usize;
    for name in names {
        let mut raw = b"ysbin/".to_vec();
        raw.extend_from_slice(&name);
        let Some(data) = LookupKey::new(&raw, yu_engine::project::CODEC)
            .ok()
            .and_then(|k| source.open(&k).ok())
        else {
            continue;
        };
        let Ok(segs) = ystb::Segments::decrypt(&data, &key.bytes, &raw) else {
            continue;
        };
        let sid: u32 = std::str::from_utf8(&name[3..8])
            .unwrap()
            .parse()
            .unwrap_or(0);
        let Ok(script) = ystb::decode(&segs, sid, &table, Codec::Cp932, &raw) else {
            continue;
        };
        for g in &script.groups {
            if g.command.as_ref() != "WORD" {
                continue;
            }
            let Some(w) = g.windows.first() else { continue };
            let len = u32::from_le_bytes(w.bytes[4..8].try_into().unwrap()) as usize;
            let off = u32::from_le_bytes(w.bytes[8..12].try_into().unwrap()) as usize;
            let Some(bytes) = segs.args_data.get(off..off.saturating_add(len)) else {
                continue;
            };
            if bytes.is_empty() {
                continue;
            }
            let cp = Codec::Cp932.decode(bytes, b"word");
            let gb = Codec::Gbk.decode(bytes, b"word");
            if cp.is_ok() {
                cp_ok += 1
            } else {
                cp_bad += 1
            }
            if gb.is_ok() {
                gb_ok += 1
            } else {
                gb_bad += 1
            }
            if printed < 24 {
                printed += 1;
                println!(
                    "\n[{} s{}] len={} hex={}\n  cp932={:?}\n  gbk  ={:?}",
                    String::from_utf8_lossy(&name),
                    sid,
                    bytes.len(),
                    bytes
                        .iter()
                        .take(96)
                        .map(|b| format!("{b:02x}"))
                        .collect::<String>(),
                    cp.unwrap_or_else(|_| "<invalid>".into()),
                    gb.unwrap_or_else(|_| "<invalid>".into())
                );
            }
        }
    }
    println!(
        "\nWORD decode: cp932 ok={cp_ok} bad={cp_bad}; gbk ok={gb_ok} bad={gb_bad}"
    );
}
