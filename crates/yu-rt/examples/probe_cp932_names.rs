//! 取证探针:v494 商业包的「路径/标签」按 CP932 正确解码后的真名,
//! 以及关键标签跳转目标是否落在合理脚本上。
//!
//! 用法:`cargo run -p yu-rt --example probe_cp932_names -- <game_root>`
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DataSource, DirSource, LookupKey, YpfSource};

fn cp932(bytes: &[u8]) -> String {
    yu_format::Codec::Cp932
        .decode(bytes, bytes)
        .unwrap_or_else(|_| String::from_utf8_lossy(bytes).into_owned())
}

fn main() {
    let root = std::env::args().nth(1).expect("game root");
    let pack_path = format!("{root}/data.ypf");

    // 直接从 yst_list 取原始路径字节,按 CP932 解码。
    let pack = YpfSource::open(&pack_path, Codec::Cp932).expect("data.ypf");
    let list_bytes = {
        let key = LookupKey::new(b"ysbin/yst_list.ybn", Codec::Cp932).expect("key");
        pack.open(&key).expect("yst_list.ybn")
    };
    let list = yu_format::ystl::ScriptList::parse(&list_bytes, b"ysbin/yst_list.ybn").expect("ystl");

    println!("== yst_list 路径(CP932 正确解码)== ");
    for e in &list.entries {
        if (70..=100).contains(&e.index) {
            println!("  s{:<4} {}", e.index, cp932(&e.name));
        }
    }

    // 标签表:按 CP932 解码标签名 + 目标脚本真名。
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

    let path_of = |sid: u32| -> String {
        list.entries
            .iter()
            .find(|e| e.index == sid)
            .map(|e| cp932(&e.name))
            .unwrap_or_else(|| format!("<无条目 s{sid}>"))
    };

    println!("== 关键标签(CP932 名 → 脚本真名)== ");
    let mut order: Vec<(Vec<u8>, u32, u32)> = Vec::new();
    for lb in vm.tables().boot.labels.iter() {
        if lb.name_bytes.iter().any(|b| *b >= 0x80) {
            order.push((lb.name_bytes.clone(), u32::from(lb.script_id), lb.pc));
        }
    }
    println!("  (非 ASCII 标签共 {} 条,列前 20)", order.len());
    for (name, sid, pc) in order.iter().take(20) {
        println!(
            "  {:<28} → s{:<4} pc{:<5} {}",
            cp932(name),
            sid,
            pc,
            path_of(*sid)
        );
    }

    println!("== 定义类标签 → 脚本真名 == ");
    for want in [
        "SYSTEM_START",
        "es.ERIS",
        "ES.FIRST",
        "ES.CHARNAME.DEFINE",
        "es.CHAR.NAME",
        "ES.ADDVAR.DEFINE.PRE",
        "ES.FOLDER.DEFINE",
        "ES.SYSTEM.DEFINE",
        "ES.TITLE.DEFINE",
        "ES.GAMEMAIN.DEFINE",
        "ES.SAVELOAD.DEFINE",
        "ES.DATE.DEFINE",
        "ES.GSD.INIT",
        "ES.S.SHOT",
        "es.SSHOT.PRE",
        "es.SSHOT.POST",
        "es.S.SYSTEMStartCheck",
        "es.SYSTEMStartCheck",
        "SCENARIO_LOGO",
        "ES.SCENARIO.SubLoop",
        "ES.SCENARIO.MainLoop",
        "ES.TITLE",
        "ES.GAMEMAIN",
        "ES.TITLE.LOOP",
    ] {
        match vm.tables().boot.labels.get(want) {
            Some(lb) => println!(
                "  {:<26} → s{:<4} pc{:<5} {}",
                want,
                lb.script_id,
                lb.pc,
                path_of(u32::from(lb.script_id))
            ),
            None => println!("  {:<26} → <缺失>", want),
        }
    }

    println!("== 所有 SCENARIO* 标签 == ");
    for lb in vm.tables().boot.labels.iter() {
        let name = cp932(&lb.name_bytes);
        if name.to_ascii_uppercase().starts_with("SCENARIO") {
            println!(
                "  {:<30} → s{:<4} pc{:<6} {}",
                name,
                lb.script_id,
                lb.pc,
                path_of(u32::from(lb.script_id))
            );
        }
    }
    println!("== 含 FIRST / GSD.INIT / Initialize / TITLE 的标签(不区分大小写)== ");
    for lb in vm.tables().boot.labels.iter() {
        let name = cp932(&lb.name_bytes);
        let lower = name.to_ascii_lowercase();
        if lower.contains("first") || lower.contains("gsd.init") || lower.contains("sshot")
            || lower == "es.initialize" || lower.contains("title.define") || lower.contains("gamemain")
        {
            println!(
                "  {:<30} → s{:<4} pc{:<6} {}",
                name,
                lb.script_id,
                lb.pc,
                path_of(u32::from(lb.script_id))
            );
        }
    }
    println!("== 标签总数 / 带点前缀的样例 ==");
    let mut dotted = 0usize;
    for lb in vm.tables().boot.labels.iter() {
        if lb.name_bytes.starts_with(b".") { dotted += 1; }
    }
    println!("  以 '.' 开头的局部标签: {dotted} / {}", vm.tables().boot.labels.len());

    println!("== 大小写/模糊匹配相近标签(防大小写硬编码)== ");
    let mut hits: Vec<String> = Vec::new();
    for lb in vm.tables().boot.labels.iter() {
        let name = cp932(&lb.name_bytes);
        let lower = name.to_ascii_lowercase();
        if lower.contains("sshot") || lower.contains("s.shot") || lower.contains("screenshot") {
            hits.push(format!(
                "  {name:<28} → s{:<4} pc{:<5} {}",
                lb.script_id,
                lb.pc,
                path_of(u32::from(lb.script_id))
            ));
        }
    }
    for h in hits.iter().take(20) {
        println!("{h}");
    }
}
