//! 日志级别的**唯一**解析入口(跨平台)。
//!
//! 同一组用户可见取值(`quiet|error|warn|info|debug|trace|verbose`)在三个
//! 位置消费,以前各写一份解析:
//!
//! | 消费点 | 以前 | 现在 |
//! |---|---|---|
//! | CLI 全局旗标 `--log-level` | `yu-cli::main::extract_globals` 自己 match | [`LogLevel::parse`] |
//! | 内核诊断门槛 | [`crate::console::MinLevel::from_level_value`] | [`LogLevel::min_level`] |
//! | 依赖门面桥 | `facade::FacadeLevel::parse` | [`LogLevel::facade_level`] |
//!
//! 三处各写一份的直接后果是**取值集合不一致**:Android 宿主的
//! `run-config.json` 拒绝 `debug`/`trace`(CLI 接受),而 `verbose` 在门面
//! 侧根本没有对应项 —— `--log-level verbose` 会让依赖日志(wgpu/winit 等)
//! 从 `info` 档**倒挂**回 `warn` 档,越调越少。现在只有这一份。
//!
//! # 两档语义并不完全同步(有意)
//!
//! `MinLevel` 管**内核 `Diag`** 的可见性,`FacadeLevel` 管**依赖经 `log`
//! 发的行**;两者粒度不同,不能强行一一对应:
//!
//! - `quiet|error|warn` 都压掉内核 `Info`/`Verbose`(内核只有"要不要看
//!   正常流水"这一档),但门面仍按各自档位放行;
//! - `debug`/`trace` 让内核看到 `Info`(内核没有 debug/trace 级),门面
//!   分别放行到 `Debug`/`Trace`;
//! - `verbose` 是内核**独有**的最低档(显示 `Severity::Verbose`),门面
//!   取最低档 `Trace`,保证"开了 verbose 后依赖日志只多不少"。

use crate::console::MinLevel;
use crate::facade::FacadeLevel;

/// 用户可见的日志级别(`--log-level` / `YU_LOG` / 宿主 `run-config.json`)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    /// 只留需要动作的条目(`quiet` = `warn` 的门面档,内核诊断同级)。
    Quiet,
    Error,
    Warn,
    /// 缺省档。
    Info,
    Debug,
    Trace,
    /// 最低档:额外显示 `Severity::Verbose`(幂等空操作等)。
    Verbose,
}

impl LogLevel {
    /// 全部合法取值(按由紧到松排列;错误文案与 help 共用)。
    pub const ALL: [LogLevel; 7] = [
        LogLevel::Quiet,
        LogLevel::Error,
        LogLevel::Warn,
        LogLevel::Info,
        LogLevel::Debug,
        LogLevel::Trace,
        LogLevel::Verbose,
    ];

    /// 解析用户取值(大小写不敏感,允许首尾空白)。空串按缺省处理。
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" => Ok(LogLevel::Info),
            "quiet" => Ok(LogLevel::Quiet),
            "error" => Ok(LogLevel::Error),
            "warn" => Ok(LogLevel::Warn),
            "info" => Ok(LogLevel::Info),
            "debug" => Ok(LogLevel::Debug),
            "trace" => Ok(LogLevel::Trace),
            "verbose" => Ok(LogLevel::Verbose),
            other => Err(format!(
                "log_level 只接受 {},收到 `{other}`",
                Self::ALL
                    .iter()
                    .map(|level| level.as_str())
                    .collect::<Vec<_>>()
                    .join("|")
            )),
        }
    }

    /// 解析 `Option<&str>`;`None`/非法值给缺省档(`info`)。
    ///
    /// 给环境变量这类"读不到就用缺省"的入口用,不静默吞掉合法值。
    #[must_use]
    pub fn from_value_or_default(value: Option<&str>) -> Self {
        value.and_then(|raw| Self::parse(raw).ok()).unwrap_or(Self::Info)
    }

    /// 规范名(诊断/配置回写用)。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            LogLevel::Quiet => "quiet",
            LogLevel::Error => "error",
            LogLevel::Warn => "warn",
            LogLevel::Info => "info",
            LogLevel::Debug => "debug",
            LogLevel::Trace => "trace",
            LogLevel::Verbose => "verbose",
        }
    }

    /// 内核 `Diag` 门槛。
    #[must_use]
    pub const fn min_level(self) -> MinLevel {
        match self {
            LogLevel::Quiet | LogLevel::Error | LogLevel::Warn => MinLevel::Quiet,
            LogLevel::Info | LogLevel::Debug | LogLevel::Trace => MinLevel::Normal,
            LogLevel::Verbose => MinLevel::Verbose,
        }
    }

    /// 依赖门面档(见模块文档的"两档语义并不完全同步")。
    #[must_use]
    pub const fn facade_level(self) -> FacadeLevel {
        match self {
            LogLevel::Quiet | LogLevel::Warn => FacadeLevel::Warn,
            LogLevel::Error => FacadeLevel::Error,
            LogLevel::Info => FacadeLevel::Info,
            LogLevel::Debug => FacadeLevel::Debug,
            LogLevel::Trace | LogLevel::Verbose => FacadeLevel::Trace,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_documented_value() {
        for level in LogLevel::ALL {
            assert_eq!(LogLevel::parse(level.as_str()), Ok(level));
        }
        assert_eq!(LogLevel::parse("  VERBOSE "), Ok(LogLevel::Verbose));
        assert_eq!(LogLevel::parse(""), Ok(LogLevel::Info), "空串 = 缺省");
    }

    #[test]
    fn rejects_unknown_and_lists_choices_once() {
        let error = LogLevel::parse("loud").unwrap_err();
        assert!(error.contains("verbose"), "错误文案要列全取值: {error}");
        assert_eq!(LogLevel::from_value_or_default(Some("loud")), LogLevel::Info);
    }

    #[test]
    fn verbose_is_the_lowest_facade_level_not_a_downgrade() {
        // 曾经的 bug:`verbose` 在门面侧没有对应项 ⇒ 落回 `warn`,
        // 比缺省 `info` 还紧。开 verbose 必须只多不少。
        assert_eq!(LogLevel::Verbose.facade_level(), FacadeLevel::Trace);
        assert_eq!(LogLevel::Trace.facade_level(), FacadeLevel::Trace);
        assert_eq!(LogLevel::Info.facade_level(), FacadeLevel::Info);
        assert_eq!(LogLevel::Quiet.facade_level(), FacadeLevel::Warn);
    }

    #[test]
    fn kernel_threshold_matches_legacy_mapping() {
        assert_eq!(LogLevel::Quiet.min_level(), MinLevel::Quiet);
        assert_eq!(LogLevel::Warn.min_level(), MinLevel::Quiet);
        assert_eq!(LogLevel::Info.min_level(), MinLevel::Normal);
        assert_eq!(LogLevel::Trace.min_level(), MinLevel::Normal);
        assert_eq!(LogLevel::Verbose.min_level(), MinLevel::Verbose);
    }
}
