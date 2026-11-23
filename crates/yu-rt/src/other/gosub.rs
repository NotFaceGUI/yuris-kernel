//! 独立运行面的 GOSUB/RETURN 三组参数帧。
//!
//! 共享 `GosubFrame` 保持 `c1549eb` 的 `args`/`ret` 单组布局;本模块的
//! 三组帧只服务精确 555 商业分支。

use crate::event::JumpKind;
use crate::expr::ParamTy;
use crate::semantics::RefIntParamMode;
use crate::task::StepOutcome;
use crate::value::{ElemType, Value};
use crate::vm::handlers::helpers::{checked_param_index, windows_error};
use crate::vm::handlers::GOSUB_MAX_CALLS;
use crate::vm::{Cmd, ExecState, GosubFrame as SharedFrame, Vm, VmError};

#[derive(Clone, Debug, Default)]
pub(crate) struct GosubFrame {
    pub(crate) args_int: [Option<Value>; 16],
    pub(crate) args_flt: [Option<Value>; 16],
    pub(crate) args_str: [Option<Value>; 16],
    pub(crate) arg_flags: [bool; 16],
    pub(crate) ret_int: [Option<Value>; 16],
    pub(crate) ret_flt: [Option<Value>; 16],
    pub(crate) ret_str: [Option<Value>; 16],
    pub(crate) ret_flags: [bool; 16],
}

impl Vm {
    /// 独立运行面 `GOSUB`:三组参数各自落位;控制流仍用共享帧栈。
    pub(crate) fn other_op_gosub(
        &mut self,
        st: &mut ExecState,
        cmd: &Cmd,
    ) -> Result<StepOutcome, VmError> {
        if cmd.windows.is_empty() {
            return Err(windows_error(cmd, "至少需要 # 标签窗口").into());
        }
        let label = self.label_from_window(st, cmd, 0)?;
        let mut frame = GosubFrame::default();
        let mut arg_ord = 0u32;
        for w in cmd.windows.iter().skip(1) {
            if w.rec.length == 0 {
                continue;
            }
            let (n, ty) = self.param_plan(cmd.opcode, w.rec.kind);
            let idx = checked_param_index(n, cmd.opcode, w.rec.kind, self, "GOSUB")?;
            // 整数参数窗口若是字符串数组元素引用,官方传变量槽号;宏链
            // 用 `_STR(_PINT(2), i)` 读取参数数组。
            let ref_slot = matches!(ty, ParamTy::Int | ParamTy::Raw)
                && self.semantics().ref_int_param == RefIntParamMode::SlotId
                && w.lvalue_plan.is_some();
            let value = if ref_slot {
                let lv = self.eval_lvalue_window(st, w)?;
                let is_str_array = self
                    .store()
                    .slot(lv.var)
                    .is_some_and(|s| matches!(s.ty, ElemType::Str));
                // ERIS 旧式数组空间(`#`/`` ` ``)的整数参数是**槽号引用**:
                // `es_scenario_parser.yst` 以 PINT2=`#0x0b5f[0]`(STR)、
                // PINT3=`` `0x0b60[0] ``(INT) 调 `es.ScenarioMacroExec`,
                // 被调方 `_STR(_PINT(2), i)` / `_INT(_PINT(3), i)` 再用槽号
                // 读参数。只认字符串数组会把 PINT3 变成元素值 1,
                // SetParam 的 `_INT(1+i)` 全 0,DrawBG 把 logo_a/bg_black
                // 当缺参清空。`@` 普通数值数组仍按值(`com_no.pos.SET`)。
                if !lv.indices.is_empty() && (is_str_array || lv.var.space.is_legacy()) {
                    Value::Int(i64::from(lv.var.id))
                } else {
                    ty.coerce(self.eval_window(st, w)?)
                }
            } else {
                ty.coerce(self.eval_window(st, w)?)
            };
            // 运行时翻译:555 分支同样按调用点 + 求值顺序替换字符串。
            let value = match self.translation.as_mut() {
                Some(table) => {
                    table.translate_call_arg(st.script_id, cmd.pc, arg_ord, value)
                }
                None => value,
            };
            arg_ord += 1;
            match ty {
                ParamTy::Float => frame.args_flt[idx] = Some(value),
                ParamTy::Str => frame.args_str[idx] = Some(value),
                ParamTy::Int | ParamTy::Raw => frame.args_int[idx] = Some(value),
            }
            frame.arg_flags[idx] = true;
        }
        if st.frames.len() > GOSUB_MAX_CALLS {
            return Err(
                crate::task::nesting_error("GOSUB", st.frames.len() - 1, GOSUB_MAX_CALLS).into(),
            );
        }
        if std::env::var_os("YU_GOSUB_TRACE").is_some()
            && (label.script_id == 64
                || matches!(
                    (u32::from(label.script_id), label.pc),
                    (61, 72) | (61, 26) | (65, 0) | (65, 132) | (65, 153) | (214, 0)
                )
                || (st.script_id == 65 && matches!(st.pc, 152 | 174 | 177 | 180))
                || (st.script_id == 180 && st.pc == 255))
        {
            eprintln!(
                "[gosub] from s{} pc{} -> s{} pc{} int={:?} str={:?}",
                st.script_id,
                st.pc,
                label.script_id,
                label.pc,
                frame.args_int,
                frame.args_str
            );
            if (u32::from(label.script_id), label.pc) == (65, 132) {
                let rd = |space, id| {
                    self.store()
                        .read(crate::value::VarRef::new(space, id), &[])
                        .ok()
                };
                eprintln!(
                    "[drawbg] @3084={:?} $3085={:?} @3086={:?} $3087={:?} @244={:?} @242={:?}",
                    rd(crate::value::VarSpace::At, 0x0c0c),
                    rd(crate::value::VarSpace::Dollar, 0x0c0d),
                    rd(crate::value::VarSpace::At, 0x0c0e),
                    rd(crate::value::VarSpace::Dollar, 0x0c0f),
                    rd(crate::value::VarSpace::At, 0x00f4),
                    rd(crate::value::VarSpace::At, 0x00f2)
                );
            }
        }
        if std::env::var_os("YU_MACRO_TRACE").is_some()
            && (matches!(
                (u32::from(label.script_id), label.pc),
                (64, 26)
                    | (64, 5)
                    | (65, 132)
                    | (65, 151)
                    | (65, 153)
                    | (61, 72)
                    | (61, 26)
                    | (70, 918)
                    | (70, 921)
                    | (26, 0)
            ) || (st.script_id == 65 && matches!(st.pc, 152 | 153)))
        {
            let rd = |space, id, idx: &[i64]| {
                self.store()
                    .read(crate::value::VarRef::new(space, id), idx)
                    .ok()
            };
            let at = crate::value::VarSpace::At;
            let d = crate::value::VarSpace::Dollar;
            let v3059 = rd(at, 0x0bf3, &[]);
            let v3060 = rd(at, 0x0bf4, &[]);
            let v3061 = rd(at, 0x0bf5, &[]);
            let v3062 = rd(at, 0x0bf6, &[]);
            eprintln!(
                "[macro] from s{}pc{} -> s{}pc{} int={:?} str={:?}",
                st.script_id, st.pc, label.script_id, label.pc, frame.args_int, frame.args_str
            );
            eprintln!(
                "[macro] @3059={:?} @3060={:?} @3061={:?} @3062={:?} @3084={:?} $3085={:?} @3086={:?} $3087={:?}",
                v3059,
                v3060,
                v3061,
                v3062,
                rd(at, 0x0c0c, &[]),
                rd(d, 0x0c0d, &[]),
                rd(at, 0x0c0e, &[]),
                rd(d, 0x0c0f, &[])
            );
            eprintln!(
                "[macro] $3063={:?} $3064={:?} $3065={:?} $3066={:?} $3067={:?} $3068={:?}",
                rd(d, 0x0bf7, &[]),
                rd(d, 0x0bf8, &[]),
                rd(d, 0x0bf9, &[]),
                rd(d, 0x0bfa, &[]),
                rd(d, 0x0bfb, &[]),
                rd(d, 0x0bfc, &[])
            );
            eprintln!(
                "[macro] lang @1177[129]={:?} @1000={:?} @1001={:?} @1002={:?} sel@2452={:?}",
                rd(at, 0x0499, &[129]),
                rd(at, 1000, &[]),
                rd(at, 1001, &[]),
                rd(at, 1002, &[]),
                rd(at, 0x0994, &[])
            );
            if let Some((_, slot)) = self
                .store()
                .iter()
                .find(|(r, _)| r.space == crate::value::VarSpace::At && r.id == 0x0499)
            {
                eprintln!(
                    "[macro] @1177 raw dims={:?} flat129={:?} row129={:?}",
                    slot.dims,
                    slot.data.get(129),
                    slot.data.get(129usize * 256)
                );
            }
            let slot_of = |v: &Option<crate::value::Value>| match v {
                Some(crate::value::Value::Int(n)) if (1000..=65535).contains(n) => {
                    Some(*n as u16)
                }
                _ => None,
            };
            if let Some(slot) = slot_of(&v3060) {
                for i in 0..4 {
                    eprintln!(
                        "[macro] strslot{slot} $[{i}]={:?} @[{i}]={:?}",
                        rd(d, slot, &[i]),
                        rd(at, slot, &[i])
                    );
                }
            }
            if let Some(slot) = slot_of(&v3061) {
                for i in 0..4 {
                    eprintln!("[macro] intslot{slot} @[{i}]={:?}", rd(at, slot, &[i]));
                }
            }
        }
        let mut shared = SharedFrame::new(st.script_id, st.pc, st.base_a);
        shared.if_depth = st.if_stack.len();
        shared.loop_depth = st.loops.len();
        st.frames.push(shared);
        let task = self.current_task().to_string();
        if let Some(state) = self.other.as_mut() {
            let frames = state.gosub_frames.entry(task).or_default();
            // 任务起始的第一次 GOSUB:清掉同 id 任务遗留的旧帧,并补一个
            // 任务基帧。共享 `st.frames` 在任务启动时就有根帧;本栈必须与
            // 其同深,否则顶层 `RETURN` 弹出被调帧后没有调用方可承接
            // `_RINT/_RFLT/_RSTR`,调用点读回 0(555 启动链
            // `es._label.info.exist` → `IF $_RINT(1)` 即由此恒假)。
            if st.frames.len() == 2 {
                frames.clear();
                frames.push(GosubFrame::default());
            }
            frames.push(frame);
        }
        self.jump_to(
            st,
            label.script_id as u32,
            label.pc,
            label.base_a,
            JumpKind::Gosub,
            cmd.pc,
        )?;
        Ok(StepOutcome::Continue)
    }

    /// 独立运行面 `RETURN`:写回调用方的三组返回值,再弹共享帧栈。
    pub(crate) fn other_op_return(
        &mut self,
        st: &mut ExecState,
        cmd: &Cmd,
    ) -> Result<StepOutcome, VmError> {
        if st.frames.len() <= 1 {
            return Ok(StepOutcome::Finish);
        }
        let mut frame = GosubFrame::default();
        let mut flags = [false; 16];
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let (n, ty) = self.param_plan(cmd.opcode, w.rec.kind);
            let idx = checked_param_index(n, cmd.opcode, w.rec.kind, self, "RETURN")?;
            let value = ty.coerce(self.eval_window(st, w)?);
            match ty {
                ParamTy::Float => frame.ret_flt[idx] = Some(value),
                ParamTy::Str => frame.ret_str[idx] = Some(value),
                ParamTy::Int | ParamTy::Raw => frame.ret_int[idx] = Some(value),
            }
            flags[idx] = true;
        }
        if std::env::var_os("YU_GOSUB_TRACE").is_some()
            && matches!(
                (st.script_id, cmd.pc),
                (61, 80) | (61, 46) | (61, 94) | (65, 6) | (65, 11) | (65, 28) | (65, 38)
            )
        {
            eprintln!(
                "[return] s{} pc{} ret_int={:?} ret_str={:?}",
                st.script_id, cmd.pc, frame.ret_int, frame.ret_str
            );
        }
        let shared = st.frames.pop().expect("帧数 > 1");
        st.if_stack.truncate(shared.if_depth);
        st.loops.truncate(shared.loop_depth);
        let task = self.current_task().to_string();
        if let Some(state) = self.other.as_mut() {
            if let Some(frames) = state.gosub_frames.get_mut(&task) {
                frames.pop();
                if let Some(caller) = frames.last_mut() {
                    caller.ret_int = frame.ret_int;
                    caller.ret_flt = frame.ret_flt;
                    caller.ret_str = frame.ret_str;
                    caller.ret_flags = flags;
                }
            }
        }
        self.jump_to(
            st,
            shared.return_script,
            shared.return_pc,
            shared.base_a,
            JumpKind::Return,
            cmd.pc,
        )?;
        Ok(StepOutcome::Continue)
    }
}
