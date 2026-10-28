//! `yu-cli` 入口:run / serve / dev / case / bench。
//!
//! help 三态:总览(`help`/`-h`/`--help`)、逐命令(`help <command>`)、
//! 就地(`yu-cli run --help`)。

use yu_cli::build_info;
use yu_cli::{output, CliResult, Failure, EXIT_FAIL};

fn main() {
    // 统一控制台(Q1):启动时刻戳先行,日志行才有 `[  +s]` 前缀。
    yu_log::console::init_process_start();
    // log 门面桥(P4 M-E):先于任何引擎/依赖初始化;级别见 YU_LOG。
    // `--log-level X` 是全局参数,先摘出来再交给子命令(子命令不认它);
    // `--color on|off|auto` 同为全局参数(auto 时按 NO_COLOR/CLICOLOR_FORCE/
    // stderr 终端检测解析)。
    let mut tokens: Vec<String> = std::env::args().skip(1).collect();
    match extract_globals(&mut tokens) {
        Ok(()) => {}
        Err(message) => {
            output::error(format!("[yu-cli] {message}"));
            std::process::exit(EXIT_FAIL);
        }
    }
    // `run --mode desktop` 由桌面模式自建 log 门面与共享 JSONL 写者:门面在
    // init 时接管桌面模式打开的文件(与内核 Diag 同一个 `JsonlFile`,
    // Q1 R4 单写者契约)。其余路径在此统一接管。
    if !desktop_run(&tokens) {
        yu_log::facade::init(std::sync::Arc::new(yu_log::console::facade_line_to_stderr));
    }
    let code = match dispatch(tokens) {
        Ok(()) => 0,
        Err(failure) => {
            output::error(format!("[yu-cli] {}", failure.message));
            failure.exit
        }
    };
    // 日志写者异步(桌面宿主已在 `finish` 里刷过;这里兜底门面 JSONL 与
    // 控制台缓冲,`process::exit` 不跑析构)。
    log::logger().flush();
    std::process::exit(code);
}

/// 从参数里摘出全局旗标(任意位置可用;子命令不认识它们):
/// `--log-level quiet|error|warn|info|debug|trace|verbose` → `YU_LOG`,
/// `--color on|off|auto` → 控制台颜色覆盖。
fn extract_globals(tokens: &mut Vec<String>) -> Result<(), String> {
    let mut log_level: Option<String> = None;
    let mut color: Option<bool> = None;
    let mut i = 0;
    while i < tokens.len() {
        let (name, value, consumed_next) = if tokens[i] == "--log-level" || tokens[i] == "--color" {
            let name = tokens[i].trim_start_matches('-').to_string();
            if i + 1 >= tokens.len() {
                return Err(format!("--{name} 需要一个值"));
            }
            (name, tokens[i + 1].clone(), true)
        } else if let Some(rest) = tokens[i].strip_prefix("--log-level=") {
            ("log-level".to_string(), rest.to_string(), false)
        } else if let Some(rest) = tokens[i].strip_prefix("--color=") {
            ("color".to_string(), rest.to_string(), false)
        } else {
            i += 1;
            continue;
        };
        tokens.drain(i..=i + if consumed_next { 1 } else { 0 });
        match name.as_str() {
            "log-level" => log_level = Some(value),
            _ => match value.trim().to_ascii_lowercase().as_str() {
                "on" | "always" => color = Some(true),
                "off" | "never" => color = Some(false),
                "auto" => color = None,
                other => {
                    return Err(format!(
                        "--color 只接受 on|off|auto,收到 `{other}`"
                    ))
                }
            },
        }
    }
    if let Some(level) = log_level {
        let level = level.trim().to_ascii_lowercase();
        match level.as_str() {
            // `verbose` = 最低档,额外显示 `Severity::Verbose`(幂等空操作
            // 等「正常但值得留痕」的事实);其余档位都不显示该级别。
            "quiet" | "error" | "warn" | "info" | "debug" | "trace" | "verbose" => {
                std::env::set_var("YU_LOG", level);
            }
            other => {
                return Err(format!(
                    "--log-level 只接受 quiet|error|warn|info|debug|trace|verbose,收到 `{other}`"
                ))
            }
        }
    }
    if let Some(on) = color {
        yu_log::console::set_color_override(on);
    }
    Ok(())
}

/// 命令行是否为 `run --mode desktop`(桌面模式自己接管 log 门面)。
fn desktop_run(tokens: &[String]) -> bool {
    if tokens.first().map(String::as_str) != Some("run") {
        return false;
    }
    let mut i = 1;
    while i < tokens.len() {
        if tokens[i] == "--mode" {
            return tokens.get(i + 1).map(|value| value == "desktop").unwrap_or(false);
        }
        if let Some(rest) = tokens[i].strip_prefix("--mode=") {
            return rest == "desktop";
        }
        i += 1;
    }
    false
}

fn dispatch(tokens: Vec<String>) -> CliResult<()> {
    let Some(command) = tokens.first().cloned() else {
        output::raw(OVERVIEW);
        return Ok(());
    };
    let rest = tokens[1..].to_vec();

    if matches!(command.as_str(), "help" | "-h" | "--help") {
        match rest.first().map(String::as_str) {
            None | Some("help") => output::raw(OVERVIEW),
            Some(cmd) => print_command_help(cmd)?,
        }
        return Ok(());
    }
    // 就地 help:已知命令后跟 `--help` 时只打印用法,不进入参数解析。
    if rest.iter().any(|token| token == "--help") && is_known_command(&command) {
        print_command_help(&command)?;
        return Ok(());
    }

    match command.as_str() {
        "run" => yu_cli::run::from_args(rest),
        "serve" => yu_cli::server::run(yu_cli::server::ServeOptions::from_args(rest)?),
        "dev" => yu_cli::dev::from_args(rest),
        "case" => yu_cli::case::from_args(rest),
        "bench" => yu_cli::bench::from_args(rest),
        "version" | "-V" | "--version" => {
            output::line(format!(
                "yu-cli 0.1.0 (marker {})",
                build_info::ENGINE_MARKER
            ));
            Ok(())
        }
        other => Err(Failure::new(
            EXIT_FAIL,
            format!(
                "未知子命令 `{other}`;可用: run | serve | dev | case | bench | help \
                 (逐命令细节: yu-cli help run)"
            ),
        )),
    }
}

fn is_known_command(command: &str) -> bool {
    matches!(command, "run" | "serve" | "dev" | "case" | "bench")
}

fn print_command_help(command: &str) -> CliResult<()> {
    let text = match command {
        "run" => RUN_HELP,
        "serve" => SERVE_HELP,
        "dev" => DEV_HELP,
        "case" => CASE_HELP,
        "bench" => BENCH_HELP,
        other => {
            return Err(Failure::new(
                EXIT_FAIL,
                format!("`{other}` 没有单独的 help;可用: run | serve | dev | case | bench"),
            ))
        }
    };
    output::raw(text);
    Ok(())
}

const OVERVIEW: &str = "\
yu-cli · 桌面唯一入口(headless + --mode desktop)+ 本地服务 + 用例执行器

用法:
  yu-cli <command> [args...]     # 逐命令细节: yu-cli help <command>
                                 # 或就地: yu-cli <command> --help
  yu-cli run <project-root> [--mode headless|desktop] [...]
  yu-cli serve <project-root> [--port 4173] [...]
  yu-cli dev <project-root>      # HMR 监督(改码重启,改 www 刷新)
  yu-cli case <project-root> --all | --case NAME [--update]
  yu-cli bench <project-root> [--frames N]

命令:
  run     跑工程。缺省 headless(soft 后端,跑完即退,出报告/PNG/JSON);
          --mode desktop 开原生窗口(wgpu 优先,默认 feature,不单独编译)
  serve   本地 HTTP/WS 服务:服务端读数据并执行内核,浏览器只做呈现(架构 B)
  dev     监督 yu-serve 子进程:改 crates/**/*.rs → 重建重启;改 www/** → 刷新
  case    golden 用例执行器
  bench   帧耗时基准
  help    本总览,或逐命令 help

全局参数:
  --log-level quiet|error|warn|info|debug|trace|verbose
                  覆盖 YU_LOG(默认 info:内核挂号/生命周期行默认可见;
                  quiet 压掉生命周期 Info;verbose 额外显示最低档
                  「正常但值得留痕」的事实,如幂等删除空操作);
                  也可 --log-level=X 形式。
  --color on|off|auto   控制台颜色(默认 auto:stderr 非终端时自动关并
                  在文件日志留原因;显式 on|off 优先于环境;
                  CLICOLOR_FORCE=1 强制 on,NO_COLOR 非空强制 off)。
                  也可 --color=X 形式。

数据入口(唯一约定):
  <project-root>/data.ypf 优先;否则 <project-root>/data/;
  <project-root> 也可以直接是 .ypf 文件。

声音:
  出声只在 run --mode desktop(cpal 设备输出,平台 AudioOut 端口);
  headless/serve/case/bench 一律静音;@_OS_SOUND 注入口径恒 true。

构建:
  默认 feature=desktop(带窗口,一个 exe 两种模式);
  纯 headless/CI:cargo build -p yu-cli --no-default-features。
";

const RUN_HELP: &str = "\
yu-cli run · 跑工程(缺省 headless;--mode desktop 开原生窗口)

用法:
  yu-cli run <project-root> [--mode headless|desktop] [参数...]

共用参数(两种模式):
  --entry LABEL        入口标签(缺省 SYSTEM_START)
  --data PATH          数据入口覆盖(缺省 data.ypf 优先,退 data/)
  --backend soft|wgpu  渲染后端(headless 缺省 soft;desktop 缺省 wgpu,
                       wgpu 失败自动回退 soft)
  --frames N           跑 N 帧停(desktop = 验收模式,到帧关窗)
  --shot PATH          最后一帧写 PNG
  --save-root DIR      存档根重定向(缺省工程根 save/)
  --debug-mode on|off  @_DEBUGMODE 注入(缺省 off;desktop 缺省时亦读
                       环境变量 YURIS_DEBUG_MODE=1,DIV-L-001)

headless 专有(缺省模式):
  --until-visible      首个可绘制帧即停
  --until-frame N      跑到引擎第 N 帧
  --json PATH          最后一帧 display JSON 写盘
  --record-events on|off  事件录制(缺省 off;对拍时 on)
  --font FAMILY        强制字体(诊断/换字体;忽略脚本的 NAME/NO/BNO,
                       family 查不到 = 硬失败;全角/半角与空格差异自动折叠)
  --log-file PATH      JSONL 日志(内核 Diag sub=diag + 门面行 sub=log;
                       每轮截断)
  --diag-summary on|off  末尾诊断汇总(缺省 on;off 时 stderr 不出诊断行)

desktop 专有(--mode desktop;需默认 feature 构建):
  --width N  --height N  --fps N   窗口尺寸与帧率(缺省 60fps)
  --fullscreen  --pixel            全屏 / 像素整数缩放
  --title TEXT                     窗口标题(缺省 YU-RIS · <工程名>)
  --debug-ui                       启动即开 egui 调试面板(F1 切换)
  --no-sound                       不出声(@_OS_SOUND 注入口径不变)
  --script case.json               定时输入剧本(自动点击等)
  --reconcile                      wgpu 首帧与 soft 计数对账
  --log-file PATH                  宿主 JSONL 日志(缺省 %TEMP%/yu-cli-desktop.jsonl)
  --frame-stats CSV                逐帧耗时 CSV
  --profile-frames 1,4,340         指定帧打印 VM 命令耗时 top-10
  --spike-log PATH                 长帧自动取证日志(阶段耗时 + 本帧命令构成)
  --spike-ms 20                    长帧阈值(ms,work 口径;默认 20)
  --pump-cap N                     每任务每泵命令上限(DIV-HOST-002,诊断用)
  --panic-selftest N               panic 防线自测

宿主注入(BootConfig,全部显式给值):
  screen_color=32   @_SCREEN_COLOR(浏览器/桌面 32bpp)
  dx_mode=0         @_DXMODE(无 DX 分支)
  os_sound=true     @_OS_SOUND(注入恒 true;是否真出声由平台端口决定)
  media_readonly=0  @_MEDIA_READONLY
  record_events=off 默认关闭(首帧 129 万命令;对拍时 --record-events on)

示例:
  yu-cli run D:/game --frames 600 --shot out.png
  yu-cli run D:/game --mode desktop --fullscreen
";

const SERVE_HELP: &str = "\
yu-cli serve · 本地服务:服务端读数据并执行内核,浏览器只做呈现(架构 B)

用法:
  yu-cli serve <project-root> [--backend soft|wgpu] [--port 4173] [--no-open]
               [--entry LABEL] [--fps 60] [--record-events on|off]
               [--debug-mode on|off] [--data PATH] [--www DIR] [--diag-ws]
               [--font FAMILY] [--log-file PATH] [--max-step-ms N]

说明:
  启动后打印 http://127.0.0.1:<port>/,默认打开浏览器。
  WebSocket /ws:帧下发 + 输入/控制;SSE /events:HMR reload。
  --diag-ws 额外开 /diag 通道(500ms 推诊断快照:等待任务/挂号表/域计数)。
  --log-file JSONL 日志(内核 Diag + 门面行;每轮截断;关闭时打汇总)。
  --max-step-ms 调试用:实时逻辑时钟单步上限(默认关 = 不截断,墙钟语义)。
  服务端读数据并执行内核;浏览器不下载整包。
";

const DEV_HELP: &str = "\
yu-cli dev · HMR 监督

用法:
  yu-cli dev <project-root> [--port 4173] [--no-open] [--backend soft|wgpu]
              [--debug-mode on|off]

说明:
  监督 yu-serve 子进程:改 crates/**/*.rs → 重建并重启;改 www/** → 页面刷新。
";

const CASE_HELP: &str = "\
yu-cli case · golden 用例执行器

用法:
  yu-cli case <project-root> --all | --case NAME [--update] [--debug-mode on|off]
";

const BENCH_HELP: &str = "\
yu-cli bench · 帧耗时基准

用法:
  yu-cli bench <project-root> [--frames N] [--debug-mode on|off]
";
