//! 测试支撑:复用 `yu-rt` 的合成工程夹具(只读引用,不复制),并加本 crate
//! 需要的确定性头部/临时目录助手。
//!
//! 每个集成测试单独编译本模块,未用到的再导出项不应报死代码。

#![allow(dead_code, unused_imports)]

#[allow(clippy::all)]
#[path = "../../../yu-rt/tests/support/mod.rs"]
mod raw;

pub use raw::*;

pub use yu_trace::{BootSummary, HeaderPayload, InputSummary};

/// 与真实录制一致形状的确定性头部(不读环境、不含墙钟/绝对路径)。
pub fn header_for(tables: Vec<String>) -> HeaderPayload {
    HeaderPayload {
        codec: "cp932".to_string(),
        tables,
        boot: BootSummary::from_config(&yu_rt::BootConfig::default()),
        input: InputSummary {
            events: 0,
            sha256: "0".repeat(64),
        },
        kernel: yu_trace::kernel_version().to_string(),
    }
}

/// 每次调用返回一个全新的临时目录(`<target>/tmp/yu-trace/<name>`),已清空。
///
/// 用 `CARGO_TARGET_TMPDIR`(cargo 为集成测试注入):它一定在仓库 target 下,
/// 不受系统 TEMP 只读限制,也不会把绝对路径写进任何录制内容。
pub fn fresh_dir(name: &str) -> std::path::PathBuf {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("yu-trace")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}
