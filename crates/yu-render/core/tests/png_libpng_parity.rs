//! `png_libpng` 与 `image` crate 的全量像素对拍(**仅 Android**;
//! 桌面走 `image`,没有 libpng 依赖)。
//!
//! 语料由 `YURIS_SAMPLE_*` 环境变量给出;缺语料时显式跳过并打印计数。
//! 判据:同一份 PNG,两条路径的 `(w, h, rgba)` 逐字节相同。

// 整个用例只在 Android 有意义:桌面根本不链 libpng,把它编进来只会
// 产生一堆 `never used` 警告。
#![cfg(target_os = "android")]

use std::path::Path;

fn roots() -> Vec<std::path::PathBuf> {
    let mut v = Vec::new();
    for k in ["YURIS_SAMPLE_ERIS_YSBIN", "YURIS_SAMPLE_SDK0495", "YURIS_SAMPLE_SDK0479"] {
        if let Ok(p) = std::env::var(k) {
            if !p.trim().is_empty() { v.push(std::path::PathBuf::from(p)); }
        }
    }
    v
}

fn collect(dir: &Path, out: &mut Vec<std::path::PathBuf>, limit: usize) {
    if out.len() >= limit { return; }
    let Ok(es) = std::fs::read_dir(dir) else { return };
    for e in es.flatten() {
        if out.len() >= limit { return; }
        let p = e.path();
        if p.is_dir() { collect(&p, out, limit); }
        else if p.extension().and_then(|s| s.to_str()).is_some_and(|s| s.eq_ignore_ascii_case("png")) {
            out.push(p);
        }
    }
}

#[test]
#[cfg(target_os = "android")]
fn libpng_matches_image_on_corpus() {
    let rs = roots();
    if rs.is_empty() {
        eprintln!("SKIP[count=1] libpng_matches_image_on_corpus: 无语料环境变量");
        return;
    }
    let mut files = Vec::new();
    for r in &rs { collect(r, &mut files, 5000); }
    if files.is_empty() {
        eprintln!("SKIP[count=1] libpng_matches_image_on_corpus: 语料下无 PNG");
        return;
    }

    let (mut checked, mut skipped) = (0usize, 0usize);
    let mut fails: Vec<String> = Vec::new();
    for path in &files {
        let Ok(bytes) = std::fs::read(path) else { skipped += 1; continue };
        let ours = match yu_render_core::png_libpng::decode(&bytes) {
            Ok(v) => v,
            Err(why) => {
                if image::load_from_memory(&bytes).is_err() { skipped += 1; continue; }
                fails.push(format!("{}: libpng 拒绝但 image 能解({why})", path.display()));
                continue;
            }
        };
        let Ok(theirs) = image::load_from_memory(&bytes) else { skipped += 1; continue };
        let theirs = theirs.into_rgba8();
        if ours.0 != theirs.width() || ours.1 != theirs.height() {
            fails.push(format!("{}: 尺寸 {}x{} vs {}x{}", path.display(), ours.0, ours.1, theirs.width(), theirs.height()));
            continue;
        }
        if let Some((i, (a, b))) = ours.2.iter().zip(theirs.as_raw().iter()).enumerate().find(|(_, (a, b))| a != b) {
            fails.push(format!("{}: 首个差异字节 {i}({a} vs {b}),共 {}", path.display(), ours.2.len()));
            continue;
        }
        if ours.2.len() != theirs.as_raw().len() {
            fails.push(format!("{}: 长度 {} vs {}", path.display(), ours.2.len(), theirs.as_raw().len()));
            continue;
        }
        checked += 1;
    }

    eprintln!("对拍: checked={checked} skipped={skipped} failed={} / {}", fails.len(), files.len());
    assert!(fails.is_empty(), "不一致:\n{}", fails.iter().take(20).cloned().collect::<Vec<_>>().join("\n"));
    assert!(checked > 0 || skipped == files.len(), "没有任何文件被真正比对");
}
