//! 真实数据验收 —— v<495 的 YSVR 记录头(无 flag 字节)。
//!
//! 样本由 `YURIS_SAMPLE_LEGACY_YSV` 提供(本机 v474 商业包:
//! 2736 条,7 字节记录头)。缺失时显式 skip;`YURIS_REQUIRE_SAMPLES=1`
//! 时硬失败。

use std::path::PathBuf;
use yu_format::ysvr::{VarDecls, YsvLayout};

fn sample() -> Option<PathBuf> {
    let Some(path) = std::env::var_os("YURIS_SAMPLE_LEGACY_YSV") else {
        if std::env::var_os("YURIS_REQUIRE_SAMPLES").is_some_and(|v| v != "0" && !v.is_empty()) {
            panic!("YURIS_REQUIRE_SAMPLES=1 but YURIS_SAMPLE_LEGACY_YSV is not set");
        }
        eprintln!("SKIP YURIS_SAMPLE_LEGACY_YSV is not set");
        return None;
    };
    let path = PathBuf::from(path);
    if !path.is_file() {
        eprintln!("SKIP YURIS_SAMPLE_LEGACY_YSV does not exist: {}", path.display());
        return None;
    }
    Some(path)
}

#[test]
fn legacy_ysv_uses_no_flag_layout() {
    let Some(path) = sample() else {
        return;
    };
    let bytes = std::fs::read(&path).unwrap();
    let decls = VarDecls::parse(&bytes, b"ysbin/ysv.ybn").unwrap();
    eprintln!(
        "legacy ysv: version={} layout={} decls={}",
        decls.version,
        decls.layout.as_str(),
        decls.decls.len()
    );
    assert_eq!(decls.layout, YsvLayout::NoFlag);
    assert!(!decls.decls.is_empty());
    assert!(decls.decls.iter().all(|d| d.flag == 0));
    assert_eq!(decls.loaded_at_init().count() + decls.decls.iter().filter(|d| d.kind == 2).count(),
               decls.decls.len());
}
