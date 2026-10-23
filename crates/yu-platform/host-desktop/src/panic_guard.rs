//! 宿主 panic 防线:捕获、留痕、冻结,而不是让进程死掉。
//!
//! # 为什么在宿主层做
//!
//! 引擎/渲染热路径存在 panic 面(音频锁中毒、wgpu surface 丢失、
//! `yu-gfx` 的 `expect("checked")` 等)。净室纪律要求这些语义缺陷在
//! 各自的 crate 里修(见 P1 台账交接件);但在修好之前,桌面宿主必须
//! **先保住现场**:
//!
//! 1. panic 不再穿出事件循环杀掉进程;
//! 2. panic message + 位置 + backtrace 摘要写进日志文件(经 `log` 门面,
//!    与 wgpu/winit 的 `log::error!` 同一个 sink);
//! 3. 引擎进入**冻结**状态:停止推进,保留最后一帧;
//! 4. 窗口显示红色横幅,按 Esc / 点 Quit 退出。
//!
//! # 边界
//!
//! 本模块不解释 panic 的语义,也不重试:一次 panic = 该运行代际不可信
//! (锁中毒、渲染器内部状态未知),冻结是唯一诚实的选择。恢复靠重启
//! (`Restart` 按钮会丢失当前进度,所以不在横幅上提供自动重启)。

use std::cell::RefCell;
use std::panic::{self, AssertUnwindSafe};
use std::sync::Once;
use std::time::Instant;

/// panic 发生的阶段(写进日志与横幅)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// `Engine::tick_realtime()`(脚本/声音/渲染)。
    Tick,
    /// 窗口呈现路径(上传/egui/提交)。
    Draw,
    /// 宿主人为注入的自检(`--panic-selftest`)。
    SelfTest,
}

impl Phase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Phase::Tick => "tick",
            Phase::Draw => "draw",
            Phase::SelfTest => "selftest",
        }
    }
}

/// 一次被捕获的 panic。
#[derive(Clone, Debug)]
pub struct PanicRecord {
    pub phase: Phase,
    /// 宿主帧号(panic 发生前已完成的 tick 数)。
    pub host_frame: u64,
    /// 引擎逻辑帧号。
    pub engine_frame: u64,
    pub message: String,
    /// `file:line:col`(panic 宏的调用点)。
    pub location: String,
    pub thread: String,
    /// backtrace 摘要(最多 [`BACKTRACE_LINES`] 行)。
    pub backtrace: String,
}

/// 日志里保留的 backtrace 行数(完整栈对排障冗余,摘要足够定位)。
pub const BACKTRACE_LINES: usize = 16;

thread_local! {
    /// panic hook 在**当前线程**记录;`catch` 在同一个线程读走。
    static LAST_PANIC: RefCell<Option<PanicRecord>> = const { RefCell::new(None) };
}

static INSTALL: Once = Once::new();

/// 安装全局 panic hook(进程内只装一次;重复调用是 no-op)。
///
/// hook 只做三件不会失败的事:取消息/位置、抓 backtrace 摘要、写一行
/// `log::error!`。它**不**调用默认 hook(默认 hook 打 stderr,绕过项目
/// 的 `yu_cli::output` 纪律且会重复)。写日志本身包在 `catch_unwind` 里,
/// 保证“panic 中再 panic”不会变成双重 abort。
pub fn install() {
    INSTALL.call_once(|| {
        panic::set_hook(Box::new(move |info| {
            let record = PanicRecord {
                phase: Phase::Tick, // 由 `catch` 用实际阶段覆盖
                host_frame: 0,
                engine_frame: 0,
                message: payload_message(info.payload()),
                location: info
                    .location()
                    .map(|loc| format!("{}:{}:{}", loc.file(), loc.line(), loc.column()))
                    .unwrap_or_else(|| "unknown".to_string()),
                thread: std::thread::current()
                    .name()
                    .unwrap_or("<unnamed>")
                    .to_string(),
                backtrace: backtrace_summary(),
            };
            let _ = panic::catch_unwind(AssertUnwindSafe(|| {
                log::error!(
                    target: "yu-desktop",
                    "panic thread={} at {} msg={} backtrace={}",
                    record.thread,
                    record.location,
                    record.message,
                    record.backtrace.replace('\n', " | ")
                );
            }));
            LAST_PANIC.with(|slot| *slot.borrow_mut() = Some(record));
        }));
    });
}

/// 在 panic 边界内执行 `f`。成功返回 `Ok`,panic 返回带阶段/帧号的记录。
pub fn catch<R>(
    phase: Phase,
    host_frame: u64,
    engine_frame: u64,
    f: impl FnOnce() -> R,
) -> Result<R, PanicRecord> {
    match panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(value) => Ok(value),
        Err(payload) => {
            let mut record = LAST_PANIC
                .with(|slot| slot.borrow_mut().take())
                .unwrap_or_else(|| PanicRecord {
                    phase,
                    host_frame,
                    engine_frame,
                    message: payload_message(payload.as_ref()),
                    location: "unknown".to_string(),
                    thread: "<unnamed>".to_string(),
                    backtrace: String::new(),
                });
            record.phase = phase;
            record.host_frame = host_frame;
            record.engine_frame = engine_frame;
            if record.backtrace.is_empty() {
                record.backtrace = backtrace_summary();
            }
            Err(record)
        }
    }
}

fn payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_string()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}

fn backtrace_summary() -> String {
    let trace = std::backtrace::Backtrace::force_capture().to_string();
    let mut lines: Vec<&str> = trace
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .collect();
    // 第一行是 "   0: ..." 之前可能的 "stack backtrace:" 标题;保留原样。
    if lines.len() > BACKTRACE_LINES {
        lines.truncate(BACKTRACE_LINES);
        lines.push("... (truncated)");
    }
    lines.join("\n")
}

/// 冻结状态:引擎停止推进,宿主继续出帧(横幅)。
#[derive(Clone, Debug)]
pub struct Freeze {
    pub record: PanicRecord,
    pub at: Instant,
    /// 冻结后已经渲染的宿主帧数(用于“连渲染都崩”的熔断)。
    pub drawn_frames: u32,
    /// 冻结后 draw 再次 panic 的次数。>=[`DRAW_PANIC_LIMIT`] 时不再尝试。
    pub draw_panics: u32,
}

/// 冻结期间允许 draw 再崩几次;超过就只保留窗口不刷新(避免死循环刷日志)。
pub const DRAW_PANIC_LIMIT: u32 = 3;

impl Freeze {
    pub fn new(record: PanicRecord) -> Self {
        Self {
            record,
            at: Instant::now(),
            drawn_frames: 0,
            draw_panics: 0,
        }
    }

    /// 引擎是否还能推进(冻结 = 不能)。
    pub const fn engine_advances(&self) -> bool {
        false
    }

    /// 是否还尝试绘制。
    pub fn keep_drawing(&self) -> bool {
        self.draw_panics < DRAW_PANIC_LIMIT
    }

    /// 横幅标题。
    pub fn title(&self) -> String {
        format!(
            "PANIC 捕获于 {} · 引擎已冻结(保留最后一帧)",
            self.record.phase.as_str()
        )
    }

    /// 横幅正文(一行)。
    pub fn body(&self) -> String {
        format!(
            "{} @ {} · frame {} · Esc / Quit 退出",
            self.record.message, self.record.location, self.record.engine_frame
        )
    }
}

/// 关闭请求的宿主策略。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseAction {
    /// 只把 `@_WINDOW_EXEC_CLOSE=1` 推给脚本(引擎语义)。
    SignalEngine,
    /// 宿主强制退出(脚本不处理时的兜底 / 主任务已结束)。
    Exit,
}

/// 窗口关闭策略(AU4 语义 + 宿主兜底)。
///
/// 第一次关闭推 `@_WINDOW_EXEC_CLOSE=1`;第二次或主任务已结束时退出。
/// 抽成纯函数以便单测(不需要真窗口)。
pub const fn close_decision(main_finished: bool, close_requests: u32) -> CloseAction {
    if main_finished || close_requests >= 2 {
        CloseAction::Exit
    } else {
        CloseAction::SignalEngine
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catch_returns_panic_record_with_phase_and_frames() {
        let caught = catch(Phase::Tick, 42, 41, || {
            panic!("synthetic boom");
        })
        .unwrap_err();
        assert_eq!(caught.phase, Phase::Tick);
        assert_eq!(caught.host_frame, 42);
        assert_eq!(caught.engine_frame, 41);
        assert!(
            caught.message.contains("synthetic boom"),
            "{:?}",
            caught.message
        );
        assert!(!caught.location.is_empty());
    }

    #[test]
    fn catch_passes_through_success() {
        let value = catch(Phase::Tick, 0, 0, || 7u32).unwrap();
        assert_eq!(value, 7);
    }

    #[test]
    fn freeze_never_advances_and_stops_after_draw_panic_limit() {
        let record = PanicRecord {
            phase: Phase::Draw,
            host_frame: 1,
            engine_frame: 2,
            message: "wgpu lost".to_string(),
            location: "src/lib.rs:1:1".to_string(),
            thread: "main".to_string(),
            backtrace: "0: foo".to_string(),
        };
        let mut freeze = Freeze::new(record);
        assert!(!freeze.engine_advances());
        assert!(freeze.keep_drawing());
        freeze.draw_panics = DRAW_PANIC_LIMIT;
        assert!(!freeze.keep_drawing());
        assert!(freeze.title().contains("冻结"));
        assert!(freeze.body().contains("wgpu lost"));
    }

    #[test]
    fn close_twice_exits_but_first_request_signals_script() {
        assert_eq!(close_decision(false, 1), CloseAction::SignalEngine);
        assert_eq!(close_decision(false, 2), CloseAction::Exit);
        assert_eq!(close_decision(false, 3), CloseAction::Exit);
        assert_eq!(close_decision(true, 1), CloseAction::Exit);
    }

    #[test]
    fn backtrace_summary_is_bounded_and_non_empty() {
        let trace = backtrace_summary();
        assert!(!trace.is_empty());
        assert!(
            trace.lines().count() <= BACKTRACE_LINES + 1,
            "lines={}",
            trace.lines().count()
        );
    }
}
