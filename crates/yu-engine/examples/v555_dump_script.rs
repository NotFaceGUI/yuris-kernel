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
        "s{sid}({path}): {} groups, key={:02X?}",
        script.group_count(),
        key.bytes
    );
    if let Some(v) = raw_start.as_deref().and_then(|v| v.strip_prefix("off:")) {
        let offset: u32 = v.parse().expect("offset");
        let pc = script.pc_for_code_offset(offset).unwrap_or(999_999);
        println!("off {offset} -> pc {pc} (group offset {:?})", script.code_offset_at(pc as usize));
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
            let decoded = expr::decode(payload)
                .ok()
                .and_then(|instrs| expr::simple_expr(&instrs))
                .and_then(|e| match e {
                    expr::SimpleExpr::Const(v) => Some(format!("const {v}")),
                    expr::SimpleExpr::Var(v) => Some(format!("var {}", v.display())),
                    _ => None,
                });
            let text = if payload.first() == Some(&0x4d) {
                expr::decode_string_literal(payload)
                    .ok()
                    .map(|b| format!("str {:?}", String::from_utf8_lossy(&b)))
            } else {
                None
            };
            let hex: Vec<String> = payload.iter().take(48).map(|b| format!("{b:02x}")).collect();
            print!(
                " | {pname}={}{}",
                decoded.or(text).unwrap_or_else(|| hex.join(" ")),
                if w.length > 0 || !payload.is_empty() {
                    format!("(len={})", w.length)
                } else {
                    String::new()
                }
            );
        }
        println!();
    }
}
