//! 独立运行面的系统变量访问器。
//!
//! - `_PINT/_PFLT/_PSTR` / `_RINT/_RFLT/_RSTR` / `_PFLG/_RFLG` 读本模块的
//!   三组帧,不碰共享 `GosubFrame`。
//! - `_STR/_INT` 是 555 商业版的动态变量访问器(按槽号读变量表)。
//! - `_DROPFILE/_DROPFILE_NUM` 读 `PRODUCT[COM=21/24]` 的枚举结果。

use crate::code;
use crate::diag;
use crate::other::State;
use crate::value::{Value, VarRef, VarSpace};
use crate::vm::{ExecState, Vm};
use yu_log::Boundary;

pub(crate) fn read(
    vm: &Vm,
    state: &State,
    st: &ExecState,
    var: VarRef,
    indices: &[i64],
) -> Option<Result<Value, yu_log::Diag>> {
    let name = vm.sysvars().get(var.id)?.name.as_str();
    let frames = state
        .gosub_frames
        .get(vm.current_task())
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    match name {
        "_PINT" | "_PFLT" | "_PSTR" => Some(read_pint(frames, st, name, indices)),
        "_RINT" | "_RFLT" | "_RSTR" => Some(read_rint(frames, st, name, indices)),
        "_PFLG" => Some(
            param_index(indices)
                .map(|n| Value::Int(frames.last().map(|f| f.arg_flags[n] as i64).unwrap_or(0))),
        ),
        "_RFLG" => Some(
            param_index(indices)
                .map(|n| Value::Int(frames.last().map(|f| f.ret_flags[n] as i64).unwrap_or(0))),
        ),
        "_DROPFILE_NUM" => Some(Ok(Value::Int(i64::from(state.drop_file_num)))),
        "_DROPFILE" => {
            let n = indices.first().copied().unwrap_or(0);
            if n < 0 {
                return Some(Ok(Value::Str(Vec::new())));
            }
            Some(Ok(Value::Str(
                state
                    .drop_files
                    .get(n as usize)
                    .cloned()
                    .unwrap_or_default(),
            )))
        }
        "_STR" => Some(read_str(vm, st, indices)),
        "_INT" => Some(read_int(vm, indices)),
        _ => None,
    }
}

fn read_pint(
    frames: &[super::gosub::GosubFrame],
    _st: &ExecState,
    name: &str,
    indices: &[i64],
) -> Result<Value, yu_log::Diag> {
    let n = param_index(indices)?;
    let frame = frames.last();
    let out = match name {
        "_PINT" => Ok(frame
            .and_then(|f| f.args_int[n].clone())
            .unwrap_or(Value::Int(0))),
        "_PFLT" => Ok(Value::Float(
            frame
                .and_then(|f| f.args_flt[n].as_ref())
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0),
        )),
        _ => Ok(frame
            .and_then(|f| f.args_str[n].clone())
            .unwrap_or_else(|| Value::Str(Vec::new()))),
    };
    out
}

fn read_rint(
    frames: &[super::gosub::GosubFrame],
    _st: &ExecState,
    name: &str,
    indices: &[i64],
) -> Result<Value, yu_log::Diag> {
    let n = param_index(indices)?;
    // 任务基帧 = 空帧(与共享 `st.frames.last()` 的默认值一致)。
    let frame = frames.last();
    let out = match name {
        "_RINT" => Ok(frame
            .and_then(|f| f.ret_int[n].clone())
            .unwrap_or(Value::Int(0))),
        "_RFLT" => Ok(Value::Float(
            frame
                .and_then(|f| f.ret_flt[n].as_ref())
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0),
        )),
        _ => Ok(frame
            .and_then(|f| f.ret_str[n].clone())
            .unwrap_or_else(|| Value::Str(Vec::new()))),
    };
    out
}

fn read_str(vm: &Vm, st: &ExecState, indices: &[i64]) -> Result<Value, yu_log::Diag> {
    let Some((slot, index)) = dynamic_slot_index(indices)? else {
        return Ok(Value::Str(Vec::new()));
    };
    let Ok(slot) = u16::try_from(slot) else {
        return Ok(Value::Str(Vec::new()));
    };
    if slot < 1000 {
        const STRING_SYSVARS: &[&str] = &[
            "_ARGV",
            "_DROPFILE",
            "_PSTR",
            "_RSTR",
            "_TSTR",
            "_CHR",
            "_SYSTEM_PATH",
            "_SAVE_PATH",
            "_APPDATA_PATH",
            "_PERSONAL_PATH",
            "_PROJECT_PATH",
            "_TEXT",
            "_TEXT_FLAG",
        ];
        let Some(decl) = vm.sysvars().get(slot) else {
            return Ok(Value::Str(Vec::new()));
        };
        if !STRING_SYSVARS.contains(&decl.name.as_str()) {
            return Ok(Value::Str(Vec::new()));
        }
        let sysvar = VarRef::new(VarSpace::At, slot);
        return match crate::sysvar::read(vm, st, sysvar, &[index]) {
            Ok(Value::Str(bytes)) => Ok(Value::Str(bytes)),
            Ok(_) | Err(_) => Ok(Value::Str(Vec::new())),
        };
    }
    let Some(var) = vm.store().find_by_id(slot, VarSpace::Dollar) else {
        return Ok(Value::Str(Vec::new()));
    };
    Ok(match vm.store().read(var, &[index]) {
        Ok(Value::Str(bytes)) => Value::Str(bytes),
        Ok(_) | Err(_) => Value::Str(Vec::new()),
    })
}

fn read_int(vm: &Vm, indices: &[i64]) -> Result<Value, yu_log::Diag> {
    let Some((slot, index)) = dynamic_slot_index(indices)? else {
        return Ok(Value::Int(0));
    };
    // 槽号下界不下结论:官方片段只证据了目标变量类型检查(0x1a6c6),
    // <1000 是否可读未取证;交给变量表查找,未声明自然返回 0。
    let Ok(slot) = u16::try_from(slot) else {
        return Ok(Value::Int(0));
    };
    let Some(var) = vm.store().find_by_id(slot, VarSpace::At) else {
        return Ok(Value::Int(0));
    };
    // 官方槽表把标量当 1 元素:无第二下标时元素 0 合法;非 0 越界。
    // 只借维度判标量,不做 `dims.clone()`(`_INT` 读可能落在脚本循环里)。
    let scalar = vm.store().slot(var).map(|s| s.dims.is_empty()).unwrap_or(true);
    let read_index: &[i64] = if scalar {
        if index != 0 {
            return Ok(Value::Int(0));
        }
        &[]
    } else {
        core::slice::from_ref(&index)
    };
    Ok(match vm.store().read(var, read_index) {
        Ok(Value::Int(v)) => Value::Int(v),
        Ok(Value::Float(f)) => Value::Int(f as i64),
        Ok(_) | Err(_) => Value::Int(0),
    })
}

fn dynamic_slot_index(indices: &[i64]) -> Result<Option<(i64, i64)>, yu_log::Diag> {
    match indices {
        [] => Ok(None),
        [slot] => Ok(Some((*slot, 0))),
        [slot, index] => Ok(Some((*slot, *index))),
        _ => Err(diag::unimplemented(
            code::rt::SYSVAR_UNIMPLEMENTED,
            Boundary::Script,
            format!(
                "_STR/_INT 只支持 1..2 个下标(槽号[,元素号]),得到 {} 个",
                indices.len()
            ),
        )),
    }
}

fn param_index(indices: &[i64]) -> Result<usize, yu_log::Diag> {
    let n = indices.first().copied().unwrap_or(1);
    if !(1..=16).contains(&n) {
        return Err(diag::fatal(
            code::rt::SYSVAR_UNIMPLEMENTED,
            Boundary::Script,
            format!("参数编号 {n} 超出 1..16(手册口径)"),
        ));
    }
    Ok((n - 1) as usize)
}
