//! 宿主 UI 契约:`DIALOG` 的呈现视图 + 回答(所有壳共用)。
//!
//! 内核权威状态仍是 `yu_rt::dialog::DialogRequest`(CP932 原始字节,
//! `LET/LETSTR` 写回也由内核完成)。本模块只做**呈现层**的稳定映射,
//! 不改变任何脚本语义:
//!
//! - Rust 壳(桌面 egui)直接用 [`HostDialog`] / [`HostDialogAnswer`];
//! - 非 Rust 壳(Android JNI、后续 wasm/serve)用 [`dialogs_json`] 的
//!   稳定 JSON 契约。
//!
//! 官方 `MessageBoxA` 的按钮语义是 1 基序号(见 `yu_rt::dialog`),所以
//! 这里只给**角色名**(`ok/cancel/yes/no`),按钮文案由各壳按自己的
//! 语言/主题决定 —— 这就是「统一接口,实现看壳」。
//!
//! 证据锚点:`yu_rt/src/dialog.rs`(手册 `other/dialog.html` + REA
//! `0x43fc74` 的 MessageBoxA 映射);呈现/回答分离是内核侧既有契约,
//! 不是新语义。

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, OnceLock};

use crate::engine::Engine;
use crate::json::{self, Value};
use yu_rt::dialog::DialogRequest;

pub use yu_rt::dialog::{DialogButtons, DialogIcon};

/// DIALOG 的来源。
///
/// - [`HostDialogSource::Script`]:脚本 `DIALOG` 命令,回答会写回 `LET/LETSTR`;
/// - [`HostDialogSource::Host`]:内核/宿主自己的运行期**错误**(Fatal 诊断、
///   脚本错误、启动/tick 失败、panic),和脚本 DIALOG 走**同一份快照与
///   同一个呈现器**,但没有 VM 写回 —— 壳确认后仅关闭。
///   警告(Recoverable)不走这里,见 [`HostAlertQueue::take_hint`]。
///
/// 用同一个契约的原因:官方错误路径就是 `MessageBoxA`;Android/CLI/桌面
/// 不应各写一套错误上屏,DIALOG 已经具备按钮、输入框、模态队列与本地化
/// 角色,错误只是其中一种 `source`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostDialogSource {
    Script,
    Host,
}

impl HostDialogSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            HostDialogSource::Script => "script",
            HostDialogSource::Host => "host",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "script" => Some(HostDialogSource::Script),
            "host" => Some(HostDialogSource::Host),
            _ => None,
        }
    }
}

/// 一条待回答 DIALOG 的**呈现视图**(CP932 已解码;内核语义仍是原始字节)。
///
/// 所有字段都是宿主只读数据;回答走 [`HostDialogAnswer`]。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostDialog {
    /// 内核分配的稳定 ID(宿主回答时回传)。
    ///
    /// 宿主消息使用 [`HOST_DIALOG_ID_BASE`] 以上的 ID,与 VM 分配的低位
    /// ID 不冲突;`answer_dialog` 据此分流。
    pub id: u64,
    /// 来源(脚本 DIALOG / 宿主消息)。
    pub source: HostDialogSource,
    /// 发起任务 ID(诊断用;呈现层可忽略)。
    pub task: String,
    /// `CAPTION` 的显示文本。
    pub caption: String,
    /// `STR` 的显示文本。
    pub text: String,
    /// 按钮组合(1 基序号即 `LET` 的写回值)。
    pub buttons: DialogButtons,
    /// `DEFBUTTON` 原值(0 = 未指定;1..4 = 左起第几个)。
    pub default_button: u8,
    /// `ICON` 映射结果。
    pub icon: DialogIcon,
    /// `INPUT=…` 是否存在(存在 = 文本输入框;按钮固定为 OK)。
    pub input: bool,
    /// `INPUTLIMIT`(0 = 不限;壳侧输入框应据此限制字符数)。
    pub input_limit: u32,
    /// `DEFSTR`(输入框初始文本)。
    pub default_input: String,
}

impl HostDialog {
    /// 从内核请求映射出呈现视图(只做显示解码,不改原始字节)。
    pub fn from_request(request: &DialogRequest) -> Self {
        Self {
            id: request.id,
            source: HostDialogSource::Script,
            task: request.task.clone(),
            caption: request.caption_display(),
            text: request.text_display(),
            buttons: request.buttons,
            default_button: request.default_button,
            icon: request.icon,
            input: request.input,
            input_limit: request.input_limit,
            default_input: request.default_input_display(),
        }
    }

    /// 第 `index`(1 基)个按钮的**角色名**(`ok/cancel/yes/no`)。
    ///
    /// 壳据此决定本地化文案;越界返回空串(与内核 `DialogButtons::label`
    /// 同口径)。
    pub fn button_role(&self, index: u8) -> &'static str {
        self.buttons.label(index)
    }
}

/// 宿主对一条 DIALOG 的回答。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostDialogAnswer {
    /// 对应 [`HostDialog::id`]。
    pub id: u64,
    /// 1 基按钮序号(官方 MessageBox 语义,写回 `LET`)。
    pub button: u8,
    /// `INPUT` 模式的编辑文本(Unicode;由 [`Engine::answer_host_dialog`]
    /// 统一编码回 CP932)。非 INPUT 模式必须为 `None`。
    pub input_text: Option<String>,
}

impl Engine {
    /// 当前待回答 DIALOG 的呈现视图(按发起顺序)。
    ///
    /// 内核只持有事实;宿主负责呈现并经 [`Engine::answer_host_dialog`]
    /// 回答。官方 MessageBox 是模态的,正常至多一条(多任务可同时挂起)。
    pub fn host_dialogs(&self) -> Vec<HostDialog> {
        self.pending_dialogs()
            .iter()
            .map(HostDialog::from_request)
            .collect()
    }

    /// 回答一条 DIALOG(统一入口:输入框文本按对话框 codec 编码)。
    ///
    /// - `button`:1 基按钮序号;越界由内核拒绝。
    /// - `input_text`:`INPUT=1` 时的编辑文本;编码失败(字符不在该 codec)
    ///   原样返回 `Diag`,宿主应保持对话框打开让用户修改。
    ///
    /// 回答后任务在下一个逻辑帧从 `DIALOG` 的下一条继续(pc 在分派前已
    /// 前进,与官方引擎一致;见 `yu_rt/src/vm.rs::answer_dialog`)。
    pub fn answer_host_dialog(&mut self, answer: &HostDialogAnswer) -> Result<(), yu_log::Diag> {
        // 用发起对话框时记录的 codec 编码输入文本(UTF-8 补丁版/GBK 版
        // 的输入不能按 CP932 写回)。
        let codec = self
            .pending_dialogs()
            .iter()
            .find(|d| d.id == answer.id)
            .map_or(yu_format::Codec::Cp932, |d| d.codec);
        let text = match &answer.input_text {
            Some(input) => Some(DialogRequest::encode_input_with(codec, input)?),
            None => None,
        };
        self.answer_dialog(answer.id, answer.button, text.as_deref())
    }
}

/// 把呈现视图序列化成稳定的 JSON 契约(非 Rust 壳使用)。
///
/// 顶层固定 `{"protocol":1,"dialogs":[...]}`;数组为空时也返回合法 JSON。
/// 字符串是 UTF-8 显示文本(CP932 已解码),壳不需要也不应重新解码;
/// `buttons` 按 1 基顺序给角色名,壳自己映射成本地化文案。
pub fn dialogs_json(dialogs: &[HostDialog]) -> String {
    let mut entries = Vec::with_capacity(dialogs.len());
    for dialog in dialogs {
        let mut map = BTreeMap::new();
        map.insert("id".to_string(), Value::Number(dialog.id as f64));
        map.insert(
            "source".to_string(),
            Value::String(dialog.source.as_str().to_string()),
        );
        map.insert("task".to_string(), Value::String(dialog.task.clone()));
        map.insert("caption".to_string(), Value::String(dialog.caption.clone()));
        map.insert("text".to_string(), Value::String(dialog.text.clone()));
        let buttons = (1..=dialog.buttons.count())
            .map(|index| Value::String(dialog.button_role(index).to_string()))
            .collect();
        map.insert("buttons".to_string(), Value::Array(buttons));
        map.insert(
            "default_button".to_string(),
            Value::Number(f64::from(dialog.default_button)),
        );
        map.insert(
            "icon".to_string(),
            Value::String(dialog.icon.as_str().to_string()),
        );
        map.insert("input".to_string(), Value::Bool(dialog.input));
        map.insert(
            "input_limit".to_string(),
            Value::Number(f64::from(dialog.input_limit)),
        );
        map.insert(
            "default_input".to_string(),
            Value::String(dialog.default_input.clone()),
        );
        entries.push(Value::Object(map));
    }
    let mut root = BTreeMap::new();
    root.insert("protocol".to_string(), Value::Number(1.0));
    root.insert("dialogs".to_string(), Value::Array(entries));
    json::to_compact(&Value::Object(root))
}

// ---------------------------------------------------------------- 运行期消息
//
// 脚本 `DIALOG` 是**脚本语义**(任务挂起等回答);本区是**运行状态**:
// 内核 Fatal 诊断、执行器结束任务的脚本错误、宿主 panic、启动/tick 失败
// 会进模态 DIALOG(官方 `YU-RIS Debug Info` MessageBox);Recoverable 警告
// 只进非模态提示,不阻塞游戏。
//
// 为什么不在 `Engine` 里持队列:panic/启动失败发生在会话之外(甚至引擎
// 尚未建立),宿主必须能自行入库。队列放在契约层,由各宿主各持一份。

/// 运行期消息的严重级。
///
/// 与 [`yu_log::Severity`] 分开:后者是**诊断挂号**级别(含 Unimplemented/
/// Info/Verbose),前者是**用户可见**的通知级别。Fatal 诊断进模态 DIALOG;
/// Recoverable 只进非模态提示;其余只进诊断面板。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AlertSeverity {
    Info,
    Warning,
    Error,
    Fatal,
}

impl AlertSeverity {
    pub const fn as_str(self) -> &'static str {
        match self {
            AlertSeverity::Info => "info",
            AlertSeverity::Warning => "warning",
            AlertSeverity::Error => "error",
            AlertSeverity::Fatal => "fatal",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "info" => Some(AlertSeverity::Info),
            "warning" => Some(AlertSeverity::Warning),
            "error" => Some(AlertSeverity::Error),
            "fatal" => Some(AlertSeverity::Fatal),
            _ => None,
        }
    }

    /// 是否需要模态确认。
    ///
    /// Error/Fatal 必须模态(官方错误 MessageBox 语义);Warning 只做非模态
    /// 提示(Toast/横幅),不打断游戏。
    pub const fn is_modal(self) -> bool {
        matches!(self, AlertSeverity::Error | AlertSeverity::Fatal)
    }

    /// 内核诊断级别 → 宿主通知级别。
    ///
    /// `Unimplemented`/`Info`/`Verbose` 返回 `None`:它们是挂号/调试噪声,
    /// 每帧都可能出现,不能弹给用户(桌面诊断面板仍可见)。
    pub const fn from_diag(severity: yu_log::Severity) -> Option<Self> {
        match severity {
            yu_log::Severity::Fatal => Some(AlertSeverity::Fatal),
            yu_log::Severity::Recoverable => Some(AlertSeverity::Warning),
            yu_log::Severity::Unimplemented
            | yu_log::Severity::Info
            | yu_log::Severity::Verbose => None,
        }
    }
}

/// 一条待呈现的运行期消息(与脚本 DIALOG 分开)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostAlert {
    /// 宿主队列分配的稳定 ID(确认时回传)。
    pub id: u64,
    pub severity: AlertSeverity,
    /// 标题(内核侧给官方标题;壳可按 severity/语言覆盖)。
    pub title: String,
    /// 一句话正文(用户先看到的内容)。
    pub text: String,
    /// 诊断码(脚本错误为空;panic 为 `host.panic`)。
    pub code: String,
    /// 来源:`engine` / `script` / `panic` / `boot`。
    pub source: String,
    /// 位置(`file@pc` / `file:line:col`;未知为空)。
    pub location: String,
    /// 详情(诊断字段、backtrace;壳的“详情”展开用)。
    pub detail: String,
    /// 是否必须模态确认。与 [`AlertSeverity::is_modal`] 一致,单独给出是
    /// 为了让队列构造的特殊消息(如启动失败)可以覆盖。
    pub modal: bool,
}

impl HostAlert {
    fn new(
        severity: AlertSeverity,
        title: impl Into<String>,
        text: impl Into<String>,
        code: impl Into<String>,
        source: impl Into<String>,
    ) -> Self {
        Self {
            id: 0,
            severity,
            title: title.into(),
            text: text.into(),
            code: code.into(),
            source: source.into(),
            location: String::new(),
            detail: String::new(),
            modal: severity.is_modal(),
        }
    }
}

/// 队列最多保留的未确认**模态**消息数。
///
/// 警告不进这个队列(见 `MAX_HINTS`);达到上限后新来的脚本错误等直接
/// 丢弃(诊断面板仍能看到原始 `Diag`)。panic/启动失败总是入库。
pub const MAX_ALERTS: usize = 32;

/// 宿主消息的 DIALOG ID 基址。
///
/// VM 分配的 DIALOG ID 是小整数;宿主消息用这一区间,`answer_dialog`
/// 据此区分「回答脚本 DIALOG」与「确认宿主消息」。
///
/// **必须落在 JSON 数字的精确整数范围内**:当前 `host_ui` 的 JSON 用
/// `f64` 承载数字,`2^53` 以上会丢精度(实测 `1<<62` 被序列化成
/// `4611686018427388000`,壳确认时反查不到原 ID);Java/JS 还会在
/// `i64::MAX` 处饱和。`1 << 40` 精确、且离 VM 的小整数有足够隔离带。
pub const HOST_DIALOG_ID_BASE: u64 = 1 << 40;

/// 非模态提示最多保留的条数(壳每次取最新一条,其余丢弃)。
pub const MAX_HINTS: usize = 16;

/// 宿主侧告警队列(每个宿主一份;线程安全由宿主自己包)。
#[derive(Debug)]
pub struct HostAlertQueue {
    next_id: u64,
    alerts: Vec<HostAlert>,
    /// 已入库过的诊断指纹 `(code, location, detail)`。
    ///
    /// 同一处诊断每帧重复产生时不重复弹窗;用内容而不是 code 做键,是为了
    /// 让“同一 code 在不同脚本位置”仍能各报一次。
    seen: BTreeSet<(String, String, String)>,
    /// 非模态提示(Recoverable 诊断):不阻塞游戏,壳用 Toast/横幅提示。
    hints: Vec<String>,
    /// 提示去重(同一 `code + detail` 只提示一次)。
    hint_seen: BTreeSet<(String, String)>,
}

impl Default for HostAlertQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl HostAlertQueue {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            next_id: 1,
            alerts: Vec::new(),
            seen: BTreeSet::new(),
            hints: Vec::new(),
            hint_seen: BTreeSet::new(),
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.alerts.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.alerts.len()
    }

    #[must_use]
    pub fn alerts(&self) -> &[HostAlert] {
        &self.alerts
    }

    /// 清空队列与会话内去重表(会话销毁时调用)。
    pub fn clear(&mut self) {
        self.alerts.clear();
        self.seen.clear();
        self.hints.clear();
        self.hint_seen.clear();
        self.next_id = 1;
    }

    fn push(&mut self, mut alert: HostAlert) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        alert.id = id;
        self.alerts.push(alert);
        id
    }

    fn push_hint(&mut self, diag: &yu_log::Diag) {
        let key = (diag.code.to_string(), diag.detail.clone());
        if !self.hint_seen.insert(key) {
            return;
        }
        let mut text = diag.code.to_string();
        if !diag.detail.is_empty() {
            text.push_str(": ");
            text.push_str(&diag.detail);
        }
        if self.hints.len() >= MAX_HINTS {
            self.hints.remove(0);
        }
        self.hints.push(text);
    }

    /// 取走最新一条非模态提示(其余丢弃);壳每次 tick 最多提示一次。
    pub fn take_hint(&mut self) -> Option<String> {
        let latest = self.hints.pop();
        self.hints.clear();
        latest
    }

    /// 一条内核诊断入库;返回分配的消息 ID,`0` = 不需要弹窗/已去重。
    ///
    /// Fatal 升级成模态 DIALOG;Recoverable(警告)只进**非模态提示**
    /// (壳用 Toast/横幅),否则每帧资源缺失会把游戏堵死。
    pub fn ingest_diag(&mut self, diag: &yu_log::Diag) -> u64 {
        let Some(severity) = AlertSeverity::from_diag(diag.severity) else {
            return 0;
        };
        if !severity.is_modal() {
            self.push_hint(diag);
            return 0;
        }
        let location = format_loc(&diag.where_);
        let key = (
            diag.code.to_string(),
            location.clone(),
            diag.detail.clone(),
        );
        if !self.seen.insert(key) {
            return 0;
        }
        if self.alerts.len() >= MAX_ALERTS && severity < AlertSeverity::Fatal {
            return 0;
        }
        let text = if diag.detail.is_empty() {
            diag.code.to_string()
        } else {
            diag.detail.clone()
        };
        let mut alert =
            HostAlert::new(severity, "YU-RIS Debug Info", text, diag.code.to_string(), "engine");
        alert.location = location;
        alert.detail = diag_detail(diag);
        self.push(alert)
    }

    /// 执行器结束的任务留下的脚本错误入库;同一文本只入库一次。
    pub fn ingest_script_error(&mut self, error: &str) -> u64 {
        let key = (
            "script.error".to_string(),
            String::new(),
            error.to_string(),
        );
        if !self.seen.insert(key) {
            return 0;
        }
        if self.alerts.len() >= MAX_ALERTS {
            return 0;
        }
        let mut alert =
            HostAlert::new(AlertSeverity::Error, "YU-RIS Debug Info", error, "script.error", "script");
        alert.detail = error.to_string();
        self.push(alert)
    }

    /// 运行期 tick/呈现失败入库(总是新增)。
    pub fn push_runtime_error(&mut self, message: &str) -> u64 {
        let mut alert = HostAlert::new(
            AlertSeverity::Error,
            "运行错误",
            if message.is_empty() { "内核运行失败" } else { message },
            "host.runtime",
            "host",
        );
        alert.detail = message.to_string();
        self.push(alert)
    }

    /// 宿主捕获的 panic 入库(总是新增,不受去重/上限影响)。
    pub fn push_panic(&mut self, message: &str, location: &str, backtrace: &str) -> u64 {
        let mut alert = HostAlert::new(
            AlertSeverity::Fatal,
            "内核 Panic",
            if message.is_empty() { "内核异常终止" } else { message },
            "host.panic",
            "panic",
        );
        alert.location = location.to_string();
        alert.detail = if backtrace.is_empty() {
            format!("panic at {location}")
        } else {
            format!("panic at {location}\n{backtrace}")
        };
        self.push(alert)
    }

    /// 启动/初始化失败入库(总是新增)。
    pub fn push_boot_error(&mut self, message: &str) -> u64 {
        let mut alert = HostAlert::new(
            AlertSeverity::Error,
            "启动失败",
            if message.is_empty() { "内核没有返回失败原因" } else { message },
            "host.boot",
            "boot",
        );
        alert.detail = message.to_string();
        self.push(alert)
    }

    /// 壳确认/关闭一条消息;不存在返回 `false`。
    pub fn dismiss(&mut self, id: u64) -> bool {
        let before = self.alerts.len();
        self.alerts.retain(|alert| alert.id != id);
        before != self.alerts.len()
    }

    /// 把当前未确认消息映射成 DIALOG 视图(与脚本 DIALOG 同一条呈现链)。
    ///
    /// 宿主消息没有 VM 写回,所以固定 `Ok` 单按钮、`input=false`;
    /// 诊断详情并入正文(空行分隔),壳不需要为错误另写一套布局。
    #[must_use]
    pub fn dialogs(&self) -> Vec<HostDialog> {
        self.alerts
            .iter()
            .map(|alert| {
                let mut text = alert.text.clone();
                if !alert.detail.is_empty() && alert.detail != alert.text {
                    text.push_str("\n\n");
                    text.push_str(&alert.detail);
                }
                HostDialog {
                    id: HOST_DIALOG_ID_BASE.saturating_add(alert.id.saturating_sub(1)),
                    source: HostDialogSource::Host,
                    task: "host".to_string(),
                    caption: alert.title.clone(),
                    text,
                    buttons: DialogButtons::Ok,
                    default_button: 1,
                    icon: match alert.severity {
                        AlertSeverity::Fatal | AlertSeverity::Error => DialogIcon::Error,
                        AlertSeverity::Warning => DialogIcon::Warning,
                        AlertSeverity::Info => DialogIcon::Information,
                    },
                    input: false,
                    input_limit: 0,
                    default_input: String::new(),
                }
            })
            .collect()
    }

    /// 确认一条宿主 DIALOG(按 DIALOG ID);不是宿主 ID 返回 `false`。
    pub fn dismiss_dialog(&mut self, id: u64) -> bool {
        let Some(sequence) = id.checked_sub(HOST_DIALOG_ID_BASE) else {
            return false;
        };
        self.dismiss(sequence.saturating_add(1))
    }
}

fn format_loc(loc: &yu_log::Loc) -> String {
    match loc {
        yu_log::Loc::Script {
            file,
            label,
            pc,
            cmd,
        } => {
            let mut out = match label {
                Some(label) => format!("{file}:{label}@{pc}"),
                None => format!("{file}@{pc}"),
            };
            if let Some(cmd) = cmd {
                out.push_str(&format!(" ({cmd})"));
            }
            out
        }
        yu_log::Loc::Data { path, offset } => {
            format!("{}@{offset}", String::from_utf8_lossy(path))
        }
        yu_log::Loc::Table { name } => format!("table:{name}"),
        yu_log::Loc::None => String::new(),
    }
}

fn diag_detail(diag: &yu_log::Diag) -> String {
    let mut out = format!(
        "code: {}\nseverity: {}\nboundary: {}",
        diag.code,
        diag.severity.as_str(),
        diag.boundary.as_str()
    );
    if !diag.detail.is_empty() {
        out.push_str("\ndetail: ");
        out.push_str(&diag.detail);
    }
    let location = format_loc(&diag.where_);
    if !location.is_empty() {
        out.push_str("\nlocation: ");
        out.push_str(&location);
    }
    for (index, cause) in diag.causes.iter().enumerate() {
        out.push_str(&format!(
            "\ncause[{}]: {} — {}",
            index + 1,
            cause.code,
            cause.detail
        ));
    }
    out
}

// ---------------------------------------------------------------- 启动进度
//
// 手机冷启动要挂 YPF/装载表/初始化渲染,黑屏时间肉眼可见。内核把**阶段**
// 暴露成稳定快照,壳负责动画(spinner/进度条);不是平台语义,不参与 VM。
// 跨平台只有一个消费入口:boot_progress_json()。

/// 启动阶段快照。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BootProgress {
    /// true = 启动进行中(壳显示 spinner)。
    pub active: bool,
    /// 稳定阶段键(project/mount/tables/vm/render/ready)。
    pub stage: String,
    /// 显示用说明(壳可直接展示;可本地化覆盖)。
    pub detail: String,
}

fn boot_progress_state() -> &'static Mutex<BootProgress> {
    static STATE: OnceLock<Mutex<BootProgress>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(BootProgress::default()))
}

/// 内核启动阶段上报(引擎内部调用)。
pub fn report_boot_progress(stage: &str, detail: &str) {
    let mut state = boot_progress_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    state.active = true;
    state.stage.clear();
    state.stage.push_str(stage);
    state.detail.clear();
    state.detail.push_str(detail);
}

/// 启动流程结束(成功/失败都调用);壳据此隐藏 spinner。
pub fn finish_boot_progress() {
    let mut state = boot_progress_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    state.active = false;
    state.stage.clear();
    state.stage.push_str("ready");
    state.detail.clear();
}

/// 当前启动进度(壳轮询;可从任意线程调用)。
#[must_use]
pub fn boot_progress() -> BootProgress {
    boot_progress_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// 启动进度的稳定 JSON 契约(非 Rust 壳的唯一入口)。
#[must_use]
pub fn boot_progress_json() -> String {
    let progress = boot_progress();
    let mut map = BTreeMap::new();
    map.insert("active".to_string(), Value::Bool(progress.active));
    map.insert("stage".to_string(), Value::String(progress.stage));
    map.insert("detail".to_string(), Value::String(progress.detail));
    json::to_compact(&Value::Object(map))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cp932(text: &str) -> Vec<u8> {
        yu_format::Codec::Cp932
            .encode(text, &[])
            .expect("测试文本必须可编码")
    }

    fn input_request() -> DialogRequest {
        DialogRequest {
            id: 7,
            task: "es_Scenario".to_string(),
            caption: cp932("名前"),
            text: cp932("名前を入力してください"),
            buttons: DialogButtons::Ok,
            default_button: 1,
            icon: DialogIcon::None,
            input: true,
            input_limit: 0,
            default_input: cp932("彼女のクラブはあのクラブ"),
            let_target: None,
            letstr_target: None,
            ignored_params: Vec::new(),
            codec: yu_format::Codec::Cp932,
        }
    }

    fn yes_no_request() -> DialogRequest {
        DialogRequest {
            id: 8,
            task: "es.IDSubTask".to_string(),
            caption: cp932("確認"),
            text: cp932("終了しますか?"),
            buttons: DialogButtons::YesNo,
            default_button: 1,
            icon: DialogIcon::Question,
            input: false,
            input_limit: 0,
            default_input: Vec::new(),
            let_target: None,
            letstr_target: None,
            ignored_params: Vec::new(),
            codec: yu_format::Codec::Cp932,
        }
    }

    #[test]
    fn host_dialog_maps_display_text_and_semantics() {
        let dialog = HostDialog::from_request(&input_request());
        assert_eq!(dialog.id, 7);
        assert_eq!(dialog.task, "es_Scenario");
        assert_eq!(dialog.caption, "名前");
        assert_eq!(dialog.text, "名前を入力してください");
        assert_eq!(dialog.default_input, "彼女のクラブはあのクラブ");
        assert!(dialog.input);
        assert_eq!(dialog.buttons, DialogButtons::Ok);
        assert_eq!(dialog.button_role(1), "ok");
        assert_eq!(dialog.button_role(2), "", "OK 型没有第 2 个按钮");
    }

    #[test]
    fn host_dialogs_json_has_stable_contract() {
        let dialogs = vec![
            HostDialog::from_request(&input_request()),
            HostDialog::from_request(&yes_no_request()),
        ];
        let text = dialogs_json(&dialogs);
        let value = json::parse(&text).expect("必须是合法 JSON");
        assert_eq!(value.get("protocol").and_then(Value::as_u64), Some(1));
        let list = value
            .get("dialogs")
            .and_then(Value::as_array)
            .expect("dialogs 数组");
        assert_eq!(list.len(), 2);

        let input = &list[0];
        assert_eq!(input.get("id").and_then(Value::as_u64), Some(7));
        assert_eq!(
            input.get("source").and_then(Value::as_str),
            Some("script"),
            "脚本 DIALOG 必须标 source=script"
        );
        assert_eq!(input.get("caption").and_then(Value::as_str), Some("名前"));
        assert_eq!(input.get("input").and_then(Value::as_bool), Some(true));
        assert_eq!(
            input.get("default_input").and_then(Value::as_str),
            Some("彼女のクラブはあのクラブ")
        );
        let buttons = input
            .get("buttons")
            .and_then(Value::as_array)
            .expect("buttons 数组");
        assert_eq!(buttons.len(), 1);
        assert_eq!(buttons[0].as_str(), Some("ok"));

        let yes_no = &list[1];
        assert_eq!(
            yes_no.get("icon").and_then(Value::as_str),
            Some("question")
        );
        let buttons = yes_no
            .get("buttons")
            .and_then(Value::as_array)
            .expect("buttons 数组");
        let roles: Vec<&str> = buttons.iter().filter_map(Value::as_str).collect();
        assert_eq!(roles, vec!["yes", "no"], "按钮角色按 1 基顺序");
        assert_eq!(yes_no.get("input").and_then(Value::as_bool), Some(false));
    }

    fn fatal_diag() -> yu_log::Diag {
        yu_log::Diag::fatal("rt.expr.div_zero", yu_log::Boundary::Script)
            .with_detail("整数除以 0")
            .at(yu_log::Loc::script(
                "es_Scenario",
                Some("SHOW".to_string()),
                321,
                Some("LET".to_string()),
            ))
    }

    #[test]
    fn alert_severity_maps_only_user_visible_diags() {
        assert_eq!(
            AlertSeverity::from_diag(yu_log::Severity::Fatal),
            Some(AlertSeverity::Fatal)
        );
        assert_eq!(
            AlertSeverity::from_diag(yu_log::Severity::Recoverable),
            Some(AlertSeverity::Warning)
        );
        assert_eq!(
            AlertSeverity::from_diag(yu_log::Severity::Unimplemented),
            None,
            "未实现挂号只进诊断面板"
        );
        assert_eq!(
            AlertSeverity::from_diag(yu_log::Severity::Info),
            None
        );
        assert!(AlertSeverity::Fatal.is_modal());
        assert!(!AlertSeverity::Warning.is_modal());
    }

    #[test]
    fn diag_alert_dedupes_and_dismiss_removes() {
        let mut queue = HostAlertQueue::new();
        let diag = fatal_diag();
        let id = queue.ingest_diag(&diag);
        assert!(id > 0);
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.ingest_diag(&diag), 0, "同一条诊断不应重复弹");
        assert_eq!(queue.alerts()[0].severity, AlertSeverity::Fatal);
        assert_eq!(queue.alerts()[0].source, "engine");
        assert_eq!(queue.alerts()[0].location, "es_Scenario:SHOW@321 (LET)");
        assert!(queue.alerts()[0].detail.contains("code: rt.expr.div_zero"));
        assert!(!queue.alerts()[0].detail.contains("cause["));

        assert!(queue.dismiss(id));
        assert!(queue.is_empty());
        assert!(!queue.dismiss(id), "重复 dismiss 必须显式失败");
    }

    #[test]
    fn unimplemented_diag_stays_out_of_alert_queue() {
        let mut queue = HostAlertQueue::new();
        let diag = yu_log::Diag::unimplemented("rt.cmd.unimplemented", yu_log::Boundary::Script)
            .with_detail("CMD 9999");
        assert_eq!(queue.ingest_diag(&diag), 0);
        assert!(queue.is_empty());
    }

    #[test]
    fn panic_and_boot_alerts_are_always_new() {
        let mut queue = HostAlertQueue::new();
        let first = queue.push_panic("boom", "src/lib.rs:1:1", "backtrace");
        let second = queue.push_panic("boom", "src/lib.rs:1:1", "backtrace");
        assert_ne!(first, second, "两次 panic 必须各自可确认");
        assert_eq!(queue.alerts().len(), 2);
        let boot = queue.push_boot_error("data.ypf 不存在");
        assert_eq!(queue.alerts()[2].severity, AlertSeverity::Error);
        assert_eq!(queue.alerts()[2].source, "boot");
        assert!(queue.dismiss(boot));
    }

    #[test]
    fn script_errors_dedupe_by_text() {
        let mut queue = HostAlertQueue::new();
        assert!(queue.ingest_script_error("text internal error") > 0);
        assert_eq!(queue.ingest_script_error("text internal error"), 0);
        assert_eq!(queue.alerts()[0].severity, AlertSeverity::Error);
        assert_eq!(queue.alerts()[0].code, "script.error");
    }

    #[test]
    fn host_alerts_are_presented_as_dialogs() {
        let mut queue = HostAlertQueue::new();
        let id = queue.push_panic("a\"b\\c\n", "es@1", "line1\nline2");
        assert!(id > 0);
        let dialogs = queue.dialogs();
        assert_eq!(dialogs.len(), 1);
        let dialog = &dialogs[0];
        assert_eq!(dialog.source, HostDialogSource::Host);
        assert_eq!(dialog.id, HOST_DIALOG_ID_BASE);
        assert_eq!(dialog.caption, "内核 Panic");
        assert_eq!(dialog.buttons, DialogButtons::Ok);
        assert_eq!(dialog.icon, DialogIcon::Error);
        assert!(!dialog.input);
        assert!(
            dialog.text.contains("line1\nline2"),
            "诊断详情必须并入正文: {}",
            dialog.text
        );

        // 与脚本 DIALOG 走同一份 JSON 契约,转义照旧。
        let text = dialogs_json(&dialogs);
        let value = json::parse(&text).expect("必须是合法 JSON");
        let list = value
            .get("dialogs")
            .and_then(Value::as_array)
            .expect("dialogs 数组");
        assert_eq!(list.len(), 1);
        assert_eq!(
            list[0].get("source").and_then(Value::as_str),
            Some("host")
        );
        assert_eq!(
            list[0].get("id").and_then(Value::as_u64),
            Some(HOST_DIALOG_ID_BASE)
        );
        assert_eq!(
            list[0].get("text").and_then(Value::as_str),
            Some(dialog.text.as_str()),
            "转义必须可逆"
        );
        assert_eq!(
            list[0].get("caption").and_then(Value::as_str),
            Some("内核 Panic")
        );

        assert!(!queue.dismiss_dialog(7), "VM 的 ID 不能被宿主消息消费");
        assert!(queue.dismiss_dialog(HOST_DIALOG_ID_BASE));
        assert!(queue.dialogs().is_empty());
    }

    #[test]
    fn recoverable_diags_become_non_modal_hints() {
        let mut queue = HostAlertQueue::new();
        let diag = yu_log::Diag::recoverable("gfx.texture.missing", yu_log::Boundary::Domain)
            .with_detail("layer 1");
        assert_eq!(queue.ingest_diag(&diag), 0, "警告不能进模态队列");
        assert!(queue.alerts().is_empty());
        assert_eq!(
            queue.take_hint().as_deref(),
            Some("gfx.texture.missing: layer 1")
        );
        assert_eq!(queue.take_hint(), None, "取走即清空");

        // 同一条警告只提示一次(否则每帧缺资源会刷屏)。
        assert_eq!(queue.ingest_diag(&diag), 0);
        assert_eq!(queue.take_hint(), None);
    }

    #[test]
    fn hint_queue_keeps_latest_and_caps_flood() {
        let mut queue = HostAlertQueue::new();
        for index in 0..(MAX_HINTS + 4) {
            let diag = yu_log::Diag::recoverable("gfx.texture.missing", yu_log::Boundary::Domain)
                .with_detail(format!("layer {index}"));
            assert_eq!(queue.ingest_diag(&diag), 0);
        }
        let latest = queue.take_hint().expect("至少保留最新一条");
        assert_eq!(
            latest,
            format!("gfx.texture.missing: layer {}", MAX_HINTS + 3),
            "队尾覆盖队首,壳只提示最新一条"
        );
        assert_eq!(queue.take_hint(), None);
    }

    #[test]
    fn dialogs_json_escapes_and_handles_empty() {
        assert_eq!(dialogs_json(&[]), r#"{"dialogs":[],"protocol":1}"#);

        let mut request = yes_no_request();
        request.text = cp932("a\"b\\c\n");
        let text = dialogs_json(&[HostDialog::from_request(&request)]);
        assert!(
            text.contains(r#""text":"a\"b\\c\n""#),
            "引号/反斜杠/换行必须转义: {text}"
        );
        let value = json::parse(&text).expect("转义后仍是合法 JSON");
        assert_eq!(
            value
                .get("dialogs")
                .and_then(Value::as_array)
                .and_then(|list| list.first())
                .and_then(|dialog| dialog.get("text"))
                .and_then(Value::as_str),
            Some("a\"b\\c\n"),
            "转义必须可逆"
        );
    }

    #[test]
    fn boot_progress_json_has_stable_contract() {
        report_boot_progress("mount", "挂载游戏数据…");
        let value = json::parse(&boot_progress_json()).expect("合法 JSON");
        assert_eq!(value.get("active").and_then(Value::as_bool), Some(true));
        assert_eq!(
            value.get("stage").and_then(Value::as_str),
            Some("mount")
        );
        assert_eq!(
            value.get("detail").and_then(Value::as_str),
            Some("挂载游戏数据…")
        );

        finish_boot_progress();
        let value = json::parse(&boot_progress_json()).expect("合法 JSON");
        assert_eq!(value.get("active").and_then(Value::as_bool), Some(false));
        assert_eq!(value.get("stage").and_then(Value::as_str), Some("ready"));
    }
}
