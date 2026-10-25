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
use yu_engine::{output, CliResult, Failure};

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
    // 桌面缺省 wgpu(headless 缺省 soft):交互帧率要 GPU;失败自动回退。
    let backend = match args.flag("backend").as_deref() {
        None | Some("wgpu") => Backend::Wgpu,
        Some("soft") => Backend::Soft,
        Some(other) => {
            return Err(Failure::usage(format!(
                "--backend 只接受 wgpu|soft,收到 `{other}`"
            )))
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

    if cfg!(debug_assertions) {
        output::error(
            "yu-cli --mode desktop: debug build (VM/render 5–10x slower than release); \
             use `cargo build --release -p yu-cli` for playable fps"
                .to_string(),
        );
    }

    // 日志/panic 防线必须早于 boot:引擎/GPU 初始化期的 panic 也要留现场。
    let log_path = init_logging(log_file.as_deref())?;
    yu_host_desktop::panic_guard::install();
    output::line(format!(
        "yu-cli --mode desktop: log file {} (level {})",
        log_path.display(),
        std::env::var("YU_LOG").unwrap_or_else(|_| "warn(default)".to_string())
    ));

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
    };
    let mut engine = match Engine::new(project.clone(), options.clone()) {
        Ok(engine) => engine,
        Err(first) if backend == Backend::Wgpu => {
            output::error(format!("--mode desktop: wgpu 启动失败({first});回退 soft"));
            options.backend = Backend::Soft;
            Engine::new(project, options).map_err(|error| error.to_string())?
        }
        Err(error) => return Err(error),
    };
    // 平台音频端口:cpal 回调即时钟(`AudioOut`;headless 从不注入)。
    if !no_sound {
        match yu_host_desktop::SoundSink::open() {
            Ok(sink) => engine.attach_audio(Box::new(sink)),
            Err(error) => {
                output::error(format!("[sound] 设备输出未启用: {error}"));
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
        },
    )
    .map_err(|error| Failure::runtime(error))
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
            _ => std::env::temp_dir().join("yu-cli-desktop.jsonl"),
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
        Err(error) => output::error(format!(
            "--mode desktop: 无法清理旧日志 {}: {error}",
            path.display()
        )),
    }
    std::env::set_var("YU_LOG_JSONL", &path);
    if std::env::var_os("YU_LOG").is_none() {
        std::env::set_var("YU_LOG", "warn");
    }
    let level = yu_log::facade::init(|line| output::error(line.to_string()));
    if level.is_none() {
        // 已有 logger(测试/进程内重入):保持现状,但至少把路径报出来。
        output::error("--mode desktop: log 门面已被占用,沿用现有 sink");
    }
    Ok(path)
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
