//! `yu-backend-soft` · `CompositorFrame` 的确定性软件参考后端。
//!
//! 本 crate 只消费 `yu-gfx` 的公开契约,把一帧 [`CompositorFrame`] 合成为
//! RGBA8,并编码成 PNG。它是确定性参考实现,不替代 GPU 后端。
//!
//! # 定位(2026-10-27 起):CPU 模式 + 对拍基线
//!
//! 本 crate 就是 `--backend soft`(**CPU 模式**),同时充当 `yu-backend-wgpu`
//! 的语义参考与逐像素对拍基线。两条实现**互相独立**:
//!
//! - 这里的像素算法(`apply_*`、`blend_transition`、`scale_region`、`blit`)
//!   **不得**被 GPU 后端在运行期调用 —— GPU 模式的像素工作必须由 GPU 完成。
//!   否则(a)双后端失去交叉验证价值,(b)GPU 模式被 CPU 循环拖住。
//! - `Surface`/`RenderReport`/`SkippedOp`/`AssetIssue`、`TextureStore`(资产
//!   解码)、`TextComposer`(字形栅格化)、图像缓存键与 `scale_region` 已经
//!   **迁到 `yu-render-core`**(本 crate 保留同名再导出,兼容既有路径);
//!   GPU 后端应直接引用 `yu_render_core::`。
//!
//! 判定规则、现状账本与迁移顺序见 `docs/architecture/backends.md`。
//!
//! # 执行契约(与 `docs/render-parity.md` §12 一致)
//!
//! 1. 严格按 `CompositorFrame.passes` 顺序执行,不重排、不合并;
//! 2. `PassTarget::Texture`:执行 `Op::Copy`(快照)与已取证的图像加工
//!    (`RECTPAINT/2/3`、`REVERSE`、`HORIZONTAL`);其余按 `OpStatus` 跳过并计数;
//! 3. `PassTarget::Offscreen`:分配组离屏表面(组 `SX/SY`,未给时用视口),
//!    用 pass 的 `clear`(透明黑)清屏,再按 pass 内顺序绘制;
//! 4. `PassTarget::Screen`:用 `frame.clear` 清屏,ops = 主窗口组;
//! 5. 组的 `A/MODE` 只在把离屏结果作为 `Op::Group` 画进父目标时作用一次;
//! 6. `A` 走固定管线:`min(max(A,0),255)/255`;纹理 alpha × 层 alpha 在混合
//!    方程里完成,不预乘;
//! 7. 只执行 `MODE 0/1/2`;其余模式或 `OpStatus::Unimplemented` 的操作跳过并
//!    计数,不近似;
//! 8. 双纹理过渡:配对纹理的 G 通道为 mask,覆盖度见
//!    `yu_gfx::transition_coverage`(EVID-T-05);
//! 9. 文本两条路径都执行:`CGACT[TEXT]` 绘制项合成到图层基图副本;
//!    自动文本层按 `_TEXT_FLAG` 逐字排版后画到目标层/屏幕覆盖层。没有
//!    `_TEXT_FLAG` 的 `TextRun` 仍然不猜排版(`skipped_text` 计数);
//! 10. 颜色空间按存储值混合(`gamma_encoded_srgb8`),不做线性化;
//! 11. 过滤用 `frame.filter`:`Point` 最近邻,`Linear/Anisotropic` 双线性;
//!     `source=placeholder` 时报告里明确标注未取证。
//!
//! # 纹理
//!
//! [`texture::TextureStore`] 用 `yu-vfs` 的 [`yu_vfs::DataSource`] 取字节,
//! 按 `frame.image_probe_order` 探测扩展名,再用 `image` crate 解码成 RGBA8。
//! 结果按原始资产 key 缓存;缺失/解码失败产生 `asset.*` 诊断并进入缺失清单。
//!
//! # 确定性
//!
//! 渲染路径无墙钟、无线程、无散列迭代序;PNG 编码参数固定。同一帧渲染两次
//! 得到逐字节相同的 PNG(见 `tests/render.rs`)。
//!
//! # 最小用法
//!
//! ```no_run
//! # use yu_backend_soft::{png, SoftRenderer, TextureStore};
//! # use yu_gfx::CompositorFrame;
//! # use yu_vfs::{Codec, Mount};
//! # fn demo(frame: &CompositorFrame, mount: Mount) {
//! let store = TextureStore::new(
//!     Box::new(mount),
//!     Codec::Cp932,
//!     frame.image_probe_order.clone(),
//! );
//! let mut backend = SoftRenderer::new(store);
//! let out = backend.render_frame(frame);
//! let bytes = png::png_bytes(&out.surface).expect("encode");
//! # let _ = bytes;
//! # }
//! ```

#![forbid(unsafe_code)]
#![allow(clippy::result_large_err)]

pub mod imagefilters;
pub mod imageop;
pub mod render;
pub mod text;

// 共享契约/设施的**兼容再导出**:`crate::texture::…`、`crate::png::…` 等
// 既有路径保持可用,但定义在 `yu-render-core`(两个后端共享)。
pub mod png {
    pub use yu_render_core::png::*;
}
pub mod texture {
    pub use yu_render_core::texture::*;
}

pub use imageop::{
    apply_horizontal, apply_meido, apply_rect_paint, apply_resize, apply_reverse, apply_trim,
    apply_vertical, blend_transition,
};
#[allow(unused_imports)]
pub use imagefilters::{
    apply_akarusa, apply_bitwise, apply_blur, apply_diffusion, apply_merge, apply_mono,
    apply_mosaic, apply_raster, apply_rotation, apply_saido, apply_sepia, apply_whirlpool,
};
pub use render::{frame_has_drawable_output, RenderOutput, SoftRenderer};
pub use yu_render_core::{
    blit, scale_region, ImageCacheKey, ImageOpSig, ImageSource, RenderReport, SizeKey, SizeSource,
    SkippedOp, Surface,
};
pub use text::{
    content_hash_rgba, layer_base, layer_base_dims, text_base_key, texture_content_hash,
    ComposedLayer, TextComposer, TextPaintStats,
};
pub use texture::{AssetIssue, Texture, TextureStore};
