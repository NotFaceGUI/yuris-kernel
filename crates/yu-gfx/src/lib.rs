//! `yu-gfx` · YU-RIS 渲染语义层(M3)。
//!
//! 唯一职责:把脚本/引擎的图形命令固化成可序列化的 [`CompositorFrame`]
//! (“画什么”)。本 crate **不含**窗口、GPU、wasm、平台代码;后端只执行
//! `CompositorFrame`,不得修改语义。
//!
//! # 分层
//!
//! ```text
//! yu-rt 事件流(VmEvent::DomainCall)
//!   → yu-gfx GfxState(图层树/组/过渡/文本模型)
//!   → CompositorFrame(稳定 JSON)
//!   → yu-backend-soft / GPU 后端(后续会话)
//! ```
//!
//! # 铁律
//!
//! - 三层取证(手册页 + 真实脚本/产物 + 引擎 REA)缺一 ⇒ `Unimplemented`;
//!   见 `docs/render-parity.md` 与 `crates/yu-gfx/docs/divergences/`。
//! - 保留引擎单位:`A` 0..256、坐标 i64、滤镜参数原值;禁止在 gfx 内
//!   预乘/归一化/取整。
//! - 槽号 → 参数名一律查工程 `ysc.ybn`([`table::ParamTable`])。
//! - 无散列表迭代序、无墙钟、无线程、无绝对路径;输出可逐字节复现。
//! - 日志/错误走 `yu-log` 的 `gfx.*` 码,不直接打印。
//!
//! # 最小用法
//!
//! ```no_run
//! # use yu_gfx::{GfxConfig, GfxState, Viewport};
//! # fn demo(tables: &yu_rt::TableSet, events: &[yu_rt::VmEvent]) {
//! let config = GfxConfig::new(Viewport::new(800, 600));
//! let mut gfx = GfxState::from_tables(tables, config).expect("gfx tables");
//! gfx.consume_events(events);
//! let frame = gfx.frame();
//! let json = yu_gfx::json::to_display_json(&frame);
//! # let _ = json;
//! # }
//! ```

#![allow(clippy::result_large_err)]
#![forbid(unsafe_code)]

pub mod error;
pub mod frame;
pub mod group;
pub mod host;
pub mod json;
pub mod layer;
pub mod op;
pub mod pass;
pub mod path;
pub mod state;
pub mod table;
pub mod text;
pub mod transition;

pub use error::{DiagAt, FrameDiag, MAX_FRAME_AT};
pub use frame::{
    probe_order_from_codes, Clear, ColorSpace, CompositorFrame, FilterMode, FilterSource,
    MipFilter, TextureFilter, Viewport, WindowState,
};
pub use group::GroupDraw;
pub use host::{GfxHost, LayerCapture};
pub use layer::{
    fixed_pipeline_alpha, shader_alpha, transition_coverage, BlendConfidence, BlendFactor,
    BlendMode, BlendOp, CopyId, EngineValue, FilterParam, FilterTransform, FixedPipelineState,
    ImageFormat, Layer, LayerId, LayerKind, LayerStatus, RectI, SizeU32, StageArg, StageOp,
    StageState, TextureFactorRule, TextureRef, Transform, Transition,
};
pub use op::{
    CopyKind, CopyOp, CopyRegion, CopySource, CopyTiming, ImageAction, ImageOp, MaskDraw, Op,
    OpStatus,
};
pub use pass::{Pass, PassTarget};
pub use state::{GfxConfig, GfxState, TextView, TextureSampler};
pub use table::{ParamTable, ResolvedArg};
pub use text::{
    layout_text, GlyphAtlasRef, TextDraw, TextDrawKey, TextFlagsSource, TextFontRequest,
    TextGlyph, TextLayoutFrame, TextRun, TextStroke, TextStyle,
};
pub use transition::TransitionDraw;
