//! `.ypf` 封包数据源。
//!
//! # 布局
//!
//! 两代条目布局,由**固定候选 + 结构校验**裁决(版本号不参与):
//!
//! - `Meta22`(官方 YSPac 0.495,1,407 条实测):u64 dataOffset + Murmur2
//!   名字/负载;
//! - `Meta18`(v473 商业包,432 条实测):u32 dataOffset + CRC32 名字 +
//!   Adler32 负载。
//!
//! 名字长度表同时枚举 V000 / V004(<0x100)/ V010(0x12C..0x196)/ V500;
//! 名字 XOR key 先试提示值,再试 0xFF/0xC9/0xBF,索引闭合后允许全量 256
//! key 兜底。只有"索引恰好闭合 + 全量名字哈希命中 + 偏移/长度在数据区"
//! 的候选才会被接受。
//!
//! ```text
//! +0x00  "YPF\0"
//! +0x04  u32 version            (诊断字段,不参与布局判定)
//! +0x08  u32 entryCount
//! +0x0C  u32 dataStart          (v>=300 绝对 dataStart;v<300 索引块大小)
//! +0x10  u8[16] reserved        (官方产出全 0,原样保留)
//! +0x20  index[entryCount]      (起点 0x20;0x24 前缀方言无样本,拒绝)
//! 数据区: dataOffset 直接指向负载首字节,没有独立记录头
//! ```
//!
//! `Meta22` 条目(步长 = `nameLen + 27`):
//!
//! ```text
//! +0x00  u32 nameHash      MurmurHash2(名字字节, seed=0)
//! +0x04  u8  lenEnc        nameLen = T[(0xFF - lenEnc) & 0xFF]
//! +0x05  u8  name[nameLen] 逐字节取反 (~b);以 `\` 分隔、小写
//! +...   u8  fileType      0=ybn 2=png 3=jpg 6=ogg 9=psb(实测集合)
//! +...   u8  isCompressed  0 = 原样存储,1 = zlib/Snappy 流
//! +...   u32 usize         解压后大小
//! +...   u32 csize         存储字节数
//! +...   u64 dataOffset    负载绝对偏移
//! +...   u32 payloadHash   MurmurHash2(存储字节, seed=0)
//! ```
//!
//! `Meta18` 条目(步长 = `nameLen + 23`)把 offset 缩成 u32,去掉高 32 位;
//! 名字/负载哈希换成 CRC32/Adler32。两代的 `fileType` 与 `isCompressed`
//! 都是两个独立字节。
//!
//! # 未决
//!
//! - 名字 key 不是 0xFF 的包仍会产出告警(`is_verified` 只认 0xFF)。
//! - WZ 载荷反变换按 RxYuris `DecodeFile_WZ` 实现,但**无真包样本**,
//!   置信度 `Likely`;只在标准 zlib/Snappy 都失败后尝试。
//! - EXE 内嵌 YPF 已支持(`MZ` → `YSER+header_size` → `YPF\0`),v408
//!   真实 installer 样本 28/28 条闭合。
//! - 旧 NUL-XOR 方言本机无真样本(DIV-002),结构不匹配一律
//!   `Unimplemented`,不产出垃圾名字。

use crate::{missing, Codec, DataSource, DirTree, Entry, LookupKey};
use flate2::read::ZlibDecoder;
use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use yu_log::{code, Boundary, Diag};

/// 封包魔数。
pub const YPF_MAGIC: [u8; 4] = *b"YPF\0";
/// **已实测验证过的最高版本**(仅信息,不是闸门)。
///
/// 证据:官方索引装载器 `FUN_0045e3b8`(0.495 `yu-ris.exe`,
/// sha256 `4f5eefa3…`)**逐字节无版本分支** ——
/// 索引起点恒 `0x20`、步长恒 `nameLen + 0x1B`、长度码表恒
/// `T[0xFF - lenEnc]`、名字逐字节取反,`version` 字段只被读入而从不
/// 参与分支(见 `docs/tasks/task-e-fixes.md` §4 与
/// `docs/divergences/DIV-002-ypf-nul-xor-dialect.md`)。
///
/// 版本号**不参与**布局判定:候选 profile 固定枚举,接受与否由结构
/// 校验(索引闭合、全量名字哈希、偏移/长度边界)决定。曾经写死
/// `version > 495 ⇒ 拒绝` 会把 v500 商业包拒之门外;写死下限同样会
/// 误杀能由结构证明的老包。
pub const VERIFIED_MAX_VERSION: u32 = 495;
/// 固定文件头长度。
pub const HEADER_LEN: usize = 0x20;
/// 索引起点(REA `FUN_0045e3b8` 恒 `0x20`)。
pub const INDEX_START: usize = 0x20;
/// 名字之后的固定字段长度: type(1)+compressed(1)+usize(4)+csize(4)+
/// dataOffset(u64,8)+payloadHash(4) = 22。
pub const ENTRY_META_LEN: usize = 22;
/// 步长 = 5 + 22 = nameLen + 27(REA `FUN_0045e3b8` 的 `+0x1B`)。
pub const ENTRY_STEP_EXTRA: usize = 27;
/// v<479 条目的固定字段长度: type(1)+compressed(1)+usize(4)+csize(4)+
/// dataOffset(u32,4)+dataChecksum(4) = 18。
pub const LEGACY_ENTRY_META_LEN: usize = 18;
/// v<479 步长 = 5 + 18 = nameLen + 23。
pub const LEGACY_ENTRY_STEP_EXTRA: usize = 23;
/// 条目数上限(引擎实测 99,999 为上限;同时用 `index_bytes / 27` 交叉约束)。
pub const MAX_ENTRY_COUNT: usize = 99_999;
/// 索引区上限(防御损坏包导致巨量分配)。
pub const MAX_INDEX_BYTES: usize = 256 * 1024 * 1024;

/// 长度解码表 T 的 12 对固定交换(默认版本;REA `FUN_00462a10` 初始化语义)。
const NAME_LEN_SWAPS_00: [(u8, u8); 12] = [
    (3, 0x48),
    (6, 0x35),
    (9, 0x0B),
    (0x0C, 0x10),
    (0x0D, 0x13),
    (0x11, 0x19),
    (0x15, 0x1B),
    (0x1C, 0x1E),
    (0x20, 0x23),
    (0x26, 0x29),
    (0x2C, 0x2F),
    (0x2E, 0x32),
];

const fn build_table<const N: usize>(swaps: &[(u8, u8); N]) -> [u8; 256] {
    let mut table = [0u8; 256];
    let mut i = 0;
    while i < 256 {
        table[i] = i as u8;
        i += 1;
    }
    let mut j = 0;
    while j < N {
        let (a, b) = swaps[j];
        let tmp = table[a as usize];
        table[a as usize] = table[b as usize];
        table[b as usize] = tmp;
        j += 1;
    }
    table
}

const NAME_LEN_TABLE_00: [u8; 256] = build_table(&NAME_LEN_SWAPS_00);

/// v<0x100 的长度表变体(GARbro `SwapTable04` / Script-Editor `_SWAP_TABLE_04`)。
const NAME_LEN_SWAPS_04: [(u8, u8); 10] = [
    (0x0C, 0x10),
    (0x11, 0x19),
    (0x1C, 0x1E),
    (0x09, 0x0B),
    (0x0D, 0x13),
    (0x15, 0x1B),
    (0x20, 0x23),
    (0x26, 0x29),
    (0x2C, 0x2F),
    (0x2E, 0x32),
];

/// 0x12C..0x196 的长度表变体(GARbro `SwapTable10` / Script-Editor `_SWAP_TABLE_10`)。
const NAME_LEN_SWAPS_10: [(u8, u8); 7] = [
    (0x09, 0x0B),
    (0x0D, 0x13),
    (0x15, 0x1B),
    (0x20, 0x23),
    (0x26, 0x29),
    (0x2C, 0x2F),
    (0x2E, 0x32),
];

const NAME_LEN_TABLE_04: [u8; 256] = build_table(&NAME_LEN_SWAPS_04);
const NAME_LEN_TABLE_10: [u8; 256] = build_table(&NAME_LEN_SWAPS_10);

/// v500 起的长度表变体(yuri `NLMapV500` / YPF_Tool 对照;本机暂无真样本)。
const NAME_LEN_SWAPS_V500: [(u8, u8); 12] = [
    (3, 0x0A),
    (6, 0x35),
    (9, 0x0B),
    (0x0C, 0x10),
    (0x0D, 0x13),
    (0x11, 0x18),
    (0x15, 0x1B),
    (0x1C, 0x1E),
    (0x20, 0x23),
    (0x26, 0x29),
    (0x2C, 0x2F),
    (0x2E, 0x14),
];

const NAME_LEN_TABLE_V500: [u8; 256] = build_table(&NAME_LEN_SWAPS_V500);

/// CRC32(IEEE)查表,反射多项式 `0xEDB88320`。
const fn build_crc32_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut crc = i as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

const CRC32_TABLE: [u32; 256] = build_crc32_table();

/// 长度解码表。**与版本无关**(REA `FUN_00462a10`:恒等表 + 12 对交换,
/// 函数内无版本分支);`_version` 仅为调用方兼容保留。
pub fn name_len_table(_version: u32) -> &'static [u8; 256] {
    &NAME_LEN_TABLE_00
}

/// `meta` 长度:恒 22 字节(REA `FUN_0045e3b8` 的 `+0x16`);
/// `_version` 仅为调用方兼容保留。
pub fn meta_len(_version: u32) -> usize {
    ENTRY_META_LEN
}

/// 由索引里的长度码求名字字节数: `T[(0xFF - lenEnc) & 0xFF]`。
pub fn name_len_from_code(len_enc: u8, version: u32) -> u8 {
    name_len_table(version)[((0xFFu16 - len_enc as u16) & 0xFF) as usize]
}

/// 由名字字节数求索引里的长度码(写入器/测试用;与 [`name_len_from_code`] 互逆)。
pub fn name_len_code(name_len: u8, version: u32) -> u8 {
    0xFFu8.wrapping_sub(name_len_table(version)[name_len as usize])
}

/// MurmurHash2(32 位,seed 由调用方给出)。
///
/// 官方包实测: 索引 `nameHash` = `murmur2(name, 0)`,`payloadHash` =
/// `murmur2(存储字节, 0)`,各 1,407/1,407 相等。
pub fn murmur2(data: &[u8], seed: u32) -> u32 {
    murmur2_xor(data, 0, seed)
}

/// 逐字节 XOR `key` 后的 MurmurHash2,**零分配**。
///
/// 等价于 `murmur2(&data.iter().map(|b| b ^ key).collect::<Vec<_>>(), seed)`,
/// 但只在读字节处异或,不构造中间缓冲。名字 key 探测要对每个候选 key
/// (最多 256 个)重算一遍;12k 条包下逐 key 分配 Vec 是纯浪费。
pub fn murmur2_xor(data: &[u8], key: u8, seed: u32) -> u32 {
    const M: u32 = 0x5bd1_e995;
    const R: u32 = 24;

    let mut h: u32 = seed ^ (data.len() as u32);
    let mut chunks = data.chunks_exact(4);
    for chunk in &mut chunks {
        let mut k = u32::from_le_bytes([
            chunk[0] ^ key,
            chunk[1] ^ key,
            chunk[2] ^ key,
            chunk[3] ^ key,
        ]);
        k = k.wrapping_mul(M);
        k ^= k >> R;
        k = k.wrapping_mul(M);
        h = h.wrapping_mul(M);
        h ^= k;
    }
    let rem = chunks.remainder();
    match rem.len() {
        3 => {
            h ^= (u32::from(rem[2] ^ key)) << 16;
            h ^= (u32::from(rem[1] ^ key)) << 8;
            h ^= u32::from(rem[0] ^ key);
            h = h.wrapping_mul(M);
        }
        2 => {
            h ^= (u32::from(rem[1] ^ key)) << 8;
            h ^= u32::from(rem[0] ^ key);
            h = h.wrapping_mul(M);
        }
        1 => {
            h ^= u32::from(rem[0] ^ key);
            h = h.wrapping_mul(M);
        }
        _ => {}
    }
    h ^= h >> 13;
    h = h.wrapping_mul(M);
    h ^= h >> 15;
    h
}

/// CRC32(IEEE,与 zlib 一致):v<479 包的名字校验。
///
/// 证据:v473 包 432/432 条 `CRC32(name)` 与索引存储值相等;官方 YSPac
/// 0.466 二进制无 Murmur 常量、0.479/0.495 才出现 `0x5BD1E995`。
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc = CRC32_TABLE[((crc ^ u32::from(byte)) & 0xFF) as usize] ^ (crc >> 8);
    }
    !crc
}

/// 逐字节 XOR `key` 后的 CRC32(零分配;名字 key 探测用)。
fn crc32_xor(data: &[u8], key: u8) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc = CRC32_TABLE[((crc ^ u32::from(byte ^ key)) & 0xFF) as usize] ^ (crc >> 8);
    }
    !crc
}

/// Adler32:v<479 包的负载校验。
///
/// 证据:v473 包 432/432 条 `Adler32(存储字节)` 与索引存储值相等。
pub fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65_521;
    const CHUNK: usize = 5_552; // zlib 的 NMAX:保证 b 不溢出 u32
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(CHUNK) {
        for &byte in chunk {
            a += u32::from(byte);
            b += a;
        }
        a %= MOD;
        b %= MOD;
    }
    (b << 16) | a
}

/// 逐字节 XOR `key` 后的 Adler32(零分配;名字 key 探测用)。
fn adler32_xor(data: &[u8], key: u8) -> u32 {
    const MOD: u32 = 65_521;
    const CHUNK: usize = 5_552;
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(CHUNK) {
        for &byte in chunk {
            a += u32::from(byte ^ key);
            b += a;
        }
        a %= MOD;
        b %= MOD;
    }
    (b << 16) | a
}

/// xxHash32(v500 商业包的负载校验)。
///
/// 证据:`<v500-sample>` 的 `pac/bn.ypf`(YPF version 500,Meta22/V500):
/// `nameHash` 仍是 `murmur2(name, 0)`,但 `payloadHash` 是
/// `xxh32(存储字节, 0)`;抽验 `ysc.ybn`/`yst00034.ybn`/`yst00145.ybn`/
/// `yscfg.ybn`/`yst00236.ybn` 5/5 相等。
pub fn xxh32(data: &[u8], seed: u32) -> u32 {
    xxh32_xor(data, 0, seed)
}

fn xxh32_xor(data: &[u8], key: u8, seed: u32) -> u32 {
    const P1: u32 = 0x9E37_79B1;
    const P2: u32 = 0x85EB_CA77;
    const P3: u32 = 0xC2B2_AE3D;
    const P4: u32 = 0x27D4_EB2F;
    const P5: u32 = 0x1656_67B1;

    let read = |at: usize| -> u32 {
        u32::from_le_bytes([
            data[at] ^ key,
            data[at + 1] ^ key,
            data[at + 2] ^ key,
            data[at + 3] ^ key,
        ])
    };

    let len = data.len();
    let mut at = 0usize;
    let mut h: u32;
    if len >= 16 {
        let mut v1 = seed.wrapping_add(P1).wrapping_add(P2);
        let mut v2 = seed.wrapping_add(P2);
        let mut v3 = seed;
        let mut v4 = seed.wrapping_sub(P1);
        while at <= len - 16 {
            v1 = v1
                .wrapping_add(read(at).wrapping_mul(P2))
                .rotate_left(13)
                .wrapping_mul(P1);
            at += 4;
            v2 = v2
                .wrapping_add(read(at).wrapping_mul(P2))
                .rotate_left(13)
                .wrapping_mul(P1);
            at += 4;
            v3 = v3
                .wrapping_add(read(at).wrapping_mul(P2))
                .rotate_left(13)
                .wrapping_mul(P1);
            at += 4;
            v4 = v4
                .wrapping_add(read(at).wrapping_mul(P2))
                .rotate_left(13)
                .wrapping_mul(P1);
            at += 4;
        }
        h = v1
            .rotate_left(1)
            .wrapping_add(v2.rotate_left(7))
            .wrapping_add(v3.rotate_left(12))
            .wrapping_add(v4.rotate_left(18));
    } else {
        h = seed.wrapping_add(P5);
    }
    h = h.wrapping_add(len as u32);
    while at + 4 <= len {
        h = h
            .wrapping_add(read(at).wrapping_mul(P3))
            .rotate_left(17)
            .wrapping_mul(P4);
        at += 4;
    }
    while at < len {
        h = h
            .wrapping_add(u32::from(data[at] ^ key).wrapping_mul(P5))
            .rotate_left(11)
            .wrapping_mul(P1);
        at += 1;
    }
    h ^= h >> 15;
    h = h.wrapping_mul(P2);
    h ^= h >> 13;
    h = h.wrapping_mul(P3);
    h ^= h >> 16;
    h
}

/// 索引里使用的哈希算法。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HashAlg {
    Crc32,
    Adler32,
    Murmur2,
    Xxh32,
}

impl HashAlg {
    pub fn as_str(self) -> &'static str {
        match self {
            HashAlg::Crc32 => "crc32",
            HashAlg::Adler32 => "adler32",
            HashAlg::Murmur2 => "murmur2",
            HashAlg::Xxh32 => "xxh32",
        }
    }

    fn hash(self, data: &[u8]) -> u32 {
        match self {
            HashAlg::Crc32 => crc32(data),
            HashAlg::Adler32 => adler32(data),
            HashAlg::Murmur2 => murmur2(data, 0),
            HashAlg::Xxh32 => xxh32(data, 0),
        }
    }

    fn hash_xor(self, data: &[u8], key: u8) -> u32 {
        match self {
            HashAlg::Crc32 => crc32_xor(data, key),
            HashAlg::Adler32 => adler32_xor(data, key),
            HashAlg::Murmur2 => murmur2_xor(data, key, 0),
            HashAlg::Xxh32 => xxh32_xor(data, key, 0),
        }
    }
}

/// `+0x0C` 的解释。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndexSizeMode {
    /// 值 = 索引结束处的绝对文件偏移(v>=300;0.495 与 v473 样本均如此)。
    AbsoluteDataStart,
    /// 值 = 索引块字节数(v<300;参考实现 yuri 的 `l-32 if v>=300` 分支)。
    RelativeSize,
}

/// 条目定宽。`Meta18` = u32 offset + u32 checksum;`Meta22` = u64 offset + u32 checksum。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryWidth {
    Meta18,
    Meta22,
}

/// 名字长度解码表。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LenMap {
    /// 恒等表 + 12 对交换(0.495 与 v473 样本)。
    V000,
    /// v<0x100 变体(GARbro `SwapTable04` / Script-Editor `_SWAP_TABLE_04`)。
    V004,
    /// 0x12C..0x196 变体(GARbro `SwapTable10` / Script-Editor `_SWAP_TABLE_10`)。
    V010,
    /// v500 起的变体(yuri / YPF_Tool 对照;本机暂无真样本)。
    V500,
}

impl LenMap {
    fn table(self) -> &'static [u8; 256] {
        match self {
            LenMap::V000 => &NAME_LEN_TABLE_00,
            LenMap::V004 => &NAME_LEN_TABLE_04,
            LenMap::V010 => &NAME_LEN_TABLE_10,
            LenMap::V500 => &NAME_LEN_TABLE_V500,
        }
    }
}

/// 一个可由结构校验裁决的 YPF 索引 profile。
///
/// **版本号不参与**;接受与否由 [`YpfSource::open`] 的索引闭合 +
/// 全量名字哈希 + 偏移/长度校验决定,不做 `version == N` 硬编码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct YpfProfile {
    pub index_size: IndexSizeMode,
    pub entry_width: EntryWidth,
    pub name_hash: HashAlg,
    pub data_hash: HashAlg,
    /// 名字字节 XOR key 的提示;实际 key 由 [`detect_name_key`] 全量校验。
    pub name_key: u8,
    pub len_map: LenMap,
}

impl YpfProfile {
    pub fn meta_len(self) -> usize {
        match self.entry_width {
            EntryWidth::Meta18 => LEGACY_ENTRY_META_LEN,
            EntryWidth::Meta22 => ENTRY_META_LEN,
        }
    }

    pub fn offset_u64(self) -> bool {
        self.entry_width == EntryWidth::Meta22
    }

    pub fn describe(self) -> String {
        format!(
            "index={:?} meta={} name={} data={} key={:#04x} table={:?}",
            self.index_size,
            self.meta_len(),
            self.name_hash.as_str(),
            self.data_hash.as_str(),
            self.name_key,
            self.len_map
        )
    }
}

/// 固定候选 profile:两种 `+0x0C` 解释 × 三种哈希族 × 四张长度表。
///
/// 顺序只影响探测开销(现代 0.495 在前),不影响正确性:每个候选都必须
/// 通过索引闭合 + 全量名字哈希 + 边界校验才会被接受。版本号完全不参与。
fn candidate_profiles() -> Vec<YpfProfile> {
    let index_modes = [
        IndexSizeMode::AbsoluteDataStart,
        IndexSizeMode::RelativeSize,
    ];
    let families = [
        (EntryWidth::Meta22, HashAlg::Murmur2, HashAlg::Murmur2),
        (EntryWidth::Meta18, HashAlg::Crc32, HashAlg::Adler32),
        (EntryWidth::Meta18, HashAlg::Adler32, HashAlg::Adler32),
    ];
    let len_maps = [LenMap::V000, LenMap::V004, LenMap::V010, LenMap::V500];

    let mut out = Vec::with_capacity(2 * families.len() * len_maps.len());
    for index_size in index_modes {
        for (entry_width, name_hash, data_hash) in families {
            for len_map in len_maps {
                out.push(YpfProfile {
                    index_size,
                    entry_width,
                    name_hash,
                    data_hash,
                    name_key: 0xFF,
                    len_map,
                });
            }
        }
    }
    out
}

/// `+0x0C` → 索引块字节数;两种解释都必须在文件内。
fn index_len_for(mode: IndexSizeMode, dir_or_index: u32, file_len: u64) -> Option<usize> {
    let len = match mode {
        IndexSizeMode::AbsoluteDataStart => (dir_or_index as usize).checked_sub(HEADER_LEN)?,
        IndexSizeMode::RelativeSize => dir_or_index as usize,
    };
    if len == 0 || len > MAX_INDEX_BYTES {
        return None;
    }
    if HEADER_LEN as u64 + len as u64 > file_len {
        return None;
    }
    Some(len)
}

/// 索引方言。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum YpfDialect {
    /// 0.495 布局:u64 dataOffset + Murmur2 名字/负载(逐条实证)。
    HashedLen { index_start: usize, name_key: u8 },
    /// v473 布局:u32 dataOffset + CRC32 名字 + Adler32 负载(432/432 实证)。
    Crc32U32 { index_start: usize, name_key: u8 },
    /// 其它由结构校验接受的 profile(如 v500 长度表 / Adler32 名字)。
    Other { index_start: usize, name_key: u8 },
    /// 旧参考实现的 NUL 结尾 XOR 名字方言。**本机无真样本**(DIV-002);
    /// 本版本**不会产出**该变体(结构不匹配一律 Err)。
    NulXor { index_start: usize, key: u8 },
}

impl YpfDialect {
    pub fn as_str(self) -> &'static str {
        match self {
            YpfDialect::HashedLen { .. } => "hashed-len",
            YpfDialect::Crc32U32 { .. } => "crc32-u32",
            YpfDialect::Other { .. } => "other",
            YpfDialect::NulXor { .. } => "nul-xor",
        }
    }

    /// 是否为真包实证过的布局:索引起点必须 `0x20`、名字 key 必须 `0xFF`。
    /// 其它组合一律 false(调用方须告警)。
    pub fn is_verified(self) -> bool {
        matches!(
            self,
            YpfDialect::HashedLen {
                index_start: INDEX_START,
                name_key: 0xFF,
            } | YpfDialect::Crc32U32 {
                index_start: INDEX_START,
                name_key: 0xFF,
            } | YpfDialect::Other {
                index_start: INDEX_START,
                name_key: 0xFF,
            }
        )
    }
}

/// 文件头。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct YpfHeader {
    pub version: u32,
    pub entry_count: u32,
    /// `+0x0C`;索引区结束位置,也是首个负载字节的绝对偏移。
    pub data_start: u32,
    /// `+0x10..0x20`;官方产出全 0,原样保留。
    pub reserved: [u8; 16],
    pub file_len: u64,
}

/// 一条索引条目(解码后)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct YpfEntryInfo {
    /// 已解码名字字节(官方布局为逐字节取反后的结果)。
    pub name: Vec<u8>,
    /// 归一后的查找键。
    pub normalized: LookupKey,
    /// 索引存储的名字哈希(算法见 `name_hash_alg`)。
    pub name_hash: u32,
    /// 按 profile 计算的 `name_hash == hash(name)`。
    pub name_hash_ok: bool,
    /// 名字哈希算法(profile 决定)。
    pub name_hash_alg: HashAlg,
    /// 资源类型码(实测 0=ybn / 2=png / 3=jpg / 6=ogg / 9=psb)。
    /// 原样保留,不赋予未知类型任何语义。
    pub file_type: u8,
    /// `true` = 负载是 zlib 流;`false` = 原样存储。
    pub compressed: bool,
    pub uncompressed_size: u32,
    /// 存储字节数(zlib 流或裸数据的精确长度)。
    pub compressed_size: u32,
    /// 负载绝对偏移(直接指向首字节);u32/u64 由 profile 决定,这里统一放宽到 u64。
    pub data_offset: u64,
    /// `Meta22` 时是 `data_offset` 的高 32 位(`Meta18` 恒 0)。
    pub unknown0: u32,
    /// 索引存储的负载校验值(算法见 `data_hash_alg`;v473 = Adler32)。
    pub payload_hash: u32,
    /// 负载校验算法(profile 决定)。
    pub data_hash_alg: HashAlg,
    /// 仅旧 NUL-XOR 方言保留的尾部未知字节(本版本不再产出)。
    pub raw_tail: Vec<u8>,
}

/// 解析质量统计,供真包验收与诊断。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct YpfParseStats {
    pub declared_entries: u32,
    pub parsed_entries: usize,
    /// 探测出的索引起点。
    pub index_start: usize,
    pub index_bytes: usize,
    pub consumed_bytes: usize,
    pub closed: bool,
    /// 名字哈希命中的条目数(算法由 `profile.name_hash` 决定)。
    pub name_hash_matches: usize,
    /// 判定出的名字 XOR key。
    pub name_key: u8,
    /// 选中的布局 profile。
    pub profile: YpfProfile,
    /// `profile.meta_len()` 的副本(兼容旧读取方)。
    pub meta_len: usize,
    /// 各 `file_type` 的条目数(升序)。
    pub type_counts: Vec<(u8, usize)>,
    /// `isCompressed` 为 1 的条目数。
    pub compressed_entries: usize,
}

/// `.ypf` 数据源。
pub struct YpfSource {
    path: PathBuf,
    codec: Codec,
    /// 封包在文件内的起点(独立 `.ypf` = 0;EXE 内嵌 = YPF 魔数偏移)。
    base_offset: u64,
    header: YpfHeader,
    dialect: YpfDialect,
    profile: YpfProfile,
    stats: YpfParseStats,
    entries: Vec<YpfEntryInfo>,
    /// 精确查找表。**只做 membership/取值,从不参与输出迭代**,故用
    /// `HashMap`(与 `DirSource.files` 同口径)。`LookupKey` 实现
    /// `Borrow<[u8]>`,按原始字节查也行。
    lookup: HashMap<LookupKey, usize>,
    tree: DirTree,
    file: Mutex<File>,
    warnings: Vec<Diag>,
}

impl YpfSource {
    /// 打开并解析一个封包(只读索引;数据在 `open` 时按偏移随机读)。
    pub fn open(path: impl AsRef<Path>, codec: Codec) -> Result<Self, Diag> {
        let path = path.as_ref().to_path_buf();
        let mut file = File::open(&path).map_err(|e| io_error(&path, e))?;
        let total_len = file.metadata().map_err(|e| io_error(&path, e))?.len();

        let mut magic = [0u8; 4];
        file.read_exact(&mut magic).map_err(|e| {
            Diag::fatal(code::vfs::YPF_BAD_HEADER, Boundary::Data)
                .at_data(path_bytes(&path), 0)
                .with_detail(format!("cannot read 4-byte magic: {e}"))
        })?;
        let base_offset = if magic == YPF_MAGIC {
            0
        } else if magic[..2] == *b"MZ" {
            // GARbro 相容路径:EXE 内嵌 YPF = `YSER` 头 + `+0x04 header_size`
            // 之后紧跟 `YPF\0`。只接受两层魔数都成立的候选。
            find_embedded_ypf(&mut file, total_len, &path)?.ok_or_else(|| {
                Diag::fatal(code::vfs::YPF_BAD_MAGIC, Boundary::Data)
                    .at_data(path_bytes(&path), 0)
                    .with_detail("MZ executable has no YSER/header_size/YPF\\0 embedded pack")
            })?
        } else {
            return Err(Diag::fatal(code::vfs::YPF_BAD_MAGIC, Boundary::Data)
                .at_data(path_bytes(&path), 0)
                .with_detail(format!(
                    "expected YPF\\0 or MZ/YSER-embedded pack, found {magic:02x?}"
                )));
        };
        let file_len = total_len - base_offset;

        let mut head = [0u8; HEADER_LEN];
        file.seek(SeekFrom::Start(base_offset))
            .map_err(|e| io_error(&path, e))?;
        file.read_exact(&mut head).map_err(|e| {
            Diag::fatal(code::vfs::YPF_BAD_HEADER, Boundary::Data)
                .at_data(path_bytes(&path), base_offset)
                .with_detail(format!("cannot read {HEADER_LEN}-byte header: {e}"))
        })?;
        if head[..4] != YPF_MAGIC {
            return Err(Diag::fatal(code::vfs::YPF_BAD_MAGIC, Boundary::Data)
                .at_data(path_bytes(&path), base_offset)
                .with_detail(format!("expected YPF\\0, found {:02x?}", &head[..4])));
        }

        let version = u32::from_le_bytes(head[4..8].try_into().unwrap());
        let entry_count = u32::from_le_bytes(head[8..12].try_into().unwrap());
        // `+0x0C` 有两种解释(绝对 dataStart / 索引块大小);先原样保留,
        // 由每个候选 profile 自行换算,再按结构校验裁决。
        let dir_or_index = u32::from_le_bytes(head[12..16].try_into().unwrap());
        let reserved: [u8; 16] = head[0x10..0x20].try_into().unwrap();

        // 版本号只记录到 `YpfHeader`;布局由固定候选 + 结构校验裁决。
        let _ = VERIFIED_MAX_VERSION;
        if entry_count == 0 {
            return Err(
                Diag::unimplemented(code::vfs::YPF_UNSUPPORTED, Boundary::Data)
                    .at_data(path_bytes(&path), 8)
                    .with_detail(
                        "entryCount=0: empty-pack semantics are unobserved; refusing to guess",
                    ),
            );
        }
        // V7:条目数上限 = 引擎的 99,999。
        if entry_count as usize > MAX_ENTRY_COUNT {
            return Err(Diag::fatal(code::vfs::YPF_BAD_HEADER, Boundary::Data)
                .at_data(path_bytes(&path), 8)
                .with_detail(format!(
                    "entryCount {entry_count} exceeds engine cap {MAX_ENTRY_COUNT}"
                )));
        }

        let profiles = candidate_profiles();
        let max_index_len = profiles
            .iter()
            .filter_map(|p| index_len_for(p.index_size, dir_or_index, file_len))
            .max()
            .unwrap_or(0);
        if max_index_len == 0 {
            return Err(Diag::fatal(code::vfs::YPF_BAD_HEADER, Boundary::Data)
                .at_data(path_bytes(&path), 12)
                .with_detail(format!(
                    "+0x0c={dir_or_index:#x} fits neither an absolute dataStart nor a \
                     relative index size within fileLen {file_len:#x}"
                )));
        }

        let mut block = vec![0u8; max_index_len];
        file.seek(SeekFrom::Start(base_offset + HEADER_LEN as u64))
            .map_err(|e| io_error(&path, e))?;
        file.read_exact(&mut block).map_err(|e| {
            Diag::fatal(code::vfs::YPF_BAD_HEADER, Boundary::Data)
                .at_data(path_bytes(&path), HEADER_LEN as u64)
                .with_detail(format!("cannot read index block: {e}"))
        })?;

        let (dialect, profile, data_start, mut entries, stats, warnings) = select_layout(
            &block,
            &profiles,
            entry_count,
            dir_or_index,
            file_len,
            codec,
            &path,
        )?;

        let mut lookup = HashMap::new();
        let mut tree = DirTree::new();
        for (idx, info) in entries.iter_mut().enumerate() {
            if info.normalized.is_root() && !info.name.is_empty() {
                info.normalized = LookupKey::new(&info.name, codec)?;
            }
            // 一次 `insert` 同时判重与登记,省掉 `contains_key` 的二次查找;
            // 审计表不参与输出迭代,哈希顺序不影响任何结果。
            if lookup.insert(info.normalized.clone(), idx).is_some() {
                // 引擎的重复名处理(名称互换/后到者禁用)证据不足;静默
                // first-wins 会让查找结果偏离引擎,故直接 Fatal。
                return Err(Diag::fatal(code::vfs::YPF_DUPLICATE_NAME, Boundary::Data)
                    .at_data(info.normalized.as_bytes().to_vec(), 0)
                    .with_detail(format!(
                        "duplicate normalized entry name (entry {idx});                          engine duplicate-name semantics are not verified"
                    )));
            }
            tree.insert_file(&info.normalized, info.uncompressed_size as u64)?;
        }

        Ok(Self {
            path,
            codec,
            base_offset,
            header: YpfHeader {
                version,
                entry_count,
                data_start,
                reserved,
                file_len,
            },
            dialect,
            profile,
            stats,
            entries,
            lookup,
            tree,
            file: Mutex::new(file),
            warnings,
        })
    }

    pub fn header(&self) -> &YpfHeader {
        &self.header
    }

    /// 封包在文件内的起点(独立 `.ypf` = 0;EXE 内嵌 = YPF 魔数偏移)。
    pub fn base_offset(&self) -> u64 {
        self.base_offset
    }

    pub fn dialect(&self) -> YpfDialect {
        self.dialect
    }

    /// 实际选中的布局 profile(结构校验后的真值;版本号只是候选来源)。
    pub fn profile(&self) -> YpfProfile {
        self.profile
    }

    pub fn stats(&self) -> &YpfParseStats {
        &self.stats
    }

    pub fn entries(&self) -> &[YpfEntryInfo] {
        &self.entries
    }

    /// 解析期警告(重复名、名字哈希不符等)。不影响读取,但可审计。
    pub fn warnings(&self) -> &[Diag] {
        &self.warnings
    }

    pub fn codec(&self) -> Codec {
        self.codec
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 读取存储字节(尚未解压)。
    fn read_stored(&self, entry: &YpfEntryInfo) -> Result<Vec<u8>, Diag> {
        let mut file = self.file.lock().map_err(|_| {
            Diag::fatal(code::vfs::IO, Boundary::Data)
                .at_data(entry.name.clone(), entry.data_offset)
                .with_detail("ypf file mutex poisoned")
        })?;
        file.seek(SeekFrom::Start(self.base_offset + entry.data_offset))
            .map_err(|e| io_error(&self.path, e))?;
        let mut raw = vec![0u8; entry.compressed_size as usize];
        file.read_exact(&mut raw).map_err(|e| {
            Diag::fatal(code::vfs::YPF_BAD_ENTRY, Boundary::Data)
                .at_data(entry.name.clone(), entry.data_offset)
                .with_detail(format!(
                    "payload read failed ({} bytes at {:#x}): {e}",
                    entry.compressed_size, entry.data_offset
                ))
        })?;
        Ok(raw)
    }

    /// 校验存储负载的索引校验值,算法由 profile 决定:
    /// 0.495 = MurmurHash2,v<479 = Adler32(REA/真包实证)。
    ///
    /// 不匹配时显式 `Unimplemented`,不猜「多段续读」。
    fn verify_payload_hash(&self, entry: &YpfEntryInfo, stored: &[u8]) -> Result<(), Diag> {
        let actual = entry.data_hash_alg.hash(stored);
        if actual == entry.payload_hash {
            return Ok(());
        }
        // v500 商业包:nameHash 仍是 murmur2,payloadHash 是 xxh32。
        // 索引字段本身没有算法标记,按结构选中的 profile 只能给出
        // nameHash 族;这里用存储值做一次性裁决(命中才接受,不猜)。
        let xxh = HashAlg::Xxh32.hash(stored);
        if entry.data_hash_alg != HashAlg::Xxh32 && xxh == entry.payload_hash {
            return Ok(());
        }
        // WZ 包的索引哈希可能按反变换后的前 8 字节计算;只在标记命中时接受。
        if let Some(unscrambled) = wz_unscramble(stored) {
            if entry.data_hash_alg.hash(&unscrambled) == entry.payload_hash {
                return Ok(());
            }
            if entry.data_hash_alg != HashAlg::Xxh32
                && HashAlg::Xxh32.hash(&unscrambled) == entry.payload_hash
            {
                return Ok(());
            }
        }
        Err(Diag::unimplemented(code::vfs::YPF_CHECKSUM, Boundary::Data)
            .at_data(entry.name.clone(), entry.data_offset)
            .with_detail(format!(
                "stored payload {} {actual:#010x} / xxh32 {xxh:#010x} != index {:#010x}; \
                 engine multi-segment fallback is unimplemented",
                entry.data_hash_alg.as_str(),
                entry.payload_hash
            )))
    }
}

impl std::fmt::Debug for YpfSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("YpfSource")
            .field("path", &self.path)
            .field("codec", &self.codec)
            .field("base_offset", &self.base_offset)
            .field("header", &self.header)
            .field("dialect", &self.dialect)
            .field("stats", &self.stats)
            .field("entries", &self.entries.len())
            .field("warnings", &self.warnings.len())
            .finish()
    }
}

impl DataSource for YpfSource {
    /// 只读前 `max` 字节(图像尺寸探测用)。
    ///
    /// **未压缩条目**:直接 seek 到 `data_offset` 只读 `max` 字节 —— 这是
    /// 唯一能真省 IO 的形态。实测商业包 12212 条中 2542 张图未压缩、
    /// 平均 359 KB,而 PNG/BMP/JPEG/GIF 的尺寸都在前 64 字节内;整读再
    /// 截断是纯浪费(`CGINFO[SX/SY]` 单次实测 275µs,单帧 110 次=30ms)。
    ///
    /// **压缩条目 / 前缀覆盖整条负载**:回退到完整 [`Self::open`] ——
    /// 前者需要 zlib 从头部完整解码,后者本来就等于整读。两种情况都保留
    /// `verify_payload_hash` 的完整校验语义,不会「假装校验过」。
    fn open_prefix(&self, path: &LookupKey, max: usize) -> Result<Vec<u8>, Diag> {
        let Some(&idx) = self.lookup.get(path) else {
            return Err(missing(path, self.source_name()));
        };
        let entry = &self.entries[idx];
        if entry.compressed || max >= entry.compressed_size as usize {
            let mut bytes = self.open(path)?;
            bytes.truncate(max);
            return Ok(bytes);
        }
        if entry.compressed_size != entry.uncompressed_size {
            return Err(Diag::fatal(code::vfs::YPF_BAD_ENTRY, Boundary::Data)
                .at_data(path.as_bytes().to_vec(), entry.data_offset)
                .with_detail(format!(
                    "stored entry has compressedSize {} != uncompressedSize {}",
                    entry.compressed_size, entry.uncompressed_size
                )));
        }
        let mut file = self.file.lock().map_err(|_| {
            Diag::fatal(code::vfs::IO, Boundary::Data)
                .at_data(entry.name.clone(), entry.data_offset)
                .with_detail("ypf file mutex poisoned")
        })?;
        file.seek(SeekFrom::Start(self.base_offset + entry.data_offset))
            .map_err(|e| io_error(&self.path, e))?;
        let mut raw = vec![0u8; max];
        file.read_exact(&mut raw).map_err(|e| {
            Diag::fatal(code::vfs::YPF_BAD_ENTRY, Boundary::Data)
                .at_data(entry.name.clone(), entry.data_offset)
                .with_detail(format!(
                    "payload prefix read failed ({max} of {} bytes at {:#x}): {e}",
                    entry.compressed_size, entry.data_offset
                ))
        })?;
        Ok(raw)
    }

    fn open(&self, path: &LookupKey) -> Result<Vec<u8>, Diag> {
        let Some(&idx) = self.lookup.get(path) else {
            return Err(missing(path, self.source_name()));
        };
        let entry = &self.entries[idx];
        let raw = self.read_stored(entry)?;
        self.verify_payload_hash(entry, &raw)?;

        if entry.compressed {
            decompress_payload(
                &raw,
                entry.uncompressed_size,
                path.as_bytes(),
                entry.data_offset,
            )
        } else {
            if entry.compressed_size != entry.uncompressed_size {
                return Err(Diag::fatal(code::vfs::YPF_BAD_ENTRY, Boundary::Data)
                    .at_data(path.as_bytes().to_vec(), entry.data_offset)
                    .with_detail(format!(
                        "stored entry has compressedSize {} != uncompressedSize {}",
                        entry.compressed_size, entry.uncompressed_size
                    )));
            }
            Ok(raw)
        }
    }

    fn exists(&self, path: &LookupKey) -> bool {
        self.lookup.contains_key(path)
    }

    fn list(&self, path: &LookupKey) -> Result<Vec<Entry>, Diag> {
        self.tree.list(path, self.source_name())
    }

    fn source_name(&self) -> &str {
        "ypf"
    }
}

#[derive(Clone, Debug)]
struct RawEntry {
    /// 索引里的原始名字字节(尚未 XOR 解码)。
    name_enc: Vec<u8>,
    /// 用判定出的 key 解码后的名字。
    name: Vec<u8>,
    name_hash: u32,
    name_hash_ok: bool,
    file_type: u8,
    compressed: bool,
    uncompressed_size: u32,
    compressed_size: u32,
    /// u64:低 32 位 + 高 32 位(旧 `unknown0`)。
    data_offset: u64,
    unknown0: u32,
    payload_hash: u32,
}

#[derive(Debug)]
struct Candidate {
    profile: YpfProfile,
    /// 该候选解释下的索引块字节数。
    index_len: usize,
    entries: Vec<RawEntry>,
    consumed: usize,
    closed: bool,
    name_hash_matches: usize,
    name_key: u8,
    type_counts: Vec<(u8, usize)>,
    compressed_entries: usize,
    /// 结构自洽但出现 `isCompressed > 1`: 语义未知。
    bad_compression: Option<(usize, u8)>,
    /// 明确的拒绝原因与实际失败偏移(相对索引块起点,供自动定位)。
    reject: Option<(usize, String)>,
}

impl Candidate {
    fn usable(&self) -> bool {
        self.closed && self.reject.is_none() && self.bad_compression.is_none()
    }

    /// 完整通过:索引闭合 + 每条名字哈希命中。
    fn full(&self) -> bool {
        self.usable() && self.name_hash_matches == self.entries.len()
    }

    fn reject_detail(&self) -> String {
        match &self.reject {
            Some((at, reason)) => format!("{reason} (at {at:#x})"),
            None => "index closed but candidate is not usable".to_string(),
        }
    }
}

fn summarize_failures(failures: &[String]) -> String {
    if failures.is_empty() {
        return "no candidate index length fit".to_string();
    }
    let mut out = failures
        .iter()
        .take(4)
        .cloned()
        .collect::<Vec<_>>()
        .join(" | ");
    if failures.len() > 4 {
        out.push_str(&format!(" | … {} more", failures.len() - 4));
    }
    out
}

/// 选中的布局、实际 dataStart、条目、统计与警告。
type LayoutOutcome = (
    YpfDialect,
    YpfProfile,
    u32,
    Vec<YpfEntryInfo>,
    YpfParseStats,
    Vec<Diag>,
);

/// 在有限候选 profile 中按结构裁决:索引闭合 + 全量名字哈希 + 偏移/长度边界。
fn select_layout(
    block: &[u8],
    profiles: &[YpfProfile],
    entry_count: u32,
    dir_or_index: u32,
    file_len: u64,
    codec: Codec,
    path: &Path,
) -> Result<LayoutOutcome, Diag> {
    let mut best: Option<Candidate> = None;
    let mut failures: Vec<String> = Vec::new();

    for &profile in profiles {
        let Some(index_len) = index_len_for(profile.index_size, dir_or_index, file_len) else {
            failures.push(format!("{}: index size does not fit", profile.describe()));
            continue;
        };
        if index_len > block.len() {
            failures.push(format!(
                "{}: index block {index_len} > read {}",
                profile.describe(),
                block.len()
            ));
            continue;
        }
        let min_step = profile.meta_len() + 5;
        if index_len / min_step < entry_count as usize {
            failures.push(format!(
                "{}: {entry_count} entries cannot fit in {index_len} bytes",
                profile.describe()
            ));
            continue;
        }
        let data_start = (HEADER_LEN + index_len) as u32;
        let candidate = parse_index(
            &block[..index_len],
            entry_count,
            data_start,
            file_len,
            profile,
        );
        if candidate.full() {
            best = Some(candidate);
            break;
        }
        failures.push(format!(
            "{}: {}",
            profile.describe(),
            candidate.reject_detail()
        ));
        let better = match &best {
            None => true,
            Some(current) => {
                candidate.name_hash_matches > current.name_hash_matches
                    || (candidate.name_hash_matches == current.name_hash_matches
                        && candidate.usable()
                        && !current.usable())
            }
        };
        if better {
            best = Some(candidate);
        }
    }

    let Some(candidate) = best else {
        return Err(
            Diag::unimplemented(code::vfs::YPF_UNKNOWN_DIALECT, Boundary::Data)
                .at_data(path_bytes(path), INDEX_START as u64)
                .with_detail(format!(
                    "no YPF profile validated; tried: {}",
                    summarize_failures(&failures)
                )),
        );
    };

    if !candidate.full() {
        let at = candidate
            .reject
            .as_ref()
            .map(|(at, _)| *at)
            .unwrap_or(candidate.consumed);
        // 只有名字哈希已经命中(布局判定无疑)时,坏压缩字节才是数据错误;
        // 零命中说明整个候选布局不对,统一按未知方言报告。
        if candidate.name_hash_matches > 0 {
            if let Some((i, value)) = candidate.bad_compression {
                return Err(Diag::unimplemented(
                    code::vfs::YPF_UNSUPPORTED,
                    Boundary::Data,
                )
                .at_data(path_bytes(path), (INDEX_START + at) as u64)
                .with_detail(format!(
                    "entry {i} isCompressed byte {value:#04x} is not 0 (stored) or 1 (zlib); semantics unknown, refusing to guess"
                )));
            }
            let reason = candidate.reject_detail();
            return Err(Diag::fatal(code::vfs::YPF_BAD_ENTRY, Boundary::Data)
                .at_data(path_bytes(path), (INDEX_START + at) as u64)
                .with_detail(format!("{}: {reason}", candidate.profile.describe())));
        }
        let reason = candidate.reject_detail();
        return Err(Diag::unimplemented(
            code::vfs::YPF_UNKNOWN_DIALECT,
            Boundary::Data,
        )
        .at_data(path_bytes(path), (INDEX_START + at) as u64)
        .with_detail(format!(
            "no YPF profile validated (best candidate matched {}/{} name hashes): {reason}; tried: {}",
            candidate.name_hash_matches,
            candidate.entries.len(),
            summarize_failures(&failures)
        )));
    }

    // 统计与告警先取标量,随后按值消费条目(名字字节直接 move 进
    // `YpfEntryInfo`,不再逐条 clone;`name_enc` 已在解码阶段被取空)。
    let profile = candidate.profile;
    let entry_len = candidate.entries.len();
    let name_key = candidate.name_key;
    let mismatches = candidate.entries.iter().filter(|e| !e.name_hash_ok).count();

    let mut entries = Vec::with_capacity(entry_len);
    for raw in candidate.entries {
        let normalized = LookupKey::new(&raw.name, codec)?;
        entries.push(YpfEntryInfo {
            name: raw.name,
            normalized,
            name_hash: raw.name_hash,
            name_hash_ok: raw.name_hash_ok,
            name_hash_alg: profile.name_hash,
            file_type: raw.file_type,
            compressed: raw.compressed,
            uncompressed_size: raw.uncompressed_size,
            compressed_size: raw.compressed_size,
            data_offset: raw.data_offset,
            unknown0: raw.unknown0,
            payload_hash: raw.payload_hash,
            data_hash_alg: profile.data_hash,
            raw_tail: Vec::new(),
        });
    }

    let mut warnings = Vec::new();
    if mismatches > 0 {
        warnings.push(
            Diag::recoverable(code::vfs::YPF_HASH_MISMATCH, Boundary::Data)
                .at_data(path_bytes(path), INDEX_START as u64)
                .with_detail(format!(
                    "{mismatches}/{entry_len} entries have name {} mismatch under key {name_key:#04x}; official hash lookup would miss them",
                    profile.name_hash.as_str()
                )),
        );
    }
    if name_key != 0xFF {
        warnings.push(
            Diag::unimplemented(code::vfs::YPF_UNSUPPORTED, Boundary::Data)
                .at_data(path_bytes(path), INDEX_START as u64)
                .with_detail(format!(
                    "name key {name_key:#04x} differs from the engine's 0xFF; layout is not verified"
                )),
        );
    }

    let meta_len = profile.meta_len();
    let stats = YpfParseStats {
        declared_entries: entry_count,
        parsed_entries: entry_len,
        index_start: INDEX_START,
        index_bytes: candidate.index_len,
        consumed_bytes: candidate.consumed,
        closed: candidate.closed,
        name_hash_matches: candidate.name_hash_matches,
        name_key,
        profile,
        meta_len,
        type_counts: candidate.type_counts,
        compressed_entries: candidate.compressed_entries,
    };
    let data_start = (HEADER_LEN + candidate.index_len) as u32;
    Ok((
        dialect_for(profile, name_key),
        profile,
        data_start,
        entries,
        stats,
        warnings,
    ))
}

fn dialect_for(profile: YpfProfile, name_key: u8) -> YpfDialect {
    match (profile.entry_width, profile.name_hash) {
        (EntryWidth::Meta22, HashAlg::Murmur2) => YpfDialect::HashedLen {
            index_start: INDEX_START,
            name_key,
        },
        (EntryWidth::Meta18, HashAlg::Crc32) => YpfDialect::Crc32U32 {
            index_start: INDEX_START,
            name_key,
        },
        _ => YpfDialect::Other {
            index_start: INDEX_START,
            name_key,
        },
    }
}

/// 用 nameHash 在候选 XOR key 中择优(确定性)。
///
/// 顺序: profile 提示 key、`0xFF`、已知特例(0xC9/0xBF)、参考实现的
/// 扩展名推导 `enc[nameLen-4] ^ '.'`;若 `exhaustive` 再全量 0..=255。
/// 评分 = 名字哈希命中数;满分立即返回;平局优先提示 key/0xFF。
fn detect_name_key(entries: &[RawEntry], alg: HashAlg, hint: u8, exhaustive: bool) -> u8 {
    let mut candidates: Vec<u8> = Vec::with_capacity(260);
    for key in [hint, 0xFF, 0xC9, 0xBF] {
        if !candidates.contains(&key) {
            candidates.push(key);
        }
    }
    if let Some(first) = entries.first() {
        let enc = &first.name_enc;
        if enc.len() >= 4 {
            candidates.push(enc[enc.len() - 4] ^ b'.');
        }
    }
    if exhaustive {
        candidates.extend((0u16..=255u16).map(|k| k as u8));
    }

    let mut best_key = hint;
    let mut best_score = None;
    let mut seen = [false; 256];
    for key in candidates {
        if seen[key as usize] {
            continue;
        }
        seen[key as usize] = true;
        let score = entries
            .iter()
            .filter(|e| alg.hash_xor(&e.name_enc, key) == e.name_hash)
            .count();
        let better = match best_score {
            None => true,
            Some(current) => {
                score > current
                    || (score == current && best_key != hint && (key == hint || key < best_key))
            }
        };
        if better {
            best_score = Some(score);
            best_key = key;
        }
        if score == entries.len() {
            // 满分不可能被后来者超过;提前退出。
            return key;
        }
    }
    best_key
}

fn parse_index(
    block: &[u8],
    entry_count: u32,
    data_start: u32,
    file_len: u64,
    profile: YpfProfile,
) -> Candidate {
    let table = profile.len_map.table();
    let meta = profile.meta_len();
    let step_extra = meta + 5;
    let skip = INDEX_START - HEADER_LEN;
    let mut pos = skip;
    // V7:容量按物理下限与实际条目数取小,不用 entryCount 直接放大分配。
    let capacity = (entry_count as usize).min(block.len() / step_extra + 1);
    let mut entries = Vec::with_capacity(capacity);
    let mut compressed_entries = 0;
    let mut type_counts: BTreeMap<u8, usize> = BTreeMap::new();
    let mut bad_compression = None;
    let mut reject: Option<(usize, String)> = None;

    for i in 0..entry_count as usize {
        let Some(head) = pos.checked_add(5) else {
            reject = Some((pos, format!("entry {i} offset overflow")));
            break;
        };
        if head > block.len() {
            reject = Some((pos, format!("truncated at entry {i} (offset {pos:#x})")));
            break;
        }
        let name_hash = u32::from_le_bytes(block[pos..pos + 4].try_into().unwrap());
        let len_enc = block[pos + 4];
        let name_len = table[((0xFFu16 - len_enc as u16) & 0xFF) as usize] as usize;
        let name_start = head;
        let Some(name_end) = name_start.checked_add(name_len) else {
            reject = Some((name_start, format!("entry {i} name length overflow")));
            break;
        };
        let Some(meta_end) = name_end.checked_add(meta) else {
            reject = Some((name_end, format!("entry {i} meta length overflow")));
            break;
        };
        if meta_end > block.len() {
            reject = Some((
                name_end,
                format!(
                    "entry {i} truncated: nameLen {name_len}, need {meta_end:#x}, block {}",
                    block.len()
                ),
            ));
            break;
        }
        let name_enc = block[name_start..name_end].to_vec();
        let file_type = block[name_end];
        let compression = block[name_end + 1];
        let uncompressed_size =
            u32::from_le_bytes(block[name_end + 2..name_end + 6].try_into().unwrap());
        let compressed_size =
            u32::from_le_bytes(block[name_end + 6..name_end + 10].try_into().unwrap());
        let (data_offset, unknown0, payload_hash) = if profile.offset_u64() {
            // 0.495:dataOffset 8 字节(REA `FUN_0043fb9c`);高 4 字节旧称 unknown0。
            let low = u32::from_le_bytes(block[name_end + 10..name_end + 14].try_into().unwrap());
            let high = u32::from_le_bytes(block[name_end + 14..name_end + 18].try_into().unwrap());
            let checksum =
                u32::from_le_bytes(block[name_end + 18..name_end + 22].try_into().unwrap());
            ((u64::from(high) << 32) | u64::from(low), high, checksum)
        } else {
            // v<479:dataOffset 4 字节 + 4 字节负载校验(Adler32/CRC32)。
            let off = u32::from_le_bytes(block[name_end + 10..name_end + 14].try_into().unwrap());
            let checksum =
                u32::from_le_bytes(block[name_end + 14..name_end + 18].try_into().unwrap());
            (u64::from(off), 0, checksum)
        };
        pos = meta_end;

        if name_enc.is_empty() {
            reject = Some((name_start, format!("entry {i} has an empty name")));
            break;
        }
        *type_counts.entry(file_type).or_insert(0) += 1;

        if compression == 1 {
            compressed_entries += 1;
        }
        entries.push(RawEntry {
            name_enc,
            name: Vec::new(),
            name_hash,
            name_hash_ok: false,
            file_type,
            compressed: compression == 1,
            uncompressed_size,
            compressed_size,
            data_offset,
            unknown0,
            payload_hash,
        });

        if compression > 1 {
            if bad_compression.is_none() {
                bad_compression = Some((i, compression));
            }
            continue;
        }
        if compression == 0 && compressed_size != uncompressed_size {
            reject = Some((
                name_end,
                format!(
                    "entry {i}: stored sizes differ ({} != {})",
                    compressed_size, uncompressed_size
                ),
            ));
            break;
        }
        if compressed_size > 0 {
            let end = data_offset.checked_add(u64::from(compressed_size));
            if data_offset < u64::from(data_start) || end.is_none_or(|e| e > file_len) {
                reject = Some((
                    name_end,
                    format!(
                        "entry {i}: payload {data_offset:#x}+{} outside data area",
                        compressed_size
                    ),
                ));
                break;
            }
        }
        if compressed_size == 0 && data_offset != 0 && data_offset < u64::from(data_start) {
            reject = Some((
                name_end,
                format!("entry {i}: empty payload offset {data_offset:#x}"),
            ));
            break;
        }
    }

    if reject.is_none() && pos != block.len() {
        reject = Some((
            pos,
            format!(
                "index walk ended at {pos:#x}, expected {:#x} (delta {})",
                block.len(),
                block.len() as i64 - pos as i64
            ),
        ));
    }

    // 名字解码与哈希校验:即便字段被拒,已解析条目的哈希命中仍是
    // "布局判对了"的硬证据,所以无条件执行。只有索引完整闭合时才
    // 全量暴力 key(避免 12 个候选 × 256 key 的无谓开销)。
    let exhaustive = reject.is_none() && entries.len() == entry_count as usize;
    let mut name_hash_matches = 0usize;
    let mut name_key = profile.name_key;
    if !entries.is_empty() {
        name_key = detect_name_key(&entries, profile.name_hash, profile.name_key, exhaustive);
        for entry in &mut entries {
            // 名字哈希按「解码后字节」校验;等价于对 `name_enc` 做 XOR 版
            // 哈希(零分配),之后再把已分配的 `name_enc` 原地 XOR 成
            // 名字字节并 move 过去,不再分配第二个 Vec。
            entry.name_hash_ok =
                profile.name_hash.hash_xor(&entry.name_enc, name_key) == entry.name_hash;
            if entry.name_hash_ok {
                name_hash_matches += 1;
            }
            let mut name = std::mem::take(&mut entry.name_enc);
            for b in &mut name {
                *b ^= name_key;
            }
            entry.name = name;
        }
    }

    Candidate {
        profile,
        index_len: block.len(),
        entries,
        consumed: pos.saturating_sub(skip),
        closed: pos == block.len() && reject.is_none(),
        name_hash_matches,
        name_key,
        type_counts: type_counts.into_iter().collect(),
        compressed_entries,
        bad_compression,
        reject,
    }
}

/// 解压 `compressed=1` 的负载。
///
/// 两代并存:0.495 与 v473 样本是 zlib;v555 工具说明 `compressed=1` 也可
/// 是 Snappy(YPF_Tool 注释)。先按 zlib 解,失败再按 Snappy 解;两者都
/// 必须满足索引 `usize`。若仍失败且命中 WZ 标记(RxYuris
/// `YPF::DecodeFile_WZ`:首 dword 低字节 `0x5A`),再试 WZ 反变换后的
/// zlib/Snappy。WZ 无真包样本,标 `Likely`;成功仍要求长度闭合。
fn decompress_payload(
    raw: &[u8],
    expected: u32,
    origin: &[u8],
    offset: u64,
) -> Result<Vec<u8>, Diag> {
    let zlib_error = match zlib_payload(raw, expected) {
        Ok(out) => return Ok(out),
        Err(error) => error,
    };
    let snappy_error = match snappy_payload(raw, expected) {
        Ok(out) => return Ok(out),
        Err(error) => error,
    };
    let wz_note = match wz_unscramble(raw) {
        Some(unscrambled) => {
            if let Ok(out) = zlib_payload(&unscrambled, expected) {
                return Ok(out);
            }
            if let Ok(out) = snappy_payload(&unscrambled, expected) {
                return Ok(out);
            }
            "; wz 反变换后 zlib/snappy 仍不闭合"
        }
        None => "",
    };
    Err(Diag::fatal(code::vfs::YPF_DECOMPRESS, Boundary::Data)
        .at_data(origin.to_vec(), offset)
        .with_detail(format!(
            "zlib: {zlib_error}; snappy: {snappy_error}{wz_note}"
        )))
}

fn zlib_payload(raw: &[u8], expected: u32) -> Result<Vec<u8>, String> {
    let cap = (expected as usize).min(64 * 1024 * 1024);
    let mut out = Vec::with_capacity(cap);
    let mut decoder = ZlibDecoder::new(raw).take(u64::from(expected) + 1);
    match decoder.read_to_end(&mut out) {
        Ok(_) if out.len() as u64 == u64::from(expected) => Ok(out),
        Ok(_) => Err(format!("解压 {} 字节,索引声明 {}", out.len(), expected)),
        Err(e) => Err(format!("zlib inflate failed: {e}")),
    }
}

fn snappy_payload(raw: &[u8], expected: u32) -> Result<Vec<u8>, String> {
    match yu_format::snappy::decompress(raw, expected) {
        Ok(out) if out.len() as u64 == u64::from(expected) => Ok(out),
        Ok(out) => Err(format!("解压 {} 字节,索引声明 {}", out.len(), expected)),
        Err(error) => Err(error),
    }
}

/// WZ 载荷前 8 字节反变换(RxYuris `DecodeFile_WZ`,标 `Likely`)。
///
/// 标记:首 dword 低字节 == `0x5A`;变换:`d0 ^= 0x0040FB22`,
/// `d1 ^= comp_size`。正常 zlib 首字节 `0x78` 经该变换正好变成 `0x5A`,
/// 与参考实现吻合。无真包样本,只在标准解压失败后尝试。
fn wz_unscramble(raw: &[u8]) -> Option<Vec<u8>> {
    if raw.len() < 8 {
        return None;
    }
    let d0 = u32::from_le_bytes(raw[0..4].try_into().ok()?);
    if d0 & 0xFF != 0x5A {
        return None;
    }
    let mut out = raw.to_vec();
    out[0..4].copy_from_slice(&(d0 ^ 0x0040_FB22).to_le_bytes());
    let d1 = u32::from_le_bytes(raw[4..8].try_into().ok()?) ^ raw.len() as u32;
    out[4..8].copy_from_slice(&d1.to_le_bytes());
    Some(out)
}

/// 在 `MZ` 文件里找 `YSER` 头后的内嵌 `YPF\0`。
///
/// 协议(GARbro `ArcYPF.FindYser`):`YSER` 后 `+0x04` 是 header_size,
/// `YSER + header_size` 处紧跟 `YPF\0`。分块扫描,避免整读大 EXE;
/// 只有两层魔数都成立才返回偏移。
fn find_embedded_ypf(file: &mut File, file_len: u64, path: &Path) -> Result<Option<u64>, Diag> {
    const CHUNK: usize = 1 << 20;
    let mut buf = vec![0u8; CHUNK + 8];
    let mut pos = 0u64;
    while pos < file_len {
        let want = ((file_len - pos) as usize).min(CHUNK + 8);
        file.seek(SeekFrom::Start(pos))
            .map_err(|e| io_error(path, e))?;
        file.read_exact(&mut buf[..want])
            .map_err(|e| io_error(path, e))?;

        let mut start = 0usize;
        while start + 4 <= want {
            let Some(rel) = buf[start..want].windows(4).position(|w| w == b"YSER") else {
                break;
            };
            let at = start + rel;
            if at + 8 <= want {
                let hs = u32::from_le_bytes(buf[at + 4..at + 8].try_into().unwrap());
                if hs > 0 && hs < 0x0100_0000 {
                    let candidate = pos + at as u64 + u64::from(hs);
                    if candidate.checked_add(4).is_some_and(|end| end <= file_len) {
                        file.seek(SeekFrom::Start(candidate))
                            .map_err(|e| io_error(path, e))?;
                        let mut magic = [0u8; 4];
                        file.read_exact(&mut magic).map_err(|e| io_error(path, e))?;
                        if magic == YPF_MAGIC {
                            return Ok(Some(candidate));
                        }
                    }
                }
            }
            start = at + 1;
        }
        if want < CHUNK + 8 {
            break;
        }
        pos += CHUNK as u64;
    }
    Ok(None)
}

fn io_error(path: &Path, error: std::io::Error) -> Diag {
    Diag::fatal(code::vfs::IO, Boundary::Data)
        .at_data(path_bytes(path), 0)
        .with_detail(error.to_string())
}

fn path_bytes(path: &Path) -> Vec<u8> {
    path.to_string_lossy().as_bytes().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODERN: YpfProfile = YpfProfile {
        index_size: IndexSizeMode::AbsoluteDataStart,
        entry_width: EntryWidth::Meta22,
        name_hash: HashAlg::Murmur2,
        data_hash: HashAlg::Murmur2,
        name_key: 0xFF,
        len_map: LenMap::V000,
    };
    const LEGACY_CRC: YpfProfile = YpfProfile {
        index_size: IndexSizeMode::AbsoluteDataStart,
        entry_width: EntryWidth::Meta18,
        name_hash: HashAlg::Crc32,
        data_hash: HashAlg::Adler32,
        name_key: 0xFF,
        len_map: LenMap::V000,
    };

    #[test]
    fn name_len_table_is_an_involution() {
        for i in 0..256u16 {
            let b = i as u8;
            let mapped = NAME_LEN_TABLE_00[b as usize];
            assert_eq!(NAME_LEN_TABLE_00[mapped as usize], b);
        }
        assert_eq!(NAME_LEN_TABLE_00[0x12], 0x12);
        assert_eq!(NAME_LEN_TABLE_00[3], 0x48);
        assert_eq!(NAME_LEN_TABLE_00[0x48], 3);
    }

    #[test]
    fn known_len_encodes_roundtrip() {
        for len in 0u16..=255 {
            let len = len as u8;
            let len_enc = name_len_code(len, 495);
            assert_eq!(name_len_from_code(len_enc, 495), len);
        }
    }

    #[test]
    fn candidate_profiles_cover_known_families_without_version() {
        let profiles = candidate_profiles();
        assert!(profiles.contains(&MODERN));
        assert!(profiles.contains(&LEGACY_CRC));
        assert!(profiles.iter().any(|p| p.name_hash == HashAlg::Adler32));
        for map in [LenMap::V000, LenMap::V004, LenMap::V010, LenMap::V500] {
            assert!(profiles.iter().any(|p| p.len_map == map), "{map:?}");
        }
        assert!(profiles
            .iter()
            .any(|p| p.index_size == IndexSizeMode::RelativeSize));
    }

    #[test]
    fn crc32_matches_v473_pack_values() {
        // v473 `bg.ypf` 实测:名字哈希 = CRC32(name),432/432。
        assert_eq!(crc32(br"bg\dot_sq_4.png"), 0x0077_293d);
        assert_eq!(crc32(br"bg\wipeopen\openwipe23.png"), 0x0226_476e);
        assert_eq!(crc32(br"bg\ef1\bg_kaisou_wh_waku.png"), 0x00ae_2b24);
    }

    #[test]
    fn adler32_matches_reference_values() {
        assert_eq!(adler32(b""), 1);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
        // v473 首条存储负载(zlib 流)的 Adler32;值来自真包实测。
        // 这里只验证算法本身,负载由 integration oracle 读取。
    }

    #[test]
    fn murmur2_matches_official_pack_values() {
        // 官方包实测: 名字 hash 与负载 hash 都来自同一算法。
        assert_eq!(murmur2(br"ysbin\yst00034.ybn", 0), 0x0033_ae52);
        // MurmurHash2 参考实现的标准向量。
        assert_eq!(murmur2(b"", 0), 0);
        assert_eq!(murmur2(b"hello", 0), 0xe561_29cb);
    }

    /// #16:`murmur2_xor` 必须在全部 256 个 key 上与"先逐字节 XOR 再哈希"
    /// 逐位相同(零分配只是手段,结果等价才是约束)。
    #[test]
    fn murmur2_xor_matches_bytewise_for_all_keys() {
        let xor = |data: &[u8], key: u8| -> Vec<u8> { data.iter().map(|b| b ^ key).collect() };
        // 覆盖余数 0/1/2/3 与官方名字形态。
        let samples: [&[u8]; 6] = [b"", b"a", b"ab", b"abc", b"abcd", br"ysbin\yst00034.ybn"];
        let long: Vec<u8> = (0..257u32).map(|i| (i * 37 + 11) as u8).collect();
        for key in 0u16..=255 {
            let key = key as u8;
            for data in samples {
                assert_eq!(
                    murmur2_xor(data, key, 0),
                    murmur2(&xor(data, key), 0),
                    "key {key:#04x} len {}",
                    data.len()
                );
            }
            assert_eq!(
                murmur2_xor(&long, key, 0),
                murmur2(&xor(&long, key), 0),
                "long key {key:#04x}"
            );
            // seed 也必须原样参与。
            assert_eq!(
                murmur2_xor(&long, key, 0x1234_5678),
                murmur2(&xor(&long, key), 0x1234_5678),
                "seeded long key {key:#04x}"
            );
        }
    }

    /// V6:`is_verified` 只认引擎布局(0x20 起点 + 0xFF key)。
    #[test]
    fn verified_dialect_requires_engine_layout() {
        assert!(YpfDialect::HashedLen {
            index_start: 0x20,
            name_key: 0xFF
        }
        .is_verified());
        assert!(!YpfDialect::HashedLen {
            index_start: 0x24,
            name_key: 0xFF
        }
        .is_verified());
        assert!(!YpfDialect::HashedLen {
            index_start: 0x20,
            name_key: 0xC9
        }
        .is_verified());
    }

    /// V3:dataOffset 是 8 字节;高 32 位参与构造(u64),不是被忽略的字段。
    #[test]
    fn data_offset_is_read_as_u64() {
        let name = b"a.bin";
        let mut block = Vec::new();
        block.extend_from_slice(&murmur2(name, 0).to_le_bytes());
        block.push(name_len_code(name.len() as u8, 495));
        block.extend(name.iter().map(|b| !*b));
        block.push(0); // file_type
        block.push(0); // stored
        block.extend_from_slice(&3u32.to_le_bytes()); // usize
        block.extend_from_slice(&3u32.to_le_bytes()); // csize
        block.extend_from_slice(&0u32.to_le_bytes()); // offset low
        block.extend_from_slice(&1u32.to_le_bytes()); // offset high
        block.extend_from_slice(&murmur2(b"abc", 0).to_le_bytes());

        let candidate = parse_index(&block, 1, 0x20, u32::MAX as u64, MODERN);
        assert_eq!(candidate.entries[0].data_offset, 1u64 << 32);
        assert_eq!(candidate.entries[0].unknown0, 1);
        assert!(candidate.reject.is_some(), "1<<32 偏移在小文件里必须越界");
    }

    /// v473:18 字节 meta + u32 offset + CRC32/Adler32 必须被解析器接受。
    #[test]
    fn legacy_meta18_crc32_layout_parses() {
        let name = br"bg\dot_sq_4.png";
        let payload = b"abc";
        let mut block = Vec::new();
        block.extend_from_slice(&crc32(name).to_le_bytes());
        block.push(name_len_code(name.len() as u8, 473));
        block.extend(name.iter().map(|b| !*b));
        block.push(2); // png
        block.push(0); // stored
        block.extend_from_slice(&(payload.len() as u32).to_le_bytes()); // usize
        block.extend_from_slice(&(payload.len() as u32).to_le_bytes()); // csize
        block.extend_from_slice(&0x20u32.to_le_bytes()); // offset u32
        block.extend_from_slice(&adler32(payload).to_le_bytes()); // data checksum

        let candidate = parse_index(&block, 1, 0x20, 0x100, LEGACY_CRC);
        assert_eq!(candidate.entries.len(), 1);
        assert_eq!(candidate.entries[0].name, name);
        assert_eq!(candidate.entries[0].data_offset, 0x20);
        assert_eq!(candidate.entries[0].unknown0, 0);
        assert_eq!(candidate.entries[0].payload_hash, adler32(payload));
        assert_eq!(candidate.name_key, 0xFF);
        assert!(candidate.name_hash_matches == 1);
        assert!(
            candidate.full(),
            "18-byte profile must close and hash-match"
        );
    }

    /// v7 端到端:v473 包必须由 `open` 自动选中 legacy profile 并读出负载。
    #[test]
    fn open_legacy_profile_end_to_end() {
        use std::fs;

        let base = std::env::var_os("YURIS_TEST_TMP")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp")
            });
        let dir = base.join(format!("yu-vfs-legacy-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("legacy.ypf");

        let name = br"bg\dot_sq_4.png";
        let payload = b"\x89PNG\r\n\x1a\nlegacy";
        let mut index = Vec::new();
        index.extend_from_slice(&crc32(name).to_le_bytes());
        index.push(name_len_code(name.len() as u8, 473));
        index.extend(name.iter().map(|b| !*b));
        index.push(2); // png
        index.push(0); // stored
        index.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        index.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        index.extend_from_slice(&0u32.to_le_bytes()); // offset 占位
        index.extend_from_slice(&adler32(payload).to_le_bytes());
        let data_start = (HEADER_LEN + index.len()) as u32;
        let meta = 4 + 1 + name.len();
        index[meta + 10..meta + 14].copy_from_slice(&data_start.to_le_bytes());

        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"YPF\0");
        bytes.extend_from_slice(&473u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&data_start.to_le_bytes());
        bytes.extend_from_slice(&[0u8; 16]);
        bytes.extend_from_slice(&index);
        bytes.extend_from_slice(payload);
        fs::write(&path, &bytes).unwrap();

        let source = YpfSource::open(&path, Codec::Cp932).unwrap();
        assert_eq!(source.header().version, 473);
        assert_eq!(source.profile().entry_width, EntryWidth::Meta18);
        assert_eq!(source.profile().name_hash, HashAlg::Crc32);
        assert_eq!(source.profile().data_hash, HashAlg::Adler32);
        assert_eq!(
            source.profile().index_size,
            IndexSizeMode::AbsoluteDataStart
        );
        assert_eq!(source.dialect().as_str(), "crc32-u32");
        assert_eq!(source.stats().name_hash_matches, 1);
        let key = LookupKey::new(name, Codec::Cp932).unwrap();
        assert_eq!(source.open(&key).unwrap(), payload);
        assert_eq!(source.header().data_start, data_start);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn v004_and_v010_length_tables_are_selectable() {
        // V004 对长度 3/6 是恒等,V000 会映射成 0x48/0x35。
        let name3 = b"abc";
        let code_v004 = 0xFFu8.wrapping_sub(NAME_LEN_TABLE_04[3]);
        let mut block = Vec::new();
        block.extend_from_slice(&murmur2(name3, 0).to_le_bytes());
        block.push(code_v004);
        block.extend(name3.iter().map(|b| !*b));
        block.push(0);
        block.push(0);
        block.extend_from_slice(&3u32.to_le_bytes());
        block.extend_from_slice(&3u32.to_le_bytes());
        block.extend_from_slice(&0x20u32.to_le_bytes()); // offset low
        block.extend_from_slice(&0u32.to_le_bytes()); // offset high
        block.extend_from_slice(&murmur2(b"abc", 0).to_le_bytes());

        let mut v004 = MODERN;
        v004.len_map = LenMap::V004;
        let candidate = parse_index(&block, 1, 0x20, 0x100, v004);
        assert_eq!(candidate.entries[0].name, name3);
        assert_eq!(candidate.name_hash_matches, 1);
        let candidate = parse_index(&block, 1, 0x20, 0x100, MODERN);
        assert_ne!(
            candidate.entries.first().map(|e| e.name.as_slice()),
            Some(name3.as_slice()),
            "V000 必须解错"
        );

        // V010 对长度 0x0C 是恒等,V000 会映射成 0x10。
        let name12 = b"abcdefghijkl";
        let code_v010 = 0xFFu8.wrapping_sub(NAME_LEN_TABLE_10[12]);
        let mut block = Vec::new();
        block.extend_from_slice(&murmur2(name12, 0).to_le_bytes());
        block.push(code_v010);
        block.extend(name12.iter().map(|b| !*b));
        block.push(0);
        block.push(0);
        block.extend_from_slice(&12u32.to_le_bytes());
        block.extend_from_slice(&12u32.to_le_bytes());
        block.extend_from_slice(&0x20u32.to_le_bytes()); // offset low
        block.extend_from_slice(&0u32.to_le_bytes()); // offset high
        block.extend_from_slice(&murmur2(name12, 0).to_le_bytes());

        let mut v010 = MODERN;
        v010.len_map = LenMap::V010;
        let candidate = parse_index(&block, 1, 0x20, 0x100, v010);
        assert_eq!(candidate.entries[0].name, name12);
        assert_eq!(candidate.name_hash_matches, 1);
        let candidate = parse_index(&block, 1, 0x20, 0x100, MODERN);
        assert_ne!(
            candidate.entries.first().map(|e| e.name.as_slice()),
            Some(name12.as_slice()),
            "V000 必须解错"
        );
    }

    #[test]
    fn embedded_ypf_after_yser_is_opened() {
        use std::fs;

        let name = b"a.txt";
        let payload = b"embedded";
        let mut index = Vec::new();
        index.extend_from_slice(&crc32(name).to_le_bytes());
        index.push(name_len_code(name.len() as u8, 408));
        index.extend(name.iter().map(|b| !*b));
        index.push(0); // file_type
        index.push(0); // stored
        index.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        index.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        index.extend_from_slice(&0u32.to_le_bytes()); // offset 占位
        index.extend_from_slice(&adler32(payload).to_le_bytes());
        let data_start = (HEADER_LEN + index.len()) as u32;
        let meta = 4 + 1 + name.len();
        index[meta + 10..meta + 14].copy_from_slice(&data_start.to_le_bytes());

        let mut pack = Vec::new();
        pack.extend_from_slice(b"YPF\0");
        pack.extend_from_slice(&408u32.to_le_bytes());
        pack.extend_from_slice(&1u32.to_le_bytes());
        pack.extend_from_slice(&data_start.to_le_bytes());
        pack.extend_from_slice(&[0u8; 16]);
        pack.extend_from_slice(&index);
        pack.extend_from_slice(payload);

        let ypf_at = 0x110usize;
        let mut exe = vec![0u8; ypf_at];
        exe[..2].copy_from_slice(b"MZ");
        exe[0x100..0x104].copy_from_slice(b"YSER");
        exe[0x104..0x108].copy_from_slice(&0x10u32.to_le_bytes());
        exe.extend_from_slice(&pack);

        let base = std::env::var_os("YURIS_TEST_TMP")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp")
            });
        let dir = base.join(format!("yu-vfs-embedded-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("inst.exe");
        fs::write(&path, &exe).unwrap();

        let source = YpfSource::open(&path, Codec::Cp932).unwrap();
        assert_eq!(source.base_offset(), ypf_at as u64);
        assert_eq!(source.header().version, 408);
        assert_eq!(source.profile().entry_width, EntryWidth::Meta18);
        assert_eq!(source.profile().name_hash, HashAlg::Crc32);
        let key = LookupKey::new(name, Codec::Cp932).unwrap();
        assert_eq!(source.open(&key).unwrap(), payload);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn decompress_payload_accepts_wz_marked_zlib() {
        use std::io::Write as _;
        let raw = b"wz payload";
        let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(1));
        enc.write_all(raw).unwrap();
        let packed = enc.finish().unwrap();
        assert!(packed.len() >= 8);
        let mut wz = packed.clone();
        let d0 = u32::from_le_bytes(packed[0..4].try_into().unwrap()) ^ 0x0040_FB22;
        let d1 = u32::from_le_bytes(packed[4..8].try_into().unwrap()) ^ wz.len() as u32;
        wz[0..4].copy_from_slice(&d0.to_le_bytes());
        wz[4..8].copy_from_slice(&d1.to_le_bytes());
        assert_eq!(wz[0], 0x5A, "WZ 标记");

        let out = decompress_payload(&wz, raw.len() as u32, b"t", 0).expect("wz+zlib");
        assert_eq!(out, raw);
    }

    #[test]
    fn decompress_payload_accepts_snappy_fallback() {
        let raw = b"snappy fallback payload";
        let packed = yu_format::snappy::compress(raw);
        let out = decompress_payload(&packed, raw.len() as u32, b"t", 0).expect("snappy");
        assert_eq!(out, raw);

        let err = decompress_payload(&packed, raw.len() as u32 + 1, b"t", 0).unwrap_err();
        assert_eq!(err.code, code::vfs::YPF_DECOMPRESS);
    }

    /// v500 商业包负载校验 = xxh32(存储字节, seed 0)。
    #[test]
    fn xxh32_matches_reference_vectors() {
        assert_eq!(xxh32(b"", 0), 0x02CC_5D05);
        assert_eq!(xxh32(b"a", 0), 0x550D_7456);
        assert_eq!(xxh32(b"abc", 0), 0x32D1_53FF);
    }

    /// V7:巨大 entryCount + 极小索引区不得产生放大分配。
    #[test]
    fn hostile_entry_count_does_not_allocate() {
        let block = vec![0u8; 16];
        let candidate = parse_index(&block, 99_999, 0x30, 0x30, MODERN);
        assert!(candidate.reject.is_some());
        assert!(candidate.entries.len() <= 1, "不得按 entryCount 预分配");
    }
}
