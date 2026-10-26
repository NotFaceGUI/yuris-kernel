//! 本地服务:静态页面 + 状态 + SSE + WebSocket 帧/输入。
//!
//! 引擎在**主线程**按 `--fps` 推进;HTTP 连接各自一个线程。帧只编码一次
//! (`Arc<Vec<u8>>`)广播给所有客户端;慢客户端丢旧帧并计数,不阻塞引擎。

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use yu_backend_soft::RenderReport;
use yu_platform_input::{key_button, wheel_button, PointerButton};

use crate::args::ArgReader;
use crate::build_info;
use crate::engine::{Backend, Engine, EngineOptions, FrameReport};
use crate::output;
use crate::project::{json_escape, DatasetInfo, Project};
use crate::run::parse_u32;
use crate::ws::{self, WsMessage};
use crate::{CliResult, Failure};

static NEXT_CLIENT: AtomicU64 = AtomicU64::new(1);

/// `/diag` 快照节拍(H-P1-03:有订阅者时 500ms 推送一次)。
const DIAG_PUSH_MS: u64 = 500;
/// 无客户端/暂停时的空转睡眠(工程节拍,不影响游戏时钟)。
const IDLE_SLEEP_MS: u64 = 15;

/// `yu-cli serve` / `yu-engine serve` 参数。
#[derive(Clone, Debug)]
pub struct ServeOptions {
    pub root: PathBuf,
    pub data: Option<PathBuf>,
    pub backend: Backend,
    pub port: u16,
    pub no_open: bool,
    pub entry: String,
    pub fps: u32,
    pub record_events: bool,
    pub www: PathBuf,
    /// 由 HMR 监督进程拉起:stdin EOF = 退出(避免孤儿进程)。
    pub supervised: bool,
    /// 到达该逻辑帧后暂停(验收/截图用;重启后重新计)。
    pub pause_at: Option<u64>,
    /// 开 `/diag` WebSocket 诊断通道(P1 交接件 H-P1-03;www 消费)。
    pub diag_ws: bool,
    /// `@_DEBUGMODE` 注入(DIV-L-001;debug 语料对拍口径)。
    pub debug_mode: bool,
    /// 强制字体 family(诊断/换字体;`None` = 脚本语义)。
    pub force_font: Option<String>,
    /// JSONL 日志路径(Q1 R4;`None` = 不落盘,关闭时只打 stderr 汇总)。
    pub log_file: Option<PathBuf>,
}

impl ServeOptions {
    pub fn from_args(tokens: Vec<String>) -> CliResult<Self> {
        let mut args = ArgReader::new(tokens);
        let root = args
            .positional(0)
            .ok_or_else(|| Failure::usage("用法: yu-cli serve <project-root> [...]"))?;
        let data = args.flag("data").map(PathBuf::from);
        let backend = match args.flag("backend") {
            Some(value) => Backend::parse(&value)?,
            None => Backend::Soft,
        };
        let port = match args.flag("port") {
            Some(value) => value
                .parse::<u16>()
                .map_err(|_| Failure::usage(format!("--port 需要 0..65535,收到 `{value}`")))?,
            None => 4173,
        };
        let no_open = args.switch("no-open");
        let entry = args
            .flag("entry")
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "SYSTEM_START".to_string());
        let fps = match args.flag("fps") {
            Some(value) => parse_u32("fps", &value)?.max(1),
            None => 60,
        };
        let record_events = match args.flag("record-events") {
            Some(value) => ArgReader::on_off("record-events", &value)?,
            None => false,
        };
        let www = args
            .flag("www")
            .map(PathBuf::from)
            .unwrap_or_else(default_www);
        let supervised = args.switch("supervised");
        let diag_ws = args.switch("diag-ws");
        let debug_mode = match args.flag("debug-mode") {
            Some(value) => ArgReader::on_off("debug-mode", &value)?,
            None => false,
        };
        let pause_at =
            match args.flag("pause-at") {
                Some(value) => Some(value.parse::<u64>().map_err(|_| {
                    Failure::usage(format!("--pause-at 需要非负整数,收到 `{value}`"))
                })?),
                None => None,
            };
        let force_font = args.flag("font").filter(|value| !value.trim().is_empty());
        let log_file = args.flag("log-file").map(PathBuf::from);
        args.finish()?;
        Ok(Self {
            root: PathBuf::from(root),
            data,
            backend,
            port,
            no_open,
            entry,
            fps,
            record_events,
            www,
            supervised,
            pause_at,
            diag_ws,
            debug_mode,
            force_font,
            log_file,
        })
    }
}

fn default_www() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/yu-cli -> workspace root")
        .join("www")
}

/// 服务主入口(阻塞到收到 `quit` / stdin EOF)。
pub fn run(opts: ServeOptions) -> CliResult<()> {
    // 统一控制台(Q1 R2/R4):serve 聚合不即印(长跑不刷屏),关闭时打
    // 末尾汇总;`--log-file` 落 JSONL(每轮截断,与 desktop/headless 同口径)。
    let jsonl = match opts.log_file.as_deref() {
        Some(path) => Some(crate::logging::open_jsonl_truncating(path)?),
        None => None,
    };
    if let Some(file) = &jsonl {
        crate::logging::write_color_notice(file);
    }
    let console =
        yu_log::console::ConsoleHandle::new(yu_log::console::Console::stderr_aggregate(
            crate::logging::min_level(),
            jsonl,
        ));
    let project = Project::discover(&opts.root, opts.data.as_deref())?;
    let dataset = project.describe();
    let engine_opts = EngineOptions {
        backend: opts.backend,
        entry: opts.entry.clone(),
        record_events: opts.record_events,
        fps: opts.fps,
        debug_mode: opts.debug_mode,
        // 长跑宿主:首帧前预热字体表,避免第一次文本场景的 ~130ms 停顿。
        warm_text: true,
        force_font: opts.force_font.clone(),
        // 真实运行:`@_TIME` 注入系统本地时间(存档菜单的现实日期)。
        local_time: crate::clock::local_now(),
        ..EngineOptions::default()
    };
    let mut engine = Engine::new(project, engine_opts)?;
    // H-P1-05:实时路径只补 2 帧。默认 500ms 会在一次长帧(首场景装载、
    // 交互触发的重脚本)后一次性执行整段补帧,表现为“点一下就卡住”。
    // 桌面宿主同口径(set_max_step_ms(2 × frame_ms))。
    engine.set_max_step_ms((1000 / opts.fps.max(1)).max(1).saturating_mul(2));
    let viewport = engine.viewport();
    let boot_ms = engine.boot_ms();

    let listener = TcpListener::bind(("127.0.0.1", opts.port)).map_err(|e| {
        Failure::runtime(format!(
            "监听 127.0.0.1:{} 失败: {e}(端口被占用?用 --port 换一个)",
            opts.port
        ))
    })?;
    let port = listener
        .local_addr()
        .map_err(|e| Failure::runtime(format!("读取本地端口失败: {e}")))?
        .port();

    let (control_tx, control_rx) = mpsc::channel();
    let shared = Arc::new(Shared {
        status: Mutex::new(Status {
            marker: build_info::ENGINE_MARKER,
            backend: opts.backend.as_str(),
            viewport: (viewport.width, viewport.height),
            entry: opts.entry.clone(),
            fps: opts.fps,
            dataset: dataset.clone(),
            frame: 0,
            ops: 0,
            skipped: 0,
            executed: 0,
            ms: 0.0,
            tick_avg: 0.0,
            render_avg: 0.0,
            boot_ms,
            dropped_frames: 0,
            paused: false,
            report_json: "{\"ok\":false}".to_string(),
            pid: std::process::id(),
            started_ms: epoch_ms(),
        }),
        clients: Mutex::new(Vec::new()),
        sse: Mutex::new(Vec::new()),
        last_frame: Mutex::new(None),
        control: control_tx,
        diag_ws: opts.diag_ws,
        diag_json: Mutex::new("{\"ok\":false}".to_string()),
        diag_clients: AtomicUsize::new(0),
    });

    {
        let shared = shared.clone();
        let www = opts.www.clone();
        thread::spawn(move || accept_loop(listener, shared, www));
    }
    if opts.supervised {
        spawn_stdin_control(shared.clone());
    }

    let url = format!("http://127.0.0.1:{port}/");
    console.emit_host(
        yu_log::console::HostLevel::Info,
        "serve",
        &format!(
            "serving {url}  backend={} entry={} fps={} data={}:{} ({} bytes) boot_ms={:.2}",
            opts.backend.as_str(),
            opts.entry,
            opts.fps,
            dataset.kind,
            dataset.path.display(),
            dataset.bytes,
            boot_ms
        ),
    );
    console.emit_host(
        yu_log::console::HostLevel::Info,
        "serve",
        &format!("marker={} (HMR 可观测点)", build_info::ENGINE_MARKER),
    );
    if !opts.no_open {
        open_browser(&url);
    }
    engine_loop(
        engine,
        control_rx,
        shared,
        opts.fps,
        opts.backend,
        opts.pause_at,
        console,
    )
}

// ---------------------------------------------------------------- 共享状态

struct Status {
    marker: &'static str,
    backend: &'static str,
    viewport: (u32, u32),
    entry: String,
    fps: u32,
    dataset: DatasetInfo,
    frame: u64,
    ops: u32,
    skipped: u32,
    executed: u32,
    ms: f64,
    tick_avg: f64,
    render_avg: f64,
    boot_ms: f64,
    dropped_frames: u64,
    paused: bool,
    report_json: String,
    pid: u32,
    started_ms: u128,
}

impl Status {
    fn to_json(&self, clients: usize, uptime_ms: u128) -> String {
        format!(
            "{{\"ok\":true,\"marker\":\"{}\",\"backend\":\"{}\",\"frame\":{},\
             \"fps\":{},\"viewport\":[{},{}],\"entry\":\"{}\",\"dataset\":{},\
             \"clients\":{},\"dropped_frames\":{},\"uptime_ms\":{},\"pid\":{},\
             \"boot_ms\":{:.2},\"tick_ms_avg\":{:.2},\"render_ms_avg\":{:.2},\
             \"ops\":{},\"skipped\":{},\"executed\":{},\"ms\":{:.2},\"paused\":{}}}",
            json_escape(self.marker),
            json_escape(self.backend),
            self.frame,
            self.fps,
            self.viewport.0,
            self.viewport.1,
            json_escape(&self.entry),
            self.dataset.to_json(),
            clients,
            self.dropped_frames,
            uptime_ms,
            self.pid,
            self.boot_ms,
            self.tick_avg,
            self.render_avg,
            self.ops,
            self.skipped,
            self.executed,
            self.ms,
            self.paused
        )
    }
}

struct Client {
    id: u64,
    tx: SyncSender<Arc<Vec<u8>>>,
}

struct Shared {
    status: Mutex<Status>,
    clients: Mutex<Vec<Client>>,
    sse: Mutex<Vec<SyncSender<String>>>,
    /// 最近一帧已编码的二进制(新连接/重连立刻补发,暂停时也不空白)。
    last_frame: Mutex<Option<Arc<Vec<u8>>>>,
    control: Sender<Control>,
    /// `/diag` 诊断通道开关与最近一次诊断快照(H-P1-03;engine_loop 刷新)。
    diag_ws: bool,
    diag_json: Mutex<String>,
    /// 当前 `/diag` 订阅者数。无人订阅时引擎只排空诊断,不构造 JSON。
    diag_clients: AtomicUsize,
}

enum Control {
    Input {
        button: String,
        pressed: bool,
    },
    /// 单帧脉冲(滚轮一格):下一逻辑帧按下、随后自动抬起。
    Pulse {
        button: String,
    },
    Cursor {
        x: i64,
        y: i64,
    },
    Restart,
    /// 从当前状态步进 N 帧后暂停。
    Step(u64),
    /// 确定性回退:重启引擎并重放到目标帧后暂停(引擎无快照,回退 = 重放)。
    Replay(u64),
    Pause(bool),
    Reload,
    Shutdown,
}

fn status_json(shared: &Shared) -> String {
    let clients = shared.clients.lock().map(|c| c.len()).unwrap_or(0);
    let status = match shared.status.lock() {
        Ok(status) => status,
        Err(_) => return "{\"ok\":false}".to_string(),
    };
    let uptime = status.started_ms.max(epoch_ms()) - status.started_ms.min(epoch_ms());
    status.to_json(clients, uptime)
}

fn epoch_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

// ---------------------------------------------------------------- 引擎循环

fn engine_loop(
    mut engine: Engine,
    control_rx: Receiver<Control>,
    shared: Arc<Shared>,
    fps: u32,
    backend: Backend,
    pause_at: Option<u64>,
    console: yu_log::console::ConsoleHandle,
) -> CliResult<()> {
    let budget = Duration::from_micros(1_000_000 / u64::from(fps.max(1)));
    let mut next = Instant::now();
    let mut paused = false;
    let mut last_diag_snapshot = Instant::now()
        .checked_sub(Duration::from_millis(DIAG_PUSH_MS))
        .unwrap_or_else(Instant::now);
    // 步进/重放剩余帧数:Some(n) 时临时解除暂停,推进 n 帧后回到暂停。
    let mut step_left: Option<u64> = None;
    loop {
        while let Ok(control) = control_rx.try_recv() {
            match control {
                Control::Input { button, pressed } => engine.set_input(&button, pressed),
                Control::Pulse { button } => engine.pulse_input(&button),
                Control::Cursor { x, y } => engine.set_cursor(x, y),
                Control::Restart => {
                    console.emit_host(
                        yu_log::console::HostLevel::Info,
                        "serve",
                        "restart: 丢 Vm+GfxState,保留渲染器/纹理缓存",
                    );
                    engine.restart()?;
                    paused = false;
                    step_left = None;
                    if let Ok(mut last) = shared.last_frame.lock() {
                        *last = None;
                    }
                }
                Control::Step(frames) => {
                    console.emit_host(
                        yu_log::console::HostLevel::Info,
                        "serve",
                        &format!("step: +{} frame(s)", frames.max(1)),
                    );
                    paused = false;
                    step_left = Some(frames.max(1));
                }
                Control::Replay(target) => {
                    console.emit_host(
                        yu_log::console::HostLevel::Info,
                        "serve",
                        &format!("replay: restart → frame {target}"),
                    );
                    engine.restart()?;
                    if let Ok(mut last) = shared.last_frame.lock() {
                        *last = None;
                    }
                    paused = false;
                    step_left = Some(target.max(1));
                }
                Control::Pause(value) => {
                    paused = value;
                    console.emit_host(
                        yu_log::console::HostLevel::Info,
                        "serve",
                        if value { "paused" } else { "resumed" },
                    );
                }
                Control::Reload => {
                    broadcast_sse(&shared, "{\"type\":\"reload\"}");
                }
                Control::Shutdown => {
                    // 关闭时打这一轮的诊断汇总与排空计数(Q1 R2;Q1 自查:
                    // 无 /diag 订阅者时的 drain 现在有「丢了几条」的答案)。
                    console.emit_host(
                        yu_log::console::HostLevel::Info,
                        "serve",
                        &format!(
                            "shutdown: drained_diags={} sound_diags_dropped={}",
                            engine.diags_drained(),
                            engine.sound_diags_dropped()
                        ),
                    );
                    // 中毒恢复式取锁(与 ConsoleHandle 同型,不让一次
                    // panic 永久关闭汇总)。
                    let mut guard = console
                        .0
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    crate::logging::print_diag_summary(&mut guard, &mut engine);
                    console.emit_host(
                        yu_log::console::HostLevel::Info,
                        "serve",
                        "shutdown",
                    );
                    return Ok(());
                }
            }
        }

        let has_clients = shared
            .clients
            .lock()
            .map(|clients| !clients.is_empty())
            .unwrap_or(false);
        // 没有观众时不推进:游戏状态只在有人看时前进(输入也只会来自客户端)。
        if paused || !has_clients {
            // 暂停/无人观看：丢弃这段时间，恢复时不快进逻辑/声音时钟。
            engine.reset_realtime_clock();
            thread::sleep(Duration::from_millis(IDLE_SLEEP_MS));
            next = Instant::now();
            continue;
        }

        let rendered = engine.tick_realtime()?;
        if let Some(left) = step_left.as_mut() {
            *left = left.saturating_sub(1);
            if *left == 0 {
                paused = true;
                step_left = None;
                console.emit_host(
                    yu_log::console::HostLevel::Info,
                    "serve",
                    &format!("paused at frame {}", rendered.report.frame),
                );
            }
        }
        if let Some(target) = pause_at {
            if rendered.report.frame >= target && !paused {
                paused = true;
                console.emit_host(
                    yu_log::console::HostLevel::Info,
                    "serve",
                    &format!("paused at frame {} (--pause-at {target})", rendered.report.frame),
                );
            }
        }
        let payload = Arc::new(encode_frame(
            &rendered.report,
            backend.code(),
            rendered.report.drawable,
            paused,
            &rendered.surface,
        ));
        broadcast_frame(&shared, payload);
        update_status(&shared, &engine, paused);
        // H-P1-03:/diag 快照。只有**确实有订阅者**且距上次刷新 ≥500ms
        // 时才构造 DiagReport/JSON;无人订阅时只低成本排空内核缓冲
        // (否则每帧 format! 一遍全量诊断纯属浪费)。
        if shared.diag_clients.load(Ordering::Relaxed) > 0 {
            if last_diag_snapshot.elapsed() >= Duration::from_millis(DIAG_PUSH_MS) {
                let report = engine.take_diag_report();
                if let Ok(mut slot) = shared.diag_json.lock() {
                    *slot = diag_report_json(&report);
                }
                last_diag_snapshot = Instant::now();
            }
        } else {
            engine.drain_diags();
        }

        next += budget;
        let now = Instant::now();
        if next > now {
            thread::sleep(next - now);
        } else if now.duration_since(next) > budget * 4 {
            // 落后太多(调试/断点)就重置节拍,不补帧。
            next = now;
        }
    }
}

/// H-P1-03:`/diag` WebSocket —— 每 500ms 推一次 [`diag_report_json`]
/// 快照,直到客户端断开。www/egui 面板只读,不接受控制消息。
fn handle_diag_ws(
    stream: TcpStream,
    headers: &BTreeMap<String, String>,
    shared: Arc<Shared>,
) -> Result<(), String> {
    let (mut reader, mut writer) = ws::accept(stream, headers)?;
    shared.diag_clients.fetch_add(1, Ordering::Relaxed);
    // 连接存活期间保证计数回落(早退/写失败/读端关闭都走 Drop)。
    struct Subscription(Arc<Shared>);
    impl Drop for Subscription {
        fn drop(&mut self) {
            self.0.diag_clients.fetch_sub(1, Ordering::Relaxed);
        }
    }
    let _subscription = Subscription(shared.clone());
    // 关闭检测线程:读到 Close/错误即丢弃连接(写端 send 失败自然退出)。
    let closed = {
        let (tx, rx) = mpsc::sync_channel::<()>(1);
        thread::spawn(move || {
            loop {
                match reader.read_message() {
                    Ok(WsMessage::Ping(payload)) => {
                        // 无独立 writer 共享;ping 由读端忽略(诊断通道为
                        // 单向推送,浏览器默认心跳不依赖 pong)。
                        let _ = payload;
                    }
                    Ok(WsMessage::Close) | Err(_) => break,
                    _ => {}
                }
            }
            // 订阅端已断开时发送失败是常态:管道本身没有读者了,
            // 无从报告也无需报告(良性静默,Q1 自查 b 类)。
            let _ = tx.send(());
        });
        rx
    };
    writer.write_text("{\"ok\":true,\"channel\":\"diag\"}")?;
    loop {
        let snapshot = shared
            .diag_json
            .lock()
            .map(|slot| slot.clone())
            .unwrap_or_else(|_| "{\"ok\":false}".to_string());
        if writer.write_text(&snapshot).is_err() {
            break;
        }
        // 500ms 节拍;客户端断开(close 线程)或服务退出即收摊。
        if closed
            .recv_timeout(Duration::from_millis(DIAG_PUSH_MS))
            .is_ok()
        {
            break;
        }
    }
    Ok(())
}

/// [`crate::engine::DiagReport`] 的 JSON(www/egui 面板消费;字段稳定)。
fn diag_report_json(report: &crate::engine::DiagReport) -> String {
    let mut out = String::with_capacity(512);
    out.push_str("{\"ok\":true,\"frame\":");
    out.push_str(&report.frame.to_string());
    out.push_str(",\"waiting_tasks\":[");
    for (index, task) in report.waiting_tasks.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            "{{\"id\":\"{}\",\"main\":{},\"script\":{},\"pc\":{},\"wait_frames\":{},\
             \"deadline_ms\":{},\"text_rest\":{},\"enabled\":{}}}",
            json_escape(&task.id),
            task.main,
            task.script,
            task.pc,
            task.wait_frames,
            match task.deadline_ms {
                Some(ms) => ms.to_string(),
                None => "null".to_string(),
            },
            task.text_rest,
            task.enabled
        ));
    }
    out.push_str("],\"unimplemented_cmds\":[");
    for (index, (name, hits)) in report.unimplemented_cmds.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            "{{\"name\":\"{}\",\"hits\":{}}}",
            json_escape(name),
            hits
        ));
    }
    out.push_str(&format!(
        "],\"script_errors\":{},\"sound_layers\":{},\"save_slots\":{},\
         \"save_reservations\":{},\"save_calls\":{},\"load_calls\":{},\
         \"saveend_calls\":{},\"fileact_calls\":{},\"diags\":[",
        report.script_errors.len(),
        report.sound_layers,
        report.save_slots,
        report.save_reservations,
        report.save_calls,
        report.load_calls,
        report.saveend_calls,
        report.fileact_calls
    ));
    for (index, diag) in report.diags.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            "{{\"code\":\"{}\",\"severity\":\"{}\",\"detail\":\"{}\"}}",
            json_escape(diag.code.as_ref()),
            json_escape(diag.severity.as_str()),
            json_escape(&diag.detail)
        ));
    }
    out.push_str("]}");
    out
}

fn update_status(shared: &Shared, engine: &Engine, paused: bool) {
    let report = engine.last_report();
    if let Ok(mut status) = shared.status.lock() {
        status.frame = report.frame;
        status.ops = report.ops;
        status.skipped = report.skipped;
        status.executed = report.executed;
        status.ms = report.ms;
        status.tick_avg = engine.tick_avg_ms();
        status.render_avg = engine.render_avg_ms();
        status.paused = paused;
        status.report_json = render_report_json(engine.render_report());
    }
}

/// 最近一帧的完整后端报告(跳过原因/缺失资产;页面日志诊断用)。
fn render_report_json(report: Option<&RenderReport>) -> String {
    let Some(report) = report else {
        return "{\"ok\":false}".to_string();
    };
    let mut skipped = String::new();
    for (index, skip) in report.skipped.iter().enumerate() {
        if index > 0 {
            skipped.push(',');
        }
        skipped.push_str(&format!(
            "{{\"kind\":\"{}\",\"count\":{},\"reason\":\"{}\",\"target\":\"{}\"}}",
            json_escape(skip.kind),
            skip.count,
            json_escape(&skip.reason),
            json_escape(&skip.target)
        ));
    }
    let mut missing = String::new();
    for (index, issue) in report.missing_assets.iter().enumerate() {
        if index > 0 {
            missing.push(',');
        }
        missing.push_str(&format!(
            "{{\"key\":\"{}\",\"code\":\"{}\",\"detail\":\"{}\"}}",
            json_escape(&issue.key_display()),
            json_escape(&issue.code),
            json_escape(&issue.detail)
        ));
    }
    format!(
        concat!(
            "{{\"ok\":true,\"frame\":{},\"layers\":{},\"visible\":{},\"passes\":{},",
            "\"offscreen\":{},\"ops_total\":{},\"ops_drawn\":{},\"skipped_total\":{},",
            "\"text_runs\":{},\"skipped\":[{skipped}],\"missing\":[{missing}]}}"
        ),
        report.frame,
        report.layers,
        report.visible_layers,
        report.passes,
        report.offscreen_passes,
        report.ops_total,
        report.ops_drawn,
        report.skipped_ops_total,
        report.skipped_text,
        skipped = skipped,
        missing = missing
    )
}

/// 二进制帧:`YUF1` + 56 字节头 + RGBA(见 README 协议)。
fn encode_frame(
    report: &FrameReport,
    backend: u8,
    drawable: bool,
    paused: bool,
    surface: &yu_backend_soft::Surface,
) -> Vec<u8> {
    const HEADER: usize = 56;
    let mut out = Vec::with_capacity(HEADER + surface.rgba.len());
    out.extend_from_slice(b"YUF1");
    out.extend_from_slice(&surface.width.to_le_bytes());
    out.extend_from_slice(&surface.height.to_le_bytes());
    out.extend_from_slice(&report.frame.to_le_bytes());
    out.extend_from_slice(&report.ops.to_le_bytes());
    out.extend_from_slice(&report.skipped.to_le_bytes());
    out.extend_from_slice(&report.executed.to_le_bytes());
    out.extend_from_slice(&report.passes.to_le_bytes());
    out.extend_from_slice(&report.layers.to_le_bytes());
    out.extend_from_slice(&report.visible.to_le_bytes());
    out.extend_from_slice(&(report.ms.round() as u32).to_le_bytes());
    out.push(backend);
    out.push(u8::from(drawable) | (u8::from(true) << 1) | (u8::from(paused) << 2));
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(surface.rgba.len() as u32).to_le_bytes());
    debug_assert_eq!(out.len(), HEADER);
    out.extend_from_slice(&surface.rgba);
    out
}

fn broadcast_frame(shared: &Shared, payload: Arc<Vec<u8>>) {
    if let Ok(mut last) = shared.last_frame.lock() {
        *last = Some(Arc::clone(&payload));
    }
    let mut dropped = 0u64;
    if let Ok(mut clients) = shared.clients.lock() {
        clients.retain(|client| match client.tx.try_send(payload.clone()) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                dropped += 1;
                true
            }
            Err(TrySendError::Disconnected(_)) => false,
        });
    }
    if dropped > 0 {
        if let Ok(mut status) = shared.status.lock() {
            status.dropped_frames += dropped;
        }
    }
}

fn broadcast_sse(shared: &Shared, event: &str) {
    if let Ok(mut subscribers) = shared.sse.lock() {
        subscribers.retain(|tx| tx.try_send(event.to_string()).is_ok());
    }
}

// ---------------------------------------------------------------- HTTP

struct Request {
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
}

fn accept_loop(listener: TcpListener, shared: Arc<Shared>, www: PathBuf) {
    for incoming in listener.incoming() {
        match incoming {
            Ok(stream) => {
                let shared = shared.clone();
                let www = www.clone();
                thread::spawn(move || {
                    let _ = handle_connection(stream, shared, &www);
                });
            }
            Err(error) => {
                output::error(format!("[serve] accept: {error}"));
                break;
            }
        }
    }
}

fn handle_connection(mut stream: TcpStream, shared: Arc<Shared>, www: &Path) -> Result<(), String> {
    let request = read_request(&mut stream)?;
    let path = request
        .path
        .split('?')
        .next()
        .unwrap_or(&request.path)
        .to_string();
    match (request.method.as_str(), path.as_str()) {
        ("GET", "/ws") => handle_ws(stream, &request.headers, shared),
        ("GET", "/diag") => {
            if shared.diag_ws {
                handle_diag_ws(stream, &request.headers, shared)
            } else {
                respond(
                    stream,
                    "404 Not Found",
                    "text/plain; charset=utf-8",
                    b"diag channel disabled (start with --diag-ws)",
                )
            }
        }
        ("GET", "/events") => handle_sse(stream, shared),
        ("GET", "/api/status") => {
            let body = status_json(&shared);
            respond(
                stream,
                "200 OK",
                "application/json; charset=utf-8",
                body.as_bytes(),
            )
        }
        ("GET", "/api/report") => {
            let body = shared
                .status
                .lock()
                .map(|status| status.report_json.clone())
                .unwrap_or_else(|_| "{\"ok\":false}".to_string());
            respond(
                stream,
                "200 OK",
                "application/json; charset=utf-8",
                body.as_bytes(),
            )
        }
        ("GET", "/healthz") => respond(stream, "200 OK", "text/plain; charset=utf-8", b"ok"),
        ("GET", _) => serve_static(stream, www, &path),
        _ => respond(
            stream,
            "405 Method Not Allowed",
            "text/plain; charset=utf-8",
            b"method not allowed",
        ),
    }
}

fn read_request(stream: &mut TcpStream) -> Result<Request, String> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 2048];
    let header_end = loop {
        let read = stream
            .read(&mut chunk)
            .map_err(|e| format!("http: read failed: {e}"))?;
        if read == 0 {
            return Err("http: connection closed before headers".to_string());
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(pos) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
        if buffer.len() > 64 * 1024 {
            return Err("http: header too large".to_string());
        }
    };
    let text = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let mut lines = text.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| "http: missing method".to_string())?
        .to_string();
    let path = parts
        .next()
        .ok_or_else(|| "http: missing path".to_string())?
        .to_string();
    let mut headers = BTreeMap::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    Ok(Request {
        method,
        path,
        headers,
    })
}

fn respond(
    mut stream: TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
) -> Result<(), String> {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
         Cache-Control: no-cache\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(head.as_bytes())
        .and_then(|_| stream.write_all(body))
        .and_then(|_| stream.flush())
        .map_err(|e| format!("http: write failed: {e}"))
}

fn serve_static(stream: TcpStream, www: &Path, path: &str) -> Result<(), String> {
    let relative = if path == "/" {
        "index.html"
    } else {
        path.trim_start_matches('/')
    };
    let safe = !relative.is_empty()
        && relative
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '/'))
        && relative
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..");
    if !safe {
        return respond(
            stream,
            "404 Not Found",
            "text/plain; charset=utf-8",
            b"not found",
        );
    }
    let full = www.join(relative);
    let bytes = match std::fs::read(&full) {
        Ok(bytes) => bytes,
        Err(_) => {
            return respond(
                stream,
                "404 Not Found",
                "text/plain; charset=utf-8",
                b"not found",
            )
        }
    };
    let content_type = match full
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "wasm" => "application/wasm",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        _ => "application/octet-stream",
    };
    respond(stream, "200 OK", content_type, &bytes)
}

// ---------------------------------------------------------------- WebSocket

fn handle_ws(
    stream: TcpStream,
    headers: &BTreeMap<String, String>,
    shared: Arc<Shared>,
) -> Result<(), String> {
    let (mut reader, writer) = ws::accept(stream, headers)?;
    let id = NEXT_CLIENT.fetch_add(1, Ordering::Relaxed);
    let writer = Arc::new(Mutex::new(writer));
    let (tx, rx) = mpsc::sync_channel::<Arc<Vec<u8>>>(2);
    if let Ok(mut clients) = shared.clients.lock() {
        clients.push(Client { id, tx });
    }
    if let Ok(mut writer) = writer.lock() {
        writer.write_text(&status_json(&shared))?;
        // 暂停/重连:立刻补发最近一帧,避免新页面空白等待下一次 tick。
        let last = shared
            .last_frame
            .lock()
            .ok()
            .and_then(|guard| guard.clone());
        if let Some(frame) = last {
            writer.write_binary(&frame)?;
        }
    }

    // 读线程:文本 JSON → 控制通道;连接结束就注销客户端。
    {
        let control = shared.control.clone();
        let clients = Arc::clone(&shared);
        let writer = Arc::clone(&writer);
        thread::spawn(move || {
            loop {
                match reader.read_message() {
                    Ok(WsMessage::Text(text)) => {
                        if let Some(control_message) = parse_control(&text) {
                            if control.send(control_message).is_err() {
                                break;
                            }
                        }
                    }
                    Ok(WsMessage::Ping(payload)) => {
                        if let Ok(mut guard) = writer.lock() {
                            let _ = guard.write_pong(&payload);
                        }
                    }
                    Ok(WsMessage::Binary(_)) => {}
                    Ok(WsMessage::Close) | Err(_) => break,
                }
            }
            if let Ok(mut clients) = clients.clients.lock() {
                clients.retain(|client| client.id != id);
            }
        });
    }

    while let Ok(payload) = rx.recv() {
        let write = {
            let mut guard = match writer.lock() {
                Ok(guard) => guard,
                Err(_) => break,
            };
            guard.write_binary(&payload)
        };
        if write.is_err() {
            break;
        }
    }
    if let Ok(mut clients) = shared.clients.lock() {
        clients.retain(|client| client.id != id);
    }
    Ok(())
}

fn parse_control(text: &str) -> Option<Control> {
    let value = crate::json::parse(text).ok()?;
    let kind = value.get("t")?.as_str()?;
    match kind {
        // 兼容旧页面/脚本:直接给引擎按钮名。
        "input" => Some(Control::Input {
            button: value.get("button")?.as_str()?.to_string(),
            pressed: value.get("pressed")?.as_bool()?,
        }),
        "pulse" => Some(Control::Pulse {
            button: value.get("button")?.as_str()?.to_string(),
        }),
        // 平台原始事件:映射只在本进程(`yu-platform-input`),页面不复制键表。
        "key" => Some(Control::Input {
            button: key_button(value.get("code")?.as_str()?)?,
            pressed: value.get("pressed")?.as_bool()?,
        }),
        "pointer" => {
            let button = PointerButton::from_index(value.get("button")?.as_f64()? as i64)?;
            Some(Control::Input {
                button: button.name()?.to_string(),
                pressed: value.get("pressed")?.as_bool()?,
            })
        }
        "wheel" => Some(Control::Pulse {
            button: wheel_button(value.get("dy")?.as_f64()?)?.to_string(),
        }),
        "cursor" => Some(Control::Cursor {
            x: value.get("x")?.as_f64()? as i64,
            y: value.get("y")?.as_f64()? as i64,
        }),
        "restart" => Some(Control::Restart),
        "step" => Some(Control::Step((value.get("n")?.as_f64()? as u64).max(1))),
        "back" => Some(Control::Replay(
            (value.get("target")?.as_f64()? as u64).max(1),
        )),
        "pause" => Some(Control::Pause(true)),
        "resume" => Some(Control::Pause(false)),
        _ => None,
    }
}

// ---------------------------------------------------------------- SSE

fn handle_sse(mut stream: TcpStream, shared: Arc<Shared>) -> Result<(), String> {
    let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream; charset=utf-8\r\n\
                Cache-Control: no-cache\r\nConnection: keep-alive\r\n\r\n";
    stream
        .write_all(head.as_bytes())
        .map_err(|e| format!("sse: write failed: {e}"))?;
    let (tx, rx) = mpsc::sync_channel::<String>(8);
    if let Ok(mut subscribers) = shared.sse.lock() {
        subscribers.push(tx);
    }
    let hello = format!("data: {}\n\n", status_json(&shared));
    if stream.write_all(hello.as_bytes()).is_err() {
        return Ok(());
    }
    loop {
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(event) => {
                let line = format!("data: {event}\n\n");
                if stream
                    .write_all(line.as_bytes())
                    .and_then(|_| stream.flush())
                    .is_err()
                {
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if stream.write_all(b": keepalive\n\n").is_err() {
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- 控制

fn spawn_stdin_control(shared: Arc<Shared>) {
    thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut line = String::new();
        loop {
            line.clear();
            match stdin.read_line(&mut line) {
                Ok(0) => {
                    // 监督进程退出/管道关闭:不要留孤儿引擎。
                    let _ = shared.control.send(Control::Shutdown);
                    break;
                }
                Ok(_) => {
                    let command = line.trim();
                    let control = match command {
                        "reload" => Some(Control::Reload),
                        "restart" => Some(Control::Restart),
                        "quit" | "shutdown" => Some(Control::Shutdown),
                        _ => None,
                    };
                    if let Some(control) = control {
                        if shared.control.send(control).is_err() {
                            break;
                        }
                    }
                }
                Err(_) => break,
            }
        }
    });
}

fn open_browser(url: &str) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yu_backend_soft::Surface;

    #[test]
    fn frame_header_layout_is_stable() {
        let surface = Surface::new(2, 1, [1, 2, 3, 255]);
        let report = FrameReport {
            frame: 7,
            ops: 5,
            skipped: 2,
            executed: 3,
            passes: 4,
            layers: 9,
            visible: 6,
            tick_ms: 1.0,
            render_ms: 2.0,
            ms: 3.0,
            drawable: true,
        };
        let bytes = encode_frame(&report, 1, true, false, &surface);
        assert_eq!(&bytes[0..4], b"YUF1");
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), 1);
        assert_eq!(u64::from_le_bytes(bytes[12..20].try_into().unwrap()), 7);
        assert_eq!(bytes[48], 1);
        assert_eq!(bytes[49] & 1, 1);
        assert_eq!(
            u32::from_le_bytes(bytes[52..56].try_into().unwrap()) as usize,
            surface.rgba.len()
        );
        assert_eq!(bytes.len(), 56 + surface.rgba.len());
        assert_eq!(&bytes[56..], &surface.rgba[..]);
    }
}
