//! 取证探针:全脚本扫描「谁写/读了某个变量」。
//!
//! 用法:`cargo run -p yu-rt --example probe_var_refs -- <game_root> <id_hex> [space]`
//!
//! `space`: `@`(默认) / `$` / `%`。
//! 判据:窗口载荷里的变量引用指令 `[0x48|0x56|0x76, space, id_lo, id_hi]`
//! (0x56/0x76 = 引用 ⇒ 出现在 LET/GOSUB 输出等左值位置;0x48 = 取值)。
use std::collections::BTreeSet;

use yu_rt::tables::{LabelTable, ScriptFile, DEFAULT_SCRIPT_KEY};
use yu_vfs::{Codec, DataSource, LookupKey, YpfSource};

fn main() {
    let root = std::env::args().nth(1).expect("game root");
    let id = u16::from_str_radix(
        std::env::args()
            .nth(2)
            .expect("var id (hex)")
            .trim_start_matches("0x"),
        16,
    )
    .expect("hex id");
    let space_ch = std::env::args()
        .nth(3)
        .and_then(|s| s.chars().next())
        .unwrap_or('@');
    let space = space_ch as u8;

    let pack = YpfSource::open(format!("{root}/data.ypf"), Codec::Cp932).expect("data.ypf");
    let ysl = {
        let key = LookupKey::new(b"ysbin/ysl.ybn", Codec::Cp932).expect("key");
        pack.open(&key).expect("ysl")
    };
    let fmt = yu_format::yslb::YslLabels::parse(&ysl, b"ysbin/ysl.ybn").expect("parse ysl");
    let labels = LabelTable::from_format(fmt).expect("labels");
    let sids: BTreeSet<u16> = labels.iter().map(|l| l.script_id).collect();

    let yscm_names: Vec<(u8, String)> = {
        // 只需要 opcode → 名字;从 ysc.ybn 解。
        let key = LookupKey::new(b"ysbin/ysc.ybn", Codec::Cp932).expect("key");
        let bytes = pack.open(&key).expect("ysc");
        let table = yu_format::yscm::CommandTable::parse(&bytes, b"ysbin/ysc.ybn").expect("parse ysc");
        table
            .commands
            .iter()
            .enumerate()
            .map(|(i, c)| (i as u8, c.name.to_string()))
            .collect()
    };

    let needle = [space, (id & 0xff) as u8, (id >> 8) as u8];
    let mut hits = 0usize;
    for sid in sids {
        let key = match LookupKey::new(format!("ysbin/yst{sid:05}.ybn").as_bytes(), Codec::Cp932) {
            Ok(k) => k,
            Err(_) => continue,
        };
        let Ok(bytes) = pack.open(&key) else { continue };
        let Ok(script) = ScriptFile::parse(u32::from(sid), &bytes, DEFAULT_SCRIPT_KEY) else {
            continue;
        };
        for pc in 0..script.group_count() {
            let Some(g) = script.group(pc) else { continue };
            let name = yscm_names
                .iter()
                .find(|(op, _)| *op == g.opcode)
                .map(|(_, n)| n.clone())
                .unwrap_or_else(|| format!("#{}", g.opcode));
            for w in script.group_windows(pc) {
                let Some(payload) = script.window_payload(w) else {
                    continue;
                };
                let mut i = 0usize;
                while i + 3 + 2 <= payload.len() {
                    let op = payload[i];
                    let n = u16::from_le_bytes([payload[i + 1], payload[i + 2]]) as usize;
                    if i + 3 + n > payload.len() {
                        break;
                    }
                    let data = &payload[i + 3..i + 3 + n];
                    if matches!(op, 0x48 | 0x56 | 0x76) && n == 3 && data == needle {
                        println!(
                            "s{sid:<4} pc{pc:<5} {name:<12} win{} {}",
                            w.kind,
                            if op == 0x48 { "READ" } else { "REF" }
                        );
                        hits += 1;
                    }
                    i += 3 + n;
                }
            }
        }
    }
    println!("== hits {hits} ==");
}
