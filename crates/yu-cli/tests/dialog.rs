//! S5 销账证据(B2):窗口关闭的原生 DIALOG 在 Engine 层呈现并可回答。
//!
//! 语料事实(ERIS,save/data/script):
//! - 菜单「終了する」按钮 = **脚本自绘**确认框(`ES_ENDCONF.yst` 的
//!   `ES.ENDCONF.LOOP`,はい/いいえ 是精灵按钮;P1 S3b 同族,无需 DIALOG);
//! - **窗口关闭**(WM_CLOSE → `@_WINDOW_EXEC_CLOSE=1`)→ `es_Scenario.yst:481/
//!   1461` → `GOSUB[#=ES.ENDCONF.DIALOG]` → `DIALOG[… YESNO=1 LET=@Chk]`
//!   (门:`@es.GSD(114,01)==1` 終了確認あり;`ES_ENDCONF.yst:114`)。
//!   P1 台账 S5 观测的静默挂起就是这一条:VM 挂起在原生 DIALOG 上,
//!   Engine 未透出 ⇒ 宿主无法呈现/回答。
//!
//! 本测试在 Engine 层(与桌面宿主同装配)驱动关闭→呈现→回答全链:
//! `request_window_close()` → `pending_dialogs()` → `answer_dialog()`。
//! save_root 用 `target/tmp` 的**临时副本**(预拷语料 `gsd.sd` 以满足
//! 確認あり 门;绝不写工程真实存档)。
//!
//! 需要:真实语料 `YURIS_SAMPLE_ERIS_ROOT` + `<root>/save/gsd.sd`。
//! `cargo test -p yu-cli --test dialog -- --ignored --nocapture`
//!
//! 三层:①手册 `other/dialog.html`(YESNO → LET = 1 基按钮序号,
//! Yes→1 / No→2);②上引语料脚本;③REA `0x43fc74`(非 INPUT 直通
//! `MessageBoxA`)。

use std::path::PathBuf;

use yu_cli::engine::{Engine, EngineOptions};
use yu_cli::project::Project;
use yu_rt::dialog::DialogButtons;

fn sample_root() -> Option<PathBuf> {
    match std::env::var("YURIS_SAMPLE_ERIS_ROOT") {
        Ok(root) => Some(PathBuf::from(root)),
        Err(_) => {
            if std::env::var("YURIS_REQUIRE_SAMPLES").ok().as_deref() == Some("1") {
                panic!("YURIS_REQUIRE_SAMPLES=1 but YURIS_SAMPLE_ERIS_ROOT is missing");
            }
            eprintln!("SKIP title_close_dialog_presents_and_answers: no sample root");
            None
        }
    }
}

/// 临时存档根:预拷语料 `save/gsd.sd`(終了確認あり 的门),读写全部落在
/// `target/tmp` 副本里,不触碰工程真实存档。
fn temp_save_root(root: &std::path::Path) -> Option<PathBuf> {
    let source = root.join("save/gsd.sd");
    if !source.is_file() {
        eprintln!(
            "SKIP: 语料 {} 不存在(確認あり 前提不成立)",
            source.display()
        );
        return None;
    }
    let dir = std::env::current_dir()
        .expect("cwd")
        .join("target/tmp/b2/dialog-save");
    std::fs::create_dir_all(&dir).expect("create temp save root");
    let dest = dir.join("gsd.sd");
    std::fs::copy(&source, &dest).expect("copy gsd.sd");
    Some(dir)
}

fn run_frames(engine: &mut Engine, n: u32) {
    for _ in 0..n {
        engine.tick().expect("tick");
    }
}

#[test]
#[ignore = "需要真实语料;显式 --ignored 运行"]
fn title_close_dialog_presents_and_answers() {
    let Some(root) = sample_root() else {
        return;
    };
    let Some(save_root) = temp_save_root(&root) else {
        return;
    };
    let project = Project::discover(&root, None).expect("project");
    let mut engine = Engine::new(
        project,
        EngineOptions {
            record_events: false,
            save_root: Some(save_root),
            ..EngineOptions::default()
        },
    )
    .expect("engine");
    run_frames(&mut engine, 20); // 启动链稳定(关闭监视循环在跑)

    // 第一次关闭:es_Scenario → ES.ENDCONF.DIALOG → 原生 DIALOG 挂起。
    engine.request_window_close();
    run_frames(&mut engine, 5);
    let snapshot: Vec<(u64, DialogButtons, String, String, String)> = engine
        .pending_dialogs()
        .iter()
        .map(|d| {
            (
                d.id,
                d.buttons,
                d.caption_display(),
                d.text_display(),
                d.task.clone(),
            )
        })
        .collect();
    eprintln!("DIALOG 呈现 @frame{}: {snapshot:?}", engine.frame());
    assert_eq!(snapshot.len(), 1, "关闭请求应挂起一条原生 DIALOG");
    let (id, buttons, caption, text, _task) = snapshot[0].clone();
    assert_eq!(buttons, DialogButtons::YesNo, "終了確認 = はい/いいえ");
    assert!(
        caption.contains("確認"),
        "CAPTION 应为「確認」(ES_ENDCONF.DIALOG $cap),实际 {caption:?}"
    );
    assert!(
        text.contains("終了します"),
        "STR=アプリケーションを終了します。宜しいですか？,实际 {text:?}"
    );

    // いいえ(2):@Chk==2 → 不 END,任务继续。
    engine.answer_dialog(id, 2, None).expect("回答 いいえ");
    run_frames(&mut engine, 5);
    assert!(engine.pending_dialogs().is_empty(), "回答后对话框应清空");
    assert!(!engine.main_finished(), "いいえ 不应结束主线");
    eprintln!("いいえ 后主线仍在 @frame{}", engine.frame());

    // 第二次关闭 + はい(1):\es.END → GSD.SAVE → END(任务 Finish)。
    engine.request_window_close();
    run_frames(&mut engine, 5);
    let ids: Vec<u64> = engine.pending_dialogs().iter().map(|d| d.id).collect();
    assert_eq!(ids.len(), 1, "第二次关闭应再次挂起 DIALOG");
    assert_ne!(ids[0], id, "新请求应有新 id");
    // 决定性观察:はい 前删掉副本 gsd.sd —— 若 はい 真的驱动
    // `IF[@ret==1] \es.END` → GSD.SAVE,副档会重新落盘。
    let gsd_copy = std::env::current_dir()
        .expect("cwd")
        .join("target/tmp/b2/dialog-save/gsd.sd");
    std::fs::remove_file(&gsd_copy).ok();
    engine.answer_dialog(ids[0], 1, None).expect("回答 はい");
    assert!(engine.pending_dialogs().is_empty(), "はい 后对话框应清空");
    let mut end_ran = false;
    for _ in 0..180 {
        engine.tick().expect("tick after はい");
        if gsd_copy.is_file() {
            end_ran = true;
            break;
        }
    }
    assert!(
        end_ran,
        "はい 应驱动 ES.ENDCONF.DIALOG → \\es.END → GSD.SAVE(副档重新落盘)"
    );
    eprintln!(
        "S5 证据: DIALOG#{id}(いいえ→继续)/ #{}(はい→\\es.END→GSD.SAVE 落盘)全链通 @{}",
        ids[0],
        engine.frame()
    );
    // 注:main_finished 不在此断言 —— `END` 由非 main 任务(es.IDSubTask)
    // 执行时,内核只 Finish 该任务,主循环(SYSTEM)不退 ⇒ 程序退出链是
    // END 语义问题(REA 0x441334;另立台账,归 B10)。
}
