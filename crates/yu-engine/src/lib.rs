//! `yu-engine` · 唯一引擎装配(2026-10-25 架构 v2,H1)。
//!
//! 从 `yu-cli` 上移的装配层:`Engine`/`EngineOptions`/`Project`/boot/tick/
//! restart。所有宿主(`yu-cli` headless、`--mode desktop`、未来的 wasm 与
//! Android)嵌的**是同一份装配**,平台差异只能经 [`engine::AudioOut`] 这类
//! 端口进入(当前唯一端口:混音 PCM 出口,桌面宿主用 cpal 实现)。
//!
//! 附带宿主公共件(原 `yu-cli` 同名模块,`yu-cli` 仍 re-export 保持兼容):
//! [`output`](终端产品输出)、[`json`](最小 JSON 写出器)、[`sha`](摘要)。
//!
//! 本 crate **不**含:平台设备(cpal/winit/web-sys)、CLI 参数解析、
//! HTTP/WebSocket —— 那些归宿主与壳。

#![forbid(unsafe_code)]
#![allow(clippy::result_large_err)]

pub mod engine;
pub mod json;
pub mod output;
pub mod project;
pub mod sha;

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
