//! 静态倾倒一个 v555 脚本组:命令名、参数名、常量/字符串/表达式。
//!
//! 用法:`v555_dump_script <project-root> <script-id> [pc-start] [pc-end]`
//!
//! 与运行期无关:只走 TableSet 解码,便于定位 `PRODUCT[COM=23]` 等调用点。

use std::path::Path;

use yu_engine::project::Project;
use yu_rt::expr;
use yu_rt::tables::TableSet;

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let sid: u32 = std::env::args().nth(2).expect("sid").parse().expect("sid");
    let raw_start = std::env::args().nth(3);
    let start: u32 = raw_start
        .as_deref()
        .and_then(|v| v.strip_prefix("off:"))
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| raw_start.as_deref().and_then(|v| v.parse().ok()).unwrap_or(0));
    let end: u32 = std::env::args()
        .nth(4)
        .and_then(|v| v.parse().ok())
        .unwrap_or(u32::MAX);
    let project = Project::discover(Path::new(&root), None).expect("project");
    let key = project.resolve_script_key(None).expect("script key");
    let src = project.open_source().expect("source");
    let mut tables = TableSet::load_from(src.as_ref(), None, key.bytes).expect("tables");
    let script = tables.load_script(src.as_ref(), sid).expect("script").clone();
    let path = tables.script_path(sid);
    println!(
        "s{sid}({path}): {} groups, key={:02X?} script_version={} yscm_ver={:?} ycd_ver={:?}",
        script.group_count(),
        key.bytes,
        script.version,
        tables.boot.yscm.version,
        tables.boot.ycd.as_ref().map(|y| y.version)
    );
    if let Some(v) = raw_start.as_deref().and_then(|v| v.strip_prefix("off:")) {
        let offset: u32 = v.parse().expect("offset");
        let pc = script.pc_for_code_offset(offset).unwrap_or(999_999);
        println!("off {offset} -> pc {pc} (group offset {:?})", script.code_offset_at(pc as usize));
    }
    // 标签表(可选):把 pc 映射回 .yst 标签名。
    let mut label_at: std::collections::BTreeMap<u32, Vec<String>> = Default::default();
    if let Ok(k) = yu_vfs::LookupKey::new(b"ysbin/ysl.ybn", yu_vfs::Codec::Cp932) {
        if let Ok(bytes) = src.open(&k) {
            if let Ok(fmt) = yu_format::yslb::YslLabels::parse(&bytes, b"ysbin/ysl.ybn") {
                for e in fmt.entries.iter().filter(|e| u32::from(e.script_no) == sid) {
                    label_at
                        .entry(e.field_b)
                        .or_default()
                        .push(String::from_utf8_lossy(&e.name).into_owned());
                }
                println!("labels for s{sid}: {}", label_at.values().map(Vec::len).sum::<usize>());
                for (pc, names) in &label_at {
                    println!("  pc{pc:<5} {}", names.join(", "));
                }
            }
        }
    }
    for pc in start..end.min(script.group_count() as u32) {
        let Some(g) = script.group(pc as usize) else { break };
        let Some(cmd) = tables.boot.yscm.command(g.opcode) else {
            println!("pc{pc} opcode#{} (无表项)", g.opcode);
            continue;
        };
        let line = script.source_position(pc as usize).unwrap_or(0);
        print!("pc{pc:<5} L{line:<5} {:<14}", cmd.name);
        for w in script.group_windows(pc as usize) {
            let pname = cmd
                .params
                .get(w.kind as usize)
                .map(|p| p.name.as_str())
                .unwrap_or("-");
            let payload = script.window_payload(w).unwrap_or(&[]);
            let shown = expr::decode(payload)
                .ok()
                .map(|instrs| render_expr(&instrs))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| {
                    payload
                        .iter()
                        .take(48)
                        .map(|b| format!("{b:02x}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                });
            print!(
                " | {pname}[k={} f={} s={} t={}]={}",
                w.kind, w.flag, w.slot, w.length, shown
            );
        }
        println!();
    }
}

/// 极简表达式还原(仅供人读):栈机 opcode 见 `yu_rt::expr` 模块文档。
fn render_expr(instrs: &[expr::RawInstr]) -> String {
    let mut stack: Vec<String> = Vec::new();
    let mut out: Vec<String> = Vec::new();
    let flush = |stack: &mut Vec<String>, out: &mut Vec<String>| {
        while let Some(s) = stack.pop() {
            out.push(s);
        }
        stack.clear();
    };
    for ins in instrs {
        let p = ins.payload.as_slice();
        match ins.op {
            0x42 if !p.is_empty() => stack.push(format!("{}", p[0] as i8)),
            0x57 if p.len() >= 2 => {
                stack.push(format!("{}", i16::from_le_bytes([p[0], p[1]])))
            }
            0x49 if p.len() >= 4 => stack.push(format!(
                "{}",
                i32::from_le_bytes([p[0], p[1], p[2], p[3]])
            )),
            0x4c if p.len() >= 8 => stack.push(format!(
                "{}",
                i64::from_le_bytes([
                    p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7]
                ])
            )),
            0x46 if p.len() >= 8 => stack.push(format!(
                "{}",
                f64::from_le_bytes([
                    p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7]
                ])
            )),
            0x4d => stack.push(format!(
                "{:?}",
                String::from_utf8_lossy(&expr::decode_string_literal(p).unwrap_or_default())
            )),
            0x48 | 0x56 | 0x76 if p.len() >= 3 => {
                let slot = u16::from_le_bytes([p[1], p[2]]);
                let prefix = match yu_rt::value::VarSpace::from_prefix(p[0]) {
                    yu_rt::value::VarSpace::At => "@",
                    yu_rt::value::VarSpace::Dollar => "$",
                    yu_rt::value::VarSpace::LegacyNum => "`",
                    yu_rt::value::VarSpace::LegacyStr => "#",
                    yu_rt::value::VarSpace::Other(b) => {
                        return format!("<space {b:02x}>:{slot}");
                    }
                };
                let kind = if ins.op == 0x56 || ins.op == 0x76 {
                    "&"
                } else {
                    ""
                };
                stack.push(format!("{kind}{prefix}{slot}"));
            }
            0x52 => {
                let a = stack.pop().unwrap_or_default();
                stack.push(format!("-({a})"));
            }
            0x69 => {
                let a = stack.pop().unwrap_or_default();
                stack.push(format!("num({a})"));
            }
            0x73 => {
                let a = stack.pop().unwrap_or_default();
                stack.push(format!("str({a})"));
            }
            0x29 => {
                let idx = stack.pop().unwrap_or_default();
                let base = stack.pop().unwrap_or_default();
                stack.push(format!("{base}[{idx}]"));
            }
            0x2c => flush(&mut stack, &mut out),
            op @ (0x21 | 0x25 | 0x26 | 0x2a | 0x2b | 0x2d | 0x2f | 0x3c | 0x3d | 0x3e
            | 0x41 | 0x4f | 0x53 | 0x5a | 0x5e | 0x7c) => {
                let b = stack.pop().unwrap_or_default();
                let a = stack.pop().unwrap_or_default();
                let sym = match op {
                    0x21 => "!=",
                    0x25 => "%",
                    0x26 => "&&",
                    0x2a => "*",
                    0x2b => "+",
                    0x2d => "-",
                    0x2f => "/",
                    0x3c => "<",
                    0x3d => "==",
                    0x3e => ">",
                    0x41 => "&",
                    0x4f => "|",
                    0x53 => "<=",
                    0x5a => ">=",
                    0x5e => "^",
                    0x7c => "||",
                    _ => "?",
                };
                stack.push(format!("({a} {sym} {b})"));
            }
            op => stack.push(format!("<op{op:02x}>")),
        }
    }
    flush(&mut stack, &mut out);
    out.join("; ")
}
