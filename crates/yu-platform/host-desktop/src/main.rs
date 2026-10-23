//! `yu-desktop` · 原生桌面窗口入口。
//!
//! ```text
//! yu-desktop <project-root> [--entry SYSTEM_START] [--data <path>]
//!            [--backend wgpu|soft] [--title <text>]
//!            [--frames N] [--shot <png>] [--width N] [--height N]
//!            [--fps N] [--fullscreen] [--pixel] [--debug-ui]
//!            [--no-sound] [--reconcile]
//! ```
//!
//! 默认 `--backend wgpu`;GPU 初始化失败自动回退 soft。`--frames` 用于
//! 无头验收,`--shot` 把最后一帧写成 PNG(与 `yu-cli run --shot` 对比)。

use std::path::{Path, PathBuf};

use yu_cli::args::ArgReader;
use yu_cli::engine::{Backend, Engine, EngineOptions};
use yu_cli::project::Project;
use yu_host_desktop::HostOptions;

macro_rules! host_error {
    ($($arg:tt)*) => { yu_cli::output::error(format!($($arg)*)) };
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
    args.finish().map_err(|error| error.to_string())?;

    let project = Project::discover(Path::new(&root), data.as_deref())
        .map_err(|error| error.to_string())?;
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
        },
    )
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
     \x20      [--fps N] [--fullscreen] [--pixel] [--debug-ui] [--no-sound] [--reconcile]"
        .to_string()
}
