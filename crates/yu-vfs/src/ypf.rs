//! `.ypf` 封包数据源。
//!
//! # 布局(官方 YSPac 0.495 实测,2026-10-19)
//!
//! 证据: 官方 YSPac 产出的 1,407 条封包逐字节解析 —— 索引行走恰好闭合于
//! 头部 `+0x0C`;1,407/1,407 条「名字 MurmurHash2」与「负载 MurmurHash2」
//! 全部与索引存储值相等;压缩条目 zlib 流实际消耗字节数与索引 `csize`
//! 逐条相等、解压长度与 `usize` 逐条相等。详见 `docs/formats/ypf.md` 与
//! `docs/divergences/DIV-001-ypf-index-layout.md`。
//!
//! ```text
//! +0x00  "YPF\0"
//! +0x04  u32 version            (接受: ==495 或 >=234)
//! +0x08  u32 entryCount
//! +0x0C  u32 dataStart          (索引区结束位置 = 首个负载字节的绝对偏移)
//! +0x10  u8[16] reserved        (官方产出全 0,原样保留)
//! +0x20  index[entryCount]      (索引起点需探测: 0x20 / 0x24)
//! 数据区: dataOffset 直接指向负载首字节,没有独立记录头
//! ```
//!
//! 索引条目(步长 = `nameLen + 27`):
//!
//! ```text
//! +0x00  u32 nameHash      MurmurHash2(名字字节, seed=0)
//! +0x04  u8  lenEnc        nameLen = T[(0xFF - lenEnc) & 0xFF]
//! +0x05  u8  name[nameLen] 逐字节取反 (~b);以 `\` 分隔、小写
//! +...   u8  fileType      0=ybn 2=png 3=jpg 6=ogg 9=psb(实测集合)
//! +...   u8  isCompressed  0 = 原样存储,1 = zlib 流
//! +...   u32 usize         解压后大小
//! +...   u32 csize         存储字节数(zlib 流本身,或裸数据长度)
//! +...   u32 dataOffset    负载绝对偏移
//! +...   u32 unknown0      实测恒 0,原样保留
//! +...   u32 payloadHash   MurmurHash2(存储字节, seed=0),1,407/1,407 实证
//! ```
//!
//! 注意 `fileType` 与 `isCompressed` 是**两个独立字节**。旧文档把二者并成
//! 一个 `flag`(0=zlib/2=原样),但官方包里存在 `type=3`(jpg)且 zlib 压缩、
//! `type=0`(ybn)且原样存储的条目,单字节模型无法同时成立。
//!
//! # 未决
//!
//! - **名字 XOR key**: 官方引擎固定 `~b`(0xFF);开源参考实现的"逐游戏
//!   key"说法与引擎字节不符。本实现仍保留 nameHash 择优探测,但
//!   `is_verified()` 只认 `index_start == 0x20 && name_key == 0xFF`,
//!   其它组合产出警告。
//! - 旧 NUL-XOR 方言本机无真样本(DIV-002),**不再有 Ok 回退**:
//!   结构不匹配的包一律 `Unimplemented`,不产出垃圾名字。

use crate::{missing, Codec, DataSource, DirTree, Entry, LookupKey};
use flate2::read::ZlibDecoder;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use yu_log::{code, Boundary, Diag};

/// 封包魔数。
pub const YPF_MAGIC: [u8; 4] = *b"YPF\0";
/// 下限:**没有样本、且已知早期格式不同**,低于此值拒绝(DIV-002)。
///
/// 这与上限不同 —— 上限曾是 `495`,而 REA 证明装载器**没有版本分支**,
/// 见 [`VERIFIED_MAX_VERSION`]。下限保留是因为 0.2xx 时代的布局无任何
/// 本地样本,拒绝而不是硬套现布局。
pub const MIN_ACCEPTED_VERSION: u32 = 234;
/// **已实测验证过的最高版本**(仅信息,不是闸门)。
///
/// 证据:官方索引装载器 `FUN_0045e3b8`(0.495 `yu-ris.exe`,
/// sha256 `4f5eefa3…`)**逐字节无版本分支** ——
/// 索引起点恒 `0x20`、步长恒 `nameLen + 0x1B`、长度码表恒
/// `T[0xFF - lenEnc]`、名字逐字节取反,`version` 字段只被读入而从不
/// 参与分支(见 `docs/tasks/task-e-fixes.md` §4 与
/// `docs/divergences/DIV-002-ypf-nul-xor-dialect.md`)。
///
/// 因此版本号是**透传字段,不是门禁**:高于本值的包按同一套布局解析,
/// 由结构校验(条目数、索引容量、方言探测、名字哈希)判定真伪。
/// 曾经写死 `version > 495 ⇒ 拒绝`,会把低于本引擎的更高版本包
/// (如 YPF v500 的商业作品)直接拒之门外 —— 那是把"没样本"误当成
/// "格式不同",属实现缺陷而非证据缺口。
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
    const M: u32 = 0x5bd1_e995;
    const R: u32 = 24;

    let mut h: u32 = seed ^ (data.len() as u32);
    let mut chunks = data.chunks_exact(4);
    for chunk in &mut chunks {
        let mut k = u32::from_le_bytes(chunk.try_into().unwrap());
        k = k.wrapping_mul(M);
        k ^= k >> R;
        k = k.wrapping_mul(M);
        h = h.wrapping_mul(M);
        h ^= k;
    }
    let rem = chunks.remainder();
    match rem.len() {
        3 => {
            h ^= (rem[2] as u32) << 16;
            h ^= (rem[1] as u32) << 8;
            h ^= rem[0] as u32;
            h = h.wrapping_mul(M);
        }
        2 => {
            h ^= (rem[1] as u32) << 8;
            h ^= rem[0] as u32;
            h = h.wrapping_mul(M);
        }
        1 => {
            h ^= rem[0] as u32;
            h = h.wrapping_mul(M);
        }
        _ => {}
    }
    h ^= h >> 13;
    h = h.wrapping_mul(M);
    h ^= h >> 15;
    h
}

/// 索引方言。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum YpfDialect {
    /// 官方布局(逐条实证)。`index_start` 恒 `0x20`(REA),`name_key`
    /// 为名字字节的 XOR 密钥(官方样本 = 0xFF)。
    HashedLen { index_start: usize, name_key: u8 },
    /// 旧参考实现的 NUL 结尾 XOR 名字方言。**本机无真样本**(DIV-002);
    /// 本版本**不会产出**该变体(结构不匹配一律 Err)。
    NulXor { index_start: usize, key: u8 },
}

impl YpfDialect {
    pub fn as_str(self) -> &'static str {
        match self {
            YpfDialect::HashedLen { .. } => "hashed-len",
            YpfDialect::NulXor { .. } => "nul-xor",
        }
    }

    /// 是否为官方实证过的布局(REA `FUN_0045e3b8`):索引起点必须
    /// `0x20`、名字 key 必须 `0xFF`。其它组合一律 false(调用方须告警)。
    pub fn is_verified(self) -> bool {
        matches!(
            self,
            YpfDialect::HashedLen {
                index_start: INDEX_START,
                name_key: 0xFF
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
    /// 索引存储的名字 MurmurHash2。
    pub name_hash: u32,
    /// `murmur2(name, 0) == name_hash`。
    pub name_hash_ok: bool,
    /// 资源类型码(实测 0=ybn / 2=png / 3=jpg / 6=ogg / 9=psb)。
    /// 原样保留,不赋予未知类型任何语义。
    pub file_type: u8,
    /// `true` = 负载是 zlib 流;`false` = 原样存储。
    pub compressed: bool,
    pub uncompressed_size: u32,
    /// 存储字节数(zlib 流或裸数据的精确长度)。
    pub compressed_size: u32,
    /// 负载绝对偏移(直接指向首字节),**u64**(REA `FUN_0043fb9c` 读 8 字节)。
    pub data_offset: u64,
    /// `data_offset` 的高 32 位(旧字段 `unknown0`;官方包恒 0)。
    pub unknown0: u32,
    /// 存储负载的 MurmurHash2(seed=0);0 是合法哈希值,不是"未记录"。
    pub payload_hash: u32,
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
    /// 名字 MurmurHash2 命中的条目数。
    pub name_hash_matches: usize,
    /// 判定出的名字 XOR key。
    pub name_key: u8,
    /// 按版本判定出的 meta 长度。
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
    header: YpfHeader,
    dialect: YpfDialect,
    stats: YpfParseStats,
    entries: Vec<YpfEntryInfo>,
    lookup: BTreeMap<LookupKey, usize>,
    tree: DirTree,
    file: Mutex<File>,
    warnings: Vec<Diag>,
}

impl YpfSource {
    /// 打开并解析一个封包(只读索引;数据在 `open` 时按偏移随机读)。
    pub fn open(path: impl AsRef<Path>, codec: Codec) -> Result<Self, Diag> {
        let path = path.as_ref().to_path_buf();
        let mut file = File::open(&path).map_err(|e| io_error(&path, e))?;
        let file_len = file.metadata().map_err(|e| io_error(&path, e))?.len();

        let mut magic = [0u8; 4];
        file.read_exact(&mut magic).map_err(|e| {
            Diag::fatal(code::vfs::YPF_BAD_HEADER, Boundary::Data)
                .at_data(path_bytes(&path), 0)
                .with_detail(format!("cannot read 4-byte magic: {e}"))
        })?;
        if magic != YPF_MAGIC {
            return Err(Diag::fatal(code::vfs::YPF_BAD_MAGIC, Boundary::Data)
                .at_data(path_bytes(&path), 0)
                .with_detail(format!("expected YPF\\0, found {magic:02x?}")));
        }

        let mut head = [0u8; HEADER_LEN];
        head[0..4].copy_from_slice(&magic);
        file.read_exact(&mut head[4..]).map_err(|e| {
            Diag::fatal(code::vfs::YPF_BAD_HEADER, Boundary::Data)
                .at_data(path_bytes(&path), 0)
                .with_detail(format!("cannot read {HEADER_LEN}-byte header: {e}"))
        })?;

        let version = u32::from_le_bytes(head[4..8].try_into().unwrap());
        let entry_count = u32::from_le_bytes(head[8..12].try_into().unwrap());
        let data_start = u32::from_le_bytes(head[12..16].try_into().unwrap());
        let reserved: [u8; 16] = head[0x10..0x20].try_into().unwrap();

        if version < MIN_ACCEPTED_VERSION {
            return Err(Diag::fatal(code::vfs::YPF_BAD_VERSION, Boundary::Data)
                .at_data(path_bytes(&path), 4)
                .with_detail(format!(
                    "version {version} below the verified lower bound {MIN_ACCEPTED_VERSION} \
                     (early-format layouts have no local sample)"
                )));
        }
        // **不设上限**:装载器 `FUN_0045e3b8` 无版本分支(见
        // [`VERIFIED_MAX_VERSION`]),版本是透传字段。高于实测版本的包按
        // 同一布局解析,真伪由下面的结构校验(条目数 / 索引容量 /
        // 方言探测 / 名字哈希)判定。
        let _ = VERIFIED_MAX_VERSION;
        if (data_start as u64) < HEADER_LEN as u64 || data_start as u64 > file_len {
            return Err(Diag::fatal(code::vfs::YPF_BAD_HEADER, Boundary::Data)
                .at_data(path_bytes(&path), 12)
                .with_detail(format!(
                    "dataStart {data_start:#x} outside [{HEADER_LEN:#x}, fileLen {file_len:#x}]"
                )));
        }
        if entry_count == 0 {
            return Err(Diag::unimplemented(code::vfs::YPF_UNSUPPORTED, Boundary::Data)
                .at_data(path_bytes(&path), 8)
                .with_detail(
                    "entryCount=0: empty-pack semantics are unobserved; refusing to guess",
                ));
        }
        // V7:条目数上限 = 引擎的 99,999 与索引区物理下限 `index_bytes/27`。
        if entry_count as usize > MAX_ENTRY_COUNT {
            return Err(Diag::fatal(code::vfs::YPF_BAD_HEADER, Boundary::Data)
                .at_data(path_bytes(&path), 8)
                .with_detail(format!(
                    "entryCount {entry_count} exceeds engine cap {MAX_ENTRY_COUNT}"
                )));
        }
        let index_bytes = (data_start as usize) - HEADER_LEN;
        if index_bytes / ENTRY_STEP_EXTRA < entry_count as usize {
            return Err(Diag::fatal(code::vfs::YPF_BAD_HEADER, Boundary::Data)
                .at_data(path_bytes(&path), 12)
                .with_detail(format!(
                    "entryCount {entry_count} cannot fit in {index_bytes} index bytes (minimum {ENTRY_STEP_EXTRA} bytes/entry)"
                )));
        }
        if index_bytes > MAX_INDEX_BYTES {
            return Err(Diag::fatal(code::vfs::YPF_BAD_HEADER, Boundary::Data)
                .at_data(path_bytes(&path), 12)
                .with_detail(format!(
                    "index size {index_bytes} exceeds safety cap {MAX_INDEX_BYTES}"
                )));
        }

        let mut block = vec![0u8; index_bytes];
        file.seek(SeekFrom::Start(HEADER_LEN as u64))
            .map_err(|e| io_error(&path, e))?;
        file.read_exact(&mut block).map_err(|e| {
            Diag::fatal(code::vfs::YPF_BAD_HEADER, Boundary::Data)
                .at_data(path_bytes(&path), HEADER_LEN as u64)
                .with_detail(format!("cannot read index block: {e}"))
        })?;

        let (dialect, mut entries, stats, warnings) =
            detect_layout(&block, version, entry_count, data_start, file_len, codec, &path)?;

        let mut lookup = BTreeMap::new();
        let mut tree = DirTree::new();
        for (idx, info) in entries.iter_mut().enumerate() {
            if info.normalized.is_root() && !info.name.is_empty() {
                info.normalized = LookupKey::new(&info.name, codec)?;
            }
            let key = info.normalized.clone();
            if lookup.contains_key(&key) {
                // 引擎的重复名处理(名称互换/后到者禁用)证据不足;静默
                // first-wins 会让查找结果偏离引擎,故直接 Fatal。
                return Err(Diag::fatal(code::vfs::YPF_DUPLICATE_NAME, Boundary::Data)
                    .at_data(key.as_bytes().to_vec(), 0)
                    .with_detail(format!(
                        "duplicate normalized entry name (entry {idx});                          engine duplicate-name semantics are not verified"
                    )));
            }
            lookup.insert(key.clone(), idx);
            tree.insert_file(&key, info.uncompressed_size as u64)?;
        }

        Ok(Self {
            path,
            codec,
            header: YpfHeader {
                version,
                entry_count,
                data_start,
                reserved,
                file_len,
            },
            dialect,
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

    pub fn dialect(&self) -> YpfDialect {
        self.dialect
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
        file.seek(SeekFrom::Start(entry.data_offset))
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

    /// 校验存储负载的 MurmurHash2,**无条件**执行(REA `FUN_0043f71c`:
    /// 先算哈希再与索引值比较,`0` 没有哨兵含义)。
    ///
    /// 官方包的 `payloadHash` 1,407/1,407 与存储字节相等;不符时引擎走
    /// 「多段续读」,该语义本仓库未实现 —— 显式 `Unimplemented`,不猜。
    fn verify_payload_hash(&self, entry: &YpfEntryInfo, stored: &[u8]) -> Result<(), Diag> {
        let actual = murmur2(stored, 0);
        if actual == entry.payload_hash {
            return Ok(());
        }
        Err(Diag::unimplemented(code::vfs::YPF_CHECKSUM, Boundary::Data)
            .at_data(entry.name.clone(), entry.data_offset)
            .with_detail(format!(
                "stored payload MurmurHash2 {actual:#010x} != index {:#010x}; \
                 engine multi-segment fallback is unimplemented",
                entry.payload_hash
            )))
    }
}

impl std::fmt::Debug for YpfSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("YpfSource")
            .field("path", &self.path)
            .field("codec", &self.codec)
            .field("header", &self.header)
            .field("dialect", &self.dialect)
            .field("stats", &self.stats)
            .field("entries", &self.entries.len())
            .field("warnings", &self.warnings.len())
            .finish()
    }
}

impl DataSource for YpfSource {
    fn open(&self, path: &LookupKey) -> Result<Vec<u8>, Diag> {
        let Some(&idx) = self.lookup.get(path) else {
            return Err(missing(path, self.source_name()));
        };
        let entry = &self.entries[idx];
        let raw = self.read_stored(entry)?;
        self.verify_payload_hash(entry, &raw)?;

        if entry.compressed {
            let expected = entry.uncompressed_size as usize;
            let mut out = Vec::with_capacity(expected.min(64 * 1024));
            let mut decoder = ZlibDecoder::new(&raw[..]).take(expected as u64 + 1);
            decoder.read_to_end(&mut out).map_err(|e| {
                Diag::fatal(code::vfs::YPF_DECOMPRESS, Boundary::Data)
                    .at_data(path.as_bytes().to_vec(), entry.data_offset)
                    .with_detail(format!("zlib inflate failed: {e}"))
            })?;
            if out.len() != expected {
                return Err(Diag::fatal(code::vfs::YPF_DECOMPRESS, Boundary::Data)
                    .at_data(path.as_bytes().to_vec(), entry.data_offset)
                    .with_detail(format!(
                        "decompressed {} bytes, index declares {}",
                        out.len(),
                        expected
                    )));
            }
            Ok(out)
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
}

fn detect_layout(
    block: &[u8],
    _version: u32,
    entry_count: u32,
    data_start: u32,
    file_len: u64,
    codec: Codec,
    path: &Path,
) -> Result<(YpfDialect, Vec<YpfEntryInfo>, YpfParseStats, Vec<Diag>), Diag> {
    let mut warnings = Vec::new();
    // 引擎起点恒 `0x20`(REA `FUN_0045e3b8`)。不再探测 0x24 前缀方言:
    // 没有样本能证明它是 YPF 布局而不是别的数据。
    let candidate = parse_hashed_len(block, entry_count, data_start, file_len);

    if !candidate.usable() {
        // 结构能闭合、但压缩字节不是 0/1: 语义未知,不猜。
        if candidate.reject.is_none() {
            if let Some((at, value)) = candidate.bad_compression {
                return Err(Diag::unimplemented(
                    code::vfs::YPF_UNSUPPORTED,
                    Boundary::Data,
                )
                .at_data(path_bytes(path), (INDEX_START + at) as u64)
                .with_detail(format!(
                    "entry isCompressed byte {value:#04x} is not 0 (stored) or 1 (zlib); semantics unknown, refusing to guess"
                )));
            }
        }
        let (at, reason) = candidate.reject.unwrap_or_else(|| {
            (
                block.len(),
                "index walk does not close exactly at dataStart".to_string(),
            )
        });
        // 已有条目通过名字哈希校验 ⇒ 布局判定无疑,只是某条字段矛盾。
        // 这是数据错误(V9:失败偏移写进 Loc::Data.offset),不是方言问题。
        if candidate.name_hash_matches > 0 {
            return Err(Diag::fatal(code::vfs::YPF_BAD_ENTRY, Boundary::Data)
                .at_data(path_bytes(path), (INDEX_START + at) as u64)
                .with_detail(reason));
        }
        // 零哈希命中 = 无法确认是老布局;按未验证方言处理(Unimplemented),
        // 旧 NUL-XOR 回退已删除,不再尝试产出名字。
        return Err(Diag::unimplemented(
            code::vfs::YPF_UNKNOWN_DIALECT,
            Boundary::Data,
        )
        .at_data(path_bytes(path), (INDEX_START + at) as u64)
        .with_detail(format!(
            "index does not match the verified 0x20/12-pair/22-byte layout: {reason}"
        )));
    }

    // V1:名字哈希是布局判定的硬证据。零命中 = key/表都不对,绝不能
    // 把解码出的字节当名字返回(旧实现会退回 NUL-XOR 读出垃圾名)。
    if candidate.name_hash_matches == 0 {
        return Err(Diag::unimplemented(
            code::vfs::YPF_UNKNOWN_DIALECT,
            Boundary::Data,
        )
        .at_data(path_bytes(path), INDEX_START as u64)
        .with_detail(format!(
            "0/{} entry name hashes match under the selected key {:#04x}; layout/name key is unverified, refusing to expose decoded names",
            candidate.entries.len(),
            candidate.name_key
        )));
    }

    let mut entries = Vec::with_capacity(candidate.entries.len());
    for raw in &candidate.entries {
        let normalized = LookupKey::new(&raw.name, codec)?;
        entries.push(YpfEntryInfo {
            name: raw.name.clone(),
            normalized,
            name_hash: raw.name_hash,
            name_hash_ok: raw.name_hash_ok,
            file_type: raw.file_type,
            compressed: raw.compressed,
            uncompressed_size: raw.uncompressed_size,
            compressed_size: raw.compressed_size,
            data_offset: raw.data_offset,
            unknown0: raw.unknown0,
            payload_hash: raw.payload_hash,
            raw_tail: Vec::new(),
        });
    }

    let mismatches = candidate
        .entries
        .iter()
        .filter(|e| !e.name_hash_ok)
        .count();
    if mismatches > 0 {
        warnings.push(
            Diag::recoverable(code::vfs::YPF_HASH_MISMATCH, Boundary::Data)
                .at_data(path_bytes(path), INDEX_START as u64)
                .with_detail(format!(
                    "{mismatches}/{} entries have name MurmurHash2 mismatch under key {:#04x}; official hash lookup would miss them",
                    candidate.entries.len(),
                    candidate.name_key
                )),
        );
    }
    if candidate.name_key != 0xFF {
        warnings.push(
            Diag::unimplemented(code::vfs::YPF_UNSUPPORTED, Boundary::Data)
                .at_data(path_bytes(path), INDEX_START as u64)
                .with_detail(format!(
                    "name key {:#04x} differs from the engine's 0xFF; layout is not verified",
                    candidate.name_key
                )),
        );
    }

    let stats = YpfParseStats {
        declared_entries: entry_count,
        parsed_entries: candidate.entries.len(),
        index_start: INDEX_START,
        index_bytes: block.len(),
        consumed_bytes: candidate.consumed,
        closed: candidate.closed,
        name_hash_matches: candidate.name_hash_matches,
        name_key: candidate.name_key,
        meta_len: ENTRY_META_LEN,
        type_counts: candidate.type_counts.clone(),
        compressed_entries: candidate.compressed_entries,
    };
    Ok((
        YpfDialect::HashedLen {
            index_start: INDEX_START,
            name_key: candidate.name_key,
        },
        entries,
        stats,
        warnings,
    ))
}

/// 用 nameHash 在候选 XOR key 中择优(确定性)。
///
/// 顺序: 先试 `0xFF`(引擎的逐字节取反约定),再试参考实现的推导
/// `enc[nameLen-4] ^ '.'`,最后全量 0..=255。评分 = 名字哈希命中数;
/// 命中数 == 条目数时立即返回(V8),平局优先 0xFF,其次更小的 key。
fn detect_name_key(entries: &[RawEntry]) -> u8 {
    let mut candidates: Vec<u8> = Vec::with_capacity(258);
    candidates.push(0xFF);
    if let Some(first) = entries.first() {
        let enc = &first.name_enc;
        if enc.len() >= 4 {
            candidates.push(enc[enc.len() - 4] ^ b'.');
        }
    }
    candidates.extend((0u16..=255u16).map(|k| k as u8));

    let mut best_key = 0xFFu8;
    let mut best_score = None;
    let mut seen = [false; 256];
    for key in candidates {
        if seen[key as usize] {
            continue;
        }
        seen[key as usize] = true;
        let score = entries
            .iter()
            .filter(|e| {
                let decoded: Vec<u8> = e.name_enc.iter().map(|b| b ^ key).collect();
                murmur2(&decoded, 0) == e.name_hash
            })
            .count();
        let better = match best_score {
            None => true,
            Some(current) => {
                score > current
                    || (score == current && best_key != 0xFF && (key == 0xFF || key < best_key))
            }
        };
        if better {
            best_score = Some(score);
            best_key = key;
        }
        if score == entries.len() {
            // 满分不可能被后来者超过;提前退出(官方包首轮即中)。
            return key;
        }
    }
    best_key
}

fn parse_hashed_len(
    block: &[u8],
    entry_count: u32,
    data_start: u32,
    file_len: u64,
) -> Candidate {
    let table = name_len_table(0);
    let meta = ENTRY_META_LEN;
    let skip = INDEX_START - HEADER_LEN;
    let mut pos = skip;
    // V7:容量按物理下限与实际条目数取小,不用 entryCount 直接放大分配。
    let capacity = (entry_count as usize).min(block.len() / ENTRY_STEP_EXTRA + 1);
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
        // dataOffset 是 8 字节(REA `FUN_0043fb9c`);高 4 字节旧称 unknown0。
        let data_offset_low =
            u32::from_le_bytes(block[name_end + 10..name_end + 14].try_into().unwrap());
        let data_offset_high =
            u32::from_le_bytes(block[name_end + 14..name_end + 18].try_into().unwrap());
        let data_offset = (u64::from(data_offset_high) << 32) | u64::from(data_offset_low);
        let payload_hash =
            u32::from_le_bytes(block[name_end + 18..name_end + 22].try_into().unwrap());
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
            unknown0: data_offset_high,
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
    // "布局判对了"的硬证据,所以无条件执行。
    let mut name_hash_matches = 0usize;
    let mut name_key = 0xFFu8;
    if !entries.is_empty() {
        name_key = detect_name_key(&entries);
        for entry in &mut entries {
            entry.name = entry.name_enc.iter().map(|b| b ^ name_key).collect();
            entry.name_hash_ok = murmur2(&entry.name, 0) == entry.name_hash;
            if entry.name_hash_ok {
                name_hash_matches += 1;
            }
        }
    }

    Candidate {
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
    fn layout_is_version_independent() {
        // REA `FUN_00462a10`/`FUN_0045e3b8`:表与 meta 都不随版本变化。
        for version in [0u32, 234, 300, 494, 495] {
            assert_eq!(name_len_table(version), &NAME_LEN_TABLE_00);
            assert_eq!(meta_len(version), ENTRY_META_LEN);
        }
    }

    #[test]
    fn murmur2_matches_official_pack_values() {
        // 官方包实测: 名字 hash 与负载 hash 都来自同一算法。
        assert_eq!(murmur2(br"ysbin\yst00034.ybn", 0), 0x0033_ae52);
        // MurmurHash2 参考实现的标准向量。
        assert_eq!(murmur2(b"", 0), 0);
        assert_eq!(murmur2(b"hello", 0), 0xe561_29cb);
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

        let candidate = parse_hashed_len(&block, 1, 0x20, u32::MAX as u64);
        assert_eq!(candidate.entries[0].data_offset, 1u64 << 32);
        assert_eq!(candidate.entries[0].unknown0, 1);
        assert!(candidate.reject.is_some(), "1<<32 偏移在小文件里必须越界");
    }

    /// V7:巨大 entryCount + 极小索引区不得产生放大分配。
    #[test]
    fn hostile_entry_count_does_not_allocate() {
        let block = vec![0u8; 16];
        let candidate = parse_hashed_len(&block, 99_999, 0x30, 0x30);
        assert!(candidate.reject.is_some());
        assert!(candidate.entries.len() <= 1, "不得按 entryCount 预分配");
    }

}
