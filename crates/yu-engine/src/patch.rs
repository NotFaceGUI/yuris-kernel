//! 文本编码统一入口(跨平台)。
//!
//! 补丁路线由**用户显式选择**,不再靠产物特判保证正确性:
//!
//! - [`PatchKind::Original`]:原版 CP932;
//! - [`PatchKind::Gbk`]:全局 GBK(官方 `_chs` 换首字节表同构);
//! - [`PatchKind::Big5`]:全局 Big5 / CP950(繁中旧作与部分汉化补丁);
//! - [`PatchKind::Tunnel`]:CP932 + `sjis_ext.bin` 隧道表。
//!
//! [`PatchKind::Auto`] 是便利值:只在宿主没给类型时按产物识别
//! (`codecs.json` → 隧道 → YURIS_TOOLS 签名),识别结果在 evidence 里
//! 标注来源,不参与正确性保证。
//!
//! 各宿主(桌面/serve/Android/wasm)共用这套类型 + [`PatchSelection`],
//! 只负责把用户选择/`codecs.json` 传进来。

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
        }
    }

    /// 解析宿主/CLI 值。`auto|original|gbk|big5|tunnel`;`cp932` 等同
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
            other => Err(format!(
                "未知补丁类型 `{other}`;可用:auto | original | gbk | big5 | tunnel"
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
            source: PatchSource::Default,
            evidence: Vec::new(),
        }
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
    pub fn profile(self, project: &Project) -> Result<PatchProfile, Failure> {
        match self.kind {
            PatchKind::Original => Ok(PatchProfile {
                kind: PatchKind::Original,
                source: PatchSource::Cli,
                evidence: vec!["cli: original".to_string()],
                ..PatchProfile::default()
            }),
            PatchKind::Gbk => Ok(PatchProfile {
                kind: PatchKind::Gbk,
                execution: Codec::Gbk,
                text_default: Codec::Gbk,
                name: Codec::Cp932,
                source: PatchSource::Cli,
                evidence: vec!["cli: gbk".to_string()],
                ..PatchProfile::default()
            }),
            PatchKind::Big5 => Ok(PatchProfile {
                kind: PatchKind::Big5,
                execution: Codec::Big5,
                text_default: Codec::Big5,
                name: Codec::Cp932,
                source: PatchSource::Cli,
                evidence: vec!["cli: big5".to_string()],
                ..PatchProfile::default()
            }),
            PatchKind::Tunnel => {
                let tunnel = read_tunnel(project, None)?.ok_or_else(|| {
                    Failure::runtime(
                        "补丁类型 tunnel 需要游戏根的 sjis_ext.bin(未找到)",
                    )
                })?;
                Ok(PatchProfile {
                    kind: PatchKind::Tunnel,
                    execution: Codec::Cp932,
                    text_default: Codec::Cp932,
                    name: Codec::Cp932,
                    tunnel: Some(tunnel),
                    source: PatchSource::Cli,
                    evidence: vec!["cli: tunnel".to_string()],
                    ..PatchProfile::default()
                })
            }
            PatchKind::Auto => detect(project),
        }
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
            "kind={} source={} execution={} text={} name={} tunnel={}",
            self.kind.as_str(),
            self.source.as_str(),
            self.execution.as_str(),
            self.text_default.as_str(),
            self.name.as_str(),
            self.tunnel.as_ref().is_some_and(|t| !t.is_empty())
        )
    }
}

/// `PatchKind::Auto` 的启发式识别(便利,不作为正确性保证)。
///
/// 顺序:`codecs.json` → `sjis_ext.bin` → YURIS_TOOLS 产物签名 → 原版。
pub fn detect(project: &Project) -> Result<PatchProfile, Failure> {
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

    Ok(PatchProfile::default())
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
