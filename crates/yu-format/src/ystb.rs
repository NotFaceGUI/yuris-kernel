//! `yst%05d.ybn`(魔数 `YSTB`)· 脚本体解码 → 契约类型 [`DecodedScript`]。
//!
//! 布局(ERIS 语料 136 文件全量实证;参考 RxYuris YSTB.cpp `Xor`/`YSTB_V5`
//! 与 YU-RIS-Script-Editor `core/ystb.py` `_parse_v5` 的解析流程;V2 支路
//! 两家都有,但**本仓库无语料,禁臆造**——见 `docs/divergences/DIV-007`):
//!
//! ```text
//! +0x00 char[4] "YSTB"
//! +0x04 u32     版本(495;200 < v < 300 = V2 → Unimplemented)
//! +0x08 u32     指令条数 —— **唯一迭代依据**(见下)
//! +0x0C u32     s1 指令索引字节数(实测 = 条数 × 4)
//! +0x10 u32     s2 参数索引字节数(实测为 12 的倍数)
//! +0x14 u32     s3 参数数据字节数
//! +0x18 u32     s4 行号区字节数(实测 = 条数 × 4)
//! +0x1C u32     观测为 0
//! +0x20 四段依次排列,每段独立从密钥下标 0 开始异或
//!       (密钥缺省 = CRC32("Yu-ris") 小端字节,见 crate::key)
//! 四段之后若还有字节 = 尾部(原样保留,回封时逐字节写回)。
//!
//! # 头 count 是权威(REA 0.495 `FUN_004518e9`)
//!
//! 引擎装载器先用 `+0x0C..+0x18` 定位四段指针,随后**以 `+0x08` 的
//! count 作为循环上界**读取每条指令的 `opcode/argc`(`local_1c = *puVar14`
//! 即头 count,`iVar6 < count` 循环),并用每指令 `argc × 0xC` 推进参数
//! 窗口指针。它从不使用 `s1 / 4` 推断条数。因此本层同样以头 count 为准:
//! `count × 4 > s1` = `fmt.count`;`s1` 大于 `count × 4` 的富余字节原样保留。
//!
//! # 错误密钥自检
//!
//! 解密后先做与密钥无关的结构自检(指令区长度、`Σ argc × 12 == s2`、
//! 非零 type 窗口的 `offset + size` 边界),失败即 `fmt.ybn.key` ——
//! 错误密钥解出的随机字节几乎必然破坏这些不变量,不能让它滑到
//! `fmt.ybn.opcode` 或产出垃圾指令。type=0 窗口的字段语义未证
//! (ERIS 语料里存在 `offset+size` 越界的 type=0 窗口),不参与边界检查。
//!
//! 指令索引[count]: { u8 opcode, u8 参数个数, u8 anc0, u8 anc1 }
//!                  (anc0/anc1 语义未证;0.495 语料观测 0..9 / 0)
//! 参数索引[总参数数]: 12 字节窗口(Window 契约,原样保留)
//!                  { u16 id, u16 type, u32 载荷长, u32 载荷在数据区偏移 }
//! 参数数据:窗口载荷宿主。已知载荷形态(解释归运行核,本层不解释):
//!   'M' + u16 len + `"`文本`"` = 引号字符串;`H` + u16 len + 符号;
//!   `V` + u16 len + 表达式字节码;`L` + 8 字节 = i64;`v` = 变量引用。
//! 行号区[count × 4]:每指令对应 .yst 源行号(不进契约,留在
//!   [`Segments`] 备查)。
//! ```
//!
//! **指令序号 = ysc 命令表记录下标**(实证:GO=43、GOSUB=44、TEXT=96、
//! FILEACT=21、S_STR/S_INT=82/81、END=14 与源码逐条对拍一致,见
//! PROGRESS 成果 B2)。

use std::borrow::Cow;

use yu_log::{Boundary, Diag};

use crate::code;
use crate::container::{truncated, u32_at};
use crate::key::xor_segment;
use crate::yscm::CommandTable;

// 契约类型定义在 crate 根(冻结接口,见 lib.rs §契约)。
use crate::{Codec, DecodedScript, Group, LabelRef, LabelTable, Window};

/// YSTB 的 V2 版本带(200 < version < 300;无语料,不实现)。
const V2_MIN: u32 = 200;
const V2_MAX: u32 = 300;

/// 解密后的四区段(低层视图;`DecodedScript` 之外的原始材料)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segments {
    /// 容器头版本(`+0x04`;R12 起 yu-rt 适配层需要)。
    pub version: u32,
    /// 头部 `+0x08` 指令条数(权威;引擎 `FUN_004518e9` 以它为循环上界)。
    pub count: u32,
    /// 指令索引区(count × 4 字节)。
    pub inst: Vec<u8>,
    /// 参数索引区(12 字节窗口序列)。
    pub args_index: Vec<u8>,
    /// 参数数据区(窗口载荷宿主)。
    pub args_data: Vec<u8>,
    /// 行号区(count × 4 字节,原样保留)。
    pub line_numbers: Vec<u8>,
    /// 头部 `+0x1C` 字段(观测为 0;原样保留)。
    pub tail_word: u32,
    /// 四段之后的尾部字节(原样保留;回封时逐字节写回)。
    pub trailing: Vec<u8>,
}

impl Segments {
    /// 解密(或加密——异或是对合)`yst%05d.ybn` 的四区段。
    ///
    /// 头部明文不动;`key` 传 [`crate::key::DEFAULT_KEY_BYTES`] 之外的值
    /// 时,调用方自负其责(换键作品须先取回密钥,本层不猜)。
    pub fn decrypt(data: &[u8], key: &[u8; 4], origin: &[u8]) -> Result<Self, Diag> {
        if data.len() < 0x20 || &data[0..4] != b"YSTB" {
            return Err(Diag::fatal(code::MAGIC, Boundary::Format)
                .at_data(origin, 0)
                .with_detail(format!(
                    "魔数不符:期望 YSTB,实际 {:?}",
                    String::from_utf8_lossy(data.get(0..4).unwrap_or(b""))
                )));
        }
        let version = u32_at(data, 4);
        if V2_MIN < version && version < V2_MAX {
            return Err(Diag::unimplemented(code::YBN_UNSUPPORTED, Boundary::Format)
                .at_data(origin, 4)
                .with_detail(format!(
                    "YSTB V2(版本 {version})布局与 V5 不同且无语料,禁臆造;\
                     见 divergences/DIV-007(0.495 引擎装载器无版本分支,但不证明 V2 可读)"
                )));
        }
        let count = u32_at(data, 8);
        let s1 = u32_at(data, 0x0C) as usize;
        let s2 = u32_at(data, 0x10) as usize;
        let s3 = u32_at(data, 0x14) as usize;
        let s4 = u32_at(data, 0x18) as usize;
        let a1 = checked_at(0x20, s1, origin)?;
        let a2 = checked_at(a1, s2, origin)?;
        let a3 = checked_at(a2, s3, origin)?;
        let end = checked_at(a3, s4, origin)?;
        if end > data.len() {
            return Err(truncated(origin, end, data.len(), "YSTB 四区段"));
        }
        let mut inst = data[0x20..a1].to_vec();
        let mut args_index = data[a1..a2].to_vec();
        let mut args_data = data[a2..a3].to_vec();
        let mut line_numbers = data[a3..end].to_vec();
        xor_segment(&mut inst, key);
        xor_segment(&mut args_index, key);
        xor_segment(&mut args_data, key);
        xor_segment(&mut line_numbers, key);
        let segs = Self {
            version,
            count,
            inst,
            args_index,
            args_data,
            line_numbers,
            tail_word: u32_at(data, 0x1C),
            trailing: data[end..].to_vec(),
        };
        segs.self_check(origin)?;
        Ok(segs)
    }

    /// 与密钥无关的结构自检;失败按错误密钥上报 `fmt.ybn.key`。
    ///
    /// 依据:0.495 `FUN_004518e9` 以头 count 读取每条指令的 `argc`,
    /// 并以 `argc × 12` 推进参数窗口;四段长度必须与 count/argc 自洽。
    fn self_check(&self, origin: &[u8]) -> Result<(), Diag> {
        let count = usize::try_from(self.count).map_err(|_| limit(origin, 8, "count"))?;
        let inst_bytes = count
            .checked_mul(4)
            .ok_or_else(|| limit(origin, 8, "count × 4"))?;
        if inst_bytes > self.inst.len() {
            return Err(Diag::fatal(code::COUNT, Boundary::Format)
                .at_data(origin, 8)
                .with_detail(format!(
                    "头 count={count} 需要 {inst_bytes} 字节指令区,实际 {} 字节",
                    self.inst.len()
                )));
        }
        let mut windows = 0usize;
        for index in 0..count {
            let argc = usize::from(self.inst[index * 4 + 1]);
            windows = windows
                .checked_add(argc)
                .ok_or_else(|| limit(origin, (index * 4 + 1) as u64, "argc 累计"))?;
        }
        let expected = windows
            .checked_mul(12)
            .ok_or_else(|| limit(origin, 0x10, "窗口字节数"))?;
        if expected != self.args_index.len() {
            return Err(key_suspect(
                origin,
                format!(
                    "Σ argc × 12 = {expected} 与参数索引区 {} 字节不符",
                    self.args_index.len()
                ),
            ));
        }
        for slot in 0..windows {
            let at = slot * 12;
            let ty = u16::from(self.args_index[at + 2])
                | u16::from(self.args_index[at + 3]) << 8;
            let size = u32::from_le_bytes(
                self.args_index[at + 4..at + 8].try_into().unwrap(),
            ) as usize;
            let offset = u32::from_le_bytes(
                self.args_index[at + 8..at + 12].try_into().unwrap(),
            ) as usize;
            if ty != 0 && size > 0 {
                let fits = offset
                    .checked_add(size)
                    .is_some_and(|v| v <= self.args_data.len());
                if !fits {
                    return Err(key_suspect(
                        origin,
                        format!(
                            "窗口 {slot} 的 offset {offset} + size {size} 超出参数数据区 {} 字节",
                            self.args_data.len()
                        ),
                    ));
                }
            }
        }
        Ok(())
    }

    /// 重新封装为磁盘形态(异或对合:`decrypt` 后原样 `encrypt` = 原文件)。
    ///
    /// 头 count、四段长度、`+0x1C` 与尾部字节全部按原值写回。
    #[must_use]
    pub fn encrypt(&self, version: u32, key: &[u8; 4]) -> Vec<u8> {
        let size = [0x20usize, self.inst.len(), self.args_index.len(), self.args_data.len(), self.line_numbers.len(), self.trailing.len()]
            .into_iter()
            .fold(0usize, usize::saturating_add);
        let mut out = Vec::with_capacity(size);
        out.extend_from_slice(b"YSTB");
        out.extend_from_slice(&version.to_le_bytes());
        out.extend_from_slice(&self.count.to_le_bytes());
        out.extend_from_slice(&(self.inst.len() as u32).to_le_bytes());
        out.extend_from_slice(&(self.args_index.len() as u32).to_le_bytes());
        out.extend_from_slice(&(self.args_data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(self.line_numbers.len() as u32).to_le_bytes());
        out.extend_from_slice(&self.tail_word.to_le_bytes());
        for seg in [
            &self.inst,
            &self.args_index,
            &self.args_data,
            &self.line_numbers,
        ] {
            let mut copy = seg.clone();
            xor_segment(&mut copy, key);
            out.extend_from_slice(&copy);
        }
        out.extend_from_slice(&self.trailing);
        out
    }
}

/// 从解密后的区段按契约切片,并用 `table` 把指令序号翻成命令名。
///
/// `codec` 用于解码标签引用文本(GO/GOSUB 的 `#=` 实参);`origin` 用于
/// 诊断定位。任何序号越界(≥ 表条数)= `fmt.ybn.opcode` Fatal。
pub fn decode(
    segs: &Segments,
    script_id: u32,
    table: &CommandTable,
    codec: Codec,
    origin: &[u8],
) -> Result<DecodedScript, Diag> {
    let count = usize::try_from(segs.count).map_err(|_| limit(origin, 8, "count"))?;
    let inst_bytes = count
        .checked_mul(4)
        .ok_or_else(|| limit(origin, 8, "count × 4"))?;
    if inst_bytes > segs.inst.len() {
        return Err(Diag::fatal(code::COUNT, Boundary::Format)
            .at_data(origin, 8)
            .with_detail(format!(
                "头 count={count} 需要 {inst_bytes} 字节指令区,实际 {} 字节",
                segs.inst.len()
            )));
    }
    let mut groups = Vec::with_capacity(count.min(4096));
    let mut labels = LabelTable::default();
    let mut arg_at = 0usize;
    for index in 0..count {
        let base = index * 4;
        let opcode = segs.inst[base];
        let argc = usize::from(segs.inst[base + 1]);
        let name = table.name_of(u16::from(opcode)).ok_or_else(|| {
            Diag::fatal(code::YBN_OPCODE, Boundary::Format)
                .at_data(origin, index as u64)
                .with_detail(format!(
                    "指令 {index} 的序号 {opcode} 超出 ysc 表(共 {} 条)",
                    table.len()
                ))
        })?;
        let need = argc
            .checked_mul(12)
            .and_then(|n| arg_at.checked_add(n))
            .ok_or_else(|| limit(origin, arg_at as u64, "参数窗口累计"))?;
        if need > segs.args_index.len() {
            return Err(Diag::fatal(code::YBN_ARGS, Boundary::Format)
                .at_data(origin, arg_at as u64)
                .with_detail(format!(
                    "指令 {index}({name})的参数窗口越界:索引区共 {} 字节",
                    segs.args_index.len()
                )));
        }
        let mut windows = Vec::with_capacity(argc.min(4096));
        for _ in 0..argc {
            let mut bytes = [0u8; 12];
            bytes.copy_from_slice(&segs.args_index[arg_at..arg_at + 12]);
            windows.push(Window { bytes });
            arg_at += 12;
        }
        collect_label_refs(
            table,
            name,
            &windows,
            u32::try_from(index).unwrap_or(u32::MAX),
            &segs.args_data,
            codec,
            origin,
            &mut labels,
        )?;
        groups.push(Group {
            command: Cow::Owned(name.to_string()),
            windows,
        });
    }
    if arg_at != segs.args_index.len() {
        return Err(Diag::fatal(code::YBN_ARGS, Boundary::Format)
            .at_data(origin, arg_at as u64)
            .with_detail(format!(
                "参数索引区剩余 {0} 字节未被任何指令消费",
                segs.args_index.len() - arg_at
            )));
    }
    Ok(DecodedScript {
        script_id,
        groups,
        labels,
    })
}

/// 数据驱动的标签引用收集:凡 ysc 表里名为 `#` 的参数位、且窗口载荷是
/// `M` 形引号串,即为静态标签引用(`#=名字` 的编译形态);其余载荷形态
/// (`H`/`V`/数值等)= 运行期动态目标,不进引用表,解释归运行核。
///
/// # 按窗口 slot id 索引(不是发射位置)
///
/// 参数窗口的第 1 个 `u16` 是 ysc 参数表下标(slot),不是它在指令里的
/// 出现位置。证据(全部为真实字节):
/// - ERIS `yst00008.ybn` 第 232 条 TASK 的 4 个窗口 id = `0/34/3/4`;
///   ysc `TASK` 参数表 38 项,第 34 项 = `#`;按发射位置(第 2 个窗口)
///   会误配到参数名 `E`,于是 `es.SS.FADE.TASK.MAIN` 丢失。
/// - ERIS `yst00019/20/70` 的 TASK 窗口 id 落在 34/35/1/3..7,
///   全部 < 该命令 argc;全语料标签命令(GO/GOSUB/LABEL/LABELINFO/TASK)
///   的窗口 id 无一 ≥ 对应参数表长度。
#[allow(clippy::too_many_arguments)]
fn collect_label_refs(
    table: &CommandTable,
    name: &str,
    windows: &[Window],
    group_index: u32,
    args_data: &[u8],
    codec: Codec,
    origin: &[u8],
    labels: &mut LabelTable,
) -> Result<(), Diag> {
    let Some(index) = table.index_of(name) else {
        return Ok(());
    };
    let Some(command) = table.commands.get(usize::from(index)) else {
        return Ok(());
    };
    for window in windows {
        let slot = usize::from(u16::from(window.bytes[0]) | u16::from(window.bytes[1]) << 8);
        let Some(arg) = command.args.get(slot) else {
            continue;
        };
        if arg.name.as_ref() != "#" {
            continue;
        }
        let size = window_u32(window, 4) as usize;
        let offset = window_u32(window, 8) as usize;
        let Some(end) = offset.checked_add(size) else {
            continue;
        };
        if size < 5 || end > args_data.len() || args_data[offset] != b'M' {
            continue;
        }
        let len = usize::from(u16::from(args_data[offset + 1]) | u16::from(args_data[offset + 2]) << 8);
        let start = offset + 3;
        let Some(end) = start.checked_add(len) else {
            continue;
        };
        if end > args_data.len()
            || len < 2
            || args_data[start] != b'"'
            || args_data[end - 1] != b'"'
        {
            continue;
        }
        let inner = &args_data[start + 1..end - 1];
        let label = codec.decode(inner, origin)?;
        labels.entries.push(LabelRef {
            name: label,
            group: group_index,
        });
    }
    Ok(())
}

/// 窗口内小端 u32(参数记录布局:u16 id, u16 type, u32 长, u32 偏移)。
fn window_u32(window: &Window, at: usize) -> u32 {
    u32::from(window.bytes[at])
        | u32::from(window.bytes[at + 1]) << 8
        | u32::from(window.bytes[at + 2]) << 16
        | u32::from(window.bytes[at + 3]) << 24
}

/// `base + len` 的 32 位/64 位可移植 checked 版本(usize 在 32 位目标可能回绕)。
fn checked_at(base: usize, len: usize, origin: &[u8]) -> Result<usize, Diag> {
    base.checked_add(len)
        .ok_or_else(|| limit(origin, base as u64, "区段边界"))
}

/// 计数/尺寸超出可表示范围 = `fmt.limit`(不 panic,不静默截断)。
fn limit(origin: &[u8], at: u64, what: &str) -> Diag {
    Diag::fatal(code::LIMIT, Boundary::Format)
        .at_data(origin, at)
        .with_detail(format!("{what} 超出可表示范围"))
}

/// 结构自检失败时的诊断:优先按错误密钥解释(`fmt.ybn.key`)。
fn key_suspect(origin: &[u8], detail: String) -> Diag {
    Diag::fatal(code::YBN_KEY, Boundary::Format)
        .at_data(origin, 0)
        .with_detail(format!("{detail}(结构自检失败,疑似密钥不符)"))
}
