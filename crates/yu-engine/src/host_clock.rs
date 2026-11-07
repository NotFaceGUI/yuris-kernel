//! 宿主本地时间注入(`@_TIME` 手册语义 = 系统本地时间;内核不读墙钟,A3)。
//!
//! 所有**真实运行宿主**在启动时读一次系统本地时间,作为
//! [`EngineOptions::local_time`](crate::engine::EngineOptions::local_time) 的
//! 基准:桌面 `--mode desktop`、headless `run`、`serve`、Android。用例/
//! trace/golden 不注入,VM 保持 1970 注入纪元的确定性(DIV-RT-009)。
//!
//! # 为什么必须放在装配层
//!
//! 本函数原先只存在于 `yu-cli`(`crate::clock`),于是**每个宿主都得记得
//! 自己调一次**。Android 宿主就漏了这一步:`@_TIME` 恒返回 1970,存档菜单
//! 的日期永远显示 1970-01-01 —— 与桌面出现脚本可见的行为分歧,而且不报错。
//! 这类"每个宿主各自接线"的参数现在只有一份实现,漏接一次不再可能。
//!
//! # 失败语义
//!
//! 读不到本地时区(平台不提供 / 沙箱屏蔽)⇒ `None`:回落到确定性注入时钟,
//! 不 panic、不猜时区。浏览器目标暂时恒 `None`(见下)。

use yu_rt::HostLocalTime;

/// 读一次系统本地日期时间(真实运行宿主启动时调用)。
///
/// 非 wasm:经 `time` 的 `local-offset`(Unix 走 `localtime_r`,Windows 走
/// `GetTimeZoneInformation`)。读不到时区就是 `None`。
///
/// wasm:浏览器没有进程级本地时区 API(要 `Intl.DateTimeFormat` 经 JS 反查),
/// 当前恒 `None`;真 wasm 宿主接入时从这里补,不要在各宿主各写一份。
#[cfg(not(target_arch = "wasm32"))]
pub fn local_now() -> Option<HostLocalTime> {
    let now = time::OffsetDateTime::now_local().ok()?;
    HostLocalTime::new(
        now.year() as i64,
        u8::from(now.month()) as i64,
        now.day() as i64,
        now.hour() as i64,
        now.minute() as i64,
        now.second() as i64,
        now.millisecond() as i64,
    )
}

/// 浏览器目标:见模块文档(恒 `None` = 确定性注入时钟)。
#[cfg(target_arch = "wasm32")]
pub fn local_now() -> Option<HostLocalTime> {
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn local_now_reads_system_clock() {
        let Some(now) = super::local_now() else {
            // wasm/受限环境拿不到时区是合法结果;桌面必须能读(存档时间依赖)。
            #[cfg(all(windows, not(target_arch = "wasm32")))]
            panic!("Windows 必须能读系统本地时间(存档时间依赖)");
            #[cfg(not(all(windows, not(target_arch = "wasm32"))))]
            return;
        };
        assert!((2000..=2100).contains(&now.year), "本地年份异常: {now:?}");
        assert!((1..=12).contains(&now.month), "本地月份异常: {now:?}");
        assert!((0..=23).contains(&now.hour), "本地小时异常: {now:?}");
    }
}
