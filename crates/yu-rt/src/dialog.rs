//! `DIALOG` 的 VM 侧状态机(契约见 `docs/domain-event-contracts.md` §7)。
//!
//! # 三层证据
//!
//! ①**手册** `other/dialog.html`:STR / CAPTION / DEFSTR / LETSTR / LET /
//!   OK / OKCANCEL / YESNO / YESNOCANCEL / INPUT / DEFBUTTON 的语义;
//!   `LET` = 「左から何番目のボタン」。
//! ②**真实脚本**(ERIS 0.495,17 处):只用
//!   `STR/CAPTION/YESNO/LET/DEFBUTTON/INPUT/DEFSTR/LETSTR/OKCANCEL/YESNOCANCEL`。
//! ③**REA**(0.495 `yu-ris.exe` sha256 `4f5eefa3…`,处理体 `0x43fc74`):
//! - 参数槽位:`0x684160 + slot` 为「本命令第 slot 个参数是否存在」的 1 基索引
//!   (0 = 缺席);存在性判定**不看参数值**(`OKCANCEL=0` 也算存在);
//! - 非 INPUT:直接 `MessageBoxA(hwnd, STR, CAPTION, buttons|icon|defbutton)`
//!   (`0x43fe67`),返回值 `IDOK=1/IDCANCEL=2/IDYES=6/IDNO=7`;
//! - LET 写回(`0x440056/0x4400ea/0x44017e/0x440212/0x4402a6`):
//!   OK→1、Cancel→2、Yes→1、No→2、YesNoCancel 的 Cancel→3;
//! - 按钮组按存在性选择:`OKCANCEL`→MB_OKCANCEL、
//!   `YESNO`→MB_YESNO、`YESNOCANCEL`→MB_YESNOCANCEL,都没有→MB_OK;
//!   `OK` 槽在官方处理体里**从未被读取**(MB_OK 是缺省);
//! - DEFBUTTON:1→MB_DEFBUTTON1 … 4→MB_DEFBUTTON4,其它→0;
//! - ICON:1→MB_ICONINFORMATION(0x40)、2→QUESTION(0x20)、
//!   3→WARNING(0x30)、4→ERROR(0x10);
//! - INPUT:自绘对话框(`CreateDialogIndirectParamA`,模板 `0x8083c4`,
//!   过程 `0x4403d0`);`INPUTLIMIT`(槽 17)>0 时 EM_SETLIMITTEXT;
//!   OK 时取控件 1000 的文本写入 LETSTR,`EndDialog(1)` ⇒ LET=1;
//! - 模态期间 `0x440370/0x440398` 暂停/恢复宿主计时器 ⇒ 引擎是**同步阻塞**;
//!   内核改为「任务挂起 + 宿主回答」,见 [`DialogRequest`]。
//!
//! 未取证参数(`STR2..8`、`A/A2..8`、`E/E2..8`、`FILEOPEN`、`FOLDER`、
//! `CHECKBOX`、`SLIDER`、`CHECK..`、`NOWAIT`)一律不猜:命中即 `Unimplemented`,
//! 不弹窗、不写回(DIV-RT-044)。

use crate::expr::LValue;

/// 按钮组合(官方 MessageBox 家族;顺序即 LET 的 1 基下标)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogButtons {
    Ok,
    OkCancel,
    YesNo,
    YesNoCancel,
}

impl DialogButtons {
    /// 可从左到右按下的按钮数(= LET 的最大合法值)。
    pub const fn count(self) -> u8 {
        match self {
            DialogButtons::Ok => 1,
            DialogButtons::OkCancel | DialogButtons::YesNo => 2,
            DialogButtons::YesNoCancel => 3,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            DialogButtons::Ok => "ok",
            DialogButtons::OkCancel => "ok_cancel",
            DialogButtons::YesNo => "yes_no",
            DialogButtons::YesNoCancel => "yes_no_cancel",
        }
    }

    /// 第 `index`(1 基)个按钮的标签名。
    pub const fn label(self, index: u8) -> &'static str {
        match (self, index) {
            (DialogButtons::Ok, 1) => "ok",
            (DialogButtons::OkCancel, 1) => "ok",
            (DialogButtons::OkCancel, 2) => "cancel",
            (DialogButtons::YesNo, 1) => "yes",
            (DialogButtons::YesNo, 2) => "no",
            (DialogButtons::YesNoCancel, 1) => "yes",
            (DialogButtons::YesNoCancel, 2) => "no",
            (DialogButtons::YesNoCancel, 3) => "cancel",
            _ => "",
        }
    }
}

/// 图标(官方 ICON 槽 1..4;缺省/越界 = 无图标)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogIcon {
    None,
    Information,
    Question,
    Warning,
    Error,
}

impl DialogIcon {
    pub(crate) fn from_slot(v: i64) -> Self {
        match v {
            1 => DialogIcon::Information,
            2 => DialogIcon::Question,
            3 => DialogIcon::Warning,
            4 => DialogIcon::Error,
            _ => DialogIcon::None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            DialogIcon::None => "none",
            DialogIcon::Information => "information",
            DialogIcon::Question => "question",
            DialogIcon::Warning => "warning",
            DialogIcon::Error => "error",
        }
    }
}

/// 一次待回答的 DIALOG。
///
/// **内核只持有事实**:字符串按 CP932 原始字节保存(与 `WORD`/`Text` 事件
/// 一致);宿主负责解码与呈现。`LET`/`LETSTR` 的目标以左值保存到内核侧,
/// 不进入事件流(与 `SAVE/LOAD` 的引用身份缺口不同:DIALOG 的写回由内核
/// 自己完成,宿主只需要回答)。
#[derive(Clone, Debug, PartialEq)]
pub struct DialogRequest {
    /// 内核分配的稳定 ID(单调递增;宿主回答时回传)。
    pub id: u64,
    /// 发起任务 ID(`TASKEND` 等任务语义仍以任务为单位)。
    pub task: String,
    /// `CAPTION`(缺省 = 空)。
    pub caption: Vec<u8>,
    /// `STR`(缺省 = 空)。
    pub text: Vec<u8>,
    /// 按钮组合。
    pub buttons: DialogButtons,
    /// `DEFBUTTON` 原值(0 = 未指定;1..4 = 左起第几个)。
    pub default_button: u8,
    /// `ICON` 映射结果。
    pub icon: DialogIcon,
    /// `INPUT=…` 是否存在(存在 = 文本输入框;按钮固定为 OK)。
    pub input: bool,
    /// `INPUTLIMIT`(0 = 不限)。
    pub input_limit: u32,
    /// `DEFSTR`(输入框初始文本)。
    pub default_input: Vec<u8>,
    /// `LET` 目标(按钮序号 1..=count 写回;None = 脚本没给)。
    pub let_target: Option<LValue>,
    /// `LETSTR` 目标(仅 INPUT 模式写回输入文本;None = 脚本没给)。
    pub letstr_target: Option<LValue>,
}

impl DialogRequest {
    /// 宿主回答前的合法性检查:按钮序号必须在按钮组合范围内。
    pub fn accepts_button(&self, button: u8) -> bool {
        (1..=self.buttons.count()).contains(&button)
    }

    /// 文本输入模式下宿主必须给文本(空串合法)。
    pub fn requires_text(&self) -> bool {
        self.input
    }
}

/// `DIALOG` 状态机的内核侧错误(宿主回答越界/未知 ID)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DialogError {
    UnknownId(u64),
    InvalidButton { id: u64, button: u8, count: u8 },
    MissingText { id: u64 },
}

impl std::fmt::Display for DialogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DialogError::UnknownId(id) => write!(f, "DIALOG#{id} 不存在或已回答"),
            DialogError::InvalidButton { id, button, count } => {
                write!(f, "DIALOG#{id} 按钮序号 {button} 越界(合法 1..={count})")
            }
            DialogError::MissingText { id } => {
                write!(f, "DIALOG#{id} 是文本输入框,回答必须带文本")
            }
        }
    }
}

impl std::error::Error for DialogError {}
