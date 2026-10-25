//! `yu-serve`:只跑 `serve` 的引擎子进程(`yu-cli dev` 拉起)。
//!
//! 独立 bin 是为了让 HMR 监督进程能反复重建并替换它(Windows 上运行中的
//! exe 不可覆盖)。产品入口仍然是 `yu-cli serve`。

use yu_cli::output;
use yu_cli::server::{self, ServeOptions};
use yu_cli::{CliResult, Failure, EXIT_FAIL};

fn main() {
    // log 门面桥(P4 M-E):HMR 子进程与主进程同口径。
    yu_log::facade::init(|line| crate::output::error(line));
    let tokens: Vec<String> = std::env::args().skip(1).collect();
    let code = match run(tokens) {
        Ok(()) => 0,
        Err(failure) => {
            output::error(format!("yu-serve: {}", failure.message));
            failure.exit
        }
    };
    std::process::exit(code);
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
