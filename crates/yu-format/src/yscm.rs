//! `ysc.ybn`(魔数 `YSCM`)· 命令定义表 —— 运行核"按名分派、零硬编码"的数据源。
//!
//! 布局(ERIS 语料实测闭合 + 旁线 `format-ybn.md` §5 + 引擎装载器
//! `FUN_0045cf90`;参考 RxYuris/lib/YurisStaticLibrary/YSCM.cpp 的解析流程):
//!
//! ```text
//! +0x00 char[4] "YSCM"
//! +0x04 u32     版本
//! +0x08 u32     命令条数(0.495 语料 = 119)
//! +0x0C u32     观测为 0
//! +0x10 记录[条数](顺序 = opcode,实证见 PROGRESS 成果 B2):
//!        char[]  命令名(NUL 结尾,ASCII)
//!        u8      参数个数 n
//!        n × { char[] 参数名(NUL 结尾), u8 flag, u8 kind }
//! 之后  37 条 NUL 结尾消息(引擎常量;0.495 语料多为错误文案)
//! 之后  256 字节表(引擎整块装进 DAT_00689a00;本次实证其内容为
//!        键位→VK 码映射,如 A–Z=65..89、F1–F12=112..123,与 ysc 无 opcode 关系)
//! ```
//!
//! YSTB 指令序号 = 本表记录下标(字母序)。注意:**命令只在源码写出参数时
//! 才发射参数记录**,因此"指令参数个数"不必然等于表中 argc —— 控制流命令
//! (IF/LET/GO…)在表中 argc=0 是占位,不是约束(实证:GO[#=x] 编为 1 参,
//! LET[@a=1] 编为 2 参)。

use crate::container::{find_nul, read_header, truncated, u32_at};
use yu_log::Diag;

/// 引擎硬编码的消息条数(旁线 RE:`FUN_0045cf90`,`iVar4 < 0x91` 步进 4)。
pub const MESSAGE_COUNT: usize = 37;

/// 尾部附表的字节数(引擎整块拷入 `DAT_00689a00`)。
pub const KEY_TABLE_BYTES: usize = 0x100;

/// 一条命令参数(`参数名 + 两个元数据字节`)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Arg {
    /// 参数名(如 `#`、`SX`、`FILE`)。
    pub name: Box<str>,
    /// 元数据字节 1(ycd 全宽形态的高半部分;语义归运行核/手册)。
    pub flag: u8,
    /// 元数据字节 2(如 `SX`=0x14、`Z`=2;语义归运行核/手册)。
    pub kind: u8,
}

/// 一条命令定义。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    /// 命令名(如 `GO`、`CG`)。
    pub name: Box<str>,
    /// 表内登记的参数个数(控制流命令多为 0,见模块文档)。
    pub argc: u8,
    /// 参数表。
    pub args: Vec<Arg>,
}

/// 已解码的命令表。**下标即 opcode**(ERIS 语料实证)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandTable {
    /// 容器头版本(`+0x04`;ERIS 语料 495。R12 起 yu-rt 适配层需要)。
    pub version: u32,
    /// 命令(文件顺序)。
    pub commands: Vec<Command>,
    /// 表尾 37 条消息(原始字节,编码按运行配置解码)。
    pub messages: Vec<Vec<u8>>,
    /// 尾部 256 字节附表(键位映射;原样保留)。
    pub key_table: Box<[u8; KEY_TABLE_BYTES]>,
}

impl CommandTable {
    /// 解码 `ysc.ybn` 字节。`origin` 用于诊断定位。
    pub fn parse(buf: &[u8], origin: &[u8]) -> Result<Self, Diag> {
        let header = read_header(buf, origin, b"YSCM")?;
        let count = header.word08 as usize;
        let mut at = 0x10usize;
        let mut commands = Vec::with_capacity(count.min(4096));
        for _index in 0..count {
            let name_end = find_nul(buf, at, origin, "YSCM 命令名")?;
            let name = &buf[at..name_end];
            let argc = *buf.get(name_end + 1).ok_or_else(|| {
                truncated(origin, name_end + 2, buf.len(), "YSCM 命令参数个数")
            })?;
            let mut at_arg = name_end + 2;
            let mut args = Vec::with_capacity(usize::from(argc));
            for _ in 0..argc {
                let end = find_nul(buf, at_arg, origin, "YSCM 参数名")?;
                let flag = *buf.get(end + 1).ok_or_else(|| {
                    truncated(origin, end + 2, buf.len(), "YSCM 参数元数据")
                })?;
                let kind = *buf.get(end + 2).ok_or_else(|| {
                    truncated(origin, end + 3, buf.len(), "YSCM 参数元数据")
                })?;
                args.push(Arg {
                    name: String::from_utf8_lossy(&buf[at_arg..end]).into_owned().into(),
                    flag,
                    kind,
                });
                at_arg = end + 3;
            }
            commands.push(Command {
                name: String::from_utf8_lossy(name).into_owned().into(),
                argc,
                args,
            });
            at = at_arg;
        }
        let mut messages = Vec::with_capacity(MESSAGE_COUNT);
        for _ in 0..MESSAGE_COUNT {
            let end = find_nul(buf, at, origin, "YSCM 消息")?;
            messages.push(buf[at..end].to_vec());
            at = end + 1;
        }
        if at + KEY_TABLE_BYTES > buf.len() {
            return Err(truncated(
                origin,
                at + KEY_TABLE_BYTES,
                buf.len(),
                "YSCM 尾部附表",
            ));
        }
        let mut key_table = Box::new([0u8; KEY_TABLE_BYTES]);
        key_table.copy_from_slice(&buf[at..at + KEY_TABLE_BYTES]);
        let _ = u32_at(buf, 0x0C); // 观测为 0;保留不解释
        Ok(Self {
            version: header.version,
            commands,
            messages,
            key_table,
        })
    }

    /// 序号 → 命令名(下标即 opcode)。越界 = `None`。
    #[must_use]
    pub fn name_of(&self, index: u16) -> Option<&str> {
        self.commands.get(usize::from(index)).map(|c| c.name.as_ref())
    }

    /// 命令名 → 序号。未知命令 = `None`(调用方产出 `fmt.ybn.opcode` 类诊断)。
    #[must_use]
    pub fn index_of(&self, name: &str) -> Option<u16> {
        self.commands
            .iter()
            .position(|c| c.name.as_ref() == name)
            .and_then(|i| u16::try_from(i).ok())
    }

    /// 命令条数。
    #[must_use]
    pub fn len(&self) -> u16 {
        // 表条数来自 u32 计数字段;本表由引擎派发表(0..118)约束,
        // 超过 u16 的表不是已证形态。
        u16::try_from(self.commands.len()).unwrap_or(u16::MAX)
    }

    /// 是否为空。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

