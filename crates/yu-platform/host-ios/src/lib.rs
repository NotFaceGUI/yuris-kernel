//! `yu-host-ios` · iOS 宿主骨架(C ABI 薄壳)。
//!
//! # 定位
//!
//! 与 Android / 桌面共用同一份 `yu-engine` 装配:本 crate 不解释游戏语义,
//! 只做「会话管理 + 平台出口」。会话核心复用 `yu-host-android` 的公开 Rust
//! API —— 该 crate 的 JNI 导出(`jni_entry`)只是薄壳,`init_session` /
//! `tick` / `frame_rgba` / `set_input` / `suspend` / `pending_dialogs_json`
//! 等与目标架构无关,开发机上的 `tests/session.rs` 就是直接跑这条链路。
//!
//! **这是临时桥**:等 `host-android` 把平台中立会话抽成 `yu-host-mobile`
//! 后,本 crate 改为依赖公共核心;C ABI 与 Swift 壳不受影响。
//!
//! # 现状(骨架)
//!
//! - **可用**:`soft` 后端完整会话(启动 / tick / RGBA 帧拉取 / 输入 /
//!   DIALOG / 挂起恢复 / 存档);C ABI 见 `include/yu_ios.h`。
//! - **未接**:Metal 零拷贝上屏。`attach_layer` 固定返回 `UNSUPPORTED`;
//!   接线需要先给 `host-android` 抽 `Presenter` 端口(现在的
//!   `attach_surface` 直接收 `present::NativePresenter`),iOS 侧再用
//!   `wgpu::SurfaceTargetUnsafe::CoreAnimationLayer` 建 surface
//!   (wgpu 30 `src/api/surface.rs:430`,Metal backend 默认开启)。
//!
//! # 线程模型
//!
//! 与 `host-android` 相同:会话状态在 `thread_local`,`Engine` 是
//! `Rc`/`RefCell`。除 `suspend` / `resume` / `pending_dialogs_json` /
//! `answer_dialog` / `shutdown` 登记外,所有调用必须在**同一条 OS 线程**
//! (推荐专用渲染线程;Swift 骨架先放主线程)。
//!
//! # 数据与存档
//!
//! iOS 是沙箱:游戏数据由用户经「文件」App / Finder 放进 app 的
//! `Documents/`(Info.plist 开 `UIFileSharingEnabled` +
//! `LSSupportsOpeningDocumentsInPlace`);存档放 `Library/Application
//! Support/...`,不跟游戏数据混在一起。

#![deny(unsafe_code)]
#![allow(clippy::result_large_err)]

use std::cell::RefCell;
use std::ffi::c_void;

pub mod present;

thread_local! {
    /// 最近一次失败原因(供 C ABI 的 `yu_ios_last_error` 报告)。
    /// 成功初始化时清空;tick 失败只记返回码,详细原因看 `status_text`。
    #[allow(clippy::missing_const_for_thread_local)]
    static LAST_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn set_error(message: impl Into<String>) {
    let message = message.into();
    log::warn!(target: "yu-ios", "{message}");
    LAST_ERROR.with(|slot| *slot.borrow_mut() = Some(message));
}

fn clear_error() {
    LAST_ERROR.with(|slot| *slot.borrow_mut() = None);
}

/// 最近一次失败原因;`None` = 没有记录。
#[must_use]
pub fn last_error() -> Option<String> {
    LAST_ERROR.with(|slot| slot.borrow().clone())
}

// ---------------------------------------------------------------- 会话 API
//
// 这一层是 C ABI 与宿主侧测试共用的安全 Rust 面。所有函数都直接转发到
// `yu-host-android` 的平台中立会话核心,不复制任何引擎语义。

/// 按 JSON 配置启动会话(与 Android `nativeInitJson` 同一套键)。
///
/// `data_root` / `save_root` 由 iOS 壳给 app 容器内的路径;`backend` 缺省
/// `soft`。返回启动摘要(含数据包数量、后端、日志路径等)。
pub fn init_json(json: &str) -> Result<String, String> {
    // `host-android` 的 `init_logcat` / `init_file_logging` 只在 Android
    // 目标生效;iOS 上先装同一个平台无关日志器(stderr → Xcode console,
    // 可选文件),否则引擎内部 `log::info!/warn!` 全部被丢弃。
    if let Err(error) = init_logging(json) {
        set_error(error.clone());
        return Err(error);
    }
    match yu_host_android::start_session_json(json) {
        Ok(summary) => {
            clear_error();
            log::info!(target: "yu-ios", "[boot] {summary}");
            Ok(summary)
        }
        Err(error) => {
            set_error(error.clone());
            Err(error)
        }
    }
}

/// 安装 `log` 门面并登记可选日志文件。
///
/// 复用 `yu-host-android::logging`(平台无关的 `log::Log` 实现;非 Android
/// 出口写 stderr)。`log_file` 的相对路径与 `start_engine` 同口径:相对
/// `data_root` 解析。
fn init_logging(json: &str) -> Result<(), String> {
    let config = yu_host_android::RunConfig::from_json(json)?;
    yu_host_android::logging::init(if config.quiet {
        log::Level::Warn
    } else {
        log::Level::Info
    });
    if let Some(path) = &config.log_file {
        let resolved = if path.is_absolute() {
            path.clone()
        } else {
            config.data_root.join(path)
        };
        yu_host_android::logging::set_file(&resolved)
            .map_err(|error| format!("创建日志文件 {} 失败: {error}", resolved.display()))?;
    }
    Ok(())
}

/// 推进一帧。返回码与 `host-android` 一致:
/// `0` 继续 / `1` 游戏结束 / `4` 宿主销毁请求 / `-1` 未初始化 / `-2` 失败。
pub fn tick() -> i32 {
    let code = yu_host_android::tick();
    if code < 0 {
        set_error(format!("tick 返回 {code}(详细原因看 status)"));
    }
    code
}

/// 当前帧尺寸 `(w, h)`;`(0, 0)` = 还没有帧。
#[must_use]
pub fn frame_size() -> (u32, u32) {
    yu_host_android::frame_size()
}

/// 当前帧 RGBA 副本;空 = 还没有帧。
#[must_use]
pub fn frame_rgba() -> Vec<u8> {
    yu_host_android::frame_rgba()
}

/// 注入按钮(`MOUSE_L` / `MOUSE_R` / `KEY_ENTER` …)。
pub fn set_input(button: &str, pressed: bool) -> i32 {
    yu_host_android::set_input(button, pressed)
}

/// 注入光标(游戏视口像素坐标)。
pub fn set_cursor(x: i64, y: i64) -> i32 {
    yu_host_android::set_cursor(x, y)
}

/// 当前待回答 DIALOG 的 JSON;**空串 = 没有**。
#[must_use]
pub fn pending_dialogs_json() -> String {
    yu_host_android::pending_dialogs_json()
}

/// 回答一条 DIALOG(可从任意线程调用)。
pub fn answer_dialog(id: u64, button: u8, text: Option<&str>) -> Result<(), String> {
    match yu_host_android::answer_dialog(id, button, text) {
        Ok(()) => Ok(()),
        Err(error) => {
            set_error(error.clone());
            Err(error)
        }
    }
}

/// 生命周期挂起(停时钟/停泵;可从任意线程调用)。
pub fn suspend() -> i32 {
    yu_host_android::suspend()
}

/// 生命周期恢复(可从任意线程调用)。
pub fn resume() -> i32 {
    yu_host_android::resume()
}

/// 退出细分:1 主脚本走完 / 2 脚本 `END` / 3 `END`+RESTART / 0 运行中。
pub fn lifecycle() -> i32 {
    yu_host_android::lifecycle_code()
}

/// 取走并清除关窗请求。
pub fn take_shutdown_request() -> i32 {
    yu_host_android::take_shutdown_request()
}

/// 登记视口尺寸变化(渲染线程下一帧执行)。
pub fn request_resize(width: u32, height: u32) -> i32 {
    yu_host_android::request_resize(width, height)
}

/// 释放会话;返回是否真的销毁了一个会话。
pub fn shutdown() -> bool {
    yu_host_android::shutdown()
}

/// 诊断摘要(状态、帧号、fps、对账数字、最近错误)。
#[must_use]
pub fn status_text() -> String {
    yu_host_android::status_text()
}

/// 探测一个路径是不是引擎认得的数据入口(`Project::discover` 同一判据)。
#[must_use]
pub fn probe_root(path: &str) -> String {
    yu_host_android::probe_game_root(path)
}

/// 系统字体族名单(每行一个)。
#[must_use]
pub fn list_fonts() -> String {
    yu_host_android::list_font_families().join("\n")
}

/// 构建目标标记。
#[must_use]
pub fn version_tag() -> String {
    format!(
        "yu-host-ios {} / {}",
        env!("CARGO_PKG_VERSION"),
        yu_host_android::version_tag()
    )
}

/// Metal 上屏的接线占位。
///
/// `layer` 是 Swift 侧 `CAMetalLayer` 的裸指针(`Unmanaged.passUnretained`),
/// 生命周期由壳保证。当前固定返回错误,原因与接线步骤见 [`present`]。
pub fn attach_layer(_layer: *mut c_void, _width: u32, _height: u32) -> Result<(), String> {
    Err(present::UNSUPPORTED.to_string())
}

// ---------------------------------------------------------------- C ABI
//
// 规则:
// - 字符串一律 UTF-8、调用方给缓冲;返回「所需字节数(含 NUL)」,
//   缓冲不足时不写。Swift 侧不需要 free。
// - 每个导出都过 `catch_unwind`: unwind 越过 `extern "C"` 是 UB。
// - 原始指针解引用只在本模块。

#[allow(unsafe_code)]
mod c_entry {
    use super::*;
    use std::ffi::{c_char, CStr};
    use std::panic::{catch_unwind, AssertUnwindSafe};

    /// panic 兜底返回码(与业务返回码区分)。
    const PANIC: i32 = -99;

    fn guard(f: impl FnOnce() -> i32) -> i32 {
        catch_unwind(AssertUnwindSafe(f)).unwrap_or(PANIC)
    }

    fn guard_i64(f: impl FnOnce() -> i64) -> i64 {
        catch_unwind(AssertUnwindSafe(f)).unwrap_or(-99)
    }

    /// 把 UTF-8 文本写进调用方缓冲;返回所需字节数(含结尾 NUL)。
    ///
    /// # Safety
    ///
    /// `out` 要么为空,要么指向至少 `cap` 字节的可写内存。
    unsafe fn copy_text(text: &str, out: *mut c_char, cap: usize) -> i32 {
        let bytes = text.as_bytes();
        let need = bytes.len().saturating_add(1);
        if !out.is_null() && cap >= need {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), out.cast::<u8>(), bytes.len());
            *out.add(bytes.len()) = 0;
        }
        i32::try_from(need).unwrap_or(i32::MAX)
    }

    /// 解析一个 C 字符串参数并立即使用;不把 `&str` 的生命周期带出调用。
    ///
    /// # Safety
    ///
    /// `ptr` 要么为空,要么指向以 NUL 结尾的可读字符串。
    unsafe fn with_str<R>(ptr: *const c_char, f: impl FnOnce(&str) -> R) -> Result<R, i32> {
        if ptr.is_null() {
            return Err(-1);
        }
        let text = CStr::from_ptr(ptr).to_str().map_err(|_| -2)?;
        Ok(f(text))
    }

    /// `int32_t yu_ios_init_json(const char *json)`
    #[no_mangle]
    pub extern "C" fn yu_ios_init_json(json: *const c_char) -> i32 {
        guard(|| match unsafe { with_str(json, super::init_json) } {
            Ok(Ok(_summary)) => 0,
            Ok(Err(error)) => {
                log::error!(target: "yu-ios", "[boot] {error}");
                -1
            }
            Err(code) => code,
        })
    }

    /// `int32_t yu_ios_tick(void)`
    #[no_mangle]
    pub extern "C" fn yu_ios_tick() -> i32 {
        guard(super::tick)
    }

    /// `void yu_ios_frame_size(uint32_t *out_w, uint32_t *out_h)`
    #[no_mangle]
    pub extern "C" fn yu_ios_frame_size(out_w: *mut u32, out_h: *mut u32) {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            let (width, height) = super::frame_size();
            // SAFETY: 调用方按头文件约定给可写指针或 NULL。
            unsafe {
                if !out_w.is_null() {
                    *out_w = width;
                }
                if !out_h.is_null() {
                    *out_h = height;
                }
            }
        }));
    }

    /// `int64_t yu_ios_copy_frame(uint8_t *out, size_t cap)`
    ///
    /// 返回写入字节数;`-1` = 还没有帧,`-2` = 缓冲不足/空指针。
    #[no_mangle]
    pub extern "C" fn yu_ios_copy_frame(out: *mut u8, cap: usize) -> i64 {
        guard_i64(|| {
            let frame = super::frame_rgba();
            if frame.is_empty() {
                return -1;
            }
            if out.is_null() || cap < frame.len() {
                return -2;
            }
            // SAFETY: 调用方保证 out 至少 cap 字节可写,且 cap >= frame.len()。
            unsafe {
                std::ptr::copy_nonoverlapping(frame.as_ptr(), out, frame.len());
            }
            frame.len() as i64
        })
    }

    /// `int32_t yu_ios_set_input(const char *button, int32_t pressed)`
    #[no_mangle]
    pub extern "C" fn yu_ios_set_input(button: *const c_char, pressed: i32) -> i32 {
        guard(
            || match unsafe { with_str(button, |name| super::set_input(name, pressed != 0)) } {
                Ok(code) => code,
                Err(code) => code,
            },
        )
    }

    /// `int32_t yu_ios_set_cursor(int64_t x, int64_t y)`
    #[no_mangle]
    pub extern "C" fn yu_ios_set_cursor(x: i64, y: i64) -> i32 {
        guard(|| super::set_cursor(x, y))
    }

    /// `int32_t yu_ios_suspend(void)`
    #[no_mangle]
    pub extern "C" fn yu_ios_suspend() -> i32 {
        guard(super::suspend)
    }

    /// `int32_t yu_ios_resume(void)`
    #[no_mangle]
    pub extern "C" fn yu_ios_resume() -> i32 {
        guard(super::resume)
    }

    /// `int32_t yu_ios_lifecycle(void)`
    #[no_mangle]
    pub extern "C" fn yu_ios_lifecycle() -> i32 {
        guard(super::lifecycle)
    }

    /// `int32_t yu_ios_take_shutdown_request(void)`
    #[no_mangle]
    pub extern "C" fn yu_ios_take_shutdown_request() -> i32 {
        guard(super::take_shutdown_request)
    }

    /// `int32_t yu_ios_request_resize(uint32_t width, uint32_t height)`
    #[no_mangle]
    pub extern "C" fn yu_ios_request_resize(width: u32, height: u32) -> i32 {
        guard(|| super::request_resize(width, height))
    }

    /// `int32_t yu_ios_shutdown(void)`
    #[no_mangle]
    pub extern "C" fn yu_ios_shutdown() -> i32 {
        guard(|| if super::shutdown() { 0 } else { -1 })
    }

    /// `int32_t yu_ios_pending_dialogs(char *out, size_t cap)`
    #[no_mangle]
    pub extern "C" fn yu_ios_pending_dialogs(out: *mut c_char, cap: usize) -> i32 {
        guard(|| {
            let json = super::pending_dialogs_json();
            // SAFETY: 见 copy_text。
            unsafe { copy_text(&json, out, cap) }
        })
    }

    /// `int32_t yu_ios_answer_dialog(uint64_t id, uint8_t button, const char *text)`
    ///
    /// `text` 为空表示非输入框 DIALOG;输入框必须给文本。
    #[no_mangle]
    pub extern "C" fn yu_ios_answer_dialog(id: u64, button: u8, text: *const c_char) -> i32 {
        guard(|| {
            let result = if text.is_null() {
                super::answer_dialog(id, button, None)
            } else {
                match unsafe {
                    with_str(text, |value| super::answer_dialog(id, button, Some(value)))
                } {
                    Ok(result) => result,
                    Err(code) => return code,
                }
            };
            match result {
                Ok(()) => 0,
                Err(error) => {
                    log::warn!(target: "yu-ios", "DIALOG#{id} 回答失败: {error}");
                    -1
                }
            }
        })
    }

    /// `int32_t yu_ios_status(char *out, size_t cap)`
    #[no_mangle]
    pub extern "C" fn yu_ios_status(out: *mut c_char, cap: usize) -> i32 {
        guard(|| unsafe { copy_text(&super::status_text(), out, cap) })
    }

    /// `int32_t yu_ios_list_fonts(char *out, size_t cap)`
    #[no_mangle]
    pub extern "C" fn yu_ios_list_fonts(out: *mut c_char, cap: usize) -> i32 {
        guard(|| unsafe { copy_text(&super::list_fonts(), out, cap) })
    }

    /// `int32_t yu_ios_probe_root(const char *path, char *out, size_t cap)`
    #[no_mangle]
    pub extern "C" fn yu_ios_probe_root(path: *const c_char, out: *mut c_char, cap: usize) -> i32 {
        guard(|| match unsafe { with_str(path, super::probe_root) } {
            Ok(json) => unsafe { copy_text(&json, out, cap) },
            Err(code) => code,
        })
    }

    /// `int32_t yu_ios_last_error(char *out, size_t cap)`
    #[no_mangle]
    pub extern "C" fn yu_ios_last_error(out: *mut c_char, cap: usize) -> i32 {
        guard(|| {
            let text = super::last_error().unwrap_or_default();
            unsafe { copy_text(&text, out, cap) }
        })
    }

    /// `int32_t yu_ios_attach_layer(void *layer, uint32_t width, uint32_t height)`
    ///
    /// 预留的 Metal 上屏入口。当前 `-3` = 未接线;`0` = 成功。
    #[no_mangle]
    pub extern "C" fn yu_ios_attach_layer(layer: *mut c_void, width: u32, height: u32) -> i32 {
        guard(|| match super::attach_layer(layer, width, height) {
            Ok(()) => 0,
            Err(_) => -3,
        })
    }

    /// `const char *yu_ios_version(void)`(静态存储,调用方不释放)
    const VERSION: &[u8] = concat!(
        "yu-host-ios ",
        env!("CARGO_PKG_VERSION"),
        " (soft; metal pending)\0"
    )
    .as_bytes();

    #[no_mangle]
    pub extern "C" fn yu_ios_version() -> *const c_char {
        VERSION.as_ptr().cast()
    }

    /// `uint32_t yu_ios_abi_version(void)` — C ABI 版本号,壳启动时可断言。
    #[no_mangle]
    pub extern "C" fn yu_ios_abi_version() -> u32 {
        1
    }
}
