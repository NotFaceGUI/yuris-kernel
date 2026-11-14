//! 真实数据验收 —— v<495 的 YSSD 存档容器(未使用偏移槽为垃圾)。
//!
//! 样本目录由 `YURIS_SAMPLE_LEGACY_SAVE` 提供(本机 v474 商业包 `save/`:
//! gsd.sd / kidoku.sd / seld.sd)。缺失时显式 skip。

use std::path::PathBuf;
use yu_format::yssd::YssdSave;

fn sample_dir() -> Option<PathBuf> {
    let Some(path) = std::env::var_os("YURIS_SAMPLE_LEGACY_SAVE") else {
        eprintln!("SKIP YURIS_SAMPLE_LEGACY_SAVE is not set");
        return None;
    };
    let path = PathBuf::from(path);
    if !path.is_dir() {
        eprintln!(
            "SKIP YURIS_SAMPLE_LEGACY_SAVE is not a directory: {}",
            path.display()
        );
        return None;
    }
    Some(path)
}

#[test]
fn legacy_saves_parse_with_garbage_unused_slots() {
    let Some(dir) = sample_dir() else {
        return;
    };
    for name in ["gsd.sd", "kidoku.sd", "seld.sd"] {
        let path = dir.join(name);
        if !path.is_file() {
            eprintln!("SKIP missing {}", path.display());
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        let save =
            YssdSave::parse(&bytes, name.as_bytes()).unwrap_or_else(|d| panic!("{name}: {d}"));
        assert!(!save.entries.is_empty(), "{name}: no blocks");
        assert!(save.entry(1).is_some(), "{name}: DNO=1 missing");
        // 首块必须能解压且非空(块载荷是 zlib/snappy 二者之一)。
        let raw = save.entries[0]
            .decompress(name.as_bytes())
            .unwrap_or_else(|d| panic!("{name}: {d}"));
        assert!(!raw.is_empty(), "{name}: first block empty");
        eprintln!(
            "legacy save {name}: version={:#x} blocks={} first=({} bytes, DNO={})",
            save.version,
            save.entries.len(),
            raw.len(),
            save.entries[0].dno
        );
    }
}
