//! `yu-format` · ybn 家族容器与结构解码(纯字节进出,不碰 IO)。
//!
//! 边界(任务书 B):容器与结构解码;**ypf 包布局归 `yu-vfs`,本 crate
//! 不实现 ypf**;执行语义(窗口字节求值、表达式)归 `yu-rt`。
//! 依赖仅 [`yu-log`](yu_log)(错误 + 日志契约)。
//!
//! # 模块地图
//!
//! | 模块 | 文件 | 内容 |
//! |---|---|---|
//! | [`key`] | yst%05d | CRC-32 与分段异或密钥 |
//! | [`container`] | 全家族 | 16 字节容器头与字节读取工具 |
//! | [`yscm`] | ysc.ybn | **命令表**(下标 = opcode;按名分派的数据源) |
//! | [`ystb`] | yst%05d.ybn | 脚本体四区段 → [`DecodedScript`] |
//! | [`ystl`] | yst_list.ybn | 脚本清单(下标 ↔ 源脚本名 + 统计) |
//! | [`yslb`] | ysl.ybn | 全局标签表 |
//! | [`ysvr`] | ysv.ybn | 变量声明(kind/flag/file/slot/ty/dims/初值) |
//! | [`ystd`] | yst.ybn | 变量槽位表头(varCount) |
//! | [`yssd`] | save/*.sd | **存档容器**(YSSD:块表 + zlib level-1 写路径) |
//! | [`yscf`] | yscfg.ybn | 引擎配置 |
//! | [`yser`] | yse.ybn | id ↔ 名字表 |
//! | [`codec`] | — | CP932/GBK 静态码表 + auto 评分器 |
//!
//! # 与 yu-rt 的接口契约(冻结,任务书 §4)
//!
//! ```text
//! pub struct DecodedScript { pub script_id: u32, pub groups: Vec<Group>, pub labels: LabelTable }
//! pub struct Group { pub command: Cow<'static, str>, pub windows: Vec<Window> }
//! pub struct Window { pub bytes: [u8; 12] }
//! ```
//!
//! 设计理由:窗口字节语义(参数槽 tag 等)属运行核;格式层只负责**忠实
//! 切分**。`command` 查不到(序号超出 ysc 表)= `fmt.ybn.opcode` 诊断,
//! 绝不落序号。
//!
//! [`LabelTable`] 的内部形状由本 crate 定义(契约未冻结其字段):
//! 它是**本脚本静态可辨的标签引用表**(GO/GOSUB/TASK 等命令的 `#=`
//! 实参,按 ysc 表中名为 `#` 的参数位识别),全局标签表在 [`yslb`]。
//!
//! 参考(算法与事实来源,均已实证;致谢链见
//! `docs/references/open-source-tools.md` §4):
//! - RxYuris `YSTB.cpp` / `YSCM.cpp` / `YSTL.cpp`(分段异或与解析流程)
//! - YU-RIS-Script-Editor `core/ystb.py`(V2/V5 支路、文本段形态)
//!   与 `core/encoding.py`(编码自检测思路)
//! - 旧接手线 `<repo-old>/crates/yuris-format`(记录布局事实与
//!   YSVR `ty=0 无初值段` 的双语料闭合证据)
//! - 旁线逆向笔记 `<<side-repo>>/docs/format-ybn*.md`(引擎装载器取证)

#![allow(clippy::result_large_err)]

pub mod codec;
pub mod code;
pub mod container;
pub mod key;
pub mod yscm;
pub mod yslb;
pub mod ystb;
pub mod yscf;
pub mod yser;
pub mod yssd;
pub mod ystd;
pub mod ystl;
pub mod ysvr;

use std::borrow::Cow;

pub use codec::{auto, Codec};
pub use yscm::CommandTable;
pub use yslb::{LabelRecord, YslLabels};
pub use ystb::Segments;
pub use ystl::ScriptList;
pub use ysvr::{Init, VarDecl, VarDecls};
pub use ystd::YstdHeader;

/// 与 yu-rt 的接口契约(冻结):一个解码后的脚本。
///
/// `script_id` = `yst%05d.ybn` 的编号;`groups` 为指令序列(下标即
/// 执行顺序);`labels` 为本脚本静态可辨的标签引用。
#[derive(Clone, Debug, PartialEq)]
pub struct DecodedScript {
    /// 脚本编号(`yst%05d` 中的编号)。
    pub script_id: u32,
    /// 指令组(命令 + 窗口切片)。
    pub groups: Vec<Group>,
    /// 本脚本的标签引用(静态可辨部分;动态目标归运行核)。
    pub labels: LabelTable,
}

/// 契约(冻结):一条指令 = 命令名 + 参数窗口切片。
///
/// `command` 来自 ysc 表(`Cow::Owned` 形态;契约要求 `'static` 以便
/// 运行核用自有的静态名构造同类组)。窗口求值归 `yu-rt`。
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    /// 经 ysc 表解析的命令名;查不到 = `fmt.ybn.opcode` 诊断,不是序号。
    pub command: Cow<'static, str>,
    /// 窗口(参数记录)切片,原样保留。
    pub windows: Vec<Window>,
}

/// 契约(冻结):12 字节参数记录,字节语义归 `yu-rt`。
///
/// 布局(u16/u32 小端,由本层解析但不解释):
/// `{ u16 id, u16 type, u32 载荷长, u32 载荷在参数数据区的偏移 }`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    /// 原始 12 字节。
    pub bytes: [u8; 12],
}

/// 本脚本的标签引用表(静态可辨部分,按出现顺序)。
///
/// 识别规则(数据驱动,零硬编码):命令的 ysc 参数表里名为 `#` 的参数位
/// (`#=名字` 语法,官方手册 basic/basic),且窗口载荷是 `M` 形引号串
/// (`'M' + u16 长度 + \"…\"`);动态目标(`H`/`V`/数值载荷)不进表。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LabelTable {
    /// 引用(按出现顺序)。
    pub entries: Vec<LabelRef>,
}

/// 一条标签引用。
#[derive(Clone, Debug, PartialEq)]
pub struct LabelRef {
    /// 标签名(按运行编解码器从字节解出)。
    pub name: String,
    /// 引用它的指令组下标(`groups` 中的位置)。
    pub group: u32,
}
