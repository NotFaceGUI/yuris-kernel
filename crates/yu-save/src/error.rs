//! `sv.*` 诊断码与构造助手(范式照抄 `yu-sound::error`)。
//!
//! 所有错误/未实现都走 `yu-log::Diag`(唯一日志出口)。码表稳定、可计数、
//! 可进 golden、可直链 `docs/divergences/`。

use yu_log::{Boundary, Diag, Severity};

/// 稳定错误码表(`sv.*` 命名空间)。
pub mod code {
    /// 存档/文件域命令未实现或未取证(SAVEINFO/图层导出等)。
    pub const COMMAND_UNIMPLEMENTED: &str = "sv.command.unimplemented";
    /// 命令名不在工程 ysc 表里。
    pub const COMMAND_UNKNOWN: &str = "sv.command.unknown";
    /// 参数类型不符(字符串/数值)。
    pub const ARG_TYPE: &str = "sv.arg.type";
    /// 参数缺失(DNO/FILE 等)。
    pub const ARG_MISSING: &str = "sv.arg.missing";
    /// 参数值越界(DNO 1..=1024 等)。
    pub const ARG_RANGE: &str = "sv.arg.range";
    /// 存档文件不存在(LOAD/FILEACT 目标)。
    pub const FILE_MISSING: &str = "sv.file.missing";
    /// 存档文件读取失败。
    pub const FILE_READ: &str = "sv.file.read";
    /// 存档文件写入失败(预留;当前写路径被忠实性门禁拒绝,见
    /// `WRITE_REFUSED`)。
    pub const FILE_WRITE: &str = "sv.file.write";
    /// 文件/空文件夹已删除(信息级事实,不是未实现)。
    pub const FILE_DELETED: &str = "sv.file.deleted";
    /// YSSD 容器解析失败(载荷来自 `yu-format::yssd` 的诊断)。
    pub const CONTAINER: &str = "sv.container";
    /// 图像块形状不符(SAVE[ID] 像素数 ≠ w*h*4,或块内段长自证失败)。
    pub const IMAGE_SHAPE: &str = "sv.image.shape";
    /// 路径越界:目标解析后不在 save 根内(安全拒绝,不猜)。
    pub const PATH_ESCAPE: &str = "sv.path.escape";
    /// 写盘拒绝:`SAVE[SET/LET]` 的变量引用身份在 `DomainCall` 事件层丢失
    /// (DIV-RT-042),据此写不出与官方一致的块 ⇒ 宁可不写,不猜。
    pub const WRITE_REFUSED: &str = "sv.write.refused";
    /// `FILEACT[EXEC]` 拒绝:宿主不执行/不关联打开外部程序(无引擎语义
    /// 取证 + 安全策略)。
    pub const EXEC_REFUSED: &str = "sv.exec.refused";
    /// 内部不变量被破坏(应为不可能)。
    pub const INTERNAL: &str = "sv.internal";
    /// 诊断达到硬上限,最旧项被丢弃。
    pub const LOG_TRUNCATED: &str = "sv.log.truncated";
}

/// 全部稳定码(唯一性单测用)。
pub const ALL_CODES: &[&str] = &[
    code::COMMAND_UNIMPLEMENTED,
    code::COMMAND_UNKNOWN,
    code::ARG_TYPE,
    code::ARG_MISSING,
    code::ARG_RANGE,
    code::FILE_MISSING,
    code::FILE_READ,
    code::FILE_WRITE,
    code::FILE_DELETED,
    code::CONTAINER,
    code::IMAGE_SHAPE,
    code::PATH_ESCAPE,
    code::WRITE_REFUSED,
    code::EXEC_REFUSED,
    code::INTERNAL,
    code::LOG_TRUNCATED,
];

pub fn fatal(code: &'static str, detail: impl Into<String>) -> Diag {
    Diag::new(code, Severity::Fatal, Boundary::Domain).with_detail(detail)
}

pub fn recoverable(code: &'static str, detail: impl Into<String>) -> Diag {
    Diag::new(code, Severity::Recoverable, Boundary::Domain).with_detail(detail)
}

pub fn unimplemented(code: &'static str, detail: impl Into<String>) -> Diag {
    Diag::new(code, Severity::Unimplemented, Boundary::Domain).with_detail(detail)
}

pub fn info(code: &'static str, detail: impl Into<String>) -> Diag {
    Diag::new(code, Severity::Info, Boundary::Domain).with_detail(detail)
}

/// 最低档:**只在 `--log-level verbose` 时显示**。
///
/// 用于「正常但值得留痕」的事实(幂等空操作等)。默认输出里出现只会
/// 训练使用者忽略日志;需要排查时手动打开。
pub fn verbose(code: &'static str, detail: impl Into<String>) -> Diag {
    Diag::new(code, Severity::Verbose, Boundary::Domain).with_detail(detail)
}

/// 给诊断补命令名/帧号(宿主只拿得到事件帧号)。
pub fn at_command(mut diag: Diag, frame: u64, command: &str) -> Diag {
    diag.where_ = yu_log::Loc::script(
        format!("sv@frame{frame}"),
        None,
        0,
        Some(command.to_string()),
    );
    diag
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn codes_are_unique_and_namespaced() {
        let mut seen = BTreeSet::new();
        for code in ALL_CODES {
            assert!(code.starts_with("sv."), "code outside sv.*: {code}");
            assert!(seen.insert(*code), "duplicate code: {code}");
        }
        assert_eq!(seen.len(), ALL_CODES.len());
    }
}
