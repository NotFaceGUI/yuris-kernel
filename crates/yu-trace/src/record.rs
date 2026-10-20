//! 记录封套、载荷类型与稳定 JSONL 读写。
//!
//! 协议定义见 `SCHEMA.md`(本 crate 的唯一定义)。要点:
//!
//! - 每条记录固定六字段,顺序固定:`schema, seq, frame, sub, kind, payload`;
//! - `seq` 从 0 起严格连续递增;`seq = 0` 且 `sub = "meta"`、`kind = "header"`
//!   的记录是头部,先于流比较;
//! - 载荷键顺序固定,输出不经过任何无序容器;
//! - 未知 `schema` / 未知 `kind` / 未知字段 = `trace.schema`;缺字段 = `trace.parse`。
//!
//! 本模块只拥有协议,不依赖 `yu_rt::json` 的内部序列化。

use std::fs;
use std::path::Path;

use yu_log::{Boundary, Diag};
use yu_vfs::EntryKind;

use crate::error;
use crate::json::{self, FieldError, Fields, Json};

/// 当前协议版本。任何字段增删或语义变化都必须递增,并在 `SCHEMA.md` 留痕。
///
/// - v1:2026-10-19 初版(边界①②)。
/// - v2:2026-10-19 审查修订 —— header 补全启动配置(script_key/debug_mode/argv 摘要),
///   否则"用 header 重跑"无法闭合;`trace.length` 单列记录数不等。
/// - v3:2026-10-20 producer-neutral —— header 增加 `producer`(kernel/official/
///   采集器版本/样本 hash)与 `clock`(tick_at 的 now_ms 序列);③
///   `frame/frame_description` 成为真实记录;④ `platform/platform_call` 继续拒绝。
pub const SCHEMA_VERSION: u32 = 3;

/// 子系统。③(`frame`)已实现;④(`platform`)仍为占位,本版本拒绝其记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Sub {
    Meta,
    Vfs,
    Vm,
    /// ③ 帧描述(M3,未实现)。
    Frame,
    /// ④ 平台命令(M3,未实现)。
    Platform,
}

impl Sub {
    pub const fn as_str(self) -> &'static str {
        match self {
            Sub::Meta => "meta",
            Sub::Vfs => "vfs",
            Sub::Vm => "vm",
            Sub::Frame => "frame",
            Sub::Platform => "platform",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "meta" => Some(Sub::Meta),
            "vfs" => Some(Sub::Vfs),
            "vm" => Some(Sub::Vm),
            "frame" => Some(Sub::Frame),
            "platform" => Some(Sub::Platform),
            _ => None,
        }
    }
}

/// 记录种类。变体与 `sub` 的对应关系由 [`Kind::sub`] 固定,解析时强制校验。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Header,
    VfsOpen,
    VfsExists,
    VfsList,
    VmFrameStart,
    VmFrameEnd,
    VmCommand,
    VmJump,
    VmTaskSpawn,
    VmTaskFinish,
    VmWait,
    VmText,
    VmReturnCode,
    VmFps,
    VmSystemCall,
    VmDomainCall,
    VmEnd,
    VmDiag,
    /// ③ 帧描述:与 `yu-gfx` 的字段解耦,由调用方传入规范化 display JSON 或摘要。
    FrameDescription,
    /// ④ 平台命令(M3,未实现):本版本无法写出也无法解析。
    PlatformCall,
}

impl Kind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Kind::Header => "header",
            Kind::VfsOpen => "open",
            Kind::VfsExists => "exists",
            Kind::VfsList => "list",
            Kind::VmFrameStart => "frame_start",
            Kind::VmFrameEnd => "frame_end",
            Kind::VmCommand => "command",
            Kind::VmJump => "jump",
            Kind::VmTaskSpawn => "task_spawn",
            Kind::VmTaskFinish => "task_finish",
            Kind::VmWait => "wait",
            Kind::VmText => "text",
            Kind::VmReturnCode => "return_code",
            Kind::VmFps => "fps",
            Kind::VmSystemCall => "system_call",
            Kind::VmDomainCall => "domain_call",
            Kind::VmEnd => "end",
            Kind::VmDiag => "diag",
            Kind::FrameDescription => "frame_description",
            Kind::PlatformCall => "platform_call",
        }
    }

    pub const fn sub(self) -> Sub {
        match self {
            Kind::Header => Sub::Meta,
            Kind::VfsOpen | Kind::VfsExists | Kind::VfsList => Sub::Vfs,
            Kind::VmFrameStart
            | Kind::VmFrameEnd
            | Kind::VmCommand
            | Kind::VmJump
            | Kind::VmTaskSpawn
            | Kind::VmTaskFinish
            | Kind::VmWait
            | Kind::VmText
            | Kind::VmReturnCode
            | Kind::VmFps
            | Kind::VmSystemCall
            | Kind::VmDomainCall
            | Kind::VmEnd
            | Kind::VmDiag => Sub::Vm,
            Kind::FrameDescription => Sub::Frame,
            Kind::PlatformCall => Sub::Platform,
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "header" => Some(Kind::Header),
            "open" => Some(Kind::VfsOpen),
            "exists" => Some(Kind::VfsExists),
            "list" => Some(Kind::VfsList),
            "frame_start" => Some(Kind::VmFrameStart),
            "frame_end" => Some(Kind::VmFrameEnd),
            "command" => Some(Kind::VmCommand),
            "jump" => Some(Kind::VmJump),
            "task_spawn" => Some(Kind::VmTaskSpawn),
            "task_finish" => Some(Kind::VmTaskFinish),
            "wait" => Some(Kind::VmWait),
            "text" => Some(Kind::VmText),
            "return_code" => Some(Kind::VmReturnCode),
            "fps" => Some(Kind::VmFps),
            "system_call" => Some(Kind::VmSystemCall),
            "domain_call" => Some(Kind::VmDomainCall),
            "end" => Some(Kind::VmEnd),
            "diag" => Some(Kind::VmDiag),
            "frame_description" => Some(Kind::FrameDescription),
            "platform_call" => Some(Kind::PlatformCall),
            _ => None,
        }
    }
}

/// 记录生产者。跨 producer diff 只比较语义字段,不比较 producer 身份字符串。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProducerKind {
    /// 本仓库内核(`yu-trace` 录制)。
    Kernel,
    /// 官方引擎(事件级或屏幕级采集器)。
    Official,
}

impl ProducerKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            ProducerKind::Kernel => "kernel",
            ProducerKind::Official => "official",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "kernel" => Some(ProducerKind::Kernel),
            "official" => Some(ProducerKind::Official),
            _ => None,
        }
    }
}

/// header 中的 producer 元数据。
///
/// `version` 在 kernel 侧是内核版本,在 official 侧是**采集器版本**。
/// `sample_sha256` 是两边必须能对齐的样本身份(官方数据包/脚本集合的摘要);
/// 它不参与"内核版本/prose/表来源"的整行比较,但在两边都存在时参与
/// producer-neutral 的 header 投影比较。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProducerInfo {
    pub kind: ProducerKind,
    pub name: String,
    pub version: String,
    pub sample_sha256: Option<String>,
}

impl ProducerInfo {
    /// 内核 producer(`yu-trace` 版本)。
    pub fn kernel() -> Self {
        Self {
            kind: ProducerKind::Kernel,
            name: "yu-trace".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            sample_sha256: None,
        }
    }

    /// 官方 producer(采集器名称/版本)。
    pub fn official(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            kind: ProducerKind::Official,
            name: name.into(),
            version: version.into(),
            sample_sha256: None,
        }
    }

    /// 附加/覆盖样本摘要。
    pub fn with_sample_sha256(mut self, sha256: impl Into<String>) -> Self {
        self.sample_sha256 = Some(sha256.into());
        self
    }

    fn validate(&self) -> Result<(), String> {
        if self.name.is_empty() {
            return Err("producer name must not be empty".to_string());
        }
        if self.version.is_empty() {
            return Err("producer version must not be empty".to_string());
        }
        if let Some(hash) = &self.sample_sha256 {
            validate_sha256(hash).map_err(|detail| format!("producer sample_sha256: {detail}"))?;
        }
        Ok(())
    }
}

/// 录制时钟模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ClockMode {
    /// 没有推进过任何 tick(只有启动链)。
    None,
    /// 只使用了 [`yu_rt::Vm::tick`](内部时钟)。
    Tick,
    /// 只使用了 `tick_at`(显式 now_ms)。
    TickAt,
    /// 两种混用:逐 tick 的 `now_ms` 列表用 `null` 表示内部时钟。
    Mixed,
}

impl ClockMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            ClockMode::None => "none",
            ClockMode::Tick => "tick",
            ClockMode::TickAt => "tick_at",
            ClockMode::Mixed => "mixed",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "none" => Some(ClockMode::None),
            "tick" => Some(ClockMode::Tick),
            "tick_at" => Some(ClockMode::TickAt),
            "mixed" => Some(ClockMode::Mixed),
            _ => None,
        }
    }
}

/// `tick_at` 的 now_ms 序列(回放闭包)。
///
/// - `None` / `Tick`:不携带逐 tick 列表(`now_ms` 为空);
/// - `TickAt`:`now_ms` 每项都是 `Some`;
/// - `Mixed`:按调用顺序,`None` = 该次使用 `tick()`。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClockSummary {
    pub mode: ClockMode,
    pub now_ms: Vec<Option<u32>>,
}

impl ClockSummary {
    pub fn none() -> Self {
        Self {
            mode: ClockMode::None,
            now_ms: Vec::new(),
        }
    }

    /// 从逐 tick 的 `now_ms` 序列推导模式(空序列 = `none`)。
    pub fn from_ticks(ticks: &[Option<u32>]) -> Self {
        if ticks.is_empty() {
            return Self::none();
        }
        let explicit = ticks.iter().filter(|tick| tick.is_some()).count();
        if explicit == 0 {
            Self {
                mode: ClockMode::Tick,
                now_ms: Vec::new(),
            }
        } else if explicit == ticks.len() {
            Self {
                mode: ClockMode::TickAt,
                now_ms: ticks.to_vec(),
            }
        } else {
            Self {
                mode: ClockMode::Mixed,
                now_ms: ticks.to_vec(),
            }
        }
    }

    /// 第 `index` 次 tick 的时钟:`Some(None)` = 内部时钟;`None` = 没有逐 tick 列表。
    pub fn now_ms_at(&self, index: usize) -> Option<Option<u32>> {
        self.now_ms.get(index).copied()
    }

    fn validate(&self) -> Result<(), String> {
        match self.mode {
            ClockMode::None | ClockMode::Tick => {
                if !self.now_ms.is_empty() {
                    return Err(format!(
                        "clock mode `{}` must not carry a now_ms list",
                        self.mode.as_str()
                    ));
                }
            }
            ClockMode::TickAt => {
                if self.now_ms.is_empty() {
                    return Err("clock mode `tick_at` requires a now_ms list".to_string());
                }
                if self.now_ms.iter().any(Option::is_none) {
                    return Err(
                        "clock mode `tick_at` requires every now_ms to be an integer".to_string(),
                    );
                }
            }
            ClockMode::Mixed => {
                if self.now_ms.is_empty() {
                    return Err("clock mode `mixed` requires a now_ms list".to_string());
                }
                if !self.now_ms.iter().any(Option::is_none)
                    || !self.now_ms.iter().any(Option::is_some)
                {
                    return Err(
                        "clock mode `mixed` requires both implicit (null) and explicit now_ms"
                            .to_string(),
                    );
                }
            }
        }
        Ok(())
    }
}

/// 帧描述的等级。屏幕级记录只证明"画面像",不等价于事件级 L1。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FrameLevel {
    /// 事件级:调用方从帧语义层规范化出的 display JSON。
    Event,
    /// 屏幕级:官方截图的摘要(无 display JSON)。
    Screen,
}

impl FrameLevel {
    pub const fn as_str(self) -> &'static str {
        match self {
            FrameLevel::Event => "event",
            FrameLevel::Screen => "screen",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "event" => Some(FrameLevel::Event),
            "screen" => Some(FrameLevel::Screen),
            _ => None,
        }
    }
}

/// 规范化后的完整 display JSON。
///
/// 本 crate **不硬绑** `yu-gfx` 的字段:调用方传入任意合法 JSON,本类型只负责
/// 解析、规范序列化(键序/数组序保持调用方顺序,去掉空白)与内容寻址。
/// 写出与读回使用同一规范文本,因此同一输入两次写入逐字节一致。
#[derive(Clone, Debug, PartialEq)]
pub struct FrameDisplay {
    value: crate::json::Json,
}

impl FrameDisplay {
    /// 从 UTF-8 JSON 字节构造(必须是单个 JSON 值)。
    pub fn from_json_bytes(bytes: &[u8]) -> Result<Self, Diag> {
        let text = std::str::from_utf8(bytes).map_err(|e| {
            Diag::fatal(crate::error::code::PARSE, Boundary::Format)
                .with_detail(format!("display JSON is not valid UTF-8: {e}"))
        })?;
        Self::from_json_text(text)
    }

    /// 从 JSON 文本构造。
    pub fn from_json_text(text: &str) -> Result<Self, Diag> {
        let value = json::parse(text).map_err(|detail| {
            Diag::fatal(crate::error::code::PARSE, Boundary::Format)
                .with_detail(format!("invalid display JSON: {detail}"))
        })?;
        Ok(Self { value })
    }

    /// 规范文本(无多余空白;对象键序与数组序保持调用方顺序)。
    pub fn canonical(&self) -> String {
        json::to_text(&self.value)
    }

    /// 规范文本的 SHA-256。
    pub fn sha256(&self) -> String {
        crate::hash::sha256_hex(self.canonical().as_bytes())
    }

    /// 解析后的 JSON 值(只读)。
    pub(crate) fn as_json(&self) -> &crate::json::Json {
        &self.value
    }
}

/// ③ 帧描述记录的内容。
#[derive(Clone, Debug, PartialEq)]
pub struct FrameSummary {
    pub frame: u64,
    pub level: FrameLevel,
    /// 规范化 display JSON 的 SHA-256;摘要级记录时由调用方提供。
    pub display_sha256: String,
    pub pass_count: u32,
    pub op_count: u64,
    pub executable_ops: u64,
    pub skipped_ops: u64,
    /// 完整 display JSON;摘要级记录为 `None`。
    pub display: Option<FrameDisplay>,
}

/// 调用方(gfx/后端/官方采集器)交给 `yu-trace` 的帧描述输入。
///
/// `display` 是**规范化后的** frame JSON 字节;本 crate 会再次解析并规范
/// 序列化(空白被去掉,键序/数组序保留)。`display_sha256` 可选:
/// - 给了 `display`:不传则自动计算;传了则必须等于规范文本的摘要;
/// - 没给 `display`:必须传 `display_sha256`(摘要级记录)。
#[derive(Clone, Debug, PartialEq)]
pub struct FrameDescriptionInput {
    pub frame: u64,
    pub level: FrameLevel,
    pub pass_count: u32,
    pub op_count: u64,
    pub executable_ops: u64,
    pub skipped_ops: u64,
    pub display: Option<Vec<u8>>,
    pub display_sha256: Option<String>,
}

impl FrameSummary {
    /// 校验并构造帧描述。失败返回 `trace.parse` / `trace.schema`。
    pub fn from_input(input: FrameDescriptionInput) -> Result<Self, Diag> {
        let (display, display_sha256) = match (input.display, input.display_sha256) {
            (Some(bytes), provided) => {
                let display = FrameDisplay::from_json_bytes(&bytes)?;
                let computed = display.sha256();
                if let Some(provided) = provided {
                    validate_sha256(&provided).map_err(|detail| {
                        Diag::fatal(crate::error::code::PARSE, Boundary::Format)
                            .with_detail(format!("display_sha256: {detail}"))
                    })?;
                    if provided != computed {
                        return Err(Diag::fatal(
                            crate::error::code::SCHEMA,
                            Boundary::Format,
                        )
                        .with_detail(format!(
                            "display_sha256 {provided} does not match canonical display JSON {computed}"
                        )));
                    }
                }
                (Some(display), computed)
            }
            (None, Some(hash)) => {
                validate_sha256(&hash).map_err(|detail| {
                    Diag::fatal(crate::error::code::PARSE, Boundary::Format)
                        .with_detail(format!("display_sha256: {detail}"))
                })?;
                (None, hash)
            }
            (None, None) => {
                return Err(
                    Diag::fatal(crate::error::code::PARSE, Boundary::Format).with_detail(
                        "frame description requires display JSON or display_sha256".to_string(),
                    ),
                )
            }
        };
        Ok(Self {
            frame: input.frame,
            level: input.level,
            display_sha256,
            pass_count: input.pass_count,
            op_count: input.op_count,
            executable_ops: input.executable_ops,
            skipped_ops: input.skipped_ops,
            display,
        })
    }
}

/// 64 位小写十六进制 SHA-256 的校验。
fn validate_sha256(text: &str) -> Result<(), String> {
    if text.len() != 64 {
        return Err(format!(
            "expected 64 lowercase hex digits, got {} chars",
            text.len()
        ));
    }
    if !text
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!("expected 64 lowercase hex digits, got {text:?}"));
    }
    Ok(())
}

/// 一次 vfs 调用的结果身份。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListEntry {
    pub name: Vec<u8>,
    pub kind: EntryKind,
    pub size: Option<u64>,
}

impl ListEntry {
    pub fn from_entry(entry: &yu_vfs::Entry) -> Self {
        Self {
            name: entry.name.clone(),
            kind: entry.kind,
            size: entry.size,
        }
    }
}

/// 启动配置摘要(全部来自注入的 [`yu_rt::BootConfig`],不含墙钟/主机信息)。
///
/// `argv` 只存**摘要**而不是值:命令行可能含绝对路径,而录制内容禁止出现
/// 绝对路径。回放时由调用方提供同一 `argv`,并用 [`BootSummary::to_config`]
/// 校验摘要。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BootSummary {
    pub entry: String,
    pub fps: u32,
    pub start_ms: u32,
    pub frame_ms: u32,
    /// 脚本 XOR 密钥(4 字节,写成 8 位小写十六进制)。
    pub script_key: [u8; 4],
    pub debug_mode: bool,
    pub argv_len: u64,
    pub argv_sha256: String,
    /// 宿主注入项(影响系统变量读取,必须进 replay)。
    pub host: HostSummary,
}

/// 宿主注入项摘要。
///
/// 标量直接记录;路径只记摘要(路径可能是绝对路径,禁止进录制内容),
/// 回放时由调用方提供同一 `HostPaths` 并校验。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostSummary {
    pub screen_color: u32,
    pub media_readonly: bool,
    pub os_sound: bool,
    pub dx_mode: u32,
    /// `Some` 的路径条数(0..=5)。
    pub paths_len: u64,
    pub paths_sha256: String,
}

impl BootSummary {
    pub fn from_config(cfg: &yu_rt::BootConfig) -> Self {
        let (argv_len, argv_sha256) = argv_summary(&cfg.argv);
        let (paths_len, paths_sha256) = paths_summary(&cfg.paths);
        Self {
            entry: cfg.entry_label.clone(),
            fps: cfg.fps,
            start_ms: cfg.start_ms,
            frame_ms: cfg.frame_ms,
            script_key: cfg.script_key,
            debug_mode: cfg.debug_mode,
            argv_len,
            argv_sha256,
            host: HostSummary {
                screen_color: cfg.screen_color,
                media_readonly: cfg.media_readonly,
                os_sound: cfg.os_sound,
                dx_mode: cfg.dx_mode,
                paths_len,
                paths_sha256,
            },
        }
    }

    /// 用本摘要重建 [`yu_rt::BootConfig`]。
    ///
    /// `argv` / `paths` 必须与录制时的摘要一致;不一致 ⇒ `trace.replay`
    /// (Fatal):拒绝用不同的命令行/宿主路径"重放"。
    pub fn to_config(
        &self,
        argv: Vec<Vec<u8>>,
        paths: yu_rt::vm::HostPaths,
    ) -> Result<yu_rt::BootConfig, Diag> {
        let (len, sha256) = argv_summary(&argv);
        if len != self.argv_len || sha256 != self.argv_sha256 {
            return Err(
                Diag::fatal(crate::error::code::REPLAY, Boundary::Script).with_detail(format!(
                    "replay argv mismatch: recorded {} args sha256 {}, supplied {} args sha256 {}",
                    self.argv_len, self.argv_sha256, len, sha256
                )),
            );
        }
        let (paths_len, paths_sha256) = paths_summary(&paths);
        if paths_len != self.host.paths_len || paths_sha256 != self.host.paths_sha256 {
            return Err(Diag::fatal(crate::error::code::REPLAY, Boundary::Script)
                .with_detail(format!(
                    "replay host paths mismatch: recorded {} paths sha256 {}, supplied {} paths sha256 {}",
                    self.host.paths_len, self.host.paths_sha256, paths_len, paths_sha256
                )));
        }
        Ok(yu_rt::BootConfig {
            entry_label: self.entry.clone(),
            fps: self.fps,
            start_ms: self.start_ms,
            frame_ms: self.frame_ms,
            script_key: self.script_key,
            debug_mode: self.debug_mode,
            argv,
            screen_color: self.host.screen_color,
            paths,
            media_readonly: self.host.media_readonly,
            os_sound: self.host.os_sound,
            dx_mode: self.host.dx_mode,
            record_events: true,
        })
    }
}

/// `argv` 的规范摘要:`(条数, sha256)`。
///
/// 规范字节序:逐项 `len:u64 LE + bytes`,不做任何归一化。
pub fn argv_summary(argv: &[Vec<u8>]) -> (u64, String) {
    let mut bytes = Vec::new();
    for arg in argv {
        bytes.extend_from_slice(&(arg.len() as u64).to_le_bytes());
        bytes.extend_from_slice(arg);
    }
    (argv.len() as u64, crate::hash::sha256_hex(&bytes))
}

/// `HostPaths` 的规范摘要:`(Some 条数, sha256)`。
///
/// 规范字节序:固定字段顺序 `system/save/appdata/personal/project`,
/// 每项 `tag:u8, present:u8, [len:u64 LE + bytes]`。
pub fn paths_summary(paths: &yu_rt::vm::HostPaths) -> (u64, String) {
    let fields: [&Option<Vec<u8>>; 5] = [
        &paths.system_path,
        &paths.save_path,
        &paths.appdata_path,
        &paths.personal_path,
        &paths.project_path,
    ];
    let mut bytes = Vec::new();
    let mut present = 0u64;
    for (tag, field) in fields.iter().enumerate() {
        bytes.push(tag as u8);
        match field {
            Some(value) => {
                present += 1;
                bytes.push(1);
                bytes.extend_from_slice(&(value.len() as u64).to_le_bytes());
                bytes.extend_from_slice(value);
            }
            None => bytes.push(0),
        }
    }
    (present, crate::hash::sha256_hex(&bytes))
}

/// 输入时间线摘要。`sha256` 覆盖 `[first, first + frames)` 的每帧有效输入。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputSummary {
    /// 时间线中显式设定的帧数(`InputTimeline::len`),不是回放帧数。
    pub events: u64,
    pub sha256: String,
}

/// 头部载荷。`tables` 来自 [`yu_rt::Vm::table_provenance`],只含相对路径。
///
/// v3 起 `producer` 与 `clock` 是必填字段。跨 producer diff 只投影
/// `codec` + `boot` + `input` + 双方都存在的 `producer.sample_sha256`;
/// `kernel` / `tables` / producer 名称版本不参与语义对齐(见 `diff` 模块)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderPayload {
    pub codec: String,
    pub producer: ProducerInfo,
    pub tables: Vec<String>,
    pub boot: BootSummary,
    pub input: InputSummary,
    pub clock: ClockSummary,
    /// 人类可读的内核标识;不参与跨 producer diff。
    pub kernel: String,
}

/// 参数值(与 `yu_rt::Value` 同构,但由本 crate 拥有序列化)。
#[derive(Clone, Debug, PartialEq)]
pub enum TraceValue {
    Int(i64),
    Float(f64),
    /// 原始字节(CP932 等;与 `VmEvent::Text.bytes` 一致)。
    Str(Vec<u8>),
}

impl TraceValue {
    pub fn from_value(value: &yu_rt::Value) -> Self {
        match value {
            yu_rt::Value::Int(v) => TraceValue::Int(*v),
            yu_rt::Value::Float(v) => TraceValue::Float(*v),
            yu_rt::Value::Str(bytes) => TraceValue::Str(bytes.clone()),
        }
    }
}

/// 一个参数槽。
#[derive(Clone, Debug, PartialEq)]
pub struct TraceArg {
    pub slot: u8,
    pub value: TraceValue,
}

impl TraceArg {
    pub fn from_arg(arg: &yu_rt::event::ArgValue) -> Self {
        Self {
            slot: arg.slot,
            value: TraceValue::from_value(&arg.value),
        }
    }
}

/// 载荷。每个变体与 [`Kind`] 一一对应(空载荷用 [`Payload::Empty`])。
///
/// header 是最大的变体;它是协议层的一次性载荷,箱化会改变公开 API,
/// 因此这里显式保留并关闭 clippy 的大小警告。
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
pub enum Payload {
    Header(HeaderPayload),
    VfsOpen {
        source: String,
        key: Vec<u8>,
        /// 成功时的字节数;失败为 `None`。
        bytes: Option<u64>,
        /// 成功时的内容 SHA-256;失败为 `None`。
        sha256: Option<String>,
        /// 失败时的稳定错误码;成功为 `None`。
        diag: Option<String>,
    },
    VfsExists {
        source: String,
        key: Vec<u8>,
        exists: bool,
    },
    VfsList {
        source: String,
        key: Vec<u8>,
        entries: Vec<ListEntry>,
        diag: Option<String>,
    },
    /// 空载荷的帧边界;`start` 区分 `frame_start` / `frame_end`。
    VmFrame {
        start: bool,
    },
    VmCommand {
        script: u32,
        pc: u32,
        name: String,
        args: Vec<TraceArg>,
    },
    VmJump {
        script: u32,
        pc: u32,
        to_script: u32,
        to_pc: u32,
        jump: String,
    },
    VmTaskSpawn {
        id: String,
        script: u32,
        pc: u32,
        enabled: bool,
        main: bool,
    },
    VmTaskFinish {
        id: String,
    },
    VmWait {
        task: String,
        frames: Option<u32>,
        time_ms: Option<u32>,
    },
    VmText {
        id: String,
        bytes: Vec<u8>,
    },
    /// 一行文本结束(`RETURNCODE`;`line` = 编译器写入的本脚本内改行计数)。
    VmReturnCode {
        line: u32,
    },
    VmFps {
        fps: u32,
        skip: bool,
    },
    VmCall {
        /// `system_call` 为 `None`;`domain_call` 携带域名字符串。
        domain: Option<String>,
        name: String,
        args: Vec<TraceArg>,
    },
    VmEnd {
        restart: bool,
    },
    VmDiag {
        code: String,
        detail: String,
    },
    /// ③ 帧描述(`sub = "frame"`)。
    FrameDescription(FrameSummary),
}

impl Payload {
    fn kind(&self) -> Kind {
        match self {
            Payload::Header(_) => Kind::Header,
            Payload::VfsOpen { .. } => Kind::VfsOpen,
            Payload::VfsExists { .. } => Kind::VfsExists,
            Payload::VfsList { .. } => Kind::VfsList,
            Payload::VmFrame { start: true } => Kind::VmFrameStart,
            Payload::VmFrame { start: false } => Kind::VmFrameEnd,
            Payload::VmCommand { .. } => Kind::VmCommand,
            Payload::VmJump { .. } => Kind::VmJump,
            Payload::VmTaskSpawn { .. } => Kind::VmTaskSpawn,
            Payload::VmTaskFinish { .. } => Kind::VmTaskFinish,
            Payload::VmWait { .. } => Kind::VmWait,
            Payload::VmText { .. } => Kind::VmText,
            Payload::VmReturnCode { .. } => Kind::VmReturnCode,
            Payload::VmFps { .. } => Kind::VmFps,
            Payload::VmCall { domain: None, .. } => Kind::VmSystemCall,
            Payload::VmCall {
                domain: Some(_), ..
            } => Kind::VmDomainCall,
            Payload::VmEnd { .. } => Kind::VmEnd,
            Payload::VmDiag { .. } => Kind::VmDiag,
            Payload::FrameDescription(_) => Kind::FrameDescription,
        }
    }
}

/// 一条记录。字段私有:序号只能由 [`TraceLog::push`] 分配,防止手写出裂缝。
#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    seq: u64,
    frame: Option<u64>,
    kind: Kind,
    payload: Payload,
}

impl Record {
    pub fn seq(&self) -> u64 {
        self.seq
    }

    pub fn frame(&self) -> Option<u64> {
        self.frame
    }

    pub fn sub(&self) -> Sub {
        self.kind.sub()
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    pub fn payload(&self) -> &Payload {
        &self.payload
    }

    pub(crate) fn to_json(&self) -> Json {
        Json::Obj(vec![
            ("schema".to_string(), Json::UInt(SCHEMA_VERSION as u64)),
            ("seq".to_string(), Json::UInt(self.seq)),
            (
                "frame".to_string(),
                match self.frame {
                    Some(frame) => Json::UInt(frame),
                    None => Json::Null,
                },
            ),
            (
                "sub".to_string(),
                Json::Str(self.sub().as_str().to_string()),
            ),
            (
                "kind".to_string(),
                Json::Str(self.kind.as_str().to_string()),
            ),
            ("payload".to_string(), payload_to_json(&self.payload)),
        ])
    }

    /// 单行序列化(不含换行)。
    pub fn to_line(&self) -> String {
        json::to_text(&self.to_json())
    }

    /// 解析一行。`line` 仅用于诊断定位(1 起)。
    ///
    /// 只做单行结构校验;序号连续性由 [`TraceLog::parse`] 负责。
    pub fn parse_line(line: u64, text: &str) -> Result<Self, Diag> {
        let value = json::parse(text).map_err(|detail| error::parse(line, detail))?;
        let obj = json::as_obj(&value).map_err(|detail| error::parse(line, detail))?;
        let mut fields = Fields::new(obj);

        let schema = take_u64(&mut fields, line, "schema")?;
        if schema != SCHEMA_VERSION as u64 {
            return Err(error::schema(
                line,
                format!("unsupported schema {schema}; this build accepts {SCHEMA_VERSION}"),
            ));
        }
        let seq = take_u64(&mut fields, line, "seq")?;
        let frame = match fields.take("frame").map_err(|e| field_error(line, e))? {
            Json::Null => None,
            Json::UInt(frame) => Some(*frame),
            other => {
                return Err(error::parse(
                    line,
                    format!(
                        "frame must be null or unsigned integer, got {}",
                        json_type(other)
                    ),
                ))
            }
        };
        let sub_text = take_str(&mut fields, line, "sub")?.to_string();
        let sub = Sub::parse(&sub_text)
            .ok_or_else(|| error::schema(line, format!("unknown sub `{sub_text}`")))?;
        let kind_text = take_str(&mut fields, line, "kind")?.to_string();
        let kind = Kind::parse(&kind_text)
            .ok_or_else(|| error::schema(line, format!("unknown kind `{kind_text}`")))?;
        if kind == Kind::PlatformCall {
            return Err(error::schema(
                line,
                format!(
                    "kind `{}` is reserved for M3 platform backend and not defined by schema {SCHEMA_VERSION}",
                    kind.as_str()
                ),
            ));
        }
        if sub != kind.sub() {
            return Err(error::schema(
                line,
                format!(
                    "sub `{}` does not match kind `{}` (expected `{}`)",
                    sub.as_str(),
                    kind.as_str(),
                    kind.sub().as_str()
                ),
            ));
        }
        if kind == Kind::Header && frame.is_some() {
            return Err(error::schema(line, "header record must have frame = null"));
        }
        // 边界①(vfs)可发生在帧外 ⇒ frame = null;边界②(事件流)与 ③(帧描述)
        // 必属于某逻辑帧。
        if matches!(kind.sub(), Sub::Vm | Sub::Frame) && frame.is_none() {
            return Err(error::schema(
                line,
                format!("kind `{}` requires a frame", kind.as_str()),
            ));
        }

        let payload_value = fields.take("payload").map_err(|e| field_error(line, e))?;
        let payload = parse_payload(line, kind, frame, payload_value)?;
        fields.finish().map_err(|e| field_error(line, e))?;

        Ok(Self {
            seq,
            frame,
            kind,
            payload,
        })
    }
}

/// 有序记录集。`push` 是唯一分配 `seq` 的入口。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TraceLog {
    records: Vec<Record>,
}

impl TraceLog {
    pub fn new() -> Self {
        Self::default()
    }

    /// 推入头部。只允许作为第一条记录。
    pub fn push_header(&mut self, header: HeaderPayload) -> u64 {
        assert!(self.records.is_empty(), "header must be the first record");
        self.records.push(Record {
            seq: 0,
            frame: None,
            kind: Kind::Header,
            payload: Payload::Header(header),
        });
        0
    }

    /// 在已有记录前插入头部并重编号。
    ///
    /// 用途:启动链的 vfs 读取发生在表装载期,此时还拿不到
    /// `table_provenance`;启动完成后补头,保证 `header` 永远是 `seq = 0`。
    pub fn prepend_header(&mut self, header: HeaderPayload) {
        assert!(
            !self
                .records
                .iter()
                .any(|record| record.kind == Kind::Header),
            "trace already has a header"
        );
        self.records.insert(
            0,
            Record {
                seq: 0,
                frame: None,
                kind: Kind::Header,
                payload: Payload::Header(header),
            },
        );
        for (index, record) in self.records.iter_mut().enumerate() {
            record.seq = index as u64;
        }
    }

    /// 推入一条记录,返回分配的 `seq`。
    ///
    /// # Panics
    ///
    /// 头部只能通过 [`Self::push_header`] / [`Self::prepend_header`] 写入;
    /// 传入 [`Payload::Header`] 会 panic,避免产出"自己解析不回来"的文件。
    pub fn push(&mut self, frame: Option<u64>, payload: Payload) -> u64 {
        assert!(
            !matches!(&payload, Payload::Header(_)),
            "use push_header/prepend_header for the header record"
        );
        let seq = self.records.len() as u64;
        let kind = payload.kind();
        self.records.push(Record {
            seq,
            frame,
            kind,
            payload,
        });
        seq
    }

    pub fn records(&self) -> &[Record] {
        &self.records
    }

    /// 推入一条帧描述;封套 `frame` 取自 `summary.frame`。
    pub fn push_frame(&mut self, summary: FrameSummary) -> u64 {
        let frame = Some(summary.frame);
        self.push(frame, Payload::FrameDescription(summary))
    }

    /// 校验并推入调用方输入。
    pub fn push_frame_input(&mut self, input: FrameDescriptionInput) -> Result<u64, Diag> {
        let summary = FrameSummary::from_input(input)?;
        Ok(self.push_frame(summary))
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn header(&self) -> Option<&HeaderPayload> {
        self.records
            .first()
            .and_then(|record| match &record.payload {
                Payload::Header(header) => Some(header),
                _ => None,
            })
    }

    /// 确定性 JSONL(每条记录一行,末尾换行)。
    pub fn to_jsonl(&self) -> String {
        let mut out = String::new();
        for record in &self.records {
            out.push_str(&record.to_line());
            out.push('\n');
        }
        out
    }

    /// 解析完整文本:头部必须在 `seq=0`,`seq` 严格连续,不允许空行。
    pub fn parse(text: &str) -> Result<Self, Diag> {
        if text.is_empty() {
            return Err(error::truncated("trace is empty (missing header)"));
        }
        if !text.ends_with('\n') {
            return Err(error::truncated(
                "trace file does not end with a newline (truncated write?)",
            ));
        }
        let mut records = Vec::new();
        for (index, line) in text.split_terminator('\n').enumerate() {
            let line_no = index as u64 + 1;
            if line.is_empty() {
                return Err(error::parse(line_no, "empty line"));
            }
            if line.contains('\r') {
                return Err(error::parse(line_no, "CR is not part of this schema"));
            }
            let record = Record::parse_line(line_no, line)?;
            if record.seq != index as u64 {
                return Err(error::schema(
                    line_no,
                    format!(
                        "expected seq {} at line {line_no}, got {}",
                        index, record.seq
                    ),
                ));
            }
            if record.seq == 0 && record.kind != Kind::Header {
                return Err(error::schema(
                    line_no,
                    "first record must be the header (sub=meta, kind=header)",
                ));
            }
            if record.seq > 0 && record.kind == Kind::Header {
                return Err(error::schema(line_no, "header is only allowed at seq 0"));
            }
            records.push(record);
        }
        Ok(Self { records })
    }

    pub fn read_from(path: impl AsRef<Path>) -> Result<Self, Diag> {
        let path = path.as_ref();
        let bytes =
            fs::read(path).map_err(|e| error::io(format!("read {}: {e}", path.display())))?;
        let text = String::from_utf8(bytes)
            .map_err(|e| error::parse(0, format!("trace is not valid UTF-8: {e}")))?;
        Self::parse(&text)
    }

    pub fn write_to(&self, path: impl AsRef<Path>) -> Result<(), Diag> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)
                    .map_err(|e| error::io(format!("create {}: {e}", parent.display())))?;
            }
        }
        fs::write(path, self.to_jsonl())
            .map_err(|e| error::io(format!("write {}: {e}", path.display())))
    }
}

fn field_error(line: u64, error: FieldError) -> Diag {
    match error {
        FieldError::Missing(key) => error::parse(line, format!("missing field `{key}`")),
        FieldError::Unknown(key) => {
            error::schema(line, format!("unknown field `{key}` for this kind"))
        }
    }
}

fn take_u64(fields: &mut Fields<'_>, line: u64, key: &str) -> Result<u64, Diag> {
    let value = fields.take(key).map_err(|e| field_error(line, e))?;
    json::as_u64(value).map_err(|detail| error::parse(line, detail))
}

fn take_str<'a>(fields: &mut Fields<'a>, line: u64, key: &str) -> Result<&'a str, Diag> {
    let value = fields.take(key).map_err(|e| field_error(line, e))?;
    json::as_str(value).map_err(|detail| error::parse(line, detail))
}

fn take_bool(fields: &mut Fields<'_>, line: u64, key: &str) -> Result<bool, Diag> {
    let value = fields.take(key).map_err(|e| field_error(line, e))?;
    json::as_bool(value).map_err(|detail| error::parse(line, detail))
}

fn take_opt_u32(fields: &mut Fields<'_>, line: u64, key: &str) -> Result<Option<u32>, Diag> {
    let value = fields.take(key).map_err(|e| field_error(line, e))?;
    match value {
        Json::Null => Ok(None),
        other => {
            let n = json::as_u64(other).map_err(|detail| error::parse(line, detail))?;
            u32::try_from(n)
                .map(Some)
                .map_err(|_| error::parse(line, format!("{key} out of u32 range: {n}")))
        }
    }
}

fn take_u32(fields: &mut Fields<'_>, line: u64, key: &str) -> Result<u32, Diag> {
    let n = take_u64(fields, line, key)?;
    u32::try_from(n).map_err(|_| error::parse(line, format!("{key} out of u32 range: {n}")))
}

fn take_bytes(fields: &mut Fields<'_>, line: u64, key: &str) -> Result<Vec<u8>, Diag> {
    let text = take_str(fields, line, key)?;
    json::bytes_from_string(text).map_err(|detail| error::parse(line, detail))
}

fn take_args(fields: &mut Fields<'_>, line: u64, key: &str) -> Result<Vec<TraceArg>, Diag> {
    let value = fields.take(key).map_err(|e| field_error(line, e))?;
    let items = json::as_arr(value).map_err(|detail| error::parse(line, detail))?;
    let mut args = Vec::with_capacity(items.len());
    for item in items {
        let obj = json::as_obj(item).map_err(|detail| error::parse(line, detail))?;
        let mut arg_fields = Fields::new(obj);
        let slot = take_u64(&mut arg_fields, line, "slot")?;
        let slot = u8::try_from(slot)
            .map_err(|_| error::parse(line, format!("slot out of u8 range: {slot}")))?;
        let value = arg_fields.take("value").map_err(|e| field_error(line, e))?;
        let value = parse_trace_value(line, value)?;
        arg_fields.finish().map_err(|e| field_error(line, e))?;
        args.push(TraceArg { slot, value });
    }
    Ok(args)
}

fn parse_trace_value(line: u64, value: &Json) -> Result<TraceValue, Diag> {
    let obj = json::as_obj(value).map_err(|detail| error::parse(line, detail))?;
    let mut fields = Fields::new(obj);
    let tag = take_str(&mut fields, line, "t")?.to_string();
    let raw = fields.take("v").map_err(|e| field_error(line, e))?;
    let parsed = match tag.as_str() {
        "int" => TraceValue::Int(json::as_i64(raw).map_err(|detail| error::parse(line, detail))?),
        "flt" => TraceValue::Float(json::as_f64(raw).map_err(|detail| error::parse(line, detail))?),
        "str" => TraceValue::Str(
            json::bytes_from_string(
                json::as_str(raw).map_err(|detail| error::parse(line, detail))?,
            )
            .map_err(|detail| error::parse(line, detail))?,
        ),
        other => {
            return Err(error::schema(
                line,
                format!("unknown argument type `{other}`"),
            ))
        }
    };
    fields.finish().map_err(|e| field_error(line, e))?;
    Ok(parsed)
}

fn parse_payload(line: u64, kind: Kind, frame: Option<u64>, value: &Json) -> Result<Payload, Diag> {
    let obj = json::as_obj(value).map_err(|detail| error::parse(line, detail))?;
    if kind == Kind::Header {
        return Ok(Payload::Header(parse_header(line, obj)?));
    }
    if kind == Kind::FrameDescription {
        return Ok(Payload::FrameDescription(parse_frame_description(
            line, frame, obj,
        )?));
    }
    let mut fields = Fields::new(obj);
    let payload = match kind {
        Kind::Header => {
            // 上方已提前返回;此处保留显式分支,不给 match 留通配兜底。
            unreachable!("header payload is parsed before field cursor setup")
        }
        Kind::VfsOpen => {
            let source = take_str(&mut fields, line, "source")?.to_string();
            let key = take_bytes(&mut fields, line, "key")?;
            let bytes = match fields.take("bytes").map_err(|e| field_error(line, e))? {
                Json::Null => None,
                other => Some(json::as_u64(other).map_err(|detail| error::parse(line, detail))?),
            };
            let sha256 = match fields.take("sha256").map_err(|e| field_error(line, e))? {
                Json::Null => None,
                other => Some(
                    json::as_str(other)
                        .map_err(|detail| error::parse(line, detail))?
                        .to_string(),
                ),
            };
            let diag = take_opt_code(&mut fields, line, "diag")?;
            if diag.is_some() && (bytes.is_some() || sha256.is_some()) {
                return Err(error::schema(
                    line,
                    "vfs open with diag must not carry bytes/sha256",
                ));
            }
            if diag.is_none() && (bytes.is_none() || sha256.is_none()) {
                return Err(error::parse(
                    line,
                    "vfs open without diag requires bytes and sha256",
                ));
            }
            Payload::VfsOpen {
                source,
                key,
                bytes,
                sha256,
                diag,
            }
        }
        Kind::VfsExists => {
            let source = take_str(&mut fields, line, "source")?.to_string();
            let key = take_bytes(&mut fields, line, "key")?;
            let exists = take_bool(&mut fields, line, "exists")?;
            Payload::VfsExists {
                source,
                key,
                exists,
            }
        }
        Kind::VfsList => {
            let source = take_str(&mut fields, line, "source")?.to_string();
            let key = take_bytes(&mut fields, line, "key")?;
            let entries_value = fields.take("entries").map_err(|e| field_error(line, e))?;
            let items = json::as_arr(entries_value).map_err(|detail| error::parse(line, detail))?;
            let mut entries = Vec::with_capacity(items.len());
            for item in items {
                let entry_obj = json::as_obj(item).map_err(|detail| error::parse(line, detail))?;
                let mut entry_fields = Fields::new(entry_obj);
                let name = take_bytes(&mut entry_fields, line, "name")?;
                let kind_text = take_str(&mut entry_fields, line, "kind")?.to_string();
                let kind = match kind_text.as_str() {
                    "file" => EntryKind::File,
                    "dir" => EntryKind::Dir,
                    other => {
                        return Err(error::schema(line, format!("unknown entry kind `{other}`")))
                    }
                };
                let size = match entry_fields
                    .take("size")
                    .map_err(|e| field_error(line, e))?
                {
                    Json::Null => None,
                    other => {
                        Some(json::as_u64(other).map_err(|detail| error::parse(line, detail))?)
                    }
                };
                if kind == EntryKind::File && size.is_none() {
                    return Err(error::parse(
                        line,
                        "file entry requires a size (dir may have null)",
                    ));
                }
                if kind == EntryKind::Dir && size.is_some() {
                    return Err(error::schema(line, "dir entry must have size = null"));
                }
                entry_fields.finish().map_err(|e| field_error(line, e))?;
                entries.push(ListEntry { name, kind, size });
            }
            let diag = take_opt_code(&mut fields, line, "diag")?;
            if diag.is_some() && !entries.is_empty() {
                return Err(error::schema(
                    line,
                    "vfs list with diag must not carry entries",
                ));
            }
            Payload::VfsList {
                source,
                key,
                entries,
                diag,
            }
        }
        Kind::VmFrameStart => Payload::VmFrame { start: true },
        Kind::VmFrameEnd => Payload::VmFrame { start: false },
        Kind::VmCommand => {
            let script = take_u32(&mut fields, line, "script")?;
            let pc = take_u32(&mut fields, line, "pc")?;
            let name = take_str(&mut fields, line, "name")?.to_string();
            let args = take_args(&mut fields, line, "args")?;
            Payload::VmCommand {
                script,
                pc,
                name,
                args,
            }
        }
        Kind::VmJump => {
            let script = take_u32(&mut fields, line, "script")?;
            let pc = take_u32(&mut fields, line, "pc")?;
            let to_script = take_u32(&mut fields, line, "to_script")?;
            let to_pc = take_u32(&mut fields, line, "to_pc")?;
            let jump = take_str(&mut fields, line, "jump")?.to_string();
            if !is_known_jump(&jump) {
                return Err(error::schema(line, format!("unknown jump kind `{jump}`")));
            }
            Payload::VmJump {
                script,
                pc,
                to_script,
                to_pc,
                jump,
            }
        }
        Kind::VmTaskSpawn => {
            let id = take_str(&mut fields, line, "id")?.to_string();
            let script = take_u32(&mut fields, line, "script")?;
            let pc = take_u32(&mut fields, line, "pc")?;
            let enabled = take_bool(&mut fields, line, "enabled")?;
            let main = take_bool(&mut fields, line, "main")?;
            Payload::VmTaskSpawn {
                id,
                script,
                pc,
                enabled,
                main,
            }
        }
        Kind::VmTaskFinish => {
            let id = take_str(&mut fields, line, "id")?.to_string();
            Payload::VmTaskFinish { id }
        }
        Kind::VmWait => {
            let task = take_str(&mut fields, line, "task")?.to_string();
            let frames = take_opt_u32(&mut fields, line, "frames")?;
            let time_ms = take_opt_u32(&mut fields, line, "time_ms")?;
            if frames.is_none() && time_ms.is_none() {
                return Err(error::schema(
                    line,
                    "wait requires at least one of frames/time_ms",
                ));
            }
            Payload::VmWait {
                task,
                frames,
                time_ms,
            }
        }
        Kind::VmText => {
            let id = take_str(&mut fields, line, "id")?.to_string();
            let bytes = take_bytes(&mut fields, line, "bytes")?;
            Payload::VmText { id, bytes }
        }
        Kind::VmReturnCode => {
            let line_no = take_u32(&mut fields, line, "line")?;
            Payload::VmReturnCode { line: line_no }
        }
        Kind::VmFps => {
            let fps = take_u32(&mut fields, line, "fps")?;
            let skip = take_bool(&mut fields, line, "skip")?;
            Payload::VmFps { fps, skip }
        }
        Kind::VmSystemCall => {
            let name = take_str(&mut fields, line, "name")?.to_string();
            let args = take_args(&mut fields, line, "args")?;
            Payload::VmCall {
                domain: None,
                name,
                args,
            }
        }
        Kind::VmDomainCall => {
            let domain = take_str(&mut fields, line, "domain")?.to_string();
            if !is_known_domain(&domain) {
                return Err(error::schema(line, format!("unknown domain `{domain}`")));
            }
            let name = take_str(&mut fields, line, "name")?.to_string();
            let args = take_args(&mut fields, line, "args")?;
            Payload::VmCall {
                domain: Some(domain),
                name,
                args,
            }
        }
        Kind::VmEnd => {
            let restart = take_bool(&mut fields, line, "restart")?;
            Payload::VmEnd { restart }
        }
        Kind::VmDiag => {
            let code = take_str(&mut fields, line, "code")?.to_string();
            let detail = take_str(&mut fields, line, "detail")?.to_string();
            Payload::VmDiag { code, detail }
        }
        Kind::FrameDescription => {
            // 上方已提前返回;此处保留显式分支,不给 match 留通配兜底。
            unreachable!("frame_description payload is parsed before field cursor setup")
        }
        Kind::PlatformCall => {
            return Err(error::schema(
                line,
                format!(
                    "kind `{}` is reserved for M3 platform backend",
                    kind.as_str()
                ),
            ))
        }
    };
    let payload_kind = payload.kind();
    if payload_kind != kind {
        return Err(error::schema(
            line,
            format!(
                "payload shape maps to `{}`, not envelope kind `{}`",
                payload_kind.as_str(),
                kind.as_str()
            ),
        ));
    }
    fields.finish().map_err(|e| field_error(line, e))?;
    Ok(payload)
}

fn parse_frame_description(
    line: u64,
    envelope_frame: Option<u64>,
    obj: &[(String, Json)],
) -> Result<FrameSummary, Diag> {
    let mut fields = Fields::new(obj);
    let frame = take_u64(&mut fields, line, "frame")?;
    match envelope_frame {
        Some(envelope) if envelope == frame => {}
        Some(envelope) => {
            return Err(error::schema(
                line,
                format!("payload frame {frame} does not match envelope frame {envelope}"),
            ))
        }
        None => {
            return Err(error::schema(
                line,
                "frame_description requires an integer envelope frame",
            ))
        }
    }
    let level_text = take_str(&mut fields, line, "level")?.to_string();
    let level = FrameLevel::parse(&level_text)
        .ok_or_else(|| error::schema(line, format!("unknown frame level `{level_text}`")))?;
    let display_sha256 = take_str(&mut fields, line, "display_sha256")?.to_string();
    validate_sha256(&display_sha256).map_err(|detail| error::parse(line, detail))?;
    let pass_count = take_u32(&mut fields, line, "pass_count")?;
    let op_count = take_u64(&mut fields, line, "op_count")?;
    let executable_ops = take_u64(&mut fields, line, "executable_ops")?;
    let skipped_ops = take_u64(&mut fields, line, "skipped_ops")?;
    let display_value = fields.take("display").map_err(|e| field_error(line, e))?;
    let display = match display_value {
        Json::Null => None,
        other => {
            let canonical = json::to_text(other);
            let computed = crate::hash::sha256_hex(canonical.as_bytes());
            if computed != display_sha256 {
                return Err(error::schema(
                    line,
                    format!(
                        "display_sha256 {display_sha256} does not match canonical display JSON {computed}"
                    ),
                ));
            }
            Some(FrameDisplay {
                value: other.clone(),
            })
        }
    };
    fields.finish().map_err(|e| field_error(line, e))?;
    Ok(FrameSummary {
        frame,
        level,
        display_sha256,
        pass_count,
        op_count,
        executable_ops,
        skipped_ops,
        display,
    })
}

fn take_opt_code(fields: &mut Fields<'_>, line: u64, key: &str) -> Result<Option<String>, Diag> {
    match fields.take(key).map_err(|e| field_error(line, e))? {
        Json::Null => Ok(None),
        other => Ok(Some(
            json::as_str(other)
                .map_err(|detail| error::parse(line, detail))?
                .to_string(),
        )),
    }
}

fn parse_header(line: u64, obj: &[(String, Json)]) -> Result<HeaderPayload, Diag> {
    let mut fields = Fields::new(obj);
    let codec = take_str(&mut fields, line, "codec")?.to_string();
    let producer_value = fields.take("producer").map_err(|e| field_error(line, e))?;
    let producer = parse_producer(line, producer_value)?;
    let tables_value = fields.take("tables").map_err(|e| field_error(line, e))?;
    let table_items = json::as_arr(tables_value).map_err(|detail| error::parse(line, detail))?;
    let mut tables = Vec::with_capacity(table_items.len());
    for item in table_items {
        tables.push(
            json::as_str(item)
                .map_err(|detail| error::parse(line, detail))?
                .to_string(),
        );
    }
    let boot_value = fields.take("boot").map_err(|e| field_error(line, e))?;
    let boot_obj = json::as_obj(boot_value).map_err(|detail| error::parse(line, detail))?;
    let mut boot_fields = Fields::new(boot_obj);
    let entry = take_str(&mut boot_fields, line, "entry")?.to_string();
    let fps = take_u32(&mut boot_fields, line, "fps")?;
    let start_ms = take_u32(&mut boot_fields, line, "start_ms")?;
    let frame_ms = take_u32(&mut boot_fields, line, "frame_ms")?;
    let script_key = parse_hex_key(line, take_str(&mut boot_fields, line, "script_key")?)?;
    let debug_mode = take_bool(&mut boot_fields, line, "debug_mode")?;
    let argv_value = boot_fields.take("argv").map_err(|e| field_error(line, e))?;
    let argv_obj = json::as_obj(argv_value).map_err(|detail| error::parse(line, detail))?;
    let mut argv_fields = Fields::new(argv_obj);
    let argv_len = take_u64(&mut argv_fields, line, "len")?;
    let argv_sha256 = take_str(&mut argv_fields, line, "sha256")?.to_string();
    argv_fields.finish().map_err(|e| field_error(line, e))?;

    let host_value = boot_fields.take("host").map_err(|e| field_error(line, e))?;
    let host_obj = json::as_obj(host_value).map_err(|detail| error::parse(line, detail))?;
    let mut host_fields = Fields::new(host_obj);
    let screen_color = take_u32(&mut host_fields, line, "screen_color")?;
    let media_readonly = take_bool(&mut host_fields, line, "media_readonly")?;
    let os_sound = take_bool(&mut host_fields, line, "os_sound")?;
    let dx_mode = take_u32(&mut host_fields, line, "dx_mode")?;
    let paths_value = host_fields
        .take("paths")
        .map_err(|e| field_error(line, e))?;
    let paths_obj = json::as_obj(paths_value).map_err(|detail| error::parse(line, detail))?;
    let mut paths_fields = Fields::new(paths_obj);
    let paths_len = take_u64(&mut paths_fields, line, "len")?;
    let paths_sha256 = take_str(&mut paths_fields, line, "sha256")?.to_string();
    paths_fields.finish().map_err(|e| field_error(line, e))?;
    host_fields.finish().map_err(|e| field_error(line, e))?;
    boot_fields.finish().map_err(|e| field_error(line, e))?;

    let input_value = fields.take("input").map_err(|e| field_error(line, e))?;
    let input_obj = json::as_obj(input_value).map_err(|detail| error::parse(line, detail))?;
    let mut input_fields = Fields::new(input_obj);
    let events = take_u64(&mut input_fields, line, "events")?;
    let sha256 = take_str(&mut input_fields, line, "sha256")?.to_string();
    input_fields.finish().map_err(|e| field_error(line, e))?;

    let clock_value = fields.take("clock").map_err(|e| field_error(line, e))?;
    let clock = parse_clock(line, clock_value)?;

    let kernel = take_str(&mut fields, line, "kernel")?.to_string();
    fields.finish().map_err(|e| field_error(line, e))?;

    Ok(HeaderPayload {
        codec,
        producer,
        tables,
        boot: BootSummary {
            entry,
            fps,
            start_ms,
            frame_ms,
            script_key,
            debug_mode,
            argv_len,
            argv_sha256,
            host: HostSummary {
                screen_color,
                media_readonly,
                os_sound,
                dx_mode,
                paths_len,
                paths_sha256,
            },
        },
        input: InputSummary { events, sha256 },
        clock,
        kernel,
    })
}

fn parse_producer(line: u64, value: &Json) -> Result<ProducerInfo, Diag> {
    let obj = json::as_obj(value).map_err(|detail| error::parse(line, detail))?;
    let mut fields = Fields::new(obj);
    let kind_text = take_str(&mut fields, line, "kind")?.to_string();
    let kind = ProducerKind::parse(&kind_text)
        .ok_or_else(|| error::schema(line, format!("unknown producer kind `{kind_text}`")))?;
    let name = take_str(&mut fields, line, "name")?.to_string();
    let version = take_str(&mut fields, line, "version")?.to_string();
    let sample_sha256 = match fields
        .take("sample_sha256")
        .map_err(|e| field_error(line, e))?
    {
        Json::Null => None,
        other => {
            let hash = json::as_str(other)
                .map_err(|detail| error::parse(line, detail))?
                .to_string();
            validate_sha256(&hash).map_err(|detail| error::parse(line, detail))?;
            Some(hash)
        }
    };
    fields.finish().map_err(|e| field_error(line, e))?;
    let producer = ProducerInfo {
        kind,
        name,
        version,
        sample_sha256,
    };
    producer
        .validate()
        .map_err(|detail| error::parse(line, detail))?;
    Ok(producer)
}

fn parse_clock(line: u64, value: &Json) -> Result<ClockSummary, Diag> {
    let obj = json::as_obj(value).map_err(|detail| error::parse(line, detail))?;
    let mut fields = Fields::new(obj);
    let mode_text = take_str(&mut fields, line, "mode")?.to_string();
    let mode = ClockMode::parse(&mode_text)
        .ok_or_else(|| error::schema(line, format!("unknown clock mode `{mode_text}`")))?;
    let items_value = fields.take("now_ms").map_err(|e| field_error(line, e))?;
    let items = json::as_arr(items_value).map_err(|detail| error::parse(line, detail))?;
    let mut now_ms = Vec::with_capacity(items.len());
    for item in items {
        match item {
            Json::Null => now_ms.push(None),
            other => {
                let n = json::as_u64(other).map_err(|detail| error::parse(line, detail))?;
                let n = u32::try_from(n)
                    .map_err(|_| error::parse(line, format!("now_ms out of u32 range: {n}")))?;
                now_ms.push(Some(n));
            }
        }
    }
    fields.finish().map_err(|e| field_error(line, e))?;
    let clock = ClockSummary { mode, now_ms };
    clock
        .validate()
        .map_err(|detail| error::schema(line, detail))?;
    Ok(clock)
}

/// 4 字节密钥 → 8 位小写十六进制(字节序即数组顺序)。
fn hex_key(key: &[u8; 4]) -> String {
    let mut out = String::with_capacity(8);
    for byte in key {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn parse_hex_key(line: u64, text: &str) -> Result<[u8; 4], Diag> {
    if text.len() != 8 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(error::parse(
            line,
            format!("script_key must be 8 lowercase hex digits, got {text:?}"),
        ));
    }
    let mut key = [0u8; 4];
    for (index, slot) in key.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)
            .map_err(|_| error::parse(line, format!("invalid script_key {text:?}")))?;
    }
    Ok(key)
}

fn is_known_jump(text: &str) -> bool {
    crate::event::JUMP_KINDS.contains(&text)
}

fn is_known_domain(text: &str) -> bool {
    crate::event::DOMAIN_NAMES.contains(&text)
}

pub(crate) fn payload_to_json(payload: &Payload) -> Json {
    match payload {
        Payload::Header(header) => Json::Obj(vec![
            ("codec".to_string(), Json::Str(header.codec.clone())),
            (
                "producer".to_string(),
                Json::Obj(vec![
                    (
                        "kind".to_string(),
                        Json::Str(header.producer.kind.as_str().to_string()),
                    ),
                    ("name".to_string(), Json::Str(header.producer.name.clone())),
                    (
                        "version".to_string(),
                        Json::Str(header.producer.version.clone()),
                    ),
                    (
                        "sample_sha256".to_string(),
                        match &header.producer.sample_sha256 {
                            Some(hash) => Json::Str(hash.clone()),
                            None => Json::Null,
                        },
                    ),
                ]),
            ),
            (
                "tables".to_string(),
                Json::Arr(
                    header
                        .tables
                        .iter()
                        .map(|table| Json::Str(table.clone()))
                        .collect(),
                ),
            ),
            (
                "boot".to_string(),
                Json::Obj(vec![
                    ("entry".to_string(), Json::Str(header.boot.entry.clone())),
                    ("fps".to_string(), Json::UInt(header.boot.fps as u64)),
                    (
                        "start_ms".to_string(),
                        Json::UInt(header.boot.start_ms as u64),
                    ),
                    (
                        "frame_ms".to_string(),
                        Json::UInt(header.boot.frame_ms as u64),
                    ),
                    (
                        "script_key".to_string(),
                        Json::Str(hex_key(&header.boot.script_key)),
                    ),
                    ("debug_mode".to_string(), Json::Bool(header.boot.debug_mode)),
                    (
                        "argv".to_string(),
                        Json::Obj(vec![
                            ("len".to_string(), Json::UInt(header.boot.argv_len)),
                            (
                                "sha256".to_string(),
                                Json::Str(header.boot.argv_sha256.clone()),
                            ),
                        ]),
                    ),
                    (
                        "host".to_string(),
                        Json::Obj(vec![
                            (
                                "screen_color".to_string(),
                                Json::UInt(header.boot.host.screen_color as u64),
                            ),
                            (
                                "media_readonly".to_string(),
                                Json::Bool(header.boot.host.media_readonly),
                            ),
                            (
                                "os_sound".to_string(),
                                Json::Bool(header.boot.host.os_sound),
                            ),
                            (
                                "dx_mode".to_string(),
                                Json::UInt(header.boot.host.dx_mode as u64),
                            ),
                            (
                                "paths".to_string(),
                                Json::Obj(vec![
                                    ("len".to_string(), Json::UInt(header.boot.host.paths_len)),
                                    (
                                        "sha256".to_string(),
                                        Json::Str(header.boot.host.paths_sha256.clone()),
                                    ),
                                ]),
                            ),
                        ]),
                    ),
                ]),
            ),
            (
                "input".to_string(),
                Json::Obj(vec![
                    ("events".to_string(), Json::UInt(header.input.events)),
                    ("sha256".to_string(), Json::Str(header.input.sha256.clone())),
                ]),
            ),
            (
                "clock".to_string(),
                Json::Obj(vec![
                    (
                        "mode".to_string(),
                        Json::Str(header.clock.mode.as_str().to_string()),
                    ),
                    (
                        "now_ms".to_string(),
                        Json::Arr(
                            header
                                .clock
                                .now_ms
                                .iter()
                                .map(|value| match value {
                                    Some(now) => Json::UInt(*now as u64),
                                    None => Json::Null,
                                })
                                .collect(),
                        ),
                    ),
                ]),
            ),
            ("kernel".to_string(), Json::Str(header.kernel.clone())),
        ]),
        Payload::VfsOpen {
            source,
            key,
            bytes,
            sha256,
            diag,
        } => Json::Obj(vec![
            ("source".to_string(), Json::Str(source.clone())),
            ("key".to_string(), bytes_json(key)),
            (
                "bytes".to_string(),
                match bytes {
                    Some(n) => Json::UInt(*n),
                    None => Json::Null,
                },
            ),
            (
                "sha256".to_string(),
                match sha256 {
                    Some(hash) => Json::Str(hash.clone()),
                    None => Json::Null,
                },
            ),
            (
                "diag".to_string(),
                match diag {
                    Some(code) => Json::Str(code.clone()),
                    None => Json::Null,
                },
            ),
        ]),
        Payload::VfsExists {
            source,
            key,
            exists,
        } => Json::Obj(vec![
            ("source".to_string(), Json::Str(source.clone())),
            ("key".to_string(), bytes_json(key)),
            ("exists".to_string(), Json::Bool(*exists)),
        ]),
        Payload::VfsList {
            source,
            key,
            entries,
            diag,
        } => Json::Obj(vec![
            ("source".to_string(), Json::Str(source.clone())),
            ("key".to_string(), bytes_json(key)),
            (
                "entries".to_string(),
                Json::Arr(
                    entries
                        .iter()
                        .map(|entry| {
                            Json::Obj(vec![
                                ("name".to_string(), bytes_json(&entry.name)),
                                (
                                    "kind".to_string(),
                                    Json::Str(entry.kind.as_str().to_string()),
                                ),
                                (
                                    "size".to_string(),
                                    match entry.size {
                                        Some(size) => Json::UInt(size),
                                        None => Json::Null,
                                    },
                                ),
                            ])
                        })
                        .collect(),
                ),
            ),
            (
                "diag".to_string(),
                match diag {
                    Some(code) => Json::Str(code.clone()),
                    None => Json::Null,
                },
            ),
        ]),
        Payload::VmFrame { .. } => Json::Obj(Vec::new()),
        Payload::VmCommand {
            script,
            pc,
            name,
            args,
        } => Json::Obj(vec![
            ("script".to_string(), Json::UInt(*script as u64)),
            ("pc".to_string(), Json::UInt(*pc as u64)),
            ("name".to_string(), Json::Str(name.clone())),
            ("args".to_string(), args_json(args)),
        ]),
        Payload::VmJump {
            script,
            pc,
            to_script,
            to_pc,
            jump,
        } => Json::Obj(vec![
            ("script".to_string(), Json::UInt(*script as u64)),
            ("pc".to_string(), Json::UInt(*pc as u64)),
            ("to_script".to_string(), Json::UInt(*to_script as u64)),
            ("to_pc".to_string(), Json::UInt(*to_pc as u64)),
            ("jump".to_string(), Json::Str(jump.clone())),
        ]),
        Payload::VmTaskSpawn {
            id,
            script,
            pc,
            enabled,
            main,
        } => Json::Obj(vec![
            ("id".to_string(), Json::Str(id.clone())),
            ("script".to_string(), Json::UInt(*script as u64)),
            ("pc".to_string(), Json::UInt(*pc as u64)),
            ("enabled".to_string(), Json::Bool(*enabled)),
            ("main".to_string(), Json::Bool(*main)),
        ]),
        Payload::VmTaskFinish { id } => Json::Obj(vec![("id".to_string(), Json::Str(id.clone()))]),
        Payload::VmWait {
            task,
            frames,
            time_ms,
        } => Json::Obj(vec![
            ("task".to_string(), Json::Str(task.clone())),
            (
                "frames".to_string(),
                match frames {
                    Some(n) => Json::UInt(*n as u64),
                    None => Json::Null,
                },
            ),
            (
                "time_ms".to_string(),
                match time_ms {
                    Some(n) => Json::UInt(*n as u64),
                    None => Json::Null,
                },
            ),
        ]),
        Payload::VmText { id, bytes } => Json::Obj(vec![
            ("id".to_string(), Json::Str(id.clone())),
            ("bytes".to_string(), bytes_json(bytes)),
        ]),
        Payload::VmReturnCode { line } => {
            Json::Obj(vec![("line".to_string(), Json::UInt(*line as u64))])
        }
        Payload::VmFps { fps, skip } => Json::Obj(vec![
            ("fps".to_string(), Json::UInt(*fps as u64)),
            ("skip".to_string(), Json::Bool(*skip)),
        ]),
        Payload::VmCall { domain, name, args } => {
            let mut fields = Vec::new();
            if let Some(domain) = domain {
                fields.push(("domain".to_string(), Json::Str(domain.clone())));
            }
            fields.push(("name".to_string(), Json::Str(name.clone())));
            fields.push(("args".to_string(), args_json(args)));
            Json::Obj(fields)
        }
        Payload::VmEnd { restart } => {
            Json::Obj(vec![("restart".to_string(), Json::Bool(*restart))])
        }
        Payload::VmDiag { code, detail } => Json::Obj(vec![
            ("code".to_string(), Json::Str(code.clone())),
            ("detail".to_string(), Json::Str(detail.clone())),
        ]),
        Payload::FrameDescription(summary) => Json::Obj(vec![
            ("frame".to_string(), Json::UInt(summary.frame)),
            (
                "level".to_string(),
                Json::Str(summary.level.as_str().to_string()),
            ),
            (
                "display_sha256".to_string(),
                Json::Str(summary.display_sha256.clone()),
            ),
            (
                "pass_count".to_string(),
                Json::UInt(summary.pass_count as u64),
            ),
            ("op_count".to_string(), Json::UInt(summary.op_count)),
            (
                "executable_ops".to_string(),
                Json::UInt(summary.executable_ops),
            ),
            ("skipped_ops".to_string(), Json::UInt(summary.skipped_ops)),
            (
                "display".to_string(),
                match &summary.display {
                    Some(display) => display.as_json().clone(),
                    None => Json::Null,
                },
            ),
        ]),
    }
}

fn args_json(args: &[TraceArg]) -> Json {
    Json::Arr(
        args.iter()
            .map(|arg| {
                Json::Obj(vec![
                    ("slot".to_string(), Json::UInt(arg.slot as u64)),
                    ("value".to_string(), value_json(&arg.value)),
                ])
            })
            .collect(),
    )
}

fn value_json(value: &TraceValue) -> Json {
    match value {
        TraceValue::Int(v) => Json::Obj(vec![
            ("t".to_string(), Json::Str("int".to_string())),
            ("v".to_string(), Json::Int(*v)),
        ]),
        TraceValue::Float(v) => Json::Obj(vec![
            ("t".to_string(), Json::Str("flt".to_string())),
            ("v".to_string(), Json::Float(*v)),
        ]),
        TraceValue::Str(bytes) => Json::Obj(vec![
            ("t".to_string(), Json::Str("str".to_string())),
            ("v".to_string(), bytes_json(bytes)),
        ]),
    }
}

/// 单字节 → U+00XX 的字符映射;写出时由 `write_string` 负责转义,
/// 读回时 `bytes_from_string` 只接受 ≤ U+00FF,因此逐字节往返。
fn bytes_json(bytes: &[u8]) -> Json {
    Json::Str(bytes.iter().map(|&b| b as char).collect())
}

fn json_type(value: &Json) -> &'static str {
    match value {
        Json::Null => "null",
        Json::Bool(_) => "bool",
        Json::UInt(_) => "unsigned integer",
        Json::Int(_) => "integer",
        Json::Float(_) => "float",
        Json::Str(_) => "string",
        Json::Arr(_) => "array",
        Json::Obj(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header() -> HeaderPayload {
        HeaderPayload {
            codec: "cp932".to_string(),
            producer: ProducerInfo::kernel(),
            tables: vec!["ysc: v495 (105 命令)".to_string()],
            boot: BootSummary::from_config(&yu_rt::BootConfig::default()),
            input: InputSummary {
                events: 0,
                sha256: "0".repeat(64),
            },
            clock: ClockSummary::none(),
            kernel: "yu-trace 0.1.0".to_string(),
        }
    }

    #[test]
    fn header_line_is_canonical() {
        let mut log = TraceLog::new();
        log.push_header(header());
        let line = log.to_jsonl();
        assert!(line.starts_with(&format!(
            "{{\"schema\":{},\"seq\":0,\"frame\":null,\"sub\":\"meta\",\"kind\":\"header\",\"payload\":{{",
            SCHEMA_VERSION
        )));
        assert!(line.ends_with("}\n"));
    }

    #[test]
    fn log_roundtrip_is_byte_identical() {
        let mut log = TraceLog::new();
        log.push_header(header());
        log.push(
            Some(3),
            Payload::VfsOpen {
                source: "mem".to_string(),
                key: b"ysbin/ysc.ybn".to_vec(),
                bytes: Some(4),
                sha256: Some("abcd".to_string()),
                diag: None,
            },
        );
        log.push(
            Some(3),
            Payload::VmText {
                id: String::new(),
                bytes: vec![0x83, 0x5c, 0x0a, 0xff],
            },
        );
        let first = log.to_jsonl();
        let parsed = TraceLog::parse(&first).unwrap();
        let second = parsed.to_jsonl();
        assert_eq!(first, second);
        assert_eq!(log, parsed);
    }

    #[test]
    fn rejects_unknown_schema_and_kind() {
        let mut log = TraceLog::new();
        log.push_header(header());
        let text = log.to_jsonl();
        let bad_schema = text.replace(
            &format!("\"schema\":{SCHEMA_VERSION}"),
            &format!("\"schema\":{}", SCHEMA_VERSION + 1),
        );
        let err = TraceLog::parse(&bad_schema).unwrap_err();
        assert_eq!(err.code, crate::error::code::SCHEMA);

        let bad_kind = text.replace("\"kind\":\"header\"", "\"kind\":\"nope\"");
        let err = TraceLog::parse(&bad_kind).unwrap_err();
        assert_eq!(err.code, crate::error::code::SCHEMA);
    }

    #[test]
    fn rejects_missing_field_and_truncation() {
        let mut log = TraceLog::new();
        log.push_header(header());
        let text = log.to_jsonl();
        let missing = text.replace("\"codec\":\"cp932\",", "");
        let err = TraceLog::parse(&missing).unwrap_err();
        assert_eq!(err.code, crate::error::code::PARSE);

        let mut truncated = text.clone();
        truncated.pop();
        let err = TraceLog::parse(&truncated).unwrap_err();
        assert_eq!(err.code, crate::error::code::TRUNCATED);
    }

    #[test]
    fn rejects_sequence_gap_and_header_position() {
        let mut log = TraceLog::new();
        log.push_header(header());
        log.push(Some(0), Payload::VmEnd { restart: false });
        let text = log.to_jsonl();
        let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
        lines[1] = lines[1].replacen("\"seq\":1", "\"seq\":2", 1);
        let joined = format!("{}\n", lines.join("\n"));
        let err = TraceLog::parse(&joined).unwrap_err();
        assert_eq!(err.code, crate::error::code::SCHEMA);
    }

    #[test]
    fn platform_kind_is_refused_but_frame_is_defined() {
        let platform = format!(
            "{{\"schema\":{SCHEMA_VERSION},\"seq\":0,\"frame\":null,\"sub\":\"platform\",\"kind\":\"platform_call\",\"payload\":{{}}}}\n"
        );
        let err = TraceLog::parse(&platform).unwrap_err();
        assert_eq!(err.code, crate::error::code::SCHEMA);

        // ③ 不再是保留 kind:合法的 frame_description 必须能被解析。
        let mut log = TraceLog::new();
        log.push_header(header());
        log.push_frame(
            FrameSummary::from_input(FrameDescriptionInput {
                frame: 7,
                level: FrameLevel::Event,
                pass_count: 1,
                op_count: 3,
                executable_ops: 2,
                skipped_ops: 1,
                display: Some(br#"{"ops":[1,2,3]}"#.to_vec()),
                display_sha256: None,
            })
            .unwrap(),
        );
        let text = log.to_jsonl();
        let parsed = TraceLog::parse(&text).unwrap();
        assert_eq!(parsed.to_jsonl(), text);
    }

    fn frame_input(display: Option<&[u8]>, hash: Option<&str>) -> FrameDescriptionInput {
        FrameDescriptionInput {
            frame: 7,
            level: FrameLevel::Event,
            pass_count: 1,
            op_count: 10,
            executable_ops: 9,
            skipped_ops: 1,
            display: display.map(|bytes| bytes.to_vec()),
            display_sha256: hash.map(str::to_string),
        }
    }

    #[test]
    fn frame_description_roundtrip_and_determinism() {
        let summary =
            FrameSummary::from_input(frame_input(Some(br#"{"b":2, "a":[true,null,"x"]}"#), None))
                .unwrap();
        assert_eq!(
            summary.display.as_ref().unwrap().canonical(),
            r#"{"b":2,"a":[true,null,"x"]}"#
        );
        let hash = summary.display_sha256.clone();
        assert_eq!(hash.len(), 64);

        let mut first = TraceLog::new();
        first.push_header(header());
        first.push_frame(summary.clone());
        let mut second = TraceLog::new();
        second.push_header(header());
        second.push_frame(summary);
        // 同一帧写入两次(两本日志)逐字节一致。
        assert_eq!(first.to_jsonl(), second.to_jsonl());

        let text = first.to_jsonl();
        let parsed = TraceLog::parse(&text).unwrap();
        assert_eq!(parsed.to_jsonl(), text);
        match parsed.records()[1].payload() {
            Payload::FrameDescription(summary) => {
                assert_eq!(summary.frame, 7);
                assert_eq!(summary.display_sha256, hash);
                assert_eq!(summary.skipped_ops, 1);
            }
            other => panic!("unexpected payload: {other:?}"),
        }
    }

    #[test]
    fn frame_description_requires_digest_and_rejects_bad_hash() {
        let err = FrameSummary::from_input(frame_input(None, None)).unwrap_err();
        assert_eq!(err.code, crate::error::code::PARSE);

        let err = FrameSummary::from_input(frame_input(None, Some("ABC"))).unwrap_err();
        assert_eq!(err.code, crate::error::code::PARSE);

        // 摘要与 display 内容不一致:显式拒绝,不做"取其一"的猜测。
        let err = FrameSummary::from_input(frame_input(Some(br#"{"a":1}"#), Some(&"0".repeat(64))))
            .unwrap_err();
        assert_eq!(err.code, crate::error::code::SCHEMA);

        // 摘要级记录(无 display)是合法的。
        let summary = FrameSummary::from_input(frame_input(None, Some(&"a".repeat(64)))).unwrap();
        assert!(summary.display.is_none());
    }

    #[test]
    fn frame_description_envelope_and_field_validation() {
        let summary = FrameSummary::from_input(frame_input(Some(b"{}"), None)).unwrap();
        let mut log = TraceLog::new();
        log.push_header(header());
        log.push_frame(summary);
        let text = log.to_jsonl();

        // 封套 frame 与 payload frame 必须一致。
        let mismatch = text.replacen(
            "\"frame\":7,\"sub\":\"frame\"",
            "\"frame\":8,\"sub\":\"frame\"",
            1,
        );
        let err = TraceLog::parse(&mismatch).unwrap_err();
        assert_eq!(err.code, crate::error::code::SCHEMA);

        // 缺 display 字段 = parse;未知字段 = schema。
        let missing = text.replace(",\"display\":{}", "");
        let err = TraceLog::parse(&missing).unwrap_err();
        assert_eq!(err.code, crate::error::code::PARSE);
        let unknown = text.replacen("\"display\":{}", "\"display\":{},\"extra\":1", 1);
        let err = TraceLog::parse(&unknown).unwrap_err();
        assert_eq!(err.code, crate::error::code::SCHEMA);

        // 篡改 display 内容但保留旧摘要 = schema(内容寻址失败)。
        let tampered = text.replacen("\"display\":{}", "\"display\":{\"x\":1}", 1);
        let err = TraceLog::parse(&tampered).unwrap_err();
        assert_eq!(err.code, crate::error::code::SCHEMA);
    }

    #[test]
    fn header_clock_roundtrip_and_validation() {
        let mut log = TraceLog::new();
        let mut h = header();
        h.clock = ClockSummary::from_ticks(&[Some(0), None, Some(32)]);
        assert_eq!(h.clock.mode, ClockMode::Mixed);
        log.push_header(h);
        let text = log.to_jsonl();
        let parsed = TraceLog::parse(&text).unwrap();
        assert_eq!(parsed.to_jsonl(), text);
        assert_eq!(parsed.header().unwrap().clock.mode, ClockMode::Mixed);

        // 非法的 mode/list 组合必须显式拒绝。
        for (mode, now_ms) in [
            ("tick_at", vec![]),
            ("tick", vec![Some(1)]),
            ("none", vec![None]),
            ("mixed", vec![Some(1), Some(2)]),
        ] {
            let clock = ClockSummary {
                mode: ClockMode::parse(mode).unwrap(),
                now_ms,
            };
            let invalid = HeaderPayload {
                clock,
                ..parsed.header().unwrap().clone()
            };
            let line = Record {
                seq: 0,
                frame: None,
                kind: Kind::Header,
                payload: Payload::Header(invalid),
            };
            let err = TraceLog::parse(&format!("{}\n", line.to_line())).unwrap_err();
            assert_eq!(
                err.code,
                crate::error::code::SCHEMA,
                "clock mode {mode} with invalid list must be schema"
            );
        }
    }

    #[test]
    fn vfs_list_failure_with_entries_is_schema() {
        let line = format!(
            "{{\"schema\":{},\"seq\":1,\"frame\":null,\"sub\":\"vfs\",\"kind\":\"list\",\"payload\":{{\"source\":\"mem\",\"key\":\"x\",\"entries\":[{{\"name\":\"a\",\"kind\":\"file\",\"size\":1}}],\"diag\":\"vfs.missing\"}}}}",
            SCHEMA_VERSION
        );
        let err = Record::parse_line(2, &line).unwrap_err();
        assert_eq!(err.code, crate::error::code::SCHEMA);
    }

    #[test]
    #[should_panic(expected = "push_header/prepend_header")]
    fn push_rejects_header_payload() {
        let mut log = TraceLog::new();
        log.push(None, Payload::Header(header()));
    }

    #[test]
    fn boot_summary_rebuilds_config_and_rejects_injection_mismatch() {
        let mut cfg = yu_rt::BootConfig {
            argv: vec![b"-window".to_vec(), b"1280x720".to_vec()],
            debug_mode: true,
            script_key: [0xd3, 0x6f, 0xac, 0x96],
            screen_color: 32,
            media_readonly: true,
            os_sound: false,
            dx_mode: 1,
            ..yu_rt::BootConfig::default()
        };
        cfg.paths.save_path = Some(b"save".to_vec());
        let summary = BootSummary::from_config(&cfg);
        assert_eq!(summary.argv_len, 2);
        assert_eq!(summary.script_key, [0xd3, 0x6f, 0xac, 0x96]);
        assert!(summary.debug_mode);
        assert_eq!(summary.host.screen_color, 32);
        assert_eq!(summary.host.paths_len, 1);

        let rebuilt = summary
            .to_config(cfg.argv.clone(), cfg.paths.clone())
            .expect("same injections");
        assert_eq!(rebuilt.entry_label, cfg.entry_label);
        assert_eq!(rebuilt.fps, cfg.fps);
        assert_eq!(rebuilt.start_ms, cfg.start_ms);
        assert_eq!(rebuilt.frame_ms, cfg.frame_ms);
        assert_eq!(rebuilt.script_key, cfg.script_key);
        assert_eq!(rebuilt.debug_mode, cfg.debug_mode);
        assert_eq!(rebuilt.argv, cfg.argv);
        assert_eq!(rebuilt.screen_color, cfg.screen_color);
        assert_eq!(rebuilt.media_readonly, cfg.media_readonly);
        assert_eq!(rebuilt.os_sound, cfg.os_sound);
        assert_eq!(rebuilt.dx_mode, cfg.dx_mode);

        let err = summary
            .to_config(vec![b"-other".to_vec()], cfg.paths.clone())
            .unwrap_err();
        assert_eq!(err.code, crate::error::code::REPLAY);
        let err = summary
            .to_config(Vec::new(), cfg.paths.clone())
            .unwrap_err();
        assert_eq!(err.code, crate::error::code::REPLAY);
        let err = summary
            .to_config(cfg.argv.clone(), yu_rt::vm::HostPaths::default())
            .unwrap_err();
        assert_eq!(err.code, crate::error::code::REPLAY);
    }
}
