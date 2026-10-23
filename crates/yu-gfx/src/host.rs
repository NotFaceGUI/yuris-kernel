//! `GraphicsHost`:把 `GfxState` 接成 `yu-rt` 的同步图形宿主。
//!
//! 安装到 `Vm` 后,图形/窗口/文本/帧事件在**产生时**同步进入 `GfxState`,
//! `CGINFO/FONTINFO/WINDOWINFO` 因此能在脚本执行期读到同一帧的最新状态
//! (`LET` 写回)。调用方**不得**再把同一批事件 drain 后喂给该状态。
//!
//! 用法:
//!
//! ```no_run
//! # use std::cell::RefCell;
//! # use std::rc::Rc;
//! # use yu_gfx::{GfxConfig, GfxHost, GfxState, Viewport};
//! # fn demo(tables: &yu_rt::TableSet, vm: &mut yu_rt::Vm) {
//! let state = Rc::new(RefCell::new(
//!     GfxState::from_tables(tables, GfxConfig::new(Viewport::new(800, 600)))
//!         .expect("gfx"),
//! ));
//! vm.set_graphics_host(Box::new(GfxHost::new(state.clone())));
//! // tick 之后:
//! let frame = state.borrow().frame();
//! # let _ = frame;
//! # }
//! ```

use std::cell::RefCell;
use std::rc::Rc;

use yu_log::Diag;
use yu_rt::event::{ArgValue, GraphicsHost, VmEvent};
use yu_rt::value::Value;

use crate::state::{GfxState, TextureSampler};

/// `GfxState` 的共享句柄(宿主与调用方共用同一状态)。
pub struct GfxHost {
    state: Rc<RefCell<GfxState>>,
    /// `CGINFO[ONMOUSE*]` 的像素采样面;未注入时查询返回 Unimplemented。
    sampler: Option<Box<dyn TextureSampler>>,
}

impl GfxHost {
    pub fn new(state: Rc<RefCell<GfxState>>) -> Self {
        Self {
            state,
            sampler: None,
        }
    }

    /// 注入纹理采样器(soft/wgpu 共用 `TextureStore`;由宿主装配)。
    pub fn set_texture_sampler(&mut self, sampler: Box<dyn TextureSampler>) {
        self.sampler = Some(sampler);
    }

    /// 取回共享状态,便于 tick 后构建 `CompositorFrame`。
    pub fn state(&self) -> Rc<RefCell<GfxState>> {
        self.state.clone()
    }
}

impl GraphicsHost for GfxHost {
    fn apply_event(&mut self, ev: &VmEvent) {
        self.state.borrow_mut().on_event(ev);
    }

    fn query(&mut self, command: &str, args: &[ArgValue]) -> Result<Value, Diag> {
        let GfxHost { state, sampler } = self;
        let sampler = sampler
            .as_mut()
            .map(|boxed| boxed.as_mut() as &mut dyn TextureSampler);
        state.borrow().query_info_with(command, args, sampler)
    }

    fn note_asset_size(&mut self, key: &[u8], width: u32, height: u32) {
        self.state
            .borrow_mut()
            .set_asset_size(key, crate::layer::SizeU32 { width, height });
    }

    fn set_pointer(&mut self, x: i64, y: i64) {
        self.state.borrow_mut().set_pointer(x, y);
    }
}
