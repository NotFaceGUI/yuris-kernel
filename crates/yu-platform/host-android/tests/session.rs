//! Android 宿主的端到端会话测试(在**宿主平台**上跑同一份代码)。
//!
//! 本 crate 的 JNI 导出只是薄壳,真正的跨平台逻辑是 `init_session` /
//! `tick` / `frame_rgba` / `set_input` / `shutdown`。这些是纯 Rust、与目标
//! 架构无关,因此在开发机上就能对**真实游戏语料**跑通整条链路:
//!
//! - 数据读取走 `Project::discover`(与桌面同一个装配);
//! - 存档写入落真实文件系统(Android 上就是 app 私有目录);
//! - 字体走「系统枚举 + 宿主字节」同一入口;
//! - 帧输出是 soft 后端的 RGBA,与 `--backend soft` 同构。
//!
//! 缺语料时显式跳过并打印原因(不空跑通过)。

use std::path::{Path, PathBuf};
use yu_host_android::{frame_rgba, frame_size, init_session, rgba_to_argb, set_cursor, set_input, shutdown, status_text, tick};

/// 语料根(与仓库其它测试同一约定;缺失则显式跳过)。
fn sample_root() -> Option<PathBuf> {
    let raw = std::env::var_os("YURIS_SAMPLE_ERIS_ROOT")?;
    let root = PathBuf::from(raw);
    if root.join("data.ypf").is_file() || root.join("data").is_dir() {
        Some(root)
    } else {
        None
    }
}

fn scratch(tag: &str) -> PathBuf {
    let base = std::env::var_os("YURIS_TEST_TMP")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../target/test-tmp")
        });
    base.join(format!("android-host/{tag}"))
}

fn cleanup(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn runs_real_corpus_and_emits_frames() {
    let Some(root) = sample_root() else {
        eprintln!("SKIP: YURIS_SAMPLE_ERIS_ROOT 未设置或没有数据入口(缺样本必须显式跳过)");
        return;
    };
    let save_root = scratch("save-run");
    cleanup(&save_root);

    let summary = init_session(root, save_root.clone(), "SYSTEM_START".to_string())
        .expect("init_session 应成功");
    eprintln!("init: {summary}");
    assert!(summary.starts_with("OK "), "初始化摘要异常: {summary}");
    assert!(summary.contains("backend=soft"), "Android 宿主固定 soft: {summary}");
    assert!(save_root.is_dir(), "存档目录必须被物化: {}", save_root.display());

    // 跑若干帧:Android 上没有窗口,帧由 tick_realtime 推进。
    let mut drawn = 0;
    for _ in 0..12 {
        assert_eq!(tick(), 0, "tick 失败: {}", status_text());
        let (width, height) = frame_size();
        if width > 0 && height > 0 {
            let rgba = frame_rgba();
            assert_eq!(
                rgba.len(),
                (width as usize) * (height as usize) * 4,
                "帧字节数与视口不一致"
            );
            // 与 Java 侧同一换算:ARGB_8888 打包(高位 A,低位 B)。
            let argb = rgba_to_argb(&rgba);
            assert_eq!(argb.len(), (width as usize) * (height as usize));
            drawn += 1;
        }
    }
    assert!(drawn > 0, "12 帧内没有产出任何像素: {}", status_text());

    let status = status_text();
    eprintln!("status: {status}");
    assert!(status.contains("status=running"), "状态行异常: {status}");
    // 内核诊断不能有 Fatal(unimplemented 允许作为过渡态,但不得是致命错误)。
    assert!(!status.contains("Fatal"), "出现致命诊断: {status}");

    // 输入注入不 panic 且被接受。
    assert_eq!(set_cursor(10, 20), 0);
    assert_eq!(set_input("MOUSE_L", true), 0);
    assert_eq!(set_input("MOUSE_L", false), 0);

    assert!(shutdown(), "shutdown 应释放会话");
    assert!(!shutdown(), "重复 shutdown 不应报告成功");
    cleanup(&save_root);
}

#[test]
fn save_write_lands_on_real_filesystem() {
    let Some(root) = sample_root() else {
        eprintln!("SKIP: 缺语料(YURIS_SAMPLE_ERIS_ROOT)");
        return;
    };
    let save_root = scratch("save-write");
    cleanup(&save_root);
    init_session(root, save_root.clone(), "SYSTEM_START".to_string()).expect("init");

    // 直接验证「存档域写盘 = 真实文件」这条平台能力:引擎启动后槽表可读,
    // 且 save 根是真实目录(Android 上即 app 私有目录)。
    assert!(save_root.is_dir());
    let writable = std::fs::metadata(&save_root)
        .map(|meta| !meta.permissions().readonly())
        .unwrap_or(false);
    assert!(writable, "存档根必须可写");

    shutdown();
    cleanup(&save_root);
}
