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
    if x1 < x0 || y1 < y0 || src.width == 0 || src.height == 0 {
        return;
    }
    let x0 = x0.max(0);
    let y0 = y0.max(0);
    let x1 = x1.min(i64::from(src.width) - 1);
    let y1 = y1.min(i64::from(src.height) - 1);
    if x1 < x0 || y1 < y0 {
        return;
    }
    let run = (x1 - x0 + 1) as u32;
    let run = run.min(dst.width);
    for y in y0..=y1 {
        let dy = (y - y0) as u32;
        if dy >= dst.height {
            break;
        }
        for i in 0..run {
            let dx = i;
            let Some(px) = src.pixel((x0 as u32) + i, y as u32) else {
                break;
            };
            dst.set_pixel(dx, dy, px);
        }
    }
}
