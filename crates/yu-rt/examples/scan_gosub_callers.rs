//! 取证探针(DIV-RT-050):全语料扫描 GOSUB/GO/TASK 的 #-标签窗口,
//! 列出调用指定标签的全部 (script, pc, 命令) 位置。
//! 用法: `scan_gosub_callers <label子串> [label子串2 ...]`
use yu_rt::tables::ScriptFile;
use yu_vfs::{Codec, DataSource};

fn eris_root() -> String {
    std::env::var("YURIS_SAMPLE_ERIS_ROOT")
        .or_else(|_| {
            std::env::var("YURIS_SAMPLE_ERIS_YSBIN").map(|p| {
                std::path::Path::new(&p)
                    .ancestors()
                    .nth(2)
                    .unwrap()
                    .display()
                    .to_string()
            })
        })
        .expect("set YURIS_SAMPLE_ERIS_ROOT or YURIS_SAMPLE_ERIS_YSBIN")
}

fn ysbin_dir() -> String {
    std::env::var("YURIS_SAMPLE_ERIS_YSBIN")
        .or_else(|_| std::env::var("YURIS_SAMPLE_ERIS_ROOT").map(|r| format!("{r}/save/data/ysbin")))
        .expect("set YURIS_SAMPLE_ERIS_YSBIN or ROOT")
}

fn parse_script(sid: u32) -> Option<ScriptFile> {
    let root = eris_root();
    let pack = yu_vfs::YpfSource::open(format!("{root}/data.ypf"), Codec::Cp932).ok()?;
    let key = yu_vfs::LookupKey::new(format!("ysbin/yst{sid:05}.ybn").as_bytes(), Codec::Cp932).ok()?;
    let bytes = pack.open(&key).ok()?;
    ScriptFile::parse(sid, &bytes, yu_rt::tables::DEFAULT_SCRIPT_KEY).ok()
}

/// 窗口载荷里的字符串字面量(0x4d 指令)。
fn window_strings(script: &ScriptFile, pc: usize) -> Vec<String> {
    let mut out = Vec::new();
    for w in script.group_windows(pc) {
        let Some(p) = script.window_payload(w) else { continue };
        let mut i = 0usize;
        while i + 3 <= p.len() {
            let n = u16::from_le_bytes([p[i + 1], p[i + 2]]) as usize;
            let Some(d) = p.get(i + 3..i + 3 + n) else { break };
            if p[i] == 0x4d {
                if let Ok(b) = yu_rt::expr::decode_string_literal(d) {
                    if let Ok(s) = yu_format::codec::Codec::Cp932.decode(&b, b"scan") {
                        out.push(s);
                    }
                }
            }
            i += 3 + n;
        }
    }
    out
}

fn main() {
    let needles: Vec<String> = std::env::args().skip(1).collect();
    assert!(!needles.is_empty(), "用法: scan_gosub_callers <标签子串>...");
    for sid in 0..141u32 {
        let Some(script) = parse_script(sid) else { continue };
        for pc in 0..script.group_count() {
            let Some(g) = script.group(pc) else { continue };
            // 只看跳转/任务类命令(GOSUB/GO/TASK);按窗口内容判,不依赖命令名。
            let strs = window_strings(&script, pc);
            for s in &strs {
                if needles.iter().any(|n| s.contains(n.as_str())) {
                    let line = script.source_position(pc).unwrap_or(0);
                    println!(
                        "s{sid} pc{pc} L{line} op{} strings={strs:?}",
                        g.opcode
                    );
                    break;
                }
            }
        }
    }
}
