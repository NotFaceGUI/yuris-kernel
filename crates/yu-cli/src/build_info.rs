//! 构建标记。
//!
//! HMR 冒烟测试会改写 `ENGINE_MARKER` 的字符串并断言子进程重建后
//! `/api/status.marker` 变化。**不要**把这个常量内联到别处,否则测试的
//! 可观测点会消失。

/// 引擎子进程的可观测构建标记。
pub const ENGINE_MARKER: &str = "k2-engine-0";
