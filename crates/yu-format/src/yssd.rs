//! `save/*.sd`(魔数 `YSSD`)· 存档容器解码 + 写路径(字节级对齐官方)。
//!
//! 证据:
//! - **官方产物自证**(真值层级 3):`<project-eris>/save/` 下 7 个 `.sd`
//!   (extra/gsd/kidoku/save30001/save30002/save50001/seld)由官方 exe
//!   0.495 写出;本模块 parse→rebuild 对全部文件**逐字节一致**(见
//!   `tests/yssd_roundtrip.rs`)。
//! - 旧接手线 `<repo-old>/crates/yuris-format/src/yssd.rs` 的布局事实
//!   (v494 引擎 SAVE 写盘 `0x44cba7` / LOAD `0x446ed0` 反汇编;本仓库
//!   只引用事实,未复制代码):头 = `magic + 版本 0x1E0 + 块数 +
//!   偏移表容量 0x400` + 偏移表 `0x400 × u32`(表项 `[i]` = 块号 `i+1`
//!   的绝对偏移,0 = 空);块 = `dno u32 + 类型 u8 + 严格维数 u8 +
//!   变量 id u16 + 压缩长 u32 + 未压长 u32 + 压缩载荷`。
//! - 压缩层:0.495 官方产物全部是 **zlib level 1**(`78 01` 头;每块
//!   重编码逐字节一致,`tests/yssd_roundtrip.rs` 全量断言)。旧线另一
//!   语料的 SNP 形态在本语料未出现 ⇒ 不实现、挂账(见模块尾)。
//!
//! 注意:容器版本是**格式修订号**,与引擎版本号没有换算关系 —— 0.479 SDK
//! 样例 `0x138`、0.495 SDK 样例 `0x1E0`、0.481 商业包 `0x1E1`,三者布局
//! 逐字节同构。因此读取**不校验版本**,只做结构自检(魔数 / 偏移表容量 /
//! DNO 连续 / 边界 / zlib 闭合);写回版本由调用方指定。
//!
//! 净室声明:实现为本仓库新写;flate2(双后端下 C zlib 优先)用于保证
//! `compress2(level=1)` 逐字节复现。

use std::io::{Read, Write};

use crate::code;
use crate::container::{read_header, truncated, u16_at, u32_at};
use yu_log::{Boundary, Diag};

/// YSSD 魔数。
pub const MAGIC: [u8; 4] = *b"YSSD";

/// 默认偏移表容量(官方产物恒 0x400)。读取以头部 `+0x0C` 为准。
pub const DEFAULT_OFFSET_TABLE_LEN: u32 = 0x400;

/// 默认「头 + 偏移表」字节数(数据区起点;官方产物首个块偏移即此值)。
pub const DEFAULT_TABLE_END: usize = 0x10 + DEFAULT_OFFSET_TABLE_LEN as usize * 4;

/// 一个 YSSD 块(压缩形态;载荷不在此解压)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct YssdEntry {
    /// 块号(= 脚本 `SAVE/LOAD` 的 `DNO`,1 基)。
    pub dno: u32,
    /// 块类型(0 = 单变量;1 = 配置块;2 = 多变量块)。
    pub btype: u8,
    /// 严格维数标志(官方产物恒 1)。
    pub strict: u8,
    /// 目标变量 id(容器内记录;`DomainCall` 事件层拿不到,DIV-RT-042)。
    pub var_id: u16,
    /// 未压长(与解压输出闭合)。
    pub ulen: u32,
    /// zlib level 1 压缩载荷(原样字节)。
    pub compressed: Vec<u8>,
}

impl YssdEntry {
    /// 解压载荷。两代压缩层并存:0.495 SDK 语料 = zlib level 1(块首
    /// `78 01`);商业包(0.481,随包带 `YSSnp.dll`)= Google Snappy。
    /// 先按 zlib 解,失败再按 Snappy 解;两者都不成立才报 `fmt.yssd.codec`。
    pub fn decompress(&self, origin: &[u8]) -> Result<Vec<u8>, Diag> {
        decompress_payload(&self.compressed, self.ulen, origin, self.dno)
    }
}

/// 已解析的存档容器。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct YssdSave {
    /// 容器版本(只记录,不参与校验;实测 0x138/0x1E0/0x1E1 布局同构)。
    pub version: u32,
    /// 块(按文件出现顺序;官方产物 = DNO 升序)。
    pub entries: Vec<YssdEntry>,
    /// DNO → `entries` 下标(偏移表展开;空槽为 `None`)。
    index: Vec<Option<u16>>,
}

impl YssdSave {
    /// 从字节解析(头 + 偏移表 + 全部块)。偏移表之外的尾随字节不存在
    /// (官方产物块区连续到文件尾)。
    pub fn parse(buf: &[u8], origin: &[u8]) -> Result<Self, Diag> {
        let header = read_header(buf, origin, &MAGIC)?;
        // 版本只记录、不校验:0x138/0x1E0/0x1E1
        // 布局同构,真正的合法性由下面的结构自检决定。
        let capacity = header.word0c as usize;
        if capacity == 0 {
            return Err(Diag::fatal(code::LIMIT, Boundary::Format)
                .at_data(origin, 0x0C)
                .with_detail("YSSD 偏移表容量为 0(块表无处寻址)"));
        }
        // 块号槽是 u16(DNO 1..=capacity);容量超过 u16 上限无法建索引。
        if capacity > usize::from(u16::MAX) + 1 {
            return Err(Diag::fatal(code::LIMIT, Boundary::Format)
                .at_data(origin, 0x0C)
                .with_detail(format!(
                    "YSSD 偏移表容量 {capacity:#x} 超出 16 位块号槽上限"
                )));
        }
        let table_end = capacity
            .checked_mul(4)
            .and_then(|bytes| 0x10usize.checked_add(bytes))
            .ok_or_else(|| {
                Diag::fatal(code::LIMIT, Boundary::Format)
                    .at_data(origin, 0x0C)
                    .with_detail(format!("YSSD 偏移表容量 {capacity:#x} 导致表长溢出"))
            })?;
        if buf.len() < table_end {
            return Err(truncated(origin, table_end, buf.len(), "YSSD 头+偏移表"));
        }
        let mut entries = Vec::new();
        let mut index: Vec<Option<u16>> = vec![None; capacity];
        for (i, cell) in buf[0x10..table_end].chunks_exact(4).enumerate() {
            let off = u32::from_le_bytes([cell[0], cell[1], cell[2], cell[3]]) as usize;
            if off == 0 {
                continue;
            }
            // 块至少要有 0x10 字节头;偏移必须落在数据区内。
            if off < table_end || off > buf.len().saturating_sub(0x10) {
                return Err(Diag::fatal(code::COUNT, Boundary::Format)
                    .at_data(origin, (0x10 + i * 4) as u64)
                    .with_detail(format!(
                        "YSSD 偏移表[{}](DNO={})偏移 {off:#x} 越界(文件 {} 字节,数据区自 {table_end:#x})",
                        i + 1,
                        i + 1,
                        buf.len()
                    )));
            }
            let dno = u32_at(buf, off);
            if dno as usize != i + 1 {
                return Err(Diag::fatal(code::COUNT, Boundary::Format)
                    .at_data(origin, off as u64)
                    .with_detail(format!(
                        "YSSD 块号与偏移表不符:表项[{}]=DNO {dno}(官方容器两处一致)",
                        i + 1
                    )));
            }
            let clen = u32_at(buf, off + 8) as usize;
            let payload_at = off + 0x10;
            if clen > buf.len().saturating_sub(payload_at) {
                return Err(truncated(
                    origin,
                    payload_at + clen,
                    buf.len(),
                    "YSSD 块载荷",
                ));
            }
            index[i] = Some(entries.len() as u16);
            entries.push(YssdEntry {
                dno,
                btype: buf[off + 4],
                strict: buf[off + 5],
                var_id: u16_at(buf, off + 6),
                ulen: u32_at(buf, off + 12),
                compressed: buf[payload_at..payload_at + clen].to_vec(),
            });
        }
        Ok(Self {
            version: header.version,
            entries,
            index,
        })
    }

    /// 按 DNO 取块(DNO 0 或超出表容量 = `None`)。
    #[must_use]
    pub fn entry(&self, dno: u32) -> Option<&YssdEntry> {
        let slot = usize::try_from(dno).ok()?.checked_sub(1)?;
        self.index
            .get(slot)
            .copied()
            .flatten()
            .and_then(|i| self.entries.get(usize::from(i)))
    }
}

/// zlib(RFC1950)解压,校验未压长闭合。
pub fn zlib_decompress(
    src: &[u8],
    expected_len: u32,
    origin: &[u8],
    dno: u32,
) -> Result<Vec<u8>, Diag> {
    let mut out = Vec::with_capacity(expected_len as usize);
    let mut decoder = flate2::read::ZlibDecoder::new(src);
    decoder
        .read_to_end(&mut out)
        .map_err(|e| codec_diag(origin, dno, format!("zlib 解压失败: {e}")))?;
    if out.len() as u64 != u64::from(expected_len) {
        return Err(codec_diag(
            origin,
            dno,
            format!("zlib 解压长不符:头称 {},实际 {}", expected_len, out.len()),
        ));
    }
    Ok(out)
}

/// `compress2(level = 1)` 等价压缩(0.495 官方写盘形态;roundtrip 逐字节
/// 一致由 `tests/yssd_roundtrip.rs` 对全部官方产物断言)。
#[must_use]
pub fn zlib_compress_level1(raw: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(
        Vec::with_capacity(raw.len() / 2 + 64),
        flate2::Compression::new(1),
    );
    // Vec 的 Write 永不失败(容量不足只会扩容)。
    let _ = encoder.write_all(raw);
    encoder
        .finish()
        .expect("ZlibEncoder finish over Vec never fails")
}

fn codec_diag(origin: &[u8], dno: u32, detail: String) -> Diag {
    Diag::fatal(code::YSSD_CODEC, Boundary::Format)
        .at_data(origin, 0)
        .with_detail(format!("DNO={dno}: {detail}"))
}

/// 已解压的值载荷(块类型 0;`FUN_0044564d` 同族消费形态)。
///
/// 布局:`ty u32(1=INT 2=FLT 3=STR) + 维数 u32 + 边界 u32[维数] +
/// 数据长 u32 + 数据`。INT/FLT 每元素 8 字节(i64/f64);STR 每元素
/// `{u32 长度 + 字节}`(长度 = 字节数,无终止符)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct YssdValue {
    pub ty: YssdValueType,
    /// 每维边界(下标 0 = 第一维;标量为空)。
    pub dims: Vec<u32>,
    /// 数据(INT/FLT = 8B/元素;STR = 逐元素 `{u32 长度 + 字节}`)。
    pub data: Vec<u8>,
}

/// 值类型(容器内 `ty u32`)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum YssdValueType {
    Int,
    Flt,
    Str,
}

impl YssdValueType {
    pub const fn code(self) -> u32 {
        match self {
            YssdValueType::Int => 1,
            YssdValueType::Flt => 2,
            YssdValueType::Str => 3,
        }
    }

    fn parse(code: u32) -> Option<Self> {
        match code {
            1 => Some(YssdValueType::Int),
            2 => Some(YssdValueType::Flt),
            3 => Some(YssdValueType::Str),
            _ => None,
        }
    }
}

impl YssdValue {
    /// 元素总数(维数积;标量 = 1)。
    #[must_use]
    pub fn element_count(&self) -> u64 {
        self.dims
            .iter()
            .map(|&d| u64::from(d))
            .product::<u64>()
            .max(if self.dims.is_empty() { 1 } else { 0 })
    }

    /// 从已解压字节解析(结构闭合校验;官方 7 文件全量通过)。
    pub fn parse(raw: &[u8], origin: &[u8], dno: u32) -> Result<Self, Diag> {
        if raw.len() < 8 {
            return Err(truncated(origin, 8, raw.len(), "YSSD 值载荷头"));
        }
        let Some(ty) = YssdValueType::parse(u32_at(raw, 0)) else {
            return Err(Diag::fatal(code::COUNT, Boundary::Format)
                .at_data(origin, 0)
                .with_detail(format!(
                    "DNO={dno}:值类型 {} 不在 1(INT)/2(FLT)/3(STR)",
                    u32_at(raw, 0)
                )));
        };
        let dim = u32_at(raw, 4) as usize;
        if dim > 8 {
            return Err(Diag::fatal(code::LIMIT, Boundary::Format)
                .at_data(origin, 4)
                .with_detail(format!("DNO={dno}:维数 {dim} > 8(引擎上限 900000 同族)")));
        }
        let dims_at = 8;
        let dlen_at = dims_at + dim * 4;
        if raw.len() < dlen_at + 4 {
            return Err(truncated(origin, dlen_at + 4, raw.len(), "YSSD 值载荷边界"));
        }
        let dims: Vec<u32> = raw[dims_at..dlen_at]
            .chunks_exact(4)
            .map(u32_at_nocopy)
            .collect();
        let dlen = u32_at(raw, dlen_at) as usize;
        let data_at = dlen_at + 4;
        if dlen > raw.len().saturating_sub(data_at) {
            return Err(truncated(
                origin,
                data_at + dlen,
                raw.len(),
                "YSSD 值载荷数据",
            ));
        }
        let data = raw[data_at..data_at + dlen].to_vec();
        // 结构闭合:每类型的数据布局必须恰好装满。
        let elements = match ty {
            YssdValueType::Int | YssdValueType::Flt => {
                let count = dims
                    .iter()
                    .map(|&d| u64::from(d))
                    .product::<u64>()
                    .max(if dims.is_empty() { 1 } else { 0 });
                count.saturating_mul(8)
            }
            YssdValueType::Str => {
                let count = dims
                    .iter()
                    .map(|&d| u64::from(d))
                    .product::<u64>()
                    .max(if dims.is_empty() { 1 } else { 0 });
                let mut consumed = 0u64;
                let mut cursor = 0usize;
                for i in 0..count {
                    if cursor + 4 > data.len() {
                        return Err(truncated(
                            origin,
                            data_at + cursor + 4,
                            data_at + data.len(),
                            &format!("DNO={dno}:STR 元素[{i}]长度头"),
                        ));
                    }
                    let len = u32_at(&data[cursor..], 0) as u64;
                    cursor += 4;
                    if cursor + len as usize > data.len() {
                        return Err(truncated(
                            origin,
                            data_at + cursor + len as usize,
                            data_at + data.len(),
                            &format!("DNO={dno}:STR 元素[{i}]内容"),
                        ));
                    }
                    cursor += len as usize;
                    consumed = cursor as u64;
                }
                consumed
            }
        };
        if dlen as u64 != elements {
            return Err(Diag::fatal(code::COUNT, Boundary::Format)
                .at_data(origin, dlen_at as u64)
                .with_detail(format!(
                    "DNO={dno}:数据长 {dlen} 与类型/维数闭合不符(应 {elements})"
                )));
        }
        Ok(Self { ty, dims, data })
    }

    /// 序列化([`Self::parse`] 的逆)。
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(12 + self.dims.len() * 4 + self.data.len());
        out.extend_from_slice(&self.ty.code().to_le_bytes());
        out.extend_from_slice(&(self.dims.len() as u32).to_le_bytes());
        for &d in &self.dims {
            out.extend_from_slice(&d.to_le_bytes());
        }
        out.extend_from_slice(&(self.data.len() as u32).to_le_bytes());
        out.extend_from_slice(&self.data);
        out
    }
}

fn u32_at_nocopy(cell: &[u8]) -> u32 {
    u32::from_le_bytes([cell[0], cell[1], cell[2], cell[3]])
}

/// 块体(`dno u32` 之后):`btype u8 + strict u8 + var u16 + clen u32 +
/// ulen u32 + 压缩载荷`。
#[must_use]
pub fn encode_block_body(btype: u8, strict: u8, var_id: u16, raw_payload: &[u8]) -> Vec<u8> {
    let compressed = zlib_compress_level1(raw_payload);
    let mut out = Vec::with_capacity(12 + compressed.len());
    out.push(btype);
    out.push(strict);
    out.extend_from_slice(&var_id.to_le_bytes());
    out.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
    out.extend_from_slice(&(raw_payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&compressed);
    out
}

/// 存档容器写出器(`SAVE[FILE]` 刷盘形态)。
///
/// 头 = `magic + 版本 + 块数 + 0x400` + 全零偏移表(逐块回填);块按
/// **DNO 升序**落盘(官方产物实测:extra 11,12,21,22,31,41;gsd 1..4;
/// save30001 1..8 均升序)。
///
/// 版本由调用方给出(**没有默认常量**):读改写应沿用源文件版本,
/// 新建存档应沿用同一存档目录里既有 `.sd` 的版本(见 `yu-save`)。
#[derive(Debug)]
pub struct YssdWriter {
    blocks: Vec<(u32, Vec<u8>)>,
    version: u32,
}

impl YssdWriter {
    /// 指定写出的容器版本(调用方从源文件/同目录存档学习,不猜)。
    #[must_use]
    pub fn new(version: u32) -> Self {
        Self {
            blocks: Vec::new(),
            version,
        }
    }

    /// 登记一块(DNO 1..=1024,手册 `file/save.html` 值域)。
    pub fn set_block(&mut self, dno: u32, body: Vec<u8>) {
        debug_assert!(
            (1..=1024).contains(&dno),
            "DNO 越界(手册 file/save.html:1~1024)"
        );
        self.blocks.push((dno, body));
    }

    /// 已登记块数。
    #[must_use]
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    /// 产出完整 `.sd` 字节(块按 DNO 升序;同 DNO 后登记者覆盖)。
    #[must_use]
    pub fn build(self) -> Vec<u8> {
        // 同 DNO 覆盖:保留每个 DNO 的最后一次登记。
        let mut latest: std::collections::BTreeMap<u32, Vec<u8>> = Default::default();
        for (dno, body) in self.blocks {
            latest.insert(dno, body);
        }
        let mut out = Vec::with_capacity(DEFAULT_TABLE_END + latest.len() * 64);
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&self.version.to_le_bytes());
        out.extend_from_slice(&(latest.len() as u32).to_le_bytes());
        out.extend_from_slice(&DEFAULT_OFFSET_TABLE_LEN.to_le_bytes());
        let table_at = out.len();
        out.resize(table_at + DEFAULT_OFFSET_TABLE_LEN as usize * 4, 0);
        for (dno, body) in latest {
            let off = out.len() as u32;
            out.extend_from_slice(&dno.to_le_bytes());
            out.extend_from_slice(&body);
            let cell = table_at + (dno as usize - 1) * 4;
            out[cell..cell + 4].copy_from_slice(&off.to_le_bytes());
        }
        out
    }
}

/// 块类型 1:配置块载荷(`u32 段A长 + u32 段B长 + 段A + 段B`;引擎
/// `0x44760c` 同族。段内字段语义未逆向,只保证结构往返 —— 旧线同判)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct YssdConfigPayload {
    pub section_a: Vec<u8>,
    pub section_b: Vec<u8>,
}

impl YssdConfigPayload {
    pub fn parse(raw: &[u8], origin: &[u8], dno: u32) -> Result<Self, Diag> {
        if raw.len() < 8 {
            return Err(truncated(
                origin,
                8,
                raw.len(),
                &format!("DNO={dno} 配置载荷头"),
            ));
        }
        let (a_len, b_len) = (u32_at(raw, 0) as usize, u32_at(raw, 4) as usize);
        if a_len.saturating_add(b_len) > raw.len().saturating_sub(8) {
            return Err(truncated(
                origin,
                8 + a_len + b_len,
                raw.len(),
                &format!("DNO={dno} 配置载荷段(头称 A={a_len} B={b_len})"),
            ));
        }
        Ok(Self {
            section_a: raw[8..8 + a_len].to_vec(),
            section_b: raw[8 + a_len..8 + a_len + b_len].to_vec(),
        })
    }

    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(8 + self.section_a.len() + self.section_b.len());
        out.extend_from_slice(&(self.section_a.len() as u32).to_le_bytes());
        out.extend_from_slice(&(self.section_b.len() as u32).to_le_bytes());
        out.extend_from_slice(&self.section_a);
        out.extend_from_slice(&self.section_b);
        out
    }
}

/// 块类型 2 的子块(`pad u16 + var u16 + clen u32 + ulen u32 + zlib[clen]`;
/// 引擎 LOAD 循环消费,无子块计数字段)。官方产物 `pad` 恒 `0x0100`,
/// 语义未取证(旧线同判:Unknown 不猜)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct YssdSubBlock {
    pub pad: u16,
    pub var_id: u16,
    pub ulen: u32,
    pub compressed: Vec<u8>,
}

impl YssdSubBlock {
    pub fn decompress(&self, origin: &[u8], dno: u32) -> Result<Vec<u8>, Diag> {
        decompress_payload(&self.compressed, self.ulen, origin, dno)
    }
}

/// 块载荷解压:两代压缩层并存 —— 0.495 SDK 语料 = zlib level 1;商业包
/// (0.481,随包带 `YSSnp.dll`)= Google Snappy。先 zlib 后 Snappy,两者都
/// 不成立才报 `fmt.yssd.codec`。
fn decompress_payload(
    src: &[u8],
    expected_len: u32,
    origin: &[u8],
    dno: u32,
) -> Result<Vec<u8>, Diag> {
    match zlib_decompress(src, expected_len, origin, dno) {
        Ok(raw) => Ok(raw),
        Err(zlib_err) => match crate::snappy::decompress(src, expected_len) {
            Ok(raw) => Ok(raw),
            Err(snappy_err) => Err(codec_diag(
                origin,
                dno,
                format!(
                    "压缩层既不是 zlib 也不是 snappy(zlib: {};snappy: {snappy_err})",
                    zlib_err.detail
                ),
            )),
        },
    }
}

/// 解析块类型 2 的外层裸载荷(子块首尾相接,按「已消费 ≥ 总长」终止)。
pub fn parse_multi_payload(raw: &[u8], origin: &[u8], dno: u32) -> Result<Vec<YssdSubBlock>, Diag> {
    let mut subs = Vec::new();
    let mut cursor = 0usize;
    while cursor < raw.len() {
        if cursor + 12 > raw.len() {
            return Err(truncated(
                origin,
                cursor + 12,
                raw.len(),
                &format!("DNO={dno} 多变量子块头"),
            ));
        }
        let pad = u16_at(raw, cursor);
        let var_id = u16_at(raw, cursor + 2);
        let clen = u32_at(raw, cursor + 4) as usize;
        let ulen = u32_at(raw, cursor + 8);
        cursor += 12;
        if clen > raw.len().saturating_sub(cursor) {
            return Err(truncated(
                origin,
                cursor + clen,
                raw.len(),
                "YSSD 多变量子块载荷",
            ));
        }
        subs.push(YssdSubBlock {
            pad,
            var_id,
            ulen,
            compressed: raw[cursor..cursor + clen].to_vec(),
        });
        cursor += clen;
    }
    Ok(subs)
}

/// 编码块类型 2 块体。外层**不整体压缩**:`clen = 内层实际字节长`;
/// `ulen = Σ(子块 ulen + 12)`(官方 ERIS 语料实测:gsd DNO=3/4 ——
/// 引擎把「子块头 + 未压子载荷」的总长记为外层未压长;旧线的
/// 「clen = ulen = 内层长」在 ERIS 不成立,以官方产物为准)。
/// 子块各自由 [`zlib_compress_level1`] 压缩,`pad` 恒 `0x0100`(语料恒值)。
pub fn encode_multi_body(subs: &[YssdSubBlockRaw<'_>]) -> Vec<u8> {
    let mut inner = Vec::new();
    let mut ulen_total: u64 = 0;
    for sub in subs {
        let compressed = zlib_compress_level1(sub.raw);
        inner.extend_from_slice(&0x0100u16.to_le_bytes());
        inner.extend_from_slice(&sub.var_id.to_le_bytes());
        inner.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        inner.extend_from_slice(&(sub.raw.len() as u32).to_le_bytes());
        inner.extend_from_slice(&compressed);
        ulen_total += sub.raw.len() as u64 + 12;
    }
    let mut out = Vec::with_capacity(12 + inner.len());
    out.push(2);
    out.push(1);
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(inner.len() as u32).to_le_bytes());
    out.extend_from_slice(&(ulen_total as u32).to_le_bytes());
    out.extend_from_slice(&inner);
    out
}

/// [`encode_multi_body`] 的输入:子变量 id + 未压子载荷。
#[derive(Clone, Copy, Debug)]
pub struct YssdSubBlockRaw<'a> {
    pub var_id: u16,
    pub raw: &'a [u8],
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 官方形态的最小 INT 标量块(与 save30001 DNO=1 同构:
    /// ty=1 dims=0 dlen=8 值 5)。zlib level 1 字节为固定输入的确定产物,
    /// 硬编码为 golden,守卫「压缩层参数漂移」。
    #[test]
    fn scalar_int_payload_compresses_to_pinned_zlib_level1() {
        let payload = YssdValue {
            ty: YssdValueType::Int,
            dims: Vec::new(),
            data: 5i64.to_le_bytes().to_vec(),
        };
        let compressed = zlib_compress_level1(&payload.to_bytes());
        // Python `zlib.compress(raw, 1)`(zlib 1.3.1)与官方引擎 compress2(1)
        // 同族;且与官方 extra.sd DNO=12 块(`78 01 63 64 …`,INT 标量 5)
        // 逐字节相同 —— 该常量同时就是官方产物切片。
        assert_eq!(
            compressed,
            [
                0x78, 0x01, 0x63, 0x64, 0x80, 0x00, 0x0e, 0x20, 0xc5, 0x0a, 0x65, 0x03, 0x00, 0x00,
                0xb0, 0x00, 0x0f,
            ]
        );
        let back = YssdValue::parse(
            &zlib_decompress(&compressed, payload.to_bytes().len() as u32, b"t", 1).expect("zlib"),
            b"t",
            1,
        )
        .expect("parse");
        assert_eq!(back, payload);
    }

    #[test]
    fn writer_roundtrips_a_synthetic_container() {
        let payload = YssdValue {
            ty: YssdValueType::Int,
            dims: vec![3],
            data: vec![0u8; 24],
        };
        let mut writer = YssdWriter::new(0x1234);
        writer.set_block(2, encode_block_body(0, 1, 0x0a43, &payload.to_bytes()));
        writer.set_block(1, encode_block_body(0, 1, 0x0a44, &payload.to_bytes()));
        let bytes = writer.build();
        let save = YssdSave::parse(&bytes, b"synthetic").expect("parse");
        assert_eq!(save.version, 0x1234);
        assert_eq!(save.entries.len(), 2);
        // 块按 DNO 升序落盘。
        assert_eq!(save.entries[0].dno, 1);
        assert_eq!(save.entries[1].dno, 2);
        assert_eq!(save.entry(2).expect("dno2").var_id, 0x0a43);
        // 容器级往返:重建字节完全一致。
        let mut again = YssdWriter::new(0x1234);
        for entry in &save.entries {
            let raw = entry.decompress(b"synthetic").expect("decompress");
            again.set_block(
                entry.dno,
                encode_block_body(entry.btype, entry.strict, entry.var_id, &raw),
            );
        }
        assert_eq!(again.build(), bytes);
    }

    #[test]
    fn value_payload_rejects_bad_shapes() {
        // INT 声明 2 元素但数据只有 8 字节 ⇒ COUNT。
        let mut raw = Vec::new();
        raw.extend_from_slice(&1u32.to_le_bytes());
        raw.extend_from_slice(&1u32.to_le_bytes());
        raw.extend_from_slice(&2u32.to_le_bytes());
        raw.extend_from_slice(&8u32.to_le_bytes());
        raw.extend_from_slice(&0i64.to_le_bytes());
        let err = YssdValue::parse(&raw, b"t", 7).expect_err("count mismatch");
        assert_eq!(err.code, code::COUNT);
        // 未知类型 ⇒ COUNT。
        let mut bad = Vec::new();
        bad.extend_from_slice(&9u32.to_le_bytes());
        bad.extend_from_slice(&0u32.to_le_bytes());
        bad.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(
            YssdValue::parse(&bad, b"t", 7).expect_err("type").code,
            code::COUNT
        );
    }

    #[test]
    fn parse_accepts_any_version_but_checks_structure() {
        // 魔数错 ⇒ MAGIC。
        let mut bad_magic = vec![b'X', b'S', b'S', b'D'];
        bad_magic.extend_from_slice(&0x1234u32.to_le_bytes());
        bad_magic.extend_from_slice(&[0u8; 8]);
        bad_magic.resize(DEFAULT_TABLE_END, 0);
        assert_eq!(
            YssdSave::parse(&bad_magic, b"t").expect_err("magic").code,
            code::MAGIC
        );

        // 版本不校验:0x138 / 0x1E0 / 0x1E1 / 未知值都按结构接受,版本原样保留。
        for version in [0x138u32, 0x1E0, 0x1E1, 0x1EF] {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(b"YSSD");
            bytes.extend_from_slice(&version.to_le_bytes());
            bytes.extend_from_slice(&0u32.to_le_bytes()); // 块数
            bytes.extend_from_slice(&DEFAULT_OFFSET_TABLE_LEN.to_le_bytes());
            bytes.resize(DEFAULT_TABLE_END, 0);
            let save = YssdSave::parse(&bytes, b"t").expect("结构合法即接受");
            assert_eq!(save.version, version);
            assert!(save.entries.is_empty());
        }

        // 容量为 0 ⇒ LIMIT(块表无处寻址)。
        let mut zero = Vec::new();
        zero.extend_from_slice(b"YSSD");
        zero.extend_from_slice(&0x1234u32.to_le_bytes());
        zero.extend_from_slice(&0u32.to_le_bytes());
        zero.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(
            YssdSave::parse(&zero, b"t").expect_err("capacity").code,
            code::LIMIT
        );
    }

    #[test]
    fn parse_uses_header_capacity_not_a_constant() {
        // 4 槽表 + 1 块(DNO=2):容量来自头部 +0x0C,不是写死的 0x400。
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"YSSD");
        bytes.extend_from_slice(&0x1E1u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes()); // 块数
        bytes.extend_from_slice(&4u32.to_le_bytes()); // 容量 4
        let table_at = bytes.len();
        bytes.resize(table_at + 4 * 4, 0);
        let block_at = bytes.len() as u32;
        bytes[table_at + 4..table_at + 8].copy_from_slice(&block_at.to_le_bytes());
        bytes.extend_from_slice(&2u32.to_le_bytes()); // dno
        bytes.push(0); // btype
        bytes.push(1); // strict
        bytes.extend_from_slice(&0x1234u16.to_le_bytes()); // var_id
        bytes.extend_from_slice(&0u32.to_le_bytes()); // clen
        bytes.extend_from_slice(&0u32.to_le_bytes()); // ulen

        let save = YssdSave::parse(&bytes, b"t").expect("parse");
        assert_eq!(save.entry(2).expect("dno2").var_id, 0x1234);
        assert!(save.entry(1).is_none());
        assert!(save.entry(5).is_none(), "超出表容量 = None");
    }

    #[test]
    fn writer_preserves_requested_version() {
        let mut writer = YssdWriter::new(0x1E1);
        writer.set_block(1, encode_block_body(0, 1, 0x0a44, &[0u8; 8]));
        let bytes = writer.build();
        let save = YssdSave::parse(&bytes, b"t").expect("parse");
        assert_eq!(save.version, 0x1E1);
    }
}
