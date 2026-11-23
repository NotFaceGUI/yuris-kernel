//! 运行时翻译真实样本回归(ERIS 0.495 + CP932 隧道)。
//!
//! ```bash
//! export YURIS_SAMPLE_ERIS_ROOT=<project-eris>
//! export YURIS_TRANSLATE_JSON=<翻译后的 translate.json>
//! cargo test -p yu-engine --test translation -- --nocapture
//! ```
//!
//! 缺任一环境变量时显式 SKIP;`YURIS_REQUIRE_SAMPLES=1` 时缺样本失败。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use yu_engine::patch::PatchSelection;
use yu_engine::project::Project;
use yu_engine::translation;
use yu_format::text::{DisplayConfig, TextCodec};
use yu_format::Codec;
use yu_rt::event::VmEvent;
use yu_rt::input::InputTimeline;
use yu_rt::{BootConfig, Vm};

fn sample(name: &str) -> Option<PathBuf> {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => Some(PathBuf::from(value)),
        _ => {
            let message = format!("SKIP translation test: {name} not set");
            if std::env::var("YURIS_REQUIRE_SAMPLES").as_deref() == Ok("1") {
                panic!("{message} (YURIS_REQUIRE_SAMPLES=1)");
            }
            eprintln!("{message}");
            None
        }
    }
}

fn encode(text: &str, display: &DisplayConfig) -> Vec<u8> {
    let mut out = Vec::new();
    for ch in text.chars() {
        if let Some(unit) = display.default.encode_char(ch) {
            if unit < 0x100 {
                out.push(unit as u8);
            } else {
                out.push((unit & 0xFF) as u8);
                out.push((unit >> 8) as u8);
            }
        } else if let Some(pair) = display
            .tunnel
            .as_ref()
            .and_then(|table| table.encode_char(ch))
        {
            out.extend_from_slice(&pair);
        } else {
            panic!("测试译文含不可编码字符 U+{:04X}", u32::from(ch));
        }
    }
    out
}

fn load_translated_words(path: &Path, display: &DisplayConfig) -> BTreeSet<Vec<u8>> {
    let bytes = std::fs::read(path).expect("读 translate.json");
    let text = std::str::from_utf8(&bytes).expect("translate.json 是 UTF-8");
    let document = yu_engine::json::parse(text).expect("translate.json 是 JSON");
    let mut out = BTreeSet::new();
    for entry in document
        .get("text")
        .and_then(yu_engine::json::Value::as_array)
        .expect("text 数组")
    {
        if entry.get("source").and_then(yu_engine::json::Value::as_str) != Some("word") {
            continue;
        }
        let Some(translated) = entry
            .get("translation")
            .and_then(yu_engine::json::Value::as_str)
        else {
            continue;
        };
        if !translated.is_empty() {
            out.insert(encode(translated, display));
        }
    }
    out
}

#[test]
fn eris_translation_replaces_word_payloads() {
    let Some(root) = sample("YURIS_SAMPLE_ERIS_ROOT") else {
        return;
    };
    let Some(translate) = sample("YURIS_TRANSLATE_JSON") else {
        return;
    };
    let project = Project::discover(&root, None).expect("Project::discover");
    let key = project.resolve_script_key(None).expect("script key");
    let profile = PatchSelection::default()
        .profile(&project)
        .expect("patch profile");
    let source = project.open_source().expect("open source");
    let aux = project.open_aux().expect("open aux");
    let loaded = translation::load(
        &translate,
        &profile,
        source.as_ref(),
        Some(aux.as_ref()),
        key.bytes,
    )
    .expect("装载翻译");
    assert_eq!(loaded.report.words, 464, "ERIS 应有 464 条 WORD 译文");
    assert!(loaded.report.tunnel_chars > 0, "简体中文应分配隧道码");
    assert_eq!(loaded.display.default, Codec::Cp932);

    let expected = load_translated_words(&translate, &loaded.display);
    let display = loaded.display.clone();
    let cfg = BootConfig {
        // RP_101 是官方 userscript 文本测试(10 行 WORD+RETURNCODE),
        // 直接进入即可验证 WORD 替换,不必模拟标题画面点击。
        entry_label: "RP_101".to_string(),
        script_key: key.bytes,
        codecs: profile.codecs(),
        display: display.clone(),
        translation: Some(loaded.table),
        ..BootConfig::default()
    };
    let mut vm = Vm::boot_with_aux(source, Some(aux), cfg).expect("Vm::boot_with_aux");
    // RP_101 本身不建文本层;给一个足够大的表面,避免自动排版停死。
    vm.set_text_window(Some((800, 600)));

    let mut seen = 0usize;
    let mut matched = None;
    let mut events = vm.take_events();
    for frame in 0..60u32 {
        for event in events.drain(..) {
            if let VmEvent::Text { bytes, .. } = event {
                seen += 1;
                if expected.contains(&bytes) {
                    matched = Some(bytes);
                    break;
                }
            }
        }
        if matched.is_some() {
            break;
        }
        let _ = vm.tick_at(frame.saturating_mul(16), &InputTimeline::new());
        events = vm.take_events();
    }
    let matched =
        matched.unwrap_or_else(|| panic!("60 帧内未命中任何 WORD 译文(共 {seen} 个文本事件)"));
    let text = TextCodec::with_tunnel(display.default, display.tunnel.as_ref().expect("隧道表"));
    let (chars, invalid) = text.decode_lossy(&matched);
    let decoded: String = chars.into_iter().collect();
    assert!(invalid == 0, "译文事件必须能按扩展隧道解码: {decoded}");
    let stats = vm.translation_stats().expect("翻译统计");
    assert!(stats.word_hits > 0, "WORD hook 应至少命中一次");
    eprintln!(
        "translation hook: seen={seen} matched={decoded:?} hits={} misses={} tunnel=+{}",
        stats.word_hits, stats.word_misses, loaded.report.tunnel_chars
    );
}

// ------------------------------------------------------------------ 合成工程

#[path = "../../yu-rt/tests/support/mod.rs"]
mod support;

use support::{GroupSpec, ProjectBuilder};
use yu_rt::translation::TranslationTable;

const OP_END: u8 = 14;
const OP_RETURNCODE: u8 = 79;
const OP_WORD: u8 = 106;

#[test]
fn synthetic_word_hook_uses_translation_payload() {
    let mut builder = ProjectBuilder::new();
    builder.labels = vec![("SYSTEM_START", 0, 0, 0)];
    builder.scripts = vec![vec![
        GroupSpec::new(OP_WORD).window(0, b"AA".to_vec()),
        GroupSpec::new(OP_RETURNCODE).window(1, Vec::new()),
        GroupSpec::new(OP_END),
    ]];
    let source = builder.build();

    let mut table = TranslationTable::default();
    table.insert_word(0, 0, b"BB".to_vec());
    let cfg = BootConfig {
        translation: Some(table),
        ..BootConfig::default()
    };
    let mut vm = Vm::boot(Box::new(source), cfg).expect("Vm::boot");
    let _ = vm.tick_at(0, &InputTimeline::new());
    let mut texts = Vec::new();
    for event in vm.take_events() {
        if let VmEvent::Text { bytes, .. } = event {
            texts.push(bytes);
        }
    }
    assert!(
        texts.iter().any(|bytes| bytes == b"BB"),
        "WORD(0,0) 应输出译文 BB,实际事件 {texts:?}"
    );
    let stats = vm.translation_stats().expect("统计");
    assert_eq!(stats.word_hits, 1, "一次 WORD 命中");
}
