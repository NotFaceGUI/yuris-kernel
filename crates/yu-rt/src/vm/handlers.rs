//! 命令处理器(按 ERIS 语料频度排序实现)。
//!
//! # 频度(官方 YSCom 编译的 137 个 YSTB,36,974 组 / 80,102 窗口全量扫描)
//!
//! | 命令 | 次数 | 本文件 | 三层出处 |
//! |---|---|---|---|
//! | `GOSUB` | 10,215 | ✅ | 手册 `control/gosub.html`;引擎 §3.16 |
//! | `LET` | 8,136 | ✅ | 手册 `variable/let.html`;引擎 §3.10/§3.17 |
//! | `IF` | 3,326 | ✅ | 手册 `control/if.html`;引擎 §3.13 |
//! | `IFEND` | 3,326 | ✅ | 同上 |
//! | `RETURN` | 1,750 | ✅ | 手册 `control/return.html`;引擎 §3.16 |
//! | `INT` | 1,428 | ✅ | 手册 `variable/int.html` |
//! | `IFBLEND` | 887 | ✅ | 手册 `control/if.html`(IF 链) |
//! | `ELSE` | 887 | ✅ | 手册 `control/else.html` |
//! | `S_INT` | 767 | ✅ | 手册 `variable/s_int.html` |
//! | `LOOP` | 522 | ✅ | 手册 `control/loop.html`;引擎 §3.13 |
//! | `STR` | 431 | ✅ | 手册 `variable/str.html` |
//! | `RETURNCODE` | 396 | ✅ | 行尾改行标记(见 `op_returncode`;三层齐) |
//! | `WORD` | 464 | ✅ | 文本段输出(见 `op_word`;三层齐) |
//! | `F_INT` | 337 | ⛔ | **手册无页** ⇒ 不实现(DIV-RT-011) |
//! | `VARACT` | 310 | ✅部分 | 手册 `variable/varact.html`;引擎 §3.26 |
//! | `SAVE`/`LOAD` | 290/289 | ⛔ | 手册有页,存档容器未取证 ⇒ 交宿主(DIV-RT-012) |
//! | `CG*` | 534+274+100+100 | ⛔ | 手册有页,渲染域;事件流记 DomainCall |
//! | `GO` | 195 | ✅ | 手册 `control/go.html` |
//! | `FLT` | 173 | ✅ | 手册 `variable/flt.html` |
//! | `LOOPBREAK` | 159 | ✅ | 手册 `control/loopbreak.html` |
//! | `S_STR` | 144 | ✅ | 手册 `variable/s_str.html` |
//! | `VARINFO` | 119 | ✅部分 | 手册 `variable/varinfo.html`;引擎 §3.21 |
//! | `LOOPCONTINUE` | 40 | ✅ | 手册 `control/loopcontinue.html` |
//! | `WAIT` | 95 | ✅ | 手册 `control/wait.html`;引擎 §3.2b |
//! | `FPS` | 2 | ✅ | 手册 `system/fps.html` |
//! | `END` | 205 | ✅ | 手册 `control/end.html` |
//! | `TASK`/`TASKEND`/`TASKINFO` | 10/2/19 | ✅部分 | 手册 `control/task*.html` |
//! | `MATH` | 21 | ✅部分 | 手册 `math/math.html`;引擎 §3.20 |
//! | `TEXT`/`_` | 2/33 | ✅ | 手册 `text/text.html`+`basic/basic.html ●テキスト表示` |
//!
//! ⛔ = 按任务书军规「无手册页不实现 / 无引擎证据不猜」显式挂号。

use yu_log::Boundary;

use crate::code;
use crate::diag;
use crate::event::{ArgValue, Domain, JumpKind, VmEvent};
use crate::task::{Effect, StepOutcome, TaskSeed};
use crate::value::{ElemType, Value, VarRef, VarSpace};
use crate::vm::{Cmd, CmdWindow, ExecState, GosubFrame, IfFrame, LoopFrame, Vm, VmError};

/// 引擎嵌套上限(手册 `control/if.html` 64 / `control/loop.html` 64 /
/// `control/gosub.html`「GOSUBのネストは64回まで可能です」)。
const NEST_LIMIT: usize = 64;

/// GOSUB 嵌套上限的引擎判据(REA 0.495 `FUN_004459c4` @0x4459e6..0x4459f3):
/// `MOVSX ESI, byte [task+0x140]`(当前深度)→ `LEA EAX,[ESI+1]`
/// → `CMP EAX, 0x40; JE <报错>`。即**新深度 == 64** 就报错,允许的最大深度
/// 是 63 层调用栈帧;`task+0x140` 初值 0(只有调用帧计数,基帧不在其中)。
/// 故允许的 GOSUB 次数 = 64,对应本实现 `frames.len() == 65`(基帧 + 64 帧)。
const GOSUB_MAX_CALLS: usize = 64;

/// 运行核依赖的命令名(处理器注册表;缺任一 ⇒ 工程表不完整 ⇒ 拒绝启动)。
///
/// 这不是"内置命令表":它只列出**本 crate 实现了语义**的名字,名字本身仍
/// 必须由工程 `ysc.ybn` 提供;表里没有 = 该工程版本不兼容,直接拒启。
pub fn required_commands(yscm: &crate::tables::YscmTable) -> Result<(), yu_log::Diag> {
    const REQUIRED: &[&str] = &[
        "GOSUB", "RETURN", "GO", "IF", "ELSE", "IFBLEND", "IFEND", "LOOP", "LOOPEND",
        "WAIT", "LET", "INT", "FLT", "STR", "END",
    ];
    for name in REQUIRED {
        if yscm.index_of(name).is_none() {
            return Err(diag::fatal(
                code::table::INCOMPLETE,
                Boundary::Table,
                format!("ysc.ybn 缺少运行核必需命令 `{name}`(工程命令表不完整)"),
            )
            .at_table("ysc.ybn"));
        }
    }
    Ok(())
}

/// 分派热路径的处理器类别(启动期按工程 `ysc.ybn` 名字解析成 opcode 下标表;
/// 每条命令一次数组取下标,替代逐名字符串比较)。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HandlerKind {
    Gosub,
    Return,
    Go,
    If,
    Else,
    Ifblend,
    Ifend,
    Ifbreak,
    Ifcontinue,
    Loop,
    Loopend,
    Loopbreak,
    Loopcontinue,
    Wait,
    Fps,
    End,
    Let,
    Declare,
    Varinfo,
    Varact,
    Math,
    Task,
    Taskend,
    Taskinfo,
    Systeminfo,
    Text,
    TextOut,
    Word,
    ReturnCode,
    Labelinfo,
    /// 手册有页的域命令(CG/SOUND/WINDOW/…)→ DomainCall 事件。
    Domain(Domain),
    /// 无处理器(手册无页且非域)。
    Unknown,
}

/// 按工程 yscm 建表(名字 → 处理器类别)。表 = 数据解析结果,不是内置命令表。
pub fn build_handler_table(yscm: &crate::tables::YscmTable) -> Vec<HandlerKind> {
    let mut table = Vec::with_capacity(yscm.len());
    for i in 0..yscm.len() {
        let name = yscm.command(i as u8).map(|c| c.name.as_str()).unwrap_or("");
        let kind = match name {
            "GOSUB" => HandlerKind::Gosub,
            "RETURN" => HandlerKind::Return,
            "GO" => HandlerKind::Go,
            "IF" => HandlerKind::If,
            "ELSE" => HandlerKind::Else,
            "IFBLEND" => HandlerKind::Ifblend,
            "IFEND" => HandlerKind::Ifend,
            "IFBREAK" => HandlerKind::Ifbreak,
            "IFCONTINUE" => HandlerKind::Ifcontinue,
            "LOOP" => HandlerKind::Loop,
            "LOOPEND" => HandlerKind::Loopend,
            "LOOPBREAK" => HandlerKind::Loopbreak,
            "LOOPCONTINUE" => HandlerKind::Loopcontinue,
            "WAIT" => HandlerKind::Wait,
            "FPS" => HandlerKind::Fps,
            "END" => HandlerKind::End,
            "LET" => HandlerKind::Let,
            "INT" | "FLT" | "STR" | "S_INT" | "S_FLT" | "S_STR" => HandlerKind::Declare,
            "VARINFO" => HandlerKind::Varinfo,
            "VARACT" => HandlerKind::Varact,
            "MATH" => HandlerKind::Math,
            "TASK" => HandlerKind::Task,
            "TASKEND" => HandlerKind::Taskend,
            "TASKINFO" => HandlerKind::Taskinfo,
            "SYSTEMINFO" => HandlerKind::Systeminfo,
            "TEXT" => HandlerKind::Text,
            "_" => HandlerKind::TextOut,
            "WORD" => HandlerKind::Word,
            "RETURNCODE" => HandlerKind::ReturnCode,
            "LABELINFO" => HandlerKind::Labelinfo,
            other => {
                let domain = domain_of(other);
                if domain != Domain::Other {
                    HandlerKind::Domain(domain)
                } else {
                    HandlerKind::Unknown
                }
            }
        };
        table.push(kind);
    }
    table
}

impl Vm {
    /// 把命令窗口逐条求值成 `(槽号, 值)`。跳转窗口与空窗口跳过;
    /// 求值失败省略该窗口(处理器/诊断会给出真正的失败原因)。
    fn eval_arg_windows(
        &mut self,
        st: &mut ExecState,
        cmd: &Cmd,
    ) -> Vec<ArgValue> {
        let mut args = Vec::new();
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            if let Ok(v) = self.eval_value(st, &w.payload) {
                args.push(ArgValue::new(w.rec.kind, v));
            }
        }
        args
    }

    pub(crate) fn dispatch(
        &mut self,
        st: &mut ExecState,
        task: &mut crate::task::TaskNode,
        cmd: &Cmd,
    ) -> Result<StepOutcome, VmError> {
        // 统一事件:尽量把每个窗口求值结果作为参数记录(失败则省略该窗口,
        // 真正的失败会在处理器里以 Diag 报出)。控制流窗口(跳转目标)是
        // 「length 字段 = 组号」而不是表达式,所以只记录成功求值者。
        // `record_events=false` 时跳过(热路径最大开销;Diag 不受影响)。
        let args = if self.record_events {
            self.eval_arg_windows(st, cmd)
        } else {
            Vec::new()
        };
        self.record_command(st, cmd, args);

        // 启动期解析的 opcode → 处理器表(见 `build_handler_table`)。
        let kind = self
            .handler_of
            .get(cmd.opcode as usize)
            .copied()
            .unwrap_or(HandlerKind::Unknown);
        match kind {
            HandlerKind::Gosub => self.op_gosub(st, cmd),
            HandlerKind::Return => self.op_return(st, cmd),
            HandlerKind::Go => self.op_go(st, cmd),
            HandlerKind::If => self.op_if(st, cmd),
            HandlerKind::Else => self.op_else(st, cmd),
            HandlerKind::Ifblend => self.op_ifblend(st, cmd),
            HandlerKind::Ifend => self.op_ifend(st, cmd),
            HandlerKind::Ifbreak => self.op_ifbreak(st, cmd),
            HandlerKind::Ifcontinue => self.op_ifcontinue(st, cmd),
            HandlerKind::Loop => self.op_loop(st, cmd),
            HandlerKind::Loopend => self.op_loopend(st, cmd),
            HandlerKind::Loopbreak => self.op_loopbreak(st, cmd),
            HandlerKind::Loopcontinue => self.op_loopcontinue(st, cmd),
            HandlerKind::Wait => self.op_wait(st, task, cmd),
            HandlerKind::Fps => self.op_fps(st, cmd),
            HandlerKind::End => self.op_end(st, cmd),
            HandlerKind::Let => self.op_let(st, cmd),
            HandlerKind::Declare => self.op_declare(st, cmd),
            HandlerKind::Varinfo => self.op_varinfo(st, cmd),
            HandlerKind::Varact => self.op_varact(st, cmd),
            HandlerKind::Math => self.op_math(st, cmd),
            HandlerKind::Task => self.op_task(st, task, cmd),
            HandlerKind::Taskend => self.op_taskend(st, task, cmd),
            HandlerKind::Taskinfo => self.op_taskinfo(st, task, cmd),
            HandlerKind::Systeminfo => self.op_systeminfo(st, cmd),
            HandlerKind::Text => self.op_text(st, cmd),
            HandlerKind::TextOut => self.op_text_out(st, cmd),
            HandlerKind::Word => self.op_word(st, cmd),
            HandlerKind::ReturnCode => self.op_returncode(st, cmd),
            HandlerKind::Labelinfo => self.op_labelinfo(st, cmd),
            HandlerKind::Domain(domain) => {
                // 域命令的参数是宿主域状态机唯一能看到的输入;必须带上
                // 求值结果(槽号 = 工程 ysc 表的参数序号)。`record_events`
                // 只控制通用 Command 事件,不影响域事件。
                let args = self.eval_arg_windows(st, cmd);
                self.emit(VmEvent::DomainCall {
                    frame: self.frame(),
                    domain,
                    name: cmd.name.clone(),
                    args,
                });
                self.report_unimplemented(
                    &cmd.name,
                    "域命令未接线:语义属宿主域状态机(见任务书 A 的域边界)".to_string(),
                );
                Ok(StepOutcome::Continue)
            }
            HandlerKind::Unknown => {
                self.report_unimplemented(
                    &cmd.name,
                    "命令无处理器(手册无页或语义未取证)".to_string(),
                );
                Ok(StepOutcome::Continue)
            }
        }
    }

    // ---------------------------------------------------------- 控制流

    /// `GOSUB`:手册 `control/gosub.html`;引擎 §3.16(参数组 A + 调用栈)。
    fn op_gosub(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        if cmd.windows.is_empty() {
            return Err(windows_error(cmd, "至少需要 # 标签窗口").into());
        }
        let label = self.label_from_window(st, cmd, 0)?;
        // 参数:窗口 kind ≥ 1 对应 PINT/PFLT/PSTRn(名字来自工程 ysc)。
        let mut frame = GosubFrame::new(st.script_id, st.pc, st.base_a);
        for w in cmd.windows.iter().skip(1) {
            if w.rec.length == 0 {
                continue;
            }
            let pname = self.param_name(cmd.opcode, w.rec.kind).unwrap_or_default();
            let idx = param_index_of(&pname).unwrap_or(1);
            // 工程表异常(PINT0/PINT17+)→ Fatal 诊断,不回绕/不越界(任务书 E R7)。
            if !(1..=16).contains(&idx) {
                return Err(diag::fatal(
                    code::rt::CMD_WINDOWS,
                    Boundary::Script,
                    format!("GOSUB 参数名 {pname} 超出 1..16(工程 ysc 表异常)"),
                )
                .into());
            }
            let value = self.eval_value(st, &w.payload)?;
            let value = coerce_param(&pname, value);
            frame.args[idx - 1] = Some(value);
            frame.arg_flags[idx - 1] = true;
        }
        if st.frames.len() > GOSUB_MAX_CALLS {
            return Err(crate::task::nesting_error(
                "GOSUB",
                st.frames.len() - 1,
                GOSUB_MAX_CALLS,
            )
            .into());
        }
        st.frames.push(frame);
        let from_pc = cmd.pc;
        self.jump_to(st, label.script_id as u32, label.pc, label.base_a, JumpKind::Gosub, from_pc)?;
        Ok(StepOutcome::Continue)
    }

    /// `RETURN`:手册 `control/return.html`;引擎 §3.16(写调用方那一层后弹栈)。
    ///
    /// 手册 `basic/task.html`:`GOSUB` 嵌套不存在时到达 `RETURN` 的任务**自动消失**。
    fn op_return(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        if st.frames.len() <= 1 {
            // 基帧 RETURN = 任务结束(手册 `basic/task.html`)。
            return Ok(StepOutcome::Finish);
        }
        let mut ret: [Option<Value>; 16] = Default::default();
        let mut flags = [false; 16];
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let pname = self.param_name(cmd.opcode, w.rec.kind).unwrap_or_default();
            let idx = param_index_of(&pname).unwrap_or(1);
            let value = self.eval_value(st, &w.payload)?;
            let value = coerce_param(&pname, value);
            ret[idx - 1] = Some(value);
            flags[idx - 1] = true;
        }
        let frame = st.frames.pop().expect("帧数 > 1");
        // 返回值写进**调用方**那一层(弹出后 `last_mut()`)。
        if let Some(caller) = st.frames.last_mut() {
            caller.ret = ret;
            caller.ret_flags = flags;
        }
        let from_pc = cmd.pc;
        self.jump_to(
            st,
            frame.return_script,
            frame.return_pc,
            frame.base_a,
            JumpKind::Return,
            from_pc,
        )?;
        Ok(StepOutcome::Continue)
    }

    /// `GO`:手册 `control/go.html`(标签跳转,可跨文件;标签名全局唯一)。
    fn op_go(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let label = self.label_from_window(st, cmd, 0)?;
        let from_pc = cmd.pc;
        self.jump_to(st, label.script_id as u32, label.pc, label.base_a, JumpKind::Go, from_pc)?;
        Ok(StepOutcome::Continue)
    }

    /// `IF`:手册 `control/if.html`;引擎 §3.13(块帧 + 假分支跳 `item[2].length`,
    /// 为 0 则跳块出口)。
    fn op_if(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        if cmd.windows.len() < 3 {
            return Err(windows_error(cmd, "IF 需要 3 个窗口(条件/ELSE 目标/出口)").into());
        }
        if st.if_stack.len() >= NEST_LIMIT {
            return Err(crate::task::nesting_error("IF", st.if_stack.len(), NEST_LIMIT).into());
        }
        let end_pc = cmd.windows[2].rec.length;
        // **先压块帧,再求值** —— 引擎 §3.13 opcode 45 的顺序:
        // 「压块帧(base_a++、出口 = item[3].length)→ 求值 item[1]」。
        // 顺序不是风格问题:条件求值若失败(例如未取证的系统变量),
        // 先压帧能保证块结构完整,否则后面的 IFEND 会失配。
        st.if_stack.push(IfFrame {
            entry_pc: cmd.pc,
            end_pc,
            base_a: st.base_a,
        });
        // 条件求值:可恢复失败(Unimplemented)⇒ 记诊断并按「假」处理;
        // Fatal 照常抛出(任务终止)。
        let truthy = match self.eval_value(st, &cmd.windows[0].payload) {
            Ok(v) => v.truthy()?,
            Err(d) => {
                let recoverable = matches!(
                    d.severity,
                    yu_log::Severity::Unimplemented | yu_log::Severity::Recoverable
                );
                if !recoverable {
                    return Err(d.into());
                }
                self.report(d);
                false
            }
        };
        if truthy {
            return Ok(StepOutcome::Continue);
        }
        let else_pc = cmd.windows[1].rec.length;
        let to = if else_pc != 0 { else_pc } else { end_pc };
        if to == 0 {
            return Err(diag::fatal(
                code::rt::CMD_WINDOWS,
                Boundary::Script,
                "IF 假分支目标为 0(ELSE 目标与出口都未给)",
            )
            .into());
        }
        self.emit(VmEvent::Jump {
            frame: self.frame(),
            script: st.script_id,
            pc: cmd.pc,
            to_pc: to,
            to_script: st.script_id,
            kind: JumpKind::IfFalse,
        });
        st.pc = to;
        Ok(StepOutcome::Continue)
    }

    /// `ELSE`:手册 `control/else.html`(条件可缺省;假 → 下一个 ELSE 或块出口)。
    ///
    /// 两种真实形态(两套语料都实测):
    /// - **3 窗口**:ERIS 887 次 / 商业包 704 次 —— `[条件, 下一 ELSE 组号, 下一 ELSE 组号]`;
    /// - **0 窗口**:商业包 726 次 —— 编译器在「无条件 else 体」处发出的形态
    ///   (实测结构:`ELSE wc=0` → 体 → `IFEND`,如 v494 s52 g60..g65)。
    ///   语义 = 进入 else 体(条件缺省为真)。
    fn op_else(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        if cmd.windows.is_empty() {
            // 无条件 else 体:继续执行下一条(与「条件缺省 = 真」同义)。
            return Ok(StepOutcome::Continue);
        }
        if cmd.windows.len() < 3 {
            return Err(windows_error(cmd, "ELSE 的窗口数既不是 0 也不是 3").into());
        }
        let top = *st.if_stack.last().ok_or_else(|| {
            diag::fatal(
                code::rt::CMD_WINDOWS,
                Boundary::Script,
                "ELSE 无配对 IF(引擎同族报错)",
            )
        })?;
        let cond = if cmd.windows[0].payload.is_empty() {
            None
        } else {
            Some(self.eval_value(st, &cmd.windows[0].payload)?)
        };
        let truthy = match cond {
            Some(v) => v.truthy()?,
            None => true,
        };
        if truthy {
            return Ok(StepOutcome::Continue);
        }
        let next = cmd.windows[1].rec.length;
        let to = if next != 0 { next } else { top.end_pc };
        self.emit(VmEvent::Jump {
            frame: self.frame(),
            script: st.script_id,
            pc: cmd.pc,
            to_pc: to,
            to_script: st.script_id,
            kind: JumpKind::Else,
        });
        st.pc = to;
        Ok(StepOutcome::Continue)
    }

    /// `IFBLEND`:无条件跳到当前块出口(§3.13;手册 IF 页)。
    fn op_ifblend(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let top = *st.if_stack.last().ok_or_else(|| {
            diag::fatal(
                code::rt::CMD_WINDOWS,
                Boundary::Script,
                "IFBLEND 无配对 IF",
            )
        })?;
        self.emit(VmEvent::Jump {
            frame: self.frame(),
            script: st.script_id,
            pc: cmd.pc,
            to_pc: top.end_pc,
            to_script: st.script_id,
            kind: JumpKind::Else,
        });
        st.pc = top.end_pc;
        Ok(StepOutcome::Continue)
    }

    /// `IFEND`:弹块帧(§3.13:depth-- 并清出口)。
    fn op_ifend(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        if st.if_stack.pop().is_none() {
            return Err(diag::fatal(
                code::rt::CMD_WINDOWS,
                Boundary::Script,
                format!("IFEND 无配对 IF(s{} pc={})", st.script_id, cmd.pc),
            )
            .into());
        }
        Ok(StepOutcome::Continue)
    }

    /// `IFBREAK[LV]`:退 LV 层块并跳该层出口(§3.13 opcode 47)。
    fn op_ifbreak(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let lv = self.level_param(st, cmd)?;
        self.if_break_to(st, cmd, lv, false)
    }

    /// `IFCONTINUE[LV]`:退 LV 层并跳该层入口 pc(§3.13 opcode 48)。
    fn op_ifcontinue(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let lv = self.level_param(st, cmd)?;
        self.if_break_to(st, cmd, lv, true)
    }

    fn if_break_to(
        &mut self,
        st: &mut ExecState,
        cmd: &Cmd,
        lv: usize,
        cont: bool,
    ) -> Result<StepOutcome, VmError> {
        // LV=0:无层可退(与 `LOOPBREAK[LV=0]` 同口径;修复越界 panic,
        // 任务书 E R7)。
        if lv == 0 {
            return Ok(StepOutcome::Continue);
        }
        if st.if_stack.len() < lv {
            return Err(diag::fatal(
                code::rt::CMD_WINDOWS,
                Boundary::Script,
                format!(
                    "{} 层级 {lv} 越界(IF 深度 {})",
                    if cont { "IFCONTINUE" } else { "IFBREAK" },
                    st.if_stack.len()
                ),
            )
            .into());
        }
        let keep = st.if_stack.len() - lv;
        let target = &st.if_stack[keep];
        let to = if cont { target.entry_pc } else { target.end_pc };
        let base = target.base_a;
        if cont {
            // IFCONTINUE(REA 0x446490):引擎把深度设为 `depth-LV`,pc = 目标层
            // `+4`(入口 pc);目标 IF 会在入口处**重新压帧**。所以这里必须
            // **丢弃**目标层本身,否则 IF 再压一次会让块帧永久累积
            // (任务书 E R9)。
            st.if_stack.truncate(keep);
        } else {
            // IFBREAK(REA 0x446408):引擎把深度设为 `depth-LV+1` 并跳目标层
            // `+8`(IFEND 组);IFEND 执行时再 `depth--` 落到 `depth-LV`。
            // 即**保留**目标层,由目标 IFEND 弹掉它。
            st.if_stack.truncate(keep + 1);
        }
        st.base_a = base;
        self.emit(VmEvent::Jump {
            frame: self.frame(),
            script: st.script_id,
            pc: cmd.pc,
            to_pc: to,
            to_script: st.script_id,
            kind: if cont { JumpKind::IfContinue } else { JumpKind::IfBreak },
        });
        st.pc = to;
        Ok(StepOutcome::Continue)
    }

    /// `LOOP`:手册 `control/loop.html`;引擎 §3.13(opcode 56)。
    fn op_loop(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        if cmd.windows.len() < 2 {
            return Err(windows_error(cmd, "LOOP 需要 2 个窗口(SET/LOOPEND 目标)").into());
        }
        if st.loops.len() >= NEST_LIMIT {
            return Err(crate::task::nesting_error("LOOP", st.loops.len(), NEST_LIMIT).into());
        }
        // SET 缺省(空窗口)= 无限(手册)。
        let limit = if cmd.windows[0].payload.is_empty() {
            -1
        } else {
            match self.eval_value(st, &cmd.windows[0].payload)? {
                Value::Int(n) => n,
                other => {
                    return Err(diag::fatal(
                        code::rt::VAR_TYPE,
                        Boundary::Script,
                        format!("LOOP 计数必须是整数,得到 {}", other.type_name()),
                    )
                    .into())
                }
            }
        };
        let end_pc = cmd.windows[1].rec.length;
        st.loops.push(LoopFrame {
            body_pc: cmd.pc + 1,
            end_pc,
            limit,
            counter: 1,
            if_depth: st.if_stack.len(),
            base_a: st.base_a,
        });
        if limit == 0 {
            // 0 轮:直接去 LOOPEND(由它弹栈;手册「一度も通らず」)。
            st.pc = end_pc;
        }
        Ok(StepOutcome::Continue)
    }

    /// `LOOPEND`:手册 `control/loop.html`;引擎 §3.13(opcode 59)。
    fn op_loopend(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let Some(top) = st.loops.last().copied() else {
            return Err(diag::fatal(
                code::rt::CMD_WINDOWS,
                Boundary::Script,
                format!("LOOPEND 无配对 LOOP(s{} pc={})", st.script_id, cmd.pc),
            )
            .into());
        };
        let counter = top.counter + 1;
        let continue_loop = if top.limit < 0 {
            true
        } else {
            counter <= top.limit
        };
        if continue_loop {
            if let Some(f) = st.loops.last_mut() {
                f.counter = counter;
            }
            // `_LC` 必须读到新值;引擎 LOOPEND 先 +1 再回跳。
            st.base_a = top.base_a;
            self.emit(VmEvent::Jump {
                frame: self.frame(),
                script: st.script_id,
                pc: cmd.pc,
                to_pc: top.body_pc,
                to_script: st.script_id,
                kind: JumpKind::LoopBack,
            });
            st.pc = top.body_pc;
        } else {
            st.loops.pop();
        }
        Ok(StepOutcome::Continue)
    }

    /// `LOOPBREAK[LV]`:手册 `control/loopbreak.html`;引擎 §3.13(opcode 57)。
    fn op_loopbreak(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let lv = self.level_param(st, cmd)?;
        self.loop_jump(st, cmd, lv, false)
    }

    /// `LOOPCONTINUE[LV]`:手册 `control/loopcontinue.html`;§3.13(opcode 58)。
    fn op_loopcontinue(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let lv = self.level_param(st, cmd)?;
        self.loop_jump(st, cmd, lv, true)
    }

    fn loop_jump(
        &mut self,
        st: &mut ExecState,
        cmd: &Cmd,
        lv: usize,
        cont: bool,
    ) -> Result<StepOutcome, VmError> {
        if lv == 0 {
            return Ok(StepOutcome::Continue);
        }
        if st.loops.len() < lv {
            return Err(diag::fatal(
                code::rt::CMD_WINDOWS,
                Boundary::Script,
                format!(
                    "{} 层级 {lv} 越界(LOOP 深度 {})",
                    if cont { "LOOPCONTINUE" } else { "LOOPBREAK" },
                    st.loops.len()
                ),
            )
            .into());
        }
        let keep = st.loops.len() - lv;
        let target = st.loops[keep];
        st.loops.truncate(keep + 1);
        if !cont {
            // break:把目标层的目标轮数清零 ⇒ 即将执行的 LOOPEND 立即结束。
            if let Some(f) = st.loops.last_mut() {
                f.limit = 0;
                f.counter = 0;
            }
        }
        // 体内残留 IF 帧一并展开(引擎块记录随执行位恢复;§3.13)。
        if st.if_stack.len() > target.if_depth {
            st.if_stack.truncate(target.if_depth);
        }
        st.base_a = target.base_a;
        self.emit(VmEvent::Jump {
            frame: self.frame(),
            script: st.script_id,
            pc: cmd.pc,
            to_pc: target.end_pc,
            to_script: st.script_id,
            kind: if cont { JumpKind::LoopContinue } else { JumpKind::LoopBreak },
        });
        st.pc = target.end_pc;
        Ok(StepOutcome::Continue)
    }

    /// `WAIT[FRAME=n|TIME=ms]`:手册 `control/wait.html`;引擎 §3.2b/§3.2c。
    fn op_wait(
        &mut self,
        st: &mut ExecState,
        task: &mut crate::task::TaskNode,
        cmd: &Cmd,
    ) -> Result<StepOutcome, VmError> {
        let mut frames = None;
        let mut time = None;
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let v = self.eval_value(st, &w.payload)?;
            let n = v.as_int().ok_or_else(|| {
                diag::fatal(
                    code::rt::VAR_TYPE,
                    Boundary::Script,
                    format!("WAIT 参数必须是整数,得到 {}", v.type_name()),
                )
            })?;
            match w.rec.kind {
                0 => frames = Some(n.max(0) as u32),
                1 => time = Some(n.max(0) as u32),
                other => {
                    return Err(diag::fatal(
                        code::rt::CMD_SLOT,
                        Boundary::Script,
                        format!("WAIT 未声明参数序号 {other}(工程 ysc 只有 FRAME/TIME)"),
                    )
                    .into())
                }
            }
        }
        if frames.is_none() && time.is_none() {
            return Ok(StepOutcome::Continue);
        }
        task.wait_frames = frames.unwrap_or(0);
        task.deadline_ms = time.map(|ms| self.now_ms().wrapping_add(ms));
        self.emit(VmEvent::Wait {
            frame: self.frame(),
            task: task.id.clone(),
            frames,
            time_ms: task.deadline_ms.map(|d| d.wrapping_sub(self.now_ms())),
        });
        Ok(StepOutcome::Yield)
    }

    /// `FPS[SET=1..256 SKIP=0/1]`:手册 `system/fps.html`。
    fn op_fps(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let mut fps = self.fps();
        let mut skip = self.skip();
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let v = self.eval_value(st, &w.payload)?;
            let n = v.as_int().unwrap_or(0);
            match w.rec.kind {
                0 => {
                    if !(1..=256).contains(&n) {
                        return Err(diag::fatal(
                            code::rt::CMD_SLOT,
                            Boundary::Script,
                            format!("FPS[SET={n}] 超出手册范围 1..256"),
                        )
                        .into());
                    }
                    fps = n as u32;
                }
                1 => skip = n != 0,
                other => {
                    return Err(diag::fatal(
                        code::rt::CMD_SLOT,
                        Boundary::Script,
                        format!("FPS 未声明参数序号 {other}"),
                    )
                    .into())
                }
            }
        }
        self.set_fps(fps, skip);
        self.emit(VmEvent::Fps {
            frame: self.frame(),
            fps,
            skip,
        });
        Ok(StepOutcome::Continue)
    }

    /// `END[RESTART]`:手册 `control/end.html`。
    fn op_end(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let mut restart = false;
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let v = self.eval_value(st, &w.payload)?;
            if w.rec.kind == 0 {
                restart = v.as_int().unwrap_or(0) != 0;
            }
        }
        self.emit(VmEvent::End {
            frame: self.frame(),
            restart,
        });
        let _ = st;
        Ok(StepOutcome::Finish)
    }

    // ---------------------------------------------------------- 变量

    /// `LET`:手册 `variable/let.html`;引擎 §3.17(opcode 54)+ §3.10。
    ///
    /// 复合赋值码在窗口 `slot` 高字节(语料实测 0..8;`tag>>24` 同源)。
    fn op_let(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        if cmd.windows.len() != 2 {
            return Err(diag::fatal(
                code::rt::CMD_WINDOWS,
                Boundary::Script,
                format!("LET 窗口数 {} != 2", cmd.windows.len()),
            )
            .into());
        }
        let lval = self.eval_lvalue(st, &cmd.windows[0].payload)?;
        // ERIS 旧式空间按 id 的声明类型归一化;其它未知前缀挂号继续(DIV-RT-017)。
        let lval = crate::expr::LValue {
            var: self.canonical_var(lval.var),
            indices: lval.indices.clone(),
        };
        if matches!(lval.var.space, VarSpace::Other(_)) {
            return Err(diag::unimplemented(
                code::rt::EXPR_UNSUPPORTED,
                Boundary::Script,
                format!(
                    "写入未取证变量空间 {:#04x}({});本次写入被忽略(DIV-RT-017)",
                    lval.var.space.prefix(),
                    lval.var.display()
                ),
            )
            .into());
        }
        // 输入系统变量(手册标 ○ 可赋值):写入 = 设定该按钮的帧计数器。
        if lval.var.id < self.sys_count {
            let name = self
                .sysvars()
                .get(lval.var.id)
                .map(|d| d.name.clone())
                .unwrap_or_default();
            let button = name
                .strip_prefix("_KEY_")
                .or_else(|| name.strip_prefix("_MOUSE_"))
                .filter(|rest| {
                    // 只收按钮族(排除 _MOUSE_X/Y 与 _KEYNUM_* 等)。
                    !matches!(*rest, "X" | "Y" | "LAST")
                        && !rest.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(true)
                })
                .map(|rest| {
                    if name.starts_with("_KEY_") {
                        format!("KEY_{rest}")
                    } else {
                        format!("MOUSE_{rest}")
                    }
                });
            if let Some(button) = button {
                let rhs = self.eval_value(st, &cmd.windows[1].payload)?;
                let v = rhs.as_int().unwrap_or(0);
                self.input_state_mut().set_counter(&button, v);
                return Ok(StepOutcome::Continue);
            }
            // `@_MENUCODE` / `@_WINDOW_EXEC_MAX/CLOSE`(手册 ○)= 状态覆盖。
            if name == "_MENUCODE" {
                let rhs = self.eval_value(st, &cmd.windows[1].payload)?;
                let v = rhs.as_int().unwrap_or(0);
                self.set_menu_code(v);
                return Ok(StepOutcome::Continue);
            }
            if name == "_WINDOW_EXEC_MAX" || name == "_WINDOW_EXEC_CLOSE" {
                let rhs = self.eval_value(st, &cmd.windows[1].payload)?;
                let v = rhs.as_int().unwrap_or(0);
                let which = name.rsplit('_').next().unwrap_or("").to_string();
                self.set_window_exec(&which, v);
                return Ok(StepOutcome::Continue);
            }
            // `$_TEXT`(手册 ○)= 替换当前显示文本。
            if name == "_TEXT" {
                let rhs = self.eval_value(st, &cmd.windows[1].payload)?;
                let bytes = match rhs.cast_to_str()? {
                    Value::Str(b) => b,
                    _ => Vec::new(),
                };
                self.set_text(bytes);
                return Ok(StepOutcome::Continue);
            }
            // `_CODE_*`(手册 ○;键位重映射)= 覆盖引擎缺省按钮码。
            if name.starts_with("_CODE_") {
                let rhs = self.eval_value(st, &cmd.windows[1].payload)?;
                let v = rhs.as_int().unwrap_or(0);
                let id = lval.var.id;
                self.input_state_mut().set_code(id, v);
                return Ok(StepOutcome::Continue);
            }
            // `_PAD_*` / `_PADNUM_*`(手册 ○)= 设定手柄码计数器。
            if name.starts_with("_PADNUM_") || name.starts_with("_PAD_") {
                let is_num = name.starts_with("_PADNUM_");
                let base = name
                    .strip_prefix("_PADNUM_")
                    .or_else(|| name.strip_prefix("_PAD_"))
                    .unwrap_or("");
                let idx: Vec<i64> = lval
                    .indices
                    .iter()
                    .copied()
                    .collect();
                let code = match (base, idx.as_slice()) {
                    ("UP", [n]) | ("DOWN", [n]) | ("LEFT", [n]) | ("RIGHT", [n]) => {
                        let dir = match base {
                            "UP" => 0,
                            "DOWN" => 1,
                            "LEFT" => 2,
                            _ => 3,
                        };
                        crate::input::pad_dir_code(dir, *n)
                    }
                    ("BUTTON", [n, m]) => crate::input::pad_button_code(*n, *m),
                    _ => {
                        return Err(diag::fatal(
                            code::rt::SYSVAR_WRITE,
                            Boundary::Script,
                            format!("{name}:写入下标形态不符"),
                        )
                        .into())
                    }
                };
                let rhs = self.eval_value(st, &cmd.windows[1].payload)?;
                let v = rhs.as_int().unwrap_or(0);
                self.input_state_mut().set_pad_counter(code, v, is_num);
                return Ok(StepOutcome::Continue);
            }
        }
        // 其它系统变量写入:语义未取证 ⇒ 挂号(DIV-RT-007)。
        //
        // 手册 `basic/svariable.html` 用 ○ 标出「可赋值」的系统变量
        // (输入/窗口状态族),但没有给出写入后的**可观测语义**;
        // 引擎侧只有间接线索。按军规:挂号 + 无副作用 + 继续执行(DIV-RT-007)。
        if lval.var.id < self.sys_count {
            let name = self
                .sysvars()
                .get(lval.var.id)
                .map(|d| d.name.clone())
                .unwrap_or_else(|| lval.var.display());
            return Err(diag::unimplemented(
                code::rt::SYSVAR_WRITE,
                Boundary::Script,
                format!(
                    "写入系统变量 {name}(手动 ○ 可赋值,但写入语义未取证);本次写入被忽略(DIV-RT-007)"
                ),
            )
            .into());
        }
        let rhs = self.eval_value(st, &cmd.windows[1].payload)?;
        let code = (cmd.windows[0].rec.slot >> 8) as u8;
        let value = if code == 0 {
            rhs
        } else {
            let cur = self.read_lvalue(&lval)?;
            compound(code, &cur, &rhs)?
        };
        self.store.write(lval.var, &lval.indices, &value)?;
        Ok(StepOutcome::Continue)
    }

    /// `INT/FLT/STR/S_INT/S_FLT/S_STR`:手册 `variable/*.html`。
    ///
    /// 声明窗口 = 变量引用(数组时末条 `0x29` 前是各维长度);
    /// 初始值窗口 = 表达式(缺省 = 类型零值,手册「初期値を省略した場合０」)。
    fn op_declare(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        if cmd.windows.is_empty() {
            return Err(windows_error(cmd, "声明命令缺少变量窗口").into());
        }
        let ty = match cmd.name.as_str() {
            "INT" | "S_INT" => ElemType::Int,
            "FLT" | "S_FLT" => ElemType::Float,
            "STR" | "S_STR" => ElemType::Str,
            _ => unreachable!(),
        };
        let target = self.decl_target(st, &cmd.windows[0])?;
        let dims: Vec<usize> = target.iter().map(|&d| d.max(0) as usize).collect();
        let init = match cmd.windows.get(1) {
            Some(w) if !w.payload.is_empty() => self.eval_value(st, &w.payload)?,
            _ => ty.default_value(),
        };
        self.store.declare(target_var(&cmd.windows[0])?, ty, &dims, &init);
        let _ = st;
        Ok(StepOutcome::Continue)
    }

    /// 声明命令的变量窗口:引用 + 可选各维长度。
    fn decl_target(
        &self,
        st: &ExecState,
        w: &CmdWindow,
    ) -> Result<Vec<i64>, VmError> {
        // 单条变量指令 = 标量;含 `0x29` = 数组(前导值 = 各维长度)。
        let lv = self.eval_lvalue(st, &w.payload)?;
        Ok(lv.indices)
    }

    /// `VARINFO`:手册 `variable/varinfo.html`;引擎 §3.21。
    fn op_varinfo(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let mut target: Option<crate::expr::LValue> = None;
        let mut set: Option<crate::expr::LValue> = None;
        let mut op: Option<(u8, i64)> = None;
        // 全部数值槽(SEARCH 的 NO=20 与 INT/FLT 针值要用)。
        let mut flags: Vec<(u8, i64)> = Vec::new();
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            match w.rec.kind {
                0 => set = Some(self.eval_lvalue(st, &w.payload)?),
                1 => target = Some(self.eval_lvalue(st, &w.payload)?),
                slot => {
                    let v = self.eval_value(st, &w.payload)?;
                    if let Some(n) = v.as_int() {
                        flags.push((slot, n));
                        if op.is_none() && n != 0 && !matches!(slot, 17..=20) {
                            op = Some((slot, n));
                        }
                    }
                }
            }
        }
        let (slot, _) = op.ok_or_else(|| {
            diag::fatal(
                code::rt::CMD_SLOT,
                Boundary::Script,
                "VARINFO 无查询项(手册:至少给 TYPE/LENGTH/DIMNUM/DIMSIZE*)",
            )
        })?;
        let set = set.ok_or_else(|| {
            diag::fatal(
                code::rt::CMD_SLOT,
                Boundary::Script,
                "VARINFO 缺 SET(被查询变量)",
            )
        })?;
        let set_var = set.var;
        let (shape_ty, shape_dims) = match self.var_shape(set_var) {
            Some(v) => v,
            None => {
                return Err(diag::unimplemented(
                    code::rt::VAR_UNDECLARED,
                    Boundary::Script,
                    format!("VARINFO 查询未声明变量 {}", set_var.display()),
                )
                .into())
            }
        };
        let shape = (shape_ty, shape_dims.as_slice());
        let result_val = match slot {
            2 => Value::Int(match shape.0 {
                ElemType::Int => 1,
                ElemType::Float => 2,
                ElemType::Str => 3,
            }),
            4 => Value::Int(shape.1.len() as i64),
            5..=12 => {
                let k = (slot - 5) as usize;
                match shape.1.get(k) {
                    Some(&d) => Value::Int(d as i64),
                    None => {
                        return Err(diag::fatal(
                            code::rt::VAR_SHAPE,
                            Boundary::Script,
                            format!("VARINFO DIMSIZE{} 超出维数 {}", k + 1, shape.1.len()),
                        )
                        .into())
                    }
                }
            }
            13 => {
                // 保留 SET 里写的下标(`$_PSTR(1)` 的 `(1)`);无下标才用 0 号元素。
                let idx: Vec<i64> = if set.indices.is_empty() {
                    vec![0; shape.1.len()]
                } else {
                    set.indices.clone()
                };
                let v = self.read_var(st, set_var, &idx)?;
                match v {
                    Value::Str(bytes) => Value::Int(str_char_count(&bytes) as i64),
                    other => {
                        return Err(diag::fatal(
                            code::rt::VAR_TYPE,
                            Boundary::Script,
                            format!("VARINFO LENGTH 需要字符串变量,得到 {}", other.type_name()),
                        )
                        .into())
                    }
                }
            }
            // `SEARCH`:手册 `variable/varinfo.html` 例 5/6/7 ——
            // 从 NO 号元素开始在一维数组里找第一个相等的元素,返回下标,
            // 找不到返回 -1。要搜索的值由 `INT=`/`FLT=`/`STR=` 参数给出。
            14 => {
                let need = array_needle(self, cmd, st, set_var)?;
                let (ty, dims) = shape;
                let (ty, dims) = (ty, dims.to_vec());
                if dims.len() != 1 {
                    return Err(diag::fatal(
                        code::rt::VAR_SHAPE,
                        Boundary::Script,
                        format!("VARINFO SEARCH 只支持一维数组,目标 {} 维", dims.len()),
                    )
                    .into());
                }
                let start = flags_get(&flags, 20).unwrap_or(0).max(0);
                let n = dims[0] as i64;
                let mut found = -1i64;
                for i in start..n {
                    let v = self.read_var(st, set_var, &[i])?;
                    let hit = match (&need, &v) {
                        (None, _) => false,
                        (Some(want), got) => match ty {
                            ElemType::Str => {
                                matches!((want, got), (Value::Str(a), Value::Str(b)) if a == b)
                            }
                            _ => want.as_f64() == got.as_f64(),
                        },
                    };
                    if hit {
                        found = i;
                        break;
                    }
                }
                Value::Int(found)
            }
            other => {
                return Err(diag::unimplemented(
                    code::rt::CMD_UNIMPLEMENTED,
                    Boundary::Script,
                    format!("VARINFO 参数序号 {other} 未实现(STRTYPE/SJISCODE/UTF8CODE 无手册例;DIV-RT-011)"),
                )
                .into())
            }
        };
        if let Some(t) = target {
            self.write_target(st, &t, &result_val)?;
        }
        Ok(StepOutcome::Continue)
    }

    /// 写回一个左值(保留下标);系统变量目标 ⇒ 挂号(DIV-RT-007)。
    fn write_target(
        &mut self,
        _st: &ExecState,
        target: &crate::expr::LValue,
        v: &Value,
    ) -> Result<(), VmError> {
        let target = crate::expr::LValue {
            var: self.canonical_var(target.var),
            indices: target.indices.clone(),
        };
        let target = &target;
        if target.var.id < self.sys_count {
            return Err(diag::unimplemented(
                code::rt::SYSVAR_WRITE,
                Boundary::Script,
                format!(
                    "写入系统变量 {}(语义未取证;DIV-RT-007)",
                    target.var.display()
                ),
            )
            .into());
        }
        self.store.write(target.var, &target.indices, v)?;
        Ok(())
    }

    /// `VARACT`:手册 `variable/varact.html`;引擎 §3.26。
    fn op_varact(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let mut set: Option<crate::expr::LValue> = None;
        let mut letv: Option<crate::expr::LValue> = None;
        let mut flags: Vec<(u8, i64)> = Vec::new();
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            match w.rec.kind {
                0 => set = Some(self.eval_lvalue(st, &w.payload)?),
                1 => letv = Some(self.eval_lvalue(st, &w.payload)?),
                slot => {
                    let v = self.eval_value(st, &w.payload)?;
                    if let Some(n) = v.as_int() {
                        flags.push((slot, n));
                    }
                }
            }
        }
        // `INIT`/`G_*` 分支不读 `SET`(引擎 §3.26 ③:`0x454d3d..0x454e00`
        // 一次都没碰 `0x683a60`),因此把「缺 SET → Fatal」的判定推到它们之后。
        let set_missing = set.is_none();
        let set = set.unwrap_or(crate::expr::LValue {
            var: VarRef::new(VarSpace::At, 0),
            indices: Vec::new(),
        });
        let set_var = set.var;
        // `SET` 为系统变量是合法用法:ERIS `stdlib/ystdlib.yst` 的
        // `_strlen($_PSTR(1))` / `_inttostr(@_PINT(1))` 等 29 处实测如此。
        // 写系统变量(LET=系统变量)语义未取证 ⇒ 单独挂号(DIV-RT-007)。
        if let Some(t) = &letv {
            if t.var.id < self.sys_count {
                return Err(diag::unimplemented(
                    code::rt::SYSVAR_WRITE,
                    Boundary::Script,
                    format!(
                        "VARACT 目标为系统变量 {}(写入语义未取证;DIV-RT-007)",
                        t.var.display()
                    ),
                )
                .into());
            }
        }
        let flag = |slot: u8| flags.iter().find(|(s, _)| *s == slot).map(|(_, v)| *v);
        let num = |slot: u8| flags.iter().find(|(s, _)| *s == slot).map(|(_, v)| *v).unwrap_or(0);
        if flag(6).unwrap_or(0) != 0 {
            // TYPE:按目标类型转换(手册例 1/2;§3.22)。
            let target = letv.ok_or_else(|| {
                diag::fatal(code::rt::CMD_SLOT, Boundary::Script, "VARACT TYPE 缺 LET")
            })?;
            let ty = self.var_shape(target.var).map(|s| s.0).ok_or_else(|| {
                diag::unimplemented(
                    code::rt::VAR_UNDECLARED,
                    Boundary::Script,
                    format!("VARACT 目标未声明 {}", target.var.display()),
                )
            })?;
            let v = self.read_var(st, set_var, &set.indices)?;
            let converted = match ty {
                ElemType::Int => v.cast_to_int()?,
                ElemType::Float => Value::Float(v.as_f64().unwrap_or(0.0)),
                ElemType::Str => v.cast_to_str()?,
            };
            self.write_target(st, &target, &converted)?;
            return Ok(StepOutcome::Continue);
        }
        // ---- INIT(16) + G_INT..G_STR4(17..28):按变量镜像恢复全局变量 ----
        //
        // 手册 `variable/varact.html` **未列这一组**(参数表里没有 INIT/G_*),
        // 但引擎有完整算法(旁线 `format-vm-opcodes.md` §3.26 ④,
        // 处理体 `0x454d3d` 掩码构造 + `0x455b60` 恢复体,逐条反汇编):
        //
        //  - `INIT` 只是守卫:**不在场 → 整条指令什么都不做**;
        //  - 17..28 各置一位掩码(INT 组 0..3 / FLT 组 0..3 / STR 组 0..3);
        //  - 恢复时**只恢复 `ysv` 里 `kind == 1`(全局)且组号位被置位**的记录;
        //    组号 ≥5 的记录没有否决项 ⇒ 照常恢复;
        //  - 重建是「整槽按声明重建」(写描述符 + 类型数组),不是逐元素赋值;
        //  - 这一组**不读 `SET`**。
        //
        // 这是 ERIS 启动链的硬依赖(`es_SoundDefine` 之类在初始化时
        // `VARACT[INIT=1 G_*]` 把全局表恢复成初值)。
        if flag(16).is_some() || (17..=28).any(|s| flag(s).is_some()) {
            if flag(16).unwrap_or(0) == 0 {
                // 无 INIT:引擎同位置 `je` 到函数尾 —— 什么都不做。
                return Ok(StepOutcome::Continue);
            }
            let mut masks = [0u8; 3]; // INT / FLT / STR 三族的 4 位掩码
            for slot in 17..=28u8 {
                if flag(slot).unwrap_or(0) == 0 {
                    continue;
                }
                let (family, bit) = match slot {
                    17..=20 => (0usize, slot - 17),
                    21..=24 => (1usize, slot - 21),
                    _ => (2usize, slot - 25),
                };
                masks[family] |= 1 << bit;
            }
            self.restore_globals(st, &masks)?;
            return Ok(StepOutcome::Continue);
        }
        if set_missing {
            return Err(diag::fatal(
                code::rt::CMD_SLOT,
                Boundary::Script,
                "VARACT 缺 SET(源变量)",
            )
            .into());
        }
        // 手册 `variable/varact.html` 例 9/10:`VARACT[DIMSIZE=500 SET=@A()]`
        // 与 `VARACT[DIMSIZE+=1 SET=@A()]` —— **没有 LET**,原地改数组。
        // 同理 PUSH/POP(例未给,但参数表无 LET 语义需求)。
        if flag(13).unwrap_or(0) != 0 {
            // 手册例 9/10:`DIMSIZE=500`(绝对值)/ `DIMSIZE+=1`(相对值)。
            // 语料实测 **104/104 都是 `+=`**,复合码在窗口 `slot` 高字节
            // (与 `LET` 同一编码:0== 1+= 2-= …)。
            let code = (cmd.windows.iter().find(|w| w.rec.kind == 13).map(|w| w.rec.slot >> 8).unwrap_or(0)) as u8;
            let operand = num(13);
            let (ty, dims) = self.var_shape(set_var).ok_or_else(|| {
                diag::unimplemented(
                    code::rt::VAR_UNDECLARED,
                    Boundary::Script,
                    format!("DIMSIZE 目标未声明 {}", set_var.display()),
                )
            })?;
            if dims.len() != 1 {
                return Err(diag::fatal(
                    code::rt::VAR_SHAPE,
                    Boundary::Script,
                    format!("DIMSIZE 只支持一维数组,目标 {} 维", dims.len()),
                )
                .into());
            }
            let old = dims[0] as i64;
            let new_len = match code {
                0 => operand,
                1 => old + operand,
                2 => old - operand,
                3 => old * operand,
                4 => {
                    if operand == 0 {
                        return Err(diag::fatal(
                            code::rt::EXPR_DIV_ZERO,
                            Boundary::Script,
                            "DIMSIZE /= 0",
                        )
                        .into());
                    }
                    old / operand
                }
                other => {
                    return Err(diag::unimplemented(
                        code::rt::CMD_UNIMPLEMENTED,
                        Boundary::Script,
                        format!("DIMSIZE 复合码 {other} 未实现"),
                    )
                    .into())
                }
            };
            self.resize_1d(set_var, ty, new_len.max(0) as usize)?;
            return Ok(StepOutcome::Continue);
        }
        if flag(14).unwrap_or(0) != 0 || flag(15).unwrap_or(0) != 0 {
            let push = flag(14).unwrap_or(0) != 0;
            let pos = num(if push { 14 } else { 15 }).max(0) as usize;
            self.shift_1d(set_var, pos, push)?;
            return Ok(StepOutcome::Continue);
        }
        let target = letv.ok_or_else(|| {
            diag::fatal(code::rt::CMD_SLOT, Boundary::Script, "VARACT 缺 LET(目标变量)")
        })?;
        let src = self.read_var(st, set_var, &set.indices)?;
        let Value::Str(bytes) = src else {
            return Err(diag::fatal(
                code::rt::VAR_TYPE,
                Boundary::Script,
                "VARACT 字符串加工需要字符串源变量",
            )
            .into());
        };
        let out;
        if flag(3).unwrap_or(0) != 0 || flag(2).unwrap_or(0) != 0 {
            let pos = num(4).max(1) as usize;
            let len = num(5).max(0) as usize;
            out = if flag(3).unwrap_or(0) != 0 {
                copy_chars(&bytes, pos, len)
            } else {
                cut_chars(&bytes, pos, len)
            };
        } else if flag(7).unwrap_or(0) != 0 {
            out = ascii_case(&bytes, true);
        } else if flag(8).unwrap_or(0) != 0 {
            out = fullwidth_case(&bytes, true);
        } else if flag(9).unwrap_or(0) != 0 {
            out = ascii_case(&bytes, false);
        } else if flag(10).unwrap_or(0) != 0 {
            out = fullwidth_case(&bytes, false);
        } else if flag(11).unwrap_or(0) != 0 || flag(12).unwrap_or(0) != 0 {
            return Err(diag::unimplemented(
                code::rt::CMD_UNIMPLEMENTED,
                Boundary::Script,
                "VARACT HANTOZEN/ZENTOHAN 依赖引擎二进制转换表(0x808840/0x5B5170/0x5B5230),未移植(DIV-RT-013)",
            )
            .into());
        } else {
            return Err(diag::unimplemented(
                code::rt::CMD_UNIMPLEMENTED,
                Boundary::Script,
                "VARACT 无已识别的操作项(INIT/G_* 无手册页;DIV-RT-011)",
            )
            .into());
        }
        self.write_target(st, &target, &Value::Str(out))?;
        Ok(StepOutcome::Continue)
    }

    fn read_lvalue(&self, lval: &crate::expr::LValue) -> Result<Value, yu_log::Diag> {
        self.store.read(lval.var, &lval.indices)
    }

    /// `VARACT[INIT + G_*]`:按 `ysv.ybn` 镜像把全局变量整槽重建。
    ///
    /// 算法逐条照旁线 §3.26 ④(0.495 处理体 `0x455b60`):
    /// ① 只收 `kind == 1`(全局);② 按 `(值类型, 组号)` 查掩码,位为 0 则否决;
    /// ③ 组号超出 1..=4 时无否决项 ⇒ 照常恢复;④ 整槽重建(声明 + 类型 + 维数 + 初值)。
    fn restore_globals(
        &mut self,
        st: &ExecState,
        masks: &[u8; 3],
    ) -> Result<(), VmError> {
        let _ = st;
        let records: Vec<_> = self
            .tables
            .boot
            .vars
            .records
            .iter()
            .filter(|r| r.kind == 1)
            .cloned()
            .collect();
        for r in records {
            // 掩码位 = 组号 - 1;组号 0 或 >4 没有否决项。
            if (1..=4).contains(&r.group) {
                let family = match r.value_type {
                    2 => 1usize,
                    3 => 2usize,
                    _ => 0usize,
                };
                let bit = 1u8 << (r.group - 1);
                if masks[family] & bit == 0 {
                    continue;
                }
            }
            let Some(ty) = r.elem_type() else { continue };
            if r.var_id < self.sys_count {
                continue; // 系统变量镜像:由运行核按语义计算
            }
            let dims: Vec<usize> = r.dims.iter().map(|&d| d as usize).collect();
            let init = r.init_value().unwrap_or_else(|| ty.default_value());
            // 整槽重建(引擎 `0x455d8a`:写描述符 + 重建存储 + 写初值)。
            self.store
                .declare(VarRef::new(VarSpace::for_elem(ty), r.var_id), ty, &dims, &init);
        }
        Ok(())
    }

    /// 改变一维数组长度并保留已有元素(见 [`crate::value::VariableStore::resize_1d`])。
    fn resize_1d(&mut self, var: VarRef, _ty: ElemType, new_len: usize) -> Result<(), VmError> {
        if new_len == 0 {
            // 引擎对 0 长度的处理未取证:保留 1 个空槽(手册例 9/10 均 >=1)。
            self.store.resize_1d(var, 0)?;
            return Ok(());
        }
        self.store.resize_1d(var, new_len)?;
        Ok(())
    }

    fn shift_1d(&mut self, var: VarRef, pos: usize, push: bool) -> Result<(), VmError> {
        let Some((ty, dims)) = self.store.shape(var) else {
            return Err(diag::unimplemented(
                code::rt::VAR_UNDECLARED,
                Boundary::Script,
                format!("PUSH/POP 目标未声明 {}", var.display()),
            )
            .into());
        };
        if dims.len() != 1 {
            return Err(diag::fatal(
                code::rt::VAR_SHAPE,
                Boundary::Script,
                "PUSH/POP 只支持一维数组",
            )
            .into());
        }
        let n = dims[0];
        let name: Vec<Value> = (0..n as i64)
            .map(|i| self.store.read(var, &[i]).unwrap_or_else(|_| ty.default_value()))
            .collect();
        let mut out = name.clone();
        if n > 0 {
            let p = pos.min(n - 1);
            if push {
                for i in (p + 1..n).rev() {
                    out[i] = name[i - 1].clone();
                }
                out[p] = ty.default_value();
            } else {
                for i in p..n - 1 {
                    out[i] = name[i + 1].clone();
                }
                out[n - 1] = ty.default_value();
            }
        }
        for (i, v) in out.into_iter().enumerate() {
            self.store.write(var, &[i as i64], &v)?;
        }
        Ok(())
    }

    // ---------------------------------------------------------- 数学

    /// `MATH`:手册 `math/math.html`;引擎 §3.20。
    fn op_math(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let mut target: Option<VarRef> = None;
        let mut set: f64 = 0.0;
        let mut set2: f64 = 0.0;
        let mut flags: Vec<u8> = Vec::new();
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            match w.rec.kind {
                0 => target = Some(self.eval_lvalue(st, &w.payload)?.var),
                1 => set = self.eval_value(st, &w.payload)?.as_f64().unwrap_or(0.0),
                2 => set2 = self.eval_value(st, &w.payload)?.as_f64().unwrap_or(0.0),
                3 => {
                    return Err(diag::unimplemented(
                        code::rt::CMD_UNIMPLEMENTED,
                        Boundary::Script,
                        "MATH[SETSTR] 字符串运算未取证(手册页只列数值函数;DIV-RT-011)",
                    )
                    .into())
                }
                slot => {
                    let v = self.eval_value(st, &w.payload)?;
                    if v.as_int().unwrap_or(0) != 0 {
                        flags.push(slot);
                    }
                }
            }
        }
        let target = target.ok_or_else(|| {
            diag::fatal(code::rt::CMD_SLOT, Boundary::Script, "MATH 缺 LET")
        })?;
        let Some(slot) = flags.first().copied() else {
            self.store.write(target, &[], &Value::Float(set))?;
            return Ok(StepOutcome::Continue);
        };
        // 参数名→函数由工程 ysc 给出(参数表顺序即手册函数表)。
        let fname = self.param_name(cmd.opcode, slot).unwrap_or_default();
        let r = match fname.as_str() {
            "SIN" => set.sin(),
            "ASIN" => set.asin(),
            "SINH" => set.sinh(),
            "COS" => set.cos(),
            "ACOS" => set.acos(),
            "COSH" => set.cosh(),
            "TAN" => set.tan(),
            "ATAN" => set.atan(),
            "ATAN2" => set.atan2(set2),
            "TANH" => set.tanh(),
            "SQRT" => set.sqrt(),
            "ABS" => set.abs(),
            "EXP" => set.exp(),
            "LOG" => set.ln(),
            "LOG10" => set.log10(),
            "POW" => set.powf(set2),
            "RND" => {
                return Err(diag::unimplemented(
                    code::rt::CMD_UNIMPLEMENTED,
                    Boundary::Script,
                    "MATH[RND] 引擎用 MT19937(§3.20),种子来源未取证;需注入 RNG(DIV-RT-014)",
                )
                .into())
            }
            other => {
                return Err(diag::fatal(
                    code::rt::CMD_SLOT,
                    Boundary::Script,
                    format!("MATH 未声明参数 `{other}`"),
                )
                .into())
            }
        };
        self.store.write(target, &[], &Value::Float(r))?;
        Ok(StepOutcome::Continue)
    }

    // ---------------------------------------------------------- 任务

    /// `TASK`:手册 `control/task.html`。
    ///
    /// **无 ID = 执行者自身**(手册:「タスクＩＤを指定しなかった場合、この命令を
    /// 実行した自身のタスクが対象になります」;REA `0x45074a`:
    /// `MOV EBP, [0x801720]`(当前任务记录)→ 以 `[EBP+0x34]` 为操作对象)。
    fn op_task(
        &mut self,
        st: &mut ExecState,
        task: &crate::task::TaskNode,
        cmd: &Cmd,
    ) -> Result<StepOutcome, VmError> {
        let mut id: Option<String> = None;
        let mut enabled = true;
        let mut priority: i64 = 1;
        let mut label: Option<crate::tables::Label> = None;
        // 任务局部寄存器初值(TASK 参数序号 4..13=TINT..TINT10 / 14..23=TFLT.. /
        // 24..33=TSTR..;0.495 ycd 实测)。手册 `control/task.html` 未文档化这些
        // 关键字,但官方 ERIS 源码给出完整生产者/消费者对:
        //   生产者 `es_ERIS.yst:524`:`TASK[ID=… #=es.SS.FADE.TASK.MAIN Z=… TINT=@_PINT(1)]`
        //   消费者 `es_ERIS.yst:533`:任务体 `\es.CGTFADEOUT(@_TINT(1))`
        //   (stdlib `es._task` 同型:TINT..TINT4 → `@_TINT(1..4)`)。
        // 引擎侧任务记录 `task+0x3d0`(旁线 §3.16)。
        // 引擎建任务时先把 T* 寄存器逐槽清零(`0x450916..0x450943`:
        // `+0x348/+0x34c`=TFLT、`+0x3d0/+0x3d4`=TINT、`+0x458`=TSTR),
        // 再按 TINT/TFLT/TSTR 参数落位 ⇒ **未传的槽为 0,不继承调用者**。
        let mut tints = [0i64; 16];
        let mut tflts = [0f64; 16];
        let mut tstrs: [Vec<u8>; 16] = std::array::from_fn(|_| Vec::new());
        let mut script_pos: Option<(u32, u32)> = None;
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            match w.rec.kind {
                0 => {
                    if let Value::Str(bytes) = self.eval_value(st, &w.payload)? {
                        id = Some(cp932_lossy(&bytes));
                    }
                }
                1 => enabled = self.eval_value(st, &w.payload)?.as_int().unwrap_or(1) != 0,
                3 => priority = self.eval_value(st, &w.payload)?.as_int().unwrap_or(1),
                slot @ 4..=13 => {
                    tints[(slot - 4) as usize] =
                        self.eval_value(st, &w.payload)?.as_int().unwrap_or(0);
                }
                slot @ 14..=23 => {
                    tflts[(slot - 14) as usize] =
                        self.eval_value(st, &w.payload)?.as_f64().unwrap_or(0.0);
                }
                slot @ 24..=33 => {
                    if let Value::Str(bytes) = self.eval_value(st, &w.payload)? {
                        tstrs[(slot - 24) as usize] = bytes;
                    }
                }
                // SCRIPTPOS(手册 `control/task.html` 例 4:位置情報の上書き =
                // 事実上のスクリプト位置ロード)。blob 由 TASKINFO[SCRIPTPOS]
                // 产出,脚本视为不透明数据;本实现 blob = [script_id u32][pc u32]。
                35 => {
                    if let Value::Str(bytes) = self.eval_value(st, &w.payload)? {
                        if bytes.len() == 8 {
                            script_pos = Some((
                                u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
                                u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
                            ));
                        }
                    }
                }
                34 => {
                    if let Value::Str(bytes) = self.eval_value(st, &w.payload)? {
                        label = Some(self.find_label(&bytes).ok_or_else(|| {
                            diag::fatal(
                                code::rt::LABEL,
                                Boundary::Script,
                                format!("TASK 入口标签不存在:{}", cp932_lossy(&bytes)),
                            )
                        })?);
                    }
                }
                slot => {
                    return Err(diag::unimplemented(
                        code::rt::CMD_UNIMPLEMENTED,
                        Boundary::Script,
                        format!("TASK 参数序号 {slot}(SCRIPTPOS/TEXTPOS/RESTART 等)未实现(DIV-RT-015)"),
                    )
                    .into())
                }
            }
        }
        // 缺省目标 = 执行者自身(手册;REA 0x45074a 用当前任务记录)。
        let target_id = id.unwrap_or_else(|| task.id.clone());
        match label {
            Some(label) => {
                let seed = TaskSeed {
                    id: target_id,
                    script_id: label.script_id as u32,
                    pc: label.pc,
                    enabled,
                    priority,
                    main: false,
                };
                let pos = script_pos.unwrap_or((seed.script_id, seed.pc));
                let mut fresh = ExecState::new(pos.0, pos.1);
                fresh.task_ints = tints;
                fresh.task_floats = tflts;
                fresh.task_strs = tstrs.clone();
                if seed.id == task.id {
                    // 目标 = 自身:引擎 `TASK` 直接写当前任务记录的 pc/脚本号
                    // (`0x4508bc..0x4508d9`),不是另建一条执行流。这里同样改写
                    // 当前执行位(块/循环/调用栈随新入口重置)。
                    *st = fresh;
                } else {
                    // 同 ID 重配置 = 换入口(引擎复用任务记录并重载脚本表)。
                    self.exec.insert(seed.id.clone(), fresh);
                }
                self.emit(VmEvent::TaskSpawn {
                    frame: self.frame(),
                    id: seed.id.clone(),
                    script: seed.script_id,
                    pc: seed.pc,
                    enabled,
                    main: false,
                });
                self.push_effect(Effect::SpawnTask(seed));
                self.push_effect(Effect::RequestExtraFrame);
            }
            None => {
                // SCRIPTPOS 无 # 形态(手册例 4:对既有任务做「位置ロード」)。
                if let Some((sid, pc)) = script_pos {
                    let target = target_id.clone();
                    if target == task.id {
                        // 目标 = 自身:直接改当前执行位(引擎 `0x450fe0` 走同一记录)。
                        st.script_id = sid;
                        st.pc = pc;
                        st.if_stack.clear();
                        st.loops.clear();
                    } else if let Some(target_st) = self.exec.get_mut(&target) {
                        target_st.script_id = sid;
                        target_st.pc = pc;
                        target_st.if_stack.clear();
                        target_st.loops.clear();
                    }
                }
                self.push_effect(Effect::SetEnabled(target_id, enabled));
            }
        }
        Ok(StepOutcome::Continue)
    }

    /// `TASKEND[ID]`:手册 `control/taskend.html`(缺省 = 自身)。
    ///
    /// **立即生效**(REA 0.495 `0x4521dc`):
    /// - 无 `ID`:取 `[0x801720]`(当前任务记录)→ `CALL 0x452364`(链表摘除):
    ///   `0x45237f..0x4523b9` 按 `+8/+0xc` 前后指针从链中解链并 `DEC [pool+0x20]`;
    ///   `0x452223: MOV EAX, 0xffffffff` —— 返回非 0 = 本任务本帧让出,泵立即停。
    ///   因此同一条命令之后的 `LET` 不执行,`TASKINFO EXIST` 立即为 0,
    ///   同 ID 再建从新标签开始(旧执行位已随摘链丢弃)。
    /// - 指定 `ID`:查名字表;命中他人 → 摘链 + 返回 0(继续执行);
    ///   命中自身 → 同无 ID 路径,返回 -1。
    fn op_taskend(
        &mut self,
        st: &mut ExecState,
        task: &crate::task::TaskNode,
        cmd: &Cmd,
    ) -> Result<StepOutcome, VmError> {
        let mut id: Option<String> = None;
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            if w.rec.kind == 0 {
                if let Value::Str(bytes) = self.eval_value(st, &w.payload)? {
                    id = Some(cp932_lossy(&bytes));
                }
            }
        }
        let target = id.unwrap_or_else(|| task.id.clone());
        if target == task.id {
            // 自身:摘链由调度器在 Finish 分支完成(节点随帧尾 retain 移除),
            // 执行位在 step_command 的 Finish 路径丢弃。
            return Ok(StepOutcome::Finish);
        }
        self.push_effect(Effect::FinishTask(target));
        Ok(StepOutcome::Continue)
    }

    /// `TASKINFO[ID LET EXIST/E/Z/FILE/LINE/IFNEST/GOSUBNEST/LOOPNEST]`:
    /// 手册 `control/taskinfo.html`。
    ///
    /// 取值来源 = **引擎任务记录**(旁线 §3.2):`+0x20` pc、`+0x3c` 脚本号、
    /// `+0x3e` 块深度(base_a)、`+0x244` 调用栈深度。手册逐条:
    /// `LINE`=当前执行行数、`IFNEST`/`GOSUBNEST`/`LOOPNEST`=三种嵌套数、
    /// `FILE`=当前文件名(这里按脚本号给出,名字表在 `yst_list`)、
    /// `E`/`Z`=启用位/优先级、`EXIST`=任务存在。
    ///
    /// 行号:引擎另有「数据区 B 逐 pc 源码位置」(脚本 `+0x38`,本 crate
    /// `ScriptFile::source_position`),此处直接用它 —— 那是编译产物里
    /// 与 pc 一一对应的真实行号,不是推算。
    fn op_taskinfo(
        &mut self,
        st: &mut ExecState,
        task: &crate::task::TaskNode,
        cmd: &Cmd,
    ) -> Result<StepOutcome, VmError> {
        let mut id: Option<String> = None;
        let mut target: Option<crate::expr::LValue> = None;
        let mut ops: Vec<(u8, i64)> = Vec::new();
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            match w.rec.kind {
                0 => {
                    if let Value::Str(bytes) = self.eval_value(st, &w.payload)? {
                        id = Some(cp932_lossy(&bytes));
                    }
                }
                1 => target = Some(self.eval_lvalue(st, &w.payload)?),
                slot => {
                    let v = self.eval_value(st, &w.payload)?;
                    if let Some(n) = v.as_int() {
                        ops.push((slot, n));
                    }
                }
            }
        }
        // 缺省 = 自身(手册:「タスクＩＤを指定しなかった場合、この命令を実行した
        // 自身のタスクが対象」;REA 0x4521dc/0x4524xx 同样取当前任务记录)。
        let id = id.unwrap_or_else(|| task.id.clone());
        // 引擎从任务链表取记录;泵期间调度器被 `Vm::tick` 借出,故读同步过来的投影。
        let node = self
            .task_view()
            .iter()
            .find(|n| n.id == id && !n.finished)
            .cloned();
        // 该任务的执行位(自身就在这里;其它任务在 exec 表里)。
        let exec = if id == task.id {
            Some(st.clone())
        } else {
            self.exec_of(&id).cloned()
        };
        let Some((slot, _)) = ops.iter().find(|(_, v)| *v != 0).copied() else {
            return Ok(StepOutcome::Continue);
        };
        let value = match slot {
            2 => Value::Int(node.is_some() as i64),
            3 => Value::Int(node.as_ref().map(|n| n.enabled as i64).unwrap_or(0)),
            5 => Value::Int(node.as_ref().map(|n| n.priority).unwrap_or(0)),
            6 => {
                // FILE:当前脚本文件名(手册)→ 由 `yst_list` 的名字表给出。
                let sid = exec.as_ref().map(|e| e.script_id).unwrap_or(0);
                Value::Int(sid as i64)
            }
            7 => Value::Int(exec.as_ref().map(|e| e.script_id as i64).unwrap_or(-1)),
            8 => {
                // LINE:当前执行行数(数据区 B 的逐 pc 源码位置)。
                let (sid, pc) = exec
                    .as_ref()
                    .map(|e| (e.script_id, e.pc.saturating_sub(1)))
                    .unwrap_or((0, 0));
                let line = self
                    .tables
                    .script(sid)
                    .and_then(|s| s.source_position(pc as usize))
                    .map(|v| v as i64)
                    .unwrap_or(-1);
                Value::Int(line)
            }
            16 => Value::Int(exec.as_ref().map(|e| e.if_stack.len() as i64).unwrap_or(0)),
            17 => Value::Int(
                exec.as_ref()
                    .map(|e| e.frames.len().saturating_sub(1) as i64)
                    .unwrap_or(0),
            ),
            18 => Value::Int(exec.as_ref().map(|e| e.loops.len() as i64).unwrap_or(0)),
            // SCRIPTPOS(手册 `control/taskinfo.html`:現在実行されている
            // スクリプト位置情報)。脚本视为不透明 blob;本实现 = 8 字节
            // [script_id u32 LE][pc u32 LE],与 `TASK[SCRIPTPOS=…]` 配对往返。
            19 => {
                let (sid, pc) = exec
                    .as_ref()
                    .map(|e| (e.script_id, e.pc))
                    .unwrap_or((0, 0));
                let mut b = Vec::with_capacity(8);
                b.extend_from_slice(&sid.to_le_bytes());
                b.extend_from_slice(&pc.to_le_bytes());
                Value::Str(b)
            }
            other => {
                return Err(diag::unimplemented(
                    code::rt::CMD_UNIMPLEMENTED,
                    Boundary::Script,
                    format!("TASKINFO 参数序号 {other} 未实现(DIV-RT-015)"),
                )
                .into())
            }
        };
        if let Some(t) = target {
            self.write_target(st, &t, &value)?;
        }
        Ok(StepOutcome::Continue)
    }

    /// `SYSTEMINFO[SCRIPTFILE/SCRIPTNUM/TEXTNUM/EMOTE/NO/LET]`。
    ///
    /// **手册无页**(手册 `system/` 只有 fps/input/mouse;全目录 `grep SYSTEMINFO`
    /// 零命中)。语义来自引擎处理体 v494 `0x450478` 的逐槽反汇编(旧线
    /// `yuris-vm/src/exec/sysinfo.rs` 记录,本实现按同一结论重写):
    ///
    /// | 槽 | 值 |
    /// |---|---|
    /// | `SCRIPTFILE`(0) | `NO`(1 基) → `yst_list` 里的源文件名(越界写空串) |
    /// | `SCRIPTNUM`(1) | 脚本总数 = `yst_list` 条目数 |
    /// | `TEXTNUM`(2) | `NO` → 该脚本的文本数(越界写 0) |
    /// | `EMOTE`(3) | E-mote 运行时标志(宿主能力) |
    /// | `NO`(4) | 1 基下标 |
    /// | `LET`(5) | 写回目标 |
    ///
    /// 三层状态:①手册缺(挂 DIV-RT-011);②ERIS 语料 5 次(全为 `NO`+`LET`);
    /// ③引擎处理体地址级反汇编(见上)。**按军规本应 Unimplemented**,
    /// 但这条是 ERIS 启动链的硬依赖(缺它 → `@mTLine=0` → 除零 → 主线终止),
    /// 且引擎证据完整 —— 故实现并明确登记"缺手册页"这一事实。
    fn op_systeminfo(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let mut target: Option<crate::expr::LValue> = None;
        let mut no: Option<i64> = None;
        let mut want = [false; 4]; // SCRIPTFILE / SCRIPTNUM / TEXTNUM / EMOTE
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            match w.rec.kind {
                5 => target = Some(self.eval_lvalue(st, &w.payload)?),
                4 => {
                    no = self.eval_value(st, &w.payload)?.as_int();
                }
                0..=3 => {
                    let v = self.eval_value(st, &w.payload)?;
                    want[w.rec.kind as usize] = v.as_int().unwrap_or(0) != 0;
                }
                _ => {}
            }
        }
        let count = self.tables().boot.scripts.len() as i64;
        let mut answers: Vec<Value> = Vec::new();
        if want[1] {
            answers.push(Value::Int(count));
        }
        if want[2] {
            // `TEXTNUM`:该脚本的文本条数(`yst_list` 记录第 3 个 u32,
            // 实测与 `TEXT` 条目数一致;引擎同源:处理体查 `0x802b2c[n-1]`)。
            let n = match no {
                Some(n) if n >= 1 && n <= count => self
                    .tables()
                    .boot
                    .scripts
                    .text_count((n - 1) as u16)
                    .map(|c| c as i64)
                    .unwrap_or(0),
                _ => 0, // 越界 → 引擎写 0
            };
            answers.push(Value::Int(n));
        }
        if want[0] {
            let name = match no {
                Some(n) if n >= 1 && n <= count => self
                    .tables()
                    .boot
                    .scripts
                    .path_of((n - 1) as u16)
                    .map(|p| p.to_vec())
                    .unwrap_or_default(),
                _ => Vec::new(),
            };
            answers.push(Value::Str(name));
        }
        if want[3] {
            answers.push(Value::Int(0));
        }
        if let Some(t) = target {
            for v in &answers {
                self.write_target(st, &t, v)?;
            }
        }
        Ok(StepOutcome::Continue)
    }

    // ---------------------------------------------------------- 文本 / 其它

    /// `TEXT`:手册 `text/text.html`(属性设置;可见文本由 `_` 输出)。
    fn op_text(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        // 参数全部是配置项;手册页逐项给出语义。运行核只记录事件,
        // 不实现显示(显示层归宿主域)。
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let _ = self.eval_value(st, &w.payload);
        }
        self.emit(VmEvent::SystemCall {
            frame: self.frame(),
            name: "TEXT".to_string(),
            args: Vec::new(),
        });
        Ok(StepOutcome::Continue)
    }

    /// `_`(文本输出通道):把式子的值作为文本显示。
    ///
    /// 手册 `basic/basic.html` ●テキスト表示:`_[@BANANA]` / `_[2+3*4]`
    /// ——下划线 + 式,值直接进入文本流。语料 33 处全部是 YSCom 把改行宏
    /// `\r` 展开成的 `_[ $_TEXT(239)+$_TEXT(240) ]`(userscript 演示实测);
    /// `$_TEXT` = 引擎自管显示缓冲(手册 `basic/svariable.html`),运行核
    /// 照常求值即可。撤销 DIV-RT-008 挂账(三层已齐)。
    fn op_text_out(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let v = self.eval_value(st, &w.payload)?;
            let bytes = match v.cast_to_str()? {
                Value::Str(b) => b,
                _ => Vec::new(),
            };
            self.append_text(&bytes);
            self.emit(VmEvent::Text { frame: self.frame(), id: String::new(), bytes });
        }
        Ok(StepOutcome::Continue)
    }

    /// `WORD`:一个文本段(YSCom 把一行剧本文本编译为若干 WORD + 行尾
    /// `RETURNCODE`;行内 `\r` 编译为 `_`)。
    ///
    /// 三层:①手册 `basic/basic.html` ●テキスト表示(全角文本行即显示文本);
    /// ②语料 464 处,载荷全部是 **CP932 原始字节**(无表达式壳,137 文件实测);
    /// ③RxYuris `src/YSTB_TextEditor_V5.cpp` 以 WORD 为文本提取单元、按
    /// YSCM 名字动态取 opcode,且 YSTL `text_count` == 每脚本 WORD 数
    /// (全部 YU-RIS 5.x 商业作品实测通过)。
    fn op_word(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let _ = st;
        let Some(w) = cmd.windows.first() else {
            return Err(diag::fatal(
                code::rt::CMD_WINDOWS,
                Boundary::Script,
                "WORD 缺文本窗口",
            )
            .into());
        };
        self.append_text(&w.payload);
        self.emit(VmEvent::Text {
            frame: self.frame(),
            id: String::new(),
            bytes: w.payload.clone(),
        });
        Ok(StepOutcome::Continue)
    }

    /// `RETURNCODE`:一行文本的行尾标记(改行)。
    ///
    /// ysc/ycd 双表都声明它**无参数**;唯一窗口的 `kind` 是编译器写入的
    /// **本脚本内文本行计数**(1 起逐行 +1;137 文件实测——0 参数命令的
    /// 窗口 kind 不可能是参数序号,只能是行号语义)。运行核记录事实,
    /// 换行渲染归宿主域。
    ///
    /// **让出点**(任务书 E R3;REA 0.495 `0x44c390`):
    /// - `0x44c451: MOV byte ptr [ESI+9], 1` —— 置 `task+9`(让出标志);
    /// - `0x44c455: MOV EAX, 0xffffffff` —— 处理体返回非 0;
    /// - 泵侧 `0x40442e: MOVSX ECX, byte ptr [ESI+0x9]; TEST; JNZ next_task`
    ///   —— 下一次取任务时该任务直接跳过,本 vsync 链结束于此处;
    /// - 帧内唤醒:`FUN_004046fc` 在输入/超时满足时把 `task+9` 清零。
    ///
    /// 这条**否定**了"文本段(`WORD`/`_`)各自让出"的读法:`WORD`
    /// (`0x458d44`)与 `_`(`0x4567c0`)成功路径都 `XOR EAX,EAX; RET`,
    /// 只有 `RETURNCODE` 置标志并返回 -1。故一行 = `WORD…_[…]…WORD` 全部
    /// 在同一次泵内执行完,行尾才让出(与手册 `basic/task.html`
    /// 「表示テキスト…が来ると…一旦休憩」的行级粒度一致:一行文本执行完才休憩)。
    fn op_returncode(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let _ = st;
        let line = cmd.windows.first().map(|w| w.rec.kind as u32).unwrap_or(0);
        self.emit(VmEvent::ReturnCode { frame: self.frame(), line });
        Ok(StepOutcome::TextYield)
    }

    /// `LABELINFO[# LET EXIST]`:手册 `other/labelinfo.html`。
    fn op_labelinfo(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let mut target: Option<VarRef> = None;
        let mut name: Option<Vec<u8>> = None;
        let mut exist = false;
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            match w.rec.kind {
                0 => {
                    if let Value::Str(b) = self.eval_value(st, &w.payload)? {
                        name = Some(b);
                    }
                }
                1 => target = Some(self.eval_lvalue(st, &w.payload)?.var),
                2 => exist = self.eval_value(st, &w.payload)?.as_int().unwrap_or(0) != 0,
                _ => {}
            }
        }
        if exist {
            if let (Some(name), Some(t)) = (name, target) {
                let found = self.find_label(&name).is_some();
                self.store.write(t, &[], &Value::Int(found as i64))?;
            }
        }
        Ok(StepOutcome::Continue)
    }

    // ---------------------------------------------------------- 辅助

    fn level_param(&mut self, st: &ExecState, cmd: &Cmd) -> Result<usize, VmError> {
        match cmd.windows.first() {
            Some(w) if !w.payload.is_empty() => {
                let v = self.eval_value(st, &w.payload)?;
                Ok(v.as_int().unwrap_or(1).max(0) as usize)
            }
            _ => Ok(1),
        }
    }

    fn label_from_window(
        &mut self,
        st: &ExecState,
        cmd: &Cmd,
        index: usize,
    ) -> Result<crate::tables::Label, VmError> {
        let w = cmd.windows.get(index).ok_or_else(|| {
            windows_error(cmd, "缺少标签窗口")
        })?;
        let value = self.eval_value(st, &w.payload)?;
        let Value::Str(name) = value else {
            return Err(diag::fatal(
                code::rt::VAR_TYPE,
                Boundary::Script,
                format!("标签参数必须是字符串,得到 {}", value.type_name()),
            )
            .into());
        };
        self.find_label(&name).ok_or_else(|| {
            let d = diag::fatal(
                code::rt::LABEL,
                Boundary::Script,
                format!("标签不存在:{}", cp932_lossy(&name)),
            );
            VmError::Fatal(d)
        })
    }

    fn jump_to(
        &mut self,
        st: &mut ExecState,
        to_script: u32,
        to_pc: u32,
        base_a: u8,
        kind: JumpKind,
        from_pc: u32,
    ) -> Result<(), VmError> {
        self.ensure_script(to_script)?;
        let script = self
            .tables
            .script(to_script)
            .ok_or_else(|| {
                diag::fatal(
                    code::rt::INTERNAL,
                    Boundary::Script,
                    format!("脚本 {to_script} 刚装载却取不到"),
                )
            })?;
        if to_pc as usize >= script.group_count() {
            return Err(diag::fatal(
                code::rt::LABEL,
                Boundary::Script,
                format!("跳转目标 s{to_script} pc={to_pc} 超出脚本 {} 组", script.group_count()),
            )
            .into());
        }
        self.emit(VmEvent::Jump {
            frame: self.frame(),
            script: st.script_id,
            pc: from_pc,
            to_pc,
            to_script,
            kind,
        });
        st.script_id = to_script;
        st.pc = to_pc;
        st.base_a = base_a;
        Ok(())
    }
}

// ---------------------------------------------------------------- 纯函数

fn flags_get(flags: &[(u8, i64)], slot: u8) -> Option<i64> {
    flags.iter().find(|(s, _)| *s == slot).map(|(_, v)| *v)
}

/// VARINFO SEARCH 的针值:`INT=`/`FLT=`/`STR=`(手册例 5/6/7)。
fn array_needle(
    vm: &mut Vm,
    cmd: &Cmd,
    st: &ExecState,
    set: VarRef,
) -> Result<Option<Value>, VmError> {
    let ty = vm.store.shape(set).map(|s| s.0);
    for w in &cmd.windows {
        if w.rec.length == 0 {
            continue;
        }
        let is_needle = match (ty, w.rec.kind) {
            (Some(ElemType::Str), 19) => true,
            (Some(ElemType::Float), 18) => true,
            (Some(ElemType::Int), 17) => true,
            _ => false,
        };
        if is_needle {
            return Ok(Some(vm.eval_value(st, &w.payload)?));
        }
    }
    let _ = cmd;
    Ok(None)
}

fn windows_error(cmd: &Cmd, detail: &str) -> yu_log::Diag {
    diag::fatal(
        code::rt::CMD_WINDOWS,
        Boundary::Script,
        format!("{}: {detail}(实给 {} 个窗口)", cmd.name, cmd.windows.len()),
    )
}

/// 参数名尾部编号:`PINT2` → 2,`PINT` → 1。
pub(crate) fn param_index_of(name: &str) -> Option<usize> {
    let digits: String = name
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    if digits.is_empty() {
        Some(1)
    } else {
        digits.parse().ok()
    }
}

fn coerce_param(name: &str, v: Value) -> Value {
    if name.starts_with("PFLT") || name.starts_with("RFLT") {
        Value::Float(v.as_f64().unwrap_or(0.0))
    } else if name.starts_with("PSTR") || name.starts_with("RSTR") {
        match v {
            Value::Str(_) => v,
            other => Value::Str(other.to_event_repr().into_bytes()),
        }
    } else if name.starts_with("PINT") || name.starts_with("RINT") {
        Value::Int(v.as_int().unwrap_or_else(|| v.as_f64().unwrap_or(0.0) as i64))
    } else {
        v
    }
}

/// 复合赋值码(语料实测 0..8;`LET` 窗口 `slot` 高字节)。
fn compound(code: u8, cur: &Value, rhs: &Value) -> Result<Value, yu_log::Diag> {
    Ok(match code {
        0 => rhs.clone(),
        1 => cur.add(rhs)?,
        2 => cur.sub(rhs)?,
        3 => cur.mul(rhs)?,
        4 => cur.div(rhs)?,
        5 => cur.rem(rhs)?,
        6 => cur.bitand(rhs)?,
        7 => cur.bitor(rhs)?,
        8 => cur.bitxor(rhs)?,
        other => {
            return Err(diag::fatal(
                code::rt::CMD_SLOT,
                Boundary::Script,
                format!("LET 复合赋值码 {other} 超出语料观测范围 0..8"),
            ))
        }
    })
}

/// 声明窗口的目标变量(单条变量指令或数组引用)。
fn target_var(w: &CmdWindow) -> Result<VarRef, yu_log::Diag> {
    let instrs = crate::expr::decode(&w.payload)?;
    let first = instrs
        .iter()
        .find(|i| matches!(i.op, 0x48 | 0x56 | 0x76))
        .ok_or_else(|| {
            diag::fatal(
                code::rt::EXPR_MALFORMED,
                Boundary::Script,
                "声明窗口不含变量引用",
            )
        })?;
    if first.payload.len() != 3 {
        return Err(diag::fatal(
            code::rt::EXPR_MALFORMED,
            Boundary::Script,
            "变量引用载荷不是 3 字节",
        ));
    }
    Ok(VarRef::new(
        VarSpace::from_prefix(first.payload[0]),
        u16::from_le_bytes([first.payload[1], first.payload[2]]),
    ))
}

/// CP932 字符串的字符数(前导字节吞掉后继)。
pub fn str_char_count(bytes: &[u8]) -> usize {
    let mut i = 0;
    let mut n = 0;
    while i < bytes.len() {
        if is_lead(bytes[i]) && i + 1 < bytes.len() {
            i += 2;
        } else {
            i += 1;
        }
        n += 1;
    }
    n
}

fn is_lead(b: u8) -> bool {
    (0x81..=0x9F).contains(&b) || (0xE0..=0xFC).contains(&b)
}

/// 按字符位置取子串(1 起算;手册 `VARACT` 例 5)。
fn copy_chars(bytes: &[u8], pos: usize, len: usize) -> Vec<u8> {
    let n = char_spans(bytes).len();
    let start = pos.saturating_sub(1).min(n);
    slice_chars(bytes, start, start + len)
}

/// 内部辅助:trait 不存在,改用函数式写法。
fn char_spans(bytes: &[u8]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let step = if is_lead(bytes[i]) && i + 1 < bytes.len() { 2 } else { 1 };
        out.push((i, i + step));
        i += step;
    }
    out
}

fn slice_chars(bytes: &[u8], start: usize, end: usize) -> Vec<u8> {
    let spans = char_spans(bytes);
    if start >= end || start >= spans.len() {
        return Vec::new();
    }
    let end = end.min(spans.len());
    bytes[spans[start].0..spans[end - 1].1].to_vec()
}

/// 挖掉 `[pos, pos+len)` 字符,保留头尾(手册 `VARACT` 例 6)。
fn cut_chars(bytes: &[u8], pos: usize, len: usize) -> Vec<u8> {
    let n = char_spans(bytes).len();
    let start = pos.saturating_sub(1).min(n);
    let end = (start + len).min(n);
    let mut out = slice_chars(bytes, 0, start);
    out.extend_from_slice(&slice_chars(bytes, end, n));
    out
}

/// 半角 ASCII 大小写(手册:UPPER/LOWER 只碰半角;引擎 §3.26)。
fn ascii_case(bytes: &[u8], upper: bool) -> Vec<u8> {
    bytes
        .iter()
        .map(|&b| {
            if upper {
                b.to_ascii_uppercase()
            } else {
                b.to_ascii_lowercase()
            }
        })
        .collect()
}

/// 半角 + 全角拉丁字母大小写(手册 UPPER2/LOWER2)。
///
/// CP932 全角:Ａ-Ｚ = `0x82 0x60..0x79`,ａ-ｚ = `0x82 0x81..0x9A`。
fn fullwidth_case(bytes: &[u8], upper: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == 0x82 {
            let t = bytes[i + 1];
            let mapped = if upper && (0x81..=0x9A).contains(&t) {
                Some(0x82u8)
            } else if !upper && (0x60..=0x79).contains(&t) {
                Some(0x81u8)
            } else {
                None
            };
            if let Some(marker) = mapped {
                let tt = if marker == 0x82 {
                    0x60 + (t - 0x81)
                } else {
                    0x81 + (t - 0x60)
                };
                out.push(0x82);
                out.push(tt);
                i += 2;
                continue;
            }
        }
        let b = bytes[i];
        out.push(if upper {
            b.to_ascii_uppercase()
        } else {
            b.to_ascii_lowercase()
        });
        i += 1;
    }
    out
}

/// CP932 → 显示文本(lossy;仅事件/诊断)。
fn cp932_lossy(bytes: &[u8]) -> String {
    // 不引入码表:非 ASCII 字节按 `\xNN` 显示,保证确定性与可逆诊断。
    let mut s = String::new();
    for &b in bytes {
        if (0x20..0x7F).contains(&b) {
            s.push(b as char);
        } else {
            s.push_str(&format!("\\x{b:02x}"));
        }
    }
    s
}

/// 命令名 → 域(仅用于事件归属;命令名来自工程表)。
fn domain_of(name: &str) -> Domain {
    if name.starts_with("CG") {
        Domain::Graphics
    } else if name.starts_with("SOUND") || name == "MIDI" || name == "MIDIEND" || name == "MIDIINFO" {
        Domain::Sound
    } else if name == "SAVE" || name == "LOAD" || name.starts_with("SAVEINFO") || name == "SAVEEND" {
        Domain::Save
    } else if name.starts_with("FILE") {
        Domain::File
    } else if name.starts_with("MENU") {
        Domain::Menu
    } else if name.starts_with("WINDOW") {
        Domain::Window
    } else if name.starts_with("MOVIE") {
        Domain::Movie
    } else if name.starts_with("FLASH") {
        Domain::Flash
    } else if name.starts_with("EMOTE") {
        Domain::Emote
    } else if name.starts_with("SYSTEM") || name == "INPUT" || name == "MOUSE" {
        Domain::System
    } else if name.starts_with("FONT") {
        // 手册 `font/font.html` / `fontinfo.html`(字体渲染域,归宿主 gfx)。
        Domain::Graphics
    } else {
        Domain::Other
    }
}
