//! 文本编码统一入口(跨平台)。
//!
//! 补丁路线由**用户显式选择**,不再靠产物特判保证正确性:
//!
//! - [`PatchKind::Original`]:原版 CP932;
//! - [`PatchKind::Gbk`]:全局 GBK(官方 `_chs` 换首字节表同构);
//! - [`PatchKind::Big5`]:全局 Big5 / CP950(繁中旧作与部分汉化补丁);
//! - [`PatchKind::Tunnel`]:CP932 + `sjis_ext.bin` 隧道表。
//!
//! 另有与编码正交的**补丁字体**路线([`PatchFont`]):部分汉化补丁把中文写成
//! 合法 CP932 的“载体字”,再随包提供重映射字体(`hook.ini` 的
//! `[FONT] FONTFILENAME`;FONTCHANGER 同型)。字体不是换编码,必须把
//! 全部 FONT 请求替换成该字体才成立,因此由引擎在启动时强制注入。
//!
//! [`PatchKind::Auto`] 是便利值:只在宿主没给类型时按产物识别
//! (`codecs.json` → 隧道 → YURIS_TOOLS 签名),识别结果在 evidence 里
//! 标注来源,不参与正确性保证。
//!
//! 各宿主(桌面/serve/Android/wasm)共用这套类型 + [`PatchSelection`],
//! 只负责把用户选择/`codecs.json` 传进来。

use std::path::{Path, PathBuf};

use yu_format::text::{CodecConfig, DisplayConfig, TunnelTable};
use yu_format::Codec;
use yu_vfs::LookupKey;

use crate::json;
use crate::project::{Project, CODEC};
use crate::Failure;

/// 文本编码路线(用户可选;跨平台统一)。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PatchKind {
    /// 便捷识别:仅在宿主未指定时使用,结果不作为正确性保证。
    #[default]
    Auto,
    /// 原版 CP932。
    Original,
    /// 全局 GBK(官方 `_chs` 同构)。
    Gbk,
    /// 全局 Big5 / CP950(繁中;港台旧作与部分汉化补丁)。
    ///
    /// 与 [`PatchKind::Gbk`] 同构:换掉整包文本的编码,路径/文件名仍按
    /// CP932(官方引擎的资源命名从未随汉化改变)。**不参与 `Auto` 识别**
    /// —— cp950 与 cp936 前导区间重叠,只能由用户/宿主显式指定。
    Big5,
    /// CP932 + SJIS 隧道表(`sjis_ext.bin`)。
    Tunnel,
    /// 全局 UTF-8(2025 年本地化发行版,如 Kagura 简中/英文版)。
    ///
    /// 与 [`PatchKind::Gbk`] 同构:换掉整包脚本文本的编码,路径/文件名
    /// 仍按 CP932。**不参与 `Auto` 识别**(脚本池采样与版本门控尚未取证),
    /// 由用户/宿主显式指定,或 `codecs.json` 的 `script: "utf8"`。
    Utf8,
}

impl PatchKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            PatchKind::Auto => "auto",
            PatchKind::Original => "original",
            PatchKind::Gbk => "gbk",
            PatchKind::Big5 => "big5",
            PatchKind::Tunnel => "tunnel",
            PatchKind::Utf8 => "utf8",
        }
    }

    /// 解析宿主/CLI 值。`auto|original|gbk|big5|tunnel|utf8`;`cp932` 等同
    /// `original`。全平台唯一一份(CLI `--patch`、serve、Android
    /// `run-config.json` 的 `patch` 键共用)。
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(PatchKind::Auto),
            "original" | "off" | "none" | "cp932" | "sjis" | "shift_jis" => {
                Ok(PatchKind::Original)
            }
            "gbk" | "cp936" | "gb2312" => Ok(PatchKind::Gbk),
            "big5" | "cp950" | "big-5" => Ok(PatchKind::Big5),
            "tunnel" | "sjis_ext" => Ok(PatchKind::Tunnel),
            "utf8" | "utf-8" => Ok(PatchKind::Utf8),
            other => Err(format!(
                "未知补丁类型 `{other}`;可用:auto | original | gbk | big5 | tunnel | utf8"
            )),
        }
    }
}

/// 最终生效的补丁配置(所有宿主共用)。
#[derive(Clone, Debug, PartialEq)]
pub struct PatchProfile {
    /// 实际生效的路线(不会是 `Auto`)。
    pub kind: PatchKind,
    /// 执行层全局 codec(官方汉化版 = 全局 GBK)。
    pub execution: Codec,
    /// 显示层 codec(全局;官方 CHS 同构)。
    pub text_default: Codec,
    /// 路径/文件名 codec。
    pub name: Codec,
    /// 隧道表(`PatchKind::Tunnel` 时非空)。
    pub tunnel: Option<TunnelTable>,
    /// 补丁自带的重映射字体(FONTCHANGER 同型;与编码路线正交)。
    pub font: Option<PatchFont>,
    /// 决策来源:`cli` / `manifest` / `heuristic` / `default`。
    pub source: PatchSource,
    /// 识别证据(供日志/诊断)。
    pub evidence: Vec<String>,
}

/// 补丁配置的来源(日志/诊断用,便于判断“为什么是这个编码”)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatchSource {
    /// 宿主/CLI 显式指定。
    Cli,
    /// 游戏根 `codecs.json`。
    Manifest,
    /// 产物签名启发式(`PatchKind::Auto` 命中)。
    Heuristic,
    /// 没有任何补丁产物,按原版。
    Default,
}

impl PatchSource {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            PatchSource::Cli => "cli",
            PatchSource::Manifest => "manifest",
            PatchSource::Heuristic => "heuristic",
            PatchSource::Default => "default",
        }
    }
}

impl Default for PatchProfile {
    fn default() -> Self {
        Self {
            kind: PatchKind::Original,
            execution: Codec::Cp932,
            text_default: Codec::Cp932,
            name: Codec::Cp932,
            tunnel: None,
            font: None,
            source: PatchSource::Default,
            evidence: Vec::new(),
        }
    }
}

/// 补丁自带的重映射字体(`hook.ini` + FONTCHANGER 同型)。
///
/// 这类补丁的正文是**合法 CP932 的载体字**(例如 `髮` 承载 `咚`),只有
/// 随包的 ttf 把载体码位映射到目标字形时才读得通。引擎必须像
/// FONTCHANGER 一样**替换全部字体**,只设默认字体会让脚本显式点名
/// 字体/按 `FONT NO` 选字体的文本继续用错字形。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatchFont {
    /// 已解析的字体文件绝对/相对工程根路径(存在性已在检测时确认)。
    pub path: PathBuf,
    /// `hook.ini` 的 `[FONT] FONTNAME`(仅诊断;真实 family 由字体字节解析)。
    pub declared_name: Option<String>,
    /// 检测证据(进 profile.evidence)。
    pub source: String,
}

impl PatchFont {
    /// 字体单文件上限(与 CLI `--font` 同口径)。
    pub const MAX_BYTES: u64 = 64 * 1024 * 1024;

    /// 读取字体字节(空文件/超大文件显式报错,不静默降级)。
    pub fn load(&self) -> Result<Vec<u8>, Failure> {
        let meta = std::fs::metadata(&self.path).map_err(|e| {
            Failure::runtime(format!("读补丁字体 {} 失败: {e}", self.path.display()))
        })?;
        if meta.len() == 0 {
            return Err(Failure::runtime(format!(
                "补丁字体 {} 是空文件",
                self.path.display()
            )));
        }
        if meta.len() > Self::MAX_BYTES {
            return Err(Failure::runtime(format!(
                "补丁字体 {} 超过 {}MiB",
                self.path.display(),
                Self::MAX_BYTES / (1024 * 1024)
            )));
        }
        std::fs::read(&self.path).map_err(|e| {
            Failure::runtime(format!("读补丁字体 {} 失败: {e}", self.path.display()))
        })
    }
}

/// 启动时的补丁选择(跨平台统一入口;宿主只负责传用户选择)。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PatchSelection {
    pub kind: PatchKind,
}

impl PatchSelection {
    #[must_use]
    pub const fn new(kind: PatchKind) -> Self {
        Self { kind }
    }

    /// 解析宿主/CLI 值(`auto|original|gbk|tunnel`)。
    pub fn parse(value: &str) -> Result<Self, String> {
        PatchKind::parse(value).map(Self::new)
    }

    /// 解析为最终 profile。
    ///
    /// - 显式 `original` / `gbk` / `tunnel`:直接采用,不读产物签名;
    /// - `auto`:`codecs.json` 优先,其次隧道,其次 YURIS_TOOLS 签名;
    /// - 什么都没有:`original` + [`PatchSource::Default`]。
    ///
    /// 所有路线都附带补丁字体检测(编码与字体正交;`hook.ini` 存在才命中)。
    pub fn profile(self, project: &Project) -> Result<PatchProfile, Failure> {
        let mut profile = match self.kind {
            PatchKind::Original => PatchProfile {
                kind: PatchKind::Original,
                source: PatchSource::Cli,
                evidence: vec!["cli: original".to_string()],
                ..PatchProfile::default()
            },
            PatchKind::Gbk => PatchProfile {
                kind: PatchKind::Gbk,
                execution: Codec::Gbk,
                text_default: Codec::Gbk,
                name: Codec::Cp932,
                source: PatchSource::Cli,
                evidence: vec!["cli: gbk".to_string()],
                ..PatchProfile::default()
            },
            PatchKind::Big5 => PatchProfile {
                kind: PatchKind::Big5,
                execution: Codec::Big5,
                text_default: Codec::Big5,
                name: Codec::Cp932,
                source: PatchSource::Cli,
                evidence: vec!["cli: big5".to_string()],
                ..PatchProfile::default()
            },
            PatchKind::Utf8 => PatchProfile {
                kind: PatchKind::Utf8,
                execution: Codec::Utf8,
                text_default: Codec::Utf8,
                name: Codec::Cp932,
                source: PatchSource::Cli,
                evidence: vec!["cli: utf8".to_string()],
                ..PatchProfile::default()
            },
            PatchKind::Tunnel => {
                let tunnel = read_tunnel(project, None)?.ok_or_else(|| {
                    Failure::runtime(
                        "补丁类型 tunnel 需要游戏根的 sjis_ext.bin(未找到)",
                    )
                })?;
                PatchProfile {
                    kind: PatchKind::Tunnel,
                    execution: Codec::Cp932,
                    text_default: Codec::Cp932,
                    name: Codec::Cp932,
                    tunnel: Some(tunnel),
                    source: PatchSource::Cli,
                    evidence: vec!["cli: tunnel".to_string()],
                    ..PatchProfile::default()
                }
            }
            PatchKind::Auto => return detect(project),
        };
        attach_patch_font(&mut profile, project)?;
        Ok(profile)
    }
}

impl PatchProfile {
    #[must_use]
    pub fn codecs(&self) -> CodecConfig {
        CodecConfig {
            script: self.execution,
            text: self.text_default,
            name: self.name,
        }
    }

    /// 显示层配置:全局 codec + 隧道(与官方 CHS 同构,不逐文件区分)。
    #[must_use]
    pub fn display(&self) -> DisplayConfig {
        DisplayConfig {
            default: self.text_default,
            tunnel: self.tunnel.clone(),
        }
    }

    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "kind={} source={} execution={} text={} name={} tunnel={} font={}",
            self.kind.as_str(),
            self.source.as_str(),
            self.execution.as_str(),
            self.text_default.as_str(),
            self.name.as_str(),
            self.tunnel.as_ref().is_some_and(|t| !t.is_empty()),
            self.font
                .as_ref()
                .map_or_else(|| "-".to_string(), |font| font.path.display().to_string())
        )
    }
}

/// `PatchKind::Auto` 的启发式识别(便利,不作为正确性保证)。
///
/// 顺序:`codecs.json` → `sjis_ext.bin` → YURIS_TOOLS 产物签名 → 原版。
/// 编码路线之外始终附带补丁字体检测(与编码正交)。
pub fn detect(project: &Project) -> Result<PatchProfile, Failure> {
    let mut profile = detect_route(project)?;
    attach_patch_font(&mut profile, project)?;
    Ok(profile)
}

fn detect_route(project: &Project) -> Result<PatchProfile, Failure> {
    if let Some(manifest) = read_manifest(project)? {
        return Ok(manifest);
    }

    if let Some(tunnel) = read_tunnel(project, None)? {
        return Ok(PatchProfile {
            kind: PatchKind::Tunnel,
            execution: Codec::Cp932,
            text_default: Codec::Cp932,
            name: Codec::Cp932,
            tunnel: Some(tunnel),
            source: PatchSource::Heuristic,
            evidence: vec!["heuristic: sjis_ext.bin".to_string()],
            ..PatchProfile::default()
        });
    }

    if has_yuris_tools_signature(project) {
        return Ok(PatchProfile {
            kind: PatchKind::Gbk,
            execution: Codec::Gbk,
            text_default: Codec::Gbk,
            name: Codec::Cp932,
            source: PatchSource::Heuristic,
            evidence: vec![
                "heuristic: YURIS_TOOLS signature (MyDll PDB/import);                  prefer explicit --patch gbk for correctness"
                    .to_string(),
            ],
            ..PatchProfile::default()
        });
    }

    // v555:exe 里的 256 字节静态 lead 表(`DAT_0059b0c0`)就是该构建的
    // 执行编码。旧版本不探测,`Auto` 行为保持 c1549eb 原样。
    if let Some((codec, evidence)) = static_codec_for_v555(project) {
        let mut profile = match codec {
            Codec::Gbk => PatchProfile {
                kind: PatchKind::Gbk,
                execution: Codec::Gbk,
                text_default: Codec::Gbk,
                name: Codec::Cp932,
                source: PatchSource::Heuristic,
                ..PatchProfile::default()
            },
            Codec::Big5 => PatchProfile {
                kind: PatchKind::Big5,
                execution: Codec::Big5,
                text_default: Codec::Big5,
                name: Codec::Cp932,
                source: PatchSource::Heuristic,
                ..PatchProfile::default()
            },
            Codec::Cp932 => PatchProfile {
                source: PatchSource::Heuristic,
                ..PatchProfile::default()
            },
            Codec::Utf8 => PatchProfile {
                kind: PatchKind::Utf8,
                execution: Codec::Utf8,
                text_default: Codec::Utf8,
                name: Codec::Cp932,
                source: PatchSource::Heuristic,
                ..PatchProfile::default()
            },
        };
        profile.evidence.push(evidence);
        return Ok(profile);
    }

    Ok(PatchProfile::default())
}

/// 非游戏本体的辅助 exe(安装器/设置器/本仓库宿主):不参与编码/存档探测。
///
/// 证据:<v555-1> 目录实测主 exe `<main-exe>` 与 `游戏设置.exe` 同表(CP932,
/// 0x19b4c0);<v555-5> 官方包带 `settings.exe`。helper 与主 exe
/// 编码不一致时,若参与扫描会把结果误判成另一种编码,或冲突成 `None`
/// (退回原版默认,丢证据)。主 exe 才是构建静态表的真值。
pub(crate) fn is_helper_exe(path: &Path) -> bool {
    let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
        return false;
    };
    matches!(
        stem.to_ascii_lowercase().as_str(),
        "inst"
            | "install"
            | "setup"
            | "uninst"
            | "uninstall"
            | "settings"
            | "setting"
            | "ysicon"
            | "yu-cli"
            | "yu-serve"
            | "yu-engine"
            | "游戏设置"
            | "エンジン設定"
            | "セーブファイル設定"
    )
}

/// v555 构建的静态执行编码:扫描工程根 `*.exe` 的 256 字节 lead 表。
///
/// 证据:`DAT_0059b0c0`(256 B)在 v555 构建里是静态表,
/// `0x81..=0x9F`+`0xA1..=0xFE`=1 判为 Big5;CP932/GBK 各有不同区间。
/// 只在精确 `ysc_version == 555` 时启用。
///
/// 多个主 exe 的表不一致时(<v555-sample>:`<main-exe>`=CP932、
/// `<chs-exe>`=GBK),不静默回退默认值:用工程自己的脚本文本做
/// **数据自证**(候选 lead 表下字符串字面量能否正常闭合),唯一胜出者
/// 生效;数据也说不出唯一解才放弃。发行包与 exe 是配套的:数据是 GBK
/// 时,带 GBK 表的那只 exe 胜出。
fn static_codec_for_v555(project: &Project) -> Option<(Codec, String)> {
    let version = project.ysc_version()?;
    if !yu_rt::semantics::SemanticProfile::from_yscm_version(version).is_v555() {
        return None;
    }
    let entries = std::fs::read_dir(project.root()).ok()?;
    let mut exes: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
        })
        .collect();
    exes.sort_by_key(|path| std::fs::metadata(path).map(|m| m.len()).unwrap_or(0));
    exes.reverse();
    // 按 Codec::ALL 的稳定顺序收集候选;同编码只保留体积最大的 exe 证据。
    let mut evidence: Vec<(Codec, String)> = Vec::new();
    for exe in exes {
        // 安装器/设置器/宿主 exe 不是构建本体:它们的静态表不参与判定
        // (<v555-1> 实测 `游戏设置.exe` 是 helper,主 exe 才是真值)。
        if is_helper_exe(&exe) {
            continue;
        }
        let Ok(bytes) = std::fs::read(&exe) else {
            continue;
        };
        let Some(codec) = static_codec_in_bytes(&bytes) else {
            continue;
        };
        if evidence.iter().any(|(known, _)| *known == codec) {
            continue;
        }
        let name = exe
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| exe.display().to_string());
        evidence.push((codec, name));
    }
    match evidence.as_slice() {
        [] => None,
        [(codec, name)] => Some((
            *codec,
            format!("heuristic: v555 static lead table in {name} = {}", codec.as_str()),
        )),
        _ => {
            let candidates: Vec<Codec> = evidence.iter().map(|(codec, _)| *codec).collect();
            let chosen = probe_codec_by_script_literals(project, &candidates)?;
            let tables = evidence
                .iter()
                .map(|(codec, name)| format!("{name}={}", codec.as_str()))
                .collect::<Vec<_>>()
                .join(", ");
            Some((
                chosen,
                format!(
                    "heuristic: v555 static lead tables conflict ({tables}); \
                     script literal self-check -> {}",
                    chosen.as_str()
                ),
            ))
        }
    }
}

/// 冲突候选的**数据自证**:用工程自己的脚本字符串字面量给候选 lead 表打分。
///
/// 只看完整落在窗口内的 `0x4d` 字面量记录(窗口被截断的记录整条跳过,
/// 避免把“汉化改长文本留下的过期窗口长度”算到正确编码头上);按字面量
/// 能否在该 lead 表下闭合计分:未闭合 +8、结尾悬空前导 +4。样本不足或
/// 没有唯一显著胜者时返回 `None`(交回“不猜”的旧行为)。
///
/// 证据:<v555-sample> `s273 pc3` 的 `"姓"`(GBK `d1 e0`)在 CP932 下
/// `e0` 吞掉结束引号 → 未闭合;GBK 下正常闭合。数据即发行包自带的
/// 汉化脚本,不需要游戏名/exe 名特判。
fn probe_codec_by_script_literals(project: &Project, candidates: &[Codec]) -> Option<Codec> {
    const MAX_SCRIPTS: usize = 512;
    const MAX_TOTAL_BYTES: u64 = 32 * 1024 * 1024;
    const MAX_SCRIPT_BYTES: u64 = 8 * 1024 * 1024;
    const MAX_LITERALS: usize = 8192;
    const PER_SCRIPT_LITERALS: usize = 64;
    const MIN_LITERALS: u32 = 8;
    const MIN_NON_ASCII: u64 = 64;

    if candidates.len() < 2 {
        return None;
    }
    let key = project.resolve_script_key(None).ok()?;
    let source = project.open_source().ok()?;
    let dir = LookupKey::new(b"ysbin", CODEC).ok()?;
    let mut names: Vec<(u64, Vec<u8>)> = source
        .list(&dir)
        .ok()?
        .into_iter()
        .filter(|entry| matches!(entry.kind, yu_vfs::EntryKind::File))
        .filter_map(|entry| {
            let size = entry.size?;
            if size > MAX_SCRIPT_BYTES || script_id_of_name(&entry.name).is_none() {
                return None;
            }
            Some((size, entry.name))
        })
        .collect();
    // 小脚本优先(定义/宏脚本通常不大,但字面量密度高);同尺寸按名字
    // 稳定排序。总读取量有上限,冲突探测不拖慢启动。
    names.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    names.truncate(MAX_SCRIPTS);

    let mut scores = vec![LiteralScore::default(); candidates.len()];
    let mut literals = 0usize;
    let mut total_bytes = 0u64;
    'scripts: for (size, name) in names {
        if total_bytes.saturating_add(size) > MAX_TOTAL_BYTES {
            continue;
        }
        let mut raw = b"ysbin/".to_vec();
        raw.extend_from_slice(&name);
        let Ok(path) = LookupKey::new(&raw, CODEC) else {
            continue;
        };
        let Ok(data) = source.open(&path) else {
            continue;
        };
        let Some(script_id) = script_id_of_name(&name) else {
            continue;
        };
        let Ok(script) = yu_rt::tables::ScriptFile::parse(script_id, &data, key.bytes) else {
            continue;
        };
        total_bytes += size;
        let mut script_literals = 0usize;
        'windows: for pc in 0..script.group_count() {
            for rec in script.group_windows(pc) {
                let Some(payload) = script.window_payload(rec) else {
                    continue;
                };
                let mut pos = 0usize;
                while pos + 3 <= payload.len() {
                    let op = payload[pos];
                    let len = u16::from_le_bytes([payload[pos + 1], payload[pos + 2]]) as usize;
                    let Some(end) = pos.checked_add(3 + len) else {
                        break;
                    };
                    if end > payload.len() {
                        // 记录越过窗口(过期 length):整条跳过,不参与评分。
                        break;
                    }
                    if op == 0x4d {
                        for (index, codec) in candidates.iter().enumerate() {
                            scores[index].score_literal(&payload[pos + 3..end], *codec);
                        }
                        literals += 1;
                        script_literals += 1;
                        if script_literals >= PER_SCRIPT_LITERALS {
                            break 'windows;
                        }
                    }
                    pos = end;
                }
                if literals >= MAX_LITERALS {
                    break 'scripts;
                }
            }
        }
    }

    let mut order: Vec<usize> = (0..candidates.len()).collect();
    order.sort_by_key(|&index| scores[index].score);
    let best = scores[order[0]];
    let second = scores[order[1]];
    if best.closed + best.unterminated < MIN_LITERALS
        || best.non_ascii < MIN_NON_ASCII
        || best.score + 8 > second.score
    {
        return None;
    }
    Some(candidates[order[0]])
}

/// 一种候选 lead 表在采样字面量上的闭合评分。
#[derive(Clone, Copy, Default)]
struct LiteralScore {
    score: u64,
    closed: u32,
    unterminated: u32,
    non_ascii: u64,
}

impl LiteralScore {
    /// 扫一条 `0x4d` 记录载荷(`[界定符][body...]`):与表达式层同一套
    /// 转义/前导规则,遇到界定符即闭合;到记录末尾仍未闭合记罚分。
    fn score_literal(&mut self, payload: &[u8], codec: Codec) {
        let Some(&term) = payload.first() else {
            return;
        };
        if !matches!(term, b'"' | b'\'' | b'`') {
            return;
        }
        let body = &payload[1..];
        let mut index = 0usize;
        let mut non_ascii = 0u64;
        while index < body.len() {
            let byte = body[index];
            if byte == term {
                self.closed += 1;
                self.non_ascii += non_ascii;
                return;
            }
            if byte == b'\\' {
                index += 2;
                continue;
            }
            if byte >= 0x80 {
                non_ascii += 1;
            }
            if codec.is_lead(byte) {
                if index + 1 < body.len() {
                    index += 2;
                    continue;
                }
                // 结尾悬空前导:汉化截断或编码不对,都按未闭合重罚。
                self.score += 12;
                self.unterminated += 1;
                self.non_ascii += non_ascii;
                return;
            }
            index += 1;
        }
        self.score += 8;
        self.unterminated += 1;
        self.non_ascii += non_ascii;
    }
}

/// `ystNNNNN.ybn` → 脚本号(其它名字返回 `None`)。
fn script_id_of_name(name: &[u8]) -> Option<u32> {
    let digits = name.strip_prefix(b"yst")?.strip_suffix(b".ybn")?;
    if digits.len() != 5 || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(digits).ok()?.parse().ok()
}

/// 在字节流里找静态 lead 表;恰好一种编码命中才返回。
///
/// 先用表内 8 字节特征段定位(`0xDF..0xE7`:CP932=`0,1..`、Big5=`1,0..`、
/// GBK=全 1),再核对完整 256 字节。避免对 3.5MB exe 做 256 字节窗口
/// 全量比较(debug 下会拖慢启动)。
fn static_codec_in_bytes(bytes: &[u8]) -> Option<Codec> {
    // 0xA0..0xFF 的特征:CP932=`0…0,1…1,0`、Big5=`0,1…1,0`、
    // GBK=`1…1,0`。尾部的 0 让特征不会和 exe 里的 0xFF 填充连成一片。
    const SIGNATURE_AT: usize = 0xA0;
    const SIGNATURE_LEN: usize = 0x60;
    if bytes.len() < 256 {
        return None;
    }
    let mut hits = Vec::new();
    for codec in [Codec::Cp932, Codec::Gbk, Codec::Big5] {
        let table = lead_table(codec);
        let signature = &table[SIGNATURE_AT..SIGNATURE_AT + SIGNATURE_LEN];
        let mut found = false;
        for position in memchr::memmem::find_iter(bytes, signature) {
            let Some(start) = position.checked_sub(SIGNATURE_AT) else {
                continue;
            };
            if start + 256 <= bytes.len() && &bytes[start..start + 256] == table {
                found = true;
                break;
            }
        }
        if found {
            hits.push(codec);
        }
    }
    if hits.len() == 1 {
        Some(hits[0])
    } else {
        None
    }
}

/// 官方 256 字节 lead 表(1 = 双字节前导)。
fn lead_table(codec: Codec) -> [u8; 256] {
    let mut table = [0u8; 256];
    for (byte, slot) in table.iter_mut().enumerate() {
        let byte = byte as u8;
        *slot = u8::from(match codec {
            Codec::Cp932 => matches!(byte, 0x81..=0x9F | 0xE0..=0xFC),
            Codec::Gbk => matches!(byte, 0x81..=0xFE),
            Codec::Big5 => matches!(byte, 0x81..=0x9F | 0xA1..=0xFE),
            // UTF-8 没有「双字节前导表」;本函数只被三张双字节表的扫描调用。
            Codec::Utf8 => false,
        });
    }
    table
}

/// `hook.ini` 的字体声明(只取与补丁字体有关的两个键)。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct HookFont {
    /// `[FONT] FONTNAME`(诊断用;真实 family 以字体字节为准)。
    name: Option<String>,
    /// `[FONT] FONTFILENAME`(相对 `hook.ini` 所在目录或游戏根)。
    file: Option<String>,
}

/// `hook.ini` 上限(补丁配置是纯文本)。
const HOOK_INI_MAX_BYTES: u64 = 256 * 1024;

/// 把检测到的补丁字体挂进 profile(已挂则不动)。
fn attach_patch_font(profile: &mut PatchProfile, project: &Project) -> Result<(), Failure> {
    if profile.font.is_some() {
        return Ok(());
    }
    let Some(font) = detect_patch_font(project)? else {
        return Ok(());
    };
    profile.evidence.push(font.source.clone());
    profile.font = Some(font);
    Ok(())
}

/// 检测补丁自带的重映射字体。
///
/// 触发条件 = 游戏根(或开发布局的 `data/`)存在 `hook.ini`,且
/// `[FONT] FONTFILENAME` 非空。字体文件按以下顺序查找:
/// `hook.ini` 同目录 → 同目录 `fonts/` → 游戏根 → 游戏根 `fonts/`。
///
/// 声明了字体却找不到时**显式报错**(不静默降级成“普通字体渲染”,
/// 否则用户只会看到“一部分乱码”而不知道补丁字体没装上)。
pub fn detect_patch_font(project: &Project) -> Result<Option<PatchFont>, Failure> {
    detect_patch_font_in(project.root())
}

/// 同 [`detect_patch_font`],但直接收游戏根路径(引擎用;测试也可直接调用)。
pub(crate) fn detect_patch_font_in(root: &Path) -> Result<Option<PatchFont>, Failure> {
    let Some(hook) = find_hook_ini(root) else {
        return Ok(None);
    };
    let meta = std::fs::metadata(&hook).map_err(|e| {
        Failure::runtime(format!("读补丁配置 {} 失败: {e}", hook.display()))
    })?;
    if meta.len() > HOOK_INI_MAX_BYTES {
        return Err(Failure::runtime(format!(
            "补丁配置 {} 超过 {}KiB",
            hook.display(),
            HOOK_INI_MAX_BYTES / 1024
        )));
    }
    let bytes = std::fs::read(&hook).map_err(|e| {
        Failure::runtime(format!("读补丁配置 {} 失败: {e}", hook.display()))
    })?;
    let parsed = parse_hook_font(&bytes);
    let Some(file) = parsed.file.as_deref().map(str::trim).filter(|v| !v.is_empty()) else {
        // hook.ini 只做窗口改名/文本替换时没有字体声明:正常,不是字体补丁。
        return Ok(None);
    };
    // 补丁作者常按 Windows 习惯写 `fonts\foo.ttf`;统一成 `/` 再 join。
    let relative = file.replace('\\', "/");
    let hook_dir = hook.parent().unwrap_or(root);
    let hook_fonts = hook_dir.join("fonts");
    let root_fonts = root.join("fonts");
    let mut candidates: Vec<PathBuf> = Vec::new();
    for base in [hook_dir, hook_fonts.as_path(), root, root_fonts.as_path()] {
        let candidate = base.join(&relative);
        if !candidates.contains(&candidate) {
            candidates.push(candidate);
        }
    }
    let Some(path) = candidates.iter().find(|path| path.is_file()).cloned() else {
        let searched = candidates
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(Failure::runtime(format!(
            "补丁 {} 声明了字体 `{file}`,但找不到文件(已查找: {searched});\
             请把字体与 hook.ini 一起放进游戏根",
            hook.display()
        )));
    };
    let size = std::fs::metadata(&path)
        .map_err(|e| Failure::runtime(format!("读补丁字体 {} 失败: {e}", path.display())))?
        .len();
    if size == 0 {
        return Err(Failure::runtime(format!(
            "补丁字体 {} 是空文件",
            path.display()
        )));
    }
    if size > PatchFont::MAX_BYTES {
        return Err(Failure::runtime(format!(
            "补丁字体 {} 超过 {}MiB",
            path.display(),
            PatchFont::MAX_BYTES / (1024 * 1024)
        )));
    }
    let source = format!(
        "hook.ini: FONTFILENAME={file}{}",
        parsed
            .name
            .as_deref()
            .map(|name| format!(" FONTNAME={name}"))
            .unwrap_or_default()
    );
    Ok(Some(PatchFont {
        path,
        declared_name: parsed.name,
        source,
    }))
}

/// 在 `root` 与开发布局 `root/data` 下找 `hook.ini`(文件名大小写不敏感)。
fn find_hook_ini(root: &Path) -> Option<PathBuf> {
    for dir in [root.to_path_buf(), root.join("data")] {
        let direct = dir.join("hook.ini");
        if direct.is_file() {
            return Some(direct);
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut hits: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.is_file()
                    && path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.eq_ignore_ascii_case("hook.ini"))
            })
            .collect();
        hits.sort();
        if let Some(hit) = hits.into_iter().next() {
            return Some(hit);
        }
    }
    None
}

/// 解析 `hook.ini`,只保留 `[FONT]` 段的 `FONTNAME` / `FONTFILENAME`。
///
/// 键与段名大小写不敏感;`#`/`;` 行注释;容忍 UTF-8 BOM 与 CRLF。
fn parse_hook_font(bytes: &[u8]) -> HookFont {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let text = String::from_utf8_lossy(bytes);
    let mut out = HookFont::default();
    let mut section = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].trim().to_ascii_lowercase();
            continue;
        }
        if section != "font" {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value
            .trim()
            .trim_matches(|ch| ch == '"' || ch == '\'')
            .trim();
        if value.is_empty() {
            continue;
        }
        match key.as_str() {
            "fontname" => out.name = Some(value.to_string()),
            "fontfilename" => out.file = Some(value.to_string()),
            _ => {}
        }
    }
    out
}

/// 从 `codecs.json` 读取显式 profile。
///
/// 只认 `default{script,text,name}` 与可选 `tunnel{path}`;逐文件/逐载荷
/// 配置已删除(与官方 CHS 同构:整包一个编码)。
fn read_manifest(project: &Project) -> Result<Option<PatchProfile>, Failure> {
    let Some(bytes) = read_any(project, b"codecs.json", "codecs.json")? else {
        return Ok(None);
    };
    let text = std::str::from_utf8(&bytes)
        .map_err(|e| Failure::runtime(format!("codecs.json 不是 UTF-8: {e}")))?;
    let value = json::parse(text)
        .map_err(|e| Failure::runtime(format!("codecs.json 解析失败: {e}")))?;
    let object = value
        .as_object()
        .ok_or_else(|| Failure::runtime("codecs.json 顶层必须是对象"))?;

    let mut profile = PatchProfile {
        kind: PatchKind::Gbk,
        source: PatchSource::Manifest,
        evidence: vec!["manifest: codecs.json".to_string()],
        ..PatchProfile::default()
    };
    if let Some(default) = object.get("default") {
        let cfg = parse_codec_object(default)?;
        profile.execution = cfg.script;
        profile.text_default = cfg.text;
        profile.name = cfg.name;
    }
    // 三域全是 CP932 = 原版声明。
    if profile.execution == Codec::Cp932
        && profile.text_default == Codec::Cp932
        && profile.name == Codec::Cp932
        && object.get("tunnel").is_none()
    {
        profile.kind = PatchKind::Original;
    }
    if let Some(tunnel) = object.get("tunnel") {
        let obj = tunnel
            .as_object()
            .ok_or_else(|| Failure::runtime("codecs.json `tunnel` 必须是对象"))?;
        let path = obj
            .get("path")
            .and_then(json::Value::as_str)
            .ok_or_else(|| Failure::runtime("codecs.json tunnel.path 缺失"))?;
        let table = read_tunnel(project, Some(path))?
            .ok_or_else(|| Failure::runtime(format!("tunnel 表不存在: {path}")))?;
        profile.tunnel = Some(table);
        profile.kind = PatchKind::Tunnel;
        profile.execution = Codec::Cp932;
        profile.text_default = Codec::Cp932;
    }
    Ok(Some(profile))
}

fn parse_codec_object(value: &json::Value) -> Result<CodecConfig, Failure> {
    let object = value
        .as_object()
        .ok_or_else(|| Failure::runtime("codec 配置项必须是对象"))?;
    let mut cfg = CodecConfig::default();
    for (key, value) in object {
        let name = value
            .as_str()
            .ok_or_else(|| Failure::runtime(format!("codec 字段 `{key}` 必须是字符串")))?;
        let codec = Codec::from_name(name.trim())
            .map_err(|d| Failure::runtime(format!("codec 字段 `{key}` 非法: {d}")))?;
        match key.as_str() {
            "script" => cfg.script = codec,
            "text" => cfg.text = codec,
            "name" => cfg.name = codec,
            _ => {}
        }
    }
    Ok(cfg)
}

/// `YURIS_TOOLS` 的字体验证 DLL 签名：PDB 路径或导入名。
fn has_yuris_tools_signature(project: &Project) -> bool {
    let Ok(entries) = std::fs::read_dir(project.root()) else {
        return false;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        if !matches!(ext.to_ascii_lowercase().as_str(), "dll" | "exe") {
            continue;
        }
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        if meta.len() > 16 * 1024 * 1024 {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        if bytes
            .windows(b"yuris_tools\\ChangeFont".len())
            .any(|w| w == b"yuris_tools\\ChangeFont")
            || bytes
                .windows(b"MyDll.pdb".len())
                .any(|w| w == b"MyDll.pdb")
        {
            return true;
        }
    }
    false
}

/// 读取游戏根的 `sjis_ext.bin`(SJIS 隧道表);缺失返回 `None`。
///
/// 文本导出/诊断等只读消费者用它把隧道码回解成 Unicode;编码路线本身仍由
/// [`PatchSelection`] 决定。
pub fn load_tunnel(project: &Project) -> Result<Option<TunnelTable>, Failure> {
    read_tunnel(project, None)
}

fn read_tunnel(project: &Project, path: Option<&str>) -> Result<Option<TunnelTable>, Failure> {
    let path = path.unwrap_or("sjis_ext.bin");
    let Some(bytes) = read_any(project, path.as_bytes(), path)? else {
        return Ok(None);
    };
    if bytes.len() > 1024 * 1024 {
        return Err(Failure::runtime(format!("{path} 超过 1MiB")));
    }
    let table = TunnelTable::from_bytes(&bytes)
        .map_err(|d| Failure::runtime(format!("解析 {path} 失败: {d}")))?;
    Ok((!table.is_empty()).then_some(table))
}

fn read_any(
    project: &Project,
    key: &[u8],
    root_name: &str,
) -> Result<Option<Vec<u8>>, Failure> {
    let source = project.open_source()?;
    if let Ok(key) = LookupKey::new(key, CODEC) {
        if let Ok(bytes) = source.open(&key) {
            return Ok(Some(bytes));
        }
    }
    let path = project.root().join(root_name);
    if path.is_file() {
        return std::fs::read(&path)
            .map(Some)
            .map_err(|e| Failure::runtime(format!("读 {} 失败: {e}", path.display())));
    }
    Ok(None)
}

/// 只读工具:打印识别结果。
pub fn describe(project: &Project) -> Result<String, Failure> {
    let profile = detect(project)?;
    Ok(format!("{} evidence={:?}", profile.summary(), profile.evidence))
}

#[cfg(test)]
mod patch_font_tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        // 与项目其余测试同一约定:落在仓库内 `target/test-tmp`,避免系统
        // TEMP 在受限环境不可写。
        let base = std::env::var_os("YURIS_TEST_TMP")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp")
            });
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = base.join(format!(
            "yu-patch-font-{tag}-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample_hook_ini() -> &'static [u8] {
        // 与真实补丁同形(浅冬 FONTCHANGER):注释、空行、其它段、CRLF。
        b"# \xe4\xb8\x8d\xe9\x9c\x80\xe8\xa6\x81\xe7\x9a\x84\xe9\x83\xa8\xe5\x88\x86\xe5\x8f\xaf\xe4\xbb\xa5\xe7\x9b\xb4\xe6\x8e\xa5\xe5\x88\xa0\xe6\x8e\x89\r\n\
          [GLOBAL]\r\nDEBUG=0\r\n\r\n\
          [FONT]\r\n\
          # \xe5\xad\x97\xe4\xbd\x93\xe5\x90\x8d\r\n\
          FONTNAME=qiandong\r\n\
          FONTFILENAME=qiandong.ttf\r\n\
          CHARSET=128\r\n\r\n\
          [WINDOW]\r\nENABLE=0\r\n"
    }

    #[test]
    fn parses_font_section_case_insensitively() {
        let hook = parse_hook_font(sample_hook_ini());
        assert_eq!(hook.name.as_deref(), Some("qiandong"));
        assert_eq!(hook.file.as_deref(), Some("qiandong.ttf"));
    }

    #[test]
    fn ignores_font_keys_outside_font_section() {
        let ini = b"[GLOBAL]\nFONTFILENAME=wrong.ttf\n[FONT]\nfontfilename=ok.ttf\n";
        let hook = parse_hook_font(ini);
        assert_eq!(hook.file.as_deref(), Some("ok.ttf"));
    }

    #[test]
    fn strips_quotes_around_values() {
        let ini = b"[FONT]\nFONTNAME=\"qiandong\"\nFONTFILENAME='fonts\\\\qiandong.ttf'\n";
        let hook = parse_hook_font(ini);
        assert_eq!(hook.name.as_deref(), Some("qiandong"));
        assert_eq!(hook.file.as_deref(), Some("fonts\\\\qiandong.ttf"));
    }

    #[test]
    fn no_hook_ini_means_no_font() {
        let root = temp_root("none");
        assert_eq!(detect_patch_font_in(&root).unwrap(), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn detects_font_next_to_hook_ini() {
        let root = temp_root("direct");
        std::fs::write(root.join("hook.ini"), sample_hook_ini()).unwrap();
        std::fs::write(root.join("qiandong.ttf"), b"not a real font").unwrap();
        let font = detect_patch_font_in(&root).unwrap().unwrap();
        assert_eq!(font.path, root.join("qiandong.ttf"));
        assert_eq!(font.declared_name.as_deref(), Some("qiandong"));
        assert!(font.source.contains("FONTFILENAME=qiandong.ttf"));
        assert_eq!(font.load().unwrap(), b"not a real font");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn detects_font_in_fonts_subdir_and_case_insensitive_hook_name() {
        let root = temp_root("subdir");
        std::fs::create_dir_all(root.join("fonts")).unwrap();
        std::fs::write(root.join("Hook.ini"), sample_hook_ini()).unwrap();
        std::fs::write(root.join("fonts/qiandong.ttf"), b"font").unwrap();
        let font = detect_patch_font_in(&root).unwrap().unwrap();
        assert_eq!(font.path, root.join("fonts/qiandong.ttf"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn detects_font_in_development_data_layout() {
        let root = temp_root("data-layout");
        std::fs::create_dir_all(root.join("data")).unwrap();
        std::fs::write(root.join("data/hook.ini"), sample_hook_ini()).unwrap();
        std::fs::write(root.join("data/qiandong.ttf"), b"font").unwrap();
        let font = detect_patch_font_in(&root).unwrap().unwrap();
        assert_eq!(font.path, root.join("data/qiandong.ttf"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_declared_font_is_explicit_error() {
        let root = temp_root("missing");
        std::fs::write(root.join("hook.ini"), sample_hook_ini()).unwrap();
        let err = detect_patch_font_in(&root).unwrap_err().to_string();
        assert!(err.contains("qiandong.ttf"), "报错需含声明文件名: {err}");
        assert!(err.contains("找不到文件"), "报错需说明未找到: {err}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn empty_font_file_is_explicit_error() {
        let root = temp_root("empty");
        std::fs::write(root.join("hook.ini"), sample_hook_ini()).unwrap();
        std::fs::write(root.join("qiandong.ttf"), b"").unwrap();
        let err = detect_patch_font_in(&root).unwrap_err().to_string();
        assert!(err.contains("空文件"), "{err}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn hook_without_font_section_is_not_a_font_patch() {
        let root = temp_root("nofont");
        std::fs::write(root.join("hook.ini"), b"[GLOBAL]\nDEBUG=0\n").unwrap();
        assert_eq!(detect_patch_font_in(&root).unwrap(), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn static_lead_table_detects_each_codec() {
        for codec in [Codec::Cp932, Codec::Gbk, Codec::Big5] {
            let mut bytes = vec![0xFFu8; 37];
            bytes.extend_from_slice(&lead_table(codec));
            bytes.extend_from_slice(&[0xAAu8; 19]);
            assert_eq!(
                static_codec_in_bytes(&bytes),
                Some(codec),
                "{} lead table",
                codec.as_str()
            );
        }
    }

    #[test]
    fn static_lead_table_absent_or_ambiguous_is_none() {
        assert_eq!(static_codec_in_bytes(&[0u8; 512]), None);
        let mut both = Vec::new();
        both.extend_from_slice(&lead_table(Codec::Cp932));
        both.extend_from_slice(&lead_table(Codec::Gbk));
        assert_eq!(static_codec_in_bytes(&both), None, "两种表同时命中不猜");
    }
}

#[cfg(test)]
mod static_codec_scan_tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let base = std::env::var_os("YURIS_TEST_TMP")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp")
            });
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = base.join(format!(
            "yu-patch-static-{tag}-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 合成 555 工程:`data/ysbin/ysc.ybn` + 根目录 `(文件名, 编码, 额外体积)`。
    fn synthetic_555(tag: &str, exes: &[(&str, Codec, usize)]) -> (Project, PathBuf) {
        let root = temp_root(tag);
        let data = root.join("data");
        std::fs::create_dir_all(data.join("ysbin")).unwrap();
        let mut header = b"YSCM".to_vec();
        header.extend_from_slice(&555u32.to_le_bytes());
        std::fs::write(data.join("ysbin/ysc.ybn"), header).unwrap();
        for (name, codec, padding) in exes {
            let mut bytes = vec![0u8; *padding];
            bytes.extend_from_slice(&lead_table(*codec));
            std::fs::write(root.join(name), bytes).unwrap();
        }
        let project = Project::discover(&root, None).unwrap();
        (project, root)
    }

    #[test]
    fn helper_exe_names_are_classified() {
        for name in [
            "游戏设置.exe",
            "settings.exe",
            "Settings.exe",
            "inst.exe",
            "install.exe",
            "ysicon.exe",
            "エンジン設定.exe",
        ] {
            assert!(is_helper_exe(Path::new(name)), "{name} 应判为 helper");
        }
        for name in [
            "MainGame.exe",
            "Sample5.exe",
            "ゲーム本体.exe",
            "sample.exe",
        ] {
            assert!(!is_helper_exe(Path::new(name)), "{name} 不是 helper");
        }
    }

    #[test]
    fn helper_exe_encoding_does_not_poison_or_override_main_exe() {
        // <v555-1> 实测:主 exe 与 `游戏设置.exe` 同为 CP932;即使 helper 是另一种
        // 编码,也不能让主 exe 的判定变成 None 或被覆盖。
        let (project, root) = synthetic_555(
            "helper",
            &[
                ("MainGame.exe", Codec::Cp932, 1024),
                ("游戏设置.exe", Codec::Gbk, 0),
            ],
        );
        let (codec, evidence) = static_codec_for_v555(&project).expect("main exe 静态表");
        assert_eq!(codec, Codec::Cp932);
        assert!(evidence.contains("MainGame.exe"), "{evidence}");
        assert!(!evidence.contains("游戏设置.exe"), "{evidence}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn main_exe_conflict_is_none() {
        let (project, root) = synthetic_555(
            "conflict",
            &[
                ("main_a.exe", Codec::Cp932, 1024),
                ("main_b.exe", Codec::Gbk, 512),
            ],
        );
        assert_eq!(static_codec_for_v555(&project), None, "主 exe 编码冲突不猜");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 冲突裁决只看完整落在窗口内的 `0x4d` 字面量:
    /// <v555-sample> 的 GBK 数据在 GBK 表下闭合、在 CP932 表下被 `e0`
    /// 吞掉结束引号。样本不足/两表打平 ⇒ 不猜。
    #[test]
    fn literal_scores_pick_the_table_that_closes_gbk_text() {
        let pc3 = b"\x22\xd1\xe0\x22"; // "姓"(GBK d1 e0)
        let mut gbk = LiteralScore::default();
        gbk.score_literal(pc3, Codec::Gbk);
        assert_eq!((gbk.closed, gbk.unterminated, gbk.score), (1, 0, 0));
        let mut cp932 = LiteralScore::default();
        cp932.score_literal(pc3, Codec::Cp932);
        assert_eq!((cp932.closed, cp932.unterminated), (0, 1));

        // s274 pc0 的真实 GBK 长字面量:CP932 下 `fb 22` 吞引号。
        let long = b"\x22\xa1\xbe\xb9\xb2\xcd\xa8\xa1\xbf\xcc\xec\xca\xb9\
                     \xb3\xc1\xc3\xdf\xb5\xc4\xd0\xa1\xd5\xf2\xa1\xaa\xc3\xd7\
                     \xb5\xc2\xb1\xb4\xb6\xfb\x22";
        let mut gbk = LiteralScore::default();
        gbk.score_literal(long, Codec::Gbk);
        let mut cp932 = LiteralScore::default();
        cp932.score_literal(long, Codec::Cp932);
        assert_eq!(gbk.score, 0, "GBK 应闭合");
        assert!(cp932.score >= 8, "CP932 应未闭合: {}", cp932.score);
    }

    #[test]
    fn script_names_map_to_script_ids() {
        assert_eq!(script_id_of_name(b"yst00273.ybn"), Some(273));
        assert_eq!(script_id_of_name(b"yst00000.ybn"), Some(0));
        assert_eq!(script_id_of_name(b"ysc.ybn"), None);
        assert_eq!(script_id_of_name(b"yst273.ybn"), None);
        assert_eq!(script_id_of_name(b"yst00a73.ybn"), None);
    }
}
