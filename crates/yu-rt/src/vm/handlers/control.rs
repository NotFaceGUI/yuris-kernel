//! 控制流:GOSUB/RETURN/GO/IF/ELSE/LOOP/WAIT/FPS/END 与标签跳转。
//!
//! 由 `handlers/mod.rs` 的命令族拆分而来;只搬运,函数体逐字不变。

use super::helpers::*;
use super::*;

impl Vm {

    // ---------------------------------------------------------- 控制流

    /// `GOSUB`:手册 `control/gosub.html`;引擎 §3.16(参数组 A + 调用栈)。
    pub(super) fn op_gosub(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
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
            let (n, ty) = self.param_plan(cmd.opcode, w.rec.kind);
            let idx = checked_param_index(n, cmd.opcode, w.rec.kind, self, "GOSUB")?;
            let value = self.eval_window(st, w)?;
            frame.args[idx] = Some(ty.coerce(value));
            frame.arg_flags[idx] = true;
        }
        if st.frames.len() > GOSUB_MAX_CALLS {
            return Err(
                crate::task::nesting_error("GOSUB", st.frames.len() - 1, GOSUB_MAX_CALLS).into(),
            );
        }
        // 块/循环栈按调用层隔离:RETURN 时截断回进入前的深度。
        frame.if_depth = st.if_stack.len();
        frame.loop_depth = st.loops.len();
        st.frames.push(frame);
        let from_pc = cmd.pc;
        self.jump_to(
            st,
            label.script_id as u32,
            label.pc,
            label.base_a,
            JumpKind::Gosub,
            from_pc,
        )?;
        Ok(StepOutcome::Continue)
    }

    /// `RETURN`:手册 `control/return.html`;引擎 §3.16(写调用方那一层后弹栈)。
    ///
    /// 手册 `basic/task.html`:`GOSUB` 嵌套不存在时到达 `RETURN` 的任务**自动消失**。
    pub(super) fn op_return(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
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
            // 与 GOSUB 同一守卫:此前 RETURN 缺这条,**`RINT0` 会 usize 下溢、
            // `RINT17+` 会越界 panic**(DIV-RT-034「panic 收口」)。
            let (n, ty) = self.param_plan(cmd.opcode, w.rec.kind);
            let idx = checked_param_index(n, cmd.opcode, w.rec.kind, self, "RETURN")?;
            let value = self.eval_window(st, w)?;
            ret[idx] = Some(ty.coerce(value));
            flags[idx] = true;
        }
        let frame = st.frames.pop().expect("帧数 > 1");
        // 被调层未闭合的 IF/LOOP 不得泄漏给调用方(块结构按调用层独立)。
        st.if_stack.truncate(frame.if_depth);
        st.loops.truncate(frame.loop_depth);
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
    ///
    /// 空标签 = **不跳转**(继续执行下一条)。证据:v474 商业样本
    /// `es_scenarioflow.yst` pc8 的 `GO[#=$es.STARTLABEL]` 在 boot 期
    /// 变量初值为空(YSVR slot 空串),官方引擎仍能启动 ⇒ 官方对空目标
    /// 不按“标签不存在”Fatal;此处按无操作处理,避免误杀整条任务。
    /// 非空但查不到的标签仍走 `rt.label` Fatal。
    pub(super) fn op_go(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let Some(label) = self.label_from_window_opt(st, cmd, 0, true)? else {
            return Ok(StepOutcome::Continue);
        };
        let from_pc = cmd.pc;
        self.jump_to(
            st,
            label.script_id as u32,
            label.pc,
            label.base_a,
            JumpKind::Go,
            from_pc,
        )?;
        Ok(StepOutcome::Continue)
    }

    /// 条件窗口求值(`IF` / `ELSE` 共用策略)。
    ///
    /// 可恢复失败(`Unimplemented` / `Recoverable`,典型 = 未取证的系统变量)
    /// ⇒ 记诊断后按**「假」**处理;`Fatal` 照常抛出(任务终止)。
    /// 策略集中在这里:两处各写一份时曾经分叉过 —— `ELSE` 用 `?` 抛出,
    /// 通用错误处理「留痕后继续下一条」,于是直接落进 else 体。
    pub(super) fn eval_condition(&mut self, st: &mut ExecState, w: &CmdWindow) -> Result<bool, VmError> {
        match self.eval_window(st, w) {
            Ok(v) => Ok(v.truthy()?),
            Err(d) => {
                let recoverable = matches!(
                    d.severity,
                    yu_log::Severity::Unimplemented | yu_log::Severity::Recoverable
                );
                if !recoverable {
                    return Err(d.into());
                }
                self.report(d);
                Ok(false)
            }
        }
    }

    /// `IF`:手册 `control/if.html`;引擎 §3.13(块帧 + 假分支跳 `item[2].length`,
    /// 为 0 则跳块出口)。
    pub(super) fn op_if(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
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
            // 引擎 IF 帧 `+0` = 进入时的 LOOP 深度(0x4462f5)。
            loop_depth: st.loops.len(),
        });
        // 条件求值:可恢复失败(Unimplemented)⇒ 记诊断并按「假」处理;
        // Fatal 照常抛出(任务终止)。
        let truthy = self.eval_condition(st, &cmd.windows[0])?;
        // 临时取证(`YU_SLOT_TRACE`):3DAYS 文本循环断点排查。取证完删除。
        if std::env::var_os("YU_SLOT_TRACE").is_some()
            && (st.script_id == 105 || st.script_id == 104)
        {
            let probe = match (st.script_id, cmd.pc) {
                (105, 29) => {
                    let a = self
                        .store
                        .read(crate::value::VarRef::new(crate::value::VarSpace::At, 5709), &[])
                        .ok();
                    let b = self
                        .store
                        .read(crate::value::VarRef::new(crate::value::VarSpace::At, 5710), &[])
                        .ok();
                    format!(" @5709={a:?} @5710={b:?}")
                }
                (104, _) => {
                    let c = self
                        .store
                        .read(crate::value::VarRef::new(crate::value::VarSpace::Dollar, 5682), &[])
                        .ok();
                    let pos = self
                        .store
                        .read(crate::value::VarRef::new(crate::value::VarSpace::At, 5674), &[])
                        .ok();
                    let live_text = self.text_bytes().len();
                    let live_flags = self.text_flags_ref().len();
                    let copy = self
                        .store
                        .read(crate::value::VarRef::new(crate::value::VarSpace::Dollar, 5590), &[])
                        .ok();
                    let copy_len = match &copy {
                        Some(crate::value::Value::Str(b)) => Some(b.len()),
                        _ => None,
                    };
                    format!(
                        " $5682={c:?} pos={pos:?} live_text={live_text} live_flags={live_flags} copy5590={copy_len:?}"
                    )
                }
                _ => String::new(),
            };
            eprintln!(
                "[if] s{} pc{} truthy={} end={} else={}{probe}",
                st.script_id,
                cmd.pc,
                truthy,
                cmd.windows[2].rec.length,
                cmd.windows[1].rec.length
            );
        }
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
        self.emit_flow(VmEvent::Jump {
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
    pub(super) fn op_else(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
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
        // 条件求值与 `IF` 同策(见 [`Vm::eval_condition`]):可恢复失败
        // (Unimplemented)按「假」处理,先记诊断再跳下一 ELSE/出口。
        //
        // 旧实现用 `?` 抛出 ⇒ 通用错误处理「留痕后继续下一条」,直接落进
        // else 体:实测 0.481 商业包把存档路径改写成 `Documents\…\save\`
        // (分支 2),而正确行为是三个条件全假、保留宿主注入的 `$_SAVE_PATH`。
        let truthy = if cmd.windows[0].payload.is_empty() {
            true
        } else {
            self.eval_condition(st, &cmd.windows[0])?
        };
        if truthy {
            return Ok(StepOutcome::Continue);
        }
        let next = cmd.windows[1].rec.length;
        let to = if next != 0 { next } else { top.end_pc };
        self.emit_flow(VmEvent::Jump {
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
    pub(super) fn op_ifblend(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let top = *st.if_stack.last().ok_or_else(|| {
            diag::fatal(code::rt::CMD_WINDOWS, Boundary::Script, "IFBLEND 无配对 IF")
        })?;
        self.emit_flow(VmEvent::Jump {
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
    pub(super) fn op_ifend(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
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
    pub(super) fn op_ifbreak(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let lv = self.level_param(st, cmd)?;
        self.if_break_to(st, cmd, lv, false)
    }

    /// `IFCONTINUE[LV]`:退 LV 层并跳该层入口 pc(§3.13 opcode 48)。
    pub(super) fn op_ifcontinue(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let lv = self.level_param(st, cmd)?;
        self.if_break_to(st, cmd, lv, true)
    }

    pub(super) fn if_break_to(
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
        self.emit_flow(VmEvent::Jump {
            frame: self.frame(),
            script: st.script_id,
            pc: cmd.pc,
            to_pc: to,
            to_script: st.script_id,
            kind: if cont {
                JumpKind::IfContinue
            } else {
                JumpKind::IfBreak
            },
        });
        st.pc = to;
        Ok(StepOutcome::Continue)
    }

    /// `LOOP`:手册 `control/loop.html`;引擎 §3.13(opcode 56)。
    pub(super) fn op_loop(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
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
            match self.eval_window(st, &cmd.windows[0])? {
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
    pub(super) fn op_loopend(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
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
            self.emit_flow(VmEvent::Jump {
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
    pub(super) fn op_loopbreak(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let lv = self.level_param(st, cmd)?;
        self.loop_jump(st, cmd, lv, false)
    }

    /// `LOOPCONTINUE[LV]`:手册 `control/loopcontinue.html`;§3.13(opcode 58)。
    pub(super) fn op_loopcontinue(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let lv = self.level_param(st, cmd)?;
        self.loop_jump(st, cmd, lv, true)
    }

    pub(super) fn loop_jump(
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
        self.emit_flow(VmEvent::Jump {
            frame: self.frame(),
            script: st.script_id,
            pc: cmd.pc,
            to_pc: target.end_pc,
            to_script: st.script_id,
            kind: if cont {
                JumpKind::LoopContinue
            } else {
                JumpKind::LoopBreak
            },
        });
        st.pc = target.end_pc;
        Ok(StepOutcome::Continue)
    }

    /// `WAIT[FRAME=n|TIME=ms]`:手册 `control/wait.html`;引擎 §3.2b/§3.2c。
    pub(super) fn op_wait(
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
            let v = self.eval_window(st, w)?;
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
            // 裸 `WAIT[]`:手册 `basic/task.html`「ウェイト命令…タスクは
            // それを実行したあと、一旦休憩します(約0.016秒)」⇒ 让出一帧
            // (`control/wait.html` 只定义 TIME/FRAME,无参时长未定义 ——
            // 取 1 帧,Likely;语料 95/95 全带参,零行为差)。
            task.wait_frames = 1;
            self.emit(VmEvent::Wait {
                frame: self.frame(),
                task: task.id.clone(),
                frames: Some(1),
                time_ms: None,
            });
            return Ok(StepOutcome::Yield);
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
    pub(super) fn op_fps(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let mut fps = self.fps();
        let mut skip = self.skip();
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let v = self.eval_window(st, w)?;
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
    ///
    /// 引擎语义是**全程序退出**,不是结束当前任务(REA 0.495 `0x441334`):
    /// 处理体把 `RESTART` 参数值的**低 8 位**写入 `[0x80152c]`,再把
    /// `[0x80186c]` 置 1;主循环看到后者就不再跑帧泵(0x403bfc/0x4040f8),
    /// 由宿主层 `WM_CLOSE` 关窗。`RESTART=1` 在进程收尾时用
    /// `CreateProcessA` 重启自身(0x403e78)。因此这里返回
    /// [`StepOutcome::ExitProgram`],由调度器终止整条任务链;内核不重启进程。
    pub(super) fn op_end(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let mut restart = false;
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let v = self.eval_window(st, w)?;
            if w.rec.kind == 0 {
                // 官方 `mov [0x80152c], al` ⇒ 只保留低 8 位。
                restart = (v.as_int().unwrap_or(0) as u8) != 0;
            }
        }
        self.emit(VmEvent::End {
            frame: self.frame(),
            restart,
        });
        let _ = st;
        Ok(StepOutcome::ExitProgram { restart })
    }

    // ---------------------------------------------------------- 辅助

    pub(super) fn level_param(&mut self, st: &ExecState, cmd: &Cmd) -> Result<usize, VmError> {
        match cmd.windows.first() {
            Some(w) if !w.payload.is_empty() => {
                let v = self.eval_window(st, w)?;
                Ok(v.as_int().unwrap_or(1).max(0) as usize)
            }
            _ => Ok(1),
        }
    }

    pub(crate) fn label_from_window(
        &mut self,
        st: &ExecState,
        cmd: &Cmd,
        index: usize,
    ) -> Result<crate::vm::LabelTarget, VmError> {
        // `empty_ok=false`:空标签按官方「标签不存在」Fatal 处理。
        match self.label_from_window_opt(st, cmd, index, false)? {
            Some(target) => Ok(target),
            None => Err(diag::fatal(
                code::rt::LABEL,
                Boundary::Script,
                "标签不存在:(空标签)".to_string(),
            )
            .into()),
        }
    }

    /// [`Self::label_from_window`] 的共享实现。
    ///
    /// `empty_ok=true` 时空标签返回 `None`(`GO` 的 boot 期空变量语义),
    /// 其余调用方传 `false`,保持「空标签 = 标签不存在」。
    /// 求值只做一次:常量标签仍走 `label_cache`,动态标签不会因为空检查
    /// 被求值两次(表达式可能含 `_RND` 等有状态函数)。
    pub(crate) fn label_from_window_opt(
        &mut self,
        st: &ExecState,
        cmd: &Cmd,
        index: usize,
        empty_ok: bool,
    ) -> Result<Option<crate::vm::LabelTarget>, VmError> {
        let w = cmd
            .windows
            .get(index)
            .ok_or_else(|| windows_error(cmd, "缺少标签窗口"))?;
        // 常量标签(窗口里没有变量引用)按调用点缓存:官方语料的
        // `GOSUB #label` 每帧成千上万次,旧实现每次都重解码窗口、
        // 在 BTreeMap 里按字节查表、再把 `Label` 的两个堆字符串克隆一遍。
        let constant = w
            .instrs
            .as_deref()
            .map(|instrs| instrs.iter().all(|i| !matches!(i.op, 0x48 | 0x56 | 0x76)))
            .unwrap_or(false);
        let site = (st.script_id, cmd.pc);
        if constant {
            if let Some(hit) = self.label_cache.get(&site) {
                return Ok(Some(*hit));
            }
        }
        let value = self.eval_window(st, w)?;
        let Value::Str(name) = value else {
            return Err(diag::fatal(
                code::rt::VAR_TYPE,
                Boundary::Script,
                format!("标签参数必须是字符串,得到 {}", value.type_name()),
            )
            .into());
        };
        if name.is_empty() {
            return if empty_ok {
                Ok(None)
            } else {
                Err(diag::fatal(
                    code::rt::LABEL,
                    Boundary::Script,
                    "标签不存在:(空标签)".to_string(),
                )
                .into())
            };
        }
        let label = self.find_label_target(&name).ok_or_else(|| {
            let d = diag::fatal(
                code::rt::LABEL,
                Boundary::Script,
                format!("标签不存在:{}", cp932_lossy(&name)),
            );
            VmError::Fatal(d)
        })?;
        let target = label;
        if constant {
            self.label_cache.insert(site, target);
        }
        Ok(Some(target))
    }

    pub(crate) fn jump_to(
        &mut self,
        st: &mut ExecState,
        to_script: u32,
        to_pc: u32,
        base_a: u8,
        kind: JumpKind,
        from_pc: u32,
    ) -> Result<(), VmError> {
        self.ensure_script(to_script)?;
        let script = self.tables.script(to_script).ok_or_else(|| {
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
                format!(
                    "跳转目标 s{to_script} pc={to_pc} 超出脚本 {} 组",
                    script.group_count()
                ),
            )
            .into());
        }
        self.emit_flow(VmEvent::Jump {
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
