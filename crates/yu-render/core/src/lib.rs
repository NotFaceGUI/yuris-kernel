//! `yu-render-core` · 两个渲染后端共享的契约与平台设施。
//!
//! # 为什么存在
//!
//! `yu-backend-soft`(CPU 模式)与 `yu-backend-wgpu`(GPU 模式)是**两条独立
//! 实现**(`docs/architecture/backends.md`)。它们共享的只有**数据契约**与
//! **平台设施**,不共享渲染实现:
//!
//! - [`surface`]/[`report`]:`Surface`、`RenderReport`、`SkippedOp`、`blit`;
//! - [`texture`]:`TextureStore`(资产解码)、`Texture`、`AssetIssue`;
//! - [`cache`]:图像加工缓存键(与后端无关的纯数据);
//! - [`scale`]:共享区域重采样原语(GPU 后端只在 CPU 读回路径使用);
//! - [`png`]:RGBA8 → PNG(确定性编码);
//! - [`frame`]:帧级判断。
//!
//! 这里**不放**任何后端行为:软后端的 `apply_*` 像素算法、wgpu 后端的
//! 管线/目标管理各自留在本 crate 之外。

pub mod cache;
pub mod png;
pub mod report;
pub mod scale;
pub mod surface;
pub mod text;
pub mod texture;

pub use cache::{ImageCacheKey, ImageOpSig, ImageSource, SizeKey, SizeSource};
pub use report::{RenderReport, SkippedOp};
pub use scale::scale_region;
pub use surface::{blit, Surface};
pub use text::{
    content_hash_rgba, text_base_key, texture_content_hash, ComposedLayer, TextComposer,
    TextPaintStats,
};
pub use texture::{AssetIssue, Texture, TextureStore};
