//! 真实商业包布局回归(封包在文件夹里的分布形态)。
//!
//! 样本由 `YURIS_SAMPLE_NATSU_PAC` 指向游戏根(实测形态:`pac/` 下 7 个按
//! 内容命名的基础包 + `update1/2` 增量层 + `ed01..04.ypf`(MPEG-PS,不是
//! YPF))。缺样本**显式跳过并计数**,不静默通过(AGENTS.md §2)。

use std::path::PathBuf;

use yu_engine::project::{Project, CODEC};
use yu_vfs::LookupKey;

fn sample_root() -> Option<PathBuf> {
    let raw = std::env::var_os("YURIS_SAMPLE_NATSU_PAC")?;
    let path = PathBuf::from(raw);
    path.is_dir().then_some(path)
}

#[test]
fn real_game_pac_layout_is_discovered() {
    let Some(root) = sample_root() else {
        eprintln!("SKIP[count=1] YURIS_SAMPLE_NATSU_PAC 不可用,跳过真实商业包布局断言");
        return;
    };

    let project = Project::discover(&root, None).expect("发现数据入口");
    let packs = project.packs();

    let base: Vec<String> = packs
        .base
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert!(base.contains(&"bn.ypf".to_string()), "基础包: {base:?}");
    assert!(base.contains(&"cg.ypf".to_string()), "基础包: {base:?}");
    assert!(
        base.len() >= 7,
        "商业包按内容命名,应扫到多个基础包: {base:?}"
    );

    let updates: Vec<String> = packs
        .updates
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(updates, vec!["update1.ypf", "update2.ypf"]);

    let skipped: Vec<String> = packs
        .skipped
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert!(
        skipped.iter().any(|name| name == "ed01.ypf"),
        "伪装成 .ypf 的 MPEG 影片应跳过: {skipped:?}"
    );

    // 包确实挂上了:能从 bn.ypf 里读到命令表。
    let source = project.open_source().expect("打开挂载栈");
    let key = LookupKey::new(b"ysbin/ysc.ybn", CODEC).expect("合法键");
    let ysc = source.open(&key).expect("读 ysbin/ysc.ybn");
    assert_eq!(&ysc[..4], b"YSCM", "ysbin/ysc.ybn 应可读");
}
