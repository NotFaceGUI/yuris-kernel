//! `log` 门面桥:把依赖(wgpu/winit 等)经 `log` 发的诊断接进本包结构。
//!
//! 现状背景(任务书 P4 M-E):全工作区此前没有任何 logger 初始化,依赖
//! 经 `log` 发的 error/warning 全部被静默吞掉。本模块提供唯一的桥:
//!
//! - `log::Record` → JSONL 一行(`sub:"log"`,字段固定:
//!   `level/target/file/line/message`,复用 [`crate::json`] 的确定性转义),
//!   以及控制台一行(经调用方注入的回调;控制台格式统一走
//!   [`crate::console`]——`[  +s] LEVEL [log-facade] target: message`,
//!   Q1 起不再用旧 `[log:warn]` 前缀);
//! - 级别映射 `Error/Warn/Info/Debug/Trace` 原样保留为小写字符串;
//!   **本包自身的诊断码/`Diag` schema 不受影响**(`sub:"log"` 行不是
//!   `Diag`,不走 [`crate::sink::LogSink`] 的往返契约);
//! - 级别由 `YU_LOG`/`--log-level` 控制
//!   (`error|warn|info|debug|trace|quiet`,缺省 `warn`;`quiet` 按 `warn`
//!   处理——它压的是内核 Info 行,不是门面行;非法值按 `warn` 并提示)。
//!   **不解析 `RUST_LOG`**:避免隐式全局 per-crate 过滤语义(A3)。
//!
//! 全局性:`log::set_boxed_logger` 是进程级单例,本桥只应由 CLI/宿主
//! main 入口显式调用一次;wasm 宿主不调用。桥自身不加任何过滤以外的
//! 状态,`.sd`/容器解码等纯数据路径不感知它。

use std::sync::Arc;

use crate::console::JsonlFile;

/// 控制台一行回调(由宿主注入;级别/目标/消息三件套,格式化统一在
/// [`crate::console`] 完成)。main 入口传无状态函数,桌面模式传闭包经
/// [`crate::console::ConsoleHandle`] 共享同一个 Console(聚合/文件/面板)。
pub type ConsoleFn = Arc<dyn Fn(FacadeLevel, &str, &str) + Send + Sync>;

/// 门面日志级别(与 `log::Level` 一一对应)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FacadeLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl FacadeLevel {
    pub const fn as_str(self) -> &'static str {
        match self {
            FacadeLevel::Error => "error",
            FacadeLevel::Warn => "warn",
            FacadeLevel::Info => "info",
            FacadeLevel::Debug => "debug",
            FacadeLevel::Trace => "trace",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "error" => Some(FacadeLevel::Error),
            "warn" | "quiet" => Some(FacadeLevel::Warn),
            "info" => Some(FacadeLevel::Info),
            "debug" => Some(FacadeLevel::Debug),
            "trace" => Some(FacadeLevel::Trace),
            _ => None,
        }
    }

    fn from_log(level: log::Level) -> Self {
        match level {
            log::Level::Error => FacadeLevel::Error,
            log::Level::Warn => FacadeLevel::Warn,
            log::Level::Info => FacadeLevel::Info,
            log::Level::Debug => FacadeLevel::Debug,
            log::Level::Trace => FacadeLevel::Trace,
        }
    }

    fn to_log(self) -> log::Level {
        match self {
            FacadeLevel::Error => log::Level::Error,
            FacadeLevel::Warn => log::Level::Warn,
            FacadeLevel::Info => log::Level::Info,
            FacadeLevel::Debug => log::Level::Debug,
            FacadeLevel::Trace => log::Level::Trace,
        }
    }
}

/// 从 `YU_LOG` 解析级别;缺省/非法为 `warn`(`quiet` 同 `warn`)。
#[must_use]
pub fn level_from_env(value: Option<&str>) -> FacadeLevel {
    value
        .and_then(FacadeLevel::parse)
        .unwrap_or(FacadeLevel::Warn)
}

/// 一条 `sub:"log"` 记录的确定性 JSON(字段序固定;`record_json_line` 与
/// 生命周期行的文件落盘共用)。
pub(crate) fn json_line(
    level: &str,
    target: &str,
    file: Option<&str>,
    line: Option<u32>,
    message: &str,
) -> String {
    let mut out = String::with_capacity(128);
    out.push_str("{\"sub\":\"log\",\"level\":\"");
    out.push_str(level);
    out.push_str("\",\"target\":");
    crate::json::write_string(&mut out, target);
    out.push_str(",\"file\":");
    match file {
        Some(file) => crate::json::write_string(&mut out, file),
        None => out.push_str("null"),
    }
    out.push_str(",\"line\":");
    match line {
        Some(line) => out.push_str(&line.to_string()),
        None => out.push_str("null"),
    }
    out.push_str(",\"message\":");
    crate::json::write_string(&mut out, message);
    out.push('}');
    out
}

/// 门面桥。`log::Log` 的实现;JSONL 目标可选(不设则只走控制台回调)。
///
/// JSONL 经共享 [`JsonlFile`](一个文件句柄,进程内唯一写者;桌面模式下
/// 内核 Diag 经同一个对象落 `sub:"diag"` 行,Q1 R4 单写者契约)。
pub struct FacadeLogger {
    level: FacadeLevel,
    jsonl: Option<JsonlFile>,
    console: ConsoleFn,
}

impl FacadeLogger {
    /// 构造(不安装;安装走 [`init`]/[`init_with`])。
    #[must_use]
    pub fn new(level: FacadeLevel, jsonl: Option<JsonlFile>, console: ConsoleFn) -> Self {
        Self {
            level,
            jsonl,
            console,
        }
    }

    /// 一条记录的 JSONL 单行(确定性字段序;单测也用)。
    pub fn record_json_line(&self, record: &log::Record<'_>) -> String {
        let mut message = String::new();
        // `log::Record` 的消息只经 `fmt::Display`;写不进内存缓冲是不可能的
        // (String 的 Write 永不失败),与 `log` 生态所有自定义 logger 同构。
        let _ = std::fmt::write(&mut message, *record.args());
        json_line(
            FacadeLevel::from_log(record.level()).as_str(),
            record.target(),
            record.file(),
            record.line(),
            &message,
        )
    }

    /// 回调收到的三件套(单测也用;格式化归 [`crate::console`])。
    pub fn console_parts(&self, record: &log::Record<'_>) -> (FacadeLevel, String, String) {
        let mut message = String::new();
        let _ = std::fmt::write(&mut message, *record.args());
        (
            FacadeLevel::from_log(record.level()),
            record.target().to_string(),
            message,
        )
    }
}

impl log::Log for FacadeLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= self.level.to_log()
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let (level, target, message) = self.console_parts(record);
        (self.console)(level, &target, &message);
        let line = json_line(level.as_str(), &target, record.file(), record.line(), &message);
        if let Some(file) = &self.jsonl {
            file.write_raw(&line);
        }
    }

    fn flush(&self) {
        // `JsonlFile` 内部是无缓冲 `File`,write_all 即达盘队列;无额外刷点。
    }
}

/// 初始化全局门面桥(main 入口调用一次;重复调用是温和 no-op)。
///
/// - `YU_LOG`:级别(`error|warn|info|debug|trace|quiet`;缺省 `warn`);
/// - `YU_LOG_JSONL`:可选 JSONL 文件路径(每次进程创建/截断)。
///
/// 返回实际生效的级别,供 CLI 报告;安装失败(已被占用)返回 `None`。
pub fn init(console: ConsoleFn) -> Option<FacadeLevel> {
    let jsonl = match std::env::var("YU_LOG_JSONL") {
        Ok(path) if !path.trim().is_empty() => match JsonlFile::create(path.trim()) {
            Ok(file) => Some(file),
            // 用户显式要了文件却打不开:必须留痕,不许静默禁用文件日志
            // (Q1 自查 a 类;tyn「静默无色」同类教训)。
            Err(error) => {
                (console)(
                    FacadeLevel::Warn,
                    "yu-log",
                    &format!(
                        "YU_LOG_JSONL `{}` 打开失败({error});文件日志未启用",
                        path.trim()
                    ),
                );
                None
            }
        },
        _ => None,
    };
    init_with(console, jsonl)
}

/// 显式注入 JSONL 目标的 [`init`](桌面模式与 main 共享同一 [`JsonlFile`]
/// 时用;不再读 `YU_LOG_JSONL`)。
pub fn init_with(console: ConsoleFn, jsonl: Option<JsonlFile>) -> Option<FacadeLevel> {
    let level = level_from_env(std::env::var("YU_LOG").ok().as_deref());
    let logger = FacadeLogger::new(level, jsonl, console);
    log::set_max_level(level.to_log().to_level_filter());
    // 已有 logger 时(`--repeat` 的进程内重入/测试并行)保持现状,不覆盖。
    if log::set_boxed_logger(Box::new(logger)).is_err() {
        return None;
    }
    Some(level)
}

#[cfg(test)]
mod tests {
    use super::*;
    use log::Log as _;
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static CONSOLE_HITS: AtomicUsize = AtomicUsize::new(0);
    thread_local! {
        static LAST: RefCell<Option<(FacadeLevel, String, String)>> = const { RefCell::new(None) };
    }

    fn grab_console(level: FacadeLevel, target: &str, message: &str) {
        LAST.with(|slot| {
            *slot.borrow_mut() = Some((level, target.to_string(), message.to_string()));
        });
        CONSOLE_HITS.fetch_add(1, Ordering::SeqCst);
    }

    /// `format_args!` 的临时值只能活到语句末尾,所以记录构造必须内联。
    macro_rules! test_record {
        ($level:expr, $target:expr, $message:expr) => {
            log::Record::builder()
                .level($level)
                .target($target)
                .file(Some("src/lib.rs"))
                .line(Some(42))
                .args(format_args!($message))
                .build()
        };
    }

    #[test]
    fn env_level_parses_with_warn_default() {
        assert_eq!(level_from_env(None), FacadeLevel::Warn);
        assert_eq!(level_from_env(Some("trace")), FacadeLevel::Trace);
        assert_eq!(level_from_env(Some("WARN")), FacadeLevel::Warn);
        assert_eq!(level_from_env(Some("quiet")), FacadeLevel::Warn);
        assert_eq!(level_from_env(Some("nonsense")), FacadeLevel::Warn);
    }

    #[test]
    fn record_line_has_stable_shape_and_escaping() {
        let logger = FacadeLogger::new(FacadeLevel::Trace, None, Arc::new(grab_console));
        let line = logger.record_json_line(&test_record!(
            log::Level::Warn,
            "wgpu::backend",
            "quote \" and \\ slash"
        ));
        assert_eq!(
            line,
            "{\"sub\":\"log\",\"level\":\"warn\",\"target\":\"wgpu::backend\",\
             \"file\":\"src/lib.rs\",\"line\":42,\
             \"message\":\"quote \\\" and \\\\ slash\"}"
        );
        let parsed = crate::json::parse(&line).expect("jsonl parses");
        let obj = crate::json::as_obj(&parsed).expect("object");
        assert_eq!(
            crate::json::as_str(crate::json::get(obj, "sub").expect("sub")).expect("str"),
            "log"
        );
    }

    #[test]
    fn level_filter_gates_console_and_jsonl() {
        let _ = CONSOLE_HITS.load(Ordering::SeqCst);
        let logger = FacadeLogger::new(FacadeLevel::Warn, None, Arc::new(grab_console));
        logger.log(&test_record!(log::Level::Info, "wgpu", "filtered out"));
        assert_eq!(CONSOLE_HITS.load(Ordering::SeqCst), 0, "info filtered");
        logger.log(&test_record!(log::Level::Error, "wgpu", "kept"));
        assert_eq!(CONSOLE_HITS.load(Ordering::SeqCst), 1, "error kept");
        assert!(logger.enabled(&log::Metadata::builder().level(log::Level::Warn).build()));
        assert!(!logger.enabled(&log::Metadata::builder().level(log::Level::Debug).build()));
    }

    #[test]
    fn console_parts_carry_level_target_message() {
        let logger = FacadeLogger::new(FacadeLevel::Warn, None, Arc::new(grab_console));
        let (level, target, message) = logger.console_parts(&test_record!(log::Level::Error, "naga", "bad shader"));
        assert_eq!((level.as_str(), target.as_str(), message.as_str()), ("error", "naga", "bad shader"));
    }

    #[test]
    fn json_line_escapes_and_orders_fields() {
        let line = json_line("warn", "t", None, None, "a\"b");
        assert_eq!(
            line,
            "{\"sub\":\"log\",\"level\":\"warn\",\"target\":\"t\",\"file\":null,\"line\":null,\"message\":\"a\\\"b\"}"
        );
    }
}
