//! Android 独立调试日志:**单一日志器,双写 logcat + 文件**。
//!
//! # 为什么自建
//!
//! `log` crate 只允许安装**一个**全局 logger。宿主此前混用了两套:
//!
//! - `android_logger`(写 logcat,是 Android 上唯一可见的通道);
//! - `yu_log::facade`(用 `log::set_boxed_logger` 抢占同一个位置,只写
//!   **stderr** —— 而 Android 上 Rust 的 stderr 不进 logcat)。
//!
//! 两者互相抢占的结果是「谁后装谁生效」:`[spike]`/`[summary]`/`[cgdiff]`
//! 这些诊断时有时无,排查成本极高(实测多轮全是空日志)。
//!
//! 本模块用**一个** logger 同时做两件事:
//!
//! 1. 调 `__android_log_write` 写 logcat(固定 tag `yu-kernel`);
//! 2. 追加写 `<游戏目录>/debug/yu-kernel.log`(纯文本,`BufWriter`,
//!    每 `FLUSH_EVERY` 行 flush 一次)。
//!
//! 不再依赖 `android_logger` 与 `yu_log::facade`,也没有 stderr 路径。
//!
//! # 门槛与开关
//!
//! `init()` 只装一次;`min_level` 由调用方给(发布模式 `Warn`,诊断模式
//! `Info`)。文件写入在 `min_level` 之外**额外**接受 `Verbose` 以上——
//! 诊断开关打开时能看到细节,发布模式也不会有额外开销(文件写者仅在
//! 显式开启「调试日志」时登记)。
//!
//! # 线程安全
//!
//! `log` 会从多个线程调用(渲染线程、音频监督线程)。文件写者用
//! `Mutex` 串行化;logcat 的 `__android_log_write` 本身线程安全。

#![allow(unsafe_code)]

use std::io::Write as _;
use std::sync::Mutex;

/// logcat tag,**含结尾 NUL**(编译期拼好,写日志路径零分配)。
///
/// `__android_log_write` 要 C 字符串;每次写日志现拼一个 `Vec` 纯属浪费,
/// 而这条路径每帧都可能走。
#[cfg(target_os = "android")]
const TAG: &[u8] = b"yu-kernel\0";
/// 每写这么多行落一次盘(BufWriter,掉电最多丢 ~120 行)。
const FLUSH_EVERY: u32 = 120;

/// 单一日志器。
struct AndroidLogger {
    /// logcat 的最低级别。
    min: log::Level,
    /// 文件写者(未开启「调试日志」时为 `None`)。
    file: Mutex<Option<FileSink>>,
}

struct FileSink {
    writer: std::io::BufWriter<std::fs::File>,
    /// 自上次 flush 以来写入的行数。
    pending: u32,
}

impl log::Log for AndroidLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= self.min
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let level = record.level();
        // 正文 = `<target>: <message>`(与 logcat 常规习惯一致)。
        let body = format!("{}: {}", record.target(), record.args());
        write_logcat(level, &body);
        if let Ok(mut guard) = self.file.lock() {
            if let Some(sink) = guard.as_mut() {
                let _ = writeln!(
                    sink.writer,
                    "[{}] {} {}",
                    level_mark(level),
                    timestamp_ms(),
                    body
                );
                sink.pending += 1;
                if sink.pending >= FLUSH_EVERY {
                    let _ = sink.writer.flush();
                    sink.pending = 0;
                }
            }
        }
    }

    fn flush(&self) {
        if let Ok(mut guard) = self.file.lock() {
            if let Some(sink) = guard.as_mut() {
                let _ = sink.writer.flush();
                sink.pending = 0;
            }
        }
    }
}

/// 写一条到 logcat。
///
/// `__android_log_write(prio, tag, text)`:`prio` 是 ANDROID_LOG_* 值
/// (2=VERBOSE,3=DEBUG,4=INFO,5=WARN,6=ERROR),`tag`/`text` 需 NUL 结尾。
#[cfg(target_os = "android")]
fn write_logcat(level: log::Level, body: &str) {
    let prio = match level {
        log::Level::Error => 6,
        log::Level::Warn => 5,
        log::Level::Info => 4,
        log::Level::Debug => 3,
        log::Level::Trace => 2,
    };
    // C 字符串:正文可能含 NUL(极少),先剔除,避免 CStr 截断。
    let mut text = body.replace('\0', " ").into_bytes();
    text.push(0);
    // SAFETY: 两个指针都指向本函数内/编译期有效的 NUL 结尾字节缓冲;
    // `__android_log_write` 只读取它们,不保留引用。
    unsafe {
        ndk_sys::__android_log_write(
            prio,
            TAG.as_ptr().cast::<std::ffi::c_char>(),
            text.as_ptr().cast::<std::ffi::c_char>(),
        );
    }
}

/// 非 Android 平台:写到 stderr(仅开发机测试路径)。
#[cfg(not(target_os = "android"))]
fn write_logcat(level: log::Level, body: &str) {
    eprintln!("[{level}] {}", body);
}

/// 级别单字符标记(文件行首,便于 grep)。
fn level_mark(level: log::Level) -> &'static str {
    match level {
        log::Level::Error => "E",
        log::Level::Warn => "W",
        log::Level::Info => "I",
        log::Level::Debug => "D",
        log::Level::Trace => "T",
    }
}

/// 自进程启动以来的毫秒数(定位「第几秒发生」足够;不做日期格式化)。
fn timestamp_ms() -> u128 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_millis()
}

/// 全局 logger 句柄(供 `flush` / 追加文件路径)。
static LOGGER: Mutex<Option<&'static AndroidLogger>> = Mutex::new(None);

/// 安装日志器(**只装一次**;重复调用是温和 no-op)。
///
/// `min`:logcat 门槛(发布模式 `Warn`,诊断模式 `Info`)。
pub fn init(min: log::Level) {
    use std::sync::Once;
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        // 必须显式设 `max_level`:`log` crate 在编译期按它做静态过滤,
        // 未设置时是 `Off`,所有 `log::warn!` 会被优化成空操作。
        log::set_max_level(min.to_level_filter());
        let logger: &'static AndroidLogger = Box::leak(Box::new(AndroidLogger {
            min,
            file: Mutex::new(None),
        }));
        if log::set_boxed_logger(Box::new(&*logger)).is_ok() {
            // `set_boxed_logger` 拿走 Box,这里只留静态引用给它做 flush。
            if let Ok(mut slot) = LOGGER.lock() {
                *slot = Some(logger);
            }
        }
    });
}

/// 登记文件写者(开启「调试日志」时调用)。重复调用会替换旧文件。
pub fn set_file(path: &std::path::Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let sink = FileSink {
        writer: std::io::BufWriter::with_capacity(64 * 1024, file),
        pending: 0,
    };
    if let Ok(slot) = LOGGER.lock() {
        if let Some(logger) = *slot {
            if let Ok(mut guard) = logger.file.lock() {
                if let Some(old) = guard.as_mut() {
                    let _ = old.writer.flush();
                }
                *guard = Some(sink);
            }
        }
    }
    Ok(())
}

/// 把文件缓冲落盘(退出、panic、以及宿主每次取状态时调用)。
pub fn flush() {
    use log::Log as _;
    if let Ok(slot) = LOGGER.lock() {
        if let Some(logger) = *slot {
            logger.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_marks_are_single_chars() {
        for level in [
            log::Level::Error,
            log::Level::Warn,
            log::Level::Info,
            log::Level::Debug,
            log::Level::Trace,
        ] {
            assert_eq!(level_mark(level).len(), 1);
        }
    }
}
