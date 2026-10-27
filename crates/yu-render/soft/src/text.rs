//! 软件后端的**图层基面解析**(依赖本后端的快照表)。
//!
//! 文本栅格化/合成本身在两个后端之间共享,定义在 `yu_render_core::text`;
//! 这里保留同名再导出,`crate::text::TextComposer` 等既有路径不变。

pub use yu_render_core::text::*;

use crate::texture::Texture;

/// 解析图层的基图(文本合成的输入)。
///
/// 支持资产/1×1 黑/无纹理(透明)/快照(DIV-GFX-058:按钮标签等
/// `CGACT[TEXT]` 直接画在 COPY2 产生的快照精灵层上,基面 = 快照像素);
/// 离屏组仍显式拒绝(组合成的 CPU 面归渲染器,未接线,不猜)。
pub fn layer_base(
    textures: &mut crate::texture::TextureStore,
    snapshots: &crate::render::Snapshots,
    layer: &yu_gfx::Layer,
) -> Result<Texture, String> {
    match &layer.texture {
        yu_gfx::TextureRef::Asset(key) => textures
            .resolve(key)
            .cloned()
            .map_err(|d| d.code.to_string()),
        yu_gfx::TextureRef::SolidBlack1x1 => Ok(Texture::solid_black_1x1()),
        yu_gfx::TextureRef::None => {
            let size = layer
                .image_size()
                .ok_or_else(|| "text.layer_size_unknown".to_string())?;
            Ok(Texture {
                width: size.width,
                height: size.height,
                rgba: vec![0; (size.width as usize) * (size.height as usize) * 4],
            })
        }
        yu_gfx::TextureRef::Snapshot(copy_id) => snapshots
            .get(copy_id)
            .map(|surface| Texture {
                width: surface.width,
                height: surface.height,
                rgba: surface.rgba.clone(),
            })
            .ok_or_else(|| "text.layer_snapshot_missing".to_string()),
        yu_gfx::TextureRef::Offscreen(_) => Err("text.layer_source_unsupported".to_string()),
    }
}

/// 只窥视图层基图尺寸(**免克隆**)。
///
/// 与 [`layer_base`] 同一分支序;合成缓存命中/增量路径用它做尺寸校验,
/// 不再为校验整图复制一遍。
pub fn layer_base_dims(
    textures: &mut crate::texture::TextureStore,
    snapshots: &crate::render::Snapshots,
    layer: &yu_gfx::Layer,
) -> Result<(u32, u32), String> {
    match &layer.texture {
        yu_gfx::TextureRef::Asset(key) => textures
            .resolve(key)
            .map(|t| (t.width, t.height))
            .map_err(|d| d.code.to_string()),
        yu_gfx::TextureRef::SolidBlack1x1 => Ok((1, 1)),
        yu_gfx::TextureRef::None => layer
            .image_size()
            .map(|size| (size.width, size.height))
            .ok_or_else(|| "text.layer_size_unknown".to_string()),
        yu_gfx::TextureRef::Snapshot(copy_id) => snapshots
            .get(copy_id)
            .map(|surface| (surface.width, surface.height))
            .ok_or_else(|| "text.layer_snapshot_missing".to_string()),
        yu_gfx::TextureRef::Offscreen(_) => Err("text.layer_source_unsupported".to_string()),
    }
}
