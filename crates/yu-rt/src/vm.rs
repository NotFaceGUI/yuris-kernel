//! 运行核:启动链、按名分派、执行位管理。
//!
//! # 启动链(引擎顺序)
//!
//! 旁线 `format-vm-opcodes.md` §3.1(`FUN_00465214`,0.495 `yu-ris.exe`
//! sha256 `4f5eefa3…`):`yse → ysc(+建 opcode 表) → yst → yst_list → ysv →
//! (校验) → ysl → 查入口标签 → 建任务 → 装载脚本`。
//!
//! 本实现按同一顺序,但**所有表来自工程数据**([`TableSet::load`] 缺表拒启),
//! 入口标签名由 [`BootConfig::entry_label`] 给出(默认 `SYSTEM_START`,
//! 见 `data/script/system_start.yst` 与手册 `other/label.html`)。
//!
//! # 分派
//!
//! 引擎 `DAT_007ace20[opcode]`;opcode = 工程 ysc 表下标。本内核:
//! `opcode → 命令名(工程 ysc)→ 处理器`。表里没有该 opcode ⇒ `rt.cmd.unknown`;
//! 有名字但未注册处理器 ⇒ `rt.cmd.unimplemented`(挂号,不静默)。
//!
//! # 执行位
//!
//! 引擎任务记录自带脚本号/pc/块栈/循环栈(§3.2),链序由单一扁平任务链决定
//! (旁线 §3.2b)。本实现把每个任务的执行位存进 `exec: BTreeMap<TaskId,
//! ExecState>`,单步时取出、执行、放回——这是**工程形态**(参考
//! `<repo-old>/crates/yuris-vm/src/driver.rs` 的 `swap_exec`),不影响可观测语义。

use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::Instant;

use yu_log::{Boundary, Diag, LogSink, MemorySink};

use crate::code;
use crate::diag;
use crate::event::VmEvent;
use crate::expr::{Evaluator, LValue, VarAccess};
use crate::input::{InputBinding, InputState, InputTimeline};
use crate::tables::{TableSet, VarRecord, WindowRec};
use crate::task::{
    ChainReport, Effect, Scheduler, StepOutcome, TaskExecutor, TaskNode, TaskSeed, TaskView,
    MAIN_TASK_ID,
};
use crate::value::{ElemType, Value, VarRef, VarSpace, VariableStore};

pub(crate) mod handlers;
pub use handlers::HandlerKind;

/// 事件队列默认容量(任务书 E R10)。按每帧数千事件的量级留足余量,
/// 同时保证长跑有界;需要完整全量事件流的调用方用 [`Vm::take_events`]/
/// [`Vm::drain_events`] 在帧间取走。
pub const DEFAULT_EVENT_CAPACITY: usize = 1 << 20;

/// `VARACT[INIT]` 掩码组合数:3 个类型族 × 4 位(`G_*1..4`)= 2^12。
/// 键 = `masks[0] | masks[1]<<4 | masks[2]<<8`。
const GLOBAL_RESTORE_KEYS: usize = 1 << 12;

/// 表达式窗口解码缓存上限(工程护栏,不是引擎常量):脚本窗口种类有限,
/// 超过即整体清空重建,避免长跑无界增长。
const DECODE_CACHE_CAP: usize = 200_000;
/// 命令缓存按脚本号/pc 直接下标;超出这些上限的畸形输入不走缓存,
/// 交给 `fill_cmd` 的边界检查报错(避免巨型 `Vec` 分配)。
const CMD_CACHE_SCRIPT_MAX: usize = 1 << 16;
const CMD_CACHE_PC_MAX: usize = 1 << 20;

/// 宿主注入的**本地**日期时间(`@_TIME` 的手册语义 = 系统本地时间)。
///
/// 内核不直接读墙钟(A3):宿主启动时读一次系统本地时间,经
/// [`BootConfig::local_time`] 注入。`@_TIME(1,k)` 返回注入值本身,
/// `@_TIME(0,k)` 在其上叠加游戏逻辑流逝时间(`now_ms - start_ms`)。
///
/// `month` 1..=12、`day` 1..=31、`hour` 0..=23、`minute`/`second` 0..=59、
/// `millisecond` 0..=999;越界值由 [`HostLocalTime::new`] 拒绝。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostLocalTime {
    pub year: i64,
    pub month: i64,
    pub day: i64,
    pub hour: i64,
    pub minute: i64,
    pub second: i64,
    pub millisecond: i64,
}

impl HostLocalTime {
    /// 校验并构造。字段越界 ⇒ `None`(宿主必须给合法本地时间,不猜)。
    #[must_use]
    pub fn new(
        year: i64,
        month: i64,
        day: i64,
        hour: i64,
        minute: i64,
        second: i64,
        millisecond: i64,
    ) -> Option<Self> {
        if !(1..=12).contains(&month)
            || !(1..=31).contains(&day)
            || !(0..=23).contains(&hour)
            || !(0..=59).contains(&minute)
            || !(0..=59).contains(&second)
            || !(0..=999).contains(&millisecond)
        {
            return None;
        }
        Some(Self {
            year,
            month,
            day,
            hour,
            minute,
            second,
            millisecond,
        })
    }
}

/// 启动配置。
#[derive(Clone, Debug)]
pub struct BootConfig {
    /// 入口标签名(默认 `SYSTEM_START`)。
    pub entry_label: String,
    /// 帧率初值(脚本 `FPS[SET=…]` 可覆盖;手册 `system/fps.html`)。
    pub fps: u32,
    /// 注入时钟起点(毫秒)。
    pub start_ms: u32,
    /// 宿主本地时间基准(`@_TIME` 的年月日时分秒;`None` = 确定性注入时钟)。
    ///
    /// 真实运行宿主(`yu-cli` run/serve/desktop)在启动时读一次系统本地时间
    /// 填入;用例/trace/golden 保持 `None` ⇒ `@_TIME` 继续以 1970-01-01 UTC
    /// 为纪元的注入时钟计算(可复现,DIV-RT-009)。
    pub local_time: Option<HostLocalTime>,
    /// 每逻辑帧毫秒(`WAIT[TIME]` 与 `@_TIME` 同源)。
    pub frame_ms: u32,
    /// 脚本 XOR 密钥(默认 `CRC32("Yu-ris")`)。
    pub script_key: [u8; 4],
    /// `@_DEBUGMODE` 真值(来自工程 `projectconfig.txt`)。
    pub debug_mode: bool,
    /// `@_ARGV` 命令行参数。
    pub argv: Vec<Vec<u8>>,
    /// `@_SCREEN_COLOR` = 当前色深(bpp)。**宿主显示层注入**。
    ///
    /// 手册 `basic/svariable.html`:`@_SCREEN_COLOR | 現在の色数`;
    /// 引擎实现是显示初始化写入的全局 bpp。内核无显示栈 ⇒ 由宿主给值。
    pub screen_color: u32,
    /// 路径类系统变量(宿主注入;None = 未注入 ⇒ `Unimplemented`)。
    pub paths: HostPaths,
    /// `@_MEDIA_READONLY`(手册:`読み込み専用メディア…起動した場合に１`)。
    ///
    /// 宿主判定:数据盘/存档盘是否可写。内核自己从不写游戏数据(A2),
    /// 所以这**不是**内核属性,必须由宿主注入。
    pub media_readonly: bool,
    /// `@_OS_SOUND`(手册:サウンド機能が利用可能=1/不可=0)。宿主音频层注入。
    pub os_sound: bool,
    /// `@_DXMODE`(工程 `YSCom.ycd` 声明,手册无页)。
    ///
    /// 语料用法(ERIS `es_first.yst` s10 pc81):`IF[@_DXMODE] { … CG[ID=DXBUF …] }`
    /// —— 非零时走 DirectX 缓冲图层分支。这是**宿主显示能力**,由平台层注入
    /// (wasm 后端应给 0;win 后端按实际 DX 模式给值)。见 DIV-RT-007。
    pub dx_mode: u32,
    /// `@_WINDOW_INIT_POS`(工程 `yscfg.dat` 第 10 个 int;REA `0x460b74` 写
    /// `[0x80133c]`,int getter `0x4276f3` 读)。
    ///
    /// `yscfg.dat` 是「エンジン設定.exe」的外部设置文件,不属于 `data.ypf`;
    /// 内核不读它 ⇒ 由宿主注入(缺省 0,与 ERIS 工程一致)。DIV-RT-047。
    pub window_init_pos: i64,
    /// `@_SAVE_PATH_TYPE`(手册无页;引擎读 `[0x801324]`)。
    ///
    /// 来源 = 游戏根 `yssfs.dat` 偏移 0x54 的 u32(只接受 0..=3,其余 0),
    /// 由宿主读文件后注入:0.495 `FUN_00460c80` 打开 `<app>\yssfs.dat` 用
    /// CRT `fread` 读 0x20 个 u32,取 `puVar2[0x15]` 写全局。int getter
    /// `FUN_00427350` case 112 = `MOVSX EBP,byte [0x801324]`。
    /// 缺省 0 = 文件缺失/全零(两种在官方都等价于 0)。
    pub save_path_type: i64,
    /// `MATH[RND]` 的 MT19937 种子(引擎用 `GetTickCount`,非确定)。
    /// 缺省 5489 = MT19937 参考种子,保证同输入同输出(DIV-RT-014)。
    pub rng_seed: u32,
    /// 事件流记录开关(默认开;golden/测试/对拍依赖完整事件流)。嵌入宿主
    /// 跑真实游戏时可关:`Command` 事件的窗口预求值是分派热路径的最大开销
    /// (每个窗口求值两遍)。关闭时 `Diag` 事件与挂号计数照常产出。
    ///
    /// 关闭时还跳过 `Jump`(IF/ELSE/LOOP/LOOPEND 控制流)与 Graphics/Window
    /// `DomainCall` 的入队:前者是场景构建帧的数十万条纯诊断事件,后者已由
    /// 同步图形宿主在命令时刻消费;两者都不影响脚本/画面语义。
    pub record_events: bool,
}

/// 宿主注入的路径系统变量(`basic/svariable.html`)。
#[derive(Clone, Debug, Default)]
pub struct HostPaths {
    pub system_path: Option<Vec<u8>>,
    pub save_path: Option<Vec<u8>>,
    pub appdata_path: Option<Vec<u8>>,
    pub personal_path: Option<Vec<u8>>,
    pub project_path: Option<Vec<u8>>,
}

impl Default for BootConfig {
    fn default() -> Self {
        Self {
            entry_label: "SYSTEM_START".to_string(),
            fps: 60,
            start_ms: 0,
            local_time: None,
            frame_ms: 16,
            script_key: crate::tables::DEFAULT_SCRIPT_KEY,
            debug_mode: false,
            argv: Vec::new(),
            screen_color: 32,
            paths: HostPaths::default(),
            media_readonly: false,
            os_sound: true,
            dx_mode: 0,
            window_init_pos: 0,
            save_path_type: 0,
            rng_seed: 5489,
            // 事件流记录开关(默认开;结构体字段文档有完整说明)。
            record_events: true,
        }
    }
}

/// 运行期错误。
#[derive(Clone, Debug)]
pub enum VmError {
    Fatal(Diag),
    Diag(Diag),
}

impl From<Diag> for VmError {
    fn from(d: Diag) -> Self {
        if matches!(d.severity, yu_log::Severity::Fatal) {
            VmError::Fatal(d)
        } else {
            VmError::Diag(d)
        }
    }
}

impl std::fmt::Display for VmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VmError::Fatal(d) | VmError::Diag(d) => write!(f, "{d}"),
        }
    }
}

/// `GOSUB` 一层(引擎命令对象 §3.12 的可执行投影)。
#[derive(Clone, Debug, Default)]
pub struct GosubFrame {
    pub return_script: u32,
    pub return_pc: u32,
    /// 进入时变量域基址 A(引擎 `task+0x3e`)。
    pub base_a: u8,
    /// `PINT/PFLT/PSTR`(编号 1..16;`None` = 未传)。
    pub args: [Option<Value>; 16],
    pub arg_flags: [bool; 16],
    /// `RETURN` 填回的 `RINT/RFLT/RSTR`。
    pub ret: [Option<Value>; 16],
    pub ret_flags: [bool; 16],
    /// `GOSUB` 进入时的块/循环栈深度;`RETURN` 截断回该深度。
    ///
    /// 证据:引擎 RETURN 处理器 `0x44b418` 用帧里记录的深度 truncate
    /// IF/循环栈(旧线逆向成果 51.5);块结构按调用层独立,被调脚本里
    /// 未闭合的 `IF`/`LOOP` 不得留在栈上被调用方的 `IFEND`/`LOOPEND`
    /// 弹出。ERIS 启动链实测:被调脚本遗留的 `LOOP` 帧让调用方
    /// `LOOPEND` 跳到被调脚本的 `body_pc`(953),而调用方脚本只有 554 组
    /// ⇒ 主任务 Fatal。
    pub if_depth: usize,
    pub loop_depth: usize,
}

impl GosubFrame {
    pub fn new(return_script: u32, return_pc: u32, base_a: u8) -> Self {
        Self {
            return_script,
            return_pc,
            base_a,
            ..Default::default()
        }
    }
}

/// `IF` 块帧(引擎块帧 12 字节;§3.13)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IfFrame {
    pub entry_pc: u32,
    pub end_pc: u32,
    pub base_a: u8,
    /// 进入 IF 时的 LOOP 深度(引擎 IF 帧 `+0`;SCRIPTPOS 序列化字段)。
    pub loop_depth: usize,
}

/// `LOOP` 帧(引擎循环栈帧 0x20 字节;§3.13)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoopFrame {
    pub body_pc: u32,
    pub end_pc: u32,
    /// 目标轮数(`-1` = 无限;手册:省略 SET = 无限)。
    pub limit: i64,
    /// 已执行轮数(从 1 起算;`_LC` 读它)。
    pub counter: i64,
    pub if_depth: usize,
    pub base_a: u8,
}

/// 一个任务的执行位。
#[derive(Clone, Debug)]
pub struct ExecState {
    pub script_id: u32,
    pub pc: u32,
    /// 变量域基址 A(引擎 `task+0x3e`;同时是块深度)。
    pub base_a: u8,
    /// 最近一次 `RETURNCODE` 的文本行信息(引擎 `task+0xc`)。
    ///
    /// 引擎 `0x44c390` 把当前指令首窗口记录的第一个 dword 原样抄进
    /// `task+0xc`(RETURNCODE 的窗口 `id` = 本脚本文本行计数,`type` = 0),
    /// `TASKINFO[TEXTLINE]`(`0x45272a`)直接读回。这里保存同一个打包值,
    /// 不另行解释 `flag/slot` 位。
    pub text_line: i64,
    /// 最近一次 `RETURNCODE` 的 voice 值(引擎 `task+0x10`)。
    ///
    /// 同一条窗口记录的第二个 dword(记录里的 `length` 字段);
    /// `TASKINFO[NEXTVOICE]`(`0x452708`)直接读回。
    pub next_voice: i64,
    pub frames: Vec<GosubFrame>,
    pub if_stack: Vec<IfFrame>,
    pub loops: Vec<LoopFrame>,
    pub task_ints: [i64; 16],
    pub task_floats: [f64; 16],
    pub task_strs: [Vec<u8>; 16],
}

impl ExecState {
    pub fn new(script_id: u32, pc: u32) -> Self {
        Self {
            script_id,
            pc,
            base_a: 0,
            text_line: 0,
            next_voice: 0,
            frames: vec![GosubFrame::new(script_id, pc, 0)],
            if_stack: Vec::new(),
            loops: Vec::new(),
            task_ints: [0; 16],
            task_floats: [0.0; 16],
            task_strs: std::array::from_fn(|_| Vec::new()),
        }
    }
}

/// 一条命令的视图(已从脚本里取出,避免执行期借用脚本)。
#[derive(Default)]
pub struct Cmd {
    pub opcode: u8,
    pub name: String,
    pub pc: u32,
    pub windows: Vec<CmdWindow>,
    /// 该 opcode 的参数名表(工程 `ysc.ybn` 的静态事实)。
    ///
    /// 按 opcode 共享一份 `Rc`,执行期只是引用计数(旧实现 `param_names`
    /// 每次调用克隆整张表:`VARACT` 在启动帧被调用 2.8 万次、每次约 14 个
    /// `String` 分配)。空 = 工程表里没有该命令(与旧 `unwrap_or_default` 同口径)。
    pub param_names: Rc<[String]>,
}

/// 一个窗口 + 其载荷。
pub struct CmdWindow {
    pub rec: WindowRec,
    pub payload: Vec<u8>,
    /// 填充命令时预解码的表达式(脚本字节不变;`None` = 非表达式/解码失败,
    /// 运行时回退到 `decode_cache` 并保留原诊断)。
    pub instrs: Option<Rc<[crate::expr::RawInstr]>>,
    /// 单条字面量窗口的常量值(常量折叠;`None` = 需要正常求值)。
    pub const_value: Option<Value>,
}

impl CmdWindow {
    pub fn is_empty(&self) -> bool {
        self.payload.is_empty()
    }
}

/// 运行核。
/// 标签跳转目标(只含跳转需要的标量;`Label` 自带两个堆字符串,
/// 按调用点缓存时复制它会付两份分配,故投影成 `Copy` 形态)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LabelTarget {
    pub script_id: u32,
    pub pc: u32,
    pub base_a: u8,
}

pub struct Vm {
    src: Box<dyn yu_vfs::DataSource>,
    /// 编译器表回退源(工程根/SDK);脚本与其余表仍从 `src` 读。
    aux: Option<Box<dyn yu_vfs::DataSource>>,
    tables: TableSet,
    /// `GOSUB`/`GO` 常量标签按调用点缓存(`(script_id, pc)` → 目标)。
    /// 标签表在启动装载后不变,窗口内无变量引用 ⇒ 每次结果相同。
    pub(crate) label_cache: rustc_hash::FxHashMap<(u32, u32), LabelTarget>,
    scheduler: Scheduler,
    store: VariableStore,
    /// 任务执行位(`TaskId` → 执行位)。
    ///
    /// `Box`:执行位本身 ~700B(16 组任务寄存器 + 三个栈),旧实现每条命令
    /// `remove_entry` + `insert` 搬两次整块;装箱后只搬指针。
    exec: rustc_hash::FxHashMap<String, Box<ExecState>>,
    /// **当前泵任务**的执行位缓存:同一任务连续推进时不碰哈希表。
    ///
    /// 调度器 `pump_node` 会对同一任务连跑多到上限条命令,逐条
    /// 摘/插哈希表是纯开销(`IFEND` 的 ~30ns 固定成本主要就是它)。
    /// 换任务/任务结束时由 `flush_exec_slot` 还回 `exec`。
    exec_slot: Option<(String, Box<ExecState>)>,
    input: InputState,
    binding: InputBinding,
    /// 图形域同步宿主(可选;安装后负责 INFO 查询与状态同步)。
    graphics_host: Option<Box<dyn crate::event::GraphicsHost>>,
    /// 声音域同步宿主(可选;SOUND/SOUNDEND 消费 + SOUNDINFO 查询)。
    sound_host: Option<Box<dyn crate::event::SoundHost>>,
    /// 存档域同步宿主(可选;SAVE 预约/刷盘 + LOAD 拉取,B1 裁 1-A/2-A)。
    save_host: Option<Box<dyn crate::event::SaveHost>>,
    events: Vec<VmEvent>,
    diags: MemorySink,
    pending_effects: Vec<Effect>,
    script_errors: Vec<String>,
    frame: u64,
    now_ms: u32,
    start_ms: u32,
    /// 宿主本地时间基准(`@_TIME`;`None` = 确定性注入时钟)。
    local_time: Option<HostLocalTime>,
    frame_ms: u32,
    fps: u32,
    skip: bool,
    sys_count: u16,
    screen: (u32, u32),
    debug_mode: bool,
    argv: Vec<Vec<u8>>,
    screen_color: u32,
    paths: HostPaths,
    media_readonly: bool,
    os_sound: bool,
    dx_mode: u32,
    window_init_pos: i64,
    /// `@_SAVE_PATH_TYPE`(宿主从 `yssfs.dat` 读入;见 `BootConfig`)。
    save_path_type: i64,
    /// `MATH[RND]` 的 MT19937(DIV-RT-014)。
    rng: crate::rng::Mt19937,
    record_events: bool,
    /// opcode → 处理器类别表(启动期按工程 yscm 名字解析)。
    handler_of: Vec<crate::vm::handlers::HandlerKind>,
    /// 命令视图缓存:键 = `(script_id, pc)`。脚本装载后命令字节不变,
    /// 每条命令的窗口载荷只在首次执行时复制一次(Tyn 同思路优化)。
    /// 命令视图缓存:按脚本号 → pc 直接下标(脚本号/pc 都是稠密的)。
    /// 脚本装载后命令字节不变,窗口载荷只复制一次;之后每次执行只做一次
    /// `Rc` 克隆。旧实现用 `FxHashMap<(u32,u32)>` 每命令一次哈希。
    cmd_cache: Vec<Vec<Option<Rc<Cmd>>>>,
    /// 表达式求值复用栈(热路径零分配)。
    eval_scratch: Vec<crate::expr::Slot>,
    /// 命令窗口解码缓存(键 = FNV-1a(payload);碰撞用原始字节二次比较)。
    /// 脚本装载后窗口字节不变,缓存跨帧有效。
    decode_cache: rustc_hash::FxHashMap<u64, Vec<DecodedWindow>>,
    decode_cache_entries: usize,
    texts: BTreeMap<String, Vec<u8>>,
    /// 显示文本层状态(`$_TEXT`/`$_TEXT_FLAG`/`TEXTINFO` 的数据源;
    /// 引擎 `[0x801800]` 的已取证字段,见 [`crate::text`])。
    text: crate::text::TextState,
    /// `TEXT[ID=...]`:后续文本事件的目标文本层 ID(手册 `text/text.html`)。
    /// 事件字段是 `String`;非 UTF-8 的 CP932 ID 会显式报告并保持为空。
    text_layer_id: String,
    /// `@_MENUCODE`(手册:最後に選択されたメニュー項目の CODE 値;○ 可赋值)。
    menu_code: i64,
    /// `@_WINDOW_EXEC_MAX/MIN/CLOSE/CAPTION`:窗口按钮/尺寸事件的按下状态。
    ///
    /// 官方把这四个做成**连续 4 字节全局**(0.495 `0x801918`/`0x80191c`/
    /// `0x801920`/`0x801924`),读用 `MOVSX`(有符号字节),写用 `MOV byte`
    /// —— 即**存储宽度是 1 字节**。MAX/CLOSE 手册有页(○ 可赋值),
    /// MIN/CAPTION 手册无页(DIV-RT-007)。
    window_exec_max: i64,
    window_exec_min: i64,
    window_exec_close: i64,
    /// `@_WINDOW_EXEC_CAPTION`:窗口尺寸再初始化的一次性宿主事件标志
    /// (DIV-RT-007 的宿主 setter 形态;P3 收口)。
    window_exec_caption: i64,
    executed: u64,
    /// 每条命令(opcode)的执行次数。**不受 `record_events` 影响**:
    /// 覆盖普查要在关闭事件流的热路径上拿到逐命令频度(任务书 M-1)。
    /// 下标 = 工程 `ysc.ybn` 的命令序号;长度在启动期固定。
    command_hits: Vec<u64>,
    /// `command_hits` 的增量基线(宿主逐帧取「本帧各命令执行次数」的
    /// 诊断游标;与 `command_hits` 同长,只在
    /// `take_command_hit_deltas` 里推进)。
    hit_mark: Vec<u64>,
    /// 逐命令耗时采样开关(`Vm::set_command_profile`;默认关)。
    /// 开启后每条命令两次 `Instant` 采样,只用于定位启动/场景切换卡顿,
    /// 不进正常路径。
    prof_commands: bool,
    /// 采样窗口内每条 opcode 的累计纳秒(`prof_commands` 为 false 时不写)。
    /// **按 1/32 抽样并折算**:逐条 `Instant` 本身要 ~0.2µs,原实现把测量
    /// 开销算进了命令成本(LET 显示 0.21µs 里大半是采样器自己的开销)。
    prof_ns: Vec<u64>,
    /// 采样计数器(每 `prof_sample` 条命令计时一次)。
    prof_calls: u64,
    /// 命令计时采样率(默认 32;`1` = 逐条计时,仅诊断用)。
    prof_sample: u64,
    /// 采样窗口内每条 opcode 的命中数(与 `prof_ns` 同窗口)。
    prof_hits: Vec<u64>,
    /// `VARACT[INIT=1 G_*]` 的全局槽镜像(启动期从 ysv 一次性投影)。
    ///
    /// 启动脚本对同一组掩码调用数万次;原实现每次都 clone 全部 ysv 记录、
    /// 重新解码字符串初值再重建槽位(首帧 1.5s 的 82% 在 VARACT)。
    globals_restore: Vec<GlobalRestore>,
    /// 每个掩码组合最近一次恢复后的 `VariableStore::generation()`。
    /// 与当前代号相同 ⇒ 期间没有任何写入,重复恢复可直接跳过。
    last_global_restore: Vec<Option<u64>>,
    /// `VARACT` 分支计数(`--profile-frames` 取证;正常路径只是 6 个 u64)。
    varact_calls: u64,
    varact_init_calls: u64,
    varact_type_calls: u64,
    restore_skips: u64,
    restore_rows: u64,
    /// 采样窗口内 `VARACT` 的调用点 `(script_id, pc) -> hits`。
    prof_varact_sites: BTreeMap<(u32, u32), u64>,
    /// 采样窗口内所有命令的调用点 `(opcode, script_id, pc) -> hits`。
    /// 只在 `prof_commands` 为真时写;用于回答“这 50 万条 LET 来自哪”。
    prof_sites: rustc_hash::FxHashMap<(u8, u32, u32), u64>,
    /// `VARACT` 字符串子操作计数:[COPY, CUT, UPPER, UPPER2, LOWER, LOWER2,
    /// HANTOZEN, ZENTOHAN]。
    varact_ops: [u64; 8],
    /// 字符串子操作处理的源字节总数(判断是否大字符串拷贝)。
    varact_bytes: u64,
    /// 官方默认槽(`0x459e90`)命中次数:每次执行都计入;事件/诊断只发一次
    /// (官方全局 `[0x5e4f84]` 抑制重复错误框;DIV-RT-043)。
    script_slot_hits: u64,
    /// 默认槽错误是否已上报(对应引擎全局错误标志 `[0x5e4f84]`)。
    script_error_reported: bool,
    /// 待回答的 DIALOG(官方模态,理论上至多一条;内核允许排队,
    /// 仍按 id 顺序回答)。见 [`crate::dialog`]。
    dialogs: Vec<crate::dialog::DialogRequest>,
    next_dialog_id: u64,
    /// 剪贴板文本镜像(CP932 原始字节)。`CLIPACT` 写、`CLIPINFO` 读;
    /// 宿主可经 [`Vm::set_clipboard`] 注入外部剪贴板内容。
    clipboard: Vec<u8>,
    /// `ERROR` 的逐命令启用表(引擎 `0x689b00`;默认全 1,REA `0x462124`)。
    command_enable: Vec<u8>,
    unimplemented_cmds: BTreeMap<String, u64>,
    unimplemented_reported: std::collections::BTreeSet<String>,
    /// aligned-stub 命令(MIDI 族/CDDA)的「只报一次」去重集。
    aligned_stub_reported: std::collections::BTreeSet<String>,
    /// `TASKINFO` LET 类型不符诊断的「只报一次」去重集(按 query + 目标类型)。
    /// 引擎每次都会打印错误(`0x465cdc`),但脚本热循环里重复报告只会刷屏。
    taskinfo_type_reported: std::collections::BTreeSet<String>,
    /// 事件队列容量上限(任务书 E R10:长跑不得无界增长)。
    event_capacity: usize,
    /// 因容量上限被丢弃的最旧事件数(可观测,不静默)。
    events_dropped: u64,
    /// 当前正在执行的命令位置(诊断定位用;任务书 E R11)。
    /// `report()` 在 `Diag.where_ == Loc::None` 时补进 `Loc::Script`。
    /// 命令名按 `(cur_script, cur_pc)` 从取指缓存惰性取,不逐条克隆 Rc。
    cur_script: u32,
    cur_pc: u32,
    cur_task: String,
    /// `cur_task` 对应的调度器序号(序号不变就不重写字符串)。
    cur_task_seq: u64,
    /// 本步可见的任务链投影(`TASKINFO` 的数据源;泵期间调度器被借出)。
    task_view: Vec<TaskView>,
    /// 资产像素尺寸缓存(键 = `CG[FILE]`;`None` = 探测失败也缓存)。
    asset_size_cache: BTreeMap<Vec<u8>, Option<(u32, u32)>>,
}

/// 一个已解码的命令窗口(表达式解码缓存的值)。
struct DecodedWindow {
    payload: Vec<u8>,
    instrs: Rc<[crate::expr::RawInstr]>,
}

/// `VARACT[INIT]` 可恢复的全局变量(启动期投影;ysv 记录不变)。
#[derive(Clone, Debug)]
struct GlobalRestore {
    var: VarRef,
    ty: ElemType,
    dims: Vec<usize>,
    init: Value,
    /// 恢复组号 1..4(`G_INT..G_INT4`);其他组没有否决项。
    group: u8,
    /// 类型族 0=INT / 1=FLT / 2=STR(对应 ysv `value_type` 1/2/3)。
    family: usize,
}

fn window_hash(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    h ^= bytes.len() as u64;
    h = h.wrapping_mul(0x0000_0100_0000_01b3);
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

impl Vm {
    /// 启动链(表装载 + 入口标签 + 主线任务 + 入口脚本)。
    ///
    /// 单根:`ysbin/*` 与 `system/YSCom/YSCom.ycd` 在同一挂载根下。
    pub fn boot(src: Box<dyn yu_vfs::DataSource>, cfg: BootConfig) -> Result<Self, Diag> {
        Self::boot_with_aux(src, None, cfg)
    }

    /// 双根启动:编译产物在 `src`,编译器表(`YSCom.ycd`)可回退到 `aux`。
    ///
    /// 现实布局:`<project>/data/ysbin/*` + `<project>/system/YSCom/YSCom.ycd`,
    /// 或 SDK 目录。详见 [`TableSet::load_from`]。
    pub fn boot_with_aux(
        src: Box<dyn yu_vfs::DataSource>,
        aux: Option<Box<dyn yu_vfs::DataSource>>,
        cfg: BootConfig,
    ) -> Result<Self, Diag> {
        Self::boot_inner(src, aux, cfg)
    }

    fn boot_inner(
        src: Box<dyn yu_vfs::DataSource>,
        aux: Option<Box<dyn yu_vfs::DataSource>>,
        cfg: BootConfig,
    ) -> Result<Self, Diag> {
        let mut tables = TableSet::load_from(src.as_ref(), aux.as_deref(), cfg.script_key)?;
        let handler_of = crate::vm::handlers::build_handler_table(&tables.boot.yscm);

        // 入口标签(引擎 `FUN_00451e90` 按名字查 ysl;strcmp 字节级)。
        //
        // 标签名在 ysl 里是**原始 CP932 字节**:语料的演示标签有
        // `フェイスウィンドウサンプル` 这类非 ASCII 名。`LabelTable::get`
        // 只做 ASCII 等值(见 `tables.rs`),所以这里先用 CP932 编码再走
        // 字节级 `get_bytes`;ASCII 名的编码结果与原文相同,行为不变。
        let entry_name = match yu_format::Codec::Cp932.encode(&cfg.entry_label, b"entry") {
            Ok(bytes) => bytes,
            Err(_) => cfg.entry_label.as_bytes().to_vec(),
        };
        let label = tables
            .boot
            .labels
            .get_bytes(&entry_name)
            .ok_or_else(|| {
                diag::fatal(
                    code::rt::LABEL,
                    Boundary::Script,
                    format!("入口标签 `{}` 不在工程 ysl.ybn(拒绝启动)", cfg.entry_label),
                )
                .at_table("ysl.ybn")
            })?
            .clone();
        let script_id = label.script_id as u32;
        let pc = label.pc;
        {
            let script = tables.load_script(src.as_ref(), script_id)?;
            if pc as usize >= script.group_count() {
                return Err(diag::fatal(
                    code::rt::LABEL,
                    Boundary::Script,
                    format!(
                        "入口标签 `{}` 指向 s{script_id} pc={pc},脚本仅 {} 组",
                        cfg.entry_label,
                        script.group_count()
                    ),
                )
                .at_table("ysl.ybn"));
            }
        }

        let sys_count = tables.boot.sysvars.len() as u16;
        let screen = (tables.boot.config.width, tables.boot.config.height);
        let binding = InputBinding::from_sysvars(&tables.boot.sysvars);
        // 逐命令命中计数按工程 yscm 的命令数开表(下标 = opcode)。
        let command_hits = vec![0u64; handler_of.len()];
        let handler_len = command_hits.len();

        let mut vm = Vm {
            src,
            aux,
            tables,
            label_cache: rustc_hash::FxHashMap::default(),
            scheduler: Scheduler::new(),
            store: VariableStore::new(),
            exec: rustc_hash::FxHashMap::default(),
            exec_slot: None,
            input: InputState::new(),
            binding,
            graphics_host: None,
            sound_host: None,
            save_host: None,
            events: Vec::new(),
            diags: MemorySink::new(),
            pending_effects: Vec::new(),
            script_errors: Vec::new(),
            frame: 0,
            now_ms: cfg.start_ms,
            start_ms: cfg.start_ms,
            local_time: cfg.local_time,
            frame_ms: cfg.frame_ms,
            fps: cfg.fps,
            skip: false,
            sys_count,
            screen,
            debug_mode: cfg.debug_mode,
            argv: cfg.argv,
            screen_color: cfg.screen_color,
            paths: cfg.paths,
            media_readonly: cfg.media_readonly,
            os_sound: cfg.os_sound,
            dx_mode: cfg.dx_mode,
            window_init_pos: cfg.window_init_pos,
            save_path_type: cfg.save_path_type,
            rng: crate::rng::Mt19937::new(cfg.rng_seed),
            record_events: cfg.record_events,
            handler_of,
            cmd_cache: Vec::new(),
            eval_scratch: Vec::new(),
            decode_cache: rustc_hash::FxHashMap::default(),
            decode_cache_entries: 0,
            texts: BTreeMap::new(),
            text: crate::text::TextState::default(),
            text_layer_id: String::new(),
            menu_code: 0,
            window_exec_max: 0,
            window_exec_min: 0,
            window_exec_close: 0,
            window_exec_caption: 0,
            executed: 0,
            command_hits,
            hit_mark: vec![0u64; handler_len],
            prof_commands: false,
            prof_ns: vec![0u64; handler_len],
            prof_calls: 0,
            prof_sample: 32,
            prof_hits: vec![0u64; handler_len],
            globals_restore: Vec::new(),
            last_global_restore: vec![None; GLOBAL_RESTORE_KEYS],
            varact_calls: 0,
            varact_init_calls: 0,
            varact_type_calls: 0,
            restore_skips: 0,
            restore_rows: 0,
            prof_varact_sites: BTreeMap::new(),
            prof_sites: rustc_hash::FxHashMap::default(),
            varact_ops: [0; 8],
            varact_bytes: 0,
            script_slot_hits: 0,
            script_error_reported: false,
            dialogs: Vec::new(),
            next_dialog_id: 1,
            clipboard: Vec::new(),
            command_enable: vec![1u8; 128],
            unimplemented_cmds: BTreeMap::new(),
            unimplemented_reported: std::collections::BTreeSet::new(),
            aligned_stub_reported: std::collections::BTreeSet::new(),
            taskinfo_type_reported: std::collections::BTreeSet::new(),
            event_capacity: DEFAULT_EVENT_CAPACITY,
            events_dropped: 0,
            cur_script: script_id,
            cur_pc: pc,
            cur_task: MAIN_TASK_ID.to_string(),
            cur_task_seq: u64::MAX,
            task_view: Vec::new(),
            asset_size_cache: BTreeMap::new(),
        };
        vm.apply_global_initial_values();
        let restore = vm.build_globals_restore();
        vm.globals_restore = restore;
        vm.apply_script_locals(script_id);

        vm.exec
            .insert(MAIN_TASK_ID.to_string(), Box::new(ExecState::new(script_id, pc)));
        vm.scheduler.spawn(TaskSeed::main(script_id, pc));
        vm.events.push(VmEvent::TaskSpawn {
            frame: 0,
            id: MAIN_TASK_ID.to_string(),
            script: script_id,
            pc,
            enabled: true,
            main: true,
        });
        // 兜底日志(非 Fatal,进诊断汇不进事件流——事件流是对齐对象,
        // 宿主经 `Vm::diags`/`take_diags` 可见;表来源另见
        // `table_provenance` 的 `sysvars: … <- …` 行):
        // 商业发行版不带编译器定义,系统变量按工程 ysv 指纹选 ABI 族,
        // 已用**工程自己的 ysv.ybn 逐项校验**(DIV-RT-016);命令表/
        // 参数名始终来自工程 ysc.ybn(数据驱动,无版本兜底)。
        if vm.tables.boot.ycd.is_none() {
            vm.emit_diag(
                diag::info(
                    code::table::FALLBACK,
                    Boundary::Table,
                    format!(
                        "工程无 system/YSCom/YSCom.ycd(商业发行版常态):按工程 ysv.ybn 指纹\
                         选择系统变量 ABI 族 {} (已逐项校验通过,DIV-RT-016;已知族 \
                         0292/0466/0479/0495)。命令表/参数名仍来自工程的 ysc.ybn,\
                         无版本兜底。",
                        vm.tables.boot.sysvars.source.describe()
                    ),
                )
                .at_table("YSCom.ycd"),
            );
        }
        Ok(vm)
    }

    /// `ysv.ybn` 一条记录的初值 → 运行值。
    ///
    /// **串初值是 YSTB 参数数据,要在装载时求值,不是按字面存字节。**
    /// 证据(0.481 商业引擎 `FUN_00456e50`,`0x456f61` 调用点):
    /// ```text
    /// uVar3 = *puVar11;                                  // u16 长度
    /// uVar8 = (*DAT_008097e0)(pvVar6, puVar11 + 1, uVar3); // 窗口求值器
    /// FUN_00465654(uVar10, pvVar6, uVar8);                // 写变量
    /// ```
    /// 同一条目里数值初值走 `FUN_004658e0`(int)/`FUN_00465800`(float)
    /// 直写,只有串初值过求值器 —— 因为 `0x4d` 字面量与 `$1143[38,21]`
    /// 这类表达式在字节层是同一种东西(命令参数形态)。
    ///
    /// 只认字面量的实现会把 `$1143[38,21]` 的**字节码本身**当字符串存进
    /// 变量,随后被脚本拼进资源路径(实测:`cg/face/` 类前缀位置出现
    /// `56 03 00 24 77 04 …`,VFS 报 `NUL byte in lookup key`)。
    fn ysv_init_value(&mut self, r: &VarRecord, ty: ElemType, script_id: u32) -> Value {
        if let Some(raw) = r.str_init_bytes().filter(|_| r.str_init_needs_eval()) {
            return match self.eval_init_expr(raw, script_id) {
                // 串槽位:结果按串口径归一(与参数槽同一套 coerce)。
                Ok(v) => crate::expr::ParamTy::Str.coerce(v),
                Err(d) => {
                    // 求值失败不猜:留痕后按空串建槽(与「未声明初值」同
                    // 口径),避免把字节码当内容继续传播。
                    self.report(d);
                    Value::Str(Vec::new())
                }
            };
        }
        r.init_value().unwrap_or_else(|| ty.default_value())
    }

    /// 求值一条 `ysv` 串初值窗口(装载期,无当前任务)。
    fn eval_init_expr(&mut self, raw: &[u8], script_id: u32) -> Result<Value, Diag> {
        let instrs = self.decoded_window(raw)?;
        if let Some(var) = crate::expr::single_value_var(&instrs) {
            let st = ExecState::new(script_id, 0);
            let mut access = VmAccess { vm: self, st: &st };
            return access.read(var, &[]);
        }
        let mut stack = std::mem::take(&mut self.eval_scratch);
        let st = ExecState::new(script_id, 0);
        let mut access = VmAccess { vm: self, st: &st };
        let r = Evaluator::new(&mut access).eval_value_instrs(&instrs, &mut stack);
        self.eval_scratch = stack;
        r
    }

    fn apply_global_initial_values(&mut self) {
        let sys_count = self.sys_count;
        let records: Vec<_> = self
            .tables
            .boot
            .vars
            .records
            .iter()
            .filter(|r| matches!(r.kind, 1 | 3))
            .cloned()
            .collect();
        for r in records {
            let Some(ty) = r.elem_type() else { continue };
            if r.var_id < sys_count {
                // 前 sys_count 条 kind=1 是系统变量的镜像声明
                // (0.495 实测 548/548);系统变量由运行核按语义计算。
                continue;
            }
            let dims: Vec<usize> = r.dims.iter().map(|&d| d as usize).collect();
            let init = self.ysv_init_value(&r, ty, 0);
            self.store.declare(
                VarRef::new(VarSpace::for_elem(ty), r.var_id),
                ty,
                &dims,
                &init,
            );
        }
    }

    /// 启动期投影 `VARACT[INIT]` 需要的全局记录(ysv 在本进程内不变)。
    ///
    /// 过滤条件与 `restore_globals` 的可观察条件一致:`kind == 1`、
    /// 非系统变量镜像、类型可解析。初值在这里解码一次,之后每次恢复
    /// 只按 `ty/dims/init` 重置槽位。
    fn build_globals_restore(&mut self) -> Vec<GlobalRestore> {
        let sys_count = self.sys_count;
        let records: Vec<_> = self
            .tables
            .boot
            .vars
            .records
            .iter()
            .filter(|r| r.kind == 1 && r.var_id >= sys_count)
            .cloned()
            .collect();
        let mut out = Vec::with_capacity(records.len());
        for r in records {
            let Some(ty) = r.elem_type() else { continue };
            let init = self.ysv_init_value(&r, ty, 0);
            let family = match r.value_type {
                2 => 1,
                3 => 2,
                _ => 0,
            };
            out.push(GlobalRestore {
                var: VarRef::new(VarSpace::for_elem(ty), r.var_id),
                ty,
                dims: r.dims.iter().map(|&d| d as usize).collect(),
                init,
                group: r.group,
                family,
            });
        }
        out
    }

    fn apply_script_locals(&mut self, script_id: u32) {
        let records: Vec<_> = self
            .tables
            .boot
            .vars
            .locals_of(script_id as u16)
            .cloned()
            .collect();
        for r in records {
            let Some(ty) = r.elem_type() else { continue };
            let dims: Vec<usize> = r.dims.iter().map(|&d| d as usize).collect();
            let init = self.ysv_init_value(&r, ty, script_id);
            // 局部变量的声明指令会建槽;YSVR 只补初值镜像,不覆盖已建槽。
            self.store.declare_if_absent(
                VarRef::new(VarSpace::for_elem(ty), r.var_id),
                ty,
                &dims,
                &init,
            );
        }
    }

    // ---------------------------------------------------------- 访问器

    pub fn tables(&self) -> &TableSet {
        &self.tables
    }

    /// 可变表集合(预装载脚本 / 诊断用)。
    pub fn tables_mut(&mut self) -> &mut TableSet {
        &mut self.tables
    }

    pub fn events(&self) -> &[VmEvent] {
        &self.events
    }

    /// 取走已排队事件(调用方消费式读取;长跑应每帧或定期调用)。
    pub fn take_events(&mut self) -> Vec<VmEvent> {
        std::mem::take(&mut self.events)
    }

    /// 取走已排队事件并追加到调用方缓冲(避免每次分配;任务书 E R10)。
    pub fn drain_events(&mut self, out: &mut Vec<VmEvent>) {
        out.append(&mut self.events);
    }

    /// 调事件队列容量(0 = 不保留事件)。
    pub fn with_event_capacity(mut self, cap: usize) -> Self {
        self.event_capacity = cap.max(1);
        self.events.truncate(self.event_capacity);
        self
    }

    /// 安装图形域同步宿主。
    ///
    /// 安装后,图形/窗口/文本/帧事件在产生时同步送给宿主;INFO 命令通过
    /// [`crate::event::GraphicsHost::query`] 在脚本执行期取状态并写 `LET`。
    /// **调用方不得再把同一批事件 drain 后喂给同一个宿主**(会重复应用)。
    pub fn set_graphics_host(&mut self, host: Box<dyn crate::event::GraphicsHost>) {
        self.graphics_host = Some(host);
    }

    /// 取走图形宿主(便于宿主自己持有 `Rc` 状态)。
    pub fn take_graphics_host(&mut self) -> Option<Box<dyn crate::event::GraphicsHost>> {
        self.graphics_host.take()
    }

    /// 安装声音域同步宿主。安装后 `Domain::Sound` 事件同步进入宿主；
    /// `SOUNDINFO` 通过 [`crate::event::SoundHost::query`] 取状态并写 `LET`。
    /// **调用方不得再把同一批事件 drain 后喂给同一个宿主**。
    pub fn set_sound_host(&mut self, host: Box<dyn crate::event::SoundHost>) {
        self.sound_host = Some(host);
    }

    /// 取走声音宿主(便于宿主自己持有 `Rc` 状态)。
    pub fn take_sound_host(&mut self) -> Option<Box<dyn crate::event::SoundHost>> {
        self.sound_host.take()
    }

    /// 安装存档域同步宿主(B1)。安装后 `SAVE`/`LOAD`/`SAVEEND` 在**命令
    /// 时刻**走 [`crate::event::SaveHost`] 的预约/刷盘/拉取契约;
    /// `FILEACT`/`FILEINFO`/`SAVEINFO` 的 DomainCall 也在 [`Vm::emit`] 里
    /// **同步**喂给宿主(与 Graphics/Sound 同口径,官方处理体就是命令时刻
    /// 执行)。事件照常入队供 trace 记录,因此**调用方不得再把 drain 出来的
    /// 同一批事件喂给宿主**(会重复应用)。
    pub fn set_save_host(&mut self, host: Box<dyn crate::event::SaveHost>) {
        self.save_host = Some(host);
    }

    /// 取走存档宿主。
    pub fn take_save_host(&mut self) -> Option<Box<dyn crate::event::SaveHost>> {
        self.save_host.take()
    }

    /// 因容量上限被丢弃的最旧事件数。
    pub fn events_dropped(&self) -> u64 {
        self.events_dropped
    }

    pub fn event_jsonl(&self) -> String {
        crate::json::to_jsonl(&self.events)
    }

    pub fn diags(&self) -> &[Diag] {
        self.diags.records()
    }

    pub fn take_diags(&mut self) -> Vec<Diag> {
        self.diags.take()
    }

    /// 丢弃当前诊断但保留缓冲容量(服务端无 `/diag` 订阅者时用)。
    pub fn clear_diags(&mut self) {
        self.diags.clear();
    }

    pub fn script_errors(&self) -> &[String] {
        &self.script_errors
    }

    pub fn scheduler(&self) -> &Scheduler {
        &self.scheduler
    }

    /// 本步任务链投影(`TASKINFO` 数据源)。
    pub(crate) fn task_view(&self) -> &[TaskView] {
        &self.task_view
    }

    pub fn store(&self) -> &VariableStore {
        &self.store
    }

    pub fn store_mut(&mut self) -> &mut VariableStore {
        &mut self.store
    }

    pub fn input(&self) -> &InputState {
        &self.input
    }

    pub fn binding(&self) -> &InputBinding {
        &self.binding
    }

    pub fn fps(&self) -> u32 {
        self.fps
    }

    pub fn skip(&self) -> bool {
        self.skip
    }

    pub fn now_ms(&self) -> u32 {
        self.now_ms
    }

    pub fn start_ms(&self) -> u32 {
        self.start_ms
    }

    /// 宿主注入的本地时间基准(`@_TIME`;`None` = 确定性注入时钟)。
    pub fn local_time(&self) -> Option<HostLocalTime> {
        self.local_time
    }

    pub fn debug_mode(&self) -> bool {
        self.debug_mode
    }

    pub fn argv(&self) -> &[Vec<u8>] {
        &self.argv
    }

    pub fn screen(&self) -> (u32, u32) {
        self.screen
    }

    /// 取某任务的执行位快照(诊断 / TASKINFO 用)。
    ///
    /// 正在泵的任务,执行位在 `exec_slot` 而不是 `exec` 里 —— 漏查会让
    /// `TASKINFO` 读到旧状态(脚本可见)。
    pub(crate) fn exec_of(&self, id: &str) -> Option<&ExecState> {
        if let Some((slot_id, st)) = &self.exec_slot {
            if slot_id == id {
                return Some(st);
            }
        }
        self.exec.get(id).map(|st| &**st)
    }

    /// 把缓存里的执行位还回 `exec`(换任务 / 任务结束 / 需要遍历时)。
    fn flush_exec_slot(&mut self) {
        if let Some((id, st)) = self.exec_slot.take() {
            self.exec.insert(id, st);
        }
    }

    /// 输入状态的可变借用(写入输入系统变量)。
    /// 可变输入状态(宿主注入与测试用)。
    pub fn input_state_mut(&mut self) -> &mut InputState {
        &mut self.input
    }

    /// `@_SCREEN_COLOR`(宿主注入的色深)。
    pub fn screen_color(&self) -> u32 {
        self.screen_color
    }

    /// 路径类系统变量(宿主注入)。
    pub fn host_paths(&self) -> &HostPaths {
        &self.paths
    }

    /// `@_MEDIA_READONLY`(宿主注入)。
    pub fn media_readonly(&self) -> bool {
        self.media_readonly
    }

    /// `@_OS_SOUND`(宿主注入)。
    pub fn os_sound(&self) -> bool {
        self.os_sound
    }

    /// `@_DXMODE`(宿主注入)。
    pub fn dx_mode(&self) -> u32 {
        self.dx_mode
    }

    /// `@_WINDOW_INIT_POS`(宿主从 `yscfg.dat` 第 10 项注入;DIV-RT-047)。
    pub fn window_init_pos(&self) -> i64 {
        self.window_init_pos
    }

    /// `@_SAVE_PATH_TYPE`(宿主从 `yssfs.dat` 偏移 0x54 读入)。
    pub fn save_path_type(&self) -> i64 {
        self.save_path_type
    }

    /// `@_TEXT_NUM`:工程文本总数。
    ///
    /// 引擎在装载 `ysbin/yst.ybn` 时把其头部的 text_total 写进 `[0x80174c]`
    /// (REA `0x45d73f`),getter `0x4276e3` 直接读它。内核不装载 `yst.ybn`,
    /// 改用 `yst_list` 逐脚本 `text_count` 求和 —— ERIS 0.495 实测两者都是
    /// **396**(见 DIV-RT-048)。两者若在其它工程分歧,以引擎口径为准的
    /// `yst.ybn` 头部尚未接入,按已知偏差记录。
    pub fn text_total(&self) -> i64 {
        // 缺产物的脚本用 `u32::MAX` 哨兵(DIV-RT-016):不计入总数。
        let total: u64 = self
            .tables
            .boot
            .scripts
            .entries
            .iter()
            .map(|e| u64::from(e.text_count))
            .filter(|n| *n != u64::from(u32::MAX))
            .sum();
        total.min(i64::MAX as u64) as i64
    }

    pub fn engine_version(&self) -> u32 {
        self.tables.boot.yscm.version
    }

    /// 系统变量 ABI 表(名字/类型/维数)。
    pub fn sysvars(&self) -> &crate::sysvar_abi::SysVarTable {
        &self.tables.boot.sysvars
    }

    /// 表来源描述(进 golden 头/trace,可审计)。
    pub fn table_provenance(&self) -> Vec<String> {
        let mut out = vec![
            format!(
                "ysc: v{} ({} 命令)",
                self.tables.boot.yscm.version,
                self.tables.boot.yscm.len()
            ),
            format!(
                "ysl: v{} ({} 标签)",
                self.tables.boot.labels.version,
                self.tables.boot.labels.len()
            ),
            format!(
                "ysv: v{} ({} 记录)",
                self.tables.boot.vars.version,
                self.tables.boot.vars.records.len()
            ),
            format!(
                "yst_list: v{} ({} 条)",
                self.tables.boot.scripts.version,
                self.tables.boot.scripts.len()
            ),
            format!(
                "yscfg: v{} ({}x{})",
                self.tables.boot.config.version,
                self.tables.boot.config.width,
                self.tables.boot.config.height
            ),
            format!(
                "sysvars: {} 项 <- {}",
                self.tables.boot.sysvars.len(),
                self.tables.boot.sysvars.source.describe()
            ),
        ];
        if let Some(ycd) = &self.tables.boot.ycd {
            out.push(format!(
                "ycd: v{} ({} 命令)",
                ycd.version,
                ycd.commands.len()
            ));
        } else {
            out.push("ycd: 不存在(商业发行版;系统变量改用引擎 ABI 表 + ysv 校验)".to_string());
        }
        out
    }

    pub fn texts(&self) -> &BTreeMap<String, Vec<u8>> {
        &self.texts
    }

    /// 当前显示文本(默认层;`WORD`/`_` 追加,`$_TEXT` 写入替换)。
    pub fn current_text(&self) -> Vec<u8> {
        self.text.bytes.clone()
    }

    /// `$_TEXT_FLAG` 控制字节流(引擎 `[state+0x3c]`;见 [`crate::text::flag`])。
    pub fn text_flags(&self) -> Vec<u8> {
        self.text.flags.clone()
    }

    /// 文本层状态快照(宿主/gfx 桥读取;加法式 API,不改事件字段)。
    pub fn text_state(&self) -> crate::text::TextSnapshot {
        crate::text::TextSnapshot::from(&self.text)
    }

    /// 宿主渲染器推进文本光标后推入(`TEXTINFO[CX/CY]` 的数据源)。
    /// 引擎里 `+0x14/+0x18` 由显示例程更新;yu-rt 不绘制,故由宿主注入。
    pub fn set_text_cursor(&mut self, x: i64, y: i64) {
        self.text.cursor_x = x;
        self.text.cursor_y = y;
    }

    /// 宿主渲染器到达点击等待标志后推入(`TEXTINFO[CLICK]` 的数据源)。
    pub fn set_text_click(&mut self, click: bool) {
        self.text.click = click;
    }

    /// 追加显示文本(`WORD` 文本段 / `_` 式值;手册 `basic/svariable.html`:
    /// `$_TEXT` = 現在表示されているテキストの内容)。同时按字符写
    /// `$_TEXT_FLAG` 的 `'1'`/`'2'`(REA `0x458d44`)。
    pub(crate) fn append_text(&mut self, bytes: &[u8]) {
        self.text.append_text(bytes);
    }

    /// 替换显示文本(`$_TEXT` 写入;手册 ○ 可赋值 = 覆盖当前内容)。
    /// 标志流不在此重建:引擎写入路径未取证(`DIV-RT-032` 仍挂写入)。
    pub(crate) fn set_text(&mut self, bytes: Vec<u8>) {
        self.text.bytes = bytes;
    }

    /// 替换 `$_TEXT_FLAG` 控制字节流。
    ///
    /// REA `0x446b98..0x446bb5`(id 0x7b):把 RHS 字符串**逐字节拷进**
    /// `[state+0x3c]` 已有的标志缓冲;不改文本缓冲(`+0x38`)、不重建文本、
    /// 不按字符重算标志。P3 收口 DIV-RT-032 的写入路径。
    pub(crate) fn set_text_flags(&mut self, bytes: Vec<u8>) {
        self.text.flags = bytes;
    }

    /// `@_MENUCODE` 读取(菜单域宿主选中项后应经 [`Vm::set_menu_code`] 推入)。
    pub(crate) fn menu_code(&self) -> i64 {
        self.menu_code
    }

    /// `@_MENUCODE` 写入(手册 ○;菜单域宿主)。
    pub fn set_menu_code(&mut self, code: i64) {
        self.menu_code = code;
    }

    /// `@_WINDOW_EXEC_MAX/MIN/CLOSE/CAPTION` 读写(MAX/CLOSE 手册 ○;
    /// MIN/CAPTION 手册无页,按 REA 落地)。
    ///
    /// 官方存储是**有符号字节**(0.495 getter `FUN_00427350` case
    /// 117/118/119/120 = `MOVSX EBP,byte ptr [0x801918/1c/20/24]`;
    /// 写入 `MOV byte ptr [0x80191c],DL`),所以这里按 `i8` 截断后再
    /// 符号扩展 —— 脚本写 300 读回 44,写 -1 读回 -1。
    pub(crate) fn window_exec(&self, which: &str) -> i64 {
        match which {
            "MAX" => self.window_exec_max,
            "MIN" => self.window_exec_min,
            "CAPTION" => self.window_exec_caption,
            // 剩下的只有 "CLOSE"(调用方都按名字先过滤过)。
            _ => self.window_exec_close,
        }
    }

    pub(crate) fn set_window_exec(&mut self, which: &str, v: i64) {
        let v = i64::from(v as i8);
        match which {
            "MAX" => self.window_exec_max = v,
            "MIN" => self.window_exec_min = v,
            "CAPTION" => self.window_exec_caption = v,
            _ => self.window_exec_close = v,
        }
    }

    /// 宿主窗口的“关闭”按钮被按下(手册 `○@_WINDOW_EXEC_CLOSE`)。
    ///
    /// 引擎窗口过程把 WM_CLOSE 变成脚本可读的 0/1 状态,由脚本决定是否
    /// `END`;宿主**不得**直接退出进程。重复调用保持为 1。
    pub fn request_window_close(&mut self) {
        self.set_window_exec("CLOSE", 1);
    }

    /// 宿主要求的最大化状态(与 `request_window_close` 同型)。
    pub fn request_window_maximize(&mut self) {
        self.set_window_exec("MAX", 1);
    }

    /// 宿主窗口的「最小化」按钮被按下(手册无页;REA 落地,`fixed(no-manual)`)。
    ///
    /// 官方窗口过程 `FUN_0045acdc`(0.495)在 `WM_SYSCOMMAND`(`& 0xFFF0`)
    /// 分支 `SC_MINIMIZE (0xF020)` 写 `DAT_0080191c = (char)*ecx + 1`
    /// —— 即 `_WINDOW_EXEC_MIN = 窗口槽号 + 1`(主窗口槽 0 ⇒ 1);
    /// 同函数 `SC_MAXIMIZE (0xF030)` 写 `_WINDOW_EXEC_MAX`、
    /// `SC_CLOSE (0xF060)` 写 `_WINDOW_EXEC_CLOSE = 1`。
    /// 内核只持有状态,窗口动作由宿主决定。
    pub fn request_window_minimize(&mut self) {
        self.set_window_exec("MIN", 1);
    }

    /// 宿主窗口的「尺寸再初始化」事件(手册无页;DIV-RT-007)。
    ///
    /// 语料语义(`es_Scenario.yst`):`@_WINDOW_EXEC_CAPTION == 1` ⇒ 脚本
    /// 写回 0 并按新尺寸重建窗口。内核只持有这个一次性标志;触发条件
    /// (`WM_SYSCOMMAND` 路径里 `*ecx` 的来源)未取证,因此**只能由宿主**
    /// 在它认为需要重建窗口时调用本 setter,VM 不猜。
    pub fn request_window_caption(&mut self) {
        self.set_window_exec("CAPTION", 1);
    }

    // ---------------------------------------------------------- DIALOG

    /// 当前待回答的 DIALOG(按发起顺序)。宿主应逐个呈现并调用
    /// [`Vm::answer_dialog`];官方 MessageBox 是模态的,正常至多一条。
    pub fn pending_dialogs(&self) -> &[crate::dialog::DialogRequest] {
        &self.dialogs
    }

    /// 某任务当前挂起的 DIALOG id。
    pub fn dialog_of_task(&self, task: &str) -> Option<u64> {
        self.dialogs.iter().find(|d| d.task == task).map(|d| d.id)
    }

    // ---------------------------------------------------------- 剪贴板

    /// 内核侧剪贴板文本镜像(CP932 原始字节;`CLIPACT` 写、`CLIPINFO` 读)。
    pub fn clipboard(&self) -> &[u8] {
        &self.clipboard
    }

    /// 宿主注入外部剪贴板内容(OS 剪贴板是宿主状态;内核只保存镜像)。
    pub fn set_clipboard(&mut self, bytes: Vec<u8>) {
        self.clipboard = bytes;
    }

    /// `ERROR` 设置的逐命令启用表(引擎 `0x689b00`;默认全 1)。
    ///
    /// **未取证**:该表在引擎里的消费者未定位(REA 只找到初始化
    /// `0x462124`),因此内核只保存它、不据此改变分派(DIV-RT-046)。
    pub fn command_enable(&self) -> &[u8] {
        &self.command_enable
    }

    /// 回答一条 DIALOG。
    ///
    /// - `button`:1 基按钮序号(OK/Yes/No/Cancel 的位置,见
    ///   [`crate::dialog::DialogButtons`]);写回 `LET`。
    /// - `text`:`INPUT=1` 时的输入文本(CP932 原始字节),写回 `LETSTR`;
    ///   非 INPUT 模式忽略。
    ///
    /// 官方是同步模态(宿主线程被 MessageBox 阻塞);内核改为任务挂起 +
    /// 本方法回答,回答后任务在下一个逻辑帧从 `DIALOG` 的下一条继续
    /// (pc 在分派前已前进,与引擎一致)。
    pub fn answer_dialog(
        &mut self,
        id: u64,
        button: u8,
        text: Option<&[u8]>,
    ) -> Result<(), yu_log::Diag> {
        let Some(pos) = self.dialogs.iter().position(|d| d.id == id) else {
            return Err(diag::recoverable(
                code::rt::CMD_SLOT,
                Boundary::Script,
                format!("DIALOG#{id} 不存在或已回答"),
            ));
        };
        let request = &self.dialogs[pos];
        if !request.accepts_button(button) {
            return Err(diag::recoverable(
                code::rt::CMD_SLOT,
                Boundary::Script,
                format!(
                    "DIALOG#{id} 按钮序号 {button} 越界({} 的合法范围 1..={})",
                    request.buttons.as_str(),
                    request.buttons.count()
                ),
            ));
        }
        if request.requires_text() && text.is_none() {
            return Err(diag::recoverable(
                code::rt::CMD_SLOT,
                Boundary::Script,
                format!("DIALOG#{id} 是文本输入框,回答必须带文本(空串用 Some(b\"\"))"),
            ));
        }
        let request = self.dialogs.remove(pos);
        // LET = 左起第几个按钮(REA 0x440056/0x4400ea/0x44017e/0x440212/0x4402a6)。
        if let Some(target) = &request.let_target {
            let var = self.canonical_var(target.var);
            self.store
                .write(var, &target.indices, &Value::Int(i64::from(button)))
                .map_err(|d| self.locate(d))?;
        }
        // LETSTR = 输入框文本(仅 INPUT 模式;REA 0x440001 → 0x43f0d8)。
        if request.input {
            if let (Some(target), Some(bytes)) = (&request.letstr_target, text) {
                let var = self.canonical_var(target.var);
                self.store
                    .write(var, &target.indices, &Value::Str(bytes.to_vec()))
                    .map_err(|d| self.locate(d))?;
            }
        }
        // 唤醒发起任务(下一个逻辑帧继续 DIALOG 之后的指令):请求已从
        // `dialogs` 移除,调度器的 `dialog_pending` 查询自然变为 false。
        Ok(())
    }

    /// 脚本可见的输入设置(宿主用于光标显示/形状)。
    pub fn input_settings(&self) -> &crate::input::InputSettings {
        self.input.settings()
    }

    pub fn unimplemented_cmds(&self) -> &BTreeMap<String, u64> {
        &self.unimplemented_cmds
    }

    pub fn executed_commands(&self) -> u64 {
        self.executed
    }

    /// 每条命令(opcode 下标)的**执行次数**。与 [`Vm::executed_commands`]
    /// 不同,本计数不依赖 `record_events`,可在 `record_events=false` 的
    /// 热路径上用于覆盖普查(任务书 M-1)。下标对应工程 `ysc.ybn` 序号。
    pub fn command_hits(&self) -> &[u64] {
        &self.command_hits
    }

    /// 官方默认槽(`0x459e90`,脚本执行错误)的命中次数。
    ///
    /// 每次执行都计数;事件流里的 `rt.script.error` 诊断只出现一次
    /// (对应引擎全局错误标志 `[0x5e4f84]` 的「只弹一次」语义;DIV-RT-043)。
    pub fn script_slot_hits(&self) -> u64 {
        self.script_slot_hits
    }

    /// 默认槽错误是否已上报(诊断/普查用)。
    pub fn script_error_reported(&self) -> bool {
        self.script_error_reported
    }

    /// 记录一次默认槽命中(处理器内部使用)。
    pub(crate) fn note_script_slot(&mut self) {
        self.script_slot_hits = self.script_slot_hits.saturating_add(1);
    }

    /// 标记默认槽错误已上报(对应引擎 `[0x5e4f84] = 1`)。
    pub(crate) fn mark_script_error_reported(&mut self) {
        self.script_error_reported = true;
    }

    /// 按执行次数排序的命令名(启动/卡顿诊断用)。
    pub fn top_commands(&self, limit: usize) -> Vec<(String, u64)> {
        let mut out: Vec<(String, u64)> = self
            .command_hits
            .iter()
            .enumerate()
            .filter(|(_, hits)| **hits > 0)
            .map(|(index, hits)| {
                let name = u8::try_from(index)
                    .ok()
                    .and_then(|index| self.tables.boot.yscm.command(index))
                    .map(|decl| decl.name.clone())
                    .unwrap_or_else(|| format!("#{index}"));
                (name, *hits)
            })
            .collect();
        out.sort_by_key(|x| std::cmp::Reverse(x.1));
        out.truncate(limit);
        out
    }

    /// opcode 对应的处理器类别(启动期按工程 `ysc.ybn` 解析;覆盖普查
    /// 用它区分「已实现 / 域事件 / 无处理器」,不靠名字猜测)。
    pub fn handler_kind(&self, opcode: u8) -> Option<HandlerKind> {
        self.handler_of.get(opcode as usize).copied()
    }

    /// 取走「距上次调用」各命令的执行次数增量(宿主长帧取证的逐帧口径)。
    ///
    /// 只扫一遍 `command_hits`(数百项),不解析命令名、不分配字符串;
    /// 调用方按需把 opcode 映射成名字([`Vm::command_name`])。宿主每帧
    /// 调用一次 ⇒ 返回值严格是**这一帧**的命令构成,而不是累计值。
    pub fn take_command_hit_deltas(&mut self) -> Vec<(u8, u64)> {
        let mut out = Vec::new();
        for (index, total) in self.command_hits.iter().copied().enumerate() {
            let Some(mark) = self.hit_mark.get_mut(index) else {
                continue;
            };
            if total != *mark {
                let delta = total.saturating_sub(*mark);
                *mark = total;
                if let Ok(opcode) = u8::try_from(index) {
                    out.push((opcode, delta));
                }
            }
        }
        out
    }

    /// opcode → 命令名(诊断;未声明退回 `#N`)。
    pub fn command_name(&self, opcode: u8) -> String {
        self.tables
            .boot
            .yscm
            .command(opcode)
            .map(|decl| decl.name.clone())
            .unwrap_or_else(|| format!("#{opcode}"))
    }

    /// `VARACT` 取证计数:[调用, INIT, TYPE, 其他, 恢复跳过, 恢复行数]。
    pub fn varact_stats(&self) -> [u64; 6] {
        let other = self
            .varact_calls
            .saturating_sub(self.varact_init_calls)
            .saturating_sub(self.varact_type_calls);
        [
            self.varact_calls,
            self.varact_init_calls,
            self.varact_type_calls,
            other,
            self.restore_skips,
            self.restore_rows,
        ]
    }

    /// 取走采样窗口内 `VARACT` 的调用点(按次数降序),并清零。
    pub fn take_varact_sites(&mut self, limit: usize) -> Vec<((u32, u32), u64)> {
        let mut out: Vec<((u32, u32), u64)> = self
            .prof_varact_sites
            .iter()
            .map(|(k, v)| (*k, *v))
            .collect();
        out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        out.truncate(limit);
        self.prof_varact_sites.clear();
        out
    }

    /// `VARACT` 字符串子操作计数与源字节总量(诊断用)。
    pub fn varact_op_stats(&self) -> ([u64; 8], u64) {
        (self.varact_ops, self.varact_bytes)
    }

    /// 开关逐命令耗时采样(默认关;只给启动/场景切换卡顿定位用)。
    /// 采样率(每 N 条计时一次并折算;`1` = 逐条,计时开销会算进成本)。
    pub fn set_command_profile_sample(&mut self, sample: u64) {
        self.prof_sample = sample.max(1);
    }

    pub fn set_command_profile(&mut self, on: bool) {
        self.prof_commands = on;
    }

    /// 取走采样窗口内的命令耗时,按累计纳秒降序。
    ///
    /// 返回 `(rows, total_ns)`;`rows` 每项 `(命令名, 命中数, 累计纳秒)`。
    /// 取走后窗口清零(下一次调用只反映新窗口)。
    /// 清空采样窗口(宿主每帧调用,使 `--profile-frames` 的读数是
    /// **单帧**而不是“自上次打印以来”的累计)。
    pub fn reset_command_profile(&mut self) {
        self.prof_ns.fill(0);
        self.prof_hits.fill(0);
        self.prof_calls = 0;
        self.prof_varact_sites.clear();
        self.prof_sites.clear();
        self.varact_calls = 0;
        self.varact_init_calls = 0;
        self.varact_type_calls = 0;
        self.restore_skips = 0;
        self.restore_rows = 0;
        self.varact_ops = [0; 8];
        self.varact_bytes = 0;
    }

    pub fn take_command_profile(&mut self, limit: usize) -> (Vec<(String, u64, u64)>, u64) {
        let total_ns = self.prof_ns.iter().copied().fold(0u64, u64::saturating_add);
        let mut rows: Vec<(String, u64, u64)> = self
            .prof_ns
            .iter()
            .enumerate()
            .filter(|(_, ns)| **ns > 0)
            .map(|(index, ns)| {
                let name = u8::try_from(index)
                    .ok()
                    .and_then(|index| self.tables.boot.yscm.command(index))
                    .map(|decl| decl.name.clone())
                    .unwrap_or_else(|| format!("#{index}"));
                let hits = self.prof_hits.get(index).copied().unwrap_or(0);
                (name, hits, *ns)
            })
            .collect();
        rows.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
        rows.truncate(limit);
        self.prof_ns.fill(0);
        self.prof_hits.fill(0);
        self.prof_calls = 0;
        (rows, total_ns)
    }

    // ---------------------------------------------------------- 帧推进

    /// 推进一个 vsync 链(输入 → 调度器 → 推进注入时钟)。
    pub fn tick(&mut self, timeline: &InputTimeline) -> ChainReport {
        let next_frame = self.scheduler.frame().wrapping_add(1);
        self.input.advance(next_frame, timeline);
        self.frame = next_frame;
        let now = self.now_ms;
        if let Some(host) = self.sound_host.as_mut() {
            host.advance(now);
        }
        let mut sched = std::mem::take(&mut self.scheduler);
        let report = sched.tick_frame(self, now);
        self.scheduler = sched;
        let step = 1000u32.checked_div(self.fps).unwrap_or(self.frame_ms);
        self.now_ms = self.now_ms.wrapping_add(step);
        report
    }

    /// 每个任务每次泵的命令上限(宿主 `--pump-cap`;`None` = 无限泵)。
    /// 只影响调度让出点,不改变任何命令语义;但会改变脚本的帧原子性
    /// (DIV-HOST-002)。
    pub fn set_pump_cap(&mut self, cap: Option<usize>) {
        self.scheduler.set_pump_cap(cap);
    }

    /// 时钟严格固定的单帧(对拍/测试)。
    pub fn tick_at(&mut self, now_ms: u32, timeline: &InputTimeline) -> ChainReport {
        let next_frame = self.scheduler.frame().wrapping_add(1);
        self.input.advance(next_frame, timeline);
        self.now_ms = now_ms;
        if let Some(host) = self.sound_host.as_mut() {
            host.advance(now_ms);
        }
        let mut sched = std::mem::take(&mut self.scheduler);
        let report = sched.tick_frame(self, now_ms);
        self.scheduler = sched;
        report
    }

    // ---------------------------------------------------------- 诊断

    /// 事件入队。事件流是**有界**的(任务书 E R10):超过容量时丢最旧事件并
    /// 记账,长跑不会无界增长。容量可通过 [`Vm::with_event_capacity`] 调整。
    /// 发域事件,并附当前命令的脚本坐标(P4:`DomainCall.script/pc` 可选)。
    pub(crate) fn emit_domain(
        &mut self,
        domain: crate::event::Domain,
        name: impl Into<String>,
        args: Vec<crate::event::ArgValue>,
    ) {
        let frame = self.frame();
        let script = Some(self.cur_script);
        let pc = Some(self.cur_pc);
        self.emit(VmEvent::DomainCall {
            frame,
            domain,
            name: name.into(),
            args,
            script,
            pc,
        });
    }

    /// 控制流事件(`Jump`):只在事件流开启时入队。
    ///
    /// `record_events=false` 时引擎只统计事件条数,不读取 `Jump`;而场景
    /// 构建帧的 LOOPEND/IF/ELSE 可达数十万条,逐条入队 + 队满裁剪是 tick
    /// 尖峰的主要来源。图形/声音/存档宿主同步路径与 `Jump` 无关,跳过不改变
    /// 可观测语义。
    pub(crate) fn emit_flow(&mut self, ev: VmEvent) {
        if self.record_events {
            self.emit(ev);
        }
    }

    pub(crate) fn emit(&mut self, ev: VmEvent) {
        // 图形宿主必须**同步**看到状态事件:同一帧 `CG[...]` 之后的 `CGINFO`
        // 才能读到最新图层。仅转发宿主关心的类别,避免 Command 热路径开销。
        let graphics_relevant = matches!(
            &ev,
            VmEvent::FrameStart { .. }
                | VmEvent::FrameEnd { .. }
                | VmEvent::Text { .. }
                | VmEvent::DomainCall {
                    domain: crate::event::Domain::Graphics | crate::event::Domain::Window,
                    ..
                }
        );
        let mut consumed_by_host = false;
        if graphics_relevant {
            if let Some(host) = self.graphics_host.as_mut() {
                host.apply_event(&ev);
                // 只有同步宿主确实消费过,才能安全跳过入队;没有宿主的
                // 测试/探针仍依赖 `drain_events` 驱动 `GfxState`。
                consumed_by_host = true;
            }
        }
        // 图形/窗口域事件已由同步宿主在命令时刻消费;`record_events=false`
        // 时事件队列只被引擎统计后丢弃,不再入队(场景构建帧可达数万条)。
        // Text/FrameStart/FrameEnd 不在此列:它们仍可能被事件流消费者读取。
        if !self.record_events
            && consumed_by_host
            && matches!(
                &ev,
                VmEvent::DomainCall {
                    domain: crate::event::Domain::Graphics | crate::event::Domain::Window,
                    ..
                }
            )
        {
            return;
        }
        let sound_relevant = matches!(
            &ev,
            VmEvent::FrameStart { .. }
                | VmEvent::FrameEnd { .. }
                | VmEvent::DomainCall {
                    domain: crate::event::Domain::Sound,
                    ..
                }
        );
        if sound_relevant {
            if let Some(host) = self.sound_host.as_mut() {
                host.apply_event(&ev);
            }
        }
        // 存档/文件域事实(`FILEACT`/`FILEINFO`/`SAVEINFO`)同样在**命令时刻**
        // 落地:官方 `FILEACT` 处理体就地删除/复制。若推迟到帧末 drain,
        // 存档脚本「先删旧槽、再 `SAVE[FILE]` 同步刷盘」会让删除落在写盘
        // 之后,刚写出的存档被删。事件仍入队供 trace/golden 记录。
        let save_relevant = matches!(
            &ev,
            VmEvent::DomainCall {
                domain: crate::event::Domain::Save | crate::event::Domain::File,
                ..
            }
        );
        if save_relevant {
            if let Some(host) = self.save_host.as_mut() {
                host.apply_event(&ev);
            }
        }
        if self.events.len() >= self.event_capacity {
            // 批量丢弃最旧的四分之一:`Vec::remove(0)` 是 O(len),队满后逐条
            // 搬移会退化成 O(n²)(ERIS 首帧 130 万条事件实测卡死)。批量 drain
            // 摊还到每次推入仍是 O(1)。
            let batch = (self.event_capacity / 4).max(1);
            let n = batch.min(self.events.len());
            self.events.drain(..n);
            self.events_dropped += n as u64;
        }
        self.events.push(ev);
    }

    /// 诊断入库(不做容量裁剪:诊断本身已有每命令去重)。
    pub(crate) fn emit_diag(&mut self, d: Diag) {
        // MemorySink::emit 只做 Vec::push,不会失败;丢弃的是 Result 形参
        // (良性静默,Q1 自查 b 类)。
        let _ = self.diags.emit(&d);
    }

    pub(crate) fn report(&mut self, d: Diag) {
        // 脚本定位(任务书 E R11):调用点没给位置时补当前命令的 script/pc/命令名。
        let d = self.locate(d);
        let code = d.code.to_string();
        let detail = d.detail.clone();
        self.emit(VmEvent::Diag {
            frame: self.frame,
            code,
            detail,
        });
        self.emit_diag(d);
    }

    /// 当前命令(诊断用):按执行位从取指缓存取,不持有额外 `Rc`。
    fn current_cmd(&self) -> Option<Rc<Cmd>> {
        let script = self.cur_script as usize;
        let pc = self.cur_pc as usize;
        self.cmd_cache
            .get(script)
            .and_then(|list| list.get(pc))
            .and_then(Option::as_ref)
            .map(Rc::clone)
    }

    /// 给诊断补脚本位置(仅当调用点没给 `Loc`)。
    pub(crate) fn locate(&self, d: Diag) -> Diag {
        if !matches!(d.where_, yu_log::Loc::None) {
            return d;
        }
        let file = self.script_name(self.cur_script);
        let task = (!self.cur_task.is_empty()).then(|| self.cur_task.clone());
        let cmd = self.current_cmd().map(|cmd| cmd.name.clone());
        diag::at_script_task(d, file, task, self.cur_pc, cmd)
    }

    /// 未实现命令挂号(每个命令名只进事件流一次,计数全部)。
    pub(crate) fn report_unimplemented(&mut self, name: &str, detail: String) {
        *self.unimplemented_cmds.entry(name.to_string()).or_insert(0) += 1;
        if self.unimplemented_reported.insert(name.to_string()) {
            self.report(diag::cmd_unimplemented(name, detail));
        }
    }

    /// aligned-stub 命令(MIDI 族 `0x44aa40/60/80`、CDDA `0x429fb0`):
    /// 官方 0.495 处理体是空桩,内核同型对齐 —— **不计挂号表**(它不是
    /// 未实现),只以 Info 诊断标注归属,每个命令名只报一次(事件流不刷屏)。
    pub(crate) fn report_aligned_stub(&mut self, name: &str, detail: String) {
        if self.aligned_stub_reported.insert(name.to_string()) {
            self.report(diag::info(
                code::rt::CMD_ALIGNED_STUB,
                Boundary::Script,
                format!("{name}: {detail}"),
            ));
        }
    }

    /// 处理器内部产生的 `Unimplemented` 也计入命令挂号表(任务书 E R11):
    /// 这类诊断在 `step_command` 的错误路径里发出,不经过
    /// [`Vm::report_unimplemented`],所以单独记账。
    pub(crate) fn note_unimplemented(&mut self, name: &str) {
        *self.unimplemented_cmds.entry(name.to_string()).or_insert(0) += 1;
    }

    // ---------------------------------------------------------- 脚本/求值

    fn ensure_script(&mut self, script_id: u32) -> Result<(), Diag> {
        if self.tables.script(script_id).is_some() {
            return Ok(());
        }
        self.tables.load_script(self.src.as_ref(), script_id)?;
        self.apply_script_locals(script_id);
        Ok(())
    }

    /// 取出当前 pc 的命令视图(写入复用缓冲;每命令零次 Vec/String 分配)。
    fn fill_cmd(&mut self, st: &ExecState, cmd: &mut Cmd) -> Result<(), Diag> {
        self.ensure_script(st.script_id)?;
        let script = self.tables.script(st.script_id).expect("刚刚装载");
        let Some(group) = script.group(st.pc as usize) else {
            return Err(diag::fatal(
                code::rt::SCRIPT_ERROR,
                Boundary::Script,
                format!(
                    "s{} pc={} 越界(脚本 {} 组)",
                    st.script_id,
                    st.pc,
                    script.group_count()
                ),
            ));
        };
        // 参数名表随命令缓存一次(每个 (script,pc) 站点一份;执行期只 bump
        // 引用计数)。工程 ysc 表在启动后不变。
        cmd.param_names = self
            .tables
            .boot
            .yscm
            .command(group.opcode)
            .map(|c| c.params.iter().map(|p| p.name.clone()).collect::<Vec<_>>())
            .unwrap_or_default() // silent-ok:无该命令 ⇒ 空表(与旧 `param_names` 同口径)
            .into();
        let name = match self.tables.boot.yscm.command(group.opcode) {
            Some(decl) => decl.name.as_str(),
            None => {
                return Err(diag::fatal(
                    code::rt::CMD_UNKNOWN,
                    Boundary::Script,
                    format!(
                        "s{} pc={}:opcode {} 不在工程 ysc.ybn({} 条命令)",
                        st.script_id,
                        st.pc,
                        group.opcode,
                        self.tables.boot.yscm.len()
                    ),
                ))
            }
        };
        cmd.windows.clear();
        for rec in script.group_windows(st.pc as usize) {
            let payload = script.window_payload(rec).ok_or_else(|| {
                diag::fatal(
                    code::fmt::OUT_OF_BOUNDS,
                    Boundary::Format,
                    format!(
                        "s{} pc={}:窗口载荷 {}+{} 超出共享池",
                        st.script_id, st.pc, rec.offset, rec.length
                    ),
                )
            })?;
            let instrs = crate::expr::decode(payload)
                .ok()
                .map(Rc::<[crate::expr::RawInstr]>::from);
            let const_value = instrs.as_deref().and_then(crate::expr::const_value_instrs);
            cmd.windows.push(CmdWindow {
                rec: *rec,
                payload: payload.to_vec(),
                instrs,
                const_value,
            });
        }
        cmd.opcode = group.opcode;
        cmd.name.clear();
        cmd.name.push_str(name);
        cmd.pc = st.pc;
        Ok(())
    }

    /// 读变量(系统变量与普通变量统一通路)。
    ///
    /// `VARACT`/`VARINFO` 的 `SET` 可以是系统变量——ERIS `stdlib/ystdlib.yst`
    /// 的 `_strlen($_PSTR(1))` 等共 29 处实测如此(`_PSTR`=55)。
    pub(crate) fn read_var(
        &self,
        st: &ExecState,
        var: VarRef,
        indices: &[i64],
    ) -> Result<Value, Diag> {
        if var.id < self.sys_count {
            return crate::sysvar::read(self, st, var, indices);
        }
        self.store.read(var, indices)
    }

    /// 把 ERIS 旧式变量空间(0x60 数值 / 0x23 字符串)归一化到实际存储空间。
    ///
    /// `@G(n)`/`@L(n)` 编译成前缀 0x60,`$G(n)`/`$L(n)` 编译成 0x23;
    /// id 本身唯一,类型由工程 `ysv.ybn` 声明(DIV-RT-017)。找不到声明时
    /// 原样返回(上层会报未声明)。
    #[inline]
    pub(crate) fn canonical_var(&self, var: VarRef) -> VarRef {
        if !var.space.is_legacy() {
            return var;
        }
        // id 唯一 ⇒ 直接查两种存储空间(不走 `var_shape`,避免递归)。
        let ty = if self
            .store
            .shape(VarRef::new(VarSpace::At, var.id))
            .is_some()
        {
            crate::value::ElemType::Int
        } else if self
            .store
            .shape(VarRef::new(VarSpace::Dollar, var.id))
            .is_some()
        {
            crate::value::ElemType::Str
        } else if var.id < self.sys_count {
            match self.sysvars().get(var.id).map(|d| d.value_type) {
                Some(3) => crate::value::ElemType::Str,
                Some(2) => crate::value::ElemType::Float,
                _ => crate::value::ElemType::Int,
            }
        } else {
            // 声明缺失:按空间语义兜底(0x60=数值 / 0x23=字符串)。
            match var.space {
                VarSpace::LegacyStr => crate::value::ElemType::Str,
                _ => crate::value::ElemType::Int,
            }
        };
        VarRef::new(VarSpace::for_elem(ty), var.id)
    }

    /// 变量的形状(系统变量取工程 ABI 表声明;普通变量取变量域)。
    pub(crate) fn var_shape(&self, var: VarRef) -> Option<(crate::value::ElemType, Vec<usize>)> {
        if var.id < self.sys_count {
            let d = self.sysvars().get(var.id)?;
            let ty = match d.value_type {
                2 => crate::value::ElemType::Float,
                3 => crate::value::ElemType::Str,
                _ => crate::value::ElemType::Int,
            };
            return Some((ty, d.dims.iter().map(|&x| x as usize).collect()));
        }
        if !var.space.is_legacy() {
            return self.store.shape(var).map(|(ty, dims)| (ty, dims.to_vec()));
        }
        let canonical = self.canonical_var(var);
        self.store
            .shape(canonical)
            .map(|(ty, dims)| (ty, dims.to_vec()))
    }

    /// 解码一个命令窗口;同一窗口字节只解码一次。
    ///
    /// 这是 Tyn `decode_cached_slot` 的同思路优化:脚本装载后窗口字节不变,
    /// 启动链/标题热路径每帧成千上万次求值命中缓存后不再分配/解码。
    fn decoded_window(&mut self, payload: &[u8]) -> Result<Rc<[crate::expr::RawInstr]>, Diag> {
        let key = window_hash(payload);
        if let Some(bucket) = self.decode_cache.get(&key) {
            if let Some(hit) = bucket.iter().find(|entry| entry.payload == payload) {
                return Ok(Rc::clone(&hit.instrs));
            }
        }
        let decoded: Rc<[crate::expr::RawInstr]> = crate::expr::decode(payload)?.into();
        if self.decode_cache_entries >= DECODE_CACHE_CAP {
            self.decode_cache.clear();
            self.decode_cache_entries = 0;
        }
        self.decode_cache
            .entry(key)
            .or_default()
            .push(DecodedWindow {
                payload: payload.to_vec(),
                instrs: Rc::clone(&decoded),
            });
        self.decode_cache_entries += 1;
        Ok(decoded)
    }

    /// 已填充命令窗口的求值(优先用 `fill_cmd` 时的预解码/常量折叠)。
    pub(crate) fn eval_window(&mut self, st: &ExecState, w: &CmdWindow) -> Result<Value, Diag> {
        if let Some(value) = &w.const_value {
            return Ok(value.clone());
        }
        let instrs = match &w.instrs {
            Some(instrs) => Rc::clone(instrs),
            None => self.decoded_window(&w.payload)?,
        };
        if let Some(var) = crate::expr::single_value_var(&instrs) {
            let mut access = VmAccess { vm: self, st };
            return access.read(var, &[]);
        }
        let mut stack = std::mem::take(&mut self.eval_scratch);
        let mut access = VmAccess { vm: self, st };
        let r = Evaluator::new(&mut access).eval_value_instrs(&instrs, &mut stack);
        self.eval_scratch = stack;
        r
    }

    /// 已填充命令窗口的左值解析(预解码命中路径)。
    pub(crate) fn eval_lvalue_window(
        &mut self,
        st: &ExecState,
        w: &CmdWindow,
    ) -> Result<LValue, Diag> {
        let instrs = match &w.instrs {
            Some(instrs) => Rc::clone(instrs),
            None => self.decoded_window(&w.payload)?,
        };
        if let Some(var) = crate::expr::single_var(&instrs) {
            return Ok(crate::expr::LValue {
                var,
                indices: smallvec::SmallVec::new(),
            });
        }
        let mut stack = std::mem::take(&mut self.eval_scratch);
        let mut access = VmAccess { vm: self, st };
        let r = Evaluator::new(&mut access).eval_lvalue_instrs_with(&instrs, &mut stack);
        self.eval_scratch = stack;
        r
    }

    /// 参数槽 → (1 基编号, 目标类型),零分配。
    ///
    /// 旧路径每次 `GOSUB`/`RETURN` 都 `param_name` 克隆一个 `String`,
    /// 再用 `param_index_of` 把名字尾部数字做两次 `String` 收集 + `parse`;
    /// 参数名来自工程 ysc 表(启动后不变),这些分配纯属浪费。
    pub(crate) fn param_plan(&self, opcode: u8, slot: u8) -> (usize, crate::expr::ParamTy) {
        let Some(decl) = self.tables.boot.yscm.param(opcode, slot) else {
            // 与旧路径一致:缺声明 ⇒ 空名 ⇒ 编号 1、类型不转换。
            return (1, crate::expr::ParamTy::Raw);
        };
        let name = decl.name.as_str();
        let n = param_index_bytes(name.as_bytes());
        let ty = if name.starts_with("PFLT") || name.starts_with("RFLT") {
            crate::expr::ParamTy::Float
        } else if name.starts_with("PSTR") || name.starts_with("RSTR") {
            crate::expr::ParamTy::Str
        } else if name.starts_with("PINT") || name.starts_with("RINT") {
            crate::expr::ParamTy::Int
        } else {
            crate::expr::ParamTy::Raw
        };
        (n, ty)
    }

    /// 参数声明名(工程 ysc;`None` = 该序号未声明)。诊断路径用。
    pub(crate) fn param_name(&self, opcode: u8, slot: u8) -> Option<String> {
        self.tables
            .boot
            .yscm
            .param(opcode, slot)
            .map(|p| p.name.clone())
    }

    /// 脚本文件显示名(诊断/事件头)。
    pub fn script_name(&self, script_id: u32) -> String {
        self.tables.script_path(script_id)
    }

    /// 脚本 pc → 源码行(诊断;未知返回 `None`)。
    pub fn script_position(&self, script_id: u32, pc: u32) -> Option<u32> {
        self.tables
            .script(script_id)
            .and_then(|script| script.source_position(pc as usize))
    }

    /// 最近一条已执行命令的落点(诊断:脚本显示名、pc、任务名)。
    ///
    /// 长帧取证用:和 `take_command_hit_deltas` 一起回答「这一帧在跑谁」。
    pub fn current_position(&self) -> (String, u32, String) {
        (
            self.script_name(self.cur_script),
            self.cur_pc,
            self.cur_task.clone(),
        )
    }

    /// 取走采样窗口内的命令调用点(按命中数降序),并清零。
    pub fn take_profile_sites(&mut self, limit: usize) -> Vec<(String, u32, u32, u64)> {
        let mut rows: Vec<(String, u32, u32, u64)> = self
            .prof_sites
            .iter()
            .map(|(&(opcode, script, pc), &hits)| {
                let name = self
                    .tables
                    .boot
                    .yscm
                    .command(opcode)
                    .map(|decl| decl.name.clone())
                    .unwrap_or_else(|| format!("#{opcode}"));
                (name, script, pc, hits)
            })
            .collect();
        rows.sort_by(|a, b| b.3.cmp(&a.3).then_with(|| a.0.cmp(&b.0)));
        rows.truncate(limit);
        self.prof_sites.clear();
        rows
    }

    /// 编译器表回退源是否在位(诊断)。
    pub fn has_aux_source(&self) -> bool {
        self.aux.is_some()
    }

    /// 标签按名解析(字节精确)。
    pub(crate) fn find_label(&self, name: &[u8]) -> Option<crate::tables::Label> {
        self.tables.boot.labels.get_bytes(name).cloned()
    }

    pub(crate) fn record_command(
        &mut self,
        st: &ExecState,
        cmd: &Cmd,
        args: Vec<crate::event::ArgValue>,
    ) {
        // 频度计数先行(与 `record_events` 无关):覆盖普查必须能在关闭
        // 事件流时拿到逐命令触发次数(任务书 M-1)。
        if let Some(hit) = self.command_hits.get_mut(cmd.opcode as usize) {
            *hit += 1;
        }
        if !self.record_events {
            return;
        }
        self.executed += 1;
        self.emit(VmEvent::Command {
            frame: self.frame,
            script: st.script_id,
            pc: cmd.pc,
            name: cmd.name.clone(),
            args,
        });
    }

    pub(crate) fn push_effect(&mut self, e: Effect) {
        self.pending_effects.push(e);
    }

    pub(crate) fn set_fps(&mut self, fps: u32, skip: bool) {
        self.fps = fps;
        self.skip = skip;
    }

    pub(crate) fn frame(&self) -> u64 {
        self.frame
    }
}

/// 求值期的变量访问适配器(变量域 + 系统变量)。
struct VmAccess<'a> {
    vm: &'a Vm,
    st: &'a ExecState,
}

impl VarAccess for VmAccess<'_> {
    fn read(&mut self, var: VarRef, indices: &[i64]) -> Result<Value, Diag> {
        // 一次空间判别覆盖三种情况:`@`/`$` 直通、ERIS 旧式空间
        // (`@G`/`@L`/`$G`/`$L`)按 id 的声明类型归一化、未知前缀挂号继续
        // (DIV-RT-017)。旧写法先无条件调 `canonical_var` 再做一次 matches,
        // 热路径(每帧 ~1.6 万次读)上多一次函数调用。
        let var = if var.space.is_legacy() {
            self.vm.canonical_var(var)
        } else {
            var
        };
        if matches!(var.space, VarSpace::Other(_)) {
            return Err(diag::unimplemented(
                code::rt::EXPR_UNSUPPORTED,
                Boundary::Script,
                format!(
                    "变量空间前缀 {:#04x}({})未取证:手册只定义 `@`/`$`(DIV-RT-017)",
                    var.space.prefix(),
                    var.display()
                ),
            ));
        }
        // 引擎 §3.10 结论三:槽位 id < sys_count 走系统变量通路。
        if var.id < self.vm.sys_count {
            return crate::sysvar::read(self.vm, self.st, var, indices);
        }
        self.vm.store.read(var, indices)
    }
}

impl TaskExecutor for Vm {
    fn begin_logical_frame(&mut self, frame: u64) {
        self.frame = frame;
        let (x, y) = self.input.cursor();
        if let Some(host) = self.graphics_host.as_mut() {
            host.set_pointer(x, y);
        }
        self.emit(VmEvent::FrameStart { frame });
    }

    fn end_logical_frame(&mut self, frame: u64) {
        // architecture §3 / A3:一帧事件以 `FrameStart` 开始、`FrameEnd` 结束。
        self.emit(VmEvent::FrameEnd { frame });
    }

    fn step_command(&mut self, task: &mut TaskNode) -> Result<StepOutcome, String> {
        // 同一任务连续推进时执行位留在 `exec_slot`;首次/换任务才查表。
        let cached = matches!(&self.exec_slot, Some((id, _)) if id == &task.id);
        if !cached {
            self.flush_exec_slot();
            let Some((id, st)) = self.exec.remove_entry(&task.id) else {
                return Ok(StepOutcome::Finish);
            };
            self.exec_slot = Some((id, st));
        }
        // 执行位必须临时移出 `self`,才能同时借 `&mut self` 跑脚本。
        // 装箱后这次移动只有指针 + 键,不再是 700B 整块。
        let (id, mut st) = self.exec_slot.take().expect("刚刚填过");
        let res = self.step_one(&mut st, task);
        // 同步任务记录(pc/脚本号;引擎 `task+0x20/+0x3c`)。
        task.pc = st.pc;
        task.script_id = st.script_id;

        match res {
            Ok(out) => {
                let finishing = matches!(out, StepOutcome::Finish);
                if !finishing {
                    self.exec_slot = Some((id, st));
                    // 文本行尾让出:任务级等待(引擎 `task+9`)由调度器记录,
                    // 推进输入出现时清标志(REA 0x44c451 / 0x4046fc)。
                } else {
                    self.emit(VmEvent::TaskFinish {
                        frame: self.frame,
                        id: id.clone(),
                    });
                }
                Ok(out)
            }
            Err(e) => {
                // 错误也是事件(architecture §8);带脚本位置(R11)。
                let (VmError::Fatal(d) | VmError::Diag(d)) = &e;
                let code = d.code.to_string();
                let detail = d.detail.clone();
                let recoverable = matches!(
                    d.severity,
                    yu_log::Severity::Unimplemented | yu_log::Severity::Recoverable
                );
                if matches!(d.severity, yu_log::Severity::Unimplemented) {
                    let name = self
                        .current_cmd()
                        .map(|cmd| cmd.name.clone())
                        .unwrap_or_default();
                    self.note_unimplemented(&name);
                }
                let positioned = self.locate(d.clone());
                self.events.push(VmEvent::Diag {
                    frame: self.frame,
                    code,
                    detail,
                });
                self.emit_diag(positioned);
                if recoverable {
                    // `Unimplemented`/`Recoverable` = 挂号留痕后**继续**执行下一条
                    // (该命令按未实装处理,无副作用)。architecture §8。
                    self.exec_slot = Some((id, st));
                    Ok(StepOutcome::Continue)
                } else {
                    // `Fatal`:任务终止(官方继续策略未取证 ⇒ DIV-RT-005)。
                    self.events.push(VmEvent::TaskFinish {
                        frame: self.frame,
                        id: id.clone(),
                    });
                    Err(e.to_string())
                }
            }
        }
    }

    fn drain_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.pending_effects)
    }

    fn task_removed(&mut self, id: &str) {
        // `TASKEND[ID=其他任务]`:摘链由调度器完成,这里回收执行位并落事件。
        let cached_hit = matches!(&self.exec_slot, Some((slot_id, _)) if slot_id == id);
        if cached_hit {
            self.exec_slot = None;
        }
        if cached_hit || self.exec.remove(id).is_some() {
            self.emit(VmEvent::TaskFinish {
                frame: self.frame,
                id: id.to_string(),
            });
        }
    }

    fn text_advance_requested(&self) -> bool {
        self.input
            .text_advance_requested(&self.text.click_buttons, &self.text.skip_buttons)
    }

    /// `TEXT[OUTPUTMODE]`(= 引擎文本状态 `+0x1c`):1 = 自动,其余 = 手动。
    ///
    /// 调度器用它决定 `task+9` 的恢复判据(引擎 `FUN_004046fc`
    /// `0x404711 CMP byte[state+0x1c],1` / `0x404715 JNE`):手动模式一帧即恢复,
    /// 只有自动模式才等推进输入。见 `task::TaskExecutor::text_auto_mode`。
    fn text_auto_mode(&self) -> bool {
        self.text.output_mode == 1
    }

    /// REA `0x4046fc` 手动路径的 `0x404828 call 0x4638e4`:请求位
    /// `[+0x36]`(RETURNCODE 手动模式 `0x44c437` 置位)在位 ⇒ 清空
    /// `$_TEXT`(`[+0x38]`)/`$_TEXT_FLAG`(`[+0x3c]`)与内部读位。
    fn consume_text_clear_request(&mut self) {
        if self.text.clear_pending {
            self.text.clear();
        }
    }

    fn dialog_pending(&self, task: &str) -> bool {
        self.dialogs.iter().any(|d| d.task == task)
    }

    fn sync_task_view(&mut self, view: &[TaskView]) {
        self.task_view.clear();
        self.task_view.extend_from_slice(view);
    }
}

impl Vm {
    fn step_one(
        &mut self,
        st: &mut ExecState,
        task: &mut TaskNode,
    ) -> Result<StepOutcome, VmError> {
        // pc 越界 = 脚本尾(引擎:任务体走完)。
        let cmd = self.cached_cmd(st)?;
        // 诊断定位:记住「当前正在执行的命令」(R11)。命令名只存 Rc
        // (取指缓存已有),不逐条复制字符串;任务名只在任务切换时重写。
        self.cur_script = st.script_id;
        self.cur_pc = cmd.pc;
        if self.cur_task_seq != task.seq {
            self.cur_task_seq = task.seq;
            self.cur_task.clear();
            self.cur_task.push_str(&task.id);
        }
        // 取指后先自增(引擎 §3.2b:先取后加;处理体写 pc 即跳转)。
        st.pc = st.pc.wrapping_add(1);
        // 可选耗时采样:只包住 dispatch(取指/缓存命中是固定成本)。
        // 关闭时只多一次 bool 判断。
        // 抽样计时(1/32):`Instant::now()` 本身 ~20–25ns × 2,逐条计时会把
        // 采样器开销算进命令成本;命中数仍逐条累加(数组自增,便宜)。
        let rate = self.prof_sample;
        let sample = self.prof_commands && self.prof_calls % rate == 0;
        self.prof_calls = self.prof_calls.wrapping_add(1);
        let opcode = cmd.opcode as usize;
        if self.prof_commands {
            if let Some(slot) = self.prof_hits.get_mut(opcode) {
                *slot = slot.saturating_add(1);
            }
            if sample {
                *self
                    .prof_sites
                    .entry((cmd.opcode, st.script_id, cmd.pc))
                    .or_insert(0) += 1;
            }
        }
        let profile_started = sample.then(Instant::now);
        let out = self.dispatch(st, task, &cmd);
        if let Some(started) = profile_started {
            let ns = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
            if let Some(slot) = self.prof_ns.get_mut(opcode) {
                *slot = slot.saturating_add(ns.saturating_mul(rate));
            }
        }
        out
    }

    /// 取一条命令的缓存视图。脚本装载后 `(script_id, pc)` 的命令字节不变,
    /// 窗口载荷只复制一次;之后每次执行只做一次 `Rc` 克隆。
    fn cached_cmd(&mut self, st: &ExecState) -> Result<Rc<Cmd>, Diag> {
        let script = st.script_id as usize;
        let pc = st.pc as usize;
        if script >= CMD_CACHE_SCRIPT_MAX || pc >= CMD_CACHE_PC_MAX {
            // 畸形输入:不建巨型缓存,交给 fill_cmd 的边界检查报错。
            let mut cmd = Cmd::default();
            self.fill_cmd(st, &mut cmd)?;
            return Ok(Rc::new(cmd));
        }
        if self.cmd_cache.len() <= script {
            self.cmd_cache.resize_with(script + 1, Vec::new);
        }
        if self.cmd_cache[script].len() <= pc {
            self.cmd_cache[script].resize_with(pc + 1, || None);
        }
        if let Some(hit) = self.cmd_cache[script].get(pc).and_then(Option::as_ref) {
            return Ok(Rc::clone(hit));
        }
        let mut cmd = Cmd::default();
        self.fill_cmd(st, &mut cmd)?;
        let cmd = Rc::new(cmd);
        self.cmd_cache[script][pc] = Some(Rc::clone(&cmd));
        Ok(cmd)
    }
}

/// 参数名尾部编号(字节版;`PINT2` → 2、`PINT` → 1)。
///
/// 与 `handlers::param_index_of` 严格同口径:无数字尾 ⇒ 1;`parse` 溢出 ⇒ 1。
fn param_index_bytes(name: &[u8]) -> usize {
    let mut end = name.len();
    while end > 0 && name[end - 1].is_ascii_digit() {
        end -= 1;
    }
    if end == name.len() {
        return 1;
    }
    let mut n: u128 = 0;
    for &b in &name[end..] {
        n = n * 10 + u128::from(b - b'0');
        if n > usize::MAX as u128 {
            return 1;
        }
    }
    n as usize // silent-ok:上面已拒绝溢出(与 `param_index_of` 的 `parse` 失败同口径)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_state_starts_with_one_frame() {
        let st = ExecState::new(3, 7);
        assert_eq!(st.frames.len(), 1);
        assert_eq!(st.pc, 7);
        assert_eq!(st.script_id, 3);
    }
}
