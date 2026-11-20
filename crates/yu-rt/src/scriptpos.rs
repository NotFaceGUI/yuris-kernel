//! `SCRIPTPOS` 执行位快照:与官方引擎**线格式逐字节兼容**的编解码。
//!
//! # 线格式分代(2026-11-20 以官方存档逐字节定案)
//!
//! 两代共有的骨架(帧宽跨版本一致,各版装载器推进量相同):
//!
//! ```text
//! [magic 8B "01..08"]
//! [if 计数 2 字符][IF 帧 ×18 字符:loop 深度 u8、entry pc u32、end pc u32]
//! [GOSUB 计数][GOSUB 帧 ×20 字符:IF 深度 u8、loop 深度 u8、返回 pc u32、
//!              返回脚本号 u32(装载器只取低 16 位)]
//! [LOOP 计数 2 字符][LOOP 帧 ×34 字符:IF 深度 u8、body pc u32、end pc u32、
//!                    counter 低 32 位、limit 低 32 位(装载器符号扩展)]
//! [0.47x 代专属:尾部 u8 2 字符][text_line u32][pc u32][name_len u32][名字原始字节]
//! ```
//!
//! **两代差异**(GOSUB 计数宽度 + 尾部 u8):
//!
//! | 代 | GOSUB 计数 | 尾部 u8 | 覆盖版本 | 官方证据 |
//! |---|---|---|---|---|
//! | [`WIRE_GOSUB_U16`] | **u16**(4 字符,高字节在前) | **有** | 0.450–0.479(0.13)(容器 `0x0C0`/`0x138`) | 0.450 `0x440515`、0.466 `0x4420bd`、0.470 `0x4428e9`、0.474 `0x448f69`、0.479 `0x449f3d/0x449f58` 均「两组 2 字符计数 + `add ecx/ebp,4`」,随后 `0x44a355` 读尾部 u8;官方 0.479 存档 2 份逐字节验证 |
//! | [`WIRE_GOSUB_U8`] | u8(2 字符) | **无** | 0.480–0.495 / v494(容器 `0x1E0` 系) | 0.480 `0x44fa3a` 一带、0.495 `0x4511cc`(`add ebp,2`)、v494 同构;官方 0.495 存档(`data/save/save00001.sd`)按「u8 + 3×u32 + 名」精确消耗 |
//!
//! 装载器地址(桌面 exe,`imagebase 0x400000`):
//!
//! | 版本 | 序列化器 | 装载器 |
//! |---|---|---|
//! | 0.292 | `0x438b2c` | `0x437548` |
//! | 0.466 | `0x4438d0` | `0x441ed4` |
//! | 0.479 | `0x44b718` | `0x449d7c` |
//! | 0.495 | `0x452a54` | `0x450fe0` |
//! | v494 | `0x4528c8` | `0x450e74` |
//!
//! # 为什么分代必须显式
//!
//! 旧内核只实现了「u8 计数 + 无尾部」:对 0.495 恰好正确,但写 0.479
//! 工程时官方读档会在脚本名处错位 4 字符(报「`2\script\...` が
//! 存在しません」—— `kuro` 被吃掉),反向读 0.479 官方档同样错位。
//! 版本→格式的**唯一表**在 `yu_save::profile`(`SaveProfile::scriptpos`),
//! 由宿主在开机时注入 [`crate::vm::Vm::set_scriptpos_wire`]。
//!
//! # 字符编码
//!
//! 除 8 字节魔数与末尾脚本名外,所有数值都按**字节**编码为 2 个字符:
//! 高半字节 `'a' + nibble`、低半字节 `'a' + nibble`;多字节整数
//! **高字节在前**(big-endian)。十六进制字符从 `'a'` 起,不是 `'0'`/`'A'`。
//!
//! 字符串变量按长度存储(可含魔数里的 `0x01..0x08` 与任意脚本名字节),
//! 因此本模块返回原始 `Vec<u8>`,不假设 UTF-8。

/// 引擎魔数(`0x5b4090`)。
pub const MAGIC: [u8; 8] = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];

/// 装载器对脚本名的上限(`0x451749`:`cmp edi,0x106`)。
pub const MAX_SCRIPT_NAME: usize = 0x106;

/// IF 块帧(引擎 18 字符/帧)。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IfEntry {
    /// `+0`:进入 IF 时的 LOOP 深度。
    pub loop_depth: u8,
    /// `+4`:IF 命令自身 pc。
    pub entry_pc: u32,
    /// `+8`:块出口(IFEND/ELSE)pc。
    pub end_pc: u32,
}

/// GOSUB 调用帧(线上 20 字符/项;运行期字段 `+8` 为 u16)。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GosubEntry {
    /// `+0`:调用点的 IF 深度(= 引擎变量域基址 A)。
    pub if_depth: u8,
    /// `+1`:调用点的 LOOP 深度。
    pub loop_depth: u8,
    /// `+4`:返回 pc。
    pub return_pc: u32,
    /// `+8`:返回脚本号。线上按 4 字节写出(高 16 位恒 0),装载只取低 16 位。
    pub return_script: u16,
}

/// LOOP 帧(引擎 34 字符/项)。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LoopEntry {
    /// `+0`:进入 LOOP 时的 IF 深度(= 引擎变量域基址 A)。
    pub if_depth: u8,
    /// `+4`:循环体首 pc。
    pub body_pc: u32,
    /// `+8`:LOOPEND pc。
    pub end_pc: u32,
    /// `+0x10`:已执行轮数(引擎只序列化低 32 位,装载时符号扩展)。
    pub counter: i64,
    /// `+0x18`:目标轮数(同上;`-1` = 无限)。
    pub limit: i64,
}

/// SCRIPTPOS 线格式代数(两代只有 GOSUB 计数宽度不同)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScriptPosWire {
    /// GOSUB 帧计数的线上宽度。
    pub gosub_count: GosubCount,
    /// `text_line` 之前是否有尾部 u8(2 字符)。全部已取证版本都有;
    /// 保留开关以便将来遇到缺此字节的极老版本。
    pub tail_byte: bool,
}

/// GOSUB 帧计数的线上宽度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GosubCount {
    /// 2 字符(1 字节):0.480 起(0.480 `0x44fa3a` 一带 `add ebp,2`)、
    /// 0.495 `0x4511cc`、v494 同构。
    U8,
    /// 4 字符(2 字节,u16 高字节在前):0.450 `0x440515`、0.466
    /// `0x4420bd`、0.470 `0x4428e9`、0.474 `0x448f69`、0.479
    /// `0x449f3d/0x449f58` 均为「读两组 2 字符 + `add ecx/ebp,4`」。
    U16,
}

/// 0.450–0.479(0.13)(容器 `0x0C0`/`0x138`):u16 GOSUB 计数 + 尾部 u8。
pub const WIRE_GOSUB_U16: ScriptPosWire = ScriptPosWire {
    gosub_count: GosubCount::U16,
    tail_byte: true,
};

/// 0.480–0.495 / v494(容器 `0x1E0` 系):u8 GOSUB 计数、**无**尾部 u8。
///
/// 证据:0.495 官方存档(`target/sdk/0.495/.../data/save/save00001.sd`,
/// 容器 `0x1E0`)的 DNO=11 任务块按「u8 计数 + 3×u32 + 名」恰好精确
/// 消耗;加尾部 u8 会读出错误 name_len。旧内核的线格式本就匹配这一代。
pub const WIRE_GOSUB_U8: ScriptPosWire = ScriptPosWire {
    gosub_count: GosubCount::U8,
    tail_byte: false,
};

/// 一个任务的完整执行位快照。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScriptPos {
    pub if_entries: Vec<IfEntry>,
    pub gosub_entries: Vec<GosubEntry>,
    pub loop_entries: Vec<LoopEntry>,
    /// 引擎 `task+0xc`(RETURNCODE 写入的打包文本行)。
    pub text_line: u32,
    /// 引擎 `task+0x20`(下一条 pc)。
    pub pc: u32,
    /// 引擎 `0x801850[task+0x3c]` 的脚本路径原始字节(CP932)。
    pub script_name: Vec<u8>,
    /// 线上 `text_line` 之前的尾部 u8(2 字符)。
    ///
    /// 0.495 装载器 `0x451380` 先读 2 字符存入 `[eax]`,0.479 装载器
    /// `0x44a369` 存入 `+0xa70`;**两版都有**,官方样本该值恒 0。语义
    /// 未取证(旧内核漏写这一字节,导致写出的档官方读取时整体错位 4 字符,
    /// 脚本名首 4 字节被吃掉 —— 「417」)。
    pub tail_byte: u8,
}

impl ScriptPos {
    /// 按引擎 `0x452a54` 的布局编码(0.495/u8 GOSUB 计数,旧默认)。
    pub fn encode(&self) -> Result<Vec<u8>, String> {
        self.encode_with(WIRE_GOSUB_U8)
    }

    /// 按目标写盘档案的线格式编码。
    ///
    /// 版本→线格式的**唯一表**在 `yu_save::profile`(取值即本模块的
    /// [`WIRE_GOSUB_U16`] / [`WIRE_GOSUB_U8`])。
    ///
    /// 两代共有:magic(8B)→if 计数(2 字符)→IF 帧(18 字符)→ GOSUB 计数
    /// →GOSUB 帧(20 字符)→LOOP 计数(2 字符)→LOOP 帧(34 字符)→
    /// 尾部 u8(2 字符)→`text_line`/`pc`/`name_len`(各 4 字节)→脚本名。
    #[must_use]
    pub fn encode_with_wire(&self, wire: ScriptPosWire) -> Result<Vec<u8>, String> {
        self.encode_with(wire)
    }

    fn encode_with(&self, wire: ScriptPosWire) -> Result<Vec<u8>, String> {
        if self.if_entries.len() > u8::MAX as usize
            || self.gosub_entries.len() > u8::MAX as usize
            || self.loop_entries.len() > u8::MAX as usize
        {
            return Err("SCRIPTPOS: 栈深度超过引擎字节上限(255)".into());
        }
        if self.script_name.len() > MAX_SCRIPT_NAME {
            return Err(format!(
                "SCRIPTPOS: 脚本名 {} 字节超过引擎上限 {MAX_SCRIPT_NAME}",
                self.script_name.len()
            ));
        }
        let mut out = Vec::with_capacity(
            8 + 2
                + self.if_entries.len() * 18
                + 2
                + self.gosub_entries.len() * 20
                + 2
                + self.loop_entries.len() * 34
                + 8
                + 8
                + 8
                + self.script_name.len(),
        );
        out.extend_from_slice(&MAGIC);
        push_byte(&mut out, self.if_entries.len() as u8);
        for e in &self.if_entries {
            push_byte(&mut out, e.loop_depth);
            push_u32(&mut out, e.entry_pc);
            push_u32(&mut out, e.end_pc);
        }
        match wire.gosub_count {
            GosubCount::U16 => {
                // GOSUB 计数按 u16 写(4 字符,高字节在前;0.479 序列化器
                // `0x44b924`/`0x44b948` 对应 task+0x24c/+0x24d)。
                let n = self.gosub_entries.len() as u16;
                push_byte(&mut out, (n >> 8) as u8);
                push_byte(&mut out, n as u8);
            }
            GosubCount::U8 => push_byte(&mut out, self.gosub_entries.len() as u8),
        }
        for e in &self.gosub_entries {
            push_byte(&mut out, e.if_depth);
            push_byte(&mut out, e.loop_depth);
            push_u32(&mut out, e.return_pc);
            // 引擎写 `word [frame+8]` 后按 32 位零扩展输出(0.292 `0x438eae`、
            // 0.466 `0x443c51`、0.479 `0x44ba9f`、0.495 `0x452dbb`、
            // v494 `0x452c2f` 均为 movzx word → 8 字符)。
            push_u32(&mut out, e.return_script as u32);
        }
        push_byte(&mut out, self.loop_entries.len() as u8);
        for e in &self.loop_entries {
            push_byte(&mut out, e.if_depth);
            push_u32(&mut out, e.body_pc);
            push_u32(&mut out, e.end_pc);
            // 引擎只写 `[frame+0x10]` / `[frame+0x18]` 两个低 32 位。
            push_u32(&mut out, e.counter as u32);
            push_u32(&mut out, e.limit as u32);
        }
        // 尾部 u8:0.495 装载器 `0x451380` / 0.479 装载器 `0x44a369`
        // 在 text_line 之前先读 2 字符。旧内核漏写此字节,官方读出的
        // 脚本名整体前移 4 字符(「417」的根因)。
        if wire.tail_byte {
            push_byte(&mut out, self.tail_byte);
        }
        push_u32(&mut out, self.text_line);
        push_u32(&mut out, self.pc);
        push_u32(&mut out, self.script_name.len() as u32);
        out.extend_from_slice(&self.script_name);
        Ok(out)
    }

    /// 按引擎 `0x450fe0` 的布局解码;多余尾字节按引擎行为忽略。
    pub fn decode(blob: &[u8]) -> Result<Self, String> {
        // 旧内核(2026-10-26 前)的内部格式:8 字节 [script_id u32][pc u32]。
        // 它不是引擎串,不能静默按魔数错误处理;给出可操作的提示。
        if blob.len() == 8 && blob != MAGIC {
            return Err(
                "SCRIPTPOS: 旧内核 8 字节 blob 不兼容;需要用 0.495 引擎串或重新生成存档".into(),
            );
        }
        Self::decode_for_wire(blob, WIRE_GOSUB_U8).map(|(sp, _)| sp)
    }

    /// 优先按给定线格式解析,失败再试另一代;返回识别出的线格式。
    ///
    /// 最后回退旧内核布局(缺尾部 u8;2026-11 前写出的自家档)。
    /// 判定依据是**严格消耗完整个 blob**(官方串无填充)。
    pub fn decode_for_wire(
        blob: &[u8],
        preferred: ScriptPosWire,
    ) -> Result<(Self, Option<ScriptPosWire>), String> {
        if blob.len() == 8 && blob != MAGIC {
            return Err(
                "SCRIPTPOS: 旧内核 8 字节 blob 不兼容;需要用 0.495 引擎串或重新生成存档".into(),
            );
        }
        // 显式尝试**两代规范线格式**(计数宽度与尾部字节成套,不能只换宽度),
        // 当前版本对应的代优先。
        let order: [ScriptPosWire; 2] = if preferred == WIRE_GOSUB_U16 {
            [WIRE_GOSUB_U16, WIRE_GOSUB_U8]
        } else {
            [WIRE_GOSUB_U8, WIRE_GOSUB_U16]
        };
        let mut first_err: Option<String> = None;
        for wire in order {
            match Self::decode_with(blob, wire) {
                Ok(sp) => return Ok((sp, Some(wire))),
                Err(e) => {
                    if first_err.is_none() {
                        first_err = Some(e);
                    }
                }
            }
        }
        // 旧内核布局(缺尾部 u8):自家旧档兼容读。
        if let Ok(sp) = Self::decode_legacy(blob) {
            return Ok((sp, None));
        }
        Err(first_err.unwrap_or_else(|| "SCRIPTPOS: 无法识别线格式".into()))
    }

    fn decode_with(blob: &[u8], wire: ScriptPosWire) -> Result<Self, String> {
        let mut cur = Reader { data: blob, pos: 0 };
        if cur.take(MAGIC.len())? != MAGIC {
            return Err("SCRIPTPOS: 魔数不符(不是引擎执行位快照)".into());
        }
        let if_n = cur.byte()? as usize;
        let mut if_entries = Vec::with_capacity(if_n);
        for _ in 0..if_n {
            if_entries.push(IfEntry {
                loop_depth: cur.byte()?,
                entry_pc: cur.u32()?,
                end_pc: cur.u32()?,
            });
        }
        // 0.479:gosub 计数是 u16(4 字符);高字节在装载器里只落
        // `task+0x24c`(无对应帧),这里与引擎一致丢弃,只取低字节作帧数。
        let gosub_n = match wire.gosub_count {
            GosubCount::U16 => {
                let _hi = cur.byte()?;
                cur.byte()? as usize
            }
            GosubCount::U8 => cur.byte()? as usize,
        };
        let mut gosub_entries = Vec::with_capacity(gosub_n);
        for _ in 0..gosub_n {
            gosub_entries.push(GosubEntry {
                if_depth: cur.byte()?,
                loop_depth: cur.byte()?,
                return_pc: cur.u32()?,
                // 装载器只把低 16 位写回 `+8`(0.495 `0x451302` 的 `mov word`);
                // 高 16 位恒为引擎写出的 0,这里与引擎一样丢弃。
                return_script: cur.u32()? as u16,
            });
        }
        let loop_n = cur.byte()? as usize;
        let mut loop_entries = Vec::with_capacity(loop_n);
        for _ in 0..loop_n {
            loop_entries.push(LoopEntry {
                if_depth: cur.byte()?,
                body_pc: cur.u32()?,
                end_pc: cur.u32()?,
                counter: i64::from(cur.u32()? as i32),
                limit: i64::from(cur.u32()? as i32),
            });
        }
        let tail_byte = if wire.tail_byte { cur.byte()? } else { 0 };
        let text_line = cur.u32()?;
        let pc = cur.u32()?;
        let mut name_len = cur.u32()? as usize;
        if name_len > MAX_SCRIPT_NAME {
            // 引擎装载器 `0x451749` 直接把超长长度截到 0x106,不报错。
            name_len = MAX_SCRIPT_NAME;
        }
        let script_name = cur.take(name_len)?.to_vec();
        if cur.pos != blob.len() {
            return Err(format!(
                "SCRIPTPOS: 线格式({wire:?})未精确消耗:读 {} / 总 {}",
                cur.pos,
                blob.len()
            ));
        }
        Ok(Self {
            if_entries,
            gosub_entries,
            loop_entries,
            text_line,
            pc,
            script_name,
            tail_byte,
        })
    }

    /// 旧内核布局(缺尾部 u8,2026-11 前写出的自家档):无严格校验,
    /// 仅在严格格式都失败时兜底。
    fn decode_legacy(blob: &[u8]) -> Result<Self, String> {
        let mut cur = Reader { data: blob, pos: 0 };
        if cur.take(MAGIC.len())? != MAGIC {
            return Err("SCRIPTPOS: 魔数不符".into());
        }
        let if_n = cur.byte()? as usize;
        let mut if_entries = Vec::with_capacity(if_n);
        for _ in 0..if_n {
            if_entries.push(IfEntry {
                loop_depth: cur.byte()?,
                entry_pc: cur.u32()?,
                end_pc: cur.u32()?,
            });
        }
        let gosub_n = cur.byte()? as usize;
        let mut gosub_entries = Vec::with_capacity(gosub_n);
        for _ in 0..gosub_n {
            gosub_entries.push(GosubEntry {
                if_depth: cur.byte()?,
                loop_depth: cur.byte()?,
                return_pc: cur.u32()?,
                return_script: cur.u32()? as u16,
            });
        }
        let loop_n = cur.byte()? as usize;
        let mut loop_entries = Vec::with_capacity(loop_n);
        for _ in 0..loop_n {
            loop_entries.push(LoopEntry {
                if_depth: cur.byte()?,
                body_pc: cur.u32()?,
                end_pc: cur.u32()?,
                counter: i64::from(cur.u32()? as i32),
                limit: i64::from(cur.u32()? as i32),
            });
        }
        let text_line = cur.u32()?;
        let pc = cur.u32()?;
        let mut name_len = cur.u32()? as usize;
        if name_len > MAX_SCRIPT_NAME {
            name_len = MAX_SCRIPT_NAME;
        }
        let script_name = cur.take(name_len)?.to_vec();
        if cur.pos != blob.len() {
            return Err(format!(
                "SCRIPTPOS: 旧内核布局未精确消耗:读 {} / 总 {}",
                cur.pos,
                blob.len()
            ));
        }
        Ok(Self {
            if_entries,
            gosub_entries,
            loop_entries,
            text_line,
            pc,
            script_name,
            tail_byte: 0,
        })
    }
}

fn push_byte(out: &mut Vec<u8>, v: u8) {
    out.push(b'a' + (v >> 4));
    out.push(b'a' + (v & 0x0f));
}

fn push_u32(out: &mut Vec<u8>, v: u32) {
    push_byte(out, (v >> 24) as u8);
    push_byte(out, (v >> 16) as u8);
    push_byte(out, (v >> 8) as u8);
    push_byte(out, v as u8);
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.pos.checked_add(n).ok_or("SCRIPTPOS: 长度溢出")?;
        let slice = self
            .data
            .get(self.pos..end)
            .ok_or_else(|| format!("SCRIPTPOS: 截断(需要 {n} 字节)"))?;
        self.pos = end;
        Ok(slice)
    }

    fn byte(&mut self) -> Result<u8, String> {
        let hi = self.take(1)?[0];
        let lo = self.take(1)?[0];
        if !(b'a'..=b'p').contains(&hi) || !(b'a'..=b'p').contains(&lo) {
            return Err(format!(
                "SCRIPTPOS: 非法十六进制字符 {:?}/{:?}(应为 'a'..='p')",
                hi as char, lo as char
            ));
        }
        Ok(((hi - b'a') << 4) | (lo - b'a'))
    }

    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_be_bytes([
            self.byte()?,
            self.byte()?,
            self.byte()?,
            self.byte()?,
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_stack_layout_matches_engine() {
        let sp = ScriptPos::default();
        let blob = sp.encode().expect("encode");
        let mut expect = MAGIC.to_vec();
        // IF/GOSUB/LOOP 三个计数 0 → 各 "aa"。
        expect.extend_from_slice(b"aaaaaa");
        // text_line / pc / name_len 三个 u32 0 → 各 8 个 'a'。
        expect.extend(std::iter::repeat_n(b'a', 24));
        assert_eq!(blob, expect);
        assert_eq!(ScriptPos::decode(&blob).unwrap(), sp);
    }

    #[test]
    fn nibble_order_and_frame_layout_match_engine() {
        let sp = ScriptPos {
            if_entries: vec![IfEntry {
                loop_depth: 0,
                entry_pc: 0x0a,
                end_pc: 0x0b,
            }],
            ..Default::default()
        };
        let blob = sp.encode().expect("encode");
        let mut expect = MAGIC.to_vec();
        // IF 深度 1
        expect.extend_from_slice(b"ab");
        // loop_depth 0("aa") + entry_pc 0x0000000A("aaaaaaak") = 9 个 'a' + 'k'
        expect.extend_from_slice(b"aaaaaaaaak");
        expect.extend_from_slice(b"aaaaaaal"); // end pc 0x0B
        expect.extend_from_slice(b"aaaa"); // GOSUB/LOOP 计数 0
        expect.extend(std::iter::repeat_n(b'a', 24)); // 三个 u32 0
        assert_eq!(blob, expect);
        assert_eq!(ScriptPos::decode(&blob).unwrap(), sp);
    }

    #[test]
    fn roundtrip_preserves_all_fields() {
        let sp = ScriptPos {
            if_entries: vec![
                IfEntry {
                    loop_depth: 2,
                    entry_pc: 0x1234_5678,
                    end_pc: 0x9abc_def0,
                },
                IfEntry {
                    loop_depth: 0,
                    entry_pc: 0xffff_ffff,
                    end_pc: 0,
                },
            ],
            gosub_entries: vec![GosubEntry {
                if_depth: 1,
                loop_depth: 3,
                return_pc: 0x0bad_f00d,
                return_script: 0x1234,
            }],
            loop_entries: vec![LoopEntry {
                if_depth: 1,
                body_pc: 0x1111_1111,
                end_pc: 0x2222_2222,
                counter: 5,
                limit: -1,
            }],
            text_line: 0x7fff_ffff,
            pc: 0x8000_0000,
            script_name: b"data\\script\\eris\\stdlib\\ystdlib.yst".to_vec(),
            tail_byte: 0,
        };
        let blob = sp.encode().expect("encode");
        assert_eq!(
            blob.len(),
            8 + 2 + 2 * 18 + 2 + 20 + 2 + 34 + 8 + 8 + 8 + sp.script_name.len()
        );
        assert_eq!(ScriptPos::decode(&blob).unwrap(), sp);
    }

    #[test]
    fn gosub_return_script_is_wire_u32_and_decode_keeps_low16() {
        let sp = ScriptPos {
            gosub_entries: vec![GosubEntry {
                if_depth: 1,
                loop_depth: 2,
                return_pc: 0x0c,
                return_script: 0xabcd,
            }],
            ..Default::default()
        };
        let blob = sp.encode().expect("encode");
        assert_eq!(blob.len(), 8 + 2 + 2 + 2 + 20 + 8 + 8 + 8);
        // 记录从 GOSUB 计数之后开始:8 magic + 2 IF 计数 + 2 GOSUB 计数。
        let rec = &blob[12..];
        assert_eq!(&rec[0..2], b"ab"); // if_depth 1
        assert_eq!(&rec[2..4], b"ac"); // loop_depth 2
        assert_eq!(&rec[4..12], b"aaaaaaam"); // return_pc 0x0C
        assert_eq!(&rec[12..20], b"aaaaklmn"); // 返回脚本号 0x0000ABCD
        // 高 16 位即使非零,装载器/内核也只保留低 16 位(引擎 `mov word`)。
        let mut tampered = blob.clone();
        tampered[12 + 12..12 + 16].copy_from_slice(b"abab");
        assert_eq!(
            ScriptPos::decode(&tampered).unwrap().gosub_entries[0].return_script,
            0xabcd
        );
    }

    #[test]
    fn decode_clamps_oversized_name_length_like_loader() {
        let sp = ScriptPos {
            script_name: vec![b'q'; MAX_SCRIPT_NAME],
            ..Default::default()
        };
        let mut blob = sp.encode().expect("encode");
        // 无栈时 name_len 位于 8(magic)+6(三个计数)+8(text)+8(pc)=30。
        let over = (MAX_SCRIPT_NAME + 5) as u32;
        let mut name_len = Vec::new();
        push_u32(&mut name_len, over);
        blob[30..38].copy_from_slice(&name_len);
        let decoded = ScriptPos::decode(&blob).expect("decode");
        assert_eq!(decoded.script_name.len(), MAX_SCRIPT_NAME);
    }

    #[test]
    fn decode_hand_built_engine_blob() {
        // 手工按 0x452a54 布局拼一个含三栈的串,反向验证 0x450fe0 的解析。
        let mut blob = MAGIC.to_vec();
        blob.extend_from_slice(b"ab"); // IF 深度 1
        blob.extend_from_slice(b"aa"); // IF +0 loop_depth 0
        blob.extend_from_slice(b"aaaaaaak"); // IF +4 entry 0x0A
        blob.extend_from_slice(b"aaaaaaal"); // IF +8 end 0x0B
        blob.extend_from_slice(b"ab"); // GOSUB 深度 1
        blob.extend_from_slice(b"ab"); // +0 if_depth 1
        blob.extend_from_slice(b"ac"); // +1 loop_depth 2
        blob.extend_from_slice(b"aaaaaaam"); // +4 return_pc 0x0C
        blob.extend_from_slice(b"aaaaaaad"); // +8 return_script 3(线上 u32 BE)
        blob.extend_from_slice(b"ab"); // LOOP 深度 1
        blob.extend_from_slice(b"ab"); // +0 if_depth 1
        blob.extend_from_slice(b"aaaaaaan"); // +4 body_pc 0x0D
        blob.extend_from_slice(b"aaaaaaao"); // +8 end_pc 0x0E
        blob.extend_from_slice(b"aaaaaaac"); // +0x10 counter 2
        blob.extend_from_slice(b"aaaaaaaf"); // +0x18 limit 5
        blob.extend_from_slice(b"aaaaaaah"); // text_line 7
        blob.extend_from_slice(b"aaaaaabb"); // pc 0x11
        blob.extend_from_slice(b"aaaaaaab"); // name_len 1
        blob.push(b'x');
        let sp = ScriptPos::decode(&blob).expect("decode");
        assert_eq!(
            sp.if_entries,
            vec![IfEntry {
                loop_depth: 0,
                entry_pc: 0x0A,
                end_pc: 0x0B
            }]
        );
        assert_eq!(
            sp.gosub_entries,
            vec![GosubEntry {
                if_depth: 1,
                loop_depth: 2,
                return_pc: 0x0C,
                return_script: 3
            }]
        );
        assert_eq!(
            sp.loop_entries,
            vec![LoopEntry {
                if_depth: 1,
                body_pc: 0x0D,
                end_pc: 0x0E,
                counter: 2,
                limit: 5
            }]
        );
        assert_eq!(sp.text_line, 7);
        assert_eq!(sp.pc, 0x11);
        assert_eq!(sp.script_name, b"x");
    }

    #[test]
    fn decode_rejects_legacy_eight_byte_blob() {
        let legacy = [0u8, 1, 0, 0, 2, 0, 0, 0];
        let err = ScriptPos::decode(&legacy).unwrap_err();
        assert!(err.contains("旧内核 8 字节"), "{err}");
    }

    #[test]
    fn decode_rejects_bad_magic_and_truncation() {
        let mut blob = ScriptPos::default().encode().unwrap();
        blob[0] = 0;
        assert!(ScriptPos::decode(&blob).is_err());
        let full = ScriptPos::default().encode().unwrap();
        assert!(ScriptPos::decode(&full[..full.len() - 1]).is_err());
    }

    #[test]
    fn decode_rejects_non_hex_chars_and_long_name() {
        let mut blob = ScriptPos::default().encode().unwrap();
        blob[8] = b'0'; // 非 'a'..'p'
        assert!(ScriptPos::decode(&blob).is_err());
        let sp = ScriptPos {
            script_name: vec![b'x'; MAX_SCRIPT_NAME + 1],
            ..Default::default()
        };
        assert!(sp.encode().is_err());
    }

    /// **官方 0.479 存档逐字节回归**(黒獣2 `save00001.sd` DNO=11):
    /// u16 GOSUB 计数 + 尾部 u8 的线格式;解码字段与再编码字节必须全等。
    ///
    /// 证据:0.479 装载器 `0x449d7c`(`0x449f3d/0x449f58` 两组 2 字符计数
    /// →`task+0x24c/+0x24d`,`0x44a369` 尾部 u8,`0x44a36f..` 三 u32,
    /// 最后原样拷脚本名到 `+0x106` 上限缓冲)。
    #[test]
    fn official_0479_task_blob_roundtrips_byte_exact() {
        const BLOB: [u8; 82] = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61,
            0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x6d, 0x61, 0x61,
            0x61, 0x61, 0x61, 0x61, 0x65, 0x64, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x63, 0x69,
            0x6b, 0x75, 0x72, 0x6f, 0x32, 0x5c, 0x73, 0x63, 0x72, 0x69, 0x70, 0x74, 0x5c, 0x75,
            0x73, 0x65, 0x72, 0x73, 0x63, 0x72, 0x69, 0x70, 0x74, 0x5c, 0x6e, 0x65, 0x77, 0x5c,
            0x30, 0x30, 0x5f, 0x6f, 0x70, 0x30, 0x30, 0x31, 0x2e, 0x74, 0x78, 0x74,
        ];
        let (sp, wire) =
            ScriptPos::decode_for_wire(&BLOB, WIRE_GOSUB_U16).expect("官方 0.479 任务块");
        assert_eq!(wire.map(|w| w.gosub_count), Some(GosubCount::U16));
        assert!(sp.if_entries.is_empty());
        assert!(sp.gosub_entries.is_empty());
        assert!(sp.loop_entries.is_empty());
        assert_eq!(sp.tail_byte, 0);
        assert_eq!(sp.text_line, 12);
        assert_eq!(sp.pc, 67);
        assert_eq!(sp.script_name, br"kuro2\script\userscript\new\00_op001.txt");
        assert_eq!(sp.encode_with_wire(WIRE_GOSUB_U16).unwrap(), BLOB);
    }

    /// 官方 0.479 存档(黒獣2 `save00002.sd` DNO=11):GOSUB 栈非空
    /// (2 帧,u16 计数);解码与再编码全等。
    #[test]
    fn official_0479_task_blob_with_gosub_frames_roundtrips() {
        const BLOB: [u8; 115] = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x61, 0x61, 0x61, 0x61, 0x61, 0x63,
            0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x62, 0x6c, 0x61, 0x61,
            0x61, 0x61, 0x61, 0x61, 0x69, 0x66, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61,
            0x62, 0x64, 0x70, 0x70, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x62, 0x6c, 0x61, 0x61,
            0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61,
            0x61, 0x62, 0x65, 0x6b, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x63, 0x62, 0x6b, 0x75,
            0x72, 0x6f, 0x32, 0x5c, 0x73, 0x63, 0x72, 0x69, 0x70, 0x74, 0x5c, 0x65, 0x72, 0x69,
            0x73, 0x5c, 0x65, 0x73, 0x5f, 0x73, 0x63, 0x65, 0x6e, 0x61, 0x72, 0x69, 0x6f, 0x2e,
            0x79, 0x73, 0x74,
        ];
        let (sp, _) = ScriptPos::decode_for_wire(&BLOB, WIRE_GOSUB_U16).expect("官方 0.479 任务块");
        assert_eq!(sp.gosub_entries.len(), 2);
        assert_eq!(sp.pc, 330);
        assert_eq!(sp.script_name, br"kuro2\script\eris\es_scenario.yst");
        assert_eq!(sp.encode_with_wire(WIRE_GOSUB_U16).unwrap(), BLOB);
    }

    /// 写错代(GOSUB 宽度)必须被严格消耗校验拒绝,不能静默读出垃圾。
    #[test]
    fn wrong_wire_generation_is_rejected() {
        const BLOB: [u8; 115] = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x61, 0x61, 0x61, 0x61, 0x61, 0x63,
            0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x62, 0x6c, 0x61, 0x61,
            0x61, 0x61, 0x61, 0x61, 0x69, 0x66, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61,
            0x62, 0x64, 0x70, 0x70, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x62, 0x6c, 0x61, 0x61,
            0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61,
            0x61, 0x62, 0x65, 0x6b, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x63, 0x62, 0x6b, 0x75,
            0x72, 0x6f, 0x32, 0x5c, 0x73, 0x63, 0x72, 0x69, 0x70, 0x74, 0x5c, 0x65, 0x72, 0x69,
            0x73, 0x5c, 0x65, 0x73, 0x5f, 0x73, 0x63, 0x65, 0x6e, 0x61, 0x72, 0x69, 0x6f, 0x2e,
            0x79, 0x73, 0x74,
        ];
        assert!(ScriptPos::decode_with(&BLOB, WIRE_GOSUB_U8).is_err());
    }

    /// **官方 0.495 存档逐字节回归**(SDK `data/save/save00001.sd`,容器
    /// `0x1E0`,DNO=11):u8 GOSUB 计数、**无**尾部 u8;解码与再编码全等。
    ///
    /// 证据:0.495 装载器 `0x450fe0`(`add ebp,2` 单计数,`add ebp,0x14`
    /// GOSUB 帧,`add ebp,0x22` LOOP 帧);样本精确消耗反证无尾部字节。
    #[test]
    fn official_0495_task_blob_roundtrips_byte_exact() {
        const BLOB: [u8; 90] = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x61, 0x61, 0x61, 0x62, 0x61, 0x61,
            0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x63, 0x61, 0x61, 0x61, 0x61,
            0x61, 0x61, 0x69, 0x63, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61,
            0x61, 0x61, 0x61, 0x61, 0x61, 0x62, 0x65, 0x6b, 0x61, 0x61, 0x61, 0x61, 0x61, 0x61,
            0x63, 0x61, 0x64, 0x61, 0x74, 0x61, 0x5c, 0x73, 0x63, 0x72, 0x69, 0x70, 0x74, 0x5c,
            0x65, 0x72, 0x69, 0x73, 0x5c, 0x65, 0x73, 0x5f, 0x73, 0x63, 0x65, 0x6e, 0x61, 0x72,
            0x69, 0x6f, 0x2e, 0x79, 0x73, 0x74,
        ];
        let (sp, wire) =
            ScriptPos::decode_for_wire(&BLOB, WIRE_GOSUB_U8).expect("官方 0.495 任务块");
        assert_eq!(wire.map(|w| w.gosub_count), Some(GosubCount::U8));
        assert!(sp.if_entries.is_empty());
        assert_eq!(sp.gosub_entries.len(), 1);
        assert_eq!(sp.pc, 330);
        assert_eq!(sp.script_name, br"data\script\eris\es_scenario.yst");
        assert_eq!(sp.encode_with_wire(WIRE_GOSUB_U8).unwrap(), BLOB);
    }
}
