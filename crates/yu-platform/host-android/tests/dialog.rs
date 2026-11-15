//! Android 宿主的 DIALOG 端到端证据(真实 ERIS 语料)。
//!
//! 与 `yu-cli/tests/dialog.rs` 同一条脚本链,但走 **host-android 的公开
//! API**:`init_session` → `request_window_close()` → `pending_dialogs_json()`
//! → `answer_dialog()`。它验证三件宿主侧的事:
//!
//! 1. 快照在 `tick()` 后能看到内核新挂起的 DIALOG(JSON 契约);
//! 2. 回答能清空快照并让挂起任务继续;
//! 3. 全程不碰 JNI/Java,证明「内核 + 宿主接口」本身跨平台可用。
//!
//! 需要真实语料 `YURIS_SAMPLE_ERIS_ROOT` + `<root>/save/gsd.sd`
//! (終了確認あり 的门)。缺样本显式跳过,不空跑通过。
//!
//! ```text
//! cargo test -p yu-host-android --test dialog -- --ignored --nocapture
//! ```

use std::path::{Path, PathBuf};

use yu_engine::json;
use yu_host_android::{
    answer_dialog, init_session, pending_dialogs_json, request_window_close, shutdown,
    status_text, tick,
};

fn sample_root() -> Option<PathBuf> {
    let root = PathBuf::from(std::env::var_os("YURIS_SAMPLE_ERIS_ROOT")?);
    if root.join("data.ypf").is_file() || root.join("data").is_dir() {
        Some(root)
    } else {
        eprintln!(
            "SKIP: YURIS_SAMPLE_ERIS_ROOT={} 没有 data.ypf / data/",
            root.display()
        );
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

/// 预拷语料 `save/gsd.sd`(終了確認あり 的门);读写全落临时目录。
fn temp_save_root(root: &Path) -> Option<PathBuf> {
    let source = root.join("save/gsd.sd");
    if !source.is_file() {
        eprintln!("SKIP: 语料 {} 不存在(確認あり 前提不成立)", source.display());
        return None;
    }
    let dir = scratch("dialog-save");
    cleanup(&dir);
    std::fs::create_dir_all(&dir).expect("create temp save root");
    std::fs::copy(&source, dir.join("gsd.sd")).expect("copy gsd.sd");
    Some(dir)
}

#[test]
#[ignore = "需要真实语料;显式 --ignored 运行"]
fn window_close_dialog_is_presented_and_answered() {
    let Some(root) = sample_root() else {
        return;
    };
    let Some(save_root) = temp_save_root(&root) else {
        return;
    };
    let summary = init_session(root, save_root.clone(), "SYSTEM_START".to_string())
        .expect("init_session 应成功");
    eprintln!("init: {summary}");

    // 启动链稳定(关闭监视循环在跑)。
    for _ in 0..20 {
        assert_eq!(tick(), 0, "启动帧 tick 失败: {}", status_text());
    }

    // 第一次关闭:es_Scenario → ES.ENDCONF.DIALOG → 原生 DIALOG 挂起。
    assert_eq!(request_window_close(), 0, "有会话时应登记关闭请求");
    let mut json = String::new();
    for _ in 0..30 {
        assert_eq!(tick(), 0, "关闭后 tick 失败: {}", status_text());
        json = pending_dialogs_json();
        if !json.is_empty() {
            break;
        }
    }
    assert!(
        !json.is_empty(),
        "关闭请求应挂起原生 DIALOG(快照为空): {}",
        status_text()
    );
    eprintln!("dialogs: {json}");

    let value = json::parse(&json).expect("宿主快照必须是合法 JSON");
    assert_eq!(value.get("protocol").and_then(json::Value::as_u64), Some(1));
    let dialogs = value
        .get("dialogs")
        .and_then(json::Value::as_array)
        .expect("dialogs 数组");
    assert_eq!(dialogs.len(), 1, "关闭请求应只挂起一条 DIALOG");
    let dialog = &dialogs[0];
    let id = dialog
        .get("id")
        .and_then(json::Value::as_u64)
        .expect("DIALOG id");
    let roles: Vec<&str> = dialog
        .get("buttons")
        .and_then(json::Value::as_array)
        .expect("buttons 数组")
        .iter()
        .filter_map(json::Value::as_str)
        .collect();
    assert_eq!(roles, vec!["yes", "no"], "終了確認 = はい/いいえ");
    let caption = dialog
        .get("caption")
        .and_then(json::Value::as_str)
        .unwrap_or("");
    assert!(caption.contains("確認"), "CAPTION 应为「確認」: {caption:?}");

    // いいえ(2):任务继续,快照清空。
    answer_dialog(id, 2, None).expect("回答 いいえ");
    assert!(
        pending_dialogs_json().is_empty(),
        "回答后快照应立即移除该对话框"
    );
    for _ in 0..5 {
        assert_eq!(tick(), 0, "回答后 tick 失败: {}", status_text());
    }
    assert!(
        pending_dialogs_json().is_empty(),
        "回答后不应再次挂起: {}",
        status_text()
    );
    let status = status_text();
    assert!(
        !status.contains("status=exited"),
        "いいえ 不应结束主线: {status}"
    );
    eprintln!("いいえ 后: {status}");

    assert!(shutdown(), "测试收尾应释放会话");
    cleanup(&save_root);
}
