//! 取证探针:按脚本号或标签名倾倒组(命令)序列与窗口载荷。
//!
//! 用法:
//! - `dump_script <project-root> <sid> [from] [to]` —— 按脚本号;
//! - `dump_script <project-root> <label> [from] [to]` —— 按标签名
//!   (解析出脚本/pc,缺省从该 pc 起)。
//!
//! 每个组打印 `pc opcode 命令名 param=… line=…`,随后逐窗口打印 kind、
//! 载荷 hex/ascii 与表达式层解码结果 —— 用于把运行时行为落回脚本字节。
use yu_engine::project::Project;
use yu_rt::tables::TableSet;

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let target = std::env::args().nth(2).expect("script id or label");
    let from_arg = std::env::args().nth(3).and_then(|v| v.parse::<u32>().ok());
    let to_arg = std::env::args().nth(4).and_then(|v| v.parse::<u32>().ok());

    let project = Project::discover(std::path::Path::new(&root), None).expect("project");
    let key = project.resolve_script_key(None).expect("script key");
    let src = project.open_source().expect("data source");
    let mut tables = TableSet::load_from(src.as_ref(), None, key.bytes).expect("tables");

    let (sid, label_pc) = match target.parse::<u32>() {
        Ok(sid) => (sid, None),
        Err(_) => {
            let encoded = yu_format::Codec::Cp932
                .encode(&target, b"label")
                .unwrap_or_else(|_| target.as_bytes().to_vec());
            let label = tables
                .boot
                .labels
                .get_bytes(&encoded)
                .unwrap_or_else(|| panic!("标签 `{target}` 不在工程 ysl.ybn"))
                .clone();
            println!("label `{target}` → s{} pc{}", label.script_id, label.pc);
            (u32::from(label.script_id), Some(label.pc))
        }
    };
    let from = from_arg.or(label_pc).unwrap_or(0);
    let to = to_arg.unwrap_or(u32::MAX);

    let script = tables.load_script(src.as_ref(), sid).expect("script").clone();
    println!(
        "s{sid} {} 组,路径={}",
        script.group_count(),
        tables.script_path(sid)
    );

    for pc in from..=to.min(script.group_count().saturating_sub(1) as u32) {
        let Some(g) = script.group(pc as usize) else { continue };
        let name = tables
            .boot
            .yscm
            .command(g.opcode)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| format!("op{}", g.opcode));
        let line = script.source_position(pc as usize).unwrap_or(0);
        println!("[{pc}] {name} op={} param={} line={line}", g.opcode, g.param);
        for (wi, w) in script.group_windows(pc as usize).iter().enumerate() {
            let Some(payload) = script.window_payload(w) else { continue };
            let hex: String = payload.iter().map(|b| format!("{b:02x}")).collect();
            let ascii: String = payload
                .iter()
                .map(|&b| if (0x20..0x7f).contains(&b) { b as char } else { '.' })
                .collect();
            let pname = tables
                .boot
                .yscm
                .command(g.opcode)
                .and_then(|c| c.params.get(w.kind as usize))
                .map(|p| p.name.clone())
                .unwrap_or_default();
            println!("  w{wi} kind={} {pname} len={}", w.kind, payload.len());
            println!("    hex   {hex}");
            println!("    ascii {ascii}");
            match yu_rt::expr::decode(payload) {
                Ok(instrs) => {
                    let ops: Vec<String> = instrs
                        .iter()
                        .map(|i| format!("{:#04x}:{:02x?}", i.op, i.payload))
                        .collect();
                    println!("    ops   {}", ops.join(" "));
                }
                Err(d) => println!("    ops   <decode error {d:?}>"),
            }
        }
    }
}
