//! 终端产品输出。
//!
//! 任务书 K2 §0.1:`src/` 零 `println!/eprintln!/dbg!`;`yu-cli` 作为终端
//! 工具的产品输出**只**经过本模块。其它模块一律返回 `Result`/字符串,
//! 由 `main`/`serve` 的边界决定打印到哪里。

use std::io::Write;

/// 一行标准输出(报告、进度)。
pub fn line(message: impl AsRef<str>) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{}", message.as_ref());
    let _ = out.flush();
}

/// 一行错误输出。
pub fn error(message: impl AsRef<str>) {
    let mut out = std::io::stderr().lock();
    let _ = writeln!(out, "{}", message.as_ref());
    let _ = out.flush();
}

/// 原样输出(不附加换行;已经排版好的块)。
pub fn raw(text: impl AsRef<str>) {
    let mut out = std::io::stdout().lock();
    let _ = write!(out, "{}", text.as_ref());
    let _ = out.flush();
}
