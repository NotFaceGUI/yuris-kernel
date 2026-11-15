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

use std::collections::BTreeMap;

use crate::engine::Engine;
use crate::json::{self, Value};
use yu_rt::dialog::DialogRequest;

pub use yu_rt::dialog::{DialogButtons, DialogIcon};

/// 一条待回答 DIALOG 的**呈现视图**(CP932 已解码;内核语义仍是原始字节)。
///
/// 所有字段都是宿主只读数据;回答走 [`HostDialogAnswer`]。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostDialog {
    /// 内核分配的稳定 ID(宿主回答时回传)。
    pub id: u64,
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

    /// 回答一条 DIALOG(统一入口:CP932 编码在引擎侧完成)。
    ///
    /// - `button`:1 基按钮序号;越界由内核拒绝。
    /// - `input_text`:`INPUT=1` 时的编辑文本;编码失败(字符不在 CP932)
    ///   原样返回 `Diag`,宿主应保持对话框打开让用户修改。
    ///
    /// 回答后任务在下一个逻辑帧从 `DIALOG` 的下一条继续(pc 在分派前已
    /// 前进,与官方引擎一致;见 `yu_rt/src/vm.rs::answer_dialog`)。
    pub fn answer_host_dialog(&mut self, answer: &HostDialogAnswer) -> Result<(), yu_log::Diag> {
        let text = match &answer.input_text {
            Some(input) => Some(DialogRequest::encode_input(input)?),
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
}
