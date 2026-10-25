//! `yu-cli` · 桌面唯一入口(headless + `--mode desktop`)+ 本地服务 + 用例执行器。
//!
//! 架构(2026-10-25 v2,H1/H3):引擎装配在 `yu-engine`(所有宿主同一份);
//! 桌面窗口是本 crate 的运行期模式,不再单独编译 `yu-desktop` exe。
//! 浏览器/Android 等编译目标为**预留**(各自宿主 + `yu-engine`),不经本 crate。
//!
//! 本 crate 不做渲染语义:所有图层/混合/命令语义都在 `yu-rt`/`yu-gfx`/后端。
//! 这里是壳:参数解析、数据入口、HTTP/WebSocket、HMR 编排、窗口模式分派。

#![forbid(unsafe_code)]
#![allow(clippy::result_large_err)]

pub mod args;
pub mod bench;
pub mod build_info;
pub mod case;
pub mod dev;
pub mod logging;
pub mod run;
pub mod server;
pub mod ws;
pub mod ws_client;

#[cfg(feature = "desktop")]
pub mod desktop;

// 引擎装配与宿主公共件已上移 `yu-engine`(H1);在此 re-export 保持
// `yu_cli::engine` / `yu_cli::project` / `yu_cli::{Failure, CliResult}` 等
// 既有路径兼容(用例/示例/外围脚本不改引用)。
pub use yu_engine::{
    engine, json, output, project, sha, CliResult, Failure, EXIT_FAIL, EXIT_MISSING, EXIT_OK,
};
