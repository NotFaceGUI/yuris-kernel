//! YCG 真实语料回归:4.7x 商业包的 `.png` 条目是 YCG 容器。
//!
//! 环境变量(缺省显式跳过并计数,不空跑通过):
//! - `YURIS_SAMPLE_YCG_PACK`  = 含 YCG 条目的 `.ypf` 路径;
//! - `YURIS_SAMPLE_YCG_ENTRY` = 包内条目名(缺省 `cg/tr/bg_black.png`)。
//!
//! 证据锚点:`yu_format::ycg` 模块文档(官方 0.479 `FUN_004254e4`)。

use std::sync::atomic::{AtomicUsize, Ordering};

use yu_vfs::{Codec, DataSource, LookupKey, YpfSource};

static SKIPPED: AtomicUsize = AtomicUsize::new(0);

#[test]
fn real_pack_ycg_entry_decodes() {
    let Ok(pack) = std::env::var("YURIS_SAMPLE_YCG_PACK") else {
        SKIPPED.fetch_add(1, Ordering::Relaxed);
        println!("SKIP real_pack_ycg_entry_decodes: 未设置 YURIS_SAMPLE_YCG_PACK");
        return;
    };
    let entry = std::env::var("YURIS_SAMPLE_YCG_ENTRY")
        .unwrap_or_else(|_| "cg/tr/bg_black.png".to_string());
    assert_eq!(SKIPPED.load(Ordering::Relaxed), 0, "前置跳过计数被污染");
    let source = YpfSource::open(std::path::Path::new(&pack), Codec::Cp932)
        .expect("打开 YPF 失败");
    let key = LookupKey::new(entry.as_bytes(), Codec::Cp932).expect("条目 key 非法");
    let bytes = source.open(&key).expect("包内条目不存在");
    assert!(
        yu_format::ycg::is_ycg(&bytes),
        "条目不是 YCG 容器: magic={:02x?}",
        &bytes[..4.min(bytes.len())]
    );
    let image = yu_format::ycg::decode(&bytes).expect("YCG 解码失败");
    assert!(image.width > 0 && image.height > 0);
    assert_eq!(
        image.rgba.len(),
        image.width as usize * image.height as usize * 4
    );
    // `cg/tr/bg_black.png` 是官方黑幕 CG:首像素 BGRA(0,0,0,255)→RGBA 全 0 + A=255。
    if entry.ends_with("bg_black.png") {
        assert_eq!(&image.rgba[..4], &[0, 0, 0, 255]);
    }
    println!(
        "decoded {}x{} ({} bytes RGBA) from {}",
        image.width,
        image.height,
        image.rgba.len(),
        entry
    );
}
