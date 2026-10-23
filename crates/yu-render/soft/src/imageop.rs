//! `CGACT` 图像加工的确定性像素实现。
//!
//! 语义证据(`docs/fidelity-ledger-j.md` EVID-J-02..05,
//! `docs/title-motion-ledger.md` EVID-T-13):
//! - `FUN_0042f809`(0x42f809) 行 334-342 把动作 0x18/0x19/0x1a 分派到
//!   `FUN_0043185c`(0x43185c);
//! - `FUN_0043185c` 行 47-59:模式 1 `*pixel = (SET & 0xFFFFFF) - 0x1000000`
//!   (即 `0xFF000000 | (SET & 0xFFFFFF)`,alpha 强制 255);
//! - 行 60-83:模式 2 只写像素第 4 字节(`SET & 0xFF`);模式 3 写整个 32 位值;
//! - 区域循环 `x=in_stack_00000010..=in_stack_00000018`、
//!   `y=in_stack_00000014..=in_stack_0000001c`,含端点。
//! - `REVERSE`/`HORIZONTAL` 的像素结果由官方微场景 `j2-micro-rev` 定标:
//!   REVERSE = RGB 逐通道取反(alpha 不变),HORIZONTAL = 水平镜像。
//!
//! 引擎像素 DWORD 是 `0xAARRGGBB`(内存序 B,G,R,A);本模块的 [`Texture`]
//! 是 RGBA8 行优先,因此在边界处做一次通道重排,不做任何混合/近似。

use yu_gfx::ImageAction;

use crate::texture::Texture;

/// 在纹理上执行一次 RECTPAINT 族填充。
///
/// - `set` 是脚本 `SET` 原值;模式 1/3 按 32 位截断,模式 2 只取低 8 位。
/// - `region` 为 `(x, y, x2, y2)`,含端点、纹理像素坐标;`None` = 整张纹理。
///   越界部分裁剪;`x2 < x` 或 `y2 < y` 时不写像素(引擎的循环同样不执行)。
pub fn apply_rect_paint(
    texture: &mut Texture,
    action: ImageAction,
    set: i64,
    region: Option<(i64, i64, i64, i64)>,
) -> Result<(), &'static str> {
    let mode = match action {
        ImageAction::RectPaint => 1,
        ImageAction::RectPaint2 => 2,
        ImageAction::RectPaint3 => 3,
        _ => return Err("gfx.imageop.not_rectpaint"),
    };
    if texture.width == 0 || texture.height == 0 {
        return Ok(());
    }
    let (x0, y0, x1, y1) = match region {
        Some((x, y, x2, y2)) => (x, y, x2, y2),
        None => (
            0,
            0,
            i64::from(texture.width) - 1,
            i64::from(texture.height) - 1,
        ),
    };
    if x1 < x0 || y1 < y0 {
        return Ok(());
    }
    let x0 = x0.max(0) as u32;
    let y0 = y0.max(0) as u32;
    let x1 = x1.min(i64::from(texture.width) - 1);
    let y1 = y1.min(i64::from(texture.height) - 1);
    if x1 < i64::from(x0) || y1 < i64::from(y0) {
        return Ok(());
    }
    let x1 = x1 as u32;
    let y1 = y1 as u32;
    let set = set as u32;

    for y in y0..=y1 {
        let row = (y as usize) * (texture.width as usize) * 4;
        for x in x0..=x1 {
            let i = row + (x as usize) * 4;
            match mode {
                1 => {
                    // (SET & 0xFFFFFF) | 0xFF000000 -> RGBA
                    texture.rgba[i] = ((set >> 16) & 0xFF) as u8;
                    texture.rgba[i + 1] = ((set >> 8) & 0xFF) as u8;
                    texture.rgba[i + 2] = (set & 0xFF) as u8;
                    texture.rgba[i + 3] = 0xFF;
                }
                2 => {
                    // 只写 alpha;RGB 保持。
                    texture.rgba[i + 3] = (set & 0xFF) as u8;
                }
                _ => {
                    // 整个 0xAARRGGBB -> RGBA
                    texture.rgba[i] = ((set >> 16) & 0xFF) as u8;
                    texture.rgba[i + 1] = ((set >> 8) & 0xFF) as u8;
                    texture.rgba[i + 2] = (set & 0xFF) as u8;
                    texture.rgba[i + 3] = ((set >> 24) & 0xFF) as u8;
                }
            }
        }
    }
    Ok(())
}

/// `REVERSE=1`:RGB 逐通道取反(`255-x`),alpha 不变。
///
/// 证据:`j2-micro-rev`(官方)左半 `(32,64,96)` → `(223,191,159)`,
/// 右半 `(192,32,32)` → `(63,223,223)`;alpha 两例都保持 255。
pub fn apply_reverse(texture: &mut Texture) {
    for px in texture.rgba.chunks_exact_mut(4) {
        px[0] = 255 - px[0];
        px[1] = 255 - px[1];
        px[2] = 255 - px[2];
    }
}

/// `HORIZONTAL=1`:水平镜像(左右翻转),alpha 不变。
///
/// 证据:`j2-micro-rev` 的 D 层左右两半互换。
pub fn apply_horizontal(texture: &mut Texture) {
    let w = texture.width as usize;
    if w < 2 {
        return;
    }
    let stride = w * 4;
    for row in texture.rgba.chunks_exact_mut(stride) {
        for x in 0..(w / 2) {
            let a = x * 4;
            let b = (w - 1 - x) * 4;
            for c in 0..4 {
                row.swap(a + c, b + c);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tex(w: u32, h: u32, px: [u8; 4]) -> Texture {
        Texture {
            width: w,
            height: h,
            rgba: px
                .iter()
                .copied()
                .cycle()
                .take((w * h * 4) as usize)
                .collect(),
        }
    }

    #[test]
    fn rectpaint_sets_rgb_and_forces_opaque_alpha() {
        let mut t = tex(1, 1, [10, 20, 30, 0]);
        apply_rect_paint(&mut t, ImageAction::RectPaint, 0x00112233, None).unwrap();
        // 0xAARRGGBB = 0x00112233 -> R=0x11, G=0x22, B=0x33;alpha 强制 0xFF。
        assert_eq!(t.pixel(0, 0), Some([0x11, 0x22, 0x33, 0xFF]));
    }

    #[test]
    fn rectpaint2_changes_only_alpha() {
        let mut t = tex(1, 1, [10, 20, 30, 255]);
        apply_rect_paint(&mut t, ImageAction::RectPaint2, 0x80, None).unwrap();
        assert_eq!(t.pixel(0, 0), Some([10, 20, 30, 0x80]));
    }

    #[test]
    fn rectpaint3_writes_argb() {
        let mut t = tex(1, 1, [0, 0, 0, 0]);
        apply_rect_paint(&mut t, ImageAction::RectPaint3, 0x8040C020, None).unwrap();
        assert_eq!(t.pixel(0, 0), Some([0x40, 0xC0, 0x20, 0x80]));
    }

    #[test]
    fn rectpaint_region_is_inclusive_and_clipped() {
        let mut t = tex(4, 4, [0, 0, 0, 255]);
        apply_rect_paint(
            &mut t,
            ImageAction::RectPaint3,
            0xFF00FF00,
            Some((1, 1, 2, 2)),
        )
        .unwrap();
        assert_eq!(t.pixel(0, 0), Some([0, 0, 0, 255]));
        assert_eq!(t.pixel(1, 1), Some([0, 255, 0, 255]));
        assert_eq!(t.pixel(2, 2), Some([0, 255, 0, 255]));
        assert_eq!(t.pixel(3, 3), Some([0, 0, 0, 255]));

        let mut t = tex(2, 2, [0, 0, 0, 255]);
        apply_rect_paint(
            &mut t,
            ImageAction::RectPaint3,
            0xFFFF0000,
            Some((-5, -5, 99, 99)),
        )
        .unwrap();
        assert_eq!(t.pixel(0, 0), Some([255, 0, 0, 255]));
        assert_eq!(t.pixel(1, 1), Some([255, 0, 0, 255]));
    }
}

/// `CGACT[SIZE/SIZE2]`:`src` 的矩形区域缩放到 `dst_w × dst_h`。
///
/// - 区域 `(x, y, x2, y2)` 含端点、源纹理坐标;`None` = 整张纹理;
///   越界部分按透明处理(引擎 `es.CGACT_SIZE` 的「範囲外」语义)。
/// - `SIZE`(nearest)= 目标像素映射 `src = x0 + dx*sw/dw`(整数除法);
///   `SIZE2`(bilinear)= 标准 2×2 双线性,边缘夹取。
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

#[cfg(test)]
mod scale_tests {
    use super::scale_region;
    use crate::texture::Texture;

    #[test]
    fn nearest_maps_with_integer_division() {
        let src = Texture {
            width: 2,
            height: 2,
            rgba: vec![1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255],
        };
        let out = scale_region(&src, 4, 4, None, false);
        assert_eq!(out.width, 4);
        // 目标 (0,0)/(1,0) -> 源 (0,0);(2,0)/(3,0) -> 源 (1,0)。
        assert_eq!(&out.rgba[0..4], &[1, 2, 3, 255]);
        assert_eq!(&out.rgba[8..12], &[4, 5, 6, 255]);
        // 目标 (0,2) -> 源 (0,1)。
        assert_eq!(&out.rgba[32..36], &[7, 8, 9, 255]);
    }

    #[test]
    fn region_is_inclusive() {
        let src = Texture {
            width: 4,
            height: 1,
            rgba: vec![1, 0, 0, 255, 2, 0, 0, 255, 3, 0, 0, 255, 4, 0, 0, 255],
        };
        let out = scale_region(&src, 3, 1, Some((1, 0, 2, 0)), false);
        assert_eq!(&out.rgba[0..4], &[2, 0, 0, 255]);
        assert_eq!(&out.rgba[8..12], &[3, 0, 0, 255]);
    }
}
