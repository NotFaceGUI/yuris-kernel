//! `SaveHost`:把 [`SaveState`] 接到帧循环与 VM(B1 存档闭环)。
//!
//! 全部**命令时刻同步**(与 `yu-gfx`/`yu-sound` 同型):
//! - **事件事实**(FILEACT/SAVEINFO/FILEINFO):`Vm::emit` 在命令点同步喂入
//!   (官方 `FILEACT` 处理体就地删除/复制;若推迟到帧末,存档脚本的
//!   「先删旧槽、再 `SAVE[FILE]` 刷盘」会把刚写出的文件删掉);
//! - **同步契约**(SAVE 预约/刷盘、LOAD 拉取):实现 `yu_rt::event::SaveHost`,
//!   VM 在命令时刻直接调用——预约值快照与 LOAD 写回的时序都在命令时刻
//!   (REA `0x44d73d`/`0x447b3d`),异步事件流给不了。宿主不得再消费
//!   SAVE/LOAD/SAVEEND 的 DomainCall(会重复)。

use std::sync::{Arc, Mutex};

use yu_log::Diag;
use yu_rt::event::{LoadedBlock, VarSnapshot, VmEvent};
use yu_rt::tables::YscmTable;
use yu_rt::value::{ElemType, VarRef};

use crate::state::SaveState;

/// 共享句柄 + 工程命令表(参数槽号 → 名字)。
#[derive(Clone)]
pub struct SaveHost {
    state: Arc<Mutex<SaveState>>,
    yscm: Arc<YscmTable>,
}

impl SaveHost {
    pub fn new(state: Arc<Mutex<SaveState>>, yscm: YscmTable) -> Self {
        Self {
            state,
            yscm: Arc::new(yscm),
        }
    }

    /// 共享状态句柄(`Engine` 用它取诊断/快照)。
    pub fn state(&self) -> Arc<Mutex<SaveState>> {
        Arc::clone(&self.state)
    }

    /// 喂入一条事件(非 Save/File 域直接忽略;B1 起 SAVE/LOAD/SAVEEND
    /// 的 DomainCall 被状态机忽略——语义已走同步契约)。
    pub fn apply_event(&self, ev: &VmEvent) {
        // 锁中毒降级(H-P1-01 同口径): poisoned 锁里的状态仍是合法的
        // 前一时刻快照,继续用,不 panic。
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.apply_event(ev, &self.yscm);
    }

    /// 喂入一批事件。
    pub fn apply_events<'a>(&self, events: impl IntoIterator<Item = &'a VmEvent>) {
        for ev in events {
            self.apply_event(ev);
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, SaveState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl yu_rt::event::SaveHost for SaveHost {
    fn apply_event(&mut self, ev: &VmEvent) {
        SaveHost::apply_event(self, ev);
    }

    fn reserve(&mut self, dno: u32, var: VarRef, snap: &VarSnapshot) -> Result<(), Diag> {
        self.lock().reserve(dno, var, snap)
    }

    fn reserve_group(
        &mut self,
        dno: u32,
        ty: ElemType,
        group: u8,
        vars: &[(u16, VarSnapshot)],
    ) -> Result<(), Diag> {
        self.lock().reserve_group(dno, ty, group, vars)
    }

    fn flush(&mut self, file: &str) -> Result<(), Diag> {
        self.lock().flush(file)
    }

    fn save_end(&mut self) {
        self.lock().save_end();
    }

    fn load_block(&mut self, file: &str, dno: u32) -> Result<LoadedBlock, Diag> {
        self.lock().load_block(file, dno)
    }
}
