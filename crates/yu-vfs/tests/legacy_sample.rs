//! 真实数据验收 —— v<479 legacy YPF 样本。
//!
//! 样本由环境变量提供:
//! - `YURIS_SAMPLE_LEGACY_YPF`: 任一 v<479 的商业包(本机样本为 v473,
//!   432 条,u32 offset + CRC32 名字 + Adler32 负载)。
//!
//! 样本缺失时显式打印 skip;`YURIS_REQUIRE_SAMPLES=1` 时硬失败。
//! 断言只读取索引与少量按偏移定位的负载,不整包载入。

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use yu_vfs::ypf::{EntryWidth, HashAlg, IndexSizeMode};
use yu_vfs::{Codec, DataSource, LookupKey, YpfSource};

static SKIP_COUNT: AtomicUsize = AtomicUsize::new(0);

fn skip(reason: &str) {
    if std::env::var_os("YURIS_REQUIRE_SAMPLES").is_some_and(|v| v != "0" && !v.is_empty()) {
        panic!("YURIS_REQUIRE_SAMPLES=1 but sample unavailable: {reason}");
    }
    let total = SKIP_COUNT.fetch_add(1, Ordering::SeqCst) + 1;
    eprintln!("SKIP[count={total}] {reason}");
}

fn sample() -> Option<PathBuf> {
    let Some(path) = std::env::var_os("YURIS_SAMPLE_LEGACY_YPF") else {
        skip("YURIS_SAMPLE_LEGACY_YPF is not set (v<479 legacy pack)");
        return None;
    };
    let path = PathBuf::from(path);
    if path.is_file() {
        Some(path)
    } else {
        skip(&format!(
            "YURIS_SAMPLE_LEGACY_YPF={} does not exist",
            path.display()
        ));
        None
    }
}

#[test]
fn embedded_ypf_sample() {
    let Some(path) = std::env::var_os("YURIS_SAMPLE_EMBEDDED_YPF").map(PathBuf::from) else {
        eprintln!("SKIP YURIS_SAMPLE_EMBEDDED_YPF is not set");
        return;
    };
    if !path.is_file() {
        eprintln!("SKIP embedded sample missing: {}", path.display());
        return;
    }
    let source = YpfSource::open(&path, Codec::Cp932).unwrap();
    eprintln!(
        "EMBEDDED SAMPLE: {} base=0x{:x} version={} entries={}",
        path.display(),
        source.base_offset(),
        source.header().version,
        source.entries().len()
    );
    assert!(source.base_offset() > 0);
    assert!(!source.entries().is_empty());
    assert!(source.stats().closed);
}

#[test]
fn legacy_pack_uses_crc32_adler32_profile() {
    let Some(path) = sample() else {
        return;
    };
    eprintln!("LEGACY SAMPLE: {}", path.display());
    let source = YpfSource::open(&path, Codec::Cp932).unwrap();
    let header = source.header();
    let stats = source.stats();
    let profile = source.profile();

    eprintln!(
        "header: version={} entries={} dataStart={:#x} fileLen={}",
        header.version, header.entry_count, header.data_start, header.file_len
    );
    eprintln!(
        "profile: {:?} | index={:?} meta={} name={} data={} key={:#04x} table={:?}",
        source.dialect(),
        profile.index_size,
        profile.meta_len(),
        profile.name_hash.as_str(),
        profile.data_hash.as_str(),
        stats.name_key,
        profile.len_map
    );

    assert!(header.version < 479, "legacy sample must be v<479");
    assert_eq!(profile.index_size, IndexSizeMode::AbsoluteDataStart);
    assert_eq!(profile.entry_width, EntryWidth::Meta18);
    assert_eq!(profile.name_hash, HashAlg::Crc32);
    assert_eq!(profile.data_hash, HashAlg::Adler32);
    assert_eq!(source.dialect().as_str(), "crc32-u32");

    assert_eq!(stats.declared_entries, header.entry_count);
    assert_eq!(stats.parsed_entries, header.entry_count as usize);
    assert_eq!(source.entries().len(), header.entry_count as usize);
    assert!(stats.closed, "index walk must end exactly at dataStart");
    assert_eq!(stats.consumed_bytes, stats.index_bytes);
    assert_eq!(
        stats.name_hash_matches, stats.parsed_entries,
        "CRC32(name) must match for every entry"
    );

    for entry in source.entries() {
        assert!(!entry.name.is_empty());
        assert!(entry.name_hash_ok);
        assert!(entry.data_offset >= u64::from(header.data_start));
        assert!(entry.data_offset + u64::from(entry.compressed_size) <= header.file_len);
        if entry.compressed {
            assert!(entry.compressed_size <= entry.uncompressed_size);
        } else {
            assert_eq!(entry.compressed_size, entry.uncompressed_size);
        }
    }

    // 抽读一个 PNG:走完整 open(含 Adler32 校验 + zlib 解压)。
    let png = source
        .entries()
        .iter()
        .find(|e| e.name.ends_with(b".png"))
        .expect("legacy sample must contain a PNG");
    let key = LookupKey::new(&png.name, Codec::Cp932).unwrap();
    let data = source.open(&key).unwrap();
    assert!(
        data.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]),
        "{} is not a PNG after decompression",
        String::from_utf8_lossy(&png.name)
    );
    assert_eq!(data.len(), png.uncompressed_size as usize);
}
