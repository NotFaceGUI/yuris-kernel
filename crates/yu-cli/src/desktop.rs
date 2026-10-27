//! `yu-cli run --mode desktop` · 桌面窗口模式(原 `yu-desktop` exe 合并,H3)。
//!
//! 参数与 headless **按模式解耦**:本模块只认桌面旗标(与 [`crate::run`]
//! 的 headless 旗标互不认识,写错边显式报「未知参数」);共用旗标
//! (entry/data/backend/frames/shot/save-root/debug-mode)两边都认。
//!
//! 声音走平台端口:cpal `SoundSink` 实现 `yu_engine::engine::AudioOut`,
//! 经 `Engine::attach_audio` 注入;`--no-sound` 只是不注入,**不影响**
//! `@_OS_SOUND` 注入口径(恒 `true`,对拍语义不变)。

use std::path::{Path, PathBuf};

use yu_engine::engine::{Backend, Engine, EngineOptions};
use yu_engine::project::Project;
use yu_engine::{CliResult, Failure};

use crate::args::ArgReader;

/// `--mode desktop` 的参数解析与启动(整条命令行由本函数独占)。
pub fn from_args(tokens: Vec<String>) -> CliResult<()> {
    let mut args = ArgReader::new(tokens);
    // 分派已在 `run` 里确认 `--mode desktop`;这里消费掉该旗标本体。
    let _ = args.flag("mode");
    let root = args
        .positional(0)
        .ok_or_else(|| Failure::usage(usage()))?;
    let entry = args
        .flag("entry")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "SYSTEM_START".to_string());
    let data = args.flag("data").map(PathBuf::from);
    // 桌面缺省 wgpu、headless 缺省 soft —— 这是**模式策略**,不是两套解析:
    // 旗标取值统一交给 `Backend::parse`,避免两处校验文案/取值漂移。
    let backend = match args.flag("backend") {
        Some(value) => Backend::parse(&value)?,
        None => Backend::Wgpu,
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
                    .map_err(|_| Failure::usage(format!("--profile-frames 需要逗号分隔的帧号,收到 `{part}`")))
            })
            .collect::<Result<std::collections::BTreeSet<_>, _>>()?,
        None => std::collections::BTreeSet::new(),
    };
    // 与 headless 同口径的共用旗标。
    let debug_mode = match args.flag("debug-mode") {
        Some(value) => ArgReader::on_off("debug-mode", &value)?,
        // 桌面宿主同口径:YURIS_DEBUG_MODE=1 走 debug 分支(DIV-L-001)。
        None => std::env::var("YURIS_DEBUG_MODE")
            .map(|value| value == "1")
            .unwrap_or(false),
    };
    // 存档根(P4 M-C):缺省 = 工程根 `save/`;显式给目录才重定向。
    let save_root = args.flag("save-root").map(PathBuf::from);
    args.finish()?;

    // 统一控制台(Q1 R2/R4):日志/panic 防线必须早于 boot;JSONL 单写者
    // 同时接门面行(sub:"log")与内核 Diag 行(sub:"diag")。
    let mut log_warning: Option<String> = None;
    let opened = match init_logging(log_file.as_deref())? {
        LogFileInit::Opened(path, file) => Some((path, file)),
        LogFileInit::Degraded(warning) => {
            log_warning = Some(warning);
            None
        }
    };
    let console = yu_log::console::ConsoleHandle::new(yu_log::console::Console::stderr_live(
        crate::logging::min_level(),
        LOG_RECENT_CAP,
        opened.as_ref().map(|(_, file)| file.clone()),
    ));
    // 日志文件打不开不许静默(旧代码在此静默禁用文件日志——Q1 自查 a 类);
    // 但默认路径失败不挡运行,降级为「无文件日志」并 WARN 留痕。
    if cfg!(debug_assertions) {
        console.emit_host(
            yu_log::console::HostLevel::Warn,
            "host",
            "debug build (VM/render 5–10x slower than release); \
             use `cargo build --release -p yu-cli` for playable fps",
        );
    }
    if let Some(warning) = &log_warning {
        console.emit_host(yu_log::console::HostLevel::Warn, "host", warning);
    } else if let Some(reason) = yu_log::console::color_auto_off_reason() {
        // 无文件时 auto 关色的原因也要有出处(契约:不允许静默无色)。
        console.emit_host(
            yu_log::console::HostLevel::Info,
            "host",
            &format!("color auto→off: {reason}"),
        );
    }
    if let Some((log_path, file)) = &opened {
        crate::logging::write_color_notice(file);
        console.emit_host(
            yu_log::console::HostLevel::Info,
            "host",
            &format!(
                "log file {} (facade level {})",
                log_path.display(),
                std::env::var("YU_LOG").unwrap_or_else(|_| "warn(default)".to_string())
            ),
        );
    }
    yu_host_desktop::panic_guard::install();
    // 门面桥共享同一个 Console 与 JSONL(单写者);失败 = 已被占用(进程内
    // 重入),沿用现有 sink 但要留痕。
    let facade_console = console.clone();
    if yu_log::facade::init_with(
        std::sync::Arc::new(move |level, target, message| {
            facade_console.emit_facade(level, target, message);
        }),
        opened.as_ref().map(|(_, file)| file.clone()),
    )
    .is_none()
    {
        console.emit_host(
            yu_log::console::HostLevel::Warn,
            "host",
            "log 门面已被占用,沿用现有 sink",
        );
    }

    let project =
        Project::discover(Path::new(&root), data.as_deref()).map_err(|error| error.to_string())?;
    // 决策分支留痕(减少排查时间):数据入口选了哪条、调试口径、帧率——
    // 排查「为什么行为不同」时第一眼就要看的事实。
    {
        let dataset = project.describe();
        console.emit_host(
            yu_log::console::HostLevel::Info,
            "host",
            &format!(
                "boot: entry={entry} data={}:{} ({} bytes) debug_mode={} fps={fps} backend={}",
                dataset.kind,
                dataset.path.display(),
                dataset.bytes,
                if debug_mode { "on" } else { "off" },
                backend.as_str(),
            ),
        );
    }
    let title = title.unwrap_or_else(|| {
        let name = project
            .root()
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.clone());
        format!("YU-RIS · {name}")
    });

    let options = EngineOptions {
        backend,
        entry,
        record_events: false,
        fps,
        debug_mode,
        screen_color: 32,
        dx_mode: 0,
        // 解耦决策(2026-10-25 v2):`@_OS_SOUND` 是 VM 可见变量,注入恒按
        // 对拍口径 `true`;`--no-sound` 只控制是否注入 cpal 出声端口。
        os_sound: true,
        media_readonly: false,
        reconcile,
        save_root,
        // 桌面宿主:启动时预枚举系统字体,避免第一次打开テキスト页签
        // 付 ~130ms 的 fontdb 枚举(实测)。
        warm_text: true,
        force_font: None,
        // 真实运行:`@_TIME` 注入系统本地时间(存档菜单的现实日期)。
        local_time: crate::clock::local_now(),
    };
    // **不做 GPU→CPU 自动回退**:soft 与 wgpu 是两条独立实现,自动降级会让
    // 「GPU 模式变慢/画错」永远对不上号(用户看到的是 CPU 结果,却以为在
    // 看 GPU)。要 CPU 模式请显式 `--backend soft`。
    let mut engine = Engine::new(project.clone(), options.clone()).map_err(|error| {
        if backend == Backend::Wgpu {
            format!(
                "wgpu 后端启动失败:{error}
                 本内核不会自动回退到 CPU 模式;确需 CPU 渲染请显式加 --backend soft。"
            )
        } else {
            error.to_string()
        }
    })?;
    // 平台音频端口:cpal 回调即时钟(`AudioOut`;headless 从不注入)。
    if !no_sound {
        match yu_host_desktop::SoundSink::open() {
            Ok(sink) => engine.attach_audio(Box::new(sink)),
            Err(error) => {
                console.emit_host(
                    yu_log::console::HostLevel::Warn,
                    "sound",
                    &format!("设备输出未启用: {error}"),
                );
            }
        }
    }

    yu_host_desktop::run(
        engine,
        yu_host_desktop::HostOptions {
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
            console: Some(console),
        },
    )
    .map_err(|error| Failure::runtime(error))
}

/// 桌面面板「日志」页签的环形缓冲容量(明文无色;清空只影响面板)。
const LOG_RECENT_CAP: usize = 512;

/// 初始化共享 JSONL 写者(Q1 R4)。
///
/// 只在调用方没有显式设置 `YU_LOG_JSONL` 时填默认值(不覆盖用户的
/// 环境语义);`--log-file` 优先于环境变量。每轮截断:一次运行 = 一个现场。
///
/// 失败语义(Q1 自查 a 类,不许静默):
/// - 显式 `--log-file` 打不开 = 硬失败(用户点名的路径,`Err` 上抛);
/// - 缺省路径(环境变量/系统 TEMP)打不开 = [`LogFileInit::Degraded`]
///   WARN 留痕后**无文件日志继续**(旧代码此处静默禁用文件日志,
///   正是本任务要消灭的降级)。
enum LogFileInit {
    Opened(PathBuf, yu_log::console::JsonlFile),
    /// 缺省路径打不开;载荷 = 给控制台的 WARN 文案。
    Degraded(String),
}

fn init_logging(cli: Option<&Path>) -> Result<LogFileInit, String> {
    let Some(path) = cli else {
        let path = match std::env::var("YU_LOG_JSONL") {
            Ok(value) if !value.trim().is_empty() => PathBuf::from(value.trim()),
            _ => std::env::temp_dir().join("yu-cli-desktop.jsonl"),
        };
        return match crate::logging::open_jsonl_truncating(&path) {
            Ok(file) => {
                std::env::set_var("YU_LOG_JSONL", &path);
                Ok(LogFileInit::Opened(path, file))
            }
            Err(open_error) => Ok(LogFileInit::Degraded(format!(
                "{open_error};本运行不落文件日志(--log-file 可重定向)"
            ))),
        };
    };
    let file = crate::logging::open_jsonl_truncating(path)?;
    std::env::set_var("YU_LOG_JSONL", path);
    Ok(LogFileInit::Opened(path.to_path_buf(), file))
}

fn parse_opt<T>(args: &mut ArgReader, name: &str) -> Result<Option<T>, Failure>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    match args.flag(name) {
        None => Ok(None),
        Some(value) => value
            .parse::<T>()
            .map(Some)
            .map_err(|error| Failure::usage(format!("--{name} 解析失败: {error}"))),
    }
}

fn usage() -> String {
    "用法: yu-cli run <project-root> --mode desktop [--entry NAME] [--data PATH]\n\
     \x20      [--backend wgpu|soft] [--title TEXT] [--width N] [--height N] [--fps N]\n\
     \x20      [--frames N] [--shot PNG] [--fullscreen] [--pixel] [--debug-ui]\n\
     \x20      [--no-sound] [--reconcile] [--script case.json] [--save-root DIR]\n\
     \x20      [--log-file PATH] [--frame-stats CSV] [--profile-frames 1,4,340] [--pump-cap N]"
        .to_string()
}
