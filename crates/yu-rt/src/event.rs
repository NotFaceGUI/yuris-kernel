//! 事件流(`VmEvent`)。golden 对拍的**核心可比对物**。
//!
//! # 设计
//!
//! - 事件是**确定性数据**:不含墙钟、不含指针、不含 `HashMap` 迭代序;
//!   序列化用 [`crate::json`] 的稳定输出(字段顺序固定)。
//! - 字段只记**可观测值**:命令名(来自游戏 ysc 表)、参数求值结果、
//!   跳转目标、变量的新值。不记内部猜测。
//! - 与 `docs/oracle.md` 的 trace 草案同构:`frame` + `sub` + `kind` + `payload`。
//!
//! # 真值边界
//!
//! 官方 trace 采集(0.479 引擎)尚未在本会话产出;本事件流是**内核侧**的
//! 规范形式,字段集合按「官方可观测行为能表达」设计。凡是官方观测不到的
//! 内部量一律不出现在事件里。

use crate::value::Value;
use yu_log::Diag;

/// 事件流中的一条记录。
#[derive(Clone, Debug, PartialEq)]
pub enum VmEvent {
    /// 逻辑帧开始(帧号从 0 起;一个 vsync 链可含多逻辑帧)。
    FrameStart { frame: u64 },
    /// 逻辑帧结束(本帧所有任务已让出 ⇒ 调用方可渲染一次)。
    FrameEnd { frame: u64 },
    /// 一组命令被执行(含求值后的参数)。
    Command {
        frame: u64,
        script: u32,
        pc: u32,
        /// 命令名(来自工程 `ysc.ybn`;查不到表时为 `None` 并已报 Diag)。
        name: String,
        /// 按参数序号(窗口 `kind`)给出求值结果。
        args: Vec<ArgValue>,
    },
    /// 控制流跳转(GO/GOSUB/RETURN/IF 假分支/循环回边)。
    Jump {
        frame: u64,
        script: u32,
        pc: u32,
        to_pc: u32,
        to_script: u32,
        kind: JumpKind,
    },
    /// 任务被创建/重配置。
    TaskSpawn {
        frame: u64,
        id: String,
        script: u32,
        pc: u32,
        enabled: bool,
        /// 是否主线(= 启动链建的系统任务)。
        main: bool,
    },
    /// 任务结束(TASKEND / RETURN 到基帧 / END)。
    TaskFinish { frame: u64, id: String },
    /// 暂停(WAIT):本任务让出,直到帧计数或时钟到期。
    Wait {
        frame: u64,
        task: String,
        frames: Option<u32>,
        time_ms: Option<u32>,
    },
    /// 文本输出到文本层(`TEXT`/`_` 的可见效果)。
    Text {
        frame: u64,
        /// 文本层 ID(脚本给的 `ID` 参数;空 = 默认层)。
        id: String,
        /// 本次追加的文本(CP932 字节原样)。
        bytes: Vec<u8>,
    },
    /// 一行文本结束(`RETURNCODE`;`line` = 编译器写入的本脚本内改行计数)。
    ReturnCode { frame: u64, line: u32 },
    /// FPS 由脚本设定。
    Fps { frame: u64, fps: u32, skip: bool },
    /// 引擎设置被脚本覆盖(SYSTEM 等);只记事件,不在此执行。
    SystemCall {
        frame: u64,
        name: String,
        args: Vec<ArgValue>,
    },
    /// 显示/图形/声音等域命令:交宿主域状态机,事件流只记事实。
    DomainCall {
        frame: u64,
        domain: Domain,
        name: String,
        args: Vec<ArgValue>,
    },
    /// 引擎 `END` 被执行(可带 RESTART)。
    End { frame: u64, restart: bool },
    /// 诊断(挂号/未实现/可恢复错误),与 `yu-log::Diag` 一一对应。
    Diag { frame: u64, code: String, detail: String },
}

/// 参数求值结果(按 `kind` 序号)。
#[derive(Clone, Debug, PartialEq)]
pub struct ArgValue {
    pub slot: u8,
    pub value: Value,
}

impl ArgValue {
    pub fn new(slot: u8, value: Value) -> Self {
        Self { slot, value }
    }
}

/// 跳转类型(与旧线 `JumpKind` 同构,便于人工比对)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JumpKind {
    Go,
    Gosub,
    Return,
    IfFalse,
    Else,
    LoopBack,
    LoopBreak,
    LoopContinue,
    IfBreak,
    IfContinue,
    TaskStart,
}

impl JumpKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            JumpKind::Go => "go",
            JumpKind::Gosub => "gosub",
            JumpKind::Return => "return",
            JumpKind::IfFalse => "if_false",
            JumpKind::Else => "else",
            JumpKind::LoopBack => "loop_back",
            JumpKind::LoopBreak => "loop_break",
            JumpKind::LoopContinue => "loop_continue",
            JumpKind::IfBreak => "if_break",
            JumpKind::IfContinue => "if_continue",
            JumpKind::TaskStart => "task_start",
        }
    }
}

/// 域分类(命令语义不属于运行核时的事件归属)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Domain {
    Graphics,
    Sound,
    Input,
    File,
    Save,
    Menu,
    Window,
    Movie,
    Flash,
    Emote,
    System,
    Other,
}

impl Domain {
    pub const fn as_str(self) -> &'static str {
        match self {
            Domain::Graphics => "graphics",
            Domain::Sound => "sound",
            Domain::Input => "input",
            Domain::File => "file",
            Domain::Save => "save",
            Domain::Menu => "menu",
            Domain::Window => "window",
            Domain::Movie => "movie",
            Domain::Flash => "flash",
            Domain::Emote => "emote",
            Domain::System => "system",
            Domain::Other => "other",
        }
    }
}

impl VmEvent {
    /// `sub` 字段:与 `docs/oracle.md` 的子系统命名一致。
    pub fn sub(&self) -> &'static str {
        match self {
            VmEvent::FrameStart { .. } | VmEvent::FrameEnd { .. } => "frame",
            VmEvent::Command { .. } | VmEvent::Jump { .. } => "vm",
            VmEvent::TaskSpawn { .. } | VmEvent::TaskFinish { .. } | VmEvent::Wait { .. } => "task",
            VmEvent::Text { .. } | VmEvent::ReturnCode { .. } => "text",
            VmEvent::Fps { .. } => "system",
            VmEvent::SystemCall { .. } => "system",
            VmEvent::DomainCall { domain, .. } => domain.as_str(),
            VmEvent::End { .. } => "system",
            VmEvent::Diag { .. } => "diag",
        }
    }

    /// 事件种类(kind 字段;稳定)。
    pub fn kind(&self) -> &'static str {
        match self {
            VmEvent::FrameStart { .. } => "frame_start",
            VmEvent::FrameEnd { .. } => "frame_end",
            VmEvent::Command { .. } => "command",
            VmEvent::Jump { .. } => "jump",
            VmEvent::TaskSpawn { .. } => "task_spawn",
            VmEvent::TaskFinish { .. } => "task_finish",
            VmEvent::Wait { .. } => "wait",
            VmEvent::Text { .. } => "text",
            VmEvent::ReturnCode { .. } => "return_code",
            VmEvent::Fps { .. } => "fps",
            VmEvent::SystemCall { .. } => "system_call",
            VmEvent::DomainCall { .. } => "domain_call",
            VmEvent::End { .. } => "end",
            VmEvent::Diag { .. } => "diag",
        }
    }

    /// 事件所属帧号。
    pub fn frame(&self) -> u64 {
        match self {
            VmEvent::FrameStart { frame }
            | VmEvent::FrameEnd { frame }
            | VmEvent::Command { frame, .. }
            | VmEvent::Jump { frame, .. }
            | VmEvent::TaskSpawn { frame, .. }
            | VmEvent::TaskFinish { frame, .. }
            | VmEvent::Wait { frame, .. }
            | VmEvent::Text { frame, .. }
            | VmEvent::ReturnCode { frame, .. }
            | VmEvent::Fps { frame, .. }
            | VmEvent::SystemCall { frame, .. }
            | VmEvent::DomainCall { frame, .. }
            | VmEvent::End { frame, .. }
            | VmEvent::Diag { frame, .. } => *frame,
        }
    }
}

/// 图形域同步宿主。
///
/// 为什么需要它:`CGINFO/FONTINFO/WINDOWINFO` 的 `LET` 必须在**脚本执行期**
/// 读取图形状态。`VmEvent::DomainCall` 只是异步事件流,宿主 tick 后才消费,
/// 同一帧里 `CG[...]` 后紧跟的 `CGINFO` 会读到旧状态。安装本接口后:
///
/// 1. `Vm` 在**产生**图形/窗口/文本/帧事件时同步调用 [`apply_event`];
/// 2. INFO 命令通过 [`query`] 同步取当前状态并写回 `LET`;
/// 3. 调用方**不得**再把已同步喂过的事件重复喂给同一个宿主。
///
/// 未安装宿主时,INFO 命令保持 `Unimplemented` 诊断(不猜)。
pub trait GraphicsHost {
    /// 同步应用一条事件(仅 FrameStart/FrameEnd/Text/Graphics|Window DomainCall)。
    fn apply_event(&mut self, ev: &VmEvent);

    /// 回答一条 INFO 查询;返回写入 `LET` 的值。
    ///
    /// 单值返回:查询方必须只请求一个字段;多字段顺序未取证时返回错误,
    /// 由 VM 报 `Unimplemented` 并**不写** `LET`。
    fn query(&mut self, command: &str, args: &[ArgValue]) -> Result<Value, Diag>;
}
