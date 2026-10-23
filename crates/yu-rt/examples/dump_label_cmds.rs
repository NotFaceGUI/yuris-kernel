//! 取证探针:倾倒语料脚本的解码命令(组/窗口 kind、参数名、载荷十六进制)。
//!
//! 用法:
//! - `dump_label_cmds <label> [n=8]` —— 从标签起倒 n 条命令;
//! - `dump_label_cmds scan:<sid>`    —— 扫该脚本全部 VARACT(含源码行与窗口)。
use yu_rt::input::InputTimeline;
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
        .or_else(|_| {
            std::env::var("YURIS_SAMPLE_ERIS_ROOT").map(|r| format!("{r}/save/data/ysbin"))
        })
        .expect("set YURIS_SAMPLE_ERIS_YSBIN or ROOT")
}

fn parse_script(sid: u32) -> Result<yu_rt::tables::ScriptFile, String> {
    // 与运行时同口径:封包 `data.ypf` 优先,散盘 `save/data/ysbin` 为备。
    let root = eris_root();
    let pack = yu_vfs::YpfSource::open(format!("{root}/data.ypf"), Codec::Cp932);
    let (bytes, origin): (Vec<u8>, String) = match pack {
        Ok(p) => {
            let key =
                yu_vfs::LookupKey::new(format!("ysbin/yst{sid:05}.ybn").as_bytes(), Codec::Cp932)
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
    let src = DirSource::open(format!("{root}/save/data"), Codec::Cp932).unwrap();
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

/// 全语料扫描:MATH/VARACT 的旗标形态(存在性判定的语料级核对)。
fn scan_math(root: &str) {
    let vm = boot_vm(root);
    let yscm = |op: u8, kind: u8| -> String {
        vm.tables()
            .boot
            .yscm
            .param(op, kind)
            .map(|p| p.name.clone())
            .unwrap_or_default()
    };
    // 立即数窗口载荷:`42 01 00 v`(PushInt i8)/`43 ...`(i32)。
    let imm = |payload: &[u8]| -> Option<i64> {
        match payload.first() {
            Some(0x42) if payload.len() >= 4 => Some(i64::from(i8::from_be_bytes([payload[3]]))),
            Some(0x43) if payload.len() >= 7 => {
                Some(i32::from_be_bytes([payload[3], payload[4], payload[5], payload[6]]) as i64)
            }
            _ => None,
        }
    };
    let mut math_total = 0u64;
    let mut math_flag0: Vec<String> = Vec::new();
    let mut math_setstr = 0u64;
    let mut varact_init0 = 0u64;
    let mut varact_g0 = 0u64;
    for sid in 0..141u32 {
        let Ok(script) = parse_script(sid) else {
            continue;
        }; // 语料本就跳号
        for pc in 0..script.group_count() as u32 {
            let Some(g) = script.group(pc as usize) else {
                continue;
            };
            let line = script.source_position(pc as usize).unwrap_or(0);
            match g.opcode {
                61 => {
                    // MATH
                    math_total += 1;
                    for w in script.group_windows(pc as usize) {
                        let pname = yscm(g.opcode, w.kind);
                        if pname.is_empty() {
                            continue;
                        }
                        if pname == "SETSTR" {
                            math_setstr += 1;
                        }
                        if w.kind >= 4 {
                            match script.window_payload(w).and_then(imm) {
                                Some(0) => {
                                    math_flag0.push(format!("s{sid} pc{pc} L{line} {}=0", pname))
                                }
                                Some(v) if v != 1 => {
                                    math_flag0.push(format!("s{sid} pc{pc} L{line} {}={v}", pname))
                                }
                                _ => {}
                            }
                        }
                    }
                }
                100 => {
                    // VARACT
                    for w in script.group_windows(pc as usize) {
                        let pname = yscm(g.opcode, w.kind);
                        match (pname.as_str(), script.window_payload(w).and_then(imm)) {
                            ("INIT", Some(0)) => {
                                varact_init0 += 1;
                                println!("INIT=0 @ s{sid} pc{pc} L{line}");
                            }
                            (p, Some(0)) if p.starts_with("G_") => {
                                varact_g0 += 1;
                                println!("{p}=0 @ s{sid} pc{pc} L{line}");
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
    }
    println!(
        "MATH 总数={math_total}, SETSTR 窗口={math_setstr}, 旗标非 1 值={} 条:",
        math_flag0.len()
    );
    for l in math_flag0.iter().take(20) {
        println!("  {l}");
    }
    println!("VARACT INIT=0:{varact_init0} G_*=0:{varact_g0}");
    let _ = root;
}

fn scan(sid: u32, root: &str) {
    let Ok(script) = parse_script(sid) else {
        eprintln!("脚本 {sid} 解析失败");
        return;
    };
    let vm = boot_vm(root);
    for pc in 0..script.group_count() as u32 {
        let Some(g) = script.group(pc as usize) else {
            continue;
        };
        let name = vm
            .tables()
            .boot
            .yscm
            .command(g.opcode)
            .map(|c| c.name.clone())
            .unwrap_or_default();
        if name != "VARACT" {
            continue;
        }
        let line = script.source_position(pc as usize).unwrap_or(0);
        let wins: Vec<String> = script
            .group_windows(pc as usize)
            .iter()
            .map(|w| {
                let pname = vm
                    .tables()
                    .boot
                    .yscm
                    .param(g.opcode, w.kind)
                    .map(|p| p.name.clone())
                    .unwrap_or_else(|| "-".into());
                format!(
                    "{pname}(kind{},{} {:02x?})",
                    w.kind,
                    w.length,
                    script.window_payload(w).unwrap_or(&[])
                )
            })
            .collect();
        println!("pc={pc} line={line} VARACT {}", wins.join(" "));
    }
}

fn main() {
    let root = eris_root();
    let arg = std::env::args().nth(1).expect("label 或 scan:<sid>");
    if arg == "scanmath" {
        return scan_math(&root);
    }
    if let Some(op) = arg.strip_prefix("scanparams:") {
        return scan_params(op.parse().expect("opcode"), &root);
    }
    if let Some(sid) = arg.strip_prefix("scan:") {
        return scan(sid.parse().expect("sid"), &root);
    }
    let label = arg;
    let n: u32 = std::env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(8);
    let vm = boot_vm(&root);
    let lb = vm
        .tables()
        .boot
        .labels
        .get(&label)
        .unwrap_or_else(|| panic!("标签不存在: {label}"));
    println!("label {label}: script={} pc={}", lb.script_id, lb.pc);
    let sid = u32::from(lb.script_id);
    let Ok(script) = parse_script(sid) else {
        eprintln!("脚本 {sid} 解析失败");
        return;
    };
    for pc in lb.pc..lb.pc + n {
        let Some(g) = script.group(pc as usize) else {
            break;
        };
        let name = vm
            .tables()
            .boot
            .yscm
            .command(g.opcode)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| format!("#{}", g.opcode));
        let line = script.source_position(pc as usize).unwrap_or(0);
        println!("pc={pc} [{name}] (src line {line})");
        for w in script.group_windows(pc as usize) {
            let pname = vm
                .tables()
                .boot
                .yscm
                .param(g.opcode, w.kind)
                .map(|p| p.name.clone())
                .unwrap_or_else(|| "-".into());
            let payload = script.window_payload(w).unwrap_or(&[]);
            let hex: Vec<String> = payload
                .iter()
                .take(24)
                .map(|b| format!("{b:02x}"))
                .collect();
            let text: String = payload
                .iter()
                .copied()
                .filter(|b| (0x20..0x7f).contains(b))
                .map(|b| b as char)
                .collect();
            println!(
                "    kind={:>2} ({pname:<8}) flag={} slot={} len={} [{}]{text}",
                w.kind,
                w.flag,
                w.slot,
                w.length,
                hex.join(" "),
            );
        }
    }
    let _ = InputTimeline::new();
}

/// 全语料扫描:指定 opcode 的「窗口 kind → 出现次数」直方图(含空窗口)。
fn scan_params(opcode: u8, root: &str) {
    let vm = boot_vm(root);
    let pname = |kind: u8| -> String {
        vm.tables()
            .boot
            .yscm
            .param(opcode, kind)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| format!("?{kind}"))
    };
    let cname = vm
        .tables()
        .boot
        .yscm
        .command(opcode)
        .map(|c| c.name.clone())
        .unwrap_or_default();
    let mut hist: std::collections::BTreeMap<u8, u64> = Default::default();
    let mut cmds = 0u64;
    let mut sites: Vec<String> = Vec::new();
    for sid in 0..141u32 {
        let Ok(script) = parse_script(sid) else {
            continue;
        };
        for pc in 0..script.group_count() as u32 {
            let Some(g) = script.group(pc as usize) else {
                continue;
            };
            if g.opcode != opcode {
                continue;
            }
            cmds += 1;
            let line = script.source_position(pc as usize).unwrap_or(0);
            for w in script.group_windows(pc as usize) {
                *hist.entry(w.kind).or_default() += 1;
                if w.length > 0 && sites.len() < 6 {
                    sites.push(format!(
                        "s{sid} pc{pc} L{line} {}(len{} {:02x?})",
                        pname(w.kind),
                        w.length,
                        script
                            .window_payload(w)
                            .unwrap_or(&[])
                            .iter()
                            .take(12)
                            .map(|b| format!("{b:02x}"))
                            .collect::<Vec<_>>()
                            .join(" ")
                    ));
                }
            }
        }
    }
    println!("{cname}(op{opcode}): {cmds} 处");
    for (kind, n) in &hist {
        println!("  kind{kind:<3} {:<14} x{n}", pname(*kind));
    }
    for s in &sites {
        println!("  样例 {s}");
    }
}
