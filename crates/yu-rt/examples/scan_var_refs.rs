//! 取证探针(DIV-RT-050):全语料扫描引用指定 @/$ 变量的命令位置。
//! 用法: `scan_var_refs <十进制varid> [@|$]`(默认 @)。
//! 匹配载荷中的变量引用指令 `48 03 00 <prefix> <id_le16>`(0x48=读/0x56/0x76=引用)。
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

fn parse_script(sid: u32) -> Option<ScriptFile> {
    let root = eris_root();
    let pack = yu_vfs::YpfSource::open(format!("{root}/data.ypf"), Codec::Cp932).ok()?;
    let key = yu_vfs::LookupKey::new(format!("ysbin/yst{sid:05}.ybn").as_bytes(), Codec::Cp932).ok()?;
    let bytes = pack.open(&key).ok()?;
    ScriptFile::parse(sid, &bytes, yu_rt::tables::DEFAULT_SCRIPT_KEY).ok()
}

fn main() {
    let id: u16 = std::env::args().nth(1).unwrap().parse().unwrap();
    let prefix: u8 = match std::env::args().nth(2).as_deref() {
        Some("$") => 0x24,
        _ => 0x40,
    };
    let needle: &[u8] = &[0x03, 0x00, prefix, (id & 0xff) as u8, (id >> 8) as u8];
    for sid in 0..141u32 {
        let Some(script) = parse_script(sid) else { continue };
        for pc in 0..script.group_count() {
            let Some(g) = script.group(pc) else { continue };
            for (wi, w) in script.group_windows(pc).iter().enumerate() {
                let Some(p) = script.window_payload(w) else { continue };
                // 指令流里任意位置出现该变量引用(48/56/76 开头的 5 字节序列)
                let mut i = 0usize;
                while i + 3 <= p.len() {
                    let n = u16::from_le_bytes([p[i + 1], p[i + 2]]) as usize;
                    let Some(d) = p.get(i + 3..i + 3 + n) else { break };
                    if matches!(p[i], 0x48 | 0x56 | 0x76) && d == &needle[2..] {
                        let line = script.source_position(pc).unwrap_or(0);
                        println!("s{sid} pc{pc} L{line} op{} w{wi} refop{:#04x}", g.opcode, p[i]);
                    }
                    i += 3 + n;
                }
            }
        }
    }
}
