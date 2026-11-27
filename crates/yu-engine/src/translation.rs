//! `translate.json` 运行时装载(JSON → 已编码 `TranslationTable`)。
//!
//! 编码策略(与 `docs/translation-workflow.md` §5 一致):
//!
//! - 目标 codec = 当前补丁的**显示 codec**(`PatchProfile::text_default`);
//! - CP932 工程:译文优先直接编码;CP932 表示不了的字符分配到 SJIS 隧道码,
//!   并扩展 `sjis_ext.bin` 的既有表。隧道码是 CP932 前导 + 低位尾字节
//!   (官方分配算法),脚本只按前导字节步进,`STRLEN`/`_strmid`/`TEXT[FILE]`
//!   的语义不变;
//! - GBK/Big5/CP949/CP1252 工程:直接按该 codec 编码;任何不可编码字符在
//!   装载期显式报错,不静默替换;
//! - UTF-8 工程(本地化发行版):直接写 UTF-8 字节,不需要隧道表;UTF-8
//!   覆盖全部 Unicode 码位,装载期不会出现不可编码字符。
//!
//! 隧道分配会扫描脚本体窗口载荷、`ysv` 字符串初值与外置剧本,把源码中
//! 实际出现过的双字节对全部保留,避免译文隧道码“劫持”未翻译原文。

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use yu_format::text::{DisplayConfig, TunnelTable};
use yu_format::{scenario::UnitKind, Codec};
use yu_rt::tables::VarInit;
use yu_rt::translation::{ScenarioUnit, TranslationTable};
use yu_rt::TableSet;
use yu_vfs::DataSource;

use crate::json::{self, Value};
use crate::patch::PatchProfile;
use crate::Failure;

/// 装载结果:表、扩展后的显示配置、诊断统计。
#[derive(Clone, Debug)]
pub struct LoadedTranslation {
    pub table: TranslationTable,
    pub display: DisplayConfig,
    pub report: TranslationReport,
}

/// 装载统计(启动日志/宿主展示用)。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TranslationReport {
    pub words: usize,
    pub scenario_lines: usize,
    pub scenario_units: usize,
    pub characters: usize,
    pub choices: usize,
    pub literals: usize,
    pub call_sites: usize,
    pub tunnel_chars: usize,
    pub encoded_bytes: usize,
    pub warnings: Vec<String>,
}

impl TranslationReport {
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "words={} scenario_lines={} scenario_units={} characters={} choices={} literals={} calls={} tunnel=+{} bytes={} warnings={}",
            self.words,
            self.scenario_lines,
            self.scenario_units,
            self.characters,
            self.choices,
            self.literals,
            self.call_sites,
            self.tunnel_chars,
            self.encoded_bytes,
            self.warnings.len()
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum PendingTarget {
    Word {
        script_id: u32,
        pc: u32,
    },
    Scenario {
        file: String,
        line: u32,
        seq: u32,
        kind: UnitKind,
        macro_name: Vec<u8>,
    },
    Call {
        script_id: u32,
        pc: u32,
        arg: u32,
    },
    Literal {
        script_id: u32,
        pc: u32,
        window: u16,
        segment: u32,
    },
}

struct Pending {
    origin: String,
    target: PendingTarget,
    text: String,
    /// 条目声明的源 codec(旧文件没有该字段 = `None`)。
    codec: Option<Codec>,
}

fn entry_codec(item: &Value) -> Option<Codec> {
    item.get("codec")
        .and_then(Value::as_str)
        .and_then(|name| Codec::from_name(name).ok())
}

/// 读取 `translate.json`,按目标 codec 编码并建立运行时表。
pub fn load(
    path: &Path,
    profile: &PatchProfile,
    source: &dyn DataSource,
    aux: Option<&dyn DataSource>,
    script_key: [u8; 4],
) -> Result<LoadedTranslation, Failure> {
    let bytes = std::fs::read(path)
        .map_err(|e| Failure::runtime(format!("读翻译文件 {} 失败: {e}", path.display())))?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|e| Failure::runtime(format!("翻译文件 {} 不是 UTF-8: {e}", path.display())))?;
    let document = json::parse(text)
        .map_err(|e| Failure::runtime(format!("翻译文件 {} JSON 解析失败: {e}", path.display())))?;

    if document.get("schema").and_then(Value::as_str) != Some("yuris-translate-1") {
        return Err(Failure::runtime(format!(
            "翻译文件 {} 的 schema 不是 `yuris-translate-1`",
            path.display()
        )));
    }

    let mut report = TranslationReport::default();
    let codec = profile.text_default;
    let mut pending = Vec::new();
    read_characters(&document, &mut pending, &mut report)?;
    read_choices(&document, &mut pending, &mut report)?;
    read_literals(&document, &mut pending)?;
    read_text(&document, &mut pending, &mut report)?;

    // 多语 translate.json 可能含 base/cn/tw 三套同坐标条目:让与当前
    // 显示 codec 一致(或无 codec 字段)的条目排在最后,后写覆盖前写。
    pending.sort_by_key(|entry| u8::from(entry.codec.is_none() || entry.codec == Some(codec)));
    // 先按坐标去重,只编码/分配隧道给最终生效的条目。否则被覆盖的旧
    // codec 条目只要含目标 codec 表外字符,就会把整份翻译打挂(它根本
    // 不会生效),还会污染 sjis_ext。
    let before = pending.len();
    let mut winners: Vec<Pending> = Vec::with_capacity(pending.len());
    let mut seen: BTreeSet<PendingTarget> = BTreeSet::new();
    for entry in pending.drain(..).rev() {
        if seen.insert(entry.target.clone()) {
            winners.push(entry);
        }
    }
    winners.reverse();
    let pending = winners;
    if pending.len() != before {
        report.warnings.push(format!(
            "translate.json 有 {} 条同坐标条目被覆盖,只编码最终生效项",
            before - pending.len()
        ));
    }
    // UTF-8 覆盖全部 Unicode 码位,不需要隧道分配(也没有可分配空间)。
    let mut chars: BTreeSet<char> = BTreeSet::new();
    if codec != Codec::Utf8 {
        for entry in &pending {
            for ch in entry.text.chars() {
                if codec.encode_char(ch).is_none() {
                    chars.insert(ch);
                }
            }
        }
    }
    let mut tunnel = profile.tunnel.clone();
    if !chars.is_empty() {
        if codec != Codec::Cp932 {
            let mut detail = String::new();
            for entry in &pending {
                for ch in entry.text.chars() {
                    if codec.encode_char(ch).is_none() {
                        detail.push_str(&format!(
                            "\n  {}: U+{:04X} ({ch:?}) 不能在 {} 中编码",
                            entry.origin,
                            u32::from(ch),
                            codec.as_str()
                        ));
                    }
                }
            }
            return Err(Failure::runtime(format!(
                "翻译文本含 {} 个 {} 不可编码字符;请换 GBK/Big5/CP949/CP1252/UTF-8 补丁或改用 CP932+隧道路线{detail}",
                chars.len(),
                codec.as_str()
            )));
        }
        let used = collect_source_pairs(source, aux, script_key)?;
        let (pairs, mapping) = allocate_tunnel(chars.iter().copied(), tunnel.as_ref(), &used)?;
        let mut all_pairs: Vec<([u8; 2], char)> = Vec::new();
        if let Some(base) = tunnel.as_ref() {
            all_pairs.extend(base.pairs());
        }
        all_pairs.extend(pairs.iter().copied());
        let table = TunnelTable::from_pairs(all_pairs)
            .map_err(|d| Failure::runtime(format!("扩展隧道表构造失败: {d}")))?;
        report.tunnel_chars = pairs.len();
        tunnel = Some(table);
        // 分配结果必须都能从扩展表反查(否则编码阶段会再报错)。
        if let Some(table) = tunnel.as_ref() {
            for (ch, pair) in &mapping {
                if table.char_for(*pair) != Some(*ch) {
                    return Err(Failure::runtime(format!(
                        "隧道分配自检失败:U+{:04X} 未映射到 {:02X}{:02X}",
                        u32::from(*ch),
                        pair[0],
                        pair[1]
                    )));
                }
            }
        }
    }

    let mut table = TranslationTable::default();
    let mut scenario_map: BTreeMap<(String, u32), Vec<ScenarioUnit>> = BTreeMap::new();
    for entry in pending {
        let encoded = encode_text(
            &entry.text,
            codec,
            tunnel.as_ref(),
            &entry.origin,
            &mut report,
        )?;
        report.encoded_bytes += encoded.len();
        match entry.target {
            PendingTarget::Word { script_id, pc } => {
                table.insert_word(script_id, pc, encoded);
                report.words += 1;
            }
            PendingTarget::Call { script_id, pc, arg } => {
                table.insert_call(script_id, pc, arg, encoded);
            }
            PendingTarget::Literal {
                script_id,
                pc,
                window,
                segment,
            } => {
                table.insert_literal(script_id, pc, window, segment, encoded);
                report.literals += 1;
            }
            PendingTarget::Scenario {
                file,
                line,
                seq,
                kind,
                macro_name,
            } => {
                scenario_map
                    .entry((file, line))
                    .or_default()
                    .push(ScenarioUnit {
                        seq,
                        kind,
                        name: macro_name,
                        bytes: encoded,
                    });
            }
        }
    }
    for ((file, line), mut units) in scenario_map {
        units.sort_by_key(|unit| unit.seq);
        let before = units.len();
        units.dedup_by_key(|unit| unit.seq);
        if units.len() != before {
            report.warnings.push(format!(
                "scenario {file} line {line}: 有重复 seq,保留先出现的 {} 条",
                units.len()
            ));
        }
        let unit_count = units.len();
        table.insert_scenario(&file, line, units);
        report.scenario_lines += 1;
        report.scenario_units += unit_count;
    }
    report.call_sites = table.call_count();
    Ok(LoadedTranslation {
        table,
        display: DisplayConfig {
            default: codec,
            tunnel,
        },
        report,
    })
}

fn read_characters(
    document: &Value,
    pending: &mut Vec<Pending>,
    report: &mut TranslationReport,
) -> Result<(), Failure> {
    let Some(items) = document.get("characters").and_then(Value::as_array) else {
        return Ok(());
    };
    for item in items {
        let key = item
            .get("key")
            .and_then(Value::as_str)
            .unwrap_or("<character>");
        let Some(script_id) = item.get("script_id").and_then(Value::as_u64) else {
            report.warnings.push(format!(
                "character {key}: 缺 script_id(旧版导出),角色名替换已跳过"
            ));
            continue;
        };
        let Some(pc) = item.get("pc").and_then(Value::as_u64) else {
            report
                .warnings
                .push(format!("character {key}: 缺 pc(旧版导出),角色名替换已跳过"));
            continue;
        };
        let display = item
            .get("display_translation")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let name = item
            .get("name_translation")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if display.is_empty() && name.is_empty() {
            continue;
        }
        let script_id = u32::try_from(script_id)
            .map_err(|_| Failure::runtime(format!("character {key}: script_id 超出 u32")))?;
        let pc = u32::try_from(pc)
            .map_err(|_| Failure::runtime(format!("character {key}: pc 超出 u32")))?;
        if !display.is_empty() {
            pending.push(Pending {
                origin: format!("character {key} display"),
                target: PendingTarget::Call {
                    script_id,
                    pc,
                    arg: 0,
                },
                text: display,
                codec: entry_codec(item),
            });
        }
        if !name.is_empty() {
            pending.push(Pending {
                origin: format!("character {key} name"),
                target: PendingTarget::Call {
                    script_id,
                    pc,
                    arg: 1,
                },
                text: name,
                codec: entry_codec(item),
            });
        }
        report.characters += 1;
    }
    Ok(())
}

fn read_choices(
    document: &Value,
    pending: &mut Vec<Pending>,
    report: &mut TranslationReport,
) -> Result<(), Failure> {
    let Some(items) = document.get("choices").and_then(Value::as_array) else {
        return Ok(());
    };
    let mut filled = 0usize;
    for item in items {
        let key = item
            .get("key")
            .and_then(Value::as_str)
            .unwrap_or("<choice>");
        let translation = item
            .get("translation")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if translation.is_empty() {
            continue;
        }
        let script_id = item
            .get("script_id")
            .and_then(Value::as_u64)
            .ok_or_else(|| Failure::runtime(format!("choice {key}: 缺 script_id")))?;
        let pc = item
            .get("pc")
            .and_then(Value::as_u64)
            .ok_or_else(|| Failure::runtime(format!("choice {key}: 缺 pc")))?;
        let arg = item
            .get("arg")
            .and_then(Value::as_u64)
            .ok_or_else(|| Failure::runtime(format!("choice {key}: 缺 arg")))?;
        pending.push(Pending {
            origin: format!("choice {key}"),
            target: PendingTarget::Call {
                script_id: u32::try_from(script_id)
                    .map_err(|_| Failure::runtime(format!("choice {key}: script_id 超出 u32")))?,
                pc: u32::try_from(pc)
                    .map_err(|_| Failure::runtime(format!("choice {key}: pc 超出 u32")))?,
                arg: u32::try_from(arg)
                    .map_err(|_| Failure::runtime(format!("choice {key}: arg 超出 u32")))?,
            },
            text: translation,
            codec: entry_codec(item),
        });
        filled += 1;
    }
    report.choices += filled;
    Ok(())
}

fn read_literals(document: &Value, pending: &mut Vec<Pending>) -> Result<(), Failure> {
    let Some(items) = document.get("literals").and_then(Value::as_array) else {
        return Ok(());
    };
    for item in items {
        let key = item
            .get("key")
            .and_then(Value::as_str)
            .unwrap_or("<literal>");
        let translation = item
            .get("translation")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if translation.is_empty() {
            continue;
        }
        let number = |field: &str| -> Result<u64, Failure> {
            item.get(field)
                .and_then(Value::as_u64)
                .ok_or_else(|| Failure::runtime(format!("literal {key}: 缺 {field}")))
        };
        let script_id = u32::try_from(number("script_id")?)
            .map_err(|_| Failure::runtime(format!("literal {key}: script_id 超出 u32")))?;
        let pc = u32::try_from(number("pc")?)
            .map_err(|_| Failure::runtime(format!("literal {key}: pc 超出 u32")))?;
        let window = u16::try_from(number("window")?)
            .map_err(|_| Failure::runtime(format!("literal {key}: window 超出 u16")))?;
        let segment = u32::try_from(number("segment")?)
            .map_err(|_| Failure::runtime(format!("literal {key}: segment 超出 u32")))?;
        pending.push(Pending {
            origin: key.to_string(),
            target: PendingTarget::Literal {
                script_id,
                pc,
                window,
                segment,
            },
            text: translation,
            codec: entry_codec(item),
        });
        // `report.literals` 在真正插入表后累加(空编码失败会终止装载)。
    }
    Ok(())
}

fn read_text(
    document: &Value,
    pending: &mut Vec<Pending>,
    report: &mut TranslationReport,
) -> Result<(), Failure> {
    let Some(items) = document.get("text").and_then(Value::as_array) else {
        return Ok(());
    };
    for item in items {
        let key = item.get("key").and_then(Value::as_str).unwrap_or("<text>");
        let translation = item
            .get("translation")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if translation.is_empty() {
            continue;
        }
        match item.get("source").and_then(Value::as_str).unwrap_or("") {
            "word" => {
                let script_id = item
                    .get("script_id")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| Failure::runtime(format!("word {key}: 缺 script_id")))?;
                let pc = item
                    .get("pc")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| Failure::runtime(format!("word {key}: 缺 pc")))?;
                pending.push(Pending {
                    origin: key.to_string(),
                    target: PendingTarget::Word {
                        script_id: u32::try_from(script_id).map_err(|_| {
                            Failure::runtime(format!("word {key}: script_id 超出 u32"))
                        })?,
                        pc: u32::try_from(pc)
                            .map_err(|_| Failure::runtime(format!("word {key}: pc 超出 u32")))?,
                    },
                    text: translation,
                    codec: entry_codec(item),
                });
            }
            "scenario" => {
                let file = item
                    .get("file")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Failure::runtime(format!("scenario {key}: 缺 file")))?
                    .to_string();
                let line = item
                    .get("line")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| Failure::runtime(format!("scenario {key}: 缺 line")))?;
                let seq = item
                    .get("seq")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| Failure::runtime(format!("scenario {key}: 缺 seq")))?;
                let kind = match item.get("kind").and_then(Value::as_str) {
                    Some("macro") => UnitKind::Macro,
                    Some("text") | None => UnitKind::Text,
                    Some(other) => {
                        report
                            .warnings
                            .push(format!("scenario {key}: 未知 kind `{other}`,按 text 处理"));
                        UnitKind::Text
                    }
                };
                let macro_name = item
                    .get("macro")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_ascii_uppercase()
                    .into_bytes();
                pending.push(Pending {
                    origin: key.to_string(),
                    target: PendingTarget::Scenario {
                        file,
                        line: u32::try_from(line).map_err(|_| {
                            Failure::runtime(format!("scenario {key}: line 超出 u32"))
                        })?,
                        seq: u32::try_from(seq).map_err(|_| {
                            Failure::runtime(format!("scenario {key}: seq 超出 u32"))
                        })?,
                        kind,
                        macro_name,
                    },
                    text: translation,
                    codec: entry_codec(item),
                });
            }
            other => report
                .warnings
                .push(format!("text {key}: 未知 source `{other}`,已跳过")),
        }
    }
    Ok(())
}

fn encode_text(
    text: &str,
    codec: Codec,
    tunnel: Option<&TunnelTable>,
    origin: &str,
    report: &mut TranslationReport,
) -> Result<Vec<u8>, Failure> {
    // UTF-8 没有 u16 码位表:`encode_char` 对非 ASCII 恒 `None`,必须走
    // `as_bytes`(与 `Codec::encode` 的 UTF-8 分支同一口径)。
    if codec == Codec::Utf8 {
        return Ok(text.as_bytes().to_vec());
    }
    let mut out = Vec::with_capacity(text.len());
    for ch in text.chars() {
        if let Some(unit) = codec.encode_char(ch) {
            if unit < 0x100 {
                out.push(unit as u8);
            } else {
                out.push((unit & 0xFF) as u8);
                out.push((unit >> 8) as u8);
            }
            continue;
        }
        if let Some(pair) = tunnel.and_then(|table| table.encode_char(ch)) {
            out.extend_from_slice(&pair);
            continue;
        }
        let message = format!(
            "{origin}: U+{:04X} ({ch:?}) 不能在 {} 或隧道表中编码",
            u32::from(ch),
            codec.as_str()
        );
        report.warnings.push(message.clone());
        return Err(Failure::runtime(message));
    }
    Ok(out)
}

fn valid_cp932_pair(pair: [u8; 2]) -> bool {
    matches!(pair[0], 0x81..=0x9F | 0xE0..=0xFC) && matches!(pair[1], 0x40..=0x7E | 0x80..=0xFC)
}

/// 隧道码空间:CP932 前导 + 低位尾字节(官方 `idx_to_code_point` 的
/// 取值域),也接受标准 CP932 双字节对。
fn valid_tunnel_pair(pair: [u8; 2]) -> bool {
    let lead = matches!(pair[0], 0x81..=0x9F | 0xE0..=0xFC);
    let standard = matches!(pair[1], 0x40..=0x7E | 0x80..=0xFC);
    let low =
        (0x01..=0x3F).contains(&pair[1]) && !matches!(pair[1], 0x09 | 0x0A | 0x0D | 0x20 | 0x2C);
    lead && (standard || low)
}

/// 从源数据收集所有实际出现的 CP932 双字节对。
///
/// 覆盖:全部脚本体窗口载荷、`ysv` 字符串初值、外置剧本文件。扫描按
/// 相邻字节滑动(比按 codec 步进更保守),目的是让隧道码永不覆盖源码中
/// 已经存在的字节对。
fn collect_source_pairs(
    source: &dyn DataSource,
    aux: Option<&dyn DataSource>,
    script_key: [u8; 4],
) -> Result<BTreeSet<[u8; 2]>, Failure> {
    let mut used = BTreeSet::new();
    let mut tables = TableSet::load_from(source, aux, script_key)
        .map_err(|d| Failure::runtime(format!("翻译隧道扫描:装载表失败 {d}")))?;
    let ids: Vec<u32> = tables
        .boot
        .scripts
        .entries
        .iter()
        .filter(|entry| entry.var_count != u32::MAX && entry.label_count != u32::MAX)
        .map(|entry| entry.index)
        .collect();
    for script_id in ids {
        let script = tables
            .load_script(source, script_id)
            .map_err(|d| Failure::runtime(format!("翻译隧道扫描:装载脚本 {script_id} 失败 {d}")))?;
        for window in &script.windows {
            if let Some(payload) = script.window_payload(window) {
                collect_pairs(payload, &mut used);
            }
        }
    }
    for record in &tables.boot.vars.records {
        if let VarInit::Str(bytes) = &record.init {
            collect_pairs(bytes, &mut used);
        }
    }
    drop(tables);
    collect_scenario_pairs(source, &mut used)?;
    Ok(used)
}

fn collect_scenario_pairs(
    source: &dyn DataSource,
    used: &mut BTreeSet<[u8; 2]>,
) -> Result<(), Failure> {
    let mut dirs = Vec::new();
    crate::export::find_scenario_dirs(source, &[], "", 0, None, &mut dirs)?;
    for (dir_name, dir_bytes) in dirs {
        let mut files = Vec::new();
        crate::export::walk_files(source, &dir_bytes, &dir_name, 0, &mut files)?;
        for (_rel, name_bytes) in files {
            let key = yu_vfs::LookupKey::new(&name_bytes, crate::project::CODEC)
                .map_err(|e| Failure::runtime(format!("翻译隧道扫描:剧本键非法 {e}")))?;
            match source.open(&key) {
                Ok(bytes) => collect_pairs(&bytes, used),
                Err(diag) => {
                    return Err(Failure::runtime(format!(
                        "翻译隧道扫描:读剧本 {} 失败 {diag}",
                        String::from_utf8_lossy(&name_bytes)
                    )))
                }
            }
        }
    }
    Ok(())
}

fn collect_pairs(bytes: &[u8], used: &mut BTreeSet<[u8; 2]>) {
    for pair in bytes.windows(2) {
        let lead = pair[0];
        if matches!(lead, 0x81..=0x9F | 0xE0..=0xFC) && valid_cp932_pair([lead, pair[1]]) {
            used.insert([lead, pair[1]]);
        }
    }
}

/// `(新分配对, 字符→码对)`。
type TunnelAllocation = (Vec<([u8; 2], char)>, BTreeMap<char, [u8; 2]>);

/// 为 `chars` 分配隧道码;返回新分配对与字符→码对映射。
fn allocate_tunnel(
    chars: impl Iterator<Item = char>,
    base: Option<&TunnelTable>,
    source_pairs: &BTreeSet<[u8; 2]>,
) -> Result<TunnelAllocation, Failure> {
    let mut used = source_pairs.clone();
    if let Some(base) = base {
        used.extend(base.pairs().map(|(pair, _)| pair));
    }
    let mut mapping = BTreeMap::new();
    let mut allocated = Vec::new();
    let mut next = 0usize;
    for ch in chars {
        if let Some(pair) = base.and_then(|table| table.encode_char(ch)) {
            mapping.insert(ch, pair);
            continue;
        }
        let mut chosen = None;
        while next <= 200_000 {
            let pair = yu_format::text::idx_to_code_point(next);
            next += 1;
            if !valid_tunnel_pair(pair) || used.contains(&pair) {
                continue;
            }
            chosen = Some(pair);
            break;
        }
        let Some(pair) = chosen else {
            return Err(Failure::runtime(format!(
                "隧道码空间不足:字符 U+{:04X} ({ch:?}) 无可用 CP932 双字节对",
                u32::from(ch)
            )));
        };
        used.insert(pair);
        mapping.insert(ch, pair);
        allocated.push((pair, ch));
    }
    Ok((allocated, mapping))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pair_scan_marks_valid_cp932_pairs() {
        let mut used = BTreeSet::new();
        collect_pairs(b"ab\x82\xa0\x81\x5ccd", &mut used);
        assert!(used.contains(&[0x82, 0xA0]));
        assert!(used.contains(&[0x81, 0x5C]));
        assert!(!used.contains(&[0x61, 0x62]));
    }

    #[test]
    fn tunnel_allocation_skips_source_and_forbidden_pairs() {
        let mut source = BTreeSet::new();
        source.insert(yu_format::text::idx_to_code_point(0));
        let (allocated, mapping) =
            allocate_tunnel(['这', '们'].into_iter(), None, &source).unwrap();
        assert_eq!(allocated.len(), 2);
        assert!(!allocated
            .iter()
            .any(|(pair, _)| source.contains(pair) || !valid_tunnel_pair(*pair)));
        assert_eq!(mapping.len(), 2);
    }

    #[test]
    fn tunnel_reuses_existing_mapping() {
        let base = TunnelTable::from_pairs([([0xF0, 0x40], '这')]).unwrap();
        let (allocated, mapping) =
            allocate_tunnel(['这', '们'].into_iter(), Some(&base), &BTreeSet::new()).unwrap();
        assert_eq!(mapping[&'这'], [0xF0, 0x40]);
        assert_eq!(allocated.len(), 1);
    }

    #[test]
    fn gbk_encoding_requires_direct_bytes() {
        let mut report = TranslationReport::default();
        let bytes = encode_text("中文", Codec::Gbk, None, "t", &mut report).unwrap();
        assert_eq!(bytes, b"\xD6\xD0\xCE\xC4");
        assert!(encode_text("\u{20000}", Codec::Gbk, None, "t", &mut report).is_err());
    }

    #[test]
    fn utf8_encoding_writes_bytes_without_tunnel() {
        let mut report = TranslationReport::default();
        let bytes = encode_text("中文Aあ", Codec::Utf8, None, "t", &mut report).unwrap();
        assert_eq!(bytes, "中文Aあ".as_bytes());
        // 4 字节码位(emoji)同样可直接编码。
        let emoji = encode_text("🎮", Codec::Utf8, None, "t", &mut report).unwrap();
        assert_eq!(emoji, "🎮".as_bytes());
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn encoding_prefers_direct_codec_then_tunnel() {
        let mut report = TranslationReport::default();
        let direct = encode_text("Aあ", Codec::Cp932, None, "t", &mut report).unwrap();
        assert_eq!(direct, b"A\x82\xa0");
        let tunnel = TunnelTable::from_pairs([([0xF0, 0x40], '这')]).unwrap();
        let mixed = encode_text("A这", Codec::Cp932, Some(&tunnel), "t", &mut report).unwrap();
        assert_eq!(mixed, b"A\xF0\x40");
    }
}
