//! Q1 日志收口的 CLI 侧装配:统一 Console 构造、JSONL 打开与门槛解析。
//!
//! 三种模式共用同一条格式化通道(`yu_log::console`):
//! - desktop:`stderr_live`(即印 + 面板环形缓冲 + JSONL);
//! - headless run:`stderr_aggregate`(末尾汇总;`--diag-summary off` 关);
//! - serve:`stderr_aggregate`(关闭时汇总 + 排空计数)。
//!
//! JSONL 单写者契约(Q1 R4):一个 `JsonlFile`(一个文件句柄)同时接
//! 门面行(`sub:"log"`)与内核 Diag 行(`sub:"diag"`);每轮打开前截断。

use std::path::Path;

use yu_log::console::{color_auto_off_reason, Console, JsonlFile, MinLevel};
use yu_log::Diag;
use crate::{CliResult, Failure};

/// `YU_LOG`/`--log-level` 的内核诊断门槛(quiet/error/warn 压 Info)。
pub fn min_level() -> MinLevel {
    MinLevel::from_level_value(std::env::var("YU_LOG").ok().as_deref())
}

/// 打开「每轮截断」的 JSONL(一次运行 = 一个现场):建父目录、清旧文件、
/// 创建新文件。headless/serve/desktop 同口径(Q1 R4)。
pub fn open_jsonl_truncating(path: &Path) -> Result<JsonlFile, String> {
    // 消息里的路径一律 Debug 格式:空白/NUL/非法字符要现形,不许显示成
    // 「空路径」误导排查(Q1 复审:本机 TEMP 空值经 Git Bash 变 NUL 路径
    // 时,display() 让现场不可读)。
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("创建日志目录 {:?} 失败: {e}", parent))?;
        }
    }
    // 每次启动截断:一次运行 = 一个现场,避免上一轮 panic 混进本轮证据。
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("无法清理旧日志 {:?}: {error}", path)),
    }
    JsonlFile::create(path).map_err(|e| format!("创建日志文件 {:?} 失败: {e}", path))
}

/// auto 关闭时的原因留痕(tyn 的静默无色教训):写入 JSONL 一行
/// `sub:"log"`,提示 `--color on` 可强制。
pub fn write_color_notice(file: &JsonlFile) {
    if let Some(reason) = color_auto_off_reason() {
        file.write_log_line(
            "info",
            "yu-cli",
            &format!("color auto→off: {reason}"),
        );
    }
}

/// 把 gfx 帧诊断累计行转成统一格式的 `Diag` 喂给 Console(汇总/面板同源;
/// 转换本体在 [`yu_engine::engine::frame_total_to_diag`],与桌面宿主共用)。
pub fn frame_total_to_diag(frame_diag: &yu_gfx::FrameDiag) -> Diag {
    yu_engine::engine::frame_total_to_diag(frame_diag)
}

/// headless/serve 末尾诊断汇总:三路引擎诊断 + gfx 帧诊断累计全部喂进
/// Console 聚合,频次降序打到 stderr。`--diag-summary off` 时不调用。
pub fn print_diag_summary(console: &mut Console, engine: &mut yu_engine::engine::Engine) {
    for diag in engine.take_diags() {
        console.emit_diag(&diag, 1);
    }
    for diag in engine.take_save_diags() {
        console.emit_diag(&diag, 1);
    }
    for diag in engine.take_sound_diags() {
        console.emit_diag(&diag, 1);
    }
    for (frame_diag, total) in engine.take_frame_diag_totals() {
        console.emit_diag(&frame_total_to_diag(&frame_diag), total);
    }
    console.print_summary();
}

/// `--flag on|off` 解析(缺省值由调用方给)。
pub fn parse_on_off(name: &str, value: &str) -> CliResult<bool> {
    match value.to_ascii_lowercase().as_str() {
        "on" | "true" | "1" => Ok(true),
        "off" | "false" | "0" => Ok(false),
        other => Err(Failure::usage(format!(
            "--{name} 只接受 on|off,收到 `{other}`"
        ))),
    }
}
