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

/// `CGACT[RESIZE=1 X=… Y=… X2=… Y2=…]`:改层尺寸,**像素不重采样**。
///
/// 手册 `cg/cgact.html` 例 21:
/// 「レイヤをリサイズする。(画像はそのままの大きさでレイヤサイズだけ変更する)
///  ※画像を大きくしたい場合は正の値、小さくしたい場合は負の値」
///
/// 几何来自官方执行体 `FUN_0043c971`(`0x43c971`,CGACT RESIZE 执行器):
/// ```text
/// 0x43c9b7  LEA EDX,[EDX+ESI-1]   ; EDX = Ytop + srcW - 1
/// 0x43c9ce  SUB EDX,EDI           ; EDI = -X  ⇒ EDX = srcW + X + X2 - 1
/// 0x43c9d0  LEA EDI,[EDX+1]       ; newW = srcW + X + X2
/// 0x43c9c6  LEA ESI,[ESI+EAX-1]   ; newH = srcH + Y + Y2
/// ```
/// 因此四个参数是**四边相对扩缩量**,不是源区域。
/// 扩出的部分透明(`memset(0)`),缩掉的部分裁掉;原图贴到 `(X, Y)`。
pub fn apply_resize(
    texture: &mut Texture,
    x: i64,
    y: i64,
    x2: i64,
    y2: i64,
) -> Result<(), &'static str> {
    let new_w = i64::from(texture.width) + x + x2;
    let new_h = i64::from(texture.height) + y + y2;
    // 官方 `0x43c9da..0x43c9e4`:newW<=0 || newH<=0 ⇒ 报错并返回 -1。
    if new_w < 1 || new_h < 1 {
        return Err("gfx.imageop.resize.invalid");
    }
    let (new_w, new_h) = (new_w as u32, new_h as u32);
    if new_w > 0x2000 {
        // 官方 `0x43c9f1 CMP EDI,0x2000; JG` ⇒ 宽度上限 8192。
        return Err("gfx.imageop.resize.too_large");
    }
    let mut out = vec![0u8; (new_w as usize) * (new_h as usize) * 4];
    // 原图左上角落到 (x, y);负值时按引擎的裁剪语义只拷可见部分。
    for sy in 0..texture.height as i64 {
        let dy = sy + y;
        if dy < 0 || dy >= i64::from(new_h) {
            continue;
        }
        for sx in 0..texture.width as i64 {
            let dx = sx + x;
            if dx < 0 || dx >= i64::from(new_w) {
                continue;
            }
            let si = ((sy as usize) * (texture.width as usize) + sx as usize) * 4;
            let di = ((dy as usize) * (new_w as usize) + dx as usize) * 4;
            out[di..di + 4].copy_from_slice(&texture.rgba[si..si + 4]);
        }
    }
    texture.width = new_w;
    texture.height = new_h;
    texture.rgba = out;
    Ok(())
}

/// `CGACT[TRIM=1 SET=mask]`:裁掉完全透明的边缘。
///
/// 手册 `cg/cgact.html` 例 22:`8=上, 4=下, 2=左, 1=右`,可相加
/// (`SET=15` = 四边全裁)。只裁「完全透明」的行/列(alpha 全 0)。
pub fn apply_trim(texture: &mut Texture, mask: i64) {
    let (w, h) = (texture.width, texture.height);
    if w == 0 || h == 0 {
        return;
    }
    let row_clear = |tex: &Texture, y: u32| -> bool {
        let row = (y as usize) * (tex.width as usize) * 4;
        (0..tex.width as usize).all(|x| tex.rgba[row + x * 4 + 3] == 0)
    };
    let col_clear = |tex: &Texture, x: u32| -> bool {
        (0..tex.height as usize).all(|y| tex.rgba[(y * (tex.width as usize) + x as usize) * 4 + 3] == 0)
    };
    let mut top = 0u32;
    let mut bottom = 0u32;
    let mut left = 0u32;
    let mut right = 0u32;
    if mask & 8 != 0 {
        while top < h && row_clear(texture, top) {
            top += 1;
        }
    }
    if mask & 4 != 0 {
        while bottom < h - top && row_clear(texture, h - 1 - bottom) {
            bottom += 1;
        }
    }
    if mask & 2 != 0 {
        while left < w && col_clear(texture, left) {
            left += 1;
        }
    }
    if mask & 1 != 0 {
        while right < w - left && col_clear(texture, w - 1 - right) {
            right += 1;
        }
    }
    let new_w = w.saturating_sub(left + right);
    let new_h = h.saturating_sub(top + bottom);
    if new_w == 0 || new_h == 0 {
        // 全透明:官方未取证「裁到 0」的行为,保守保持原样(不猜)。
        return;
    }
    let mut out = vec![0u8; (new_w as usize) * (new_h as usize) * 4];
    for y in 0..new_h as usize {
        for x in 0..new_w as usize {
            let si = ((y + top as usize) * (w as usize) + (x + left as usize)) * 4;
            let di = (y * (new_w as usize) + x) * 4;
            out[di..di + 4].copy_from_slice(&texture.rgba[si..si + 4]);
        }
    }
    texture.width = new_w;
    texture.height = new_h;
    texture.rgba = out;
}

/// `CGACT[MEIDO=1 SET=0xRRGGBB]`:逐通道明度缩放。
///
/// 手册 `cg/cgact.html` 例 7:「レイヤ「BG」の明度を赤２００％緑２００％青０％に
/// 設定する。`CGACT[ID=BG MEIDO=1 SET=0xffff00]`」
/// ⇒ 每个字节是该通道的**增益**,`0x80` = 100%(200% 写作 `0xFF`,0% 写作 `0x00`),
/// 与语料默认值 `@L.MD=0x808080`(等价于不变)一致。
/// `out = min(255, in * gain / 0x80)`。
pub fn apply_meido(texture: &mut Texture, set: i64) {
    let set = set as u32;
    let gain = [
        ((set >> 16) & 0xFF) as u32,
        ((set >> 8) & 0xFF) as u32,
        (set & 0xFF) as u32,
    ];
    for px in texture.rgba.chunks_exact_mut(4) {
        for c in 0..3 {
            let v = (u32::from(px[c]) * gain[c]) / 0x80;
            px[c] = v.min(255) as u8;
        }
    }
}

/// `CGACT[TRANSITION=1 SET=f ID=from ID2=to ID3=dst]`:两源按 `f` 线性插值。
///
/// `f` 与 `A` 同量纲(0..256):`out = from*(256-f)/256 + to*f/256`,逐通道
/// (含 alpha)。语料 `es_Sprite.yst:7978` 是唯一用法,`f` 由动画引擎在
/// 0..256 之间推进;缺失这条会让立绘/背景的动画分支输出全透明缓冲。
///
/// 尺寸取 `to` 的尺寸(新图),`from` 越界按透明;这与引擎「遷移途中画像」的
/// 用途一致(新图决定画布,旧图按同点采样)。
pub fn blend_transition(from: &Texture, to: &Texture, blend: i64) -> Texture {
    let f = blend.clamp(0, 256) as u32;
    let (w, h) = (to.width, to.height);
    if w == 0 || h == 0 {
        return Texture {
            width: w,
            height: h,
            rgba: Vec::new(),
        };
    }
    let mut out = vec![0u8; (w as usize) * (h as usize) * 4];
    let inv = 256 - f;
    for y in 0..h as usize {
        for x in 0..w as usize {
            let d = (y * (w as usize) + x) * 4;
            let ti = d;
            let ti = if ti + 4 <= to.rgba.len() { ti } else { 0 };
            let fi = if x < from.width as usize && y < from.height as usize {
                (y * (from.width as usize) + x) * 4
            } else {
                usize::MAX
            };
            for c in 0..4 {
                let src_to = u32::from(to.rgba.get(ti + c).copied().unwrap_or(0));
                let src_from = if fi == usize::MAX {
                    0
                } else {
                    u32::from(from.rgba.get(fi + c).copied().unwrap_or(0))
                };
                out[d + c] = ((src_from * inv + src_to * f) / 256) as u8;
            }
        }
    }
    Texture {
        width: w,
        height: h,
        rgba: out,
    }
}

#[cfg(test)]
mod blend_tests {
    use super::*;

    fn tex(w: u32, h: u32, px: [u8; 4]) -> Texture {
        Texture {
            width: w,
            height: h,
            rgba: px.iter().copied().cycle().take((w * h * 4) as usize).collect(),
        }
    }

    /// `SET=0` = 全旧图,`SET=256` = 全新图,`SET=128` = 各半。
    #[test]
    fn blend_transition_endpoints_and_midpoint() {
        let from = tex(1, 1, [0, 0, 0, 255]);
        let to = tex(1, 1, [200, 100, 50, 255]);
        assert_eq!(blend_transition(&from, &to, 0).rgba, vec![0, 0, 0, 255]);
        assert_eq!(blend_transition(&from, &to, 256).rgba, vec![200, 100, 50, 255]);
        assert_eq!(
            blend_transition(&from, &to, 128).rgba,
            vec![100, 50, 25, 255],
            "128/256 = 一半"
        );
    }

    /// 画布取新图尺寸;旧图越界部分按透明(0)。
    #[test]
    fn blend_transition_uses_to_size_and_zero_pads_from() {
        let from = tex(1, 1, [255, 255, 255, 255]);
        let to = tex(2, 1, [0, 0, 0, 0]);
        let out = blend_transition(&from, &to, 0);
        assert_eq!((out.width, out.height), (2, 1));
        assert_eq!(out.rgba.len(), 8);
        // 第 1 像素取旧图,第 2 像素旧图越界 ⇒ 透明。
        assert_eq!(&out.rgba[0..4], &[255, 255, 255, 255]);
        assert_eq!(&out.rgba[4..8], &[0, 0, 0, 0]);
    }
}

/// `VERTICAL=1`:垂直镜像(上下翻转),alpha 不变。
///
/// 与 `HORIZONTAL` 同为整层几何翻转,手册 `cg/cgact.html` 例 10/11 把两者
/// 并列(「垂直に反転」/「水平に反転」);语料 `es_Sprite.yst:8233/8240`
/// 在 `es.SP.DRAW.POST` 里成对出现(各自的 `IF[@POSTF_*]` 门控)。
pub fn apply_vertical(texture: &mut Texture) {
    let h = texture.height as usize;
    let w = texture.width as usize;
    if h < 2 || w == 0 {
        return;
    }
    let stride = w * 4;
    for y in 0..(h / 2) {
        let a = y * stride;
        let b = (h - 1 - y) * stride;
        for i in 0..stride {
            texture.rgba.swap(a + i, b + i);
        }
    }
}

#[cfg(test)]
mod flip_tests {
    use super::*;

    fn tex2x2() -> Texture {
        // 行 0: 红,绿 ; 行 1: 蓝,白
        Texture {
            width: 2,
            height: 2,
            rgba: vec![
                255, 0, 0, 255, /**/ 0, 255, 0, 255, /**/ 0, 0, 255, 255, /**/ 255, 255, 255, 128,
            ],
        }
    }

    /// `VERTICAL` = 上下翻转:行 0 与行 1 互换,列不变。
    #[test]
    fn vertical_swaps_rows_only() {
        let mut t = tex2x2();
        apply_vertical(&mut t);
        assert_eq!(&t.rgba[0..4], &[0, 0, 255, 255], "新行 0 = 原行 1 左");
        assert_eq!(&t.rgba[4..8], &[255, 255, 255, 128], "新行 0 = 原行 1 右");
        assert_eq!(&t.rgba[8..12], &[255, 0, 0, 255], "新行 1 = 原行 0 左");
        assert_eq!(&t.rgba[12..16], &[0, 255, 0, 255], "新行 1 = 原行 0 右");
    }

    /// `HORIZONTAL` = 左右翻转:列互换、行不变;且两次翻转回到原图。
    #[test]
    fn horizontal_swaps_columns_only_and_roundtrips() {
        let mut t = tex2x2();
        apply_horizontal(&mut t);
        assert_eq!(&t.rgba[0..4], &[0, 255, 0, 255], "新行 0 左 = 原行 0 右");
        assert_eq!(&t.rgba[8..12], &[255, 255, 255, 128], "新行 1 左 = 原行 1 右");
        apply_horizontal(&mut t);
        assert_eq!(t.rgba, tex2x2().rgba, "两次水平翻转应还原");
    }

    /// 三次垂直翻转应等于一次(奇偶性),且 alpha 不变。
    #[test]
    fn vertical_is_involution() {
        let mut t = tex2x2();
        apply_vertical(&mut t);
        apply_vertical(&mut t);
        assert_eq!(t.rgba, tex2x2().rgba, "两次垂直翻转应还原");
    }
}

#[cfg(test)]
mod extent_tests {
    use super::*;

    fn tex(w: u32, h: u32) -> Texture {
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                rgba.extend_from_slice(&[(x * 10) as u8, (y * 10) as u8, 7, 255]);
            }
        }
        Texture {
            width: w,
            height: h,
            rgba,
        }
    }

    fn px(t: &Texture, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * t.width + x) * 4) as usize;
        [t.rgba[i], t.rgba[i + 1], t.rgba[i + 2], t.rgba[i + 3]]
    }

    /// 手册例 21:`+10/-30`(右)、`+20/-40`(下)⇒ 宽 10+10-30=-10、
    /// 高 10+20-40=-10 ⇒ 非法(官方 `0x43c9da` 同样报错)。
    #[test]
    fn resize_follows_official_geometry() {
        let mut t = tex(10, 10);
        // newW = 10 + 5 + (-3) = 12; newH = 10 + 2 + (-1) = 11
        apply_resize(&mut t, 5, 2, -3, -1).unwrap();
        assert_eq!((t.width, t.height), (12, 11), "newW=srcW+X+X2");
        // 原图左上角贴到 (5,2):(0,0) 仍在透明补齐区
        assert_eq!(px(&t, 0, 0), [0, 0, 0, 0], "扩出的部分透明");
        assert_eq!(px(&t, 5, 2), [0, 0, 7, 255], "原图 (0,0) 落到 (X,Y)");
        assert_eq!(px(&t, 6, 3), [10, 10, 7, 255], "原图 (1,1)");
    }

    /// 负边距裁掉部分原图(左/上被切)。
    #[test]
    fn resize_negative_margin_crops() {
        let mut t = tex(10, 10);
        // newW = 10 - 4 + 0 = 6;newH = 10 - 3 + 0 = 7
        apply_resize(&mut t, -4, -3, 0, 0).unwrap();
        assert_eq!((t.width, t.height), (6, 7));
        // 原图 (4,3) 落到 (0,0)
        assert_eq!(px(&t, 0, 0), [40, 30, 7, 255]);
    }

    #[test]
    fn resize_rejects_non_positive_and_too_large() {
        let mut t = tex(10, 10);
        assert!(apply_resize(&mut t, -20, 0, 0, 0).is_err(), "newW<1 应报错");
        assert!(apply_resize(&mut t, 9000, 0, 0, 0).is_err(), "超过 0x2000 应报错");
    }

    /// 手册例 22:`SET=15` 四边全裁,只裁 alpha 全 0 的行/列。
    #[test]
    fn trim_removes_fully_transparent_edges() {
        // 4x4,只有中心 2x2 不透明
        let mut t = Texture {
            width: 4,
            height: 4,
            rgba: vec![0; 4 * 4 * 4],
        };
        for y in 1..3 {
            for x in 1..3 {
                let i = ((y * 4 + x) * 4) as usize;
                t.rgba[i] = 9;
                t.rgba[i + 3] = 255;
            }
        }
        apply_trim(&mut t, 15);
        assert_eq!((t.width, t.height), (2, 2));
        assert_eq!(px(&t, 0, 0), [9, 0, 0, 255]);
    }

    /// 只裁指定方向:`SET=8` 只裁上边。
    #[test]
    fn trim_honours_direction_mask() {
        let mut t = Texture {
            width: 2,
            height: 3,
            rgba: vec![0; 2 * 3 * 4],
        };
        // 底两行不透明,顶行透明
        for y in 1..3 {
            for x in 0..2 {
                let i = ((y * 2 + x) * 4) as usize;
                t.rgba[i + 3] = 255;
            }
        }
        apply_trim(&mut t, 8);
        assert_eq!(t.height, 2, "只裁上边");
        assert_eq!(t.width, 2, "宽度不变");
    }

    /// 手册例 7:`SET=0xffff00` ⇒ 红 200%、绿 200%、蓝 0%。
    #[test]
    fn meido_scales_channels_by_half_byte_ratio() {
        let mut t = Texture {
            width: 1,
            height: 1,
            rgba: vec![100, 100, 100, 255],
        };
        apply_meido(&mut t, 0x00ff_ff00);
        // 0xFF/0x80 ≈ 1.992 ⇒ 100*255/128 = 199
        assert_eq!(px(&t, 0, 0), [199, 199, 0, 255], "蓝色增益 0 ⇒ 0");
    }

    /// 默认值 `0x808080` 必须等价于不变(语料 `@L.MD=0x808080` 用作「无效果」)。
    #[test]
    fn meido_default_gain_is_identity() {
        let mut t = tex(2, 2);
        let before = t.rgba.clone();
        apply_meido(&mut t, 0x0080_8080);
        assert_eq!(t.rgba, before);
    }
}
