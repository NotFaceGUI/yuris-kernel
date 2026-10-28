//! 共享表面与整块拷贝(两个后端与文本合成共用)。

use crate::texture::Texture;

/// RGBA8 表面(行优先,直通 alpha)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Surface {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Surface {
    /// 用单一 RGBA 颜色填满。
    pub fn new(width: u32, height: u32, rgba: [u8; 4]) -> Self {
        let pixels = (width as usize).saturating_mul(height as usize);
        let mut bytes = Vec::with_capacity(pixels.saturating_mul(4));
        for _ in 0..pixels {
            bytes.extend_from_slice(&rgba);
        }
        Self {
            width,
            height,
            rgba: bytes,
        }
    }

    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = ((y as usize) * (self.width as usize) + x as usize) * 4;
        Some([
            self.rgba[i],
            self.rgba[i + 1],
            self.rgba[i + 2],
            self.rgba[i + 3],
        ])
    }

    pub fn set_pixel(&mut self, x: u32, y: u32, px: [u8; 4]) {
        let i = ((y as usize) * (self.width as usize) + x as usize) * 4;
        self.rgba[i..i + 4].copy_from_slice(&px);
    }

    pub fn from_texture(texture: &Texture) -> Self {
        Self {
            width: texture.width,
            height: texture.height,
            rgba: texture.rgba.clone(),
        }
    }
}

pub fn blit(src: &Surface, dst: &mut Surface, x0: i64, y0: i64, x1: i64, y1: i64) {
    blit_rgba(src.width, src.height, &src.rgba, dst, x0, y0, x1, y1);
}

/// 与 [`blit`] 同一语义,但源直接给 `(width, height, rgba)` 切片。
///
/// 快照镜像捕获用它:**不再为每次 COPY 整帧 clone 一份源**(旧实现
/// `Surface { rgba: source.rgba.clone() }` 在 1100×825/2048² 源上每帧
/// 要复制几十 MB,`snap_ms` 实测 33ms)。行内是 `copy_from_slice`
/// (memcpy),不再逐像素 `pixel()/set_pixel()`。
pub fn blit_rgba(
    src_width: u32,
    src_height: u32,
    src_rgba: &[u8],
    dst: &mut Surface,
    x0: i64,
    y0: i64,
    x1: i64,
    y1: i64,
) {
    if x1 < x0 || y1 < y0 || src_width == 0 || src_height == 0 {
        return;
    }
    let x0 = x0.max(0);
    let y0 = y0.max(0);
    let x1 = x1.min(i64::from(src_width) - 1);
    let y1 = y1.min(i64::from(src_height) - 1);
    if x1 < x0 || y1 < y0 {
        return;
    }
    let run = ((x1 - x0 + 1) as u32).min(dst.width) as usize;
    if run == 0 {
        return;
    }
    let src_stride = src_width as usize * 4;
    let dst_stride = dst.width as usize * 4;
    for y in y0..=y1 {
        let dy = (y - y0) as u32;
        if dy >= dst.height {
            break;
        }
        let src_start = y as usize * src_stride + x0 as usize * 4;
        let dst_start = dy as usize * dst_stride;
        // 源切片可能短于名义尺寸(畸形输入):按实际长度截断,与逐像素
        // `pixel()` 的越界即停保持一致。
        let available = src_rgba.len().saturating_sub(src_start);
        let bytes = (run * 4).min(available);
        if bytes == 0 {
            break;
        }
        let Some(dst_row) = dst.rgba.get_mut(dst_start..dst_start + bytes) else {
            break;
        };
        dst_row.copy_from_slice(&src_rgba[src_start..src_start + bytes]);
    }
}
