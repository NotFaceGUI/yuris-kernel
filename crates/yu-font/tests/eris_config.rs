//! Acceptance against the compiled ERIS FONT commands (Phase 0 evidence).
//!
//! Requires `YURIS_SAMPLE_ERIS_YSBIN`; when absent the test is skipped and
//! counted, and `YURIS_REQUIRE_SAMPLES=1` turns that into a hard failure.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use yu_format::key::DEFAULT_KEY_BYTES;
use yu_format::ystb::{self, Segments};
use yu_format::{Codec, CommandTable};

fn ysbin() -> Option<PathBuf> {
    std::env::var_os("YURIS_SAMPLE_ERIS_YSBIN").map(PathBuf::from)
}

fn literal(payload: &[u8]) -> Option<i64> {
    let mut p = 0usize;
    let mut value = None;
    while p + 3 <= payload.len() {
        let op = payload[p];
        let len = u16::from_le_bytes([payload[p + 1], payload[p + 2]]) as usize;
        let end = p + 3 + len;
        if end > payload.len() {
            return None;
        }
        let body = &payload[p + 3..end];
        value = Some(match op {
            0x42 if !body.is_empty() => i64::from(body[0] as i8),
            0x57 if body.len() >= 2 => i64::from(i16::from_le_bytes([body[0], body[1]])),
            0x49 if body.len() >= 4 => {
                i64::from(i32::from_le_bytes([body[0], body[1], body[2], body[3]]))
            }
            0x4c if body.len() >= 8 => i64::from_le_bytes(body[..8].try_into().ok()?),
            _ => return None,
        });
        p = end;
    }
    if p == payload.len() {
        value
    } else {
        None
    }
}

fn font_params(dir: &Path, script: &str, group_no: usize) -> BTreeMap<u8, i64> {
    let ysc = CommandTable::parse(&std::fs::read(dir.join("ysc.ybn")).unwrap(), b"ysc").unwrap();
    let bytes = std::fs::read(dir.join(script)).unwrap();
    let segs = Segments::decrypt(&bytes, &DEFAULT_KEY_BYTES, script.as_bytes()).unwrap();
    let decoded = ystb::decode(&segs, 0, &ysc, Codec::Cp932, script.as_bytes()).unwrap();
    let g = &decoded.groups[group_no];
    assert_eq!(g.command, "FONT", "{script} group {group_no}");
    let mut out = BTreeMap::new();
    for w in &g.windows {
        let param = w.bytes[0];
        let length = u32::from_le_bytes([w.bytes[4], w.bytes[5], w.bytes[6], w.bytes[7]]) as usize;
        let offset =
            u32::from_le_bytes([w.bytes[8], w.bytes[9], w.bytes[10], w.bytes[11]]) as usize;
        let payload = &segs.args_data[offset..offset + length];
        if let Some(v) = literal(payload) {
            out.insert(param, v);
        }
    }
    out
}

#[test]
fn eris_text_and_system_font_configs_are_pinned() {
    let Some(dir) = ysbin() else {
        if std::env::var_os("YURIS_REQUIRE_SAMPLES").is_some_and(|v| v != "0" && !v.is_empty()) {
            panic!("YURIS_REQUIRE_SAMPLES=1 but YURIS_SAMPLE_ERIS_YSBIN is unset");
        }
        println!("SKIP[font:eris] YURIS_SAMPLE_ERIS_YSBIN unset");
        return;
    };

    // es_text.yst dialogue style: 12x12, grey, AA off.
    let g1517 = font_params(&dir, "yst00071.ybn", 1517);
    assert_eq!(g1517.get(&9), Some(&12), "SX");
    assert_eq!(g1517.get(&10), Some(&12), "SY");
    assert_eq!(g1517.get(&4), Some(&0x99_9999), "COLOR");
    assert_eq!(g1517.get(&5), Some(&0x99_9999), "COLOR2");
    assert_eq!(g1517.get(&11), Some(&0), "BOLD");
    assert_eq!(g1517.get(&3), Some(&0), "ANTIALIASING");

    // Bold/AA variant.
    let g1698 = font_params(&dir, "yst00071.ybn", 1698);
    assert_eq!(g1698.get(&11), Some(&1), "BOLD");
    assert_eq!(g1698.get(&3), Some(&1), "ANTIALIASING");

    // Documented white->yellow gradient pair.
    let g1606 = font_params(&dir, "yst00071.ybn", 1606);
    assert_eq!(g1606.get(&4), Some(&0xff_ffff), "COLOR");
    assert_eq!(g1606.get(&5), Some(&0xff_ff00), "COLOR2");

    // es_system.yst: DRAWMODE=1, BOLD=1, SX=14, SY=15.
    let g132 = font_params(&dir, "yst00025.ybn", 132);
    assert_eq!(g132.get(&8), Some(&1), "DRAWMODE");
    assert_eq!(g132.get(&11), Some(&1), "BOLD");
    assert_eq!(g132.get(&9), Some(&14), "SX");
    assert_eq!(g132.get(&10), Some(&15), "SY");
}
