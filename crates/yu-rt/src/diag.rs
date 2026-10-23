//! 诊断构造。`Diag` 类型来自 `yu-log`(错误+日志一体包)。
//!
//! 本模块只提供 `rt.*` / `table.*` / `fmt.*` 码域下的构造助手,
//! 让每个失败点都带上稳定码、边界归属与位置。

use yu_log::{Boundary, Diag, Loc, Severity};

use crate::code;

pub fn fatal(code: &'static str, boundary: Boundary, detail: impl Into<String>) -> Diag {
    Diag::new(code, Severity::Fatal, boundary).with_detail(detail)
}

pub fn recoverable(code: &'static str, boundary: Boundary, detail: impl Into<String>) -> Diag {
    Diag::new(code, Severity::Recoverable, boundary).with_detail(detail)
}

pub fn unimplemented(code: &'static str, boundary: Boundary, detail: impl Into<String>) -> Diag {
    Diag::new(code, Severity::Unimplemented, boundary).with_detail(detail)
}

pub fn info(code: &'static str, boundary: Boundary, detail: impl Into<String>) -> Diag {
    Diag::new(code, Severity::Info, boundary).with_detail(detail)
}

/// 脚本位置的诊断构造。
pub fn at_script(mut diag: Diag, file: impl Into<String>, pc: u32, cmd: Option<String>) -> Diag {
    diag.where_ = Loc::script(file, None, pc, cmd);
    diag
}

/// 带任务标识的脚本位置(任务书 E R11:`Diag` 要能定位到 task/script/pc)。
pub fn at_script_task(
    mut diag: Diag,
    file: impl Into<String>,
    task: Option<String>,
    pc: u32,
    cmd: Option<String>,
) -> Diag {
    diag.where_ = Loc::script(file, task, pc, cmd);
    diag
}

/// 表位置的诊断构造。
pub fn at_table(mut diag: Diag, name: impl Into<String>) -> Diag {
    diag.where_ = Loc::table(name);
    diag
}

/// 数据位置的诊断构造。
pub fn at_data(mut diag: Diag, path: &[u8], offset: u64) -> Diag {
    diag.where_ = Loc::data(path.to_vec(), offset);
    diag
}

/// 便捷:未实现命令。参数 `name` 来自游戏表,不是常量。
pub fn cmd_unimplemented(name: &str, detail: impl Into<String>) -> Diag {
    unimplemented(
        code::rt::CMD_UNIMPLEMENTED,
        Boundary::Script,
        format!("{name}: {}", detail.into()),
    )
}

/// 便捷:未取证的语义细节(确定性实现 + 挂号)。
pub fn undetermined(detail: impl Into<String>) -> Diag {
    unimplemented(code::rt::UNDETERMINED, Boundary::Script, detail)
}
