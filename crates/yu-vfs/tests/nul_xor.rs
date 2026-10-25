//! 旧 NUL-XOR 索引方言的合成测试。
//!
//! **本机没有该方言的真实包**(官方 0.495 语料与旧商业包均为 HashedLen 方言)。
//! 任务 E 的 V1 结论:未验证方言**不再有 Ok 回退** —— 结构不匹配的包
//! 一律 `Err(Unimplemented)`,绝不把解码出的字节当名字返回。
//! 该方言整体标注为未验证(DIV-002),不作为任何对齐依据。

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use yu_vfs::{Codec, YpfSource};

struct Entry {
    /// 包内原始名字字节(XOR 之前,含虚拟根前缀)。
    name: Vec<u8>,
    /// 资源型条目的原始类型码;`None` = bn 型(有 flag 字节)。
    type_code: Option<u8>,
    data: Vec<u8>,
    compress: bool,
}

fn temp_dir(tag: &str) -> PathBuf {
    // 缺省落在仓库内 `target/test-tmp`:受限环境只允许写工作区,
    // 系统 TEMP 会 `PermissionDenied`。`YURIS_TEST_TMP` 仍可覆盖。
    let base = std::env::var_os("YURIS_TEST_TMP")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp"));
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = base.join(format!("yu-vfs-nulxor-{tag}-{}-{unique}", std::process::id()));
    fs::create_dir_all(&dir)
        .unwrap_or_else(|e| panic!("create_dir_all({}) 失败: {e}", dir.display()));
    dir
}

fn write_nul_xor_pack(key: u8, entries: &[Entry]) -> Vec<u8> {
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::io::Write;

    struct Payload {
        flag: u8,
        uncompressed: u32,
        compressed: u32,
        bytes: Vec<u8>,
    }

    let payloads: Vec<Payload> = entries
        .iter()
        .map(|entry| {
            if entry.compress {
                let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
                encoder.write_all(&entry.data).unwrap();
                let bytes = encoder.finish().unwrap();
                Payload {
                    flag: 1,
                    uncompressed: entry.data.len() as u32,
                    compressed: bytes.len() as u32,
                    bytes,
                }
            } else {
                Payload {
                    flag: 0,
                    uncompressed: entry.data.len() as u32,
                    compressed: entry.data.len() as u32,
                    bytes: entry.data.clone(),
                }
            }
        })
        .collect();

    // 旧方言没有 4 字节前缀,起点固定 0x24。
    let mut index = vec![0u8; 4];
    let mut record_starts = Vec::new();
    for entry in entries {
        record_starts.push(index.len());
        index.extend(entry.name.iter().map(|b| b ^ key));
        if let Some(code) = entry.type_code {
            index.push(code);
        }
        index.push(0);
        if entry.type_code.is_none() {
            index.push(0); // flag 占位
        }
        index.extend_from_slice(&0u32.to_le_bytes());
        index.extend_from_slice(&0u32.to_le_bytes());
        index.extend_from_slice(&0u32.to_le_bytes());
        index.extend_from_slice(&0u32.to_le_bytes());
        index.extend_from_slice(&[0u8; 8]);
    }

    let data_start = 0x20 + index.len();
    let mut data_offset = data_start as u32;
    let mut data = Vec::new();
    for (i, payload) in payloads.iter().enumerate() {
        let entry = &entries[i];
        let base = record_starts[i]
            + entry.name.len()
            + usize::from(entry.type_code.is_some())
            + 1;
        let fields = if entry.type_code.is_some() {
            base
        } else {
            index[base] = payload.flag;
            base + 1
        };
        index[fields..fields + 4].copy_from_slice(&payload.uncompressed.to_le_bytes());
        index[fields + 4..fields + 8].copy_from_slice(&payload.compressed.to_le_bytes());
        index[fields + 8..fields + 12].copy_from_slice(&data_offset.to_le_bytes());
        data.extend_from_slice(&payload.bytes);
        data_offset += payload.compressed;
    }

    let mut out = Vec::with_capacity(data_start + data.len());
    out.extend_from_slice(b"YPF\0");
    out.extend_from_slice(&495u32.to_le_bytes()); // 版本在已验区间,让测试落在布局判定上
    out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    out.extend_from_slice(&(data_start as u32).to_le_bytes());
    out.extend_from_slice(&[0u8; 16]);
    out.extend_from_slice(&index);
    out.extend_from_slice(&data);
    out
}

fn sample_entries() -> Vec<Entry> {
    vec![
        Entry {
            name: br"$ysbin\yst00000.ybn".to_vec(),
            type_code: None,
            data: (0..2048u32).map(|i| (i % 249) as u8).collect(),
            compress: true,
        },
        Entry {
            name: br"%ysbin\ysc.ybn".to_vec(),
            type_code: None,
            data: b"YSCD".to_vec(),
            compress: false,
        },
        Entry {
            name: br"9cg\ev\a.png".to_vec(),
            type_code: Some(2),
            data: b"\x89PNG\r\n\x1a\nstored".to_vec(),
            compress: false,
        },
    ]
}

const KEY: u8 = 0xC9;

/// V1:旧 NUL-XOR 方言必须显式失败,不能 Ok + 垃圾名字。
#[test]
fn nul_xor_dialect_is_rejected_with_unimplemented() {
    let root = temp_dir("parse");
    let path = root.join("v500.ypf");
    fs::write(&path, write_nul_xor_pack(KEY, &sample_entries())).unwrap();

    let err = YpfSource::open(&path, Codec::Cp932).unwrap_err();
    assert_eq!(err.code, yu_log::code::vfs::YPF_UNKNOWN_DIALECT, "{err:?}");
    assert_eq!(err.severity, yu_log::Severity::Unimplemented);

    fs::remove_dir_all(&root).ok();
}

#[test]
fn corrupted_nul_xor_index_is_rejected() {
    let root = temp_dir("badkey");
    let path = root.join("v500.ypf");
    fs::write(&path, write_nul_xor_pack(KEY, &sample_entries())).unwrap();
    let mut bytes = fs::read(&path).unwrap();
    for b in bytes[0x24..0x24 + 60].iter_mut() {
        *b ^= 0x55;
    }
    fs::write(&path, &bytes).unwrap();
    assert!(YpfSource::open(&path, Codec::Cp932).is_err());
    fs::remove_dir_all(&root).ok();
}
