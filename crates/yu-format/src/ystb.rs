//! `yst%05d.ybn`(魔数 `YSTB`)· 脚本体解码 → 契约类型 [`DecodedScript`]。
//!
//! 两套布局,由**版本带**判别(V5 路径保持不变;V2 见下):
//!
//! - **V5**(`version >= 300`):四区段视图;
//! - **V2**(`200 < version < 300`):`code`/`expr` 两段,指令头 6 字节内联。
//!   V2 解码后**合成**一份与 V5 同形的四区段视图(`inst`/`args_index`/
//!   `args_data`/`line_numbers`)供运行核直接消费,同时保留原始两段供
//!   逐字节回写。证据:官方 0.292 `yu-ris.exe`(sha256
//!   `72628dfa…`)装载器 `FUN_00437da9` 的走查循环(6 字节头、
//!   `argc × 0xC`、RETURNCODE 定长 0x38→+10)与两套官方语料全量闭合
//!   (`docs/divergences/DIV-007`、`DIV-009`)。
//!
//! # V2 布局(官方 0.292 装载器 + 语料实证)
//!
//! ```text
//! +0x00 char[4] "YSTB"
//! +0x04 u32     版本(255/292)
//! +0x08 u32     code_size
//! +0x0C u32     expr_size
//! +0x10 u32     expr_offset == 0x20 + code_size
//! +0x14 u32[3]  padding(观测 0;原样保留)
//! +0x20 code[code_size]   ← XOR,密钥从下标 0 重新开始
//! +...  expr[expr_size]   ← XOR,密钥从下标 0 重新开始
//! code 段逐条:u8 op(ysc 表下标)、u8 argc、u32 line;
//!   RETURNCODE(按 ysc 名识别):随后 4 字节(version==290 时 8 字节);
//!   其它:argc × 12 字节 {u16 id, u8 typ, u8 aop, u32 expr_size, u32 expr_offset}
//! ```
//!
//! 关键语义(两套语料实测):
//! - `IF/ELSE/LOOP` 的第 2/3 个参数 `expr_size` 是 **code 段字节偏移**
//!   (不是指令下标);0.292 的 2,590 个、0.255 的 2,279 个目标全部落在
//!   指令起始地址。合成视图时转换为组下标写入 `length`。
//! - `RETURNCODE` 的 4 字节是**本脚本内文本行计数**(1 起逐行 +1);
//!   opcode 随 ysc 表变化(0.292=0x38、0.255=0x36),**禁止硬编码**。
//! - `WORD` 的 expr 数据是裸 CP932 字符串,不是表达式;`_` 仍是表达式。
//!
//! V5 布局(ERIS 语料 136 文件全量实证;参考 RxYuris YSTB.cpp `Xor`/`YSTB_V5`
//! 与 YU-RIS-Script-Editor `core/ystb.py` `_parse_v5` 的解析流程):
//!
//! ```text
//! +0x00 char[4] "YSTB"
//! +0x04 u32     版本(495;200 < v < 300 = V2,走上文两段布局)
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

/// YSTB 的 V2 版本带(200 < version < 300)。
const V2_MIN: u32 = 200;
const V2_MAX: u32 = 300;

/// V2 原始段(回写用;`inst`/`args_index`/… 是它的合成视图)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct V2Raw {
    /// 已解密的 code 段(指令头内联)。
    pub code: Vec<u8>,
    /// 已解密的 expr 段。
    pub expr: Vec<u8>,
    /// 头 `+0x14/+0x18/+0x1C` 三个字(观测 0;原样保留)。
    pub padding: [u32; 3],
}

/// 段布局判别。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Layout {
    /// V5(300+):磁盘四段与 [`Segments`] 字段一一对应。
    V5,
    /// V2(200 < version < 300):两段 + 合成视图。
    V2(V2Raw),
}

impl Layout {
    /// 是否为 V2(运行核据此决定标签 `field_b` 的偏移语义)。
    #[must_use]
    pub fn is_v2(&self) -> bool {
        matches!(self, Layout::V2(_))
    }
}

/// 解密后的四区段(低层视图;`DecodedScript` 之外的原始材料)。
///
/// V2 下四个区段是**合成视图**:`inst` 每指令 4 字节 `[op, argc, 0, 0]`,
/// `args_index` 为指令窗口(控制流目标已换算成组下标),`args_data` =
/// 原始 expr,`line_numbers` = 每条指令内联行号。原始 code/expr 在
/// [`Layout::V2`] 里保留,供 [`Segments::encrypt`] 逐字节回写。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segments {
    /// 容器头版本(`+0x04`;R12 起 yu-rt 适配层需要)。
    pub version: u32,
    /// 段布局(V5 原生 / V2 合成)。
    pub layout: Layout,
    /// 头部 `+0x08` 指令条数(权威;引擎 `FUN_004518e9` 以它为循环上界)。
    pub count: u32,
    /// 指令索引区(count × 4 字节)。
    pub inst: Vec<u8>,
    /// 参数索引区(12 字节窗口序列)。
    pub args_index: Vec<u8>,
    /// 参数数据区(窗口载荷宿主)。
    pub args_data: Vec<u8>,
    /// 行号区(count × 4 字节,原样保留;V2 = 每指令内联行号)。
    pub line_numbers: Vec<u8>,
    /// 头部 `+0x1C` 字段(观测为 0;原样保留)。
    pub tail_word: u32,
    /// 四段之后的尾部字节(原样保留;回封时逐字节写回)。
    pub trailing: Vec<u8>,
    /// V2 的指令起始 code 偏移(升序;下标即组下标)。V5 = 空。
    pub code_offsets: Vec<u32>,
}

impl Segments {
    /// 解密(或加密——异或是对合)`yst%05d.ybn` 的 V5 四区段。
    ///
    /// 头部明文不动;`key` 传 [`crate::key::DEFAULT_KEY_BYTES`] 之外的值
    /// 时,调用方自负其责(换键作品须先取回密钥,本层不猜)。
    ///
    /// **V2 需要命令表**(RETURNCODE 的 opcode 随 ysc 表变化,禁止硬编码);
    /// V2 文件请走 [`Self::decrypt_with_table`],本函数对 V2 显式返回
    /// `fmt.ybn.unsupported`(保留"DIV-009:V5 解码器拒绝 V2"的既有语义)。
    pub fn decrypt(data: &[u8], key: &[u8; 4], origin: &[u8]) -> Result<Self, Diag> {
        let version = ystb_version(data, origin)?;
        if is_v2(version) {
            return Err(Diag::unimplemented(code::YBN_UNSUPPORTED, Boundary::Format)
                .at_data(origin, 4)
                .with_detail(format!(
                    "YSTB V2(版本 {version})布局与 V5 不同;按 ysc 命令名识别 \
                     RETURNCODE 需要命令表,请用 `Segments::decrypt_with_table`\
                     (见 divergences/DIV-007)"
                )));
        }
        Self::decrypt_v5(data, key, origin)
    }

    /// 解密 V5 或 V2(`table` 供 V2 按名识别 `RETURNCODE`;V5 不使用)。
    ///
    /// V2 解码后合成与 V5 同形的四区段视图,运行核只消费这一份视图。
    pub fn decrypt_with_table(
        data: &[u8],
        key: &[u8; 4],
        table: &CommandTable,
        origin: &[u8],
    ) -> Result<Self, Diag> {
        let version = ystb_version(data, origin)?;
        if is_v2(version) {
            Self::decrypt_v2(data, key, table, origin)
        } else {
            Self::decrypt_v5(data, key, origin)
        }
    }

    fn decrypt_v5(data: &[u8], key: &[u8; 4], origin: &[u8]) -> Result<Self, Diag> {
        let version = ystb_version(data, origin)?;
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
            layout: Layout::V5,
            count,
            inst,
            args_index,
            args_data,
            line_numbers,
            tail_word: u32_at(data, 0x1C),
            trailing: data[end..].to_vec(),
            code_offsets: Vec::new(),
        };
        segs.self_check(origin)?;
        Ok(segs)
    }

    /// V2:解密两段 → 走查 code → 合成 V5 视图。
    ///
    /// 结构规则与证据见模块文档;任何不自洽(段越界、指令不闭合、opcode
    /// 超表、窗口越界、控制目标不在指令起点)都显式失败,不产出"看似
    /// 能跑"的错位视图。
    fn decrypt_v2(
        data: &[u8],
        key: &[u8; 4],
        table: &CommandTable,
        origin: &[u8],
    ) -> Result<Self, Diag> {
        let version = ystb_version(data, origin)?;
        let (code_size, expr_size, expr_offset, padding) = v2_header(data, origin)?;
        let code_end = checked_at(0x20, code_size, origin)?;
        let expr_end = checked_at(expr_offset, expr_size, origin)?;
        if expr_end > data.len() {
            return Err(truncated(origin, expr_end, data.len(), "YSTB V2 expr 段"));
        }
        let mut code = data[0x20..code_end].to_vec();
        let mut expr = data[expr_offset..expr_end].to_vec();
        xor_segment(&mut code, key);
        xor_segment(&mut expr, key);
        let walk = walk_v2(&code, version, table, expr_size, origin)?;
        let mut inst = Vec::with_capacity(walk.len() * 4);
        let mut args_index =
            Vec::with_capacity(walk.iter().map(|i| usize::from(i.argc)).sum::<usize>() * 12);
        let mut line_numbers = Vec::with_capacity(walk.len() * 4);
        let control = control_ops(table);
        let rc_op = returncode_op(table, origin)?;
        for (pc, item) in walk.iter().enumerate() {
            inst.extend_from_slice(&[item.op, item.argc, 0, 0]);
            line_numbers.extend_from_slice(&item.line.to_le_bytes());
            if item.op == rc_op {
                for slot in 0..usize::from(item.argc) {
                    let mut w = [0u8; 12];
                    if slot == 0 {
                        // 4 字节行计数 → kind/flag(低 16 位)+ slot(高 16 位);
                        // 0.292 语料实测 107 个不同值,1 起逐行 +1。
                        let mut line = [0u8; 4];
                        line.copy_from_slice(&code[item.args_at..item.args_at + 4]);
                        w[0..2].copy_from_slice(&line[0..2]);
                        w[2..4].copy_from_slice(&line[2..4]);
                        if item.args_len >= 8 {
                            // version 290:第二个 dword 保留进 length(语义未取证)。
                            w[4..8].copy_from_slice(&code[item.args_at + 4..item.args_at + 8]);
                        }
                    }
                    args_index.extend_from_slice(&w);
                }
                continue;
            }
            for slot in 0..usize::from(item.argc) {
                let at = item.args_at + slot * 12;
                let mut w = [0u8; 12];
                w.copy_from_slice(&code[at..at + 12]);
                if control.contains(&item.op) && slot > 0 {
                    let target = u32::from_le_bytes([w[4], w[5], w[6], w[7]]);
                    if target != 0 {
                        let target_pc = walk
                            .binary_search_by_key(&target, |i| i.head)
                            .map_err(|_| {
                                Diag::fatal(code::YBN_LAYOUT, Boundary::Format)
                                    .at_data(origin, at as u64)
                                    .with_detail(format!(
                                        "V2 控制流目标 {target:#x} 不是指令起始地址\
                                         (指令 {pc} 的窗口 {slot})"
                                    ))
                            })?;
                        // length = 目标组下标;type/aop 置 0:VM 不消费目标窗口
                        // 的载荷,自检也不会把 pc 当载荷长度。
                        w[2..4].copy_from_slice(&[0, 0]);
                        w[4..8].copy_from_slice(&(target_pc as u32).to_le_bytes());
                    }
                }
                args_index.extend_from_slice(&w);
            }
        }
        let code_offsets = walk.iter().map(|i| i.head).collect();
        let segs = Self {
            version,
            layout: Layout::V2(V2Raw {
                code,
                expr: expr.clone(),
                padding,
            }),
            count: u32::try_from(walk.len()).unwrap_or(u32::MAX),
            inst,
            args_index,
            args_data: expr,
            line_numbers,
            tail_word: 0,
            trailing: data[expr_end..].to_vec(),
            code_offsets,
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
    /// V5:头 count、四段长度、`+0x1C` 与尾部字节全部按原值写回。
    /// V2:按原始 code/expr/padding 与**原始头版本**写回(`version` 参数
    /// 被忽略——V2 的 RETURNCODE 宽度由头版本决定,改写版本会破坏字节;
    /// 文本改写暂不承诺,见 DIV-007)。
    #[must_use]
    pub fn encrypt(&self, version: u32, key: &[u8; 4]) -> Vec<u8> {
        if let Layout::V2(raw) = &self.layout {
            return encrypt_v2(self, raw, key);
        }
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

/// V2 回写:头 + code + expr + 尾部(两段各自从密钥下标 0 异或)。
fn encrypt_v2(segs: &Segments, raw: &V2Raw, key: &[u8; 4]) -> Vec<u8> {
    let mut out = Vec::with_capacity(0x20 + raw.code.len() + raw.expr.len() + segs.trailing.len());
    out.extend_from_slice(b"YSTB");
    out.extend_from_slice(&segs.version.to_le_bytes());
    out.extend_from_slice(&(raw.code.len() as u32).to_le_bytes());
    out.extend_from_slice(&(raw.expr.len() as u32).to_le_bytes());
    out.extend_from_slice(&((0x20 + raw.code.len()) as u32).to_le_bytes());
    for word in raw.padding {
        out.extend_from_slice(&word.to_le_bytes());
    }
    for seg in [&raw.code, &raw.expr] {
        let mut copy = seg.clone();
        xor_segment(&mut copy, key);
        out.extend_from_slice(&copy);
    }
    out.extend_from_slice(&segs.trailing);
    out
}

/// `YSTB` 头版本(魔数不符 = `fmt.magic`)。
fn ystb_version(data: &[u8], origin: &[u8]) -> Result<u32, Diag> {
    if data.len() < 0x20 || &data[0..4] != b"YSTB" {
        return Err(Diag::fatal(code::MAGIC, Boundary::Format)
            .at_data(origin, 0)
            .with_detail(format!(
                "魔数不符:期望 YSTB,实际 {:?}",
                String::from_utf8_lossy(data.get(0..4).unwrap_or(b""))
            )));
    }
    Ok(u32_at(data, 4))
}

/// 版本带 = V2?(200 < version < 300)
#[must_use]
pub fn is_v2(version: u32) -> bool {
    V2_MIN < version && version < V2_MAX
}

/// V2 头字段:`(code_size, expr_size, expr_offset, padding)`。
fn v2_header(data: &[u8], origin: &[u8]) -> Result<(usize, usize, usize, [u32; 3]), Diag> {
    let code_size = u32_at(data, 8) as usize;
    let expr_size = u32_at(data, 0x0C) as usize;
    let expr_offset = u32_at(data, 0x10) as usize;
    let expected = checked_at(0x20, code_size, origin)?;
    if expected != expr_offset {
        return Err(Diag::fatal(code::YBN_LAYOUT, Boundary::Format)
            .at_data(origin, 0x10)
            .with_detail(format!(
                "V2 头不闭合:expr_offset {expr_offset:#x} != 0x20 + code_size {expected:#x}"
            )));
    }
    Ok((
        code_size,
        expr_size,
        expr_offset,
        [u32_at(data, 0x14), u32_at(data, 0x18), u32_at(data, 0x1C)],
    ))
}

/// V2 一条指令的走查结果(只记位置,窗口按需从 `code` 读)。
struct V2Inst {
    /// 指令起始 code 偏移。
    head: u32,
    op: u8,
    argc: u8,
    line: u32,
    /// 参数载荷起始(`RETURNCODE` = 4/8 字节值;其它 = `argc × 12` 窗口)。
    args_at: usize,
    /// `RETURNCODE` 的定长载荷字节数;其它 = 0。
    args_len: usize,
}

/// 走查 V2 code 段:每条指令头 6 字节;RETURNCODE 定长,其它 `argc × 12`。
///
/// 失败语义:段内截断/opcode 超表 = 结构错误;普通窗口 `offset+size`
/// 越界按错误密钥解释(`fmt.ybn.key`,与 V5 自检同口径)。
fn walk_v2(
    code: &[u8],
    version: u32,
    table: &CommandTable,
    expr_size: usize,
    origin: &[u8],
) -> Result<Vec<V2Inst>, Diag> {
    let rc = returncode_op(table, origin)?;
    let mut out = Vec::new();
    let mut at = 0usize;
    while at < code.len() {
        let head = u32::try_from(at).unwrap_or(u32::MAX);
        if code.len() - at < 6 {
            return Err(Diag::fatal(code::TRUNCATED, Boundary::Format)
                .at_data(origin, at as u64)
                .with_detail(format!(
                    "V2 指令头截断:偏移 {at},剩 {} 字节",
                    code.len() - at
                )));
        }
        let op = code[at];
        let argc = code[at + 1];
        let line = u32::from_le_bytes([code[at + 2], code[at + 3], code[at + 4], code[at + 5]]);
        if usize::from(op) >= table.commands.len() {
            return Err(Diag::fatal(code::YBN_OPCODE, Boundary::Format)
                .at_data(origin, head as u64)
                .with_detail(format!(
                    "V2 指令的序号 {op} 超出 ysc 表(共 {} 条)",
                    table.commands.len()
                )));
        }
        at += 6;
        if op == rc {
            let width = if version == 290 { 8 } else { 4 };
            let end = checked_at(at, width, origin)?;
            if end > code.len() {
                return Err(Diag::fatal(code::TRUNCATED, Boundary::Format)
                    .at_data(origin, at as u64)
                    .with_detail(format!(
                        "V2 RETURNCODE 载荷截断:偏移 {at} 需要 {width} 字节,剩 {}",
                        code.len() - at
                    )));
            }
            out.push(V2Inst {
                head,
                op,
                argc,
                line,
                args_at: at,
                args_len: width,
            });
            at = end;
            continue;
        }
        let need = usize::from(argc)
            .checked_mul(12)
            .ok_or_else(|| limit(origin, at as u64, "V2 argc × 12"))?;
        let end = checked_at(at, need, origin)?;
        if end > code.len() {
            return Err(Diag::fatal(code::TRUNCATED, Boundary::Format)
                .at_data(origin, at as u64)
                .with_detail(format!(
                    "V2 参数窗口截断:指令偏移 {head},argc {argc},剩 {} 字节",
                    code.len() - at
                )));
        }
        for slot in 0..usize::from(argc) {
            let w = at + slot * 12;
            let typ = code[w + 2];
            let size = u32::from_le_bytes([code[w + 4], code[w + 5], code[w + 6], code[w + 7]])
                as usize;
            let offset =
                u32::from_le_bytes([code[w + 8], code[w + 9], code[w + 10], code[w + 11]])
                    as usize;
            if typ != 0 && size > 0 {
                let fits = offset
                    .checked_add(size)
                    .is_some_and(|v| v <= expr_size);
                if !fits {
                    return Err(key_suspect(
                        origin,
                        format!(
                            "V2 窗口(指令偏移 {head},槽 {slot})offset {offset} + size {size} \
                             超出 expr 段 {expr_size} 字节"
                        ),
                    ));
                }
            }
        }
        out.push(V2Inst {
            head,
            op,
            argc,
            line,
            args_at: at,
            args_len: 0,
        });
        at = end;
    }
    Ok(out)
}

/// V2 控制流命令(按 ysc 名;第 2/3 参数的 `expr_size` 是 code 偏移)。
fn control_ops(table: &CommandTable) -> Vec<u8> {
    ["IF", "ELSE", "LOOP"]
        .iter()
        .filter_map(|name| table.index_of(name))
        .filter_map(|index| u8::try_from(index).ok())
        .collect()
}

/// `RETURNCODE` 的 opcode(按 ysc 名;缺失 = 工程表不完整)。
fn returncode_op(table: &CommandTable, origin: &[u8]) -> Result<u8, Diag> {
    table
        .index_of("RETURNCODE")
        .and_then(|index| u8::try_from(index).ok())
        .ok_or_else(|| {
            Diag::fatal(code::YBN_OPCODE, Boundary::Format)
                .at_data(origin, 0)
                .with_detail("V2 解码需要 ysc 表含 `RETURNCODE`(按名识别,禁止硬编码)")
        })
}

/// V2 只走指令头建立 `code 偏移 → 指令下标` 映射(标签表转换用)。
///
/// 只异或 code 段,不分配窗口/表达式;返回升序偏移表(下标 = 组下标)。
pub fn v2_code_offsets(
    data: &[u8],
    key: &[u8; 4],
    table: &CommandTable,
    origin: &[u8],
) -> Result<Vec<u32>, Diag> {
    let version = ystb_version(data, origin)?;
    if !is_v2(version) {
        return Err(Diag::fatal(code::YBN_LAYOUT, Boundary::Format)
            .at_data(origin, 4)
            .with_detail(format!(
                "v2_code_offsets 收到非 V2 版本 {version}(V5 的标签已是指令下标)"
            )));
    }
    let (code_size, expr_size, expr_offset, _) = v2_header(data, origin)?;
    let code_end = checked_at(0x20, code_size, origin)?;
    let expr_end = checked_at(expr_offset, expr_size, origin)?;
    if expr_end > data.len() {
        return Err(truncated(origin, expr_end, data.len(), "YSTB V2 expr 段"));
    }
    let mut code = data[0x20..code_end].to_vec();
    xor_segment(&mut code, key);
    Ok(walk_v2(&code, version, table, expr_size, origin)?
        .iter()
        .map(|i| i.head)
        .collect())
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
    // 命令名 → 序号:**首次出现为准**,与 `CommandTable::index_of` 的
    // `position()` 逐项等价。`decode` 每条指令都要按名回查声明,119 项
    // 线性扫描在 10 万级指令上是千万次字符串比较;这里按借用建一次表,
    // 键零字符串分配(仅工具/装载路径,热解释器不经过这里)。
    let mut by_name: std::collections::HashMap<&str, u16> =
        std::collections::HashMap::with_capacity(table.commands.len());
    for (index, command) in table.commands.iter().enumerate() {
        if let Ok(index) = u16::try_from(index) {
            by_name.entry(command.name.as_ref()).or_insert(index);
        }
    }
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
            &by_name,
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
    by_name: &std::collections::HashMap<&str, u16>,
    table: &CommandTable,
    name: &str,
    windows: &[Window],
    group_index: u32,
    args_data: &[u8],
    codec: Codec,
    origin: &[u8],
    labels: &mut LabelTable,
) -> Result<(), Diag> {
    let Some(index) = by_name.get(name).copied() else {
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
