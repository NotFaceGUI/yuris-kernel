//! `SaveHost`:把 [`SaveState`] 接到 `yu-cli` 的帧循环(事件事实消费)。
//!
//! 与 `yu-sound::SoundHost` 的差别(有意为之):`yu-rt` 本轮冻结
//! (任务书 P4 文件占用),所以存档宿主**不加 `yu-rt` trait**;由
//! `yu-cli::engine` 在每帧 `Vm::take_events()` 之后把事件喂进来。
//! 事实记录天然容忍这个一帧延迟(契约文档 §6:「事件是事实记录」)。

use std::sync::{Arc, Mutex};

use yu_rt::event::VmEvent;
use yu_rt::tables::YscmTable;

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

    /// 喂入一条事件(非 Save/File 域直接忽略)。
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
}
