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

use yu_log::{Boundary, Diag, LogSink, MemorySink};

use crate::code;
use crate::diag;
use crate::event::VmEvent;
use crate::expr::{Evaluator, LValue, VarAccess};
use crate::input::{InputBinding, InputState, InputTimeline};
use crate::tables::{TableSet, WindowRec};
use crate::task::{ChainReport, Effect, Scheduler, StepOutcome, TaskExecutor, TaskNode, TaskSeed, TaskView, MAIN_TASK_ID};
use crate::value::{Value, VarRef, VarSpace, VariableStore};

pub(crate) mod handlers;
pub use handlers::HandlerKind;

/// 事件队列默认容量(任务书 E R10)。按每帧数千事件的量级留足余量,
/// 同时保证长跑有界;需要完整全量事件流的调用方用 [`Vm::take_events`]/
/// [`Vm::drain_events`] 在帧间取走。
pub const DEFAULT_EVENT_CAPACITY: usize = 1 << 20;

/// 启动配置。
#[derive(Clone, Debug)]
pub struct BootConfig {
    /// 入口标签名(默认 `SYSTEM_START`)。
    pub entry_label: String,
    /// 帧率初值(脚本 `FPS[SET=…]` 可覆盖;手册 `system/fps.html`)。
    pub fps: u32,
    /// 注入时钟起点(毫秒)。
    pub start_ms: u32,
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
    /// 事件流记录开关(默认开;golden/测试/对拍依赖完整事件流)。嵌入宿主
    /// 跑真实游戏时可关:`Command` 事件的窗口预求值是分派热路径的最大开销
    /// (每个窗口求值两遍)。关闭时 `Diag` 事件与挂号计数照常产出。
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
            frame_ms: 16,
            script_key: crate::tables::DEFAULT_SCRIPT_KEY,
            debug_mode: false,
            argv: Vec::new(),
            screen_color: 32,
            paths: HostPaths::default(),
            media_readonly: false,
            os_sound: true,
            dx_mode: 0,
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
pub struct Cmd {
    pub opcode: u8,
    pub name: String,
    pub pc: u32,
    pub windows: Vec<CmdWindow>,
}

impl Default for Cmd {
    fn default() -> Self {
        Self {
            opcode: 0,
            name: String::new(),
            pc: 0,
            windows: Vec::new(),
        }
    }
}

/// 一个窗口 + 其载荷。
pub struct CmdWindow {
    pub rec: WindowRec,
    pub payload: Vec<u8>,
}

impl CmdWindow {
    pub fn is_empty(&self) -> bool {
        self.payload.is_empty()
    }
}

/// 运行核。
pub struct Vm {
    src: Box<dyn yu_vfs::DataSource>,
    /// 编译器表回退源(工程根/SDK);脚本与其余表仍从 `src` 读。
    aux: Option<Box<dyn yu_vfs::DataSource>>,
    tables: TableSet,
    scheduler: Scheduler,
    store: VariableStore,
    exec: BTreeMap<String, ExecState>,
    input: InputState,
    binding: InputBinding,
    /// 图形域同步宿主(可选;安装后负责 INFO 查询与状态同步)。
    graphics_host: Option<Box<dyn crate::event::GraphicsHost>>,
    events: Vec<VmEvent>,
    diags: MemorySink,
    pending_effects: Vec<Effect>,
    script_errors: Vec<String>,
    frame: u64,
    now_ms: u32,
    start_ms: u32,
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
    record_events: bool,
    /// opcode → 处理器类别表(启动期按工程 yscm 名字解析)。
    handler_of: Vec<crate::vm::handlers::HandlerKind>,
    /// 命令视图复用缓冲(热路径零分配;见 `fill_cmd`)。
    cmd_scratch: Cmd,
    /// 表达式求值复用栈(热路径零分配)。
    eval_scratch: Vec<crate::expr::Slot>,
    /// 表达式解码复用缓冲(热路径零分配)。
    decode_scratch: Vec<crate::expr::RawInstr>,
    texts: BTreeMap<String, Vec<u8>>,
    /// 显示文本层状态(`$_TEXT`/`$_TEXT_FLAG`/`TEXTINFO` 的数据源;
    /// 引擎 `[0x801800]` 的已取证字段,见 [`crate::text`])。
    text: crate::text::TextState,
    /// `TEXT[ID=...]`:后续文本事件的目标文本层 ID(手册 `text/text.html`)。
    /// 事件字段是 `String`;非 UTF-8 的 CP932 ID 会显式报告并保持为空。
    text_layer_id: String,
    /// `@_MENUCODE`(手册:最後に選択されたメニュー項目の CODE 値;○ 可赋值)。
    menu_code: i64,
    /// `@_WINDOW_EXEC_MAX`/`_WINDOW_EXEC_CLOSE`(手册:最大化/閉じるボタン
    /// 押下状態 0/1;○ 可赋值)。宿主窗口事件可经 setter 推入。
    window_exec_max: i64,
    window_exec_close: i64,
    executed: u64,
    /// 每条命令(opcode)的执行次数。**不受 `record_events` 影响**:
    /// 覆盖普查要在关闭事件流的热路径上拿到逐命令频度(任务书 M-1)。
    /// 下标 = 工程 `ysc.ybn` 的命令序号;长度在启动期固定。
    command_hits: Vec<u64>,
    unimplemented_cmds: BTreeMap<String, u64>,
    unimplemented_reported: std::collections::BTreeSet<String>,
    /// 事件队列容量上限(任务书 E R10:长跑不得无界增长)。
    event_capacity: usize,
    /// 因容量上限被丢弃的最旧事件数(可观测,不静默)。
    events_dropped: u64,
    /// 当前正在执行的命令位置(诊断定位用;任务书 E R11)。
    /// `report()` 在 `Diag.where_ == Loc::None` 时补进 `Loc::Script`。
    /// 用可复用 String(而非 `Option<String>`)避免每条命令分配。
    cur_script: u32,
    cur_pc: u32,
    cur_cmd: String,
    cur_task: String,
    /// 本步可见的任务链投影(`TASKINFO` 的数据源;泵期间调度器被借出)。
    task_view: Vec<TaskView>,
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
        let label = tables
            .boot
            .labels
            .get(&cfg.entry_label)
            .ok_or_else(|| {
                diag::fatal(
                    code::rt::LABEL,
                    Boundary::Script,
                    format!(
                        "入口标签 `{}` 不在工程 ysl.ybn(拒绝启动)",
                        cfg.entry_label
                    ),
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

        let mut vm = Vm {
            src,
            aux,
            tables,
            scheduler: Scheduler::new(),
            store: VariableStore::new(),
            exec: BTreeMap::new(),
            input: InputState::new(),
            binding,
            graphics_host: None,
            events: Vec::new(),
            diags: MemorySink::new(),
            pending_effects: Vec::new(),
            script_errors: Vec::new(),
            frame: 0,
            now_ms: cfg.start_ms,
            start_ms: cfg.start_ms,
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
            record_events: cfg.record_events,
            handler_of,
            cmd_scratch: Cmd::default(),
            eval_scratch: Vec::new(),
            decode_scratch: Vec::new(),
            texts: BTreeMap::new(),
            text: crate::text::TextState::default(),
            text_layer_id: String::new(),
            menu_code: 0,
            window_exec_max: 0,
            window_exec_close: 0,
            executed: 0,
            command_hits,
            unimplemented_cmds: BTreeMap::new(),
            unimplemented_reported: std::collections::BTreeSet::new(),
            event_capacity: DEFAULT_EVENT_CAPACITY,
            events_dropped: 0,
            cur_script: script_id,
            cur_pc: pc,
            cur_cmd: String::new(),
            cur_task: MAIN_TASK_ID.to_string(),
            task_view: Vec::new(),
        };
        vm.apply_global_initial_values();
        vm.apply_script_locals(script_id);

        vm.exec
            .insert(MAIN_TASK_ID.to_string(), ExecState::new(script_id, pc));
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
                         0292/0466/0479/0495)。命令表/参数名仍来自工程 ysbin/ysc.ybn,\
                         无版本兜底。",
                        vm.tables.boot.sysvars.source.describe()
                    ),
                )
                .at_table("YSCom.ycd"),
            );
        }
        Ok(vm)
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
            let init = r.init_value().unwrap_or_else(|| ty.default_value());
            self.store
                .declare(VarRef::new(VarSpace::for_elem(ty), r.var_id), ty, &dims, &init);
        }
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
            let init = r.init_value().unwrap_or_else(|| ty.default_value());
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
    pub(crate) fn exec_of(&self, id: &str) -> Option<&ExecState> {
        self.exec.get(id)
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
            format!("ysc: v{} ({} 命令)", self.tables.boot.yscm.version, self.tables.boot.yscm.len()),
            format!("ysl: v{} ({} 标签)", self.tables.boot.labels.version, self.tables.boot.labels.len()),
            format!("ysv: v{} ({} 记录)", self.tables.boot.vars.version, self.tables.boot.vars.records.len()),
            format!("yst_list: v{} ({} 条)", self.tables.boot.scripts.version, self.tables.boot.scripts.len()),
            format!("yscfg: v{} ({}x{})", self.tables.boot.config.version,
                self.tables.boot.config.width, self.tables.boot.config.height),
            format!("sysvars: {} 项 <- {}", self.tables.boot.sysvars.len(), self.tables.boot.sysvars.source.describe()),
        ];
        if let Some(ycd) = &self.tables.boot.ycd {
            out.push(format!("ycd: v{} ({} 命令)", ycd.version, ycd.commands.len()));
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

    /// `@_MENUCODE` 读取(菜单域宿主选中项后应经 [`Vm::set_menu_code`] 推入)。
    pub(crate) fn menu_code(&self) -> i64 {
        self.menu_code
    }

    /// `@_MENUCODE` 写入(手册 ○;菜单域宿主)。
    pub fn set_menu_code(&mut self, code: i64) {
        self.menu_code = code;
    }

    /// `@_WINDOW_EXEC_MAX/CLOSE` 读写(手册 ○;宿主窗口事件推入)。
    pub(crate) fn window_exec(&self, which: &str) -> i64 {
        match which {
            "MAX" => self.window_exec_max,
            _ => self.window_exec_close,
        }
    }

    pub(crate) fn set_window_exec(&mut self, which: &str, v: i64) {
        match which {
            "MAX" => self.window_exec_max = v,
            _ => self.window_exec_close = v,
        }
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

    /// opcode 对应的处理器类别(启动期按工程 `ysc.ybn` 解析;覆盖普查
    /// 用它区分「已实现 / 域事件 / 无处理器」,不靠名字猜测)。
    pub fn handler_kind(&self, opcode: u8) -> Option<HandlerKind> {
        self.handler_of.get(opcode as usize).copied()
    }

    // ---------------------------------------------------------- 帧推进

    /// 推进一个 vsync 链(输入 → 调度器 → 推进注入时钟)。
    pub fn tick(&mut self, timeline: &InputTimeline) -> ChainReport {
        let next_frame = self.scheduler.frame().wrapping_add(1);
        self.input.advance(next_frame, timeline);
        self.frame = next_frame;
        let now = self.now_ms;
        let mut sched = std::mem::take(&mut self.scheduler);
        let report = sched.tick_frame(self, now);
        self.scheduler = sched;
        let step = if self.fps > 0 { 1000 / self.fps } else { self.frame_ms };
        self.now_ms = self.now_ms.wrapping_add(step);
        report
    }

    /// 时钟严格固定的单帧(对拍/测试)。
    pub fn tick_at(&mut self, now_ms: u32, timeline: &InputTimeline) -> ChainReport {
        let next_frame = self.scheduler.frame().wrapping_add(1);
        self.input.advance(next_frame, timeline);
        self.now_ms = now_ms;
        let mut sched = std::mem::take(&mut self.scheduler);
        let report = sched.tick_frame(self, now_ms);
        self.scheduler = sched;
        report
    }

    // ---------------------------------------------------------- 诊断

    /// 事件入队。事件流是**有界**的(任务书 E R10):超过容量时丢最旧事件并
    /// 记账,长跑不会无界增长。容量可通过 [`Vm::with_event_capacity`] 调整。
    pub(crate) fn emit(&mut self, ev: VmEvent) {
        // 图形宿主必须**同步**看到状态事件:同一帧 `CG[...]` 之后的 `CGINFO`
        // 才能读到最新图层。仅转发宿主关心的类别,避免 Command 热路径开销。
        let host_relevant = matches!(
            &ev,
            VmEvent::FrameStart { .. }
                | VmEvent::FrameEnd { .. }
                | VmEvent::Text { .. }
                | VmEvent::DomainCall {
                    domain: crate::event::Domain::Graphics | crate::event::Domain::Window,
                    ..
                }
        );
        if host_relevant {
            if let Some(host) = self.graphics_host.as_mut() {
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

    /// 给诊断补脚本位置(仅当调用点没给 `Loc`)。
    pub(crate) fn locate(&self, d: Diag) -> Diag {
        if !matches!(d.where_, yu_log::Loc::None) {
            return d;
        }
        let file = self.script_name(self.cur_script);
        let task = (!self.cur_task.is_empty()).then(|| self.cur_task.clone());
        let cmd = (!self.cur_cmd.is_empty()).then(|| self.cur_cmd.clone());
        diag::at_script_task(d, file, task, self.cur_pc, cmd)
    }

    /// 未实现命令挂号(每个命令名只进事件流一次,计数全部)。
    pub(crate) fn report_unimplemented(&mut self, name: &str, detail: String) {
        *self.unimplemented_cmds.entry(name.to_string()).or_insert(0) += 1;
        if self.unimplemented_reported.insert(name.to_string()) {
            self.report(diag::cmd_unimplemented(name, detail));
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
                format!("s{} pc={} 越界(脚本 {} 组)", st.script_id, st.pc, script.group_count()),
            ));
        };
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
            cmd.windows.push(CmdWindow {
                rec: *rec,
                payload: payload.to_vec(),
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
    pub(crate) fn canonical_var(&self, var: VarRef) -> VarRef {
        if !var.space.is_legacy() {
            return var;
        }
        // id 唯一 ⇒ 直接查两种存储空间(不走 `var_shape`,避免递归)。
        let ty = if self.store.shape(VarRef::new(VarSpace::At, var.id)).is_some() {
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
            return self
                .store
                .shape(var)
                .map(|(ty, dims)| (ty, dims.to_vec()));
        }
        let canonical = self.canonical_var(var);
        self.store
            .shape(canonical)
            .map(|(ty, dims)| (ty, dims.to_vec()))
    }

    pub(crate) fn eval_value(
        &mut self,
        st: &ExecState,
        payload: &[u8],
    ) -> Result<Value, Diag> {
        let mut stack = std::mem::take(&mut self.eval_scratch);
        let mut instrs = std::mem::take(&mut self.decode_scratch);
        let mut access = VmAccess { vm: self, st };
        let r = Evaluator::new(&mut access).eval_value_pooled(payload, &mut stack, &mut instrs);
        self.decode_scratch = instrs;
        self.eval_scratch = stack;
        r
    }

    pub(crate) fn eval_lvalue(
        &self,
        st: &ExecState,
        payload: &[u8],
    ) -> Result<LValue, Diag> {
        let mut access = VmAccess { vm: self, st };
        Evaluator::new(&mut access).eval_lvalue(payload)
    }

    /// 参数声明名(工程 ysc;`None` = 该序号未声明)。
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

    /// 编译器表回退源是否在位(诊断)。
    pub fn has_aux_source(&self) -> bool {
        self.aux.is_some()
    }

    /// 标签按名解析(字节精确)。
    pub(crate) fn find_label(&self, name: &[u8]) -> Option<crate::tables::Label> {
        self.tables.boot.labels.get_bytes(name).cloned()
    }

    pub(crate) fn record_command(&mut self, st: &ExecState, cmd: &Cmd, args: Vec<crate::event::ArgValue>) {
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
        // 未知变量空间(实测商业语料 s91 有 5 处 `` ` `` 前缀 0x60,
        // 手册/引擎均无对应语义)⇒ 挂号并继续(DIV-RT-017)。
        // ERIS 旧式空间(`@G`/`@L`/`$G`/`$L`)按 id 的声明类型归一化(DIV-RT-017)。
        let var = self.vm.canonical_var(var);
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
        self.emit(VmEvent::FrameStart { frame });
    }

    fn end_logical_frame(&mut self, frame: u64) {
        // architecture §3 / A3:一帧事件以 `FrameStart` 开始、`FrameEnd` 结束。
        self.emit(VmEvent::FrameEnd { frame });
    }

    fn step_command(&mut self, task: &mut TaskNode) -> Result<StepOutcome, String> {
        let id = task.id.clone();
        let Some(mut st) = self.exec.remove(&id) else {
            return Ok(StepOutcome::Finish);
        };
        let res = self.step_one(&mut st, task);
        // 同步任务记录(pc/脚本号;引擎 `task+0x20/+0x3c`)。
        task.pc = st.pc;
        task.script_id = st.script_id;

        match res {
            Ok(out) => {
                let finishing = matches!(out, StepOutcome::Finish);
                if !finishing {
                    self.exec.insert(id, st);
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
                    let name = self.cur_cmd.clone();
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
                    self.exec.insert(id, st);
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
        if self.exec.remove(id).is_some() {
            self.emit(VmEvent::TaskFinish {
                frame: self.frame,
                id: id.to_string(),
            });
        }
    }

    fn text_advance_requested(&self) -> bool {
        self.input.text_advance_requested()
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
        let mut cmd = std::mem::take(&mut self.cmd_scratch);
        let filled = self.fill_cmd(st, &mut cmd);
        match filled {
            Ok(()) => {}
            Err(d) => {
                self.cmd_scratch = cmd;
                return Err(d.into());
            }
        }
        // 诊断定位:记住「当前正在执行的命令」(R11)。复用缓冲,零分配。
        self.cur_script = st.script_id;
        self.cur_pc = cmd.pc;
        self.cur_cmd.clear();
        self.cur_cmd.push_str(&cmd.name);
        self.cur_task.clear();
        self.cur_task.push_str(&task.id);
        // 取指后先自增(引擎 §3.2b:先取后加;处理体写 pc 即跳转)。
        st.pc = st.pc.wrapping_add(1);
        let outcome = self.dispatch(st, task, &cmd);
        self.cmd_scratch = cmd;
        outcome
    }
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
