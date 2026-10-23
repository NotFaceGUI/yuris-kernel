//! `yu-desktop` · 原生桌面窗口入口。
//!
//! ```text
//! yu-desktop <project-root> [--entry SYSTEM_START] [--data <path>]
//!            [--backend wgpu|soft] [--title <text>]
//!            [--frames N] [--shot <png>] [--width N] [--height N]
//!            [--fps N] [--fullscreen] [--pixel] [--debug-ui]
//!            [--no-sound] [--reconcile]
//!            [--script <case.json>] [--panic-selftest N] [--log-file <path>] [--save-root <dir>]
//! ```
//!
//! 默认 `--backend wgpu`;GPU 初始化失败自动回退 soft。`--frames` 用于
//! 无头验收,`--shot` 把最后一帧写成 PNG(与 `yu-cli run --shot` 对比)。
//!
//! # 日志
//!
//! 入口最先初始化 `yu_log::facade`(P4 M-E 的桥):wgpu/winit 的
//! `log::error!` 与宿主/panic 防线写同一份 JSONL。文件路径:
//!
//! 1. `--log-file <path>`;
//! 2. 环境变量 `YU_LOG_JSONL`(显式给值时不覆盖);
//! 3. 默认 `%TEMP%/yu-desktop-host.jsonl`(每次启动截断)。
//!
//! 级别由 `YU_LOG` 控制(`error|warn|info|debug|trace`,缺省 `warn`);
//! 复现剧本想看逐帧动作时用 `YU_LOG=info`。

use std::path::{Path, PathBuf};

use yu_cli::args::ArgReader;
use yu_cli::engine::{Backend, Engine, EngineOptions};
use yu_cli::project::Project;
use yu_host_desktop::HostOptions;

macro_rules! host_error {
    ($($arg:tt)*) => { yu_cli::output::error(format!($($arg)*)) };
}
macro_rules! host_line {
    ($($arg:tt)*) => { yu_cli::output::line(format!($($arg)*)) };
}

/// `yu-log` 门面的控制台出口(项目纪律:`src/` 不直接用 stdout 宏)。
fn facade_console(line: &str) {
    yu_cli::output::error(line);
}

fn main() {
    if cfg!(debug_assertions) {
        host_error!(
            "yu-desktop: debug build (VM/render 5–10x slower than release); \
             use `cargo build --release -p yu-host-desktop` for playable fps"
        );
    }
    if let Err(error) = run() {
        host_error!("yu-desktop: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let mut args = ArgReader::new(std::env::args().skip(1).collect());
    let root = args.positional(0).ok_or_else(usage)?;
    let entry = args
        .flag("entry")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "SYSTEM_START".to_string());
    let data = args.flag("data").map(PathBuf::from);
    let backend = match args.flag("backend").as_deref() {
        None | Some("wgpu") => Backend::Wgpu,
        Some("soft") => Backend::Soft,
        Some(other) => {
            return Err(format!("--backend 只接受 wgpu|soft,收到 `{other}`"));
        }
    };
    let title = args.flag("title").filter(|value| !value.is_empty());
    let width = parse_opt::<u32>(&mut args, "width")?;
    let height = parse_opt::<u32>(&mut args, "height")?;
    let fps = parse_opt::<u32>(&mut args, "fps")?.unwrap_or(60).max(1);
    let frames = match parse_opt::<u64>(&mut args, "frames")? {
        Some(0) | None => None,
        Some(limit) => Some(limit),
    };
    let shot = args.flag("shot").map(PathBuf::from);
    let fullscreen = args.switch("fullscreen");
    let pixel = args.switch("pixel");
    let debug_ui = args.switch("debug-ui");
    let no_sound = args.switch("no-sound");
    let reconcile = args.switch("reconcile");
    let script = args.flag("script").map(PathBuf::from);
    let panic_selftest = parse_opt::<u64>(&mut args, "panic-selftest")?;
    let log_file = args.flag("log-file").map(PathBuf::from);
    // 性能证据(只写调用方给的路径;默认不产生文件):
    // `--frame-stats` 逐帧 CSV;`--profile-frames 1,4,340` 在指定宿主帧
    // 打印 VM 命令耗时 top-10。
    let frame_stats = args.flag("frame-stats").map(PathBuf::from);
    let pump_cap = parse_opt::<usize>(&mut args, "pump-cap")?;
    let profile_frames = match args.flag("profile-frames") {
        Some(list) => list
            .split(',')
            .filter(|part| !part.trim().is_empty())
            .map(|part| {
                part.trim()
                    .parse::<u64>()
                    .map_err(|_| format!("--profile-frames 需要逗号分隔的帧号,收到 `{part}`"))
            })
            .collect::<Result<std::collections::BTreeSet<_>, _>>()?,
        None => std::collections::BTreeSet::new(),
    };
    // P4 M-C:存档根。默认 `None` = 工程 `save/` 只读观察(不写语料);
    // 显式给目录才允许存档域在该目录落盘。
    let save_root = args.flag("save-root").map(PathBuf::from);
    args.finish().map_err(|error| error.to_string())?;

    // 日志/panic 防线必须早于 boot:引擎/GPU 初始化期的 panic 也要留现场。
    let log_path = init_logging(log_file.as_deref())?;
    yu_host_desktop::panic_guard::install();
    host_line!(
        "yu-desktop: log file {} (level {})",
        log_path.display(),
        std::env::var("YU_LOG").unwrap_or_else(|_| "warn(default)".to_string())
    );

    let project =
        Project::discover(Path::new(&root), data.as_deref()).map_err(|error| error.to_string())?;
    let title = title.unwrap_or_else(|| {
        let name = project
            .root()
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.clone());
        format!("YU-RIS · {name}")
    });

    let mut options = EngineOptions {
        backend,
        entry,
        record_events: false,
        fps,
        debug_mode: false,
        screen_color: 32,
        dx_mode: 0,
        os_sound: !no_sound,
        media_readonly: false,
        reconcile,
        save_root,
        // 桌面宿主:启动时预枚举系统字体,避免第一次打开テキスト页签
        // 付 ~130ms 的 fontdb 枚举(实测)。
        warm_text: true,
    };
    let engine = match Engine::new(project.clone(), options.clone()) {
        Ok(engine) => engine,
        Err(first) if backend == Backend::Wgpu => {
            host_error!("yu-desktop: wgpu 启动失败({first});回退 soft");
            options.backend = Backend::Soft;
            Engine::new(project, options).map_err(|error| error.to_string())?
        }
        Err(error) => return Err(error.to_string()),
    };

    yu_host_desktop::run(
        engine,
        HostOptions {
            title,
            width,
            height,
            fps,
            frames,
            shot,
            fullscreen,
            pixel,
            debug_ui,
            script,
            panic_selftest,
            frame_stats,
            profile_frames,
            pump_cap,
        },
    )
}

/// 初始化 `yu-log` 门面桥,返回实际使用的 JSONL 路径。
///
/// 只在调用方没有显式设置 `YU_LOG_JSONL` 时填默认值(不覆盖用户的
/// 环境语义);`--log-file` 优先于环境变量。
fn init_logging(cli: Option<&Path>) -> Result<PathBuf, String> {
    let path = match cli {
        Some(path) => path.to_path_buf(),
        None => match std::env::var("YU_LOG_JSONL") {
            Ok(value) if !value.trim().is_empty() => PathBuf::from(value.trim()),
            _ => std::env::temp_dir().join("yu-desktop-host.jsonl"),
        },
    };
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("创建日志目录 {} 失败: {e}", parent.display()))?;
        }
    }
    // 每次启动截断:一次运行 = 一个现场,避免上一轮 panic 混进本轮证据。
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => host_error!("yu-desktop: 无法清理旧日志 {}: {error}", path.display()),
    }
    std::env::set_var("YU_LOG_JSONL", &path);
    if std::env::var_os("YU_LOG").is_none() {
        std::env::set_var("YU_LOG", "warn");
    }
    let level = yu_log::facade::init(facade_console);
    if level.is_none() {
        // 已有 logger(测试/进程内重入):保持现状,但至少把路径报出来。
        host_error!("yu-desktop: log 门面已被占用,沿用现有 sink");
    }
    Ok(path)
}

fn parse_opt<T>(args: &mut ArgReader, name: &str) -> Result<Option<T>, String>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    match args.flag(name) {
        None => Ok(None),
        Some(value) => value
            .parse::<T>()
            .map(Some)
            .map_err(|error| format!("--{name} 解析失败: {error}")),
    }
}

fn usage() -> String {
    "用法: yu-desktop <project-root> [--entry NAME] [--data PATH] [--backend wgpu|soft]\n\
     \x20      [--title TEXT] [--frames N] [--shot PNG] [--width N] [--height N]\n\
     \x20      [--fps N] [--fullscreen] [--pixel] [--debug-ui] [--no-sound] [--reconcile]\n\
     \x20      [--script case.json] [--panic-selftest N] [--log-file PATH] [--save-root DIR]\n\
     \x20      [--frame-stats CSV] [--profile-frames 1,4,340] [--pump-cap N]"
        .to_string()
}
