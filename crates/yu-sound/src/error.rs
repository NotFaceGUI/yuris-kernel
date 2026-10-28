//! `snd.*` 诊断码与构造助手。
//!
//! 所有错误/未实现都走 `yu-log::Diag`（唯一日志出口）。码表稳定、可计数、
//! 可进 golden、可直链 `docs/divergences/DIV-SND-*`。

use yu_log::{Boundary, Diag, Severity};

/// 稳定错误码表（`snd.*` 命名空间）。
pub mod code {
    /// 声音域命令未实现/未取证（未知命令；MIDI 族/CDDA 是官方空桩
    /// aligned-stub，不走此码）。
    pub const COMMAND_UNIMPLEMENTED: &str = "snd.command.unimplemented";
    /// 命令名不在工程 ysc 表里。
    pub const COMMAND_UNKNOWN: &str = "snd.command.unknown";
    /// 参数类型不符（字符串/数值）。
    pub const ARG_TYPE: &str = "snd.arg.type";
    /// 参数值越界（音量/声像/音调/时间等）。
    pub const ARG_RANGE: &str = "snd.arg.range";
    /// 参数槽缺失或命令窗口不存在。
    pub const ARG_MISSING: &str = "snd.arg.missing";
    /// 层不存在（更新型命令/查询目标的通用提示）。
    pub const LAYER_MISSING: &str = "snd.layer.missing";
    /// 层 ID 为空或非法。
    pub const LAYER_INVALID_ID: &str = "snd.layer.invalid_id";
    /// 装载器没有找到文件（含扩展名探测全部失败）。
    pub const FILE_MISSING: &str = "snd.file.missing";
    /// 文件读取失败（装载器错误）。
    pub const FILE_READ: &str = "snd.file.read";
    /// 解码失败（OGG/WAVE 结构损坏）。
    pub const DECODE_FAILED: &str = "snd.decode.failed";
    /// 声音窗口欠载(按需解码未覆盖播放光标;已计数,附首次现场)。
    pub const STREAM_UNDERRUN: &str = "snd.stream.underrun";
    /// 编码不受支持（ACM WAVE，或未知容器）。
    pub const CODEC_UNSUPPORTED: &str = "snd.codec.unsupported";
    /// 查询字段未取证/未实现。
    pub const QUERY_UNSUPPORTED: &str = "snd.query.unsupported";
    /// 多字段同调时按引擎固定检查顺序写回。
    /// `EXIST..PITCH` 短路；其余字段最后写入者胜出。仅作 Info 记录，不改变行为。
    pub const QUERY_MULTI: &str = "snd.query.multi";
    /// 设备输出未装配（headless 路径不产出音频）。
    pub const DEVICE_ABSENT: &str = "snd.device.absent";
    /// 内部不变量被破坏（应为不可能）。
    pub const INTERNAL: &str = "snd.internal";
    /// 诊断达到硬上限，最旧项被丢弃。
    pub const LOG_TRUNCATED: &str = "snd.log.truncated";
}

/// 全部稳定码（唯一性单测用）。
pub const ALL_CODES: &[&str] = &[
    code::COMMAND_UNIMPLEMENTED,
    code::COMMAND_UNKNOWN,
    code::ARG_TYPE,
    code::ARG_RANGE,
    code::ARG_MISSING,
    code::LAYER_MISSING,
    code::LAYER_INVALID_ID,
    code::FILE_MISSING,
    code::FILE_READ,
    code::DECODE_FAILED,
    code::STREAM_UNDERRUN,
    code::CODEC_UNSUPPORTED,
    code::QUERY_UNSUPPORTED,
    code::QUERY_MULTI,
    code::DEVICE_ABSENT,
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

/// 给诊断补命令名/帧号（宿主只拿得到事件帧号）。
pub fn at_command(mut diag: Diag, frame: u64, command: &str) -> Diag {
    diag.where_ = yu_log::Loc::script(
        format!("snd@frame{frame}"),
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
            assert!(code.starts_with("snd."), "code outside snd.*: {code}");
            assert!(seen.insert(*code), "duplicate code: {code}");
        }
        assert_eq!(seen.len(), ALL_CODES.len());
    }
}
