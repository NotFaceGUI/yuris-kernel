//! 等价性测试(architecture §1 的永久自证):
//! 同一棵文件树,DirSource 与"该树打成合成 ypf 后的 YpfSource"必须给出
//! 逐字节一致的 `open` / `exists` / `list` 结果。

mod support {
    pub mod ypf_writer;
}

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use support::ypf_writer::{write_ypf, TestEntry, TestPack};
use yu_log::code::vfs;
use yu_vfs::{Codec, DataSource, DirSource, LookupKey, YpfDialect, YpfSource};

fn temp_dir(tag: &str) -> PathBuf {
    let base = std::env::var_os("YURIS_TEST_TMP")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = base.join(format!("yu-vfs-eq-{tag}-{}-{unique}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

struct TreeFile {
    path: &'static [u8],
    data: Vec<u8>,
    compress: bool,
    file_type: u8,
}

fn sample_tree() -> Vec<TreeFile> {
    vec![
        TreeFile {
            path: br"README.TXT",
            data: b"hello root".to_vec(),
            compress: false,
            file_type: 0,
        },
        TreeFile {
            path: br"CG\Ev\A.PNG",
            data: b"\x89PNG\r\n\x1a\nfake-png-body".to_vec(),
            compress: false,
            file_type: 2,
        },
        TreeFile {
            path: br"CG\Ev\B.JPG",
            data: (0..997u32).map(|i| (i % 251) as u8).collect(),
            compress: true,
            file_type: 3,
        },
        TreeFile {
            path: br"ysbin\yst00000.ybn",
            data: (0..4096u32).map(|i| (i % 253) as u8).collect(),
            compress: true,
            file_type: 0,
        },
        TreeFile {
            path: br"ysbin\yst00001.ybn",
            data: b"YSTB".to_vec(),
            compress: false,
            file_type: 0,
        },
        TreeFile {
            path: br"data\cfg\yscfg.dat",
            data: vec![0x42; 128],
            compress: true,
            file_type: 0,
        },
    ]
}

fn write_tree(root: &Path, files: &[TreeFile]) {
    for file in files {
        let rel = String::from_utf8(file.path.to_vec()).unwrap();
        let path = root.join(rel.replace('\\', "/"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &file.data).unwrap();
    }
}

fn key(raw: &[u8]) -> LookupKey {
    LookupKey::new(raw, Codec::Cp932).unwrap()
}

fn all_keys(files: &[TreeFile]) -> Vec<LookupKey> {
    let mut keys = vec![LookupKey::root()];
    for file in files {
        let raw = String::from_utf8(file.path.to_vec()).unwrap();
        let parts: Vec<&str> = raw.split('\\').collect();
        for n in 1..=parts.len() {
            keys.push(key(parts[..n].join("\\").as_bytes()));
        }
    }
    keys.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    keys.dedup();
    keys
}

fn list_signature(entries: &[yu_vfs::Entry]) -> Vec<(Vec<u8>, &'static str)> {
    entries
        .iter()
        .map(|e| (e.name.clone(), e.kind.as_str()))
        .collect()
}

fn pack_entries(files: &[TreeFile]) -> Vec<TestEntry> {
    files
        .iter()
        .map(|f| TestEntry {
            name: f.path.to_vec(),
            data: f.data.clone(),
            compress: f.compress,
            file_type: f.file_type,
        })
        .collect()
}

#[test]
fn dir_and_ypf_agree_on_open_exists_list() {
    let root = temp_dir("dir");
    let files = sample_tree();
    write_tree(&root, &files);

    let dir = DirSource::open(&root, Codec::Cp932).unwrap();
    assert_eq!(dir.skipped_count(), 0, "test tree must be pure ASCII");

    let pack_bytes = write_ypf(&TestPack::default(), &pack_entries(&files));
    let pack_path = root.join("synthetic.ypf");
    fs::write(&pack_path, &pack_bytes).unwrap();
    let ypf = YpfSource::open(&pack_path, Codec::Cp932).unwrap();

    assert_eq!(
        ypf.dialect(),
        YpfDialect::HashedLen { index_start: 0x20, name_key: 0xFF }
    );
    assert_eq!(ypf.stats().declared_entries, files.len() as u32);
    assert_eq!(ypf.stats().parsed_entries, files.len());
    assert!(ypf.stats().closed);
    assert_eq!(ypf.stats().name_hash_matches, files.len());

    for lookup in all_keys(&files) {
        assert_eq!(
            dir.exists(&lookup),
            ypf.exists(&lookup),
            "exists mismatch for {}",
            lookup.display()
        );
        match (dir.open(&lookup), ypf.open(&lookup)) {
            (Ok(a), Ok(b)) => assert_eq!(a, b, "open mismatch for {}", lookup.display()),
            (Err(a), Err(b)) => {
                assert_eq!(a.code, b.code, "error mismatch for {}", lookup.display())
            }
            (Ok(_), Err(b)) => panic!(
                "dir has {}, ypf fails {}: {}",
                lookup.display(),
                b.code,
                b.detail
            ),
            (Err(a), Ok(_)) => panic!(
                "ypf has {}, dir fails {}: {}",
                lookup.display(),
                a.code,
                a.detail
            ),
        }
        match (dir.list(&lookup), ypf.list(&lookup)) {
            (Ok(a), Ok(b)) => assert_eq!(
                list_signature(&a),
                list_signature(&b),
                "list mismatch for {}",
                lookup.display()
            ),
            (Err(a), Err(b)) => {
                assert_eq!(a.code, b.code, "list error mismatch for {}", lookup.display())
            }
            (a, b) => panic!(
                "list presence mismatch for {}: dir={:?} ypf={:?}",
                lookup.display(),
                a.map(|e| e.len()),
                b.map(|e| e.len())
            ),
        }
    }

    fs::remove_dir_all(&root).ok();
}

/// V2 验收:0.495 布局但 version=300 的包必须能读(引擎无版本分支;
/// 表与 meta 都不随版本变化)。
#[test]
fn engine_layout_with_version_300_is_readable() {
    let root = temp_dir("v300");
    let files = sample_tree();
    let pack_bytes = write_ypf(
        &TestPack {
            version: 300,
            ..TestPack::default()
        },
        &pack_entries(&files),
    );
    let pack_path = root.join("v300_engine_layout.ypf");
    fs::write(&pack_path, &pack_bytes).unwrap();

    let ypf = YpfSource::open(&pack_path, Codec::Cp932).unwrap();
    assert_eq!(ypf.header().version, 300);
    assert_eq!(
        ypf.dialect(),
        YpfDialect::HashedLen { index_start: 0x20, name_key: 0xFF }
    );
    assert!(ypf.stats().closed);
    assert_eq!(ypf.stats().name_hash_matches, files.len());
    assert_eq!(ypf.open(&key(b"ysbin/yst00000.ybn")).unwrap().len(), 4096);

    fs::remove_dir_all(&root).ok();
}

/// V1 验收:结构不匹配的包(错误长度表 + nameHash=0)必须失败,
/// 不得回退成 Ok + 垃圾名字。
#[test]
fn unverified_layout_without_hash_evidence_is_rejected() {
    let root = temp_dir("v300wrong");
    let files = sample_tree();
    let mut bytes = write_ypf(
        &TestPack {
            version: 300,
            ..TestPack::default()
        },
        &pack_entries(&files),
    );
    // 12 对表不是恒等表;把每条 lenEnc 按"恒等表"编码即可制造错误长度。
    // 先定位索引起点,逐条重写 lenEnc 与 nameHash。
    let mut at = 0x20usize;
    for file in &files {
        let name = file.path;
        let len_enc_at = at + 4;
        bytes[len_enc_at] = 0xFFu8.wrapping_sub(name.len() as u8);
        bytes[at..at + 4].copy_from_slice(&0u32.to_le_bytes());
        // 步长按错误解码后的长度计算不可知;此测试只需确认不是 Ok。
        let _ = name;
        at += 4 + 1 + name.len() + 22;
    }
    let path = root.join("v300_wrong12_hash0.ypf");
    fs::write(&path, &bytes).unwrap();

    let err = YpfSource::open(&path, Codec::Cp932).unwrap_err();
    assert_eq!(err.code, vfs::YPF_UNKNOWN_DIALECT, "{err:?}");
    assert_eq!(err.severity, yu_log::Severity::Unimplemented);

    fs::remove_dir_all(&root).ok();
}

/// V1/V2:0x24 前缀方言没有样本支撑,不再作为 Ok 回退;必须显式失败。
#[test]
fn four_byte_prefix_dialect_is_rejected() {
    let root = temp_dir("start24");
    let files = sample_tree();
    let pack_bytes = write_ypf(
        &TestPack {
            index_start: 0x24,
            ..TestPack::default()
        },
        &pack_entries(&files),
    );
    let pack_path = root.join("start24.ypf");
    fs::write(&pack_path, &pack_bytes).unwrap();

    let err = YpfSource::open(&pack_path, Codec::Cp932).unwrap_err();
    assert_eq!(err.code, vfs::YPF_UNKNOWN_DIALECT, "{err:?}");

    fs::remove_dir_all(&root).ok();
}

#[test]
fn missing_entries_yield_same_code_and_not_dir_is_symmetric() {
    let root = temp_dir("edge");
    let files = sample_tree();
    write_tree(&root, &files);

    let pack_bytes = write_ypf(&TestPack::default(), &pack_entries(&files));
    let pack_path = root.join("synthetic.ypf");
    fs::write(&pack_path, &pack_bytes).unwrap();

    let dir = DirSource::open(&root, Codec::Cp932).unwrap();
    let ypf = YpfSource::open(&pack_path, Codec::Cp932).unwrap();

    let missing_key = key(b"cg/ev/no-such-file.png");
    assert_eq!(dir.open(&missing_key).unwrap_err().code, vfs::MISSING);
    assert_eq!(ypf.open(&missing_key).unwrap_err().code, vfs::MISSING);

    let file_key = key(b"README.TXT");
    assert_eq!(dir.list(&file_key).unwrap_err().code, vfs::NOT_DIR);
    assert_eq!(ypf.list(&file_key).unwrap_err().code, vfs::NOT_DIR);

    fs::remove_dir_all(&root).ok();
}

#[test]
fn payload_hash_mismatch_is_unimplemented_not_silent() {
    let root = temp_dir("checksum");
    let mut bytes = write_ypf(
        &TestPack::default(),
        &[TestEntry {
            name: b"a.bin".to_vec(),
            data: vec![1, 2, 3, 4],
            compress: true,
            file_type: 0,
        }],
    );
    // payloadHash 位于条目末 4 字节,也是整个紧凑包的最后 4 字节。
    let n = bytes.len();
    bytes[n - 4..].copy_from_slice(&0xdead_beefu32.to_le_bytes());
    let path = root.join("bad.ypf");
    fs::write(&path, &bytes).unwrap();

    let source = YpfSource::open(&path, Codec::Cp932).unwrap();
    let diag = source.open(&key(b"a.bin")).unwrap_err();
    assert_eq!(diag.code, vfs::YPF_CHECKSUM);
    assert_eq!(diag.severity, yu_log::Severity::Unimplemented);

    fs::remove_dir_all(&root).ok();
}

/// V4:hash 是数据,不是"未记录"哨兵。索引 hash = 0 时存储负载的
/// MurmurHash2 通常非 0,必须失败(REA `FUN_0043f71c` 无条件比较)。
#[test]
fn zero_payload_hash_is_still_verified() {
    let root = temp_dir("zerohash");
    let mut bytes = write_ypf(
        &TestPack::default(),
        &[TestEntry {
            name: b"a.bin".to_vec(),
            data: b"payload".to_vec(),
            compress: false,
            file_type: 0,
            }],
    );
    let n = bytes.len();
    bytes[n - 4..].copy_from_slice(&0u32.to_le_bytes());
    let path = root.join("zerohash.ypf");
    fs::write(&path, &bytes).unwrap();
    let source = YpfSource::open(&path, Codec::Cp932).unwrap();
    let err = source.open(&key(b"a.bin")).unwrap_err();
    assert_eq!(err.code, vfs::YPF_CHECKSUM);
    fs::remove_dir_all(&root).ok();
}

/// V1:零条名字哈希命中 = key/表都不对。不得把解码出的字节当名字
/// 返回(旧实现会退回 NUL-XOR 读成垃圾名),必须显式 Unimplemented。
#[test]
fn zero_name_hash_matches_is_unimplemented() {
    let root = temp_dir("namehash");
    let mut bytes = write_ypf(
        &TestPack::default(),
        &[TestEntry {
            name: b"a.bin".to_vec(),
            data: b"x".to_vec(),
            compress: false,
            file_type: 0,
        }],
    );
    // nameHash 是索引区第一个 u32(索引起点 0x20)。
    bytes[0x20..0x24].copy_from_slice(&0x1234_5678u32.to_le_bytes());
    let path = root.join("namehash.ypf");
    fs::write(&path, &bytes).unwrap();

    let err = YpfSource::open(&path, Codec::Cp932).unwrap_err();
    assert_eq!(err.code, vfs::YPF_UNKNOWN_DIALECT, "{err:?}");
    assert_eq!(err.severity, yu_log::Severity::Unimplemented);

    fs::remove_dir_all(&root).ok();
}

#[test]
fn unknown_compression_byte_is_reported_as_unimplemented() {
    let root = temp_dir("flag");
    let mut bytes = write_ypf(
        &TestPack::default(),
        &[TestEntry {
            name: b"a.bin".to_vec(),
            data: vec![7u8; 16],
            compress: false,
            file_type: 0,
        }],
    );
    // 起点 + hash(4)+lenEnc(1)+name = type 字节;compressed 紧随其后。
    let comp_at = 0x20 + 5 + b"a.bin".len() + 1;
    assert_eq!(bytes[comp_at], 0);
    bytes[comp_at] = 5;

    let path = root.join("flag.ypf");
    fs::write(&path, &bytes).unwrap();
    let diag = YpfSource::open(&path, Codec::Cp932).unwrap_err();
    assert_eq!(diag.code, vfs::YPF_UNSUPPORTED);
    assert_eq!(diag.severity, yu_log::Severity::Unimplemented);

    fs::remove_dir_all(&root).ok();
}

#[test]
fn stored_size_mismatch_is_rejected() {
    let root = temp_dir("bad");
    let mut bytes = write_ypf(
        &TestPack::default(),
        &[TestEntry {
            name: b"a.bin".to_vec(),
            data: vec![1, 2, 3],
            compress: false,
            file_type: 0,
        }],
    );
    // usize 位于 hash(4)+lenEnc(1)+name + type(1)+compressed(1)。
    let usize_at = 0x20 + 4 + 1 + b"a.bin".len() + 2;
    assert_eq!(
        u32::from_le_bytes(bytes[usize_at..usize_at + 4].try_into().unwrap()),
        3
    );
    bytes[usize_at..usize_at + 4].copy_from_slice(&9u32.to_le_bytes());

    let path = root.join("bad.ypf");
    fs::write(&path, &bytes).unwrap();
    let diag = YpfSource::open(&path, Codec::Cp932).unwrap_err();
    assert_eq!(diag.code, vfs::YPF_BAD_ENTRY);

    fs::remove_dir_all(&root).ok();
}

#[test]
fn bad_magic_and_version_are_explicit() {
    let root = temp_dir("badsig");
    let path = root.join("bad.ypf");
    fs::write(&path, b"NOPE\0\0\0\0").unwrap();
    assert_eq!(
        YpfSource::open(&path, Codec::Cp932).unwrap_err().code,
        vfs::YPF_BAD_MAGIC
    );

    let mut bytes = write_ypf(&TestPack::default(), &[]);
    bytes[4..8].copy_from_slice(&200u32.to_le_bytes());
    fs::write(&path, &bytes).unwrap();
    assert_eq!(
        YpfSource::open(&path, Codec::Cp932).unwrap_err().code,
        vfs::YPF_BAD_VERSION
    );

    fs::remove_dir_all(&root).ok();
}

#[test]
fn v479_special_version_is_accepted() {
    let root = temp_dir("v479");
    let bytes = write_ypf(
        &TestPack {
            version: 479,
            ..TestPack::default()
        },
        &[TestEntry {
            name: b"a.txt".to_vec(),
            data: b"v479".to_vec(),
            compress: true,
            file_type: 0,
        }],
    );
    let path = root.join("v479.ypf");
    fs::write(&path, &bytes).unwrap();
    let source = YpfSource::open(&path, Codec::Cp932).unwrap();
    assert_eq!(source.header().version, 479);
    assert_eq!(source.open(&key(b"a.txt")).unwrap(), b"v479");
    fs::remove_dir_all(&root).ok();
}
