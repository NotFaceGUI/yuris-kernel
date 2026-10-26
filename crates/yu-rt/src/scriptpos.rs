//! `SCRIPTPOS` 执行位快照:与官方引擎**线格式逐字节兼容**的编解码。
//!
//! # 跨版本证据(五个官方 exe 的序列化器 + 装载器)
//!
//! 线格式不是某一版的私产:0.292 / 0.466 / 0.479 / 0.495 / v494 五版
//! 的序列化器推进量都是 IF `0x12`、GOSUB `0x14`、LOOP `0x22` 字符,
//! 装载器也按同一节奏消费(各版只在**任务结构体内偏移**上不同,
//! 线上字节完全一致):
//!
//! | 版本 | 序列化器 | 装载器 |
//! |---|---|---|
//! | 0.292 | `0x438b2c` | `0x437548` |
//! | 0.466 | `0x4438d0` | `0x441ed4` |
//! | 0.479 | `0x44b718` | `0x449d7c` |
//! | 0.495 | `0x452a54` | `0x450fe0` |
//! | v494 | `0x4528c8` | `0x450e74` |
//!
//! 布局(0.495 `0x452a54` 逐指令;其余版本同构):
//! - 先写 8 字节魔数(`0x5b4090` = `01..08`);
//! - IF 计数(1 字节)+ 每帧 18 字符:loop 深度、entry pc、end pc;
//! - GOSUB 计数(1 字节)+ 每项 20 字符:IF 深度、loop 深度、返回 pc、
//!   返回脚本号。**返回脚本号在线上是 4 字节**:引擎把运行期 u16 零
//!   扩展到 u32 写出(高 2 字节恒为 `aa`),装载器消费 8 字符但只取
//!   低 16 位写回 `+8`;
//! - LOOP 计数(1 字节)+ 每项 34 字符:IF 深度、body pc、end pc、
//!   counter 低 32 位、limit 低 32 位(装载器符号扩展到 i64);
//! - `task+0xc`(文本行,u32)、`task+0x20`(pc,u32)、脚本名长度(u32)
//!   + 原始脚本名字节。装载器把超过 `0x106` 的名字长度截到 `0x106`。
//!
//! 0.495 的帧字段来源:IF `0x4462d0` / LOOP `0x447ef0` / GOSUB `0x4459c4`;
//! 写回字段:`task+0x3e`(IF 深度)、`task+0x140`(GOSUB 深度)、
//! `task+0x244`(LOOP 深度)、三个帧数组、`task+0xc`、`task+0x20`,
//! 并用脚本名在 `0x801850` 名字表里 strcmp 找回 `task+0x3c`。
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
}

impl ScriptPos {
    /// 按引擎 `0x452a54` 的布局编码。
    pub fn encode(&self) -> Result<Vec<u8>, String> {
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
        push_byte(&mut out, self.gosub_entries.len() as u8);
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
        let gosub_n = cur.byte()? as usize;
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
        let text_line = cur.u32()?;
        let pc = cur.u32()?;
        let mut name_len = cur.u32()? as usize;
        if name_len > MAX_SCRIPT_NAME {
            // 引擎装载器 `0x451749` 直接把超长长度截到 0x106,不报错。
            name_len = MAX_SCRIPT_NAME;
        }
        let script_name = cur.take(name_len)?.to_vec();
        Ok(Self {
            if_entries,
            gosub_entries,
            loop_entries,
            text_line,
            pc,
            script_name,
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
        // IF/GOSUB/LOOP 三个深度字节 0 → 各 "aa"。
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
        expect.extend_from_slice(b"aaaa"); // GOSUB/LOOP 深度 0
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
}
