//! 取证探针(DIV-RT-050):按 pc 区间倾倒脚本命令(命令名/源码行/参数+载荷反汇编)。
//!
//! 用法: `dump_pc_cmds <sid> <pc_start> <pc_end>`
//! 载荷指令流按 `[op, u16le len, data[len]]` 逐条解码(与 expr.rs RawInstr 同构)。
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DataSource, DirSource};

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

fn parse_script(sid: u32) -> Result<yu_rt::tables::ScriptFile, String> {
    let root = eris_root();
    let pack = yu_vfs::YpfSource::open(format!("{root}/data.ypf"), Codec::Cp932);
    let (bytes, origin): (Vec<u8>, String) = match pack {
        Ok(p) => {
            let key = yu_vfs::LookupKey::new(format!("ysbin/yst{sid:05}.ybn").as_bytes(), Codec::Cp932)
                .map_err(|d| format!("yst{sid:05}: {d}"))?;
            let b = p.open(&key).map_err(|d| format!("yst{sid:05}: {d}"))?;
            (b, format!("{root}/data.ypf!ysbin/yst{sid:05}.ybn"))
        }
        Err(_) => {
            let p = format!("{}/yst{sid:05}.ybn", ysbin_dir());
            let b = std::fs::read(&p).map_err(|e| format!("{p}: {e}"))?;
            (b, format!("{p} (散盘)"))
        }
    };
    println!("# 源: {origin}");
    yu_rt::tables::ScriptFile::parse(sid, &bytes, yu_rt::tables::DEFAULT_SCRIPT_KEY)
        .map_err(|d| format!("yst{sid:05}: {d}"))
}

fn boot_vm(root: &str) -> Vm {
    // 数据入口唯一约定:散盘 `<project>/data/`(2026-10-20 起不再识别 save/data)。
    let src = DirSource::open(format!("{root}/data"), Codec::Cp932).unwrap();
    let aux = DirSource::open(root, Codec::Cp932).unwrap();
    Vm::boot_with_aux(
        Box::new(src),
        Some(Box::new(aux)),
        BootConfig {
            entry_label: "SYSTEM_START".into(),
            ..BootConfig::default()
        },
    )
    .unwrap()
}

/// 载荷指令流 → 可读串。已知 op 尽量给出语义,未知保留 hex。
fn disas(payload: &[u8]) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i + 3 <= payload.len() {
        let op = payload[i];
        let n = u16::from_le_bytes([payload[i + 1], payload[i + 2]]) as usize;
        let Some(data) = payload.get(i + 3..i + 3 + n) else { break };
        i += 3 + n;
        let s = match op {
            0x42 if n == 1 => format!("i{}", data[0] as i8),
            0x57 if n == 2 => format!("i{}", i16::from_le_bytes([data[0], data[1]])),
            0x49 if n == 4 => format!("i{}", i32::from_le_bytes(data.try_into().unwrap())),
            0x4c if n == 8 => format!("i{}", i64::from_le_bytes(data.try_into().unwrap())),
            0x46 if n == 8 => format!("f{}", f64::from_le_bytes(data.try_into().unwrap())),
            0x4d => match yu_rt::expr::decode_string_literal(data) {
                Ok(b) => {
                    let txt = yu_format::codec::Codec::Cp932
                        .decode(&b, b"dump")
                        .unwrap_or_default();
                    format!("s\"{txt}\"")
                }
                Err(_) => format!("s?{:02x?}", data),
            },
            0x48 | 0x56 | 0x76 if n == 3 => {
                let id = u16::from_le_bytes([data[1], data[2]]);
                format!("v({}#{id})", data[0] as char)
            }
            // 栈式二元/逻辑算子(载荷空),给出短名便于读 IF 条件
            0x41 => "&".into(),
            0x4f => "|".into(),
            0x5e => "^".into(),
            0x53 => "<=".into(),
            0x5a => ">=".into(),
            0x58 => "<".into(),
            0x5b => ">".into(),
            0x54 => "==".into(),
            0x55 => "!=".into(),
            0x29 => "at[]".into(),
            _ => format!("op{op:02x}({n}:{:02x?})", data),
        };
        parts.push(s);
    }
    if i < payload.len() {
        parts.push(format!("tail{:02x?}", &payload[i..]));
    }
    parts.join(" ")
}

fn main() {
    let sid: u32 = std::env::args().nth(1).unwrap().parse().unwrap();
    let start: u32 = std::env::args().nth(2).unwrap().parse().unwrap();
    let end: u32 = std::env::args().nth(3).unwrap().parse().unwrap();
    let script = parse_script(sid).expect("parse");
    let vm = boot_vm(&eris_root());
    let yscm = &vm.tables().boot.yscm;
    for pc in start..=end.min(script.group_count() as u32 - 1) {
        let Some(g) = script.group(pc as usize) else { continue };
        let name = yscm.command(g.opcode).map(|c| c.name.clone()).unwrap_or_default();
        let line = script.source_position(pc as usize).unwrap_or(0);
        let wins: Vec<String> = script
            .group_windows(pc as usize)
            .iter()
            .map(|w| {
                let pname = yscm
                    .param(g.opcode, w.kind)
                    .map(|p| p.name.clone())
                    .unwrap_or_else(|| format!("k{}", w.kind));
                let pl = script.window_payload(w).unwrap_or(&[]);
                let comp = w.slot >> 8;
                let comp_s = if comp == 0 { String::new() } else { format!("~{comp}") };
                format!("{pname}{comp_s}(len{})={}", w.length, disas(pl))
            })
            .collect();
        println!("pc{pc:<5} L{line:<5} {name:<10} {}", wins.join(" | "));
    }
}
