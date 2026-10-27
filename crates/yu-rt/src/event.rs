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

use crate::value::{ElemType, Value, VarRef};
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
    ///
    /// `script`/`pc` 为**可选**定位(P4/`COV-M-05`):内核发出的域事件总是
    /// `Some`;外部构造/旧 golden 可给 `None`。宿主可据此把域命令定位回
    /// 脚本(与 `Command` 事件同坐标),不再依赖事件顺序猜位置。
    DomainCall {
        frame: u64,
        domain: Domain,
        name: String,
        args: Vec<ArgValue>,
        script: Option<u32>,
        pc: Option<u32>,
    },
    /// 引擎 `END` 被执行(可带 RESTART)。
    End { frame: u64, restart: bool },
    /// 诊断(挂号/未实现/可恢复错误),与 `yu-log::Diag` 一一对应。
    Diag {
        frame: u64,
        code: String,
        detail: String,
    },
}

/// 参数求值结果(按 `kind` 序号)。
///
/// `reference` = 引用型参数槽(SAVE[SET]/LOAD[LET]/FILEINFO[LET]/
/// SOUNDINFO[LET])保留的**变量身份**(DIV-RT-042,B1 裁 1-A):引擎把
/// `SET=`/`LET=` 当变量地址执行(REA `0x44c45c`/`0x44708c`:引用经
/// `0x683a60/0x683c00/0x683da0[slot]` 三表传递),只带求值结果会让宿主
/// 写不出/恢复不了忠实块。`None` = 非引用槽或旧事件流(向后兼容)。
#[derive(Clone, Debug, PartialEq)]
pub struct ArgValue {
    pub slot: u8,
    pub value: Value,
    pub reference: Option<VarRef>,
    /// 参数窗口是 YSCM 参数种类 2(源写法 `X+=`),即「在当前值上增量」。
    ///
    /// 证据(0.495 窗口编码 + 语料实测):
    /// - 窗口 `+2` 的 u16 高字节 = 官方参数类型字节;种类 0 = 普通值
    ///   (`CG[… X=…]` 的窗口 slot=1),种类 2 = 增量
    ///   (`CG[… X+=…]` 的窗口 slot=257 = 0x0101);
    /// - 消费方 gfx `apply_layer_params` 按本标志做 `当前值 + v`。按赋值
    ///   处理会让 `es_Sprite.yst:8173` 的
    ///   `CG[ID=$L1 X+=(@es.GSX/2)-(@WSX/2)]` 把位置清成 0(立绘钉左上),
    ///   也会让 `ES_Title.yst:104` 的 `CG[ID=FOUT_B A+=4]` 恒为 4(不淡入)。
    pub offset: bool,
}

impl ArgValue {
    pub fn new(slot: u8, value: Value) -> Self {
        Self {
            slot,
            value,
            reference: None,
            offset: false,
        }
    }

    /// 引用型槽:值 + 变量身份一起携带(`var` 应为归一化后的引用)。
    pub fn with_ref(slot: u8, value: Value, var: VarRef) -> Self {
        Self {
            slot,
            value,
            reference: Some(var),
            offset: false,
        }
    }
}

/// 域事件参数的数据驱动视图:槽号 + 工程 ysc 参数名 + 求值结果。
#[derive(Clone, Debug, PartialEq)]
pub struct DomainArg<'a> {
    /// 工程 `ysc.ybn` 中该命令的参数名(查不到时为 `@<slot>`)。
    pub name: std::borrow::Cow<'a, str>,
    /// 参数序号(= 事件里的 `ArgValue::slot`)。
    pub slot: u8,
    /// 求值结果。引用型槽(SAVE[SET]/LOAD[LET] 等)的变量身份在
    /// `ArgValue::reference`(DIV-RT-042,B1 已闭);本视图仍只给值,
    /// 需要身份的宿主走 `SaveHost` 同步契约。
    pub value: &'a Value,
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
    /// 把 `DomainCall` 的参数槽号按工程 `ysc.ybn` 映射成参数名。
    ///
    /// 非 `DomainCall` 返回 `None`。命令名不在工程表时仍返回全部参数,
    /// 名字用 `@<slot>`(不猜)。这是宿主域状态机消费事件的推荐入口:
    /// 槽号不是跨版本稳定的数字,参数名 + 顺序才是。
    ///
    /// 台账:`COV-M-04`;契约与缺口见 `docs/domain-event-contracts.md`。
    pub fn domain_args<'a>(
        &'a self,
        yscm: &'a crate::tables::YscmTable,
    ) -> Option<Vec<DomainArg<'a>>> {
        let VmEvent::DomainCall { name, args, .. } = self else {
            return None;
        };
        let opcode = yscm.index_of(name);
        Some(
            args.iter()
                .map(|a| {
                    let pname = opcode
                        .and_then(|i| yscm.param(i as u8, a.slot))
                        .map(|p| std::borrow::Cow::Borrowed(p.name.as_str()))
                        .unwrap_or_else(|| std::borrow::Cow::Owned(format!("@{}", a.slot)));
                    DomainArg {
                        name: pname,
                        slot: a.slot,
                        value: &a.value,
                    }
                })
                .collect(),
        )
    }

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

    /// VM 解析出资产像素尺寸后回填宿主(默认忽略)。
    ///
    ///
    /// `CGINFO[SX/SY]` 在引擎里读的是**已装载纹理**的尺寸;本内核的 gfx
    /// 不做文件 I/O,因此由持有数据源的 VM 解析图像头并在此回填,
    /// 供后续同键查询与 COPY 快照继承(DIV-GFX-024 / EVID-J3-03)。
    fn note_asset_size(&mut self, _key: &[u8], _width: u32, _height: u32) {}

    /// 每个逻辑帧开始时的光标逻辑坐标(供 `CGINFO[ONMOUSE*]` 命中测试)。
    ///
    /// 引擎的 `ONMOUSE*` 直接读窗口的鼠标位置(`app+0x5b8/0x5bc`),不是一个
    /// 事件;因此这里由 VM 在帧边界同步一次,而不是伪造 DomainCall。
    fn set_pointer(&mut self, _x: i64, _y: i64) {}
}

/// 声音域同步宿主（SOUND / SOUNDEND 消费 + SOUNDINFO 查询）。
///
/// 与 [`GraphicsHost`] 同契约：
/// - 事件在**产生时**同步进入状态机（调用方不得重复消费）；
/// - `query` 单值返回，由 VM 写回 `LET`；
/// - [`SoundHost::advance`] 由帧循环注入时钟，状态机只依赖该时钟推进
///   （实测：引擎 TIME 来自 DirectSound 播放游标；headless 用注入帧时钟
///   的等价推进，见 `docs/sound-ledger.md` N0-2）。
pub trait SoundHost {
    /// 同步应用一条事件（FrameStart/FrameEnd 与 `Domain::Sound` 的 DomainCall）。
    fn apply_event(&mut self, ev: &VmEvent);

    /// 注入帧时钟（毫秒）。重复同一时刻无副作用。
    fn advance(&mut self, _now_ms: u32) {}

    /// 回答一条 SOUNDINFO 查询；返回写入 `LET` 的值。
    fn query(&mut self, command: &str, args: &[ArgValue]) -> Result<Value, Diag>;
}

/// 一个变量的整组快照(SAVE 预约 / LOAD 写回的传输形态;B1)。
///
/// 形态证据(REA `0x44d5dd` 尺寸式 + 官方产物):块内容 = 变量**当前全部
/// 元素**——标量槽 `dims=[]`(save50001 DNO=421),数组槽整组(seld.sd
/// Int[2001]);语料 `SET=@X`(标量)/`SET=@X()`(整组)两形态同型。
#[derive(Clone, Debug, PartialEq)]
pub struct VarSnapshot {
    /// 元素类型(与声明一致;写容器 `ty u32` 1/2/3)。
    pub ty: ElemType,
    /// 各维边界(标量为空)。
    pub dims: Vec<u32>,
    /// 行主序元素,长度 = 维数积(标量 = 1)。
    pub data: Vec<Value>,
}

/// LOAD 的一条恢复项。
///
/// `var = None` ⇒ 写调用方(VM)的 `LET` 目标(块类型 0);
/// `var = Some` ⇒ 按**块内 var_id** 直接写回(块类型 2 的子块;LOAD 处理体
/// `0x447460..0x4474de` 不读 `LET`/`G_*` 在场位,子块无条件按 id 恢复)。
#[derive(Clone, Debug, PartialEq)]
pub struct LoadedVar {
    pub var: Option<VarRef>,
    pub snap: VarSnapshot,
}

/// `LOAD[FILE DNO]` 解出的一块恢复数据。
#[derive(Clone, Debug, PartialEq, Default)]
pub struct LoadedBlock {
    pub vars: Vec<LoadedVar>,
}

/// 存档域同步宿主(B1 裁 1-A / 裁 2-A;与 [`GraphicsHost::query`] 同模式)。
///
/// 为什么同步:`SAVE` 的预约在**命令时刻序列化当前值**(REA `0x44d73d`:
/// 写 btype/strict/var/clen/ulen + `0x44dc1d` 取值 + `0x48c4a4` 压缩,
/// `FILE` 只写文件头与偏移表),`LOAD` 的写回在命令时刻落地——异步事件流
/// 两者都拿不到正确时序。未安装宿主时,SAVE/LOAD/SAVEEND 保持纯域事件
/// (M-2 既有行为,600 帧普查逐位不变)。
///
/// `FILEACT`/`FILEINFO`/`SAVEINFO` 也走 [`SaveHost::apply_event`](Self::apply_event),
/// 由 `Vm::emit` 在**命令时刻**同步调用:官方 `FILEACT` 处理体就地删除/复制;
/// 若推迟到帧末 drain,存档脚本「先删旧槽、再 `SAVE[FILE]` 刷盘」会让删除
/// 落在写盘之后,刚写出的存档被删。
pub trait SaveHost {
    /// 同步应用一条事件(仅 `Domain::Save | Domain::File` 的 DomainCall;
    /// `Vm::emit` 在命令时刻调用,调用方不得再把 drain 出的同一事件喂入;
    /// SAVE/LOAD/SAVEEND 的语义走下面的专用方法,宿主不得重复消费)。
    fn apply_event(&mut self, ev: &VmEvent);

    /// `SAVE[DNO=n SET=@x]` 预约一块(块类型 0;**当前值在本次调用内快照**)。
    fn reserve(&mut self, dno: u32, var: VarRef, snap: &VarSnapshot) -> Result<(), Diag>;

    /// `SAVE[DNO=n G_<族><组>=1]` 预约一块(块类型 2):`vars` =
    /// (type, group) 匹配的全局变量逐个 `(var_id, 快照)`,顺序 = 工程变量
    /// 表序(REA `0x44ca8a..0x44cc57` 枚举 `[0x801724]` 记录)。
    fn reserve_group(
        &mut self,
        dno: u32,
        ty: ElemType,
        group: u8,
        vars: &[(u16, VarSnapshot)],
    ) -> Result<(), Diag>;

    /// `SAVE[FILE="x"]` 刷盘(写文件头 + 偏移表 + 全部预约块)。
    fn flush(&mut self, file: &str) -> Result<(), Diag>;

    /// `SAVEEND[]` 丢弃全部未刷盘预约。
    fn save_end(&mut self);

    /// `LOAD[FILE DNO=n]` 读一块恢复数据(容器解析归宿主;写回归 VM)。
    fn load_block(&mut self, file: &str, dno: u32) -> Result<LoadedBlock, Diag>;
}
