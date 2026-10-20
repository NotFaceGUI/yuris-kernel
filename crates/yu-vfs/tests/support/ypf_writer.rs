//! 仅测试用的 `.ypf` 写入器(官方 0.495 布局)。
//!
//! 目的: 为「DirSource ≡ YpfSource」等价性测试与索引起点探测测试提供同一棵
//! 树的封包版本。字段布局与哈希语义来自官方 YSPac 0.495 真包的实测
//! (见 `crates/yu-vfs/src/ypf.rs` 顶部与 `docs/formats/ypf.md`)。
//! **不用于生产**,也不保证与官方打包器的字节级输出一致(官方包存在
//! 未索引的历史残留负载,本写入器只写紧凑数据区)。

#![allow(dead_code)]

use flate2::write::ZlibEncoder;
use flate2::Compression;
use std::io::Write;
use yu_vfs::ypf::{murmur2, name_len_code};

/// 一个待写入的条目。
pub struct TestEntry {
    /// 包内名字(原始字节,写入时逐字节取反)。
    pub name: Vec<u8>,
    pub data: Vec<u8>,
    /// `true` = zlib 压缩(compressed=1);`false` = 原样存储(compressed=0)。
    pub compress: bool,
    /// 资源类型码(实测 0=ybn / 2=png / 3=jpg / 6=ogg / 9=psb)。
    pub file_type: u8,
}

pub struct TestPack {
    pub version: u32,
    /// 索引起点: 0x20(官方)或 0x24(旧前缀方言)。
    pub index_start: usize,
    /// 0x10..0x20 的保留字节。
    pub header_reserved: [u8; 16],
    /// `dataOffset` 高 32 位(官方恒 0;REA 读 8 字节偏移)。
    pub unknown0: u32,
    /// 是否把 `payloadHash` 写成 0 以外的错误值(测试校验失败路径)。
    pub corrupt_payload_hash: bool,
}

impl Default for TestPack {
    fn default() -> Self {
        Self {
            version: 495,
            index_start: 0x20,
            header_reserved: [0u8; 16],
            unknown0: 0,
            corrupt_payload_hash: false,
        }
    }
}

/// 打包。返回完整包字节。
pub fn write_ypf(pack: &TestPack, entries: &[TestEntry]) -> Vec<u8> {
    struct Payload {
        compressed: bool,
        uncompressed: u32,
        stored: Vec<u8>,
    }

    let payloads: Vec<Payload> = entries
        .iter()
        .map(|entry| {
            if entry.compress {
                let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
                encoder.write_all(&entry.data).unwrap();
                let stored = encoder.finish().unwrap();
                Payload {
                    compressed: true,
                    uncompressed: entry.data.len() as u32,
                    stored,
                }
            } else {
                Payload {
                    compressed: false,
                    uncompressed: entry.data.len() as u32,
                    stored: entry.data.clone(),
                }
            }
        })
        .collect();

    let prefix_len = pack.index_start.saturating_sub(0x20);
    let mut index = vec![0u8; prefix_len];
    let mut record_starts = Vec::with_capacity(entries.len());
    for entry in entries {
        let name_len = entry.name.len();
        assert!(name_len <= 255, "test writer only supports names <= 255 bytes");
        assert!(name_len > 0, "test writer does not write empty names");
        record_starts.push(index.len());
        index.extend_from_slice(&murmur2(&entry.name, 0).to_le_bytes());
        index.push(name_len_code(name_len as u8, pack.version));
        index.extend(entry.name.iter().map(|b| !*b));
        index.push(entry.file_type);
        index.push(0); // compressed 占位
        index.extend_from_slice(&0u32.to_le_bytes()); // usize
        index.extend_from_slice(&0u32.to_le_bytes()); // csize
        index.extend_from_slice(&0u32.to_le_bytes()); // dataOffset (low)
        index.extend_from_slice(&pack.unknown0.to_le_bytes()); // dataOffset (high)
        index.extend_from_slice(&0u32.to_le_bytes()); // payloadHash
    }

    let data_start = 0x20 + index.len();
    let mut data_offset = data_start as u64;
    let mut data = Vec::new();
    for (i, payload) in payloads.iter().enumerate() {
        let name_len = entries[i].name.len();
        let meta = record_starts[i] + 4 + 1 + name_len;
        index[meta + 1] = u8::from(payload.compressed);
        index[meta + 2..meta + 6].copy_from_slice(&payload.uncompressed.to_le_bytes());
        index[meta + 6..meta + 10].copy_from_slice(&(payload.stored.len() as u32).to_le_bytes());
        index[meta + 10..meta + 14].copy_from_slice(&(data_offset as u32).to_le_bytes());
        let hash = if pack.corrupt_payload_hash {
            murmur2(&payload.stored, 0) ^ 0xDEAD_BEEF
        } else {
            murmur2(&payload.stored, 0)
        };
        index[meta + 18..meta + 22].copy_from_slice(&hash.to_le_bytes());
        data.extend_from_slice(&payload.stored);
        data_offset += payload.stored.len() as u64;
    }

    let mut out = Vec::with_capacity(data_start + data.len());
    out.extend_from_slice(b"YPF\0");
    out.extend_from_slice(&pack.version.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    out.extend_from_slice(&(data_start as u32).to_le_bytes());
    out.extend_from_slice(&pack.header_reserved);
    out.extend_from_slice(&index);
    out.extend_from_slice(&data);
    out
}
