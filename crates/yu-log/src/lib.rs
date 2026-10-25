//! `yu-log` · 错误 + 日志一体包。
//!
//! 契约见 `docs/architecture.md` §8:
//! - 错误是结构化数据 [`Diag`],不是字符串;
//! - 稳定命名空间码见 [`code`];
//! - 日志显式注入 [`LogSink`],不存在隐藏全局(A3 确定性);
//! - JSONL 输出确定性、可逐字节往返。
//!
//! `clippy::result_large_err` 在此显式放行: [`Diag`] 承载边界归属、位置与
//! 因果链(architecture §8 的契约),压缩它会迫使调用方到处装箱或丢上下文。
#![allow(clippy::result_large_err)]

pub mod code;
pub mod console;
pub mod facade;
mod json;
pub mod sink;

pub use sink::{JsonlSink, LogSink, MemorySink, NullSink};

use std::borrow::Cow;
use std::fmt;

/// 严重级。语义对齐官方: `Unimplemented` = 已知语义未实装(挂号留痕);
/// `Recoverable` = 调用方可继续;`Fatal` = 当前操作无法继续。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Fatal,
    Recoverable,
    Unimplemented,
    Info,
}

impl Severity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Severity::Fatal => "Fatal",
            Severity::Recoverable => "Recoverable",
            Severity::Unimplemented => "Unimplemented",
            Severity::Info => "Info",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Self::parse_str(s)
    }

    /// 从 `as_str` 的输出解析(公开给把严重级存成字符串的序列化层,
    /// 如 `yu-gfx::FrameDiag`);未知值显式 None,不猜。
    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "Fatal" => Some(Severity::Fatal),
            "Recoverable" => Some(Severity::Recoverable),
            "Unimplemented" => Some(Severity::Unimplemented),
            "Info" => Some(Severity::Info),
            _ => None,
        }
    }
}

/// 边界归属。与 `architecture.md` §8 的码域对齐。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Boundary {
    Data,
    Format,
    Table,
    Script,
    Domain,
    Platform,
}

impl Boundary {
    pub const fn as_str(self) -> &'static str {
        match self {
            Boundary::Data => "Data",
            Boundary::Format => "Format",
            Boundary::Table => "Table",
            Boundary::Script => "Script",
            Boundary::Domain => "Domain",
            Boundary::Platform => "Platform",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "Data" => Some(Boundary::Data),
            "Format" => Some(Boundary::Format),
            "Table" => Some(Boundary::Table),
            "Script" => Some(Boundary::Script),
            "Domain" => Some(Boundary::Domain),
            "Platform" => Some(Boundary::Platform),
            _ => None,
        }
    }
}

/// 诊断位置。脚本位置、数据位置、表名三者共用一种可序列化表达。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Loc {
    Script {
        file: String,
        label: Option<String>,
        pc: u32,
        cmd: Option<String>,
    },
    Data {
        path: Vec<u8>,
        offset: u64,
    },
    Table {
        name: String,
    },
    None,
}

impl Loc {
    pub fn data(path: impl Into<Vec<u8>>, offset: u64) -> Self {
        Loc::Data {
            path: path.into(),
            offset,
        }
    }

    pub fn table(name: impl Into<String>) -> Self {
        Loc::Table { name: name.into() }
    }

    pub fn script(
        file: impl Into<String>,
        label: Option<String>,
        pc: u32,
        cmd: Option<String>,
    ) -> Self {
        Loc::Script {
            file: file.into(),
            label,
            pc,
            cmd,
        }
    }
}

/// 结构化诊断。`code` 允许 `'static` 常量,也允许从 JSONL 读回时拥有所有权。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diag {
    pub code: Cow<'static, str>,
    pub severity: Severity,
    pub boundary: Boundary,
    pub where_: Loc,
    pub detail: String,
    pub causes: Vec<Diag>,
}

impl Diag {
    pub fn new(code: &'static str, severity: Severity, boundary: Boundary) -> Self {
        Self {
            code: Cow::Borrowed(code),
            severity,
            boundary,
            where_: Loc::None,
            detail: String::new(),
            causes: Vec::new(),
        }
    }

    pub fn fatal(code: &'static str, boundary: Boundary) -> Self {
        Self::new(code, Severity::Fatal, boundary)
    }

    pub fn recoverable(code: &'static str, boundary: Boundary) -> Self {
        Self::new(code, Severity::Recoverable, boundary)
    }

    pub fn unimplemented(code: &'static str, boundary: Boundary) -> Self {
        Self::new(code, Severity::Unimplemented, boundary)
    }

    pub fn info(code: &'static str, boundary: Boundary) -> Self {
        Self::new(code, Severity::Info, boundary)
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = detail.into();
        self
    }

    pub fn at(mut self, loc: Loc) -> Self {
        self.where_ = loc;
        self
    }

    pub fn at_data(self, path: impl Into<Vec<u8>>, offset: u64) -> Self {
        self.at(Loc::data(path, offset))
    }

    pub fn at_table(self, name: impl Into<String>) -> Self {
        self.at(Loc::table(name))
    }

    pub fn caused_by(mut self, cause: Diag) -> Self {
        self.causes.push(cause);
        self
    }

    /// 是否是"未找到"这一可继续的诊断。`Mount` 用它决定是否继续向下层查找。
    pub fn is_missing(&self) -> bool {
        self.code == code::vfs::MISSING
    }

    /// 确定性 JSON(单行)。
    pub fn to_json_line(&self) -> String {
        let mut out = String::new();
        self.write_json(&mut out);
        out
    }

    /// 确定性 JSON(单行),前置一个 `"sub"` 键(JSONL 混合流用)。
    ///
    /// 字段集与 [`Self::to_json_line`] 完全一致,只是对象多一个首键;
    /// [`Self::from_json_line`] 忽略未知键,往返契约不受影响。
    pub fn to_json_line_with_sub(&self, sub: &str) -> String {
        let mut out = String::with_capacity(160);
        out.push_str("{\"sub\":");
        json::write_string(&mut out, sub);
        out.push(',');
        self.write_json_fields(&mut out);
        out.push('}');
        out
    }

    fn write_json(&self, out: &mut String) {
        out.push('{');
        self.write_json_fields(out);
        out.push('}');
    }

    /// 花括号之间的字段(供 `to_json_line` 与 `to_json_line_with_sub` 共用)。
    fn write_json_fields(&self, out: &mut String) {
        out.push_str("\"code\":");
        json::write_string(out, self.code.as_ref());
        out.push_str(",\"severity\":");
        json::write_string(out, self.severity.as_str());
        out.push_str(",\"boundary\":");
        json::write_string(out, self.boundary.as_str());
        out.push_str(",\"where\":");
        match &self.where_ {
            Loc::Script {
                file,
                label,
                pc,
                cmd,
            } => {
                out.push_str("{\"kind\":\"script\",\"file\":");
                json::write_string(out, file);
                out.push_str(",\"label\":");
                match label {
                    Some(v) => json::write_string(out, v),
                    None => out.push_str("null"),
                }
                out.push_str(",\"pc\":");
                out.push_str(&pc.to_string());
                out.push_str(",\"cmd\":");
                match cmd {
                    Some(v) => json::write_string(out, v),
                    None => out.push_str("null"),
                }
                out.push('}');
            }
            Loc::Data { path, offset } => {
                out.push_str("{\"kind\":\"data\",\"path\":");
                json::write_bytes(out, path);
                out.push_str(",\"offset\":");
                out.push_str(&offset.to_string());
                out.push('}');
            }
            Loc::Table { name } => {
                out.push_str("{\"kind\":\"table\",\"name\":");
                json::write_string(out, name);
                out.push('}');
            }
            Loc::None => out.push_str("{\"kind\":\"none\"}"),
        }
        out.push_str(",\"detail\":");
        json::write_string(out, &self.detail);
        out.push_str(",\"causes\":[");
        for (i, cause) in self.causes.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            cause.write_json(out);
        }
        out.push(']');
    }

    /// 解析 [`Self::to_json_line`] 产生的单行 JSON。
    pub fn from_json_line(line: &str) -> Result<Self, String> {
        let value = json::parse(line)?;
        Self::from_json(&value)
    }

    fn from_json(value: &json::Json) -> Result<Self, String> {
        let obj = json::as_obj(value)?;
        let code = json::as_str(required(obj, "code")?)?.to_string();
        let severity_text = json::as_str(required(obj, "severity")?)?;
        let severity = Severity::parse(severity_text)
            .ok_or_else(|| format!("unknown severity `{severity_text}`"))?;
        let boundary_text = json::as_str(required(obj, "boundary")?)?;
        let boundary = Boundary::parse(boundary_text)
            .ok_or_else(|| format!("unknown boundary `{boundary_text}`"))?;
        let where_ = parse_loc(required(obj, "where")?)?;
        let detail = json::as_str(required(obj, "detail")?)?.to_string();
        let causes = json::as_arr(required(obj, "causes")?)?
            .iter()
            .map(Self::from_json)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            code: Cow::Owned(code),
            severity,
            boundary,
            where_,
            detail,
            causes,
        })
    }
}

fn required<'a>(
    obj: &'a [(String, json::Json)],
    key: &str,
) -> Result<&'a json::Json, String> {
    json::get(obj, key).ok_or_else(|| format!("missing field `{key}`"))
}

fn parse_loc(value: &json::Json) -> Result<Loc, String> {
    let obj = json::as_obj(value)?;
    let kind = json::as_str(required(obj, "kind")?)?;
    match kind {
        "script" => {
            let file = json::as_str(required(obj, "file")?)?.to_string();
            let label = match required(obj, "label")? {
                json::Json::Null => None,
                other => Some(json::as_str(other)?.to_string()),
            };
            let pc = json::as_i64(required(obj, "pc")?)?;
            let pc = u32::try_from(pc).map_err(|_| format!("pc out of range: {pc}"))?;
            let cmd = match required(obj, "cmd")? {
                json::Json::Null => None,
                other => Some(json::as_str(other)?.to_string()),
            };
            Ok(Loc::script(file, label, pc, cmd))
        }
        "data" => {
            let path = json::bytes_from_string(json::as_str(required(obj, "path")?)?)?;
            let offset = json::as_i64(required(obj, "offset")?)?;
            let offset = u64::try_from(offset)
                .map_err(|_| format!("offset out of range: {offset}"))?;
            Ok(Loc::data(path, offset))
        }
        "table" => Ok(Loc::table(
            json::as_str(required(obj, "name")?)?.to_string(),
        )),
        "none" => Ok(Loc::None),
        other => Err(format!("unknown location kind `{other}`")),
    }
}

impl fmt::Display for Diag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}[{}] ",
            self.severity.as_str(),
            self.code.as_ref()
        )?;
        match &self.where_ {
            Loc::Script {
                file,
                label,
                pc,
                cmd,
            } => {
                write!(f, "Script({file}")?;
                if let Some(label) = label {
                    write!(f, "#{label}")?;
                }
                write!(f, "@pc={pc}")?;
                if let Some(cmd) = cmd {
                    write!(f, " cmd={cmd}")?;
                }
                write!(f, ")")?;
            }
            Loc::Data { path, offset } => {
                write!(
                    f,
                    "Data({:?}@{offset:#x})",
                    String::from_utf8_lossy(path)
                )?;
            }
            Loc::Table { name } => write!(f, "Table({name})")?,
            Loc::None => write!(f, "None")?,
        }
        if !self.detail.is_empty() {
            write!(f, ": {}", self.detail)?;
        }
        for cause in &self.causes {
            write!(f, "\n  caused by: {cause}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Diag {
        Diag::fatal(code::vfs::YPF_BAD_INDEX, Boundary::Data)
            .at_data(b"bn.ypf".as_slice(), 0x24)
            .with_detail("index does not close")
            .caused_by(
                Diag::recoverable(code::vfs::MISSING, Boundary::Data)
                    .at_data(vec![0x83, 0x5c, 0xff], 7)
                    .with_detail("entry not found"),
            )
    }

    #[test]
    fn diag_json_roundtrip() {
        let diag = sample();
        let line = diag.to_json_line();
        let back = Diag::from_json_line(&line).unwrap();
        assert_eq!(diag, back);
        assert_eq!(line, back.to_json_line());
    }

    #[test]
    fn diag_json_script_loc_roundtrip() {
        let diag = Diag::unimplemented("rt.cmd.unknown", Boundary::Script).at(Loc::script(
            "ysbin/yst00001.ybn",
            Some("start".to_string()),
            42,
            Some("WAIT".to_string()),
        ));
        let back = Diag::from_json_line(&diag.to_json_line()).unwrap();
        assert_eq!(diag, back);
    }

    #[test]
    fn display_keeps_causal_chain() {
        let text = sample().to_string();
        assert!(text.contains("vfs.ypf.bad_index"));
        assert!(text.contains("caused by"));
        assert!(text.contains("vfs.missing"));
    }

    #[test]
    fn all_codes_are_registered() {
        for c in code::ALL_CODES {
            assert!(code::is_registered(c), "unregistered: {c}");
        }
    }

    #[test]
    fn rejects_malformed_json() {
        assert!(Diag::from_json_line("{\"code\":\"x\"}").is_err());
        assert!(Diag::from_json_line("not json").is_err());
    }
}
