//! 取证探针:在工程数据入口里按子串列出条目(封包/散盘同一口径)。
//!
//! 用法:
//! - `list_assets <project-root> <needle>` —— 按工程数据入口(base + update 层);
//! - `list_assets <project-root> <needle> <单个 .ypf>` —— 只查指定包
//!   (排查「这个包里到底有没有」)。
//!
//! 封包按每个 `.ypf` 的索引逐条列(不打开文件内容),散盘按目录遍历。
//! 命中按路径打印,末尾给总数。名字统一成 `/` 分隔 —— 包内索引存的是
//! `\`,`voice/` 这种写法不归一就永远搜不到。
use std::path::Path;

use yu_engine::project::{DataEntry, Project};

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let needle = std::env::args().nth(2).unwrap_or_default().to_ascii_lowercase();
    // 可选第三参:直接指定单个 `.ypf`(排查「包里到底有没有这个文件」)。
    let single = std::env::args().nth(3);
    let project = Project::discover(Path::new(&root), None).expect("project");

    let mut hits = 0u32;
    let mut total = 0u32;
    let mut report = |name: &str, origin: &str| {
        total += 1;
        if needle.is_empty() || name.to_ascii_lowercase().contains(&needle) {
            hits += 1;
            println!("{name}    <- {origin}");
        }
    };

    match project.entry() {
        DataEntry::Packed(set) if single.is_none() => {
            for pack in set.base.iter().chain(set.updates.iter()) {
                let src = match yu_vfs::YpfSource::open(pack, yu_vfs::Codec::Cp932) {
                    Ok(s) => s,
                    Err(d) => {
                        println!("!! {} 打不开: {d:?}", pack.display());
                        continue;
                    }
                };
                let label = pack.file_name().map(|s| s.to_string_lossy().into_owned());
                for e in src.entries() {
                    // 条目名是 CP932 原始字节(引擎按字节查表),诊断按 lossy 打印;
                    // 分隔符统一成 `/`,否则 `voice/` 搜不到包内的 `voice\`。
                    report(
                        &String::from_utf8_lossy(&e.name).replace('\\', "/"),
                        label.as_deref().unwrap_or("?"),
                    );
                }
            }
            for skipped in &set.skipped {
                println!("(跳过非 YPF){}", skipped.display());
            }
        }
        DataEntry::Loose(dir) if single.is_none() => {
            let mut stack = vec![dir.clone()];
            while let Some(d) = stack.pop() {
                let Ok(rd) = std::fs::read_dir(&d) else { continue };
                for ent in rd.flatten() {
                    let p = ent.path();
                    if p.is_dir() {
                        stack.push(p);
                    } else if let Ok(rel) = p.strip_prefix(&dir) {
                        report(&rel.to_string_lossy().replace('\\', "/"), "loose");
                    }
                }
            }
        }
        // `single` 模式:显式单包,不走工程入口。
        DataEntry::Packed(_) | DataEntry::Loose(_) => {}
    }
    if let Some(pack) = single {
        let src = yu_vfs::YpfSource::open(&pack, yu_vfs::Codec::Cp932).expect("单包打开失败");
        let label = Path::new(&pack)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        for e in src.entries() {
            report(&String::from_utf8_lossy(&e.name).replace('\\', "/"), &label);
        }
    }
    println!("total={total} hits={hits}");
}
