//! 取证探针:从商业包(YpfSource)倾倒指定脚本 id 的命令序列。
//!
//! 用法:`cargo run -p yu-rt --example dump_game_cmds -- <game_root> <sid> <pc_start> <pc_end>`
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DataSource, DirSource, LookupKey, YpfSource};

fn main() {
    let root = std::env::args().nth(1).expect("game root");
    let sid: u32 = std::env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .expect("script id");
    let pc_start: u32 = std::env::args()
        .nth(3)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let pc_end: u32 = std::env::args()
        .nth(4)
        .and_then(|v| v.parse().ok())
        .unwrap_or(pc_start + 30);

    let pack = YpfSource::open(format!("{root}/data.ypf"), Codec::Cp932).expect("data.ypf");
    let bytes = {
        let key = LookupKey::new(format!("ysbin/yst{sid:05}.ybn").as_bytes(), Codec::Cp932)
            .expect("key");
        match pack.open(&key) {
            Ok(b) => b,
            Err(e) => {
                // 散盘回退(游戏根 ysbin/)。
                let p = format!("{root}/ysbin/yst{sid:05}.ybn");
                eprintln!("包内缺失({e});尝试散盘 {p}");
                std::fs::read(&p).expect("散盘也不存在")
            }
        }
    };
    let script = yu_rt::tables::ScriptFile::parse(sid, &bytes, yu_rt::tables::DEFAULT_SCRIPT_KEY)
        .expect("parse");
    println!(
        "s{sid}: {} 组, 路径={}",
        script.group_count(),
        {
            let aux = DirSource::open(&root, Codec::Cp932).expect("game root");
            let _ = aux;
            String::new()
        }
    );

    // 用 VM 取命令名/参数名表。
    let aux = DirSource::open(&root, Codec::Cp932).expect("game root");
    let vm = Vm::boot_with_aux(
        Box::new(YpfSource::open(format!("{root}/data.ypf"), Codec::Cp932).expect("pack")),
        Some(Box::new(aux)),
        BootConfig {
            entry_label: "SYSTEM_START".into(),
            ..BootConfig::default()
        },
    )
    .expect("boot");
    for pc in pc_start..pc_end.min(script.group_count() as u32) {
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
        print!("pc{pc:<5} L{line:<5} {name:<12}");
        for w in script.group_windows(pc as usize) {
            let pname = vm
                .tables()
                .boot
                .yscm
                .param(g.opcode, w.kind)
                .map(|p| p.name.clone())
                .unwrap_or_else(|| "-".into());
            let payload = script.window_payload(w).unwrap_or(&[]);
            let text: String = payload
                .iter()
                .copied()
                .filter(|b| (0x20..0x7f).contains(b))
                .map(|b| b as char)
                .collect();
            let hex: Vec<String> = payload
                .iter()
                .take(64)
                .map(|b| format!("{b:02x}"))
                .collect();
            print!(" | {pname}={}[{}{}]", w.length, hex.join(" "), text);
        }
        println!();
    }
}
