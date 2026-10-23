//! 找 TASK 带 TINT(kind=4)窗口的脚本/组/源码行 + 该组全窗口。
use yu_rt::tables::{ScriptFile, DEFAULT_SCRIPT_KEY};
fn main() {
    let dir = std::env::var("YURIS_SAMPLE_ERIS_YSBIN").expect("set YURIS_SAMPLE_ERIS_YSBIN");
    for e in std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()) {
        let name = e.file_name().to_string_lossy().into_owned();
        if !(name.len() == 12
            && name.starts_with("yst")
            && name.ends_with(".ybn")
            && name[3..8].bytes().all(|b| b.is_ascii_digit()))
        {
            continue;
        }
        let sid: u32 = name[3..8].parse().unwrap();
        let data = std::fs::read(e.path()).unwrap();
        let Ok(sf) = ScriptFile::parse(sid, &data, DEFAULT_SCRIPT_KEY) else {
            continue;
        };
        for pc in 0..sf.group_count() {
            let g = sf.group(pc).unwrap();
            if g.opcode != 93 {
                continue; // TASK
            }
            let has_tint = sf.group_windows(pc).iter().any(|w| w.kind == 4);
            if !has_tint {
                continue;
            }
            println!(
                "{name} pc={pc} line={} param={}",
                sf.source_position(pc).unwrap_or(0),
                g.param
            );
            for w in sf.group_windows(pc) {
                println!("  kind={} slot={} len={}", w.kind, w.slot, w.length);
            }
        }
    }
}
