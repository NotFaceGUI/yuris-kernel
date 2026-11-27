//! 临时取证工具(用完即删):反汇编单个 `.ybn` 的组/窗口流。
//!
//! `cargo run -p yu-engine --example script-dump -- <project-root> <script_id> [--codec utf8] [--pc N..M]`
//!
//! 输出:pc + opcode + 命令名 + 每个参数窗口的**表达式反汇编**。
//! 用途 = 判断脚本**真正做了什么**(而不是按作品/版本猜),例如
//! `<utf8-localized-build>` 的 `es_engfontcalc.yst` 逐字判宽逻辑、
//! `es_fontcg.yst` 的字形页号计算。
//!
//! 表达式记录格式见 `yu_rt::expr` 模块文档:
//! `[op:u8][payload_len:u16 LE][payload:payload_len]`。
//!
//! 设 `YU_RAW=1` 时额外把 `0x4D` 字面量的原始载荷打到 stderr(核对引号/长度时用)。

use yu_engine::project::Project;
use yu_format::{yscm::CommandTable, Codec};
use yu_rt::tables::ScriptFile;
use yu_rt::value::VarSpace;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let root = args.get(1).expect("project root");
    let id: u32 = args
        .get(2)
        .expect("script id")
        .parse()
        .expect("script id 是数字");
    let codec_name = args
        .iter()
        .position(|a| a == "--codec")
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
        .unwrap_or("utf8");
    let codec = match codec_name {
        "utf8" => Codec::Utf8,
        "gbk" => Codec::Gbk,
        "big5" => Codec::Big5,
        _ => Codec::Cp932,
    };
    let only_range = args
        .iter()
        .position(|a| a == "--pc")
        .and_then(|i| args.get(i + 1))
        .and_then(|spec| {
            let mut parts = spec.split("..");
            let lo: usize = parts.next()?.parse().ok()?;
            let hi: usize = parts.next().and_then(|s| s.parse().ok()).unwrap_or(lo);
            Some(lo..=hi)
        });

    let project = Project::discover(std::path::Path::new(root), None).expect("open project");
    let key = project.resolve_script_key(None).expect("script key");
    let source = project.open_source().expect("open source");
    let ysc = read(source.as_ref(), "ysbin/ysc.ybn");
    let table = CommandTable::parse(&ysc, b"script-dump").expect("parse ysc");
    // `--cmd NAME`:打印该命令的参数名表(按槽号),核对参数名是否与内核
    // 按名解析时用的一致(`FONT[COLOR]` 等)。
    if let Some(pos) = args.iter().position(|a| a == "--cmd") {
        let want = args.get(pos + 1).expect("command name");
        for c in &table.commands {
            if c.name.eq_ignore_ascii_case(want) {
                println!("{} argc={}", c.name, c.argc);
                for (i, a) in c.args.iter().enumerate() {
                    println!("  slot {i}: {} flag={} kind={}", a.name, a.flag, a.kind);
                }
            }
        }
        return;
    }

    let name = format!("ysbin/yst{id:05}.ybn");
    let data = read(source.as_ref(), &name);
    let script = ScriptFile::parse_with_table(id, &data, key.bytes, &table).expect("parse script");

    println!(
        "script_id={id} version={} groups={} windows={}",
        script.version,
        script.group_count(),
        script.windows.len()
    );
    for pc in 0..script.group_count() {
        if only_range.as_ref().is_some_and(|range| !range.contains(&pc)) {
            continue;
        }
        let Some(group) = script.group(pc) else { continue };
        let opcode = group.opcode;
        let command = table
            .commands
            .get(usize::from(opcode))
            .map(|c| c.name.as_ref())
            .unwrap_or("?");
        println!("pc{pc} op=0x{opcode:02X} {command} n={}", group.window_count);
        for w in script.group_windows(pc) {
            let slot = w.slot;
            let Some(payload) = script.window_payload(w) else {
                println!("    [{slot}] <oob>");
                continue;
            };
            if payload.is_empty() {
                continue;
            }
            let text = match disasm(payload, codec) {
                Ok(text) => text,
                Err(reason) => format!("[{reason}] {:02X?}", payload),
            };
            // 参数名优先(`SET=`/`X=`…),槽号仅在没有名字时显示。
            let pname = table
                .commands
                .get(usize::from(opcode))
                .and_then(|c| c.args.get(usize::from(slot)))
                .map(|a| a.name.as_ref())
                .unwrap_or("");
            println!("    {pname}[{slot}] {text}");
        }
    }
}

fn read(source: &dyn yu_vfs::DataSource, path: &str) -> Vec<u8> {
    let key = yu_vfs::LookupKey::new(path.as_bytes(), yu_engine::project::CODEC)
        .expect("lookup key");
    source.open(&key).expect("read entry")
}

/// 反汇编一个窗口载荷(表达式字节码序列)。
fn disasm(payload: &[u8], codec: Codec) -> Result<String, String> {
    let mut at = 0usize;
    let mut stack: Vec<String> = Vec::new();
    let mut out: Vec<String> = Vec::new();
    while at + 3 <= payload.len() {
        let op = payload[at];
        let len = u16::from_le_bytes([payload[at + 1], payload[at + 2]]) as usize;
        let start = at + 3;
        let end = start.checked_add(len).ok_or("len overflow")?;
        let body = payload.get(start..end).ok_or("payload oob")?;
        at = end;
        let text = match op {
            0x42 if body.len() == 1 => format!("{}", body[0] as i8),
            0x57 if body.len() == 2 => format!("{}", i16::from_le_bytes([body[0], body[1]])),
            0x49 if body.len() == 4 => format!(
                "{}",
                i32::from_le_bytes([body[0], body[1], body[2], body[3]])
            ),
            0x4c if body.len() == 8 => format!(
                "{}",
                i64::from_le_bytes([
                    body[0], body[1], body[2], body[3], body[4], body[5], body[6], body[7]
                ])
            ),
            0x46 if body.len() == 8 => format!(
                "{}f",
                f64::from_bits(u64::from_le_bytes([
                    body[0], body[1], body[2], body[3], body[4], body[5], body[6], body[7]
                ]))
            ),
            0x4d => {
                if std::env::var_os("YU_RAW").is_some() {
                    eprintln!("RAW4D len={} {:02X?}", body.len(), body);
                }
                let inner = if body.len() >= 2 && body[0] == b'"' && body[body.len() - 1] == b'"' {
                    &body[1..body.len() - 1]
                } else {
                    body
                };
                let text: String = codec.decode_lossy(inner).0.into_iter().collect();
                format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
            }
            0x48 | 0x56 | 0x76 if body.len() == 3 => format!(
                "{}{}{}",
                if op == 0x48 { "" } else { "&" },
                var_prefix(body[0]),
                u16::from_le_bytes([body[1], body[2]])
            ),
            0x21 | 0x25 | 0x26 | 0x2a | 0x2b | 0x2d | 0x2f | 0x3c | 0x3d | 0x3e | 0x41
            | 0x4f | 0x53 | 0x5a | 0x5e | 0x7c => {
                let rhs = stack.pop().unwrap_or_else(|| "?".into());
                let lhs = stack.pop().unwrap_or_else(|| "?".into());
                format!("({lhs} {} {rhs})", binop_name(op))
            }
            0x29 => {
                let index = stack.pop().unwrap_or_else(|| "?".into());
                let base = stack.pop().unwrap_or_else(|| "?".into());
                format!("{base}[{index}]")
            }
            0x52 => {
                let value = stack.pop().unwrap_or_else(|| "?".into());
                format!("-{value}")
            }
            0x69 => {
                let value = stack.pop().unwrap_or_else(|| "?".into());
                format!("TOINT({value})")
            }
            0x73 => {
                let value = stack.pop().unwrap_or_else(|| "?".into());
                format!("TOSTR({value})")
            }
            other => format!("<op 0x{other:02X} {:02X?}>", body),
        };
        stack.push(text.clone());
        out.push(text);
    }
    Ok(if stack.len() == 1 {
        stack.pop().unwrap_or_default()
    } else {
        out.join(" ")
    })
}

/// 变量前缀的可读标注(`@` 数值 / `$` 字符串 / `` ` ``-`#` 旧式 G/L)。
fn var_prefix(byte: u8) -> String {
    let space = match VarSpace::from_prefix(byte) {
        VarSpace::At => "@",
        VarSpace::Dollar => "$",
        VarSpace::LegacyNum => "`@",
        VarSpace::LegacyStr => "#$",
        VarSpace::Other(other) => return format!("?0x{other:02X}"),
    };
    format!("{space}")
}

fn binop_name(op: u8) -> &'static str {
    match op {
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
    }
}
