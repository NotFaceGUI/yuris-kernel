//! 取证探针:在工程全部脚本的窗口载荷里按子串定位站点。
//!
//! 用法:`cargo run -p yu-engine --example find_script_str -- <project-root> <needle>`
//!
//! 输出每个命中站点的 `s<脚本> pc<组> <命令名>`、命中窗口序号、载荷 hex,
//! 以及表达式层解出的指令序列 —— 用于把「某个路径为什么被拼成这样」
//! 直接落回脚本字节,而不是猜。
use yu_engine::project::Project;
use yu_rt::tables::TableSet;

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let needle = std::env::args().nth(2).expect("needle");
    let needle = match needle.strip_prefix("hex:") {
        Some(hex) => (0..hex.len() / 2)
            .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).expect("hex needle"))
            .collect::<Vec<u8>>(),
        None => needle.into_bytes(),
    };

    let project = Project::discover(std::path::Path::new(&root), None).expect("project");
    let key = project.resolve_script_key(None).expect("script key");
    println!("key={:02x?} source={}", key.bytes, key.source.describe());
    let src = project.open_source().expect("data source");
    let mut tables = TableSet::load_from(src.as_ref(), None, key.bytes).expect("tables");

    let ids: Vec<u32> = tables.boot.scripts.entries.iter().map(|e| e.index).collect();
    let names: Vec<String> = (0..tables.boot.yscm.len())
        .map(|i| {
            tables
                .boot
                .yscm
                .command(i as u8)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| format!("op{i}"))
        })
        .collect();
    let mut hits = 0u32;
    for sid in ids {
        let Ok(script) = tables.load_script(src.as_ref(), sid) else {
            continue;
        };
        let script = script.clone();
        for pc in 0..script.group_count() as u32 {
            let Some(g) = script.group(pc as usize) else {
                continue;
            };
            let name = names
                .get(g.opcode as usize)
                .cloned()
                .unwrap_or_else(|| format!("op{}", g.opcode));
            for (wi, w) in script.group_windows(pc as usize).iter().enumerate() {
                let Some(payload) = script.window_payload(w) else {
                    continue;
                };
                if !payload.windows(needle.len()).any(|win| win == needle.as_slice()) {
                    continue;
                }
                hits += 1;
                let hex: String = payload.iter().map(|b| format!("{b:02x}")).collect();
                let ascii: String = payload
                    .iter()
                    .map(|&b| if (0x20..0x7f).contains(&b) { b as char } else { '.' })
                    .collect();
                println!("s{sid} pc{pc} {name} window[{wi}] kind={} len={}", w.kind, payload.len());
                println!("  hex   {hex}");
                println!("  ascii {ascii}");
                match yu_rt::expr::decode(payload) {
                    Ok(instrs) => {
                        for ins in &instrs {
                            println!("  op {:#04x} payload {:02x?}", ins.op, ins.payload);
                        }
                    }
                    Err(d) => println!("  decode error: {d:?}"),
                }
            }
        }
    }
    println!("hits={hits}");
}
