//! `yu-serve`:只跑 `serve` 的引擎子进程(`yu-cli dev` 拉起)。
//!
//! 独立 bin 是为了让 HMR 监督进程能反复重建并替换它(Windows 上运行中的
//! exe 不可覆盖)。产品入口仍然是 `yu-cli serve`。

use yu_cli::output;
use yu_cli::server::{self, ServeOptions};
use yu_cli::{CliResult, Failure, EXIT_FAIL};

fn main() {
    // 统一控制台(Q1):启动时刻戳先行,日志行才有 `[  +s]` 前缀。
    yu_log::console::init_process_start();
    // `--log-level` / `--color` 与主进程同口径(dev 转发或手工调用时可用;
    // YU_LOG 环境变量本来就会被子进程继承)。
    let mut tokens: Vec<String> = std::env::args().skip(1).collect();
    if let Err(message) = extract_globals(&mut tokens) {
        output::error(format!("[yu-serve] {message}"));
        std::process::exit(EXIT_FAIL);
    }
    // log 门面桥(P4 M-E):HMR 子进程与主进程同口径(统一格式/颜色)。
    yu_log::facade::init(std::sync::Arc::new(yu_log::console::facade_line_to_stderr));
    let code = match run(tokens) {
        Ok(()) => 0,
        Err(failure) => {
            output::error(format!("[yu-serve] {}", failure.message));
            failure.exit
        }
    };
    std::process::exit(code);
}

/// 摘出 `--log-level quiet|error|warn|info|debug|trace` 与
/// `--color on|off|auto`(与 `yu-cli` main 的 `extract_globals` 同语义;
/// 独立 bin 无法复用其 main,此处保持两份显式同步)。
fn extract_globals(tokens: &mut Vec<String>) -> Result<(), String> {
    let mut i = 0;
    while i < tokens.len() {
        let (name, value) = if tokens[i] == "--log-level" || tokens[i] == "--color" {
            if i + 1 >= tokens.len() {
                return Err(format!("{} 需要一个值", tokens[i]));
            }
            let name = tokens[i].trim_start_matches('-').to_string();
            let value = tokens[i + 1].clone();
            tokens.drain(i..=i + 1);
            (name, value)
        } else if tokens[i].starts_with("--log-level=") {
            let rest = tokens[i]["--log-level=".len()..].to_string();
            tokens.remove(i);
            ("log-level".to_string(), rest)
        } else if tokens[i].starts_with("--color=") {
            let rest = tokens[i]["--color=".len()..].to_string();
            tokens.remove(i);
            ("color".to_string(), rest)
        } else {
            i += 1;
            continue;
        };
        match name.as_str() {
            "log-level" => match value.trim().to_ascii_lowercase().as_str() {
                "quiet" | "error" | "warn" | "info" | "debug" | "trace" => {
                    std::env::set_var("YU_LOG", value.trim().to_ascii_lowercase());
                }
                other => {
                    return Err(format!(
                        "--log-level 只接受 quiet|error|warn|info|debug|trace,收到 `{other}`"
                    ))
                }
            },
            _ => match value.trim().to_ascii_lowercase().as_str() {
                "on" | "always" => yu_log::console::set_color_override(true),
                "off" | "never" => yu_log::console::set_color_override(false),
                "auto" => {}
                other => return Err(format!("--color 只接受 on|off|auto,收到 `{other}`")),
            },
        }
    }
    Ok(())
}

fn run(tokens: Vec<String>) -> CliResult<()> {
    match tokens.first().map(String::as_str) {
        Some("serve") => server::run(ServeOptions::from_args(tokens[1..].to_vec())?),
        _ => Err(Failure::new(
            EXIT_FAIL,
            "yu-serve 只接受 `serve <project-root> [...]`(由 yu-cli dev 内部使用)",
        )),
    }
}
