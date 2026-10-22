//! `yu-cli` · 原生引擎宿主 + 本地服务 + 用例执行器。
//!
//! 架构(B,2026-10-21 主人决定):**读取与执行在 Rust**,浏览器只做呈现。
//! 本 crate 不做渲染语义:所有图层/混合/命令语义都在 `yu-rt`/`yu-gfx`/后端。
//! 这里是宿主层:数据入口、时钟、输入、帧循环、HTTP/WebSocket、HMR 编排。

#![forbid(unsafe_code)]
#![allow(clippy::result_large_err)]

pub mod args;
pub mod bench;
pub mod build_info;
pub mod case;
pub mod dev;
pub mod engine;
pub mod json;
pub mod output;
pub mod project;
pub mod run;
pub mod server;
pub mod sha;
pub mod ws;
pub mod ws_client;

/// 退出码:0 正常;2 运行/对账失败;3 缺样本或数据入口不存在。
pub const EXIT_OK: i32 = 0;
pub const EXIT_FAIL: i32 = 2;
pub const EXIT_MISSING: i32 = 3;

/// 统一失败类型(终端边界;内部仍用 `Result<_, String>`/`Diag`)。
#[derive(Clone, Debug)]
pub struct Failure {
    pub exit: i32,
    pub message: String,
}

impl Failure {
    pub fn new(exit: i32, message: impl Into<String>) -> Self {
        Self {
            exit,
            message: message.into(),
        }
    }

    pub fn runtime(message: impl Into<String>) -> Self {
        Self::new(EXIT_FAIL, message)
    }

    pub fn missing(message: impl Into<String>) -> Self {
        Self::new(EXIT_MISSING, message)
    }

    pub fn usage(message: impl Into<String>) -> Self {
        Self::new(EXIT_FAIL, message)
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl From<String> for Failure {
    fn from(value: String) -> Self {
        Self::runtime(value)
    }
}

impl From<&str> for Failure {
    fn from(value: &str) -> Self {
        Self::runtime(value)
    }
}

pub type CliResult<T> = Result<T, Failure>;
