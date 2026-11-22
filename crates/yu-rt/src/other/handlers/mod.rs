//! 独立运行面的命令拦截分派(按命令名;语义形态由 `SemanticProfile` 给出)。

pub(crate) mod product;
pub(crate) mod text;

use crate::code;
use crate::diag;
use crate::event::{ArgValue, Domain};
use crate::expr::LValue;
use crate::semantics::{TextInfoMode, TextMode};
use crate::task::StepOutcome;
use crate::value::{ElemType, Value, VarRef, VarSpace};
use crate::vm::{Cmd, ExecState, Vm, VmError};
use yu_log::Boundary;

pub(crate) fn dispatch(
    vm: &mut Vm,
    st: &mut ExecState,
    cmd: &Cmd,
) -> Option<Result<StepOutcome, VmError>> {
    match cmd.name.as_str() {
        "GOSUB" => Some(vm.other_op_gosub(st, cmd)),
        "RETURN" => Some(vm.other_op_return(st, cmd)),
        "PRODUCT" => Some(vm.op_product(st, cmd)),
        // TEXT 同名不同义:只在语义形态明确为 FileArray(精确 555)时
        // 接管;其余形态交回共享旧 handler。
        "TEXT" if vm.semantics().text == TextMode::FileArray => {
            Some(vm.other_op_text_file(st, cmd))
        }
        // v555 TEXTINFO 是空处理体(参数表 0,`0x452d54` 只装载参数后
        // return 0);旧版 10 参数查询仍走共享 handler。
        "TEXTINFO" if vm.semantics().textinfo == TextInfoMode::Inert => {
            Some(Ok(StepOutcome::Continue))
        }
        _ => None,
    }
}
