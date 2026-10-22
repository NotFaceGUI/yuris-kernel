//! RGBA8 → PNG。
//!
//! 编码参数固定(`Fast` 压缩、`NoFilter`),因此同一 [`Surface`] 重复编码得到
//! 逐字节相同的 PNG。像素本身不经过任何色彩空间转换。

use std::fs;
use std::io;
use std::path::Path;

use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder};

use crate::render::Surface;

/// 编码为 PNG 字节。
pub fn png_bytes(surface: &Surface) -> Result<Vec<u8>, image::ImageError> {
    let mut out = Vec::new();
    let encoder = PngEncoder::new_with_quality(
        &mut out,
        CompressionType::Fast,
        FilterType::NoFilter,
    );
    encoder.write_image(
        &surface.rgba,
        surface.width,
        surface.height,
        ExtendedColorType::Rgba8,
    )?;
    Ok(out)
}

/// 直接写入 PNG 文件(必要时创建父目录由调用方负责)。
pub fn write_png(path: impl AsRef<Path>, surface: &Surface) -> io::Result<()> {
    let bytes = png_bytes(surface).map_err(io::Error::other)?;
    fs::write(path, bytes)
}
