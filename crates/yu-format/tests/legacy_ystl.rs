//! 真实数据验收 —— v<495 的 YSTL 记录尾(16 字节,无 text_count)。
//!
//! 样本由 `YURIS_SAMPLE_LEGACY_YSTL` 提供(本机 v474 商业包:
//! 232 条,Old16)。缺失时显式 skip。

use std::path::PathBuf;
use yu_format::ystl::{ScriptList, YstlLayout};

fn sample() -> Option<PathBuf> {
    let Some(path) = std::env::var_os("YURIS_SAMPLE_LEGACY_YSTL") else {
        eprintln!("SKIP YURIS_SAMPLE_LEGACY_YSTL is not set");
        return None;
    };
    let path = PathBuf::from(path);
    if !path.is_file() {
        eprintln!("SKIP YURIS_SAMPLE_LEGACY_YSTL does not exist: {}", path.display());
        return None;
    }
    Some(path)
}

#[test]
fn legacy_ystl_uses_old16_layout() {
    let Some(path) = sample() else {
        return;
    };
    let bytes = std::fs::read(&path).unwrap();
    let list = ScriptList::parse(&bytes, b"ysbin/yst_list.ybn").unwrap();
    eprintln!(
        "legacy ystl: version={} layout={} entries={}",
        list.version,
        list.layout.as_str(),
        list.entries.len()
    );
    assert_eq!(list.layout, YstlLayout::Old16);
    assert!(!list.entries.is_empty());
    assert!(list.entries.iter().all(|e| e.text_count == 0));
    // 第一份样本的 index 与磁盘 yst%05d 一一对应。
    assert_eq!(list.entries[0].index, 0);
}
