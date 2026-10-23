//! `yu-cli` 入口:run / serve / dev / case / bench。

use yu_cli::build_info;
use yu_cli::{output, CliResult, Failure, EXIT_FAIL};

fn main() {
    // log 门面桥(P4 M-E):先于任何引擎/依赖初始化;级别见 YU_LOG。
    yu_log::facade::init(|line| crate::output::error(line));
    let tokens: Vec<String> = std::env::args().skip(1).collect();
    let code = match dispatch(tokens) {
        Ok(()) => 0,
        Err(failure) => {
            output::error(format!("yu-cli: {}", failure.message));
            failure.exit
        }
    };
    std::process::exit(code);
}

fn dispatch(tokens: Vec<String>) -> CliResult<()> {
    let Some(command) = tokens.first().cloned() else {
        print_help();
        return Ok(());
    };
    let rest = tokens[1..].to_vec();
    match command.as_str() {
        "run" => yu_cli::run::from_args(rest),
        "serve" => yu_cli::server::run(yu_cli::server::ServeOptions::from_args(rest)?),
        "dev" => yu_cli::dev::from_args(rest),
        "case" => yu_cli::case::from_args(rest),
        "bench" => yu_cli::bench::from_args(rest),
        "help" | "-h" | "--help" => {
            print_help();
            Ok(())
        }
        "version" | "--version" => {
            output::line(format!(
                "yu-cli 0.1.0 (marker {})",
                build_info::ENGINE_MARKER
            ));
            Ok(())
        }
        other => Err(Failure::new(
            EXIT_FAIL,
            format!("未知子命令 `{other}`;可用: run | serve | dev | case | bench | help"),
        )),
    }
}

fn print_help() {
    output::raw(
        "\
yu-cli · 原生引擎宿主 + 本地服务 + wasm 呈现端

用法:
  yu-cli run  <project-root> [--backend soft|wgpu] [--entry LABEL] [--frames N]
                             [--until-visible] [--shot PATH] [--json PATH]
                             [--record-events on|off] [--data PATH]
  yu-cli serve <project-root> [--backend soft|wgpu] [--port 4173] [--no-open]
                             [--entry LABEL] [--fps 60] [--record-events on|off]
                             [--data PATH] [--www DIR] [--diag-ws]
  yu-cli dev  <project-root> [--port 4173] [--no-open] [--backend soft|wgpu]
  yu-cli case <project-root> --all | --case NAME [--update]
  yu-cli bench <project-root>

数据入口(唯一约定):
  <project-root>/data.ypf 优先;否则 <project-root>/data/;
  <project-root> 也可以直接是 .ypf 文件。

宿主注入(BootConfig,全部显式给值):
  screen_color=32   @_SCREEN_COLOR(浏览器/桌面 32bpp)
  dx_mode=0         @_DXMODE(无 DX 分支)
  os_sound=true     @_OS_SOUND(引擎认为音频可用;默认设备输出见下)
  media_readonly=0  @_MEDIA_READONLY(只读介质语义,不是内核只读数据 A2)
  record_events=off 默认关闭(首帧 129 万命令;对拍时 --record-events on)

声音:
  默认构建含 device 特性:run/serve 把 PCM 输出到本机默认设备。
  `--no-default-features` 构建为纯 headless(不依赖声卡/不输出)。
  www 浏览器侧不实现音频(当前不是真 wasm 目标)。

serve:
  启动后打印 http://127.0.0.1:<port>/,默认打开浏览器。
  WebSocket /ws:帧下发 + 输入/控制;SSE /events:HMR reload。
  --diag-ws 额外开 /diag 通道(500ms 推诊断快照:等待任务/挂号表/域计数)。
  服务端读数据并执行内核;**浏览器不下载整包**。

dev:
  监督 yu-engine 子进程:改 crates/**/*.rs → 重建并重启;改 www/** → 页面刷新。
",
    );
}
