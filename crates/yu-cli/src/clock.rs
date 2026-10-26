//! 宿主本地时钟注入(`@_TIME` 手册语义 = 系统本地时间;内核不读墙钟,A3)。
//!
//! 只在**真实运行宿主**(`run` / `serve` / `desktop`)启动时读一次系统本地
//! 时间,作为 `BootConfig::local_time` 的基准;用例/trace/golden 不经过本
//! 模块,VM 保持 1970 注入纪元的确定性(DIV-RT-009)。
//!
//! 读不到本地时区(平台不支持)⇒ `None`:回落确定性注入时钟,不 panic、
//! 不猜时区。

use yu_rt::HostLocalTime;

/// 读一次系统本地日期时间(宿主注入用)。
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

#[cfg(test)]
mod tests {
    #[test]
    fn local_now_reads_system_clock() {
        let Some(now) = super::local_now() else {
            #[cfg(windows)]
            panic!("Windows 必须能读系统本地时间(存档时间依赖)");
            #[cfg(not(windows))]
            return;
        };
        assert!((2000..=2100).contains(&now.year), "本地年份异常: {now:?}");
        assert!((1..=12).contains(&now.month), "本地月份异常: {now:?}");
        assert!((0..=23).contains(&now.hour), "本地小时异常: {now:?}");
    }
}
