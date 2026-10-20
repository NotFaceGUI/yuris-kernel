//! `yu-backend-soft` · `CompositorFrame` 的确定性软件参考后端。
//!
//! 本 crate 只消费 `yu-gfx` 的公开契约,把一帧 [`CompositorFrame`] 合成为
//! RGBA8,并编码成 PNG。它是确定性参考实现,不替代 GPU 后端。
//!
//! # 执行契约(与 `docs/render-parity.md` §12 一致)
//!
//! 1. 严格按 `CompositorFrame.passes` 顺序执行,不重排、不合并;
//! 2. `PassTarget::Texture`:复制/图像加工当前全部 `Unimplemented`,只计数并
//!    记录跳过原因,不猜测像素算法;
//! 3. `PassTarget::Offscreen`:分配组离屏表面(组 `SX/SY`,未给时用视口),
//!    用 pass 的 `clear`(透明黑)清屏,再按 pass 内顺序绘制;
//! 4. `PassTarget::Screen`:用 `frame.clear` 清屏,ops = 主窗口组;
//! 5. 组的 `A/MODE` 只在把离屏结果作为 `Op::Group` 画进父目标时作用一次;
//! 6. `A` 走固定管线:`min(max(A,0),255)/255`;纹理 alpha × 层 alpha 在混合
//!    方程里完成,不预乘;
//! 7. 只执行 `MODE 0/1/2`;其余模式或 `OpStatus::Unimplemented` 的操作跳过并
//!    计数,不近似;
//! 8. 文本(`TextRun`)不栅格化:只统计跳过数量;
//! 9. 颜色空间按存储值混合(`gamma_encoded_srgb8`),不做线性化;
//! 10. 过滤用 `frame.filter`:`Point` 最近邻,`Linear/Anisotropic` 双线性;
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

pub mod png;
pub mod render;
pub mod texture;

pub use render::{
    frame_has_drawable_output, RenderOutput, RenderReport, SkippedOp, SoftRenderer, Surface,
};
pub use texture::{AssetIssue, Texture, TextureStore};
