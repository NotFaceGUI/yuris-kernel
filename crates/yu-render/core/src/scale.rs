//! 共享区域重采样原语。
//!
//! GPU 后端只在「CPU 消费者确实需要像素」的读回路径使用它
//! (`docs/architecture/backends.md` §4 的 ⚠️ 项),不作为常规渲染路径。

use crate::texture::Texture;

pub fn scale_region(
    src: &Texture,
    dst_w: u32,
    dst_h: u32,
    region: Option<(i64, i64, i64, i64)>,
    bilinear: bool,
) -> Texture {
    if dst_w == 0 || dst_h == 0 || src.width == 0 || src.height == 0 {
        return Texture {
            width: dst_w,
            height: dst_h,
            rgba: Vec::new(),
        };
    }
    let (x0, y0, x1, y1) = match region {
        Some((x, y, x2, y2)) => (x, y, x2, y2),
        None => (0, 0, i64::from(src.width) - 1, i64::from(src.height) - 1),
    };
    let cw = (x1 - x0 + 1).max(1);
    let ch = (y1 - y0 + 1).max(1);
    let mut out = vec![0u8; (dst_w as usize) * (dst_h as usize) * 4];
    let sw = i64::from(src.width);
    let sh = i64::from(src.height);
    let texel = |x: i64, y: i64| -> [u8; 4] {
        if x < 0 || y < 0 || x >= sw || y >= sh {
            return [0, 0, 0, 0];
        }
        let i = ((y as usize) * (src.width as usize) + x as usize) * 4;
        [
            src.rgba[i],
            src.rgba[i + 1],
            src.rgba[i + 2],
            src.rgba[i + 3],
        ]
    };
    for dy in 0..dst_h as i64 {
        for dx in 0..dst_w as i64 {
            let d = ((dy as usize) * (dst_w as usize) + dx as usize) * 4;
            if !bilinear {
                let sx = x0 + dx * cw / i64::from(dst_w);
                let sy = y0 + dy * ch / i64::from(dst_h);
                out[d..d + 4].copy_from_slice(&texel(sx, sy));
            } else {
                let u = (dx as f64 + 0.5) * (cw as f64) / (dst_w as f64) - 0.5 + x0 as f64;
                let v = (dy as f64 + 0.5) * (ch as f64) / (dst_h as f64) - 0.5 + y0 as f64;
                let fx = u.floor();
                let fy = v.floor();
                let tx = u - fx;
                let ty = v - fy;
                let (ix, iy) = (fx as i64, fy as i64);
                let p00 = texel(ix, iy);
                let p10 = texel(ix + 1, iy);
                let p01 = texel(ix, iy + 1);
                let p11 = texel(ix + 1, iy + 1);
                for c in 0..4 {
                    let top = f64::from(p00[c]) * (1.0 - tx) + f64::from(p10[c]) * tx;
                    let bot = f64::from(p01[c]) * (1.0 - tx) + f64::from(p11[c]) * tx;
                    out[d + c] = (top * (1.0 - ty) + bot * ty).round().clamp(0.0, 255.0) as u8;
                }
            }
        }
    }
    Texture {
        width: dst_w,
        height: dst_h,
        rgba: out,
    }
}
