//! 独立运行面的脚本装载期编排。
//!
//! 555 商业作把 `S_INT/S_FLT/S_STR` 与**全脚本**的 `INT/FLT/STR` 当作
//! 装载期声明;共享旧版仍按执行流声明。
//!
//! 证据(`<v555-kuro>` `es_saveload.yst` s33):`es.SL.DRAW.BIGTHUMB`
//! 入口 pc640 是无条件 `GO es.SL.DRAW.BIGTHUMB.LEnd`(pc850),声明
//! `INT @2223` 在 pc644,返回 pc851 读取 `@2223`。若只预声明入口标签
//! 之前的 `INT`,该槽永远未声明 ⇒ 官方不会出现的
//! `rt.var.undeclared`。官方因此必须在脚本装载时初始化全脚本声明。

use crate::value::ElemType;
use crate::vm::{CmdWindow, ExecState, Vm};
use crate::{code, diag};
use yu_log::{Boundary, Diag, Severity};

pub(crate) fn apply_script_statics(vm: &mut Vm, script_id: u32) -> Result<(), Diag> {
    // 语义层单点门控:只有 `LoadTime` 形态才做全脚本装载期声明。
    if vm.semantics().script_decls != crate::semantics::ScriptDeclMode::LoadTime {
        return Ok(());
    }
    let Some(script) = vm.tables().script(script_id) else {
        return Ok(());
    };
    let mut decls: Vec<(u32, ElemType, Vec<CmdWindow>)> = Vec::new();
    // `script` 借自 `vm.tables()`,循环内不能同时 `vm.report`;先记账,
    // 循环结束、借用释放后统一发诊断。
    let mut pending: Vec<Diag> = Vec::new();
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
            Some("S_INT" | "INT") => ElemType::Int,
            Some("S_FLT" | "FLT") => ElemType::Float,
            Some("S_STR" | "STR") => ElemType::Str,
            _ => continue,
        };
        let mut windows = Vec::new();
        for rec in script.group_windows(pc) {
            let Some(payload) = script.window_payload(rec) else {
                // 零长度窗口是合法的空参数;非零长度却取不到载荷说明
                // 窗口边界/共享池异常,不能静默丢声明。
                if rec.length != 0 {
                    pending.push(diag::recoverable(
                        code::rt::EXPR_MALFORMED,
                        Boundary::Script,
                        format!(
                            "装载期声明 s{script_id} pc{pc} 的窗口 kind={} 声明长度 {} 但取不到载荷",
                            rec.kind, rec.length
                        ),
                    ));
                }
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
        let lv = match vm.eval_lvalue_window(&st, &windows[0]) {
            Ok(lv) => lv,
            Err(d) => {
                pending.push(declaration_failure(script_id, pc, d));
                continue;
            }
        };
        // 只补"还不存在"的槽。已由其它脚本声明/赋值的共享槽不在装载期
        // 重置——官方是否"声明即重置"未取证,重置会静默清掉先加载脚本
        // 的状态,也会在装载期求值 `_RAND`/时间等初始化表达式。
        if vm.store().slot(lv.var).is_some() {
            continue;
        }
        let dims: Vec<usize> = lv.indices.iter().map(|&d| d.max(0) as usize).collect();
        let init = match windows.get(1) {
            Some(w) if !w.payload.is_empty() => match vm.eval_window(&st, w) {
                Ok(v) => v,
                Err(d) => {
                    // 初始化表达式失败不能静默变默认值:默认值会与官方
                    // 实际声明值不同,且没有痕迹。Fatal 降级为 Recoverable:
                    // 装载期声明失败不应弹宿主致命框(官方是否终止未取证)。
                    pending.push(declaration_failure(script_id, pc, d));
                    ty.default_value()
                }
            },
            _ => ty.default_value(),
        };
        vm.store_mut().reset_declared(lv.var, ty, &dims, &init);
    }
    for d in pending {
        vm.report(d);
    }
    Ok(())
}

/// 装载期声明求值失败的诊断:保留原码/级别,仅把 Fatal 降为 Recoverable。
///
/// 官方在装载期遇到坏声明是否终止未取证;直接上报 Fatal 会让宿主弹出
/// 致命框,比“跳过该声明并留痕”更激进。非 Fatal 原样保留。
fn declaration_failure(script_id: u32, pc: u32, d: Diag) -> Diag {
    if matches!(d.severity, Severity::Fatal) {
        diag::recoverable(
            code::rt::EXPR_MALFORMED,
            Boundary::Script,
            format!(
                "装载期声明 s{script_id} pc{pc} 求值失败(原 {}): {}",
                d.code, d.detail
            ),
        )
    } else {
        d
    }
}
