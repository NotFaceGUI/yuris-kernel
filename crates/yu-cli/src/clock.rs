//! 宿主本地时钟注入(`@_TIME` 手册语义 = 系统本地时间;内核不读墙钟,A3)。
//!
//! 实现已上移到 [`yu_engine::host_clock`]:每个宿主都需要这一步,而
//! 「每个宿主各自接一次线」正是 Android 漏接、`@_TIME` 恒 1970 的来源。
//! 本模块只为 `yu-cli` 内部调用点保留原名,不再持有独立实现。
//!
//! 只在**真实运行宿主**(`run` / `serve` / `desktop`)启动时读一次系统本地
//! 时间,作为 `BootConfig::local_time` 的基准;用例/trace/golden 不经过本
//! 模块,VM 保持 1970 注入纪元的确定性(DIV-RT-009)。

use yu_rt::HostLocalTime;

/// 读一次系统本地日期时间(宿主注入用)。
pub fn local_now() -> Option<HostLocalTime> {
    yu_engine::host_clock::local_now()
}
