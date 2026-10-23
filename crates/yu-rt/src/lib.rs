//! `yu-rt` · YU-RIS 运行核(M2)。
//!
//! # 设计约束(A5 零硬编码)
//!
//! 本 crate 内**只有引擎语义**进代码;命令序号、命令名集合、系统变量 id、
//! 变量 id、标签、脚本号、屏幕尺寸、FPS 默认值等**全部在运行期从游戏工程
//! 数据装载**。容器解码唯一来源 = `yu-format`(R12 起,`tables.rs` 只是
//! 运行核适配层);编译器配置(YSCD)不在 yu-format 范围,归 [`ycd`]:
//!
//! | 数据 | 来源(工程自带) | 解码 → 本 crate 适配 |
//! |---|---|---|
//! | 命令表(名字↔序号、参数表、参数种类) | `ysbin/ysc.ybn`(YSCM) | `yu_format::yscm` → [`tables::YscmTable`] |
//! | 编译器语言定义(命令参数属性、548 项系统变量、设置项) | `system/YSCom/YSCom.ycd`(YSCD) | [`ycd::YcdTable`] |
//! | 标签表(名字→pc/脚本号/变量域基址) | `ysbin/ysl.ybn`(YSLB) | `yu_format::yslb` → [`tables::LabelTable`] |
//! | 变量初值镜像 | `ysbin/ysv.ybn`(YSVR) | `yu_format::ysvr` → [`tables::VarImage`] |
//! | 脚本清单 | `ysbin/yst_list.ybn`(YSTL) | `yu_format::ystl` → [`tables::ScriptList`] |
//! | 引擎设置(分辨率/标题/扩展名优先序) | `ysbin/yscfg.ybn`(YSCF) | `yu_format::yscf`(直接采用,宽高 u32) |
//! | 脚本体(组/窗口/参数池) | `ysbin/yst%05d.ybn`(YSTB) | `yu_format::ystb` → [`tables::ScriptFile`] |
//!
//! 缺任一张表 ⇒ [`Diag`] `table.missing` / `table.incomplete`,**拒绝启动**
//! ([`tables::TableSet::load`])。运行核内不存在内置命令表或默认表。
//!
//! # 三层真值纪律
//!
//! 每条已实现语义在代码 doc 中标注三层出处:
//! ①官方手册页(相对 `<sdk495>/マニュアル/YU-RIS/html/` 的路径);
//! ②真实脚本使用形态(ERIS 官方编译语料 `ysbin/`,含频度);
//! ③官方引擎行为/逆向事实(0.495 `yu-ris.exe` sha256 `4f5eefa3…`,
//! 旁线笔记 `<side-repo>/docs/format-vm-opcodes.md` 的地址级证据)。
//!
//! 缺一层 ⇒ `Diag::Unimplemented` 挂号([`diag::rt`]),并在
//! `crates/yu-rt/docs/divergences/` 留档。
//!
//! # 参考实现(算法采用,已登记)
//!
//! 任务链/让出/执行位交换/启动链编排参考 `<repo-old>/crates/yuris-vm/src/{task,driver,boot}.rs`;
//! 表达式字节码记录格式参考同仓 `crates/yuris-script/src/insn.rs`;
//! YSCM 表构建流程参考 `RxYuris/lib/YurisStaticLibrary/YSCM.cpp`。
//! **语义结论不以它们为准**——见各处理器的出处注释与 divergences。

#![allow(clippy::result_large_err)]
#![forbid(unsafe_code)]

pub mod case_tables;
pub mod code;
pub mod diag;
pub mod dialog;
pub mod event;
pub mod expr;
mod image;
pub mod input;
pub mod json;
pub mod rng;
pub mod sysvar;
pub mod sysvar_abi;
pub mod tables;
pub mod task;
pub mod text;
pub mod value;
pub mod vm;
pub mod ycd;

pub use event::VmEvent;
pub use input::{InputState, InputTimeline};
pub use tables::{BootTables, TableSet};
pub use task::{ChainReport, TaskSeed};
pub use text::{TextSnapshot, TextState};
pub use value::{ElemType, Value, VarRef, VarSpace};
pub use vm::{BootConfig, Vm, VmError};
pub use yu_log::Diag;
