//! 独立运行面的脚本装载期编排。
//!
//! 555 商业作把 `S_INT/S_FLT/S_STR` 与入口标签前的 `INT/FLT/STR` 当作
//! 装载期声明;共享旧版仍按执行流声明。

use crate::value::ElemType;
use crate::vm::{CmdWindow, ExecState, Vm};
use yu_log::Diag;

pub(crate) fn apply_script_statics(vm: &mut Vm, script_id: u32) -> Result<(), Diag> {
    let Some(script) = vm.tables().script(script_id) else {
        return Ok(());
    };
    let first_label_pc = vm
        .tables()
        .boot
        .labels
        .labels
        .iter()
        .filter(|l| u32::from(l.script_id) == script_id)
        .map(|l| l.pc)
        .min()
        .unwrap_or(u32::MAX);
    let mut decls: Vec<(u32, ElemType, Vec<CmdWindow>)> = Vec::new();
    for pc in 0..script.group_count() {
        let Some(group) = script.group(pc) else {
            continue;
        };
        let name = vm
            .tables()
            .boot
            .yscm
            .command(group.opcode)
            .map(|c| c.name.as_str());
        let ty = match name {
            Some("S_INT") => ElemType::Int,
            Some("S_FLT") => ElemType::Float,
            Some("S_STR") => ElemType::Str,
            Some("INT") if (pc as u32) < first_label_pc => ElemType::Int,
            Some("FLT") if (pc as u32) < first_label_pc => ElemType::Float,
            Some("STR") if (pc as u32) < first_label_pc => ElemType::Str,
            _ => continue,
        };
        let mut windows = Vec::new();
        for rec in script.group_windows(pc) {
            let Some(payload) = script.window_payload(rec) else {
                continue;
            };
            windows.push(CmdWindow {
                rec: *rec,
                payload: payload.to_vec(),
                instrs: None,
                const_value: None,
                lvalue_plan: None,
                simple: None,
                eval_fails: false,
            });
        }
        if !windows.is_empty() {
            decls.push((pc as u32, ty, windows));
        }
    }
    for (pc, ty, windows) in decls {
        let st = ExecState::new(script_id, pc);
        let Ok(lv) = vm.eval_lvalue_window(&st, &windows[0]) else {
            continue;
        };
        let dims: Vec<usize> = lv.indices.iter().map(|&d| d.max(0) as usize).collect();
        let init = match windows.get(1) {
            Some(w) if !w.payload.is_empty() => vm
                .eval_window(&st, w)
                .unwrap_or_else(|_| ty.default_value()),
            _ => ty.default_value(),
        };
        vm.store_mut().reset_declared(lv.var, ty, &dims, &init);
    }
    Ok(())
}
