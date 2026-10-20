//! 任务调度器:单一扁平任务链 + 逻辑帧泵 + TASK 续帧标志。
//!
//! # 语义三层出处
//!
//! ①**手册**
//! - `basic/task.html`:「タスク生成の場合は、指定したラベル位置から始まる
//!   タスク(新たな制御の流れ)を生成します。タスクを生成すると、その瞬間から
//!   タスクが動作し始めます。RETURN 命令に到達すると、タスクは自動的に消滅します。
//!   ※(GOSUB でネストしている場合は別です。GOSUB のネストが無い状態で
//!   RETURN に到達すると消滅します)」——本文件的 `Finish` 判据即此。
//! - `control/task.html`:「タスクＩＤを指定しなかった場合、この命令を実行した
//!   自身のタスクが対象になります」;`Z` = 実行優先度(省略 = 1)。
//! - `control/wait.html`:`TIME`=ミリ秒 / `FRAME`=フレーム。
//!
//! ②**真实脚本**:ERIS 137 个 YSTB 中 `TASK` 10 次、`TASKEND` 2 次、
//! `WAIT` 95 次(其中 `FRAME` 81 / `TIME` 14);`GOSUB` 10,215 次 ——
//! 这说明帧调度主要由主循环 + `WAIT[FRAME=1]` 驱动(见 `es_ERIS.yst`)。
//!
//! ③**引擎取证**(REA 直接反编译 0.495 `yu-ris.exe` sha256 `4f5eefa3…`):
//! - `FUN_00404398`:逐任务取 `task+0x20` 的 pc,**取指后先自增**,再调
//!   `task+0x24` 分派表里的处理体;
//! - 处理体返回值 **0 = 继续,非 0 = 本任务本帧让出**
//!   (`0x4044de: MOV [0x801520], AL`,循环条件 `DAT_00801520 == 0`);
//! - 让出标志另有 `task+9`:`0x40442e CMP byte [ESI+9],0; JNZ next_task`;
//!   `RETURNCODE` 处理体 `0x44c451` 置 1 并返回 -1(文本行尾 = 让出点);
//! - 门控字段 `task+0x18`(帧倒计时)、`task+0x1c`(与时钟比较的期限);
//! - 任务链表为**按优先级降序的有序链表**:插入函数 `FUN_004613c0` 以
//!   `+0x28/+0x2c` 组成 64 位键做 `jl` 前插,相等时插在既有节点之后
//!   (创建序 = 同 Z 的次序);启动时 SYSTEM 任务以 `+0x28 = 0x64` 插入
//!   (`FUN_004621d0` @0x4623f4)。
//! - `TASK` 处理器置续帧标志(`DAT_00801944`):一次 vsync 内连跑逻辑帧。
//!
//! # 工程形态
//!
//! 参考 `<repo-old>/crates/yuris-vm/src/task.rs`(用户授权的算法采用):
//! 单一扁平链、链长入口快照、每逻辑帧每任务最多一步、泵到让出无预算、
//! `TASK` upsert(同 ID 重配置)。**语义结论仍以上面三层为准**;旧线里
//! 「逻辑帧与上屏解耦」「命令泵无预算」两条与手册/引擎证据一致,已采纳。

use std::collections::BTreeMap;

use yu_log::Boundary;

use crate::code;
use crate::diag;

/// 任务名上限(引擎任务 ID 是 CP932 字节串;这里保留原始字节转换后的文本)。
pub type TaskId = String;

/// 引擎任务记录的可观测投影。
///
/// 泵执行期间调度器由 `Vm::tick` 以 `mem::take` 借出,处理器**不能**直接读
/// `Scheduler`;所以每步之前把该投影同步给执行器,`TASKINFO` 读它
/// (对应引擎从任务链表取记录:REA `0x4523fc` 的 `TASKINFO`)。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TaskView {
    pub id: TaskId,
    pub script_id: u32,
    pub pc: u32,
    pub enabled: bool,
    pub priority: i64,
    pub finished: bool,
    pub main: bool,
}

/// 一条命令的执行结果(引擎处理体返回值语义,§3.2b)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepOutcome {
    /// 返回 0:继续泵下一条。
    Continue,
    /// 返回非零:本任务本帧让出。
    Yield,
    /// 文本行尾让出(`RETURNCODE`):置 `task+9`,直到推进输入出现才恢复
    /// (REA 0x44c451 / 0x4046fc)。与 [`StepOutcome::Yield`] 的区别是恢复
    /// 条件不同(输入推进 vs `+0x18`/`+0x1c` 计时)。
    TextYield,
    /// 任务体走到 END / 基帧 RETURN:任务结束(手册 `basic/task.html`)。
    Finish,
}

/// 处理器产生的调度副作用(引擎:TASK/TASKEND 处理器直接写全局)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    /// `TASK` → 置续帧标志(§3.2b `0x450c55`:写 `0x801944`),本 vsync 内再跑一个逻辑帧。
    RequestExtraFrame,
    /// `TASK` 建任务/重配置(同 ID upsert,手册「タスクIDを指定しなかった場合…」)。
    SpawnTask(TaskSeed),
    /// `TASK[E=0/1]`:只改启用位。
    SetEnabled(TaskId, bool),
    /// `TASKEND[ID]`:按 ID 结束。目标不是当前任务时由调度器立即摘链
    /// (`0x452364` 链表摘除)并回调执行器丢弃执行位。
    FinishTask(TaskId),
}

/// 新任务种子。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskSeed {
    /// 任务 ID(脚本 `ID` 槽;缺省 = 执行者自身,见手册)。
    pub id: TaskId,
    /// 入口脚本号。
    pub script_id: u32,
    /// 入口组号。
    pub pc: u32,
    /// `E` 槽:0 = 不参与调度(默认 1)。
    pub enabled: bool,
    /// 优先级 `Z`(手册:省略 = 1;仅生成时有效)。
    pub priority: i64,
    /// 是否主线(启动链建的系统任务,不是 `TASK` 产物)。
    pub main: bool,
}

impl TaskSeed {
    pub fn main(script_id: u32, pc: u32) -> Self {
        Self {
            id: MAIN_TASK_ID.to_string(),
            script_id,
            pc,
            enabled: true,
            // 手册 `basic/task.html`:「起動時に自動的に生成されるシステムタスクは
            // デフォルトで「100」」;REA `FUN_004621d0` @0x4623f4 以 `+0x28 = 0x64`
            // 插入 SYSTEM 任务,实证同一结论。
            priority: 100,
            main: true,
        }
    }
}

/// 主线(系统)任务的脚本可见 ID。
///
/// 手册 `basic/task.html`:「起動時に生成されるタスクをシステムタスクと呼び、
/// 「SYSTEM」というIDが名づけられています」。引擎启动链 `FUN_00465384`
/// (`0x4652f0: MOV EDX, 0x807b00` = "SYSTEM")按该名字建任务,后续
/// `TASK[ID="SYSTEM" …]`/`TASKINFO[ID="SYSTEM" …]` 都按同一张名字表命中。
/// 因此内核的链内 ID 直接用 `SYSTEM`——脚本可见标识与内部标识必须是同一个,
/// 否则两套行为(任务书 E R1)。
pub const MAIN_TASK_ID: &str = "SYSTEM";

/// 任务节点(引擎任务记录字段:`+0x18` 帧倒计时 / `+0x1c` 时钟期限 /
/// `+0x20` pc / `+0x3c` 脚本号)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskNode {
    pub id: TaskId,
    pub main: bool,
    pub script_id: u32,
    pub pc: u32,
    /// `+0x18`:FRAME 型等待计数,每逻辑帧访问时递减。
    pub wait_frames: u32,
    /// `+0x1c`:TIME 型绝对期限(毫秒,注入时钟;回绕语义同 u32)。
    pub deadline_ms: Option<u32>,
    /// `+0x15 & +0x5`:启用位。
    pub enabled: bool,
    /// `Z`:优先级(手册:省略 = 1)。
    pub priority: i64,
    /// 任务体已走完 / 已被 `TASKEND` 摘链(EXIST 立即为 0)。
    pub finished: bool,
    /// 文本等待标志(引擎 `task+9`):`RETURNCODE` 置位 ⇒ 该任务本 vsync 不再
    /// 被步进,直到推进输入(MOUSE_L/ENTER/CTRL/手柄)出现(REA `0x4046fc`)。
    pub text_rest: bool,
    /// 上次被步进的逻辑帧号(「每逻辑帧每任务最多一步」的载体)。
    pub last_frame: u64,
    /// 创建序号:同 Z 的 tie-break(引擎有序链表在等键时插在既有节点之后)。
    pub seq: u64,
    /// 累计步进次数(诊断)。
    pub steps: u64,
}

impl TaskNode {
    fn from_seed(seed: TaskSeed, seq: u64) -> Self {
        Self {
            id: seed.id,
            main: seed.main,
            script_id: seed.script_id,
            pc: seed.pc,
            wait_frames: 0,
            deadline_ms: None,
            enabled: seed.enabled,
            priority: seed.priority,
            finished: false,
            text_rest: false,
            last_frame: 0,
            seq,
            steps: 0,
        }
    }

    /// 重配置(同 ID `TASK`):换入口、清等待、恢复运行;`steps`/`seq` 保留。
    ///
    /// 引擎 `FUN_0045070x` 对既有任务同样复用任务记录(改 pc/脚本号/重载
    /// 脚本表),再经 `FUN_0042f7bc` 摘链按新 Z 重新插入。
    fn reconfigure(&mut self, seed: TaskSeed) {
        self.script_id = seed.script_id;
        self.pc = seed.pc;
        self.wait_frames = 0;
        self.deadline_ms = None;
        self.enabled = seed.enabled;
        self.priority = seed.priority;
        self.finished = false;
        self.text_rest = false;
    }
}

/// 一次 vsync 链的量化报告。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChainReport {
    /// 本链跑掉的逻辑帧数(含 TASK 续帧)。
    pub logical_frames: u32,
    pub tasks_stepped: u64,
    pub commands_pumped: u64,
    /// 执行器报错(官方错误路径未取证 → 让出 + 结束 + 记录;DIV-RT-005)。
    pub errors: Vec<String>,
    pub main_finished: bool,
}

/// 命令执行器抽象。由 `Vm` 实现(分派 + 求值 + 事件发射)。
pub trait TaskExecutor {
    /// 逻辑帧开始(帧号已推进)。执行器据此重置帧内状态、发帧边界事件。
    fn begin_logical_frame(&mut self, _frame: u64) {}
    /// 逻辑帧结束(所有任务已让出/帧内命令已泵完)。architecture §3/A3:
    /// 一帧事件以 `FrameStart` 开始、`FrameEnd` 结束。
    fn end_logical_frame(&mut self, _frame: u64) {}
    /// 泵一条命令。`task.pc` 由执行器在返回前同步。
    fn step_command(&mut self, task: &mut TaskNode) -> Result<StepOutcome, String>;
    /// 同步本步可见的任务链投影(`TASKINFO` 的数据源)。
    fn sync_task_view(&mut self, _view: &[TaskView]) {}
    /// 取走本条命令产生的调度副作用。
    fn drain_effects(&mut self) -> Vec<Effect> {
        Vec::new()
    }
    /// 某任务被 `TASKEND[ID=…]` 摘链(非自身路径):丢弃它的执行位并落事件。
    /// 引擎 `0x452364` 只摘链;执行位回收由内核侧完成,但可观测点相同
    /// (之后 `TASKINFO EXIST` = 0、同 ID 重建从新标签开始)。
    fn task_removed(&mut self, _id: &str) {}
    /// 本逻辑帧是否出现「推进文本」输入(引擎 `0x4046fc` 的 5 个按钮:
    /// `0x5b48ac..` = MOUSE_L(1) / ENTER(13) / PAD(0x3FF) / PAD(0x3FF) /
    /// CTRL(17),任一计数器为正 ⇒ 清 `task+9` 让该任务恢复)。
    fn text_advance_requested(&self) -> bool {
        false
    }
}

/// 逻辑帧驱动器。
pub struct Scheduler {
    /// 按优先级**降序**排列的有序表(引擎 `FUN_004613c0` 的有序链表投影);
    /// 同 Z 按创建序(引擎等键插在既有节点之后)。
    nodes: Vec<TaskNode>,
    frame: u64,
    next_seq: u64,
    /// `DAT_00801944` 等价物:本 vsync 是否还要再跑一个逻辑帧。
    extra_frame: bool,
    /// 工程护栏(非引擎常量;`None` = 与引擎同构的无限泵)。
    pump_cap: Option<usize>,
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            frame: 0,
            next_seq: 0,
            extra_frame: false,
            pump_cap: None,
        }
    }

    /// 工程护栏(诊断/失控对拍);引擎本身无每帧泵上限。
    pub fn with_pump_cap(mut self, cap: Option<usize>) -> Self {
        self.pump_cap = cap;
        self
    }

    pub fn spawn(&mut self, seed: TaskSeed) -> usize {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);
        insert_sorted(&mut self.nodes, TaskNode::from_seed(seed, seq))
    }

    /// 同 ID 重配置;不存在则新建。重配置后按新 Z 重新定位(引擎
    /// `FUN_0042f7bc` = 摘链 + `FUN_004613c0` 有序插入)。
    pub fn upsert(&mut self, seed: TaskSeed) -> usize {
        if let Some(i) = self.nodes.iter().position(|n| n.id == seed.id) {
            let mut node = self.nodes.remove(i);
            node.reconfigure(seed);
            insert_sorted(&mut self.nodes, node)
        } else {
            self.spawn(seed)
        }
    }

    /// 立即摘链(引擎 `0x452364`):节点从链中移除,`EXIST` 立即为 0。
    pub fn finish_by_id(&mut self, id: &str) -> bool {
        match self.nodes.iter().position(|n| n.id == id) {
            Some(i) => {
                self.nodes.remove(i);
                true
            }
            None => false,
        }
    }

    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> bool {
        match self.nodes.iter_mut().find(|n| n.id == id) {
            Some(n) => {
                n.enabled = enabled;
                true
            }
            None => false,
        }
    }

    /// 链视图(诊断)。已结束节点在帧尾清除,帧内可见 `finished` 标记。
    pub fn nodes(&self) -> &[TaskNode] {
        &self.nodes
    }

    pub fn node(&self, i: usize) -> Option<&TaskNode> {
        self.nodes.get(i)
    }

    /// 可变节点视图(诊断/测试钩子:注入等待、标记结束)。
    pub fn node_mut(&mut self, id: &str) -> Option<&mut TaskNode> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    /// 按 ID 查任务。已结束(`TASKEND`/`RETURN` 到基帧)的节点视为不存在:
    /// 引擎摘链后名字表已无此任务 ⇒ `TASKINFO EXIST` = 0。
    pub fn find_by_id(&self, id: &str) -> Option<&TaskNode> {
        self.nodes.iter().find(|n| n.id == id && !n.finished)
    }

    pub fn main_node(&self) -> Option<&TaskNode> {
        self.nodes.iter().find(|n| n.main && !n.finished)
    }

    pub fn frame(&self) -> u64 {
        self.frame
    }

    pub fn tick_frame(&mut self, exec: &mut dyn TaskExecutor, now_ms: u32) -> ChainReport {
        let mut report = ChainReport::default();
        loop {
            // 引擎 `0x40413d`:续帧标志在逻辑帧开头清零。
            self.extra_frame = false;
            self.frame = self.frame.wrapping_add(1);
            exec.begin_logical_frame(self.frame);
            self.step_frame(exec, now_ms, &mut report);
            exec.end_logical_frame(self.frame);
            report.logical_frames += 1;
            if !self.extra_frame {
                break;
            }
        }
        // 主线正常走到 END / 基帧 RETURN 也算结束(节点在 retain 前还带着标记)。
        if let Some(m) = self.nodes.iter().find(|n| n.main) {
            report.main_finished |= m.finished;
        }
        self.nodes.retain(|n| !n.finished);
        report
    }

    fn step_frame(
        &mut self,
        exec: &mut dyn TaskExecutor,
        now_ms: u32,
        report: &mut ChainReport,
    ) {
        let stamp = self.frame;
        let mut nodes = std::mem::take(&mut self.nodes);
        // 链长在入口快照(`0x40438d`):步进中新建的节点留给下一逻辑帧
        // (帧内新建收集到 `pending`,帧尾按 Z 有序合并)。
        let end = nodes.len();
        let mut ctx = PumpCtx {
            cap: self.pump_cap,
            extra: false,
            pending: Vec::new(),
            next_seq: self.next_seq,
        };
        let mut visited = 0usize;
        while visited < end {
            // 链表可能因 `TASKEND[ID=…]` 摘链而缩短 ⇒ 每轮重新找"本帧未访问"的
            // 第一个节点(引擎按索引走,摘链后同样按当前链取下一个)。
            let Some(i) = nodes.iter().position(|n| n.last_frame != stamp) else {
                break;
            };
            nodes[i].last_frame = stamp;
            visited += 1;
            if !nodes[i].enabled || nodes[i].finished {
                continue;
            }
            // 文本等待(引擎 `task+9`):未出现推进输入 ⇒ 本 vsync 跳过该任务。
            if nodes[i].text_rest {
                if !exec.text_advance_requested() {
                    continue;
                }
                nodes[i].text_rest = false;
            }
            // FRAME 递减(先递减再判到期 ⇒ FRAME=1 当帧即醒)。
            if nodes[i].wait_frames > 0 {
                nodes[i].wait_frames -= 1;
            }
            let time_resting = match nodes[i].deadline_ms {
                Some(d) if now_ms < d => true,
                Some(_) => {
                    nodes[i].deadline_ms = None;
                    false
                }
                None => false,
            };
            if nodes[i].wait_frames > 0 || time_resting {
                continue;
            }
            let seq = nodes[i].seq;
            pump_node(exec, &mut nodes, seq, &mut ctx, report);
            report.tasks_stepped += 1;
        }
        self.next_seq = ctx.next_seq;
        let extra = ctx.extra;
        let mut pending = std::mem::take(&mut ctx.pending);
        nodes.append(&mut pending);
        sort_nodes(&mut nodes);
        self.nodes = nodes;
        self.extra_frame |= extra;
    }
}

/// 降序按 Z 插入;等键时插在既有节点之后(创建序)。
fn insert_sorted(nodes: &mut Vec<TaskNode>, node: TaskNode) -> usize {    let pos = nodes
        .iter()
        .position(|n| n.priority < node.priority)
        .unwrap_or(nodes.len());
    nodes.insert(pos, node);
    pos
}

fn sort_nodes(nodes: &mut [TaskNode]) {
    nodes.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then_with(|| a.seq.cmp(&b.seq))
    });
}

/// 任务链 → 处理器可见投影(帧内新建节点已在链上,引擎语义如此)。
fn view_of(nodes: &[TaskNode], pending: &[TaskNode]) -> Vec<TaskView> {
    nodes
        .iter()
        .chain(pending.iter())
        .map(|n| TaskView {
            id: n.id.clone(),
            script_id: n.script_id,
            pc: n.pc,
            enabled: n.enabled,
            priority: n.priority,
            finished: n.finished,
            main: n.main,
        })
        .collect()
}

/// 一帧泵的共享上下文(避免 `pump_node` 参数爆炸)。
struct PumpCtx {
    cap: Option<usize>,
    extra: bool,
    pending: Vec<TaskNode>,
    next_seq: u64,
}

fn pump_node(
    exec: &mut dyn TaskExecutor,
    nodes: &mut Vec<TaskNode>,
    seq: u64,
    ctx: &mut PumpCtx,
    report: &mut ChainReport,
) {
    let cap = ctx.cap;
    let mut pumped = 0usize;
    let mut finished = false;
    // 任务链投影只在链变化时同步(每条命令同步会克隆整条链,热路径不可接受)。
    exec.sync_task_view(&view_of(nodes, &ctx.pending));
    loop {
        if let Some(cap) = cap {
            if pumped >= cap {
                break;
            }
        }
        // 副作用可能摘掉链中的其它节点(索引前移)⇒ 每轮按 `seq` 重新定位当前节点。
        let Some(cur) = nodes.iter().position(|n| n.seq == seq) else {
            break;
        };
        let out = exec.step_command(&mut nodes[cur]);
        let mut chain_changed = false;
        for effect in exec.drain_effects() {
            match effect {
                Effect::RequestExtraFrame => ctx.extra = true,
                Effect::SpawnTask(seed) => {
                    if let Some(n) = nodes.iter_mut().find(|n| n.id == seed.id) {
                        n.reconfigure(seed);
                    } else if let Some(n) = ctx.pending.iter_mut().find(|n| n.id == seed.id) {
                        n.reconfigure(seed);
                    } else {
                        let s = ctx.next_seq;
                        ctx.next_seq = ctx.next_seq.wrapping_add(1);
                        ctx.pending.push(TaskNode::from_seed(seed, s));
                    }
                    chain_changed = true;
                }
                Effect::SetEnabled(id, on) => {
                    if let Some(n) = nodes.iter_mut().find(|n| n.id == id) {
                        n.enabled = on;
                    }
                }
                Effect::FinishTask(id) => {
                    // 引擎 `TASKEND` 处理体对非自身目标:查找 → `0x452364` 摘链
                    // → 返回 0(自身目标返回 -1,由处理器直接 Finish)。这里立即
                    // 摘链,使 `TASKINFO EXIST` 与后续 `TASK[#=…]` 都看到"不存在"。
                    if let Some(pos) = nodes.iter().position(|n| n.id == id && !n.finished) {
                        let removed = nodes.remove(pos);
                        exec.task_removed(&id);
                        if removed.seq == seq {
                            finished = true;
                        }
                    } else if let Some(pos) = ctx.pending.iter().position(|n| n.id == id) {
                        ctx.pending.remove(pos);
                        exec.task_removed(&id);
                    }
                    chain_changed = true;
                }
            }
        }
        if chain_changed {
            exec.sync_task_view(&view_of(nodes, &ctx.pending));
        }
        if finished {
            break;
        }
        let Some(cur) = nodes.iter().position(|n| n.seq == seq) else {
            break;
        };
        match out {
            Ok(StepOutcome::Continue) => pumped += 1,
            Ok(StepOutcome::Yield | StepOutcome::TextYield) => {
                pumped += 1;
                nodes[cur].steps += 1;
                if matches!(out, Ok(StepOutcome::TextYield)) {
                    nodes[cur].text_rest = true;
                }
                break;
            }
            Ok(StepOutcome::Finish) => {
                pumped += 1;
                nodes[cur].finished = true;
                nodes[cur].steps += 1;
                break;
            }
            Err(e) => {
                // 官方错误路径未取证 → 让出 + 结束 + 记录(DIV-RT-005)。
                let (id, script_id, pc, main) = (
                    nodes[cur].id.clone(),
                    nodes[cur].script_id,
                    nodes[cur].pc,
                    nodes[cur].main,
                );
                report.errors.push(format!("task {id} s{script_id} pc{pc}: {e}"));
                if main {
                    report.main_finished = true;
                }
                nodes[cur].finished = true;
                nodes[cur].steps += 1;
                break;
            }
        }
    }
    if finished {
        if let Some(cur) = nodes.iter().position(|n| n.seq == seq) {
            nodes[cur].finished = true;
        }
    }
    report.commands_pumped += pumped as u64;
}

/// 便捷:构造任务 ID 的固定顺序表(诊断用)。
pub fn task_ids(nodes: &[TaskNode]) -> BTreeMap<&str, (u32, u32)> {
    nodes
        .iter()
        .map(|n| (n.id.as_str(), (n.script_id, n.pc)))
        .collect()
}

/// 任务相关诊断(嵌套上限等)。手册:`IF` 64 层、`LOOP` 64 层、`GOSUB` 64 层。
pub fn nesting_error(what: &str, depth: usize, limit: usize) -> yu_log::Diag {
    diag::fatal(
        code::rt::NESTING,
        Boundary::Script,
        format!("{what} 嵌套超过上限:当前 {depth},引擎上限 {limit}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, VecDeque};

    struct MockExec {
        scripts: HashMap<String, VecDeque<StepOutcome>>,
        effects_on_first: HashMap<String, Vec<Effect>>,
        first_seen: HashMap<String, usize>,
        calls: Vec<String>,
    }

    impl MockExec {
        fn new() -> Self {
            Self {
                scripts: HashMap::new(),
                effects_on_first: HashMap::new(),
                first_seen: HashMap::new(),
                calls: Vec::new(),
            }
        }
        fn script(mut self, id: &str, outs: Vec<StepOutcome>) -> Self {
            self.scripts.insert(id.to_string(), outs.into());
            self
        }
        fn effects(mut self, id: &str, e: Vec<Effect>) -> Self {
            self.effects_on_first.insert(id.to_string(), e);
            self
        }
        fn count(&self, id: &str) -> usize {
            self.calls.iter().filter(|c| c.as_str() == id).count()
        }
    }

    impl TaskExecutor for MockExec {
        fn step_command(&mut self, task: &mut TaskNode) -> Result<StepOutcome, String> {
            self.calls.push(task.id.clone());
            task.pc += 1;
            Ok(self
                .scripts
                .get_mut(&task.id)
                .and_then(|q| q.pop_front())
                .unwrap_or(StepOutcome::Yield))
        }
        fn drain_effects(&mut self) -> Vec<Effect> {
            let Some(id) = self.calls.last().cloned() else {
                return Vec::new();
            };
            let n = self.first_seen.entry(id.clone()).or_insert(0);
            *n += 1;
            if *n == 1 {
                self.effects_on_first.remove(&id).unwrap_or_default()
            } else {
                Vec::new()
            }
        }
        fn task_removed(&mut self, _id: &str) {}
    }

    fn seed(id: &str) -> TaskSeed {
        TaskSeed {
            id: id.to_string(),
            script_id: 0,
            pc: 0,
            enabled: true,
            priority: 1,
            main: false,
        }
    }

    fn seed_z(id: &str, priority: i64) -> TaskSeed {
        TaskSeed {
            priority,
            ..seed(id)
        }
    }

    #[test]
    fn frame_wait_decrements_then_wakes() {
        let mut s = Scheduler::new();
        s.spawn(seed("a"));
        s.node_mut("a").unwrap().wait_frames = 2;
        let mut exec = MockExec::new().script("a", vec![StepOutcome::Yield]);
        s.tick_frame(&mut exec, 0);
        assert_eq!(exec.count("a"), 0, "FRAME=2 第一帧休眠");
        s.tick_frame(&mut exec, 0);
        assert_eq!(exec.count("a"), 1, "第二帧醒");
    }

    #[test]
    fn time_deadline_gates_until_due() {
        let mut s = Scheduler::new();
        s.spawn(seed("a"));
        s.node_mut("a").unwrap().deadline_ms = Some(100);
        let mut exec = MockExec::new().script("a", vec![StepOutcome::Yield]);
        s.tick_frame(&mut exec, 99);
        assert_eq!(exec.count("a"), 0);
        s.tick_frame(&mut exec, 100);
        assert_eq!(exec.count("a"), 1);
    }

    #[test]
    fn task_extends_chain_and_new_task_runs_next_logical_frame() {
        let mut s = Scheduler::new();
        s.spawn(seed("a"));
        let mut exec = MockExec::new().script("a", vec![StepOutcome::Yield]).effects(
            "a",
            vec![Effect::RequestExtraFrame, Effect::SpawnTask(seed("b"))],
        );
        let r = s.tick_frame(&mut exec, 0);
        assert_eq!(r.logical_frames, 2);
        assert_eq!(exec.count("b"), 1);
    }

    #[test]
    fn one_step_per_logical_frame() {
        let mut s = Scheduler::new();
        s.spawn(seed("a"));
        let mut exec = MockExec::new().script("a", vec![StepOutcome::Yield]);
        s.tick_frame(&mut exec, 0);
        s.tick_frame(&mut exec, 0);
        assert_eq!(exec.count("a"), 2);
    }

    #[test]
    fn finished_and_disabled_are_skipped() {
        let mut s = Scheduler::new();
        s.spawn(seed("fin"));
        s.node_mut("fin").unwrap().finished = true;
        s.spawn(seed("dis"));
        s.node_mut("dis").unwrap().enabled = false;
        let mut exec = MockExec::new();
        s.tick_frame(&mut exec, 0);
        assert_eq!(exec.count("fin"), 0);
        assert_eq!(exec.count("dis"), 0);
    }

    #[test]
    fn taskend_effect_unlinks_by_id_immediately() {
        let mut s = Scheduler::new();
        s.spawn(seed("a"));
        s.spawn(seed("b"));
        let mut exec = MockExec::new().script("a", vec![StepOutcome::Yield]).effects(
            "a",
            vec![Effect::FinishTask("b".to_string())],
        );
        s.tick_frame(&mut exec, 0);
        assert!(
            s.find_by_id("b").is_none(),
            "TASKEND 后任务从链中移除(EXIST 立即为 0)"
        );
        assert_eq!(exec.count("b"), 0);
    }

    #[test]
    fn taskend_of_self_stops_the_current_pump() {
        // 回归(R2):`TASKEND[]` 之后同一帧的下一条命令不得执行。
        let mut s = Scheduler::new();
        s.spawn(seed("a"));
        let mut exec = MockExec::new()
            .script("a", vec![StepOutcome::Continue, StepOutcome::Continue])
            .effects("a", vec![Effect::FinishTask("a".to_string())]);
        s.tick_frame(&mut exec, 0);
        assert_eq!(exec.count("a"), 1, "TASKEND 后立即停止泵,不再执行下一条");
    }

    #[test]
    fn same_id_rebuild_starts_at_new_entry_after_taskend() {
        let mut s = Scheduler::new();
        s.spawn(seed("a"));
        s.node_mut("a").unwrap().pc = 99;
        let mut exec = MockExec::new().script("a", vec![StepOutcome::Yield]).effects(
            "a",
            vec![Effect::FinishTask("a".to_string())],
        );
        s.tick_frame(&mut exec, 0);
        assert!(s.find_by_id("a").is_none());
        let mut again = seed("a");
        again.pc = 7;
        s.spawn(again);
        assert_eq!(s.find_by_id("a").unwrap().pc, 7, "重建不复用旧 pc");
    }

    #[test]
    fn upsert_reconfigures_in_place() {
        let mut s = Scheduler::new();
        s.spawn(seed("a"));
        {
            let n = s.node_mut("a").unwrap();
            n.wait_frames = 5;
            n.finished = true;
        }
        let mut again = seed("a");
        again.pc = 42;
        s.upsert(again);
        let n = s.find_by_id("a").unwrap();
        assert_eq!(n.pc, 42);
        assert_eq!(n.wait_frames, 0);
        assert!(!n.finished);
    }

    #[test]
    fn higher_priority_runs_first_and_equal_priority_keeps_creation_order() {
        // 手册 `basic/task.html`:「優先度の高い順から実行されます」;
        // 引擎 `FUN_004613c0` 以 +0x28 降序有序插入,等键插在既有节点之后。
        let mut s = Scheduler::new();
        s.spawn(seed_z("low", 1));
        s.spawn(seed_z("high", 300));
        s.spawn(seed_z("mid_a", 100));
        s.spawn(seed_z("mid_b", 100));
        let order: Vec<&str> = s.nodes().iter().map(|n| n.id.as_str()).collect();
        assert_eq!(order, vec!["high", "mid_a", "mid_b", "low"]);
        let mut exec = MockExec::new();
        s.tick_frame(&mut exec, 0);
        assert_eq!(
            exec.calls,
            vec!["high", "mid_a", "mid_b", "low"],
            "泵顺序 = 优先级降序"
        );
    }

    #[test]
    fn main_task_defaults_to_priority_100_and_id_system() {
        // 手册 `basic/task.html`:系统任务 ID = "SYSTEM",默认 Z = 100。
        let seed = TaskSeed::main(0, 0);
        assert_eq!(seed.id, "SYSTEM");
        assert_eq!(seed.priority, 100);
    }

    #[test]
    fn pump_without_budget() {
        let mut s = Scheduler::new();
        s.spawn(seed("a"));
        let mut v = vec![StepOutcome::Continue; 5000];
        v.push(StepOutcome::Yield);
        let mut exec = MockExec::new().script("a", v);
        let r = s.tick_frame(&mut exec, 0);
        assert_eq!(r.commands_pumped, 5001);
    }
}
