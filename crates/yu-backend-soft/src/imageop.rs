//! `CGACT[RECTPAINT/RECTPAINT2/RECTPAINT3]` 的确定性像素实现。
//!
//! 语义证据(`docs/fidelity-ledger-j.md` EVID-J-02..05):
//! - `FUN_0042f809`(0x42f809) 行 334-342 把动作 0x18/0x19/0x1a 分派到
//!   `FUN_0043185c`(0x43185c);
//! - `FUN_0043185c` 行 47-59:模式 1 `*pixel = (SET & 0xFFFFFF) - 0x1000000`
//!   (即 `0xFF000000 | (SET & 0xFFFFFF)`,alpha 强制 255);
//! - 行 60-83:模式 2 只写像素第 4 字节(`SET & 0xFF`);模式 3 写整个 32 位值;
//! - 区域循环 `x=in_stack_00000010..=in_stack_00000018`、
//!   `y=in_stack_00000014..=in_stack_0000001c`,含端点。
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tex(w: u32, h: u32, px: [u8; 4]) -> Texture {
        Texture {
            width: w,
            height: h,
            rgba: px.iter().copied().cycle().take((w * h * 4) as usize).collect(),
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
