//! 取证探针:v494 商业包的脚本 id↔路径表 / 入口标签解析。
//!
//! 用法:`cargo run -p yu-rt --example probe_script_map -- <game_root>`
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DataSource, DirSource, LookupKey, YpfSource};

fn main() {
    let root = std::env::args().nth(1).expect("game root");
    let pack_path = format!("{root}/data.ypf");
    let pack = YpfSource::open(&pack_path, Codec::Cp932).expect("data.ypf");

    // yst_list 原样解析(与运行时同源)。
    let list_bytes = {
        let key = LookupKey::new(b"ysbin/yst_list.ybn", Codec::Cp932).expect("key");
        pack.open(&key).expect("yst_list.ybn in pack")
    };
    let list = yu_format::ystl::ScriptList::parse(&list_bytes, b"ysbin/yst_list.ybn")
        .expect("parse yst_list");
    println!("yst_list: {} 条", list.entries.len());
    for e in list.entries.iter() {
        if (0..=3).contains(&e.index) || (70..=90).contains(&e.index) {
            println!(
                "  index={:<4} path={}",
                e.index,
                String::from_utf8_lossy(&e.name)
            );
        }
    }

    // 运行时映射(BootConfig 同 CLI)。
    let aux = DirSource::open(&root, Codec::Cp932).expect("game root");
    let pack2 = YpfSource::open(&pack_path, Codec::Cp932).expect("data.ypf");
    let vm = Vm::boot_with_aux(
        Box::new(pack2),
        Some(Box::new(aux)),
        BootConfig {
            entry_label: "SYSTEM_START".into(),
            ..BootConfig::default()
        },
    )
    .expect("boot");
    println!("== VM 映射(前 100 条) ==");
    for sid in 0..100u32 {
        println!("  s{sid:<4} {}", vm.script_name(sid));
    }
    println!("== 路径原始字节(s74..s96) ==");
    for sid in 74..=96u32 {
        let p = vm.script_name(sid);
        let hex: String = p.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
        println!("  s{sid} {hex}");
    }
    println!("== 关键标签 ==");
    for name in ["SYSTEM_START", "es.ERIS", "ES.CHARNAME.DEFINE", "es.CHAR.NAME", "ES.ADDVAR.DEFINE.PRE", "ES.FOLDER.DEFINE", "ES.SYSTEM.DEFINE"] {
        match vm.tables().boot.labels.get(name) {
            Some(lb) => println!("  {name:<16} s{} pc{}", lb.script_id, lb.pc),
            None => println!("  {name:<16} <缺失>"),
        }
    }
    println!("== 脚本总数 ==");
    let mut n = 0u32;
    let mut last = 0u32;
    for sid in 0..4000u32 {
        let p = vm.script_name(sid);
        if !(p.starts_with("yst") && p.ends_with(".ybn")) {
            n += 1;
            last = sid;
        }
    }
    println!("  可解析路径条目={n}, 最大 id={last}");
}
