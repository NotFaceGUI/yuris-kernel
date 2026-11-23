//! 文本 / 角色表导出(跨平台,纯数据源 + 文件系统)。
//!
//! 用途:给翻译流程与运行时文本 hook 提供**稳定坐标**的原文表。不跑 VM、
//! 不依赖渲染/桌面 API,桌面 CLI、Android 宿主与服务端都可以调用同一份。
//!
//! # 导出内容
//!
//! | 文件 | 来源 | 说明 |
//! |---|---|---|
//! | `text.csv` | `yst%05d.ybn` 的 `WORD` 载荷 | 编译进脚本的显示文本(0.495 及以前的主路径) |
//! | `literals.csv` | 脚本里的 `M` 形字符串常量 | `CGACT[TEXT=1]`/UI 等非 `WORD` 文本;排除 `es.*` 方法名 |
//! | `scenario.csv` | 555 外置剧本 `scenario/*.txt` / `sc_*_scenario/*.txt` | 台词宏(`\LC`/`\LE`/`\LT`…)、裸文本行、命令后行内文本 |
//! | `characters.csv` | 含 `es.CHAR.NAME` 结构的定义脚本(不依赖文件名) | 角色名表(按语言变体分列) |
//! | `characters_runtime.csv` | boot 后运行时 STR 数组(`--runtime-characters`) | 注册表交叉验证 |
//!
//! `--format translate-json` 是另一种输出:**只写一个 `translate.json`**,
//! 内含 `characters` 与 `text`(WORD + 外置剧本),每条带空译文槽;不写
//! `literals.csv`/`text.csv`/人读副本。
//!
//! # 坐标(给 hook 用)
//!
//! - 编译脚本:`script_id + pc`(`WORD` 每指令一个;`literals` 再加 `window + segment`)。
//! - 外置剧本:`variant + 相对路径 + 行号`。
//! - 角色表:`variant + table + 序号`(`table` 为定义脚本源路径;序号 = 文件内出现顺序)。
//!
//! # 编码
//!
//! 文本域 codec 由 `--text-codec` 决定。`auto` 先采样 `WORD`;没有 WORD 的
//! 555 框架脚本再采样基准语种剧本文本,最后才用普通脚本字符串常量。采样只收
//! 非 ASCII 占比 >=30% 的串(英文 + CP932 标点不参与,避免 GBK 靠汉字加权
//! 误胜);平局或 CP932 出现全角假名时回退 CP932。CP932 路线若工程根有
//! `sjis_ext.bin`,自动用隧道表回解扩展字。外置剧本按目录变体固定:
//! `sc_cn_` = GBK、`sc_tw_` = Big5。
//! 所有输出是 **UTF-8 + BOM**;解码失败逐字节计数并写进清单,不静默替换。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use yu_format::codec::auto;
use yu_format::{yscm, ystb, ystl};
use yu_vfs::{DataSource, EntryKind, LookupKey};

use crate::json::{self, Value};
use crate::project::{Project, CODEC};
use crate::Failure;

/// UTF-8 BOM(Windows 编辑器靠它认编码)。
const BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

/// 自动检测采样上限(与 `docs/tasks/task-p0-source-chain.md` P0 口径一致)。
const AUTO_SAMPLE_BYTES: usize = 64 * 1024;
/// 自动检测最多采样脚本数。
const AUTO_SAMPLE_SCRIPTS: usize = 4;

/// 文本域 codec 选择。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextCodecChoice {
    /// 采样 `WORD` 载荷自动裁决(CP932/GBK,平局 CP932)。
    Auto,
    /// 显式指定。
    Fixed(yu_format::Codec),
}

impl TextCodecChoice {
    /// 解析 CLI/宿主取值:`auto|cp932|gbk|big5`(`sjis` 视为 cp932)。
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "cp932" | "sjis" | "shift_jis" | "" => Ok(Self::Fixed(yu_format::Codec::Cp932)),
            "gbk" | "cp936" => Ok(Self::Fixed(yu_format::Codec::Gbk)),
            "big5" | "cp950" => Ok(Self::Fixed(yu_format::Codec::Big5)),
            other => Err(format!(
                "text-codec 只接受 auto|cp932|gbk|big5,收到 `{other}`"
            )),
        }
    }
}

/// 导出格式。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExportFormat {
    /// CSV 表 + 人读副本(默认;诊断/批量处理)。
    #[default]
    Csv,
    /// 单个 `translate.json`:只含角色表与正文,带空 `translation` 字段。
    TranslateJson,
}

impl ExportFormat {
    /// 解析 CLI/宿主取值:`csv|translate-json`(别名 `json`)。
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "csv" | "" => Ok(Self::Csv),
            "translate-json" | "translate" | "json" => Ok(Self::TranslateJson),
            other => Err(format!("format 只接受 csv|translate-json,收到 `{other}`")),
        }
    }
}

/// 导出选项。
#[derive(Clone, Debug)]
pub struct ExportOptions {
    pub text_codec: TextCodecChoice,
    /// 显式脚本密钥(8 位十六进制,已解析)。`None` = 启动探测。
    pub script_key: Option<[u8; 4]>,
    /// 是否导出脚本字符串常量(`literals.csv`)。
    pub include_literals: bool,
    /// 导出格式(`translate.json` 模式只写角色 + 正文)。
    pub format: ExportFormat,
    /// 运行时角色名验证:boot 后扫描字符串数组,输出 `characters_runtime.csv`。
    pub runtime_characters: bool,
    /// 运行时验证的 tick 上限(默认 600;角色定义通常在启动链早期完成)。
    pub runtime_ticks: u32,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            text_codec: TextCodecChoice::Auto,
            script_key: None,
            include_literals: true,
            format: ExportFormat::Csv,
            runtime_characters: false,
            runtime_ticks: 600,
        }
    }
}

/// 一次导出的结果(终端展示 + 清单同源)。
#[derive(Clone, Debug)]
pub struct ExportReport {
    pub text_codec: yu_format::Codec,
    pub text_codec_source: String,
    pub text_codec_scores: Option<(f64, f64)>,
    /// 是否加载了 `sjis_ext.bin` 并用它回解文本。
    pub text_tunnel: bool,
    pub script_key_hex: String,
    pub script_key_source: String,
    pub scripts: u32,
    pub text_units: u64,
    pub literal_units: u64,
    pub literal_skipped: u64,
    pub scenario_files: u32,
    pub scenario_units: u64,
    pub character_variants: u32,
    pub characters: u64,
    /// 运行时验证命中的角色名数组个数 / 值个数。
    pub runtime_character_slots: u32,
    pub runtime_character_values: u64,
    pub skipped: Vec<String>,
    pub warnings: Vec<String>,
}

impl Default for ExportReport {
    fn default() -> Self {
        Self {
            text_codec: yu_format::Codec::Cp932,
            text_codec_source: String::new(),
            text_codec_scores: None,
            text_tunnel: false,
            script_key_hex: String::new(),
            script_key_source: String::new(),
            scripts: 0,
            text_units: 0,
            literal_units: 0,
            literal_skipped: 0,
            scenario_files: 0,
            scenario_units: 0,
            character_variants: 0,
            characters: 0,
            runtime_character_slots: 0,
            runtime_character_values: 0,
            skipped: Vec::new(),
            warnings: Vec::new(),
        }
    }
}

/// 导出工程文本 / 角色表到 `out`。
pub fn export_project(
    project: &Project,
    out: &Path,
    options: &ExportOptions,
) -> Result<ExportReport, Failure> {
    std::fs::create_dir_all(out)
        .map_err(|e| Failure::runtime(format!("建输出目录 {} 失败: {e}", out.display())))?;

    let source = project.open_source()?;
    let key = project.resolve_script_key(options.script_key)?;
    let origin = b"yu-cli export";

    let ysc = read_required(source.as_ref(), "ysbin/ysc.ybn", origin)?;
    let table = yscm::CommandTable::parse(&ysc, origin)
        .map_err(|e| Failure::runtime(format!("ysc.ybn 解析失败: {e}")))?;
    let list_bytes = read_required(source.as_ref(), "ysbin/yst_list.ybn", origin)?;
    let list = ystl::ScriptList::parse(&list_bytes, origin)
        .map_err(|e| Failure::runtime(format!("yst_list.ybn 解析失败: {e}")))?;

    let scripts = collect_scripts(source.as_ref())?;
    let mut report = ExportReport {
        script_key_hex: key.hex(),
        script_key_source: key.source.describe(),
        ..ExportReport::default()
    };

    // ---- 文本域 codec ----
    let (codec, source_label, scores) = resolve_text_codec(
        source.as_ref(),
        &scripts,
        &table,
        &key.bytes,
        options,
        &mut report,
    )?;
    report.text_codec = codec;
    report.text_codec_source = source_label;
    report.text_codec_scores = scores;

    // SJIS 隧道(`sjis_ext.bin`)只作用于 CP932 路线:旧作把扩展字写成
    // 合法 SJIS 码位,必须查表才能还原 Unicode(如 `<v488-x>` 的人名)。
    let tunnel = if codec == yu_format::Codec::Cp932 {
        crate::patch::load_tunnel(project)?
    } else {
        None
    };
    report.text_tunnel = tunnel.is_some();
    let tunnel = tunnel.as_ref();

    // ---- 编译脚本文本 + 字符串常量 ----
    let mut text_rows: Vec<[String; 7]> = Vec::new();
    let mut literal_rows: Vec<[String; 8]> = Vec::new();
    let text_dir = out.join("text");
    if options.format == ExportFormat::Csv {
        std::fs::create_dir_all(&text_dir)
            .map_err(|e| Failure::runtime(format!("建 {} 失败: {e}", text_dir.display())))?;
    }

    for sid in &scripts {
        let name = format!("ysbin/yst{sid:05}.ybn");
        let data = match read_optional(source.as_ref(), &name) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => {
                report.skipped.push(format!("s{sid}: 包内缺 {name}"));
                continue;
            }
            Err(e) => {
                report.skipped.push(format!("s{sid}: 读 {name} 失败: {e}"));
                continue;
            }
        };
        let segs = match ystb::Segments::decrypt(&data, &key.bytes, origin) {
            Ok(segs) => segs,
            Err(e) => {
                report.skipped.push(format!("s{sid}: 解密失败: {e}"));
                continue;
            }
        };
        let script = match ystb::decode(&segs, *sid, &table, yu_format::Codec::Cp932, origin) {
            Ok(script) => script,
            Err(e) => {
                report.skipped.push(format!("s{sid}: 解码失败: {e}"));
                continue;
            }
        };
        let entry = list.entries.iter().find(|e| e.index == *sid);
        let source_path: String = entry
            .map(|e| {
                yu_format::Codec::Cp932
                    .decode_lossy(&e.name)
                    .0
                    .into_iter()
                    .collect()
            })
            .unwrap_or_default();
        report.scripts += 1;

        let mut line = 1u32;
        let mut unit = 0u32;
        let mut body = String::new();
        for (pc, group) in script.groups.iter().enumerate() {
            if group.command.as_ref() == "RETURNCODE" {
                line += 1;
            }
            if group.command.as_ref() != "WORD" {
                continue;
            }
            let Some(window) = group.windows.first() else {
                report
                    .warnings
                    .push(format!("s{sid} pc{pc}: WORD 缺参数窗口,已跳过"));
                continue;
            };
            // 空 WORD 是合法的空文本段(`len == 0`),不告警。
            if u32::from_le_bytes(window.bytes[4..8].try_into().unwrap()) == 0 {
                continue;
            }
            let Some(payload) = window_payload(&window.bytes, &segs.args_data) else {
                report
                    .warnings
                    .push(format!("s{sid} pc{pc}: WORD 载荷越界,已跳过"));
                continue;
            };
            let (text, invalid) = decode_text(codec, tunnel, payload);
            if invalid > 0 {
                report.warnings.push(format!(
                    "s{sid} pc{pc}: {} 字节在 {} 下解码失败(已按 U+FFFD 导出)",
                    invalid,
                    codec.as_str()
                ));
            }
            report.text_units += 1;
            text_rows.push([
                sid.to_string(),
                source_path.clone(),
                pc.to_string(),
                unit.to_string(),
                line.to_string(),
                codec.as_str().to_string(),
                text.clone(),
            ]);
            unit += 1;
            body.push_str(&text);
            body.push_str("\r\n");
        }
        if !body.is_empty() && options.format == ExportFormat::Csv {
            let mut file_name = format!("{sid:05}_{}", safe_name(&source_path));
            if !file_name.ends_with(".txt") {
                file_name.push_str(".txt");
            }
            write_text(&text_dir.join(file_name), &body)?;
        }

        if options.include_literals && options.format == ExportFormat::Csv {
            for (pc, group) in script.groups.iter().enumerate() {
                for (window, segment, raw) in group_literal_bytes(group, &segs.args_data) {
                    let (text, invalid) = decode_text(codec, tunnel, raw);
                    if !text_literal_ok(&text, invalid) {
                        report.literal_skipped += 1;
                        continue;
                    }
                    report.literal_units += 1;
                    literal_rows.push([
                        sid.to_string(),
                        source_path.clone(),
                        pc.to_string(),
                        group.command.to_string(),
                        window.to_string(),
                        segment.to_string(),
                        codec.as_str().to_string(),
                        text,
                    ]);
                }
            }
        }
    }

    if options.format == ExportFormat::Csv {
        write_csv(
            &out.join("text.csv"),
            &[
                "script_id",
                "script_path",
                "pc",
                "unit",
                "line",
                "codec",
                "text",
            ],
            &text_rows,
        )?;
        if options.include_literals {
            write_csv(
                &out.join("literals.csv"),
                &[
                    "script_id",
                    "script_path",
                    "pc",
                    "command",
                    "window",
                    "segment",
                    "codec",
                    "text",
                ],
                &literal_rows,
            )?;
        }
    }

    // ---- 555 外置剧本 ----
    // 输出目录若在工程内(缺省 `<root>/export-text`),递归找剧本目录时必须
    // 跳过它,否则第二次运行会把上次导出的 `scenario/` 当成源数据。
    let skip_prefix = output_rel_prefix(project, out);
    let scenario_rows = export_scenarios(
        source.as_ref(),
        out,
        codec,
        tunnel,
        skip_prefix.as_deref(),
        options.format,
        &mut report,
    )?;

    // ---- 角色表 ----
    let character_rows = export_characters(
        source.as_ref(),
        &scripts,
        &table,
        &key.bytes,
        codec,
        tunnel,
        options.format,
        out,
        &mut report,
    )?;
    if options.runtime_characters && options.format == ExportFormat::Csv {
        if let Err(e) = export_runtime_characters(
            project,
            &key.bytes,
            codec,
            tunnel,
            &character_rows,
            options.runtime_ticks,
            out,
            &mut report,
        ) {
            report
                .warnings
                .push(format!("runtime-characters: {e}(静态角色表仍已导出)"));
        }
    }

    // ---- 清单 ----
    if options.format == ExportFormat::TranslateJson {
        write_translate_json(
            project,
            out,
            codec,
            &report,
            &text_rows,
            &scenario_rows,
            &character_rows,
        )?;
        return Ok(report);
    }
    write_manifest(project, out, options, &report)?;
    Ok(report)
}

// ------------------------------------------------------------------ 数据源

fn read_required(source: &dyn DataSource, path: &str, _origin: &[u8]) -> Result<Vec<u8>, Failure> {
    let key = LookupKey::new(path.as_bytes(), CODEC)
        .map_err(|e| Failure::runtime(format!("查找键 {path} 非法: {e}")))?;
    source
        .open(&key)
        .map_err(|e| Failure::missing(format!("数据源缺少 {path}: {e}")))
}

fn read_optional(source: &dyn DataSource, path: &str) -> Result<Option<Vec<u8>>, Failure> {
    let key = LookupKey::new(path.as_bytes(), CODEC)
        .map_err(|e| Failure::runtime(format!("查找键 {path} 非法: {e}")))?;
    match source.open(&key) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(diag) if diag.is_missing() => Ok(None),
        Err(diag) => Err(Failure::runtime(format!("读 {path} 失败: {diag}"))),
    }
}

fn collect_scripts(source: &dyn DataSource) -> Result<Vec<u32>, Failure> {
    let key = LookupKey::new(b"ysbin", CODEC)
        .map_err(|e| Failure::runtime(format!("ysbin 查找键非法: {e}")))?;
    let entries = source
        .list(&key)
        .map_err(|e| Failure::runtime(format!("列 ysbin 失败: {e}")))?;
    let mut ids = Vec::new();
    for entry in entries {
        if !matches!(entry.kind, EntryKind::File) {
            continue;
        }
        let name = yu_format::Codec::Cp932.decode_lossy(&entry.name).0;
        let name: String = name.into_iter().collect();
        let Some(digits) = name
            .strip_prefix("yst")
            .and_then(|rest| rest.strip_suffix(".ybn"))
        else {
            continue;
        };
        if digits.len() == 5 && digits.bytes().all(|b| b.is_ascii_digit()) {
            if let Ok(id) = digits.parse::<u32>() {
                ids.push(id);
            }
        }
    }
    ids.sort_unstable();
    ids.dedup();
    Ok(ids)
}

// ------------------------------------------------------------------ codec

type TextCodecDecision = (yu_format::Codec, String, Option<(f64, f64)>);

fn resolve_text_codec(
    source: &dyn DataSource,
    scripts: &[u32],
    table: &yscm::CommandTable,
    key: &[u8; 4],
    options: &ExportOptions,
    report: &mut ExportReport,
) -> Result<TextCodecDecision, Failure> {
    if let TextCodecChoice::Fixed(codec) = options.text_codec {
        return Ok((codec, "cli".into(), None));
    }
    let origin = b"yu-cli export codec";
    let mut samples: Vec<Vec<u8>> = Vec::new();
    let mut bytes = 0usize;

    // 1) WORD 载荷(0.495 及以前的显示文本主路径)。
    for sid in scripts.iter().take(AUTO_SAMPLE_SCRIPTS) {
        let name = format!("ysbin/yst{sid:05}.ybn");
        let Ok(Some(data)) = read_optional(source, &name) else {
            continue;
        };
        let Ok(segs) = ystb::Segments::decrypt(&data, key, origin) else {
            continue;
        };
        let Ok(script) = ystb::decode(&segs, *sid, table, yu_format::Codec::Cp932, origin) else {
            continue;
        };
        for group in &script.groups {
            if group.command.as_ref() != "WORD" {
                continue;
            }
            let Some(payload) = word_payload(group, &segs.args_data) else {
                continue;
            };
            if !is_text_sample(payload) {
                continue;
            }
            samples.push(payload.to_vec());
            bytes += payload.len();
            if bytes >= AUTO_SAMPLE_BYTES {
                break;
            }
        }
        if bytes >= AUTO_SAMPLE_BYTES {
            break;
        }
    }

    // 2) 555 外置剧本(基准语种):框架脚本没有 WORD,剧情才是显示文本主体。
    if !samples_have_text(&samples) {
        for sample in scenario_text_samples(source)? {
            bytes += sample.len();
            samples.push(sample);
            if bytes >= AUTO_SAMPLE_BYTES {
                break;
            }
        }
    }

    // 4) 最后才用普通脚本字符串常量;只取含非 ASCII 的,避免 ASCII 路径/方法名
    //    把两种 codec 的得分都压在 valid_ratio 上。
    if !samples_have_text(&samples) {
        'scripts: for sid in scripts.iter().take(128) {
            let name = format!("ysbin/yst{sid:05}.ybn");
            let Ok(Some(data)) = read_optional(source, &name) else {
                continue;
            };
            let Ok(segs) = ystb::Segments::decrypt(&data, key, origin) else {
                continue;
            };
            let Ok(script) = ystb::decode(&segs, *sid, table, yu_format::Codec::Cp932, origin)
            else {
                continue;
            };
            for group in &script.groups {
                for (_, _, literal) in group_literal_bytes(group, &segs.args_data) {
                    if looks_binary(literal) || !is_text_sample(literal) {
                        continue;
                    }
                    samples.push(literal.to_vec());
                    bytes += literal.len();
                    if bytes >= AUTO_SAMPLE_BYTES {
                        break 'scripts;
                    }
                }
            }
        }
    }

    if samples.is_empty() {
        report
            .warnings
            .push("codec.auto.no_samples: 没有可用文本样本,回退 CP932".into());
        return Ok((yu_format::Codec::Cp932, "default".into(), None));
    }
    let refs: Vec<&[u8]> = samples.iter().map(Vec::as_slice).collect();
    let detection = auto::detect(&refs);
    let mut codec = detection.codec;
    // 安全网:CP932 解码出现可观的全角假名时,文本就是日文;GBK 靠汉字加权
    // 险胜也只可能是标点/半角误读,不采用。
    let kana_ratio = detection.cp932.kana as f64 / detection.cp932.total_bytes.max(1) as f64;
    if codec != yu_format::Codec::Cp932 && kana_ratio >= 0.10 {
        codec = yu_format::Codec::Cp932;
    }
    Ok((
        codec,
        "auto".into(),
        Some((detection.cp932.points, detection.gbk.points)),
    ))
}

/// 只把“真正的非 ASCII 文本”纳入 codec 裁决。
///
/// 英文行里夹几个 CP932 标点(【】“”)时,GBK 会把 0x817A 解成汉字并凭
/// han 加权误胜;要求非 ASCII 字节 >=4 且占比 >=30%,双字节正文自然满足,
/// 英文句子 + 少量标点则被排除。
fn is_text_sample(bytes: &[u8]) -> bool {
    let high = bytes.iter().filter(|b| **b >= 0x80).count();
    high >= 4 && high * 100 >= bytes.len() * 30
}

fn samples_have_text(samples: &[Vec<u8>]) -> bool {
    samples
        .iter()
        .any(|sample| sample.iter().any(|b| *b >= 0x80))
}

fn trim_ascii(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map(|i| i + 1)
        .unwrap_or(start);
    &bytes[start..end]
}

/// 基准语种外置剧本里的行内文本(粗粒度、按 ASCII 语法切)。
fn scenario_text_samples(source: &dyn DataSource) -> Result<Vec<Vec<u8>>, Failure> {
    let mut dirs = Vec::new();
    find_scenario_dirs(source, &[], "", 0, None, &mut dirs)?;
    let mut out = Vec::new();
    let mut bytes = 0usize;
    let mut files_seen = 0usize;
    for (display, dir_bytes) in dirs {
        if scenario_variant(&display, yu_format::Codec::Cp932).0 != "base" {
            continue;
        }
        let mut files = Vec::new();
        walk_files(source, &dir_bytes, &display, 0, &mut files)?;
        for (_, name_bytes) in files {
            if files_seen >= 16 {
                return Ok(out);
            }
            files_seen += 1;
            let Ok(key) = LookupKey::new(&name_bytes, CODEC) else {
                continue;
            };
            let Ok(data) = source.open(&key) else {
                continue;
            };
            for raw in data.split(|b| *b == b'\n') {
                let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
                let trimmed = trim_ascii(raw);
                if trimmed.is_empty()
                    || trimmed[0] == b'#'
                    || trimmed.starts_with(b"//")
                    || trimmed.starts_with(b"/*")
                    || trimmed.starts_with(b"*/")
                {
                    continue;
                }
                let text = if trimmed[0] == 0x5C {
                    let Some(close) = trimmed.iter().rposition(|b| *b == b')') else {
                        continue;
                    };
                    trim_ascii(&trimmed[close + 1..])
                } else {
                    trimmed
                };
                if is_text_sample(text) {
                    out.push(text.to_vec());
                    bytes += text.len();
                    if bytes >= AUTO_SAMPLE_BYTES {
                        return Ok(out);
                    }
                }
            }
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------ 文本单元

/// `WORD` 首窗口的载荷切片(`{len, off}` 在窗口 `+4/+8`)。
fn word_payload<'a>(group: &yu_format::Group, args: &'a [u8]) -> Option<&'a [u8]> {
    let window = group.windows.first()?;
    window_payload(&window.bytes, args)
}

fn window_payload<'a>(window: &[u8; 12], args: &'a [u8]) -> Option<&'a [u8]> {
    let len = u32::from_le_bytes(window[4..8].try_into().ok()?) as usize;
    let off = u32::from_le_bytes(window[8..12].try_into().ok()?) as usize;
    if len == 0 {
        return None;
    }
    args.get(off..off.checked_add(len)?)
}

/// 脚本里的 `M` 形字符串常量的**原始字节**(未解码)。
///
/// 一个载荷里可能相邻存放多个引号串(`"name""annotation"`);逐段返回,
/// 不把中间的引号当成内容(RxYuris 同样按引号段切)。
fn group_literal_bytes<'a>(
    group: &yu_format::Group,
    args: &'a [u8],
) -> Vec<(usize, usize, &'a [u8])> {
    let mut out = Vec::new();
    for (window_index, window) in group.windows.iter().enumerate() {
        let Some(payload) = window_payload(&window.bytes, args) else {
            continue;
        };
        if payload.first() != Some(&b'M') {
            continue;
        }
        for (segment, bytes) in quoted_segments(payload).into_iter().enumerate() {
            out.push((window_index, segment, bytes));
        }
    }
    out
}

/// 按 `"` 段切分一个 `M` 形载荷(只处理 `\"` 转义)。
fn quoted_segments(payload: &[u8]) -> Vec<&[u8]> {
    // YU-RIS 的字符串常量有三种定界符:`"…"`、反引号 `` `…` `` 与 `'…'`
    // (旧作的人名/显示名常用后两种;vnforge `ExpressionStrings` 同样三种都收)。
    const DELIMS: [u8; 3] = [0x22, 0x60, 0x27];
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < payload.len() {
        let Some(open) = payload[i..].iter().position(|b| DELIMS.contains(b)) else {
            break;
        };
        let open = i + open;
        let delim = payload[open];
        let mut j = open + 1;
        let mut close = None;
        while j < payload.len() {
            if payload[j] == 0x5C && payload.get(j + 1) == Some(&delim) {
                j += 2;
                continue;
            }
            if payload[j] == delim {
                close = Some(j);
                break;
            }
            j += 1;
        }
        let Some(close) = close else {
            break;
        };
        out.push(&payload[open + 1..close]);
        i = close + 1;
    }
    out
}

/// 脚本里的 `M` 形字符串常量(`'M' + u16 长度 + "…"`)。
fn group_literals(
    group: &yu_format::Group,
    args: &[u8],
    codec: yu_format::Codec,
    tunnel: Option<&yu_format::text::TunnelTable>,
) -> Vec<String> {
    group_literal_bytes(group, args)
        .into_iter()
        .map(|(_, _, raw)| decode_text(codec, tunnel, raw).0)
        .collect()
}

fn decode_text(
    codec: yu_format::Codec,
    tunnel: Option<&yu_format::text::TunnelTable>,
    bytes: &[u8],
) -> (String, usize) {
    let (chars, invalid) = match tunnel {
        Some(tunnel) => yu_format::TextCodec::with_tunnel(codec, tunnel).decode_lossy(bytes),
        None => codec.decode_lossy(bytes),
    };
    (chars.into_iter().collect(), invalid)
}

/// 字面量是否像“可翻译文本”:解码全成功、无控制字符、不是资源路径/方法名。
fn text_literal_ok(text: &str, invalid: usize) -> bool {
    if invalid > 0 || text.is_empty() || text.starts_with("es.") {
        return false;
    }
    // 单个 ASCII 标点(`+`/`*`/`-`)是运算符,不是显示文本。
    if text.len() == 1 && text.is_ascii() && !text.chars().next().unwrap().is_ascii_alphanumeric() {
        return false;
    }
    if text
        .chars()
        .any(|c| c.is_control() && c != '\t' && c != '\r' && c != '\n')
    {
        return false;
    }
    // 资源名(`bgm\...`、`cg/...`)不是译文,排除;对话里极少出现反斜杠。
    if text.contains('\u{5C}') || text.contains('/') {
        return false;
    }
    // 全大写点分标识符是标签/方法名(ES.GAMEMAIN.BTLOOP),不是显示文本。
    if text.contains('.')
        && text
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_' || c == '.')
    {
        return false;
    }
    true
}

/// 自动识别采样前的二进制过滤(控制字节/超长伪字符串)。
fn looks_binary(bytes: &[u8]) -> bool {
    bytes.len() > 4096
        || bytes
            .iter()
            .any(|b| *b < 0x20 && *b != b'\t' && *b != b'\r' && *b != b'\n')
}

// ------------------------------------------------------------------ 外置剧本

fn export_scenarios(
    source: &dyn DataSource,
    out: &Path,
    fallback_codec: yu_format::Codec,
    tunnel: Option<&yu_format::text::TunnelTable>,
    skip_prefix: Option<&[u8]>,
    format: ExportFormat,
    report: &mut ExportReport,
) -> Result<Vec<[String; 10]>, Failure> {
    let mut scenario_dirs: Vec<(String, Vec<u8>)> = Vec::new();
    find_scenario_dirs(source, &[], "", 0, skip_prefix, &mut scenario_dirs)?;
    let mut rows: Vec<[String; 10]> = Vec::new();
    let scenario_dir = out.join("scenario");
    for (dir_name, dir_bytes) in scenario_dirs {
        let (variant, variant_codec) = scenario_variant(&dir_name, fallback_codec);
        // 隧道只针对 CP932;cn/tw 变体各用 GBK/Big5,不查表。
        let file_tunnel = if variant_codec == yu_format::Codec::Cp932 {
            tunnel
        } else {
            None
        };
        let mut files = Vec::new();
        walk_files(source, &dir_bytes, &dir_name, 0, &mut files)?;
        for (rel_path, name_bytes) in files {
            let key = LookupKey::new(&name_bytes, CODEC)
                .map_err(|e| Failure::runtime(format!("剧本键非法: {e}")))?;
            let bytes = match source.open(&key) {
                Ok(bytes) => bytes,
                Err(diag) => {
                    report.skipped.push(format!("{rel_path}: 读取失败 {diag}"));
                    continue;
                }
            };
            report.scenario_files += 1;
            let mut file_rows = 0usize;
            let file_lines = extract_scenario_lines(
                &bytes,
                variant_codec,
                file_tunnel,
                &rel_path,
                &mut report.warnings,
            );
            let mut decoded = String::new();
            for (line_no, seq, scene, label, kind, macro_name, text) in &file_lines {
                rows.push([
                    variant.clone(),
                    rel_path.clone(),
                    line_no.to_string(),
                    seq.to_string(),
                    kind.clone(),
                    macro_name.clone(),
                    scene.clone(),
                    label.clone(),
                    variant_codec.as_str().to_string(),
                    text.clone(),
                ]);
                report.scenario_units += 1;
                file_rows += 1;
            }
            if file_rows > 0 && format == ExportFormat::Csv {
                // 人读副本:整文件按本变体 codec 解码成 UTF-8,保留原行结构。
                let (file_text, invalid) = decode_text(variant_codec, file_tunnel, &bytes);
                if invalid > 0 {
                    report.warnings.push(format!(
                        "{rel_path}: {} 字节在 {} 下解码失败(人读副本含 U+FFFD;CSV 只含台词行)",
                        invalid,
                        variant_codec.as_str()
                    ));
                }
                decoded.push_str(&file_text);
                let dest = scenario_dir
                    .join(&variant)
                    .join(rel_path.replace('\\', "/"));
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        Failure::runtime(format!("建 {} 失败: {e}", parent.display()))
                    })?;
                }
                write_text(&dest, &decoded)?;
            }
        }
    }
    if !rows.is_empty() && format == ExportFormat::Csv {
        write_csv(
            &out.join("scenario.csv"),
            &[
                "variant", "file", "line", "seq", "kind", "macro", "scene", "label", "codec",
                "text",
            ],
            &rows,
        )?;
    }
    Ok(rows)
}

fn is_scenario_dir(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower == "scenario" || lower.ends_with("_scenario")
}

fn scenario_variant(dir: &str, fallback: yu_format::Codec) -> (String, yu_format::Codec) {
    // 目录可能是嵌套路径(`data/scenario`);变体只看最后一级。
    let leaf = dir.rsplit('/').next().unwrap_or(dir);
    let lower = leaf.to_ascii_lowercase();
    if lower == "scenario" {
        ("base".into(), fallback)
    } else if lower.contains("cn") || lower.contains("chs") {
        ("cn".into(), yu_format::Codec::Gbk)
    } else if lower.contains("tw") || lower.contains("cht") {
        ("tw".into(), yu_format::Codec::Big5)
    } else {
        let variant = lower.trim_end_matches("_scenario").to_string();
        (variant, fallback)
    }
}

/// 输出目录相对工程根的路径(在工程内才返回)。
fn output_rel_prefix(project: &Project, out: &Path) -> Option<Vec<u8>> {
    let out_abs = std::fs::canonicalize(out).ok()?;
    for base in [project.data_root(), project.root()] {
        let Ok(base_abs) = std::fs::canonicalize(base) else {
            continue;
        };
        if let Ok(rel) = out_abs.strip_prefix(&base_abs) {
            let text = rel.to_string_lossy().replace('\\', "/");
            if !text.is_empty() {
                return Some(text.into_bytes());
            }
        }
    }
    None
}

/// 递归查找剧本目录(`scenario` / `*_scenario`),返回 `(显示路径, 原始路径)`。
fn find_scenario_dirs(
    source: &dyn DataSource,
    dir_bytes: &[u8],
    display: &str,
    depth: usize,
    skip_prefix: Option<&[u8]>,
    out: &mut Vec<(String, Vec<u8>)>,
) -> Result<(), Failure> {
    if depth > 3 {
        return Ok(());
    }
    let key = if dir_bytes.is_empty() {
        LookupKey::root()
    } else {
        LookupKey::new(dir_bytes, CODEC)
            .map_err(|e| Failure::runtime(format!("目录键非法: {e}")))?
    };
    let entries = match source.list(&key) {
        Ok(entries) => entries,
        Err(diag) if diag.is_missing() => return Ok(()),
        Err(diag) => return Err(Failure::runtime(format!("列 {display} 失败: {diag}"))),
    };
    for entry in entries {
        if !matches!(entry.kind, EntryKind::Dir) {
            continue;
        }
        let name = display_name(&entry.name);
        let mut child = dir_bytes.to_vec();
        if !child.is_empty() {
            child.push(b'/');
        }
        child.extend_from_slice(&entry.name);
        let child_display = if display.is_empty() {
            name.clone()
        } else {
            format!("{display}/{name}")
        };
        if let Some(skip) = skip_prefix {
            let inside = child.starts_with(skip)
                && (child.len() == skip.len() || child.get(skip.len()) == Some(&b'/'));
            if inside {
                continue;
            }
        }
        // 上次导出的目录(含 manifest.json)不是源数据:即使本次输出到别处,
        // 也要跳过,避免把 `export-text/scenario/` 当剧本二次导出。
        let mut marker = child.clone();
        marker.extend_from_slice(b"/manifest.json");
        if LookupKey::new(&marker, CODEC)
            .map(|key| source.exists(&key))
            .unwrap_or(false)
        {
            continue;
        }
        if is_scenario_dir(&name) {
            out.push((child_display, child));
        } else {
            find_scenario_dirs(source, &child, &child_display, depth + 1, skip_prefix, out)?;
        }
    }
    Ok(())
}

/// 递归收集目录下的文件(保留相对路径与原始字节路径)。
fn walk_files(
    source: &dyn DataSource,
    dir_bytes: &[u8],
    dir_display: &str,
    depth: usize,
    out: &mut Vec<(String, Vec<u8>)>,
) -> Result<(), Failure> {
    if depth > 3 {
        return Ok(());
    }
    let key = LookupKey::new(dir_bytes, CODEC)
        .map_err(|e| Failure::runtime(format!("目录键非法: {e}")))?;
    let entries = match source.list(&key) {
        Ok(entries) => entries,
        Err(diag) if diag.is_missing() => return Ok(()),
        Err(diag) => return Err(Failure::runtime(format!("列 {dir_display} 失败: {diag}"))),
    };
    for entry in entries {
        let mut child = dir_bytes.to_vec();
        child.push(b'/');
        child.extend_from_slice(&entry.name);
        let child_display = format!("{dir_display}/{}", display_name(&entry.name));
        match entry.kind {
            EntryKind::Dir => walk_files(source, &child, &child_display, depth + 1, out)?,
            EntryKind::File => {
                let lower = child_display.to_ascii_lowercase();
                if lower.ends_with(".txt") {
                    out.push((child_display, child));
                }
            }
        }
    }
    Ok(())
}

fn display_name(bytes: &[u8]) -> String {
    yu_format::Codec::Cp932
        .decode_lossy(bytes)
        .0
        .into_iter()
        .collect()
}

/// 从外置剧本字节提取台词。
///
/// 返回 `(行号, 场景, 最近标签, kind, 宏/前导命令, 文本)`:
/// - `kind = "macro"`: `\LC("…")` 等台词宏;
/// - `kind = "text"`: 裸文本行,或命令后的行内文本(`\VO(id)【名字】「…」`)。
#[allow(unused_assignments)]
fn extract_scenario_lines(
    bytes: &[u8],
    codec: yu_format::Codec,
    tunnel: Option<&yu_format::text::TunnelTable>,
    file: &str,
    warnings: &mut Vec<String>,
) -> Vec<(u32, u32, String, String, String, String, String)> {
    let mut out = Vec::new();
    let mut in_block = false;
    let mut scene = String::new();
    let mut line_no = 0u32;
    let mut label = String::new();
    for raw in bytes.split(|b| *b == b'\n') {
        line_no += 1;
        let mut line_seq = 0u32;
        let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
        let (line, invalid) = decode_text(codec, tunnel, raw);
        let cleaned = clean_line(&line, &mut in_block);
        if let Some(rest) = cleaned.trim_start().strip_prefix('#') {
            let name = rest.trim().to_string();
            if scene.is_empty() {
                scene = name.clone();
            }
            label = name;
        }
        let mut line_units = 0usize;
        for (macro_name, text) in extract_macros(&cleaned) {
            let upper = macro_name.to_ascii_uppercase();
            if !(upper.starts_with('L') && upper.len() <= 3) {
                continue;
            }
            out.push((
                line_no,
                line_seq,
                scene.clone(),
                label.clone(),
                "macro".to_string(),
                upper,
                text,
            ));
            line_seq += 1;
            line_units += 1;
        }
        if line_units == 0 {
            let trimmed = cleaned.trim();
            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                let (command, text) = split_leading_command(trimmed);
                if let Some(text) = text {
                    out.push((
                        line_no,
                        line_seq,
                        scene.clone(),
                        label.clone(),
                        "text".to_string(),
                        command.unwrap_or_else(|| "-".to_string()),
                        text,
                    ));
                    line_seq += 1;
                    line_units += 1;
                }
            }
        }
        if line_units > 0 && invalid > 0 {
            warnings.push(format!(
                "{file} line {line_no}: {} 字节在 {} 下解码失败(该行含台词)",
                invalid,
                codec.as_str()
            ));
        }
    }
    out
}

/// 去掉前导 `\命令(...)` 链,返回 `(首个命令名, 剩余文本)`。
///
/// 剧本里既有纯命令行(`\BGM(x)`),也有「命令 + 行内台词」
/// (`\VO(x)【名字】「…」`);后者把命令之后的内容当台词。
fn split_leading_command(line: &str) -> (Option<String>, Option<String>) {
    let mut chars: Vec<char> = line.chars().collect();
    let mut command = None;
    loop {
        let trimmed: String = chars.iter().skip_while(|c| c.is_whitespace()).collect();
        if !trimmed.starts_with('\u{5C}') {
            let text = trimmed.trim().to_string();
            return (command, (!text.is_empty()).then_some(text));
        }
        let Some((name, end)) = leading_command_end(&trimmed) else {
            return (command, None);
        };
        if command.is_none() {
            command = Some(name);
        }
        chars = trimmed.chars().skip(end).collect();
        if chars.iter().all(|c| c.is_whitespace()) {
            return (command, None);
        }
    }
}

/// 解析 `\NAME(...)` 的结束位置(字符下标;括号按嵌套与引号处理)。
fn leading_command_end(line: &str) -> Option<(String, usize)> {
    let chars: Vec<char> = line.chars().collect();
    if chars.first() != Some(&'\u{5C}') {
        return None;
    }
    let mut i = 1usize;
    let mut name = String::new();
    while i < chars.len()
        && (chars[i].is_ascii_alphanumeric() || chars[i] == '_' || chars[i] == '.')
    {
        name.push(chars[i]);
        i += 1;
    }
    if name.is_empty() {
        return None;
    }
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    if chars.get(i) != Some(&'(') {
        return Some((name, i));
    }
    let mut depth = 0i32;
    let mut in_string = false;
    let mut j = i;
    while j < chars.len() {
        let c = chars[j];
        if in_string {
            match c {
                '\u{5C}' => {
                    j += 2;
                    continue;
                }
                '"' => in_string = false,
                _ => {}
            }
            j += 1;
            continue;
        }
        match c {
            '"' => in_string = true,
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some((name, j + 1));
                }
            }
            _ => {}
        }
        j += 1;
    }
    Some((name, chars.len()))
}

/// 去掉块注释与行注释(引号内的 `//`、`/*` 保留)。
fn clean_line(line: &str, in_block: &mut bool) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let mut i = 0usize;
    let mut in_string = false;
    while i < chars.len() {
        let c = chars[i];
        if *in_block {
            if c == '*' && chars.get(i + 1) == Some(&'/') {
                *in_block = false;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        if in_string {
            out.push(c);
            if c == '\\' {
                if let Some(next) = chars.get(i + 1) {
                    out.push(*next);
                }
                i += 2;
                continue;
            }
            if c == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'*') {
            *in_block = true;
            i += 2;
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'/') {
            break;
        }
        if c == '"' {
            in_string = true;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// `\NAME("…")` → `(NAME, 文本)`;支持 `\"` / `\\` 转义。
fn extract_macros(line: &str) -> Vec<(String, String)> {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] != '\\' {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        let mut name = String::new();
        while j < chars.len() && chars[j].is_ascii_alphabetic() && name.len() < 8 {
            name.push(chars[j]);
            j += 1;
        }
        let mut k = j;
        while k < chars.len() && chars[k].is_whitespace() {
            k += 1;
        }
        if !name.is_empty() && chars.get(k) == Some(&'(') {
            let mut m = k + 1;
            while m < chars.len() && chars[m].is_whitespace() {
                m += 1;
            }
            if chars.get(m) == Some(&'"') {
                let mut text = String::new();
                let mut n = m + 1;
                let mut closed = false;
                while n < chars.len() {
                    let c = chars[n];
                    if c == '\\' {
                        match chars.get(n + 1) {
                            Some('"') => text.push('"'),
                            Some('\\') => text.push('\\'),
                            Some(other) => {
                                text.push('\\');
                                text.push(*other);
                            }
                            None => text.push('\\'),
                        }
                        n += 2;
                        continue;
                    }
                    if c == '"' {
                        closed = true;
                        n += 1;
                        break;
                    }
                    text.push(c);
                    n += 1;
                }
                if closed {
                    out.push((name, text));
                    i = n;
                    continue;
                }
            }
        }
        i = j.max(i + 1);
    }
    out
}

// ------------------------------------------------------------------ 角色表

#[allow(clippy::too_many_arguments)]
fn export_characters(
    source: &dyn DataSource,
    scripts: &[u32],
    table: &yscm::CommandTable,
    key: &[u8; 4],
    fallback_codec: yu_format::Codec,
    tunnel: Option<&yu_format::text::TunnelTable>,
    format: ExportFormat,
    out: &Path,
    report: &mut ExportReport,
) -> Result<Vec<[String; 8]>, Failure> {
    let origin = b"yu-cli export characters";
    let mut all_rows: Vec<[String; 8]> = Vec::new();
    let mut marks: Vec<[String; 5]> = Vec::new();
    let mut variants: Vec<String> = Vec::new();

    // 1) 散盘/包内的纯文本定义(部分工程把 キャラ名定義 当文本资源)。
    for (path, variant, codec) in [
        (
            "data/script/userdefine/キャラ名定義.txt",
            "base",
            fallback_codec,
        ),
        (
            "data/script/userdefine/キャラ名定義_cn.txt",
            "cn",
            yu_format::Codec::Gbk,
        ),
        (
            "data/script/userdefine/キャラ名定義_tw.txt",
            "tw",
            yu_format::Codec::Big5,
        ),
    ] {
        let Ok(key_bytes) = LookupKey::new(path.as_bytes(), CODEC) else {
            continue;
        };
        if !source.exists(&key_bytes) {
            continue;
        }
        let bytes = source
            .open(&key_bytes)
            .map_err(|e| Failure::runtime(format!("读 {path} 失败: {e}")))?;
        let mut rows = Vec::new();
        for (index, raw) in bytes.split(|b| *b == b'\n').enumerate() {
            let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
            if raw.is_empty() {
                continue;
            }
            let plain_tunnel = if codec == yu_format::Codec::Cp932 {
                tunnel
            } else {
                None
            };
            let (text, _) = decode_text(codec, plain_tunnel, raw);
            rows.push([
                variant.to_string(),
                path.to_string(),
                index.to_string(),
                text.clone(),
                text,
                "-1".into(),
                "-".into(),
                codec.as_str().to_string(),
            ]);
        }
        if !rows.is_empty() {
            variants.push(variant.to_string());
            all_rows.extend(rows);
        }
    }

    // 2) 编译脚本:按**结构**识别 `es.CHAR.NAME` 调用,不依赖文件名。
    //
    //    不同作品把定义表放在 `userdefine/キャラ名定義.txt`、`userdefine/jp_0002.txt`
    //    等不同路径(ERIS demo 就是后者);文件名规则会漏。候选范围先按路径
    //    收窄(`userdefine` / `char`)避免全量重解码,再用结构确认。
    let Some(list_bytes) = read_optional(source, "ysbin/yst_list.ybn")? else {
        write_character_outputs(out, &all_rows, &marks, report, format == ExportFormat::Csv)?;
        return Ok(all_rows);
    };
    let list = ystl::ScriptList::parse(&list_bytes, origin)
        .map_err(|e| Failure::runtime(format!("yst_list.ybn 解析失败: {e}")))?;
    for entry in &list.entries {
        let path: String = yu_format::Codec::Cp932
            .decode_lossy(&entry.name)
            .0
            .into_iter()
            .collect();
        let lower = path.to_ascii_lowercase();
        // `es_charname.yst` 是框架函数(名字表由调用方传入),结构上也没有
        // display/name 两段;显式排除避免误报。其余脚本全量过结构检查:
        // 定义表可能放在任意路径(不同作品命名差异很大)。
        if lower.contains("es_charname") {
            continue;
        }
        if !scripts.contains(&entry.index) {
            continue;
        }
        let (variant, codec) = character_variant(&path, fallback_codec);
        let name = format!("ysbin/yst{:05}.ybn", entry.index);
        let Some(data) = read_optional(source, &name)? else {
            report
                .skipped
                .push(format!("角色表 s{}: 包内缺 {name}", entry.index));
            continue;
        };
        let segs = match ystb::Segments::decrypt(&data, key, origin) {
            Ok(segs) => segs,
            Err(e) => {
                report
                    .skipped
                    .push(format!("角色表 s{}: 解密失败 {e}", entry.index));
                continue;
            }
        };
        let script = match ystb::decode(&segs, entry.index, table, yu_format::Codec::Cp932, origin)
        {
            Ok(script) => script,
            Err(e) => {
                report
                    .skipped
                    .push(format!("角色表 s{}: 解码失败 {e}", entry.index));
                continue;
            }
        };
        let file_tunnel = if codec == yu_format::Codec::Cp932 {
            tunnel
        } else {
            None
        };
        let (rows, variant_marks) = extract_character_rows(
            &script,
            &segs.args_data,
            &variant,
            &path,
            codec,
            file_tunnel,
            entry.index,
        );
        if rows.is_empty() {
            continue;
        }
        if !variants.contains(&variant) {
            variants.push(variant.clone());
        }
        all_rows.extend(rows);
        marks.extend(variant_marks);
    }

    if all_rows.is_empty() {
        report.warnings.push(
            "未找到角色表(结构化 es.CHAR.NAME / キャラ名定義);若本作没有角色名表属正常".into(),
        );
    }
    write_character_outputs(out, &all_rows, &marks, report, format == ExportFormat::Csv)?;
    report.character_variants = variants.len() as u32;
    Ok(all_rows)
}

/// 运行时角色名验证:boot → 跑启动链 → 扫描字符串数组,按静态角色名匹配注册表。
///
/// 证据(2026-11-23,`<v488-x>`):角色定义脚本执行后,引擎把名字写进
/// STR 数组(`$1671` 显示名 / `$1672` 本名,`[0]` 为空,长度 = 定义数),
/// 之后再说话时按编号读。静态字面量 + 隧道表已能拿到同一份数据;这里用
/// 运行时值做交叉验证,并为“名字由变量拼装”的作品保留通用通道。
#[allow(clippy::too_many_arguments)]
fn export_runtime_characters(
    project: &Project,
    key: &[u8; 4],
    codec: yu_format::Codec,
    tunnel: Option<&yu_format::text::TunnelTable>,
    static_rows: &[[String; 8]],
    ticks: u32,
    out: &Path,
    report: &mut ExportReport,
) -> Result<(), Failure> {
    use yu_rt::input::InputTimeline;
    use yu_rt::value::Value;
    use yu_rt::{BootConfig, Vm};

    let names: std::collections::BTreeSet<&str> = static_rows
        .iter()
        .flat_map(|row| [row[3].as_str(), row[4].as_str()])
        .filter(|name| !name.is_empty())
        .collect();
    if names.is_empty() {
        report
            .warnings
            .push("runtime-characters: 静态角色表为空,跳过运行时匹配".into());
        return Ok(());
    }

    let source = project.open_source()?;
    let aux = project.open_aux()?;
    let mut vm = Vm::boot_with_aux(
        source,
        Some(aux),
        BootConfig {
            entry_label: "SYSTEM_START".into(),
            script_key: *key,
            codecs: yu_rt::CodecConfig {
                script: codec,
                text: codec,
                name: yu_format::Codec::Cp932,
            },
            ..BootConfig::default()
        },
    )
    .map_err(|e| Failure::runtime(format!("boot 失败: {e}")))?;
    for frame in 0..ticks.max(1) {
        let _ = vm.tick_at(frame, &InputTimeline::new());
    }

    let mut rows: Vec<[String; 4]> = Vec::new();
    let mut slots = 0u32;
    for (var, slot) in vm.store().iter() {
        let mut values = Vec::with_capacity(slot.data.len());
        for value in &slot.data {
            let Value::Str(bytes) = value else {
                values.push(String::new());
                continue;
            };
            values.push(decode_text(codec, tunnel, bytes).0);
        }
        let matched: Vec<(usize, &String)> = values
            .iter()
            .enumerate()
            .filter(|(_, text)| !text.is_empty() && names.contains(text.as_str()))
            .collect();
        if matched.len() < 2 {
            continue;
        }
        slots += 1;
        for (index, text) in matched {
            rows.push([
                var.id.to_string(),
                index.to_string(),
                text.clone(),
                var.display(),
            ]);
        }
    }
    if rows.is_empty() {
        report.warnings.push(format!(
            "runtime-characters: boot {ticks} 帧后未找到匹配数组(定义脚本可能未执行或名字为动态拼装)"
        ));
        return Ok(());
    }
    write_csv(
        &out.join("characters_runtime.csv"),
        &["slot", "index", "value", "var"],
        &rows,
    )?;
    report.runtime_character_slots = slots;
    report.runtime_character_values = rows.len() as u64;
    Ok(())
}

fn write_character_outputs(
    out: &Path,
    rows: &[[String; 8]],
    marks: &[[String; 5]],
    report: &mut ExportReport,
    write: bool,
) -> Result<(), Failure> {
    report.characters = rows.len() as u64;
    if rows.is_empty() || !write {
        return Ok(());
    }
    write_csv(&out.join("characters.csv"), &CHARACTER_HEADER, rows)?;
    let mut by_variant: std::collections::BTreeMap<&str, Vec<[String; 8]>> =
        std::collections::BTreeMap::new();
    for row in rows {
        by_variant
            .entry(row[0].as_str())
            .or_default()
            .push(row.clone());
    }
    for (variant, variant_rows) in by_variant {
        write_csv(
            &out.join(format!("characters_{variant}.csv")),
            &CHARACTER_HEADER,
            &variant_rows,
        )?;
    }
    if !marks.is_empty() {
        write_csv(
            &out.join("characters_marks.csv"),
            &["variant", "table", "order", "codec", "mark"],
            marks,
        )?;
    }
    report.characters = rows.len() as u64;
    Ok(())
}

/// 角色表语言变体与显示 codec(`_cn`/`_tw` 按官方补丁约定固定)。
fn character_variant(path: &str, fallback: yu_format::Codec) -> (String, yu_format::Codec) {
    let lower = path.to_ascii_lowercase();
    if lower.contains("_cn") || lower.contains("_chs") {
        ("cn".into(), yu_format::Codec::Gbk)
    } else if lower.contains("_tw") || lower.contains("_cht") {
        ("tw".into(), yu_format::Codec::Big5)
    } else {
        ("base".into(), fallback)
    }
}

/// 从 `es.CHAR.NAME` / `es.CHAR.NAME.MARK.SET` 调用提取角色名与括号标记。
///
/// 定义脚本的一组调用形如:
/// `GOSUB("es.CHAR.NAME", "【優理】", "優理", "", …)`;要求至少两段非方法
/// 字面量,只有方法名的框架调用不会被误判。
fn extract_character_rows(
    script: &yu_format::DecodedScript,
    args: &[u8],
    variant: &str,
    table: &str,
    codec: yu_format::Codec,
    tunnel: Option<&yu_format::text::TunnelTable>,
    sid: u32,
) -> (Vec<[String; 8]>, Vec<[String; 5]>) {
    let mut rows = Vec::new();
    let mut marks = Vec::new();
    let mut index = 0u32;
    let mut mark_order = 0u32;
    for (pc, group) in script.groups.iter().enumerate() {
        let literals = group_literals(group, args, codec, tunnel);
        let has_name = literals.iter().any(|text| text == "es.CHAR.NAME");
        let has_mark = literals.iter().any(|text| text == "es.CHAR.NAME.MARK.SET");
        if has_name {
            let rest: Vec<String> = literals
                .into_iter()
                .filter(|text| !text.is_empty() && !text.starts_with("es."))
                .collect();
            if rest.len() < 2 {
                continue;
            }
            rows.push([
                variant.to_string(),
                table.to_string(),
                index.to_string(),
                rest[0].clone(),
                rest[1].clone(),
                sid.to_string(),
                pc.to_string(),
                codec.as_str().to_string(),
            ]);
            index += 1;
        } else if has_mark {
            let mark: Vec<String> = literals
                .into_iter()
                .filter(|text| !text.is_empty() && !text.starts_with("es."))
                .collect();
            for text in mark {
                marks.push([
                    variant.to_string(),
                    table.to_string(),
                    mark_order.to_string(),
                    codec.as_str().to_string(),
                    text,
                ]);
                mark_order += 1;
            }
        }
    }
    (rows, marks)
}

const CHARACTER_HEADER: [&str; 8] = [
    "variant",
    "table",
    "index",
    "display",
    "name",
    "script_id",
    "pc",
    "codec",
];

// ------------------------------------------------------------------ 输出

fn safe_name(path: &str) -> String {
    let mut out = String::new();
    for ch in path.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            out.push(ch);
        } else if ch == '/' || ch == '\\' {
            out.push('_');
        } else {
            out.push('_');
        }
    }
    let trimmed = out.trim_matches('_').to_string();
    if trimmed.is_empty() {
        "unnamed".into()
    } else {
        trimmed
    }
}

fn write_text(path: &Path, text: &str) -> Result<(), Failure> {
    let mut bytes = Vec::with_capacity(text.len() + 3);
    bytes.extend_from_slice(BOM);
    bytes.extend_from_slice(text.as_bytes());
    std::fs::write(path, bytes)
        .map_err(|e| Failure::runtime(format!("写 {} 失败: {e}", path.display())))
}

fn csv_escape(field: &str) -> String {
    if field
        .chars()
        .any(|c| c == ',' || c == '"' || c == '\n' || c == '\r')
    {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

fn write_csv<const N: usize>(
    path: &Path,
    header: &[&str; N],
    rows: &[[String; N]],
) -> Result<(), Failure> {
    let mut text = String::new();
    text.push_str(
        &header
            .iter()
            .map(|h| csv_escape(h))
            .collect::<Vec<_>>()
            .join(","),
    );
    text.push_str("\r\n");
    for row in rows {
        text.push_str(
            &row.iter()
                .map(|field| csv_escape(field))
                .collect::<Vec<_>>()
                .join(","),
        );
        text.push_str("\r\n");
    }
    write_text(path, &text)
}

/// `translate.json` 写出:只含角色表与正文(不含字符串常量),每条带空译文槽。
///
/// 坐标与 CSV 模式一致:
/// - 角色:`variant + table + index`;
/// - `WORD`:`script_id + pc`;
/// - 外置剧本:`variant + file + line + seq`。
fn write_translate_json(
    project: &Project,
    out: &Path,
    codec: yu_format::Codec,
    report: &ExportReport,
    text_rows: &[[String; 7]],
    scenario_rows: &[[String; 10]],
    character_rows: &[[String; 8]],
) -> Result<(), Failure> {
    let number = |value: &str| -> Value { Value::Number(value.parse::<f64>().unwrap_or(0.0)) };

    let mut root = BTreeMap::new();
    root.insert("schema".into(), Value::String("yuris-translate-1".into()));
    let mut meta = BTreeMap::new();
    meta.insert(
        "project_root".into(),
        Value::String(project.root().display().to_string()),
    );
    meta.insert(
        "data_root".into(),
        Value::String(project.data_root().display().to_string()),
    );
    meta.insert("text_codec".into(), Value::String(codec.as_str().into()));
    meta.insert(
        "text_codec_source".into(),
        Value::String(report.text_codec_source.clone()),
    );
    meta.insert("text_tunnel".into(), Value::Bool(report.text_tunnel));
    meta.insert(
        "script_key".into(),
        Value::String(report.script_key_hex.clone()),
    );
    meta.insert(
        "script_key_source".into(),
        Value::String(report.script_key_source.clone()),
    );
    let mut counts = BTreeMap::new();
    counts.insert(
        "characters".into(),
        Value::Number(character_rows.len() as f64),
    );
    counts.insert("word_units".into(), Value::Number(text_rows.len() as f64));
    counts.insert(
        "scenario_units".into(),
        Value::Number(scenario_rows.len() as f64),
    );
    counts.insert(
        "text".into(),
        Value::Number((text_rows.len() + scenario_rows.len()) as f64),
    );
    meta.insert("counts".into(), Value::Object(counts));
    root.insert("meta".into(), Value::Object(meta));

    let mut characters = Vec::with_capacity(character_rows.len());
    for row in character_rows {
        let mut item = BTreeMap::new();
        item.insert(
            "key".into(),
            Value::String(format!("character:{}:{}:{}", row[0], row[1], row[2])),
        );
        item.insert("variant".into(), Value::String(row[0].clone()));
        item.insert("table".into(), Value::String(row[1].clone()));
        item.insert("index".into(), number(&row[2]));
        item.insert("display".into(), Value::String(row[3].clone()));
        item.insert("name".into(), Value::String(row[4].clone()));
        item.insert("codec".into(), Value::String(row[7].clone()));
        item.insert("display_translation".into(), Value::String(String::new()));
        item.insert("name_translation".into(), Value::String(String::new()));
        characters.push(Value::Object(item));
    }
    root.insert("characters".into(), Value::Array(characters));

    let mut text = Vec::with_capacity(text_rows.len() + scenario_rows.len());
    for row in text_rows {
        let mut item = BTreeMap::new();
        item.insert(
            "key".into(),
            Value::String(format!("word:s{}:pc{}", row[0], row[2])),
        );
        item.insert("source".into(), Value::String("word".into()));
        item.insert("script_id".into(), number(&row[0]));
        item.insert("script_path".into(), Value::String(row[1].clone()));
        item.insert("pc".into(), number(&row[2]));
        item.insert("unit".into(), number(&row[3]));
        item.insert("line".into(), number(&row[4]));
        item.insert("codec".into(), Value::String(row[5].clone()));
        item.insert("text".into(), Value::String(row[6].clone()));
        item.insert("translation".into(), Value::String(String::new()));
        text.push(Value::Object(item));
    }
    for row in scenario_rows {
        let mut item = BTreeMap::new();
        item.insert(
            "key".into(),
            Value::String(format!(
                "scenario:{}:{}:{}:{}",
                row[0], row[1], row[2], row[3]
            )),
        );
        item.insert("source".into(), Value::String("scenario".into()));
        item.insert("variant".into(), Value::String(row[0].clone()));
        item.insert("file".into(), Value::String(row[1].clone()));
        item.insert("line".into(), number(&row[2]));
        item.insert("seq".into(), number(&row[3]));
        item.insert("kind".into(), Value::String(row[4].clone()));
        item.insert("macro".into(), Value::String(row[5].clone()));
        item.insert("scene".into(), Value::String(row[6].clone()));
        item.insert("label".into(), Value::String(row[7].clone()));
        item.insert("codec".into(), Value::String(row[8].clone()));
        item.insert("text".into(), Value::String(row[9].clone()));
        item.insert("translation".into(), Value::String(String::new()));
        text.push(Value::Object(item));
    }
    root.insert("text".into(), Value::Array(text));

    let serialized = json::to_pretty(&Value::Object(root));
    std::fs::write(out.join("translate.json"), serialized)
        .map_err(|e| Failure::runtime(format!("写 translate.json 失败: {e}")))
}

fn write_manifest(
    project: &Project,
    out: &Path,
    options: &ExportOptions,
    report: &ExportReport,
) -> Result<(), Failure> {
    let mut root = BTreeMap::new();
    root.insert("schema".into(), Value::String("yuris-export-1".into()));
    root.insert(
        "project_root".into(),
        Value::String(project.root().display().to_string()),
    );
    root.insert(
        "data_root".into(),
        Value::String(project.data_root().display().to_string()),
    );
    root.insert(
        "text_codec".into(),
        Value::String(report.text_codec.as_str().into()),
    );
    root.insert(
        "text_codec_source".into(),
        Value::String(report.text_codec_source.clone()),
    );
    root.insert("text_tunnel".into(), Value::Bool(report.text_tunnel));
    if let Some((cp932, gbk)) = report.text_codec_scores {
        let mut scores = BTreeMap::new();
        scores.insert("cp932".into(), Value::Number(cp932));
        scores.insert("gbk".into(), Value::Number(gbk));
        root.insert("text_codec_scores".into(), Value::Object(scores));
    }
    root.insert(
        "script_key".into(),
        Value::String(report.script_key_hex.clone()),
    );
    root.insert(
        "script_key_source".into(),
        Value::String(report.script_key_source.clone()),
    );
    root.insert(
        "include_literals".into(),
        Value::Bool(options.include_literals),
    );
    let mut counts = BTreeMap::new();
    counts.insert("scripts".into(), Value::Number(report.scripts as f64));
    counts.insert("text_units".into(), Value::Number(report.text_units as f64));
    counts.insert(
        "literal_units".into(),
        Value::Number(report.literal_units as f64),
    );
    counts.insert(
        "literal_skipped".into(),
        Value::Number(report.literal_skipped as f64),
    );
    counts.insert(
        "scenario_files".into(),
        Value::Number(report.scenario_files as f64),
    );
    counts.insert(
        "scenario_units".into(),
        Value::Number(report.scenario_units as f64),
    );
    counts.insert(
        "character_variants".into(),
        Value::Number(report.character_variants as f64),
    );
    counts.insert("characters".into(), Value::Number(report.characters as f64));
    counts.insert(
        "runtime_character_slots".into(),
        Value::Number(report.runtime_character_slots as f64),
    );
    counts.insert(
        "runtime_character_values".into(),
        Value::Number(report.runtime_character_values as f64),
    );
    root.insert("counts".into(), Value::Object(counts));
    root.insert(
        "skipped".into(),
        Value::Array(
            report
                .skipped
                .iter()
                .map(|s| Value::String(s.clone()))
                .collect(),
        ),
    );
    root.insert(
        "warnings".into(),
        Value::Array(
            report
                .warnings
                .iter()
                .map(|s| Value::String(s.clone()))
                .collect(),
        ),
    );
    let text = json::to_pretty(&Value::Object(root));
    std::fs::write(out.join("manifest.json"), text)
        .map_err(|e| Failure::runtime(format!("写 manifest.json 失败: {e}")))
}

/// 报告一行摘要(CLI / 宿主共用文案)。
pub fn summary(report: &ExportReport) -> String {
    let codec_label = if report.text_tunnel {
        format!("{}+tunnel", report.text_codec_source)
    } else {
        report.text_codec_source.clone()
    };
    format!(
        "export: scripts={} text={} literals={} scenario={}({} files) characters={}({} variants) codec={}({})",
        report.scripts,
        report.text_units,
        format!("{}(skipped {})", report.literal_units, report.literal_skipped),
        report.scenario_units,
        report.scenario_files,
        report.characters,
        report.character_variants,
        report.text_codec.as_str(),
        codec_label
    )
}

/// `translate.json` 模式的一行摘要。
pub fn summary_translate(report: &ExportReport) -> String {
    let codec_label = if report.text_tunnel {
        format!("{}+tunnel", report.text_codec_source)
    } else {
        report.text_codec_source.clone()
    };
    format!(
        "export(translate-json): characters={} text={}(word {} + scenario {}) codec={}({})",
        report.characters,
        report.text_units + report.scenario_units,
        report.text_units,
        report.scenario_units,
        report.text_codec.as_str(),
        codec_label
    )
}

/// 输出目录布局(供文档/宿主引用)。
pub fn output_layout() -> [&'static str; 6] {
    [
        "manifest.json",
        "text.csv",
        "literals.csv",
        "scenario.csv",
        "characters.csv",
        "text/ + scenario/",
    ]
}

/// 供宿主直接写盘的工具(Android 也在用同一份目录构造)。
pub fn default_output_dir(project_root: &Path) -> PathBuf {
    project_root.join("export-text")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scenario_macro_extracts_quotes_and_escapes() {
        let line = "\\LC(\"a\\\"b\") \\VO(x) // tail";
        let macros = extract_macros(line);
        assert_eq!(macros, vec![("LC".to_string(), "a\"b".to_string())]);
    }

    #[test]
    fn scenario_block_comment_skips_lines() {
        let mut in_block = false;
        assert_eq!(clean_line("/* \\LC(\"x\")", &mut in_block), "");
        assert_eq!(clean_line("\\LC(\"y\") */", &mut in_block), "");
        assert_eq!(clean_line("\\LC(\"z\")", &mut in_block), "\\LC(\"z\")");
    }

    #[test]
    fn scenario_splits_bare_and_inline_text() {
        let (command, text) = split_leading_command(r"\VO(x)【名】「…」");
        assert_eq!(command.as_deref(), Some("VO"));
        assert_eq!(text.as_deref(), Some("【名】「…」"));
        let (command, text) = split_leading_command(r"\BGM(x)");
        assert_eq!(command.as_deref(), Some("BGM"));
        assert_eq!(text, None);
        let (command, text) = split_leading_command("裸文本行");
        assert_eq!(command, None);
        assert_eq!(text.as_deref(), Some("裸文本行"));
    }

    #[test]
    fn literal_payload_splits_adjacent_quotes() {
        let payload = vec![b'M', 0x12, 0x00, 0x22, b'a', 0x22, 0x22, b'b', 0x22, 0x00];
        let segments = quoted_segments(&payload);
        assert_eq!(segments, vec![&b"a"[..], &b"b"[..]]);
    }

    #[test]
    fn literal_payload_supports_backtick_and_single_quote() {
        let payload = vec![b'M', 0x06, 0x00, 0x60, b'a', 0x60, 0x27, b'b', 0x27];
        let segments = quoted_segments(&payload);
        assert_eq!(segments, vec![&b"a"[..], &b"b"[..]]);
    }

    #[test]
    fn text_codec_choice_parses() {
        assert_eq!(
            TextCodecChoice::parse("gbk").unwrap(),
            TextCodecChoice::Fixed(yu_format::Codec::Gbk)
        );
        assert!(TextCodecChoice::parse("utf8").is_err());
    }
}
