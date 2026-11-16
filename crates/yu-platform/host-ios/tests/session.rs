//! `yu-host-ios` 的宿主侧会话测试(在开发机上跑同一份代码)。
//!
//! 本 crate 的 C ABI 只是薄壳;真正的跨平台逻辑(启动 / tick / 帧 / 输入 /
//! 生命周期)来自 `yu-host-android` 的公开会话 API,`c_entry` 只做指针/字符串
//! 转换。因此这条链路在 Windows 上就能对真实语料跑通,不需要 Mac。
//!
//! 缺语料时显式跳过并打印原因(不空跑通过)。

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use yu_host_ios::{
    frame_rgba, frame_size, init_json, pending_dialogs_json, probe_root, resume, shutdown,
    status_text, suspend, tick, version_tag,
};

/// 会话是进程内单例,同一进程的多个测试必须串行。
static LOCK: Mutex<()> = Mutex::new(());

fn sample_root() -> Option<PathBuf> {
    let raw = std::env::var_os("YURIS_SAMPLE_ERIS_ROOT")?;
    let root = PathBuf::from(raw);
    (root.join("data.ypf").is_file() || root.join("data").is_dir()).then_some(root)
}

fn scratch(tag: &str) -> PathBuf {
    let base = std::env::var_os("YURIS_TEST_TMP")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = base.join(format!("yu-host-ios-{tag}-{}-{unique}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap_or_else(|error| {
        panic!(
            "创建测试目录失败 {}: {error}(可设 YURIS_TEST_TMP 指向可写目录)",
            dir.display()
        )
    });
    dir
}

fn json_escape(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

#[test]
fn soft_session_runs_and_copies_frame() {
    let Some(root) = sample_root() else {
        eprintln!("跳过: 未设置 YURIS_SAMPLE_ERIS_ROOT,或该目录下没有 data.ypf / data/");
        return;
    };
    let _guard = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

    let work = scratch("session");
    let save = work.join("save");
    std::fs::create_dir_all(&save).expect("创建存档目录");
    let log = work.join("yu-kernel.log");

    let json = format!(
        r#"{{"data_root":"{}","save_root":"{}","backend":"soft","audio":false,"quiet":true,"log_level":"info","log_file":"{}","fps":60}}"#,
        json_escape(&root),
        json_escape(&save),
        json_escape(&log),
    );
    let summary = init_json(&json).expect("soft 会话必须启动");
    eprintln!("init: {summary}");
    assert!(
        log.is_file(),
        "log_file 必须落盘(iOS 宿主在非 Android 目标上也装日志器): {}",
        log.display()
    );

    let mut frame = None;
    for _ in 0..30 {
        let code = tick();
        assert!(code >= 0, "tick 返回 {code}: {}", status_text());
        let (width, height) = frame_size();
        if width > 0 && height > 0 {
            frame = Some((width, height));
            break;
        }
        if code == 1 {
            break;
        }
    }
    let (width, height) = frame.expect("30 帧内必须出至少一帧");

    let rgba = frame_rgba();
    assert_eq!(
        rgba.len(),
        width as usize * height as usize * 4,
        "RGBA 长度必须 = w*h*4"
    );
    assert!(rgba.iter().any(|&byte| byte != 0), "首帧不能全零");

    assert!(!status_text().is_empty(), "status 不能为空");
    let _ = pending_dialogs_json();
    assert_eq!(suspend(), 0, "suspend 必须登记成功");
    assert_eq!(resume(), 0, "resume 必须登记成功");
    assert!(shutdown(), "会话应可销毁");

    eprintln!("frame: {width}x{height}, rgba={} bytes", rgba.len());
}

#[test]
fn probe_root_rejects_missing_path() {
    let json = probe_root("Z:/yu-no-such-root-xyz");
    assert!(json.contains("\"ok\":false"), "{json}");
}

#[test]
fn version_tag_mentions_ios_host() {
    assert!(version_tag().contains("yu-host-ios"));
}

/// C ABI 符号必须真的从 rlib 导出,否则 Xcode 链接时才炸。
#[test]
fn c_abi_symbols_are_exported() {
    extern "C" {
        fn yu_ios_abi_version() -> u32;
        fn yu_ios_version() -> *const std::ffi::c_char;
        fn yu_ios_copy_frame(out: *mut u8, cap: usize) -> i64;
        fn yu_ios_shutdown() -> i32;
    }

    // SAFETY: 三个函数都是本 crate 导出的 C ABI;version 返回静态存储。
    unsafe {
        assert_eq!(yu_ios_abi_version(), 1);
        let version = yu_ios_version();
        assert!(!version.is_null());
        let text = std::ffi::CStr::from_ptr(version)
            .to_str()
            .expect("版本串必须是 UTF-8");
        assert!(text.contains("yu-host-ios"), "{text}");

        // 未初始化时没有帧:-1,而不是 0 或崩溃。
        let mut buffer = [0u8; 16];
        assert_eq!(yu_ios_copy_frame(buffer.as_mut_ptr(), buffer.len()), -1);

        // 没有会话时 shutdown 必须按头文件契约返回 -1(不是 0/1)。
        assert_eq!(yu_ios_shutdown(), -1);
    }
}
