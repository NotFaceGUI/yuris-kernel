//! 找出 RETURNCODE 非空载荷窗口的形态(kind/len/bytes)。
use yu_rt::tables::{ScriptFile, DEFAULT_SCRIPT_KEY};
fn main() {
    let dir = std::env::var("YURIS_SAMPLE_ERIS_YSBIN").expect("set YURIS_SAMPLE_ERIS_YSBIN");
    let mut n = 0;
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
        let sf = ScriptFile::parse(sid, &data, DEFAULT_SCRIPT_KEY).unwrap();
        for pc in 0..sf.group_count() {
            let g = sf.group(pc).unwrap();
            if g.opcode != 79 {
                continue; // RETURNCODE
            }
            for w in sf.group_windows(pc) {
                if let Some(p) = sf.window_payload(w) {
                    if !p.is_empty() {
                        println!(
                            "{name} pc={pc} kind={} flag={} slot={} len={} bytes={p:02x?}",
                            w.kind, w.flag, w.slot, w.length
                        );
                        n += 1;
                    }
                }
            }
        }
    }
    println!("total non-empty RETURNCODE windows = {n}");
}
