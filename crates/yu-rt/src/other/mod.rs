//! 独立商业分支运行面(中性目录名 `other`;只在
//! [`SemanticProfile::is_v555_family`](crate::semantics::SemanticProfile::is_v555_family)
//! 为真时安装,即 0.500 / 0.555)。
//!
//! 共享 `yu-rt` 只保留通用 VM 核与旧版 handler;本模块承载 555 商业族
//! (0.500/0.555)的命令、系统变量、剧本/启动编排。旧版不安装,
//! 行为保持 `c1549eb`。

mod gosub;
mod handlers;
mod scenario;
mod startup;
mod sysvar;

use crate::task::StepOutcome;
use crate::value::{Value, VarRef};
use crate::vm::{Cmd, ExecState, Vm, VmError};
use yu_log::Diag;

/// 独立运行面状态(只在安装后存在;旧版 `Vm.other == None`)。
pub(crate) struct State {
    pub(crate) drop_files: Vec<Vec<u8>>,
    /// `_DROPFILE_NUM` 的脚本可写镜像(官方 v555 `DAT_008724ac`)。
    ///
    /// 用 i32 保留负数(`LET @_DROPFILE_NUM=-1` 不应静默变 4294967295);
    /// 官方宽度/符号未逐位取证,读写两侧共用本字段。
    pub(crate) drop_file_num: i32,
    pub(crate) last_text_lines: usize,
    pub(crate) gosub_frames: std::collections::BTreeMap<String, Vec<gosub::GosubFrame>>,
    /// 已执行装载期静态声明的脚本(避免重复 reset)。
    pub(crate) statics_applied: std::collections::BTreeSet<u32>,
    /// `PRODUCT[COM=22]` 的跨行词法状态(`/* ... */` 块注释)。
    pub(crate) scenario_lex: scenario::LexState,
    /// `_STR`/`_INT` 读侧发现的静默回退(未声明/错型/非字符串系统变量)。
    ///
    /// 读路径是 `&Vm`(`debug_sysvar`/表达式求值都只借不可变),不能直接
    /// `report`;这里先入队,`Vm::step_command` 在命令结束后统一带位置发出。
    pub(crate) read_diags: std::cell::RefCell<Vec<Diag>>,
}

impl State {
    fn new() -> Self {
        Self {
            drop_files: Vec::new(),
            drop_file_num: 0,
            last_text_lines: 0,
            gosub_frames: std::collections::BTreeMap::new(),
            statics_applied: std::collections::BTreeSet::new(),
            scenario_lex: scenario::LexState::default(),
            read_diags: std::cell::RefCell::new(Vec::new()),
        }
    }
}

pub(crate) fn install(vm: &mut Vm) {
    vm.other = Some(State::new());
}

/// 安装后补跑入口脚本的装载期声明(入口脚本在安装前已装载)。
///
/// 失败不能吞:装载期声明缺失会让后续 `GOSUB` 读到未声明变量(实机
/// Fatal);诊断带上脚本/命令位置后可定位到具体声明点。
pub(crate) fn apply_initial_script_statics(vm: &mut Vm) {
    if let Some(script_id) = vm.main_script_id() {
        if let Some(Err(d)) = vm.other_on_script_loaded(script_id) {
            vm.report(d);
        }
    }
}

impl Vm {
    /// 命令拦截入口。`None` = 交回共享旧 handler(旧版路径)。
    pub(crate) fn other_intercept_command(
        &mut self,
        st: &mut ExecState,
        cmd: &Cmd,
    ) -> Option<Result<StepOutcome, VmError>> {
        if self.other.is_none() {
            return None;
        }
        handlers::dispatch(self, st, cmd)
    }

    /// 系统变量读取拦截。`None` = 交回共享 `sysvar::read`。
    pub(crate) fn other_sysvar_get(
        &self,
        st: &ExecState,
        var: VarRef,
        indices: &[i64],
    ) -> Option<Result<Value, Diag>> {
        let state = self.other.as_ref()?;
        sysvar::read(self, state, st, var, indices)
    }

    /// 脚本装载后的独立运行面钩子(装载期静态声明)。
    pub(crate) fn other_on_script_loaded(&mut self, script_id: u32) -> Option<Result<(), Diag>> {
        let applied = {
            let state = self.other.as_ref()?;
            state.statics_applied.contains(&script_id)
        };
        if applied {
            return Some(Ok(()));
        }
        let result = startup::apply_script_statics(self, script_id);
        if let Some(state) = self.other.as_mut() {
            state.statics_applied.insert(script_id);
        }
        Some(result)
    }

    /// 无窗口文本推进:只在安装独立运行面时接管。
    pub(crate) fn other_text_reveal_complete(&self) -> Option<bool> {
        if self.other.is_none() {
            return None;
        }
        Some(self.text_window_none() || self.text_line_complete_now())
    }

    /// 把 `_STR`/`_INT` 读侧入队的静默回退诊断带命令位置发出。
    pub(crate) fn drain_other_read_diags(&mut self) {
        let Some(state) = self.other.as_ref() else {
            return;
        };
        // 热路径(每条命令):空队列只做一次 `borrow()` 判空,不分配 Vec。
        if state.read_diags.borrow().is_empty() {
            return;
        }
        let queued: Vec<Diag> = state.read_diags.borrow_mut().drain(..).collect();
        for d in queued {
            self.report(d);
        }
    }
}
