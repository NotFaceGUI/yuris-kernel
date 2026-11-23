//! `yu_engine::export` 的真实样本回归。
//!
//! ```bash
//! export YURIS_SAMPLE_ERIS_ROOT=<project-eris>
//! export YURIS_SAMPLE_V555_ROOT=<v555 商业样本根>
//! cargo test -p yu-engine --test export -- --nocapture
//! ```
//!
//! 缺样本时显式 SKIP 并打印原因(不空跑通过);`YURIS_REQUIRE_SAMPLES=1`
//! 时缺样本直接失败,供 CI 固定语料运行。

use std::path::{Path, PathBuf};

use yu_engine::export::{self, ExportFormat, ExportOptions, TextCodecChoice};
use yu_engine::project::Project;

fn sample(name: &str) -> Option<PathBuf> {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => Some(PathBuf::from(value)),
        _ => {
            let message = format!("SKIP export test: {name} not set");
            if std::env::var("YURIS_REQUIRE_SAMPLES").as_deref() == Ok("1") {
                panic!("{message} (YURIS_REQUIRE_SAMPLES=1)");
            }
            eprintln!("{message}");
            None
        }
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("yuris-export-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn export(root: &Path, name: &str) -> (PathBuf, export::ExportReport) {
    let project = Project::discover(root, None).expect("Project::discover");
    let out = scratch(name);
    let options = ExportOptions {
        text_codec: TextCodecChoice::Auto,
        script_key: None,
        include_literals: true,
        format: ExportFormat::Csv,
        runtime_characters: false,
        runtime_ticks: 600,
    };
    let report = export::export_project(&project, &out, &options).expect("export_project");
    (out, report)
}

fn read_csv(path: &Path) -> String {
    let bytes = std::fs::read(path).expect("读 CSV");
    assert!(
        bytes.starts_with(&[0xEF, 0xBB, 0xBF]),
        "{} 缺 UTF-8 BOM",
        path.display()
    );
    String::from_utf8(bytes[3..].to_vec()).expect("CSV 必须是 UTF-8")
}

#[test]
fn eris_export_word_text_matches_corpus() {
    let Some(root) = sample("YURIS_SAMPLE_ERIS_ROOT") else {
        return;
    };
    let (out, report) = export(&root, "eris");
    assert!(report.scripts > 0, "ERIS 应解出脚本");
    assert_eq!(
        report.text_units, 464,
        "ERIS 主语料的 WORD 段数应保持 464(见 unpack_text 交叉核对);实际 {}",
        report.text_units
    );
    assert!(report.literal_units > 0, "UI 字符串常量应有内容");
    let text = read_csv(&out.join("text.csv"));
    let mut lines = text.lines();
    assert_eq!(
        lines.next(),
        Some("script_id,script_path,pc,unit,line,codec,text"),
        "text.csv 表头"
    );
    assert!(
        text.contains(",cp932,") || text.contains(",gbk,"),
        "text.csv 每行必须带 codec 列"
    );
    assert!(
        text.contains("E-mote サンプル"),
        "ERIS 的日文 WORD 文本应完整导出"
    );
    assert!(
        report.characters >= 4,
        "ERIS demo 的 userdefine/jp_0002 有 4 个角色名"
    );
    let characters = read_csv(&out.join("characters.csv"));
    assert!(characters.contains("優理"), "角色表应解出 優理");
    assert!(
        characters.contains(r"userdefine\jp_0002.txt"),
        "角色表应记录 table 路径"
    );
    assert_eq!(std::fs::read(out.join("manifest.json")).is_ok(), true);
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn translate_json_contains_only_translation_arrays() {
    let Some(root) = sample("YURIS_SAMPLE_ERIS_ROOT") else {
        return;
    };
    let project = Project::discover(&root, None).expect("Project::discover");
    let out = scratch("eris-translate-json");
    let options = ExportOptions {
        text_codec: TextCodecChoice::Auto,
        script_key: None,
        include_literals: true,
        format: ExportFormat::TranslateJson,
        runtime_characters: false,
        runtime_ticks: 600,
    };
    let report = export::export_project(&project, &out, &options).expect("export_project");
    assert!(
        !out.join("text.csv").exists(),
        "translate-json 不应写 text.csv"
    );
    assert!(
        !out.join("literals.csv").exists(),
        "translate-json 不应写 literals.csv"
    );
    assert!(
        !out.join("scenario.csv").exists(),
        "translate-json 不应写 scenario.csv"
    );
    let text = read_plain(&out.join("translate.json"));
    let value = yu_engine::json::parse(&text).expect("translate.json 解析");
    assert_eq!(
        value.get("schema").and_then(|v| v.as_str()),
        Some("yuris-translate-1")
    );
    let characters = value.get("characters").and_then(|v| v.as_array()).unwrap();
    let choices = value.get("choices").and_then(|v| v.as_array()).unwrap();
    let literals = value
        .get("literals")
        .and_then(|v| v.as_array())
        .expect("literals 数组");
    let body = value.get("text").and_then(|v| v.as_array()).unwrap();
    assert_eq!(characters.len(), report.characters as usize);
    assert_eq!(characters.len(), 4);
    assert_eq!(choices.len(), report.choices as usize);
    assert_eq!(literals.len(), report.literal_units as usize);
    assert!(
        literals.iter().all(|item| {
            item.get("context").and_then(|v| v.as_str()).is_some()
                && item.get("translation").and_then(|v| v.as_str()) == Some("")
        }),
        "literals 必须带 context 与空译文槽"
    );
    assert!(
        characters.iter().all(|item| {
            item.get("script_id").and_then(|v| v.as_u64()).is_some()
                && item.get("pc").and_then(|v| v.as_u64()).is_some()
        }),
        "角色条目必须带运行期 script_id/pc"
    );
    assert_eq!(body.len(), report.text_units as usize);
    assert_eq!(body.len(), 464);
    assert!(text.contains("優理"));
    assert!(body
        .iter()
        .all(|item| { item.get("translation").and_then(|v| v.as_str()) == Some("") }));
    let _ = std::fs::remove_dir_all(&out);
}

fn read_plain(path: &Path) -> String {
    String::from_utf8(std::fs::read(path).expect("读文件")).expect("UTF-8")
}

#[test]
fn v555_export_external_scenario_and_characters() {
    let Some(root) = sample("YURIS_SAMPLE_V555_ROOT") else {
        return;
    };
    let (out, report) = export(&root, "v555");
    assert!(
        report.scenario_files > 0,
        "v555 外置剧本应被发现(当前 {})",
        report.scenario_files
    );
    assert!(
        report.scenario_units > 0,
        "v555 应导出台词宏(当前 {})",
        report.scenario_units
    );
    assert!(
        report.character_variants >= 3,
        "多语样本应有 base/cn/tw 三套角色表;实际 {}",
        report.character_variants
    );
    assert!(report.characters > 0, "角色表为空");

    let scenario = read_csv(&out.join("scenario.csv"));
    assert_eq!(
        scenario.lines().next(),
        Some("variant,file,line,seq,kind,macro,scene,label,codec,text"),
        "scenario.csv 表头"
    );
    assert!(scenario.contains(",macro,"), "宏台词应标 kind=macro");
    assert!(scenario.contains("sc_cn_scenario"), "应有简中变体");
    assert!(scenario.contains("sc_tw_scenario"), "应有繁中变体");
    assert!(!scenario.contains('\u{FFFD}'), "导出的台词不得含替换字符");

    let characters = read_csv(&out.join("characters.csv"));
    assert!(characters.contains("\ncn,"), "角色表缺 cn 变体");
    assert!(characters.contains("\ntw,"), "角色表缺 tw 变体");
    assert!(characters.contains("【大姊姊】"), "cn 角色名显式应解出");
    let _ = std::fs::remove_dir_all(&out);
}
