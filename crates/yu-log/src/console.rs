//! 统一控制台格式化 + 控制台汇(Q1 日志收口)。
//!
//! 行格式契约(单页文档 `docs/logging-contract.md`,与本模块同步改):
//!
//! ```text
//! [  +12.345s] WARN  [gfx] gfx.imageop.region.partial @ s66 pc6028 x2: ...
//! [   +0.102s] INFO  [host] data.ypf mounted, files=1424
//! ```
//!
//! - 前缀三段:相对进程启动秒(`+s`)、级别词、`[频道]`;
//! - 级别词与严重级映射见 [`severity_word`];门面桥行沿用 log 生态五级词
//!   (error/warn/info/debug/trace → ERROR/WARN/INFO/DEBUG/TRACE);
//! - 频道由诊断码前缀推导([`channel_for_code`],码表契约的一部分);
//! - 色表事实参照旧线 `<repo-old>/crates/yuris-log/src/level.rs:71..76`
//!   (2026-10-25 核对;只取数值事实,不搬代码):Trace=90 灰 / Debug=36 青 /
//!   Info=32 绿 / Warn=33 黄 / Error=1;31 亮红 / Fatal=1;37;41 白底红。
//!   多行消息逐行包色、空行不包、每行复位(旧线 `sink.rs:38..80` 行为);
//! - 颜色开关:`--color on|off|auto`(显式优先)> `NO_COLOR`(非空 ⇒ off)
//!   > `CLICOLOR_FORCE=1`(⇒ on)> stderr 是否终端(auto)。旧线的教训:
//!   auto 检测失败必须可解释、可显式覆盖,不允许静默无色。
//! - A3:格式化器无全局可变状态;唯一的进程级读数是"启动时刻"戳(与
//!   `build_info` 同类的进程元数据,内核语义永不读取);颜色经构造注入。

use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{self, IsTerminal, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use crate::facade::FacadeLevel;
use crate::{Diag, Loc, Severity};

// ------------------------------------------------------------ 频道(码表契约)

/// 码前缀 → 频道。**新增频道必须进本表**并过 `channels_are_unique` 测试;
/// 人读契约见 `docs/logging-contract.md`。
pub const CHANNELS: &[(&str, &str)] = &[
    ("vfs.", "vfs"),
    ("fmt.", "fmt"),
    ("table.", "table"),
    ("rt.", "vm"),
    ("gfx.", "gfx"),
    ("snd.", "sound"),
    ("sv.", "save"),
    ("inp.", "inp"),
    ("platform.", "platform"),
    ("log.", "log"),
    ("host.", "host"),
];

/// 未登记前缀的兜底频道(诊断码不撒谎,只是频道未知)。
pub const CHANNEL_FALLBACK: &str = "core";

/// 由诊断码前缀推导控制台频道。
pub fn channel_for_code(code: &str) -> &'static str {
    for (prefix, channel) in CHANNELS {
        if code.starts_with(prefix) {
            return channel;
        }
    }
    CHANNEL_FALLBACK
}

// ------------------------------------------------------------ 级别词与色表

/// 内核诊断的级别词(宽度 5,`{:<5}` 对齐)。
pub fn severity_word(severity: Severity) -> &'static str {
    match severity {
        Severity::Fatal => "FATAL",
        Severity::Recoverable => "WARN",
        Severity::Unimplemented => "UNIMP",
        Severity::Info => "INFO",
        Severity::Verbose => "VERB",
    }
}

/// 内核诊断的 ANSI 色码(旧线 level.rs:71..76 同表;见模块注)。
pub fn severity_color(severity: Severity) -> &'static str {
    match severity {
        Severity::Fatal => "1;37;41",
        Severity::Recoverable | Severity::Unimplemented => "33",
        Severity::Info => "32",
        // Verbose 是可选噪声:开启后也用最暗的灰,不抢注意力。
        Severity::Verbose => "90",
    }
}

/// 门面桥行的级别词。
pub fn facade_word(level: FacadeLevel) -> &'static str {
    match level {
        FacadeLevel::Error => "ERROR",
        FacadeLevel::Warn => "WARN",
        FacadeLevel::Info => "INFO",
        FacadeLevel::Debug => "DEBUG",
        FacadeLevel::Trace => "TRACE",
    }
}

/// 门面桥行的 ANSI 色码(旧线同表)。
pub fn facade_color(level: FacadeLevel) -> &'static str {
    match level {
        FacadeLevel::Error => "1;31",
        FacadeLevel::Warn => "33",
        FacadeLevel::Info => "32",
        FacadeLevel::Debug => "36",
        FacadeLevel::Trace => "90",
    }
}

/// 生命周期/宿主行的级别词(与门面五级词同表;host 行只用前三)。
pub fn host_word(level: HostLevel) -> &'static str {
    match level {
        HostLevel::Error => "ERROR",
        HostLevel::Warn => "WARN",
        HostLevel::Info => "INFO",
        HostLevel::Verbose => "VERB",
    }
}

pub fn host_color(level: HostLevel) -> &'static str {
    match level {
        HostLevel::Error => "1;31",
        HostLevel::Warn => "33",
        HostLevel::Info => "32",
        HostLevel::Verbose => "90",
    }
}

/// `sub:"log"` 行用的小写级别词(JSONL 字段沿用门面小写口径)。
fn host_level_json(level: HostLevel) -> &'static str {
    match level {
        HostLevel::Error => "error",
        HostLevel::Warn => "warn",
        HostLevel::Info => "info",
        HostLevel::Verbose => "verbose",
    }
}

/// 宿主生命周期行的级别(非内核诊断,不进 `Severity`)。
///
/// `Verbose` 与 [`Severity::Verbose`] 同义:默认任何档位都不显示,
/// 只有 `--log-level verbose` 才出现(周期性能采样等)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostLevel {
    Info,
    Warn,
    Error,
    Verbose,
}

impl HostLevel {
    /// 宿主级别 → 是否过 `--log-level` 门槛。宿主行没有诊断码,
    /// 但门槛语义必须与内核诊断一致(否则 verbose 档行为分裂)。
    fn passes(self, min: MinLevel) -> bool {
        match self {
            HostLevel::Verbose => min == MinLevel::Verbose,
            _ => true,
        }
    }
}

// ------------------------------------------------------------ 颜色开关

static COLOR_OVERRIDE: OnceLock<bool> = OnceLock::new();
static COLOR_STATE: OnceLock<(bool, Option<&'static str>)> = OnceLock::new();

/// `--color on|off` 的显式覆盖(main 入口调用一次;auto 不进这里)。
pub fn set_color_override(on: bool) {
    let _ = COLOR_OVERRIDE.set(on);
}

/// 解析颜色:显式覆盖 > `NO_COLOR`(非空 ⇒ off)> `CLICOLOR_FORCE=1`
/// (⇒ on)> stderr 是否终端。结果缓存,首次调用后稳定。
pub fn resolve_color() -> bool {
    color_state().0
}

/// auto 关闭的原因(`--color on` 的提示语素材;None = 未关闭或显式开关)。
pub fn color_auto_off_reason() -> Option<&'static str> {
    color_state().1
}

fn color_state() -> &'static (bool, Option<&'static str>) {
    COLOR_STATE.get_or_init(|| {
        // 显式 `--color on|off` 优先(用户意图最高;这也是 tyn「静默无色
        // 不可排查」教训的显式出口);其后 CLICOLOR_FORCE=1 > NO_COLOR >
        // stderr 终端检测(auto)。
        if let Some(on) = COLOR_OVERRIDE.get() {
            return (*on, None);
        }
        if clicolor_force() {
            return (true, None);
        }
        if no_color_set() {
            return (false, Some("NO_COLOR"));
        }
        if std::io::stderr().is_terminal() {
            (true, None)
        } else {
            (false, Some("stderr 不是终端(--color on 可强制)"))
        }
    })
}

fn no_color_set() -> bool {
    std::env::var_os("NO_COLOR")
        .map(|v| !v.is_empty())
        .unwrap_or(false)
}

fn clicolor_force() -> bool {
    std::env::var("CLICOLOR_FORCE").map(|v| v == "1").unwrap_or(false)
}

// ------------------------------------------------------------ 启动时刻戳

static STARTED: OnceLock<Instant> = OnceLock::new();

/// main 入口调用一次;测试不调用(时间恒 0)。
pub fn init_process_start() {
    let _ = STARTED.set(Instant::now());
}

fn elapsed_secs() -> f64 {
    STARTED.get().map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0)
}

// ------------------------------------------------------------ 格式化(纯函数)

/// 时间域的 ANSI 色码(暗灰;与级别色分离,降低视觉权重)。
pub const TIME_COLOR: &str = "90";

/// 频道的独立色调(按域一色,与级别色无关;扫一眼即可分辨域)。
/// 动态频道(run/serve/dev/case/core 等)走兜底亮白。
pub fn channel_color(channel: &str) -> &'static str {
    match channel {
        "vm" => "36",        // 青
        "gfx" => "35",       // 品红
        "sound" => "34",     // 蓝
        "save" => "96",      // 亮青
        "vfs" => "94",       // 亮蓝
        "fmt" => "95",       // 亮品红
        "table" => "93",     // 亮黄
        "inp" => "92",       // 亮绿
        "platform" => "37",  // 白
        "log" | "log-facade" => "90", // 灰(依赖桥,非主角)
        "host" => "93",      // 亮黄(宿主生命周期)
        _ => "97",           // 兜底亮白
    }
}

/// 一行统一格式。`body` 已含级别之后的一切(含 `x{count}` 与详情)。
/// 时间域恒定暗灰(`TIME_COLOR`),行体按级别色;多行 `body` 逐行包色、
/// 空行不包、每行复位(旧线 sink.rs 行为)。
/// 时间域恒定暗灰;`[频道]` 按域独立色调([`channel_color`],与级别色
/// 分离);级别词加粗(与行体的亮度差 = 色差);行体全亮级别色。
/// 多行 `body` 逐行包色、空行不包、每行复位(旧线 sink.rs 行为)。
pub fn format_line(secs: f64, word: &str, color_code: &str, channel: &str, body: &str, color: bool) -> String {
    let mut out = String::with_capacity(body.len() + 72);
    if color {
        out.push_str("\x1b[90m");
        out.push_str(&format!("[{secs:>+9.3}s]"));
        out.push_str("\x1b[0m ");
        // 级别词:加粗级别色(粗体 = 与行体的亮度差)。
        out.push_str(&format!("\x1b[1;{color_code}m{word:<5}\x1b[0m "));
        // 频道:按域独立色调(与级别色无关,扫一眼即可分辨域)。
        out.push_str(&format!("\x1b[{}m[{channel}]\x1b[0m ", channel_color(channel)));
    } else {
        out.push_str(&format!("[{secs:>+9.3}s] {word:<5} [{channel}] "));
    }
    if !color {
        out.push_str(body);
        return out;
    }
    let mut first = true;
    for line in body.split('\n') {
        if !first {
            out.push('\n');
        }
        first = false;
        if line.is_empty() {
            continue;
        }
        out.push_str("\x1b[");
        out.push_str(color_code);
        out.push('m');
        out.push_str(line);
        out.push_str("\x1b[0m");
    }
    out
}

/// `Loc` 的一行短显示(`Loc::None` ⇒ None,行内省略 `@ …` 段)。
pub fn loc_display(loc: &Loc) -> Option<String> {
    match loc {
        Loc::Script { file, pc, .. } => Some(format!("{file} pc{pc}")),
        Loc::Data { path, offset } => {
            Some(format!("{}+0x{offset:x}", String::from_utf8_lossy(path)))
        }
        Loc::Table { name } => Some(format!("table {name}")),
        Loc::None => None,
    }
}

/// 位置列表显示:前 [`MAX_AT`] 个,超出折成 `+N`。
pub fn at_list_display(at: &[String], more: u64) -> String {
    let mut parts: Vec<String> = at.iter().take(MAX_AT).cloned().collect();
    let hidden = at.len().saturating_sub(MAX_AT) as u64 + more;
    if hidden > 0 {
        parts.push(format!("+{hidden}"));
    }
    parts.join(",")
}

/// 单条 FrameDiag 位置上限;超出折叠(`at_more` 计数),不静默丢弃。
pub const MAX_AT: usize = 8;

/// 一条内核诊断的行体(级别词之后的部分)。
pub fn diag_body(diag: &Diag, count: u64) -> String {
    let mut body = String::new();
    body.push_str(diag.code.as_ref());
    if let Some(loc) = loc_display(&diag.where_) {
        body.push_str(" @ ");
        body.push_str(&loc);
    }
    if count > 1 {
        body.push_str(&format!(" x{count}"));
    }
    if !diag.detail.is_empty() {
        body.push_str(": ");
        body.push_str(&diag.detail);
    }
    body
}

/// 剥离 `ESC[ … m` 颜色序列(面板 recent 缓冲用;本文件是唯一 ANSI 产生点)。
fn strip_ansi(text: &str) -> String {
    if !text.contains('\x1b') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

// ------------------------------------------------------------ 级别过滤

/// 控制台最低可见级(`--log-level` 的内核诊断门槛)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MinLevel {
    /// 压掉 `Severity::Info` 与 `Verbose`(quiet/error/warn);
    /// 挂号/可恢复/致命仍显示。
    Quiet,
    /// 显示到 `Info`(info/debug/trace)。**仍压掉 `Verbose`**。
    Normal,
    /// 显式开启的最低档(仅 `--log-level verbose` / `YU_LOG=verbose`)。
    Verbose,
}

impl MinLevel {
    pub fn passes(self, severity: Severity) -> bool {
        match self {
            MinLevel::Quiet => !matches!(severity, Severity::Info | Severity::Verbose),
            MinLevel::Normal => severity != Severity::Verbose,
            MinLevel::Verbose => true,
        }
    }

    /// `YU_LOG`/`--log-level` 值 → 门槛。quiet/error/warn 压 Info 与
    /// Verbose;info/debug/trace 显示到 Info 但仍压 Verbose;
    /// **只有 `verbose` 才显示 Verbose**(门面桥自身的级别另见
    /// `facade::level_from_env`)。
    pub fn from_level_value(value: Option<&str>) -> Self {
        match value
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("quiet") | Some("error") | Some("warn") => MinLevel::Quiet,
            Some("verbose") => MinLevel::Verbose,
            _ => MinLevel::Normal,
        }
    }
}

// ------------------------------------------------------------ JSONL 单写者

/// 共享 JSONL 写者(一个文件句柄,进程内唯一写者;Q1 R4 契约)。
///
/// 两种行共存:`{"sub":"diag",…}`(内核 `Diag`,schema 不变,仅前置 sub 键;
/// `Diag::from_json_line` 忽略未知键,往返契约不受影响)与
/// `{"sub":"log",…}`(门面/生命周期行,字段序同 `facade`)。
///
/// **异步**:帧线程只做一次 `try_send`,磁盘 I/O 与格式化落盘全在后台线程
/// (`BufWriter`,队列排空后统一 flush)。旧实现是 `Arc<Mutex<File>>` + 每条
/// 两次 `write_all`:尖峰帧一次灌几千条诊断时,帧线程被同步文件 I/O 拖住,
/// 这正是「开日志更卡」的来源。队列满时丢行并计数(退出前把丢弃数落成
/// 一行,不静默)。
#[derive(Clone)]
pub struct JsonlFile(Arc<JsonlSink>);

enum LogMsg {
    Line(String),
    Flush(std::sync::mpsc::SyncSender<()>),
    Shutdown,
}

struct JsonlSink {
    tx: std::sync::mpsc::SyncSender<LogMsg>,
    dropped: AtomicU64,
}

impl Drop for JsonlSink {
    fn drop(&mut self) {
        // 最后一个句柄消失:让后台线程排空并 flush 后退出。
        let _ = self.tx.send(LogMsg::Shutdown);
    }
}

/// 队列容量(行)。按 100 行/帧的极端洪水算,可缓冲约 10 分钟。
const JSONL_QUEUE: usize = 65536;
/// 后台缓冲(字节):一次 flush 落一大块,syscall 数量与行数解耦。
const JSONL_BUF: usize = 256 * 1024;

impl JsonlFile {
    /// 创建/截断日志文件(一次运行 = 一个现场)。
    pub fn create(path: impl AsRef<std::path::Path>) -> io::Result<Self> {
        let file = File::create(path)?;
        let (tx, rx) = std::sync::mpsc::sync_channel::<LogMsg>(JSONL_QUEUE);
        let sink = Arc::new(JsonlSink {
            tx,
            dropped: AtomicU64::new(0),
        });
        let dropped_tail = Arc::clone(&sink);
        std::thread::Builder::new()
            .name("yu-log-jsonl".to_string())
            .spawn(move || {
                let mut out = std::io::BufWriter::with_capacity(JSONL_BUF, file);
                'main: loop {
                    match rx.recv() {
                        Ok(LogMsg::Line(line)) => {
                            let _ = out.write_all(line.as_bytes());
                            let _ = out.write_all(b"\n");
                        }
                        Ok(LogMsg::Flush(ack)) => {
                            let _ = out.flush();
                            let _ = ack.send(());
                            continue;
                        }
                        Ok(LogMsg::Shutdown) | Err(_) => break 'main,
                    }
                    // 突发:把已到的行一次写完,只 flush 一次。
                    loop {
                        match rx.try_recv() {
                            Ok(LogMsg::Line(line)) => {
                                let _ = out.write_all(line.as_bytes());
                                let _ = out.write_all(b"\n");
                            }
                            Ok(LogMsg::Flush(ack)) => {
                                let _ = out.flush();
                                let _ = ack.send(());
                            }
                            Ok(LogMsg::Shutdown) => break 'main,
                            // 队列暂时为空 = 这一批写完;不是关闭。
                            Err(std::sync::mpsc::TryRecvError::Empty) => break,
                            Err(std::sync::mpsc::TryRecvError::Disconnected) => break 'main,
                        }
                    }
                    let _ = out.flush();
                }
                let dropped = dropped_tail.dropped.load(Ordering::Relaxed);
                if dropped > 0 {
                    let _ = writeln!(
                        out,
                        "{{\"sub\":\"log\",\"level\":\"warn\",\"target\":\"yu-log\",\
                         \"file\":null,\"line\":null,\"message\":\"写队列满,丢弃 {dropped} 行\"}}"
                    );
                }
                let _ = out.flush();
            })
            .map_err(|error| io::Error::other(format!("启动 JSONL 写线程失败: {error}")))?;
        Ok(Self(sink))
    }

    fn write_owned(&self, line: String) {
        match self.0.tx.try_send(LogMsg::Line(line)) {
            Ok(()) => {}
            // 队列满 = 磁盘明显跟不上;丢行并计数(退出前落一行说明)。
            Err(std::sync::mpsc::TrySendError::Full(_)) => {
                self.0.dropped.fetch_add(1, Ordering::Relaxed);
            }
            Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {}
        }
    }

    pub(crate) fn write_raw(&self, line: &str) {
        self.write_owned(line.to_string());
    }

    /// 同步落盘点(退出/panic 前调用):排队并等后台线程 flush 完。
    pub fn flush(&self) {
        let (ack_tx, ack_rx) = std::sync::mpsc::sync_channel(0);
        if self.0.tx.send(LogMsg::Flush(ack_tx)).is_ok() {
            let _ = ack_rx.recv_timeout(std::time::Duration::from_millis(1000));
        }
    }

    /// 内核诊断一行(sub:"diag";schema 即 `Diag::to_json_line`,前置 sub)。
    pub fn write_diag(&self, diag: &Diag) {
        self.write_owned(diag.to_json_line_with_sub("diag"));
    }

    /// 门面/生命周期一行(sub:"log";字段序同 `facade::record_json_line`)。
    pub fn write_log_line(&self, level: &str, target: &str, message: &str) {
        self.write_owned(crate::facade::json_line(level, target, None, None, message));
    }
}

// ------------------------------------------------------------ 控制台汇

/// 控制台汇配置(构造注入,无全局状态;A3)。
pub struct ConsoleOptions {
    pub color: bool,
    /// 内核诊断的级别门槛(过滤在格式化之前)。
    pub min: MinLevel,
    /// true = 行到即印(桌面);false = 只聚合,末尾 [`Console::summary_lines`](headless)。
    pub live: bool,
    /// 最近行环形缓冲容量(0 = 不留;桌面面板「日志」页签用,明文无色)。
    pub recent_cap: usize,
    /// JSONL 单写者(内核 Diag 落 sub:"diag";宿主行落 sub:"log")。
    pub file: Option<JsonlFile>,
}

impl Default for ConsoleOptions {
    fn default() -> Self {
        Self {
            color: true,
            min: MinLevel::Normal,
            live: true,
            recent_cap: 0,
            file: None,
        }
    }
}

type DynWrite = Box<dyn Write + Send>;

struct AggEntry {
    severity: Severity,
    count: u64,
    first_secs: f64,
    code: String,
    loc: String,
    detail: String,
    /// 汇总行缓存(无色;按需重建,避免逐帧格式化)。
    cached_line: String,
}

/// 统一控制台汇。内核诊断聚合去重(同 key 只 `count += 1`,不重格式化),
/// 门面/宿主行直写。写 stderr(可注入);JSONL 经共享 [`JsonlFile`]。
pub struct Console {
    opts: ConsoleOptions,
    out: DynWrite,
    seen: HashMap<Box<str>, AggEntry>,
    recent: VecDeque<String>,
}


impl Console {
    pub fn new(opts: ConsoleOptions) -> Self {
        let recent_cap = opts.recent_cap;
        Self {
            opts,
            // 块缓冲:诊断洪水时不逐行触碰 stderr(WARN+ 仍即时 flush,
            // 见 `write_line`;退出/panic 走 `flush`)。
            out: Box::new(std::io::BufWriter::with_capacity(64 * 1024, std::io::stderr())),
            seen: HashMap::new(),
            recent: VecDeque::with_capacity(recent_cap.min(1024)),
        }
    }

    /// 测试/捕获用:替换输出目标(stderr 之外)。
    pub fn with_writer(opts: ConsoleOptions, out: DynWrite) -> Self {
        let recent_cap = opts.recent_cap;
        Self {
            opts,
            out,
            seen: HashMap::new(),
            recent: VecDeque::with_capacity(recent_cap.min(1024)),
        }
    }

    /// 桌面/serve 缺省:resolve 颜色 + 即印 + 环形缓冲。
    pub fn stderr_live(min: MinLevel, recent_cap: usize, file: Option<JsonlFile>) -> Self {
        Self::new(ConsoleOptions {
            color: resolve_color(),
            min,
            live: true,
            recent_cap,
            file,
        })
    }

    /// headless 缺省:聚合不即印(末尾 [`Self::summary_lines`])。
    pub fn stderr_aggregate(min: MinLevel, file: Option<JsonlFile>) -> Self {
        Self::new(ConsoleOptions {
            color: resolve_color(),
            min,
            live: false,
            recent_cap: 0,
            file,
        })
    }

    /// 写一行到 stderr。`flush` = 立即落盘(告警/致命;普通行走缓冲,
    /// 由 [`Console::flush`] 或缓冲满时成批刷出)。
    ///
    /// 旧实现对**每一行**都 `flush()`,而 stderr 在 Windows 控制台窗口里
    /// 每条 write 都要宿主重绘:诊断洪水时帧线程被同步写拖住(用户实测
    /// 「开日志更卡」)。现在 64KB 块缓冲,只有 WARN+ 立即刷。
    fn write_line(&mut self, line: String, flush: bool) {
        let _ = self.out.write_all(line.as_bytes());
        let _ = self.out.write_all(b"\n");
        if flush {
            let _ = self.out.flush();
        }
        if self.opts.recent_cap > 0 {
            // 面板「日志」页签存明文:彩色行进 egui monospace 会露出 ANSI
            // 转义字形(控制台与面板同源、不同形)。
            let plain = if self.opts.color {
                strip_ansi(&line)
            } else {
                line
            };
            if self.recent.len() == self.opts.recent_cap {
                self.recent.pop_front();
            }
            self.recent.push_back(plain);
        }
    }

    /// 同步落盘(退出/panic 前调用):stderr 缓冲 + JSONL 队列。
    pub fn flush(&mut self) {
        let _ = self.out.flush();
        if let Some(file) = &self.opts.file {
            file.flush();
        }
    }

    /// 一条内核诊断(`count` 来自 FrameDiag 聚合时 >1)。门槛过滤在
    /// 格式化之前;同 key 命中只 `count += 1`。JSONL 无论门槛全落盘。
    pub fn emit_diag(&mut self, diag: &Diag, count: u64) {
        if let Some(file) = &self.opts.file {
            file.write_diag(diag);
        }
        let key = format!(
            "{}\u{1f}{}\u{1f}{}",
            diag.code,
            loc_display(&diag.where_).unwrap_or_default(),
            diag.detail
        );
        match self.seen.get_mut(key.as_str()) {
            Some(entry) => {
                entry.count += count;
                entry.cached_line.clear();
            }
            None => {
                let secs = elapsed_secs();
                let loc = loc_display(&diag.where_).unwrap_or_default();
                self.seen.insert(
                    key.into_boxed_str(),
                    AggEntry {
                        severity: diag.severity,
                        count,
                        first_secs: secs,
                        code: diag.code.to_string(),
                        loc: loc.clone(),
                        detail: diag.detail.clone(),
                        cached_line: String::new(),
                    },
                );
            }
        }
        if self.opts.live && self.opts.min.passes(diag.severity) {
            let body = diag_body(diag, count);
            let line = format_line(
                elapsed_secs(),
                severity_word(diag.severity),
                severity_color(diag.severity),
                channel_for_code(diag.code.as_ref()),
                &body,
                self.opts.color,
            );
            self.write_line(
                line,
                matches!(
                    diag.severity,
                    crate::Severity::Fatal
                        | crate::Severity::Recoverable
                        | crate::Severity::Unimplemented
                ),
            );
        }
    }

    /// 门面桥一行(级别已由 FacadeLogger 过滤)。
    pub fn emit_facade(&mut self, level: FacadeLevel, target: &str, message: &str) {
        if let Some(file) = &self.opts.file {
            file.write_log_line(level.as_str(), target, message);
        }
        let body = format!("{target}: {message}");
        let line = format_line(
            elapsed_secs(),
            facade_word(level),
            facade_color(level),
            "log-facade",
            &body,
            self.opts.color,
        );
        self.write_line(line, matches!(level, FacadeLevel::Error | FacadeLevel::Warn));
    }

    /// 宿主生命周期行(`[host]`/`[serve]`/`[run]`/`[dev]` 频道)。
    pub fn emit_host(&mut self, level: HostLevel, channel: &str, message: &str) {
        if let Some(file) = &self.opts.file {
            file.write_log_line(host_level_json(level), channel, message);
        }
        if !level.passes(self.opts.min) {
            return;
        }
        let line = format_line(
            elapsed_secs(),
            host_word(level),
            host_color(level),
            channel,
            message,
            self.opts.color,
        );
        self.write_line(line, matches!(level, HostLevel::Warn | HostLevel::Error));
    }

    /// 末尾诊断汇总(频次降序,同频次按码序;行复用首次出现的时刻戳)。
    /// 汇总同样受 `--log-level` 门槛(quiet 压 Info = 连汇总一起压,
    /// 这才是「压掉」的完整语义)。
    pub fn summary_lines(&mut self) -> Vec<String> {
        let min = self.opts.min;
        let mut entries: Vec<&mut AggEntry> = self
            .seen
            .values_mut()
            .filter(|entry| min.passes(entry.severity))
            .collect();
        entries.sort_by(|a, b| {
            b.count
                .cmp(&a.count)
                .then_with(|| a.code.cmp(&b.code))
                .then_with(|| a.loc.cmp(&b.loc))
                .then_with(|| a.detail.cmp(&b.detail))
        });
        entries
            .into_iter()
            .map(|entry| {
                if entry.cached_line.is_empty() {
                    let body = if entry.count > 1 {
                        format!("{} @ {} x{}: {}", entry.code, entry.loc, entry.count, entry.detail)
                    } else if entry.loc.is_empty() {
                        format!("{}: {}", entry.code, entry.detail)
                    } else {
                        format!("{} @ {}: {}", entry.code, entry.loc, entry.detail)
                    };
                    entry.cached_line = format_line(
                        entry.first_secs,
                        severity_word(entry.severity),
                        severity_color(entry.severity),
                        channel_for_code(&entry.code),
                        &body,
                        self.opts.color,
                    );
                }
                entry.cached_line.clone()
            })
            .collect()
    }

    /// 聚合条目数(测试与状态行)。
    pub fn agg_len(&self) -> usize {
        self.seen.len()
    }

    /// 汇总直接打到本 Console 的输出(stderr;headless 末尾用)。
    pub fn print_summary(&mut self) {
        for line in self.summary_lines() {
            let _ = self.out.write_all(line.as_bytes());
            let _ = self.out.write_all(b"\n");
        }
        self.flush();
    }

    /// 面板「日志」页签的最近行快照(明文无色)。
    pub fn recent_snapshot(&self) -> Vec<String> {
        self.recent.iter().cloned().collect()
    }

    /// 面板清空(只清控制台环形缓冲,不动 JSONL 文件)。
    pub fn clear_recent(&mut self) {
        self.recent.clear();
    }
}

/// 跨线程共享句柄(`LogSink` 语义;桌面面板与门面闭包共用同一个 Console)。
#[derive(Clone)]
pub struct ConsoleHandle(pub Arc<Mutex<Console>>);

impl std::fmt::Debug for ConsoleHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConsoleHandle")
            .field("entries", &self.0.lock().map(|c| c.agg_len()).unwrap_or(0))
            .finish()
    }
}

impl ConsoleHandle {
    pub fn new(console: Console) -> Self {
        Self(Arc::new(Mutex::new(console)))
    }

    /// 锁内执行(中毒恢复式;Console 携带的都是可恢复状态——聚合计数/
    /// 环形缓冲,中毒后取回旧状态继续,不应让一次 panic 杀死全部日志)。
    fn with_lock<R>(&self, f: impl FnOnce(&mut Console) -> R) -> Option<R> {
        let mut console = self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        Some(f(&mut console))
    }

    /// 便捷:`LogSink::emit` 的锁内版本。
    pub fn emit_diag(&self, diag: &Diag) {
        self.with_lock(|console| console.emit_diag(diag, 1));
    }

    /// 带 FrameDiag 聚合计数的版本(gfx 帧诊断出行用)。
    pub fn emit_diag_counted(&self, diag: &Diag, count: u64) {
        self.with_lock(|console| console.emit_diag(diag, count));
    }

    pub fn emit_facade(&self, level: FacadeLevel, target: &str, message: &str) {
        self.with_lock(|console| console.emit_facade(level, target, message));
    }

    pub fn emit_host(&self, level: HostLevel, channel: &str, message: &str) {
        self.with_lock(|console| console.emit_host(level, channel, message));
    }

    pub fn recent_snapshot(&self) -> Vec<String> {
        self.with_lock(|console| console.recent_snapshot()).unwrap_or_default()
    }

    pub fn clear_recent(&self) {
        self.with_lock(|console| console.clear_recent());
    }

    pub fn summary_lines(&self) -> Vec<String> {
        self.with_lock(|console| console.summary_lines()).unwrap_or_default()
    }

    /// 同步落盘(stderr 缓冲 + JSONL 队列;退出/panic 前调用)。
    pub fn flush(&self) {
        self.with_lock(|console| console.flush());
    }
}

impl crate::sink::LogSink for ConsoleHandle {
    fn emit(&mut self, diag: &Diag) -> io::Result<()> {
        ConsoleHandle::emit_diag(self, diag);
        Ok(())
    }
}

/// 门面桥的无状态 stderr 行(main/engine_main 的 fn 指针闭包用;
/// 颜色经全局解析,启动时刻经 [`init_process_start`])。
pub fn facade_line_to_stderr(level: FacadeLevel, target: &str, message: &str) {
    let body = format!("{target}: {message}");
    let line = format_line(
        elapsed_secs(),
        facade_word(level),
        facade_color(level),
        "log-facade",
        &body,
        resolve_color(),
    );
    let _ = std::io::stderr().lock().write_all(line.as_bytes());
    let _ = std::io::stderr().lock().write_all(b"\n");
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::code;
    use crate::Boundary;

    fn gfx_diag() -> Diag {
        Diag::unimplemented("gfx.imageop.unsupported", Boundary::Domain)
            .at(Loc::script("s66", None, 6028, Some("CGACT".to_string())))
            .with_detail("region partly outside source")
    }

    #[test]
    fn line_format_matches_contract() {
        let line = format_line(12.345, "WARN", "33", "gfx", "a.b: c", false);
        assert_eq!(line, "[  +12.345s] WARN  [gfx] a.b: c");
        let line = format_line(0.1023, "INFO", "32", "host", "mounted", false);
        assert_eq!(line, "[   +0.102s] INFO  [host] mounted");
    }

    #[test]
    fn color_wraps_time_dim_and_body_per_level() {
        let line = format_line(1.0, "ERROR", "1;31", "log-facade", "one\ntwo\n", true);
        assert_eq!(
            line,
            "\x1b[90m[   +1.000s]\x1b[0m \x1b[1;1;31mERROR\x1b[0m \x1b[90m[log-facade]\x1b[0m \
             \x1b[1;31mone\x1b[0m\n\x1b[1;31mtwo\x1b[0m\n"
        );
        let warn = format_line(12.345, "WARN", "33", "gfx", "c", true);
        assert!(
            warn.starts_with(
                "\x1b[90m[  +12.345s]\x1b[0m \x1b[1;33mWARN \x1b[0m \x1b[35m[gfx]\x1b[0m \x1b[33mc"
            ),
            "{warn}"
        );
    }

    #[test]
    fn channel_table_covers_namespaces_and_is_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for (prefix, channel) in CHANNELS {
            assert!(seen.insert(*prefix), "duplicate code prefix: {prefix}");
            assert!(!channel.is_empty());
        }
        assert_eq!(channel_for_code("gfx.imageop.region"), "gfx");
        assert_eq!(channel_for_code("rt.cmd.unknown"), "vm");
        assert_eq!(channel_for_code("snd.query.x"), "sound");
        assert_eq!(channel_for_code("mystery.code"), CHANNEL_FALLBACK);
    }

    #[test]
    fn diag_body_carries_loc_and_count() {
        assert_eq!(
            diag_body(&gfx_diag(), 2),
            "gfx.imageop.unsupported @ s66 pc6028 x2: region partly outside source"
        );
        let bare = Diag::info("table.fallback", Boundary::Table).with_detail("hi");
        assert_eq!(diag_body(&bare, 1), "table.fallback: hi");
    }

    #[test]
    fn min_level_gates_only_info() {
        assert!(!MinLevel::Quiet.passes(Severity::Info));
        assert!(MinLevel::Quiet.passes(Severity::Fatal));
        assert!(MinLevel::Quiet.passes(Severity::Recoverable));
        assert!(MinLevel::Quiet.passes(Severity::Unimplemented));
        for s in [Severity::Fatal, Severity::Recoverable, Severity::Unimplemented, Severity::Info] {
            assert!(MinLevel::Normal.passes(s));
        }
        assert_eq!(MinLevel::from_level_value(Some("QUIET")), MinLevel::Quiet);
        assert_eq!(MinLevel::from_level_value(Some("warn")), MinLevel::Quiet);
        assert_eq!(MinLevel::from_level_value(Some("info")), MinLevel::Normal);
        assert_eq!(MinLevel::from_level_value(None), MinLevel::Normal);
    }

    #[test]
    fn console_aggregates_and_summarizes_by_count() {
        let mut console = Console::new(ConsoleOptions { live: false, ..Default::default() });
        console.emit_diag(&gfx_diag(), 1);
        console.emit_diag(&gfx_diag(), 1);
        console.emit_diag(&gfx_diag(), 1);
        let other = Diag::recoverable(code::vfs::MISSING, Boundary::Data)
            .with_detail("entry not found");
        console.emit_diag(&other, 1);
        assert_eq!(console.agg_len(), 2);
        let lines = console.summary_lines();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("x3"), "频次降序: {lines:?}");
        assert!(lines[0].contains("[gfx]"));
        assert!(lines[1].contains("vfs.missing"));
    }

    #[test]
    fn live_console_suppresses_info_under_quiet_but_keeps_summary() {
        let mut console = Console::with_writer(
            ConsoleOptions {
                color: false,
                min: MinLevel::Quiet,
                live: true,
                recent_cap: 8,
                file: None,
            },
            Box::new(Vec::<u8>::new()),
        );
        let info = Diag::info("table.fallback", Boundary::Table).with_detail("lifecycle");
        console.emit_diag(&info, 1);
        assert_eq!(console.recent_snapshot().len(), 0, "quiet 压掉 Info,不进控制台");
        assert_eq!(console.agg_len(), 1, "聚合不受门槛影响(末尾汇总仍可见)");
        console.emit_diag(&gfx_diag(), 1);
        assert_eq!(console.recent_snapshot().len(), 1);
        assert!(console.recent_snapshot()[0].contains("s66 pc6028"));
        let summary = console.summary_lines();
        assert_eq!(summary.len(), 1, "汇总同受 quiet 门槛(Info 被压)");
        assert!(summary[0].contains("gfx"), "{summary:?}");
    }

    #[test]
    fn normal_level_summary_keeps_everything() {
        let mut console = Console::new(ConsoleOptions {
            color: false,
            min: MinLevel::Normal,
            live: false,
            recent_cap: 0,
            file: None,
        });
        console.emit_diag(&gfx_diag(), 2);
        console.emit_diag(
            &Diag::info("table.fallback", Boundary::Table).with_detail("abi"),
            1,
        );
        let summary = console.summary_lines();
        assert_eq!(summary.len(), 2, "缺省(info)汇总不丢级别");
        assert!(summary[0].contains("x2"), "频次降序: {summary:?}");
    }

    #[test]
    fn live_console_writes_bytes_and_jsonl() {
        let shared = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
        struct Shared(Arc<Mutex<Vec<u8>>>);
        impl Write for Shared {
            fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut console = Console::with_writer(
            ConsoleOptions {
                color: false,
                min: MinLevel::Normal,
                live: true,
                recent_cap: 0,
                file: None,
            },
            Box::new(Shared(shared.clone())),
        );
        console.emit_diag(&gfx_diag(), 1);
        console.emit_host(HostLevel::Warn, "host", "wgpu 启动失败;回退 soft");
        let text = String::from_utf8(shared.lock().unwrap().clone()).unwrap();
        let mut lines = text.lines();
        let first = lines.next().unwrap();
        assert!(first.starts_with("[   +0.000s] UNIMP [gfx] gfx.imageop.unsupported @ s66 pc6028"), "{first}");
        let second = lines.next().unwrap();
        assert_eq!(second, "[   +0.000s] WARN  [host] wgpu 启动失败;回退 soft");
        assert!(lines.next().is_none());
    }

    #[test]
    fn jsonl_file_writes_diag_and_log_lines() {
        // 系统 TEMP 不可写(AGENTS §2);落 workspace target/tmp。
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("yu-log-test-{}.jsonl", std::process::id()));
        let file = JsonlFile::create(&path).unwrap();
        file.write_diag(&gfx_diag());
        file.write_log_line("info", "host", "data.ypf mounted");
        // 写者异步(帧线程不碰磁盘):读文件前先要一次同步落盘点。
        file.flush();
        let text = std::fs::read_to_string(&path).unwrap();
        let mut lines = text.lines();
        let diag_line = lines.next().unwrap();
        assert!(diag_line.starts_with("{\"sub\":\"diag\",\"code\":\"gfx."), "{diag_line}");
        let back = Diag::from_json_line(diag_line).unwrap();
        assert_eq!(back, gfx_diag(), "sub 前置不影响 Diag 往返");
        let log_line = lines.next().unwrap();
        assert_eq!(
            log_line,
            "{\"sub\":\"log\",\"level\":\"info\",\"target\":\"host\",\"file\":null,\"line\":null,\"message\":\"data.ypf mounted\"}"
        );
        drop(file);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn at_list_display_caps_and_folds() {
        let at: Vec<String> = (0..10).map(|i| format!("s{i} pc1")).collect();
        assert_eq!(at_list_display(&at, 0), "s0 pc1,s1 pc1,s2 pc1,s3 pc1,s4 pc1,s5 pc1,s6 pc1,s7 pc1,+2");
        assert_eq!(at_list_display(&["a".to_string()], 3), "a,+3");
    }
}
