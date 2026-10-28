//! `CGACT` 逐层滤镜族(原地动作)的像素实现。
//!
//! 参数含义一律取手册 `cg/cgact.html` 的示例原文:
//! - `[5]  ROTATION=1 SET=30`        → 時計回りに３０度回転
//! - `[6]  SAIDO=1 SET=64`           → 彩度を64に設定
//! - `[7]  MEIDO=1 SET=0xffff00`     → 明度を赤２００％緑２００％青０％
//! - `[8]  AKARUSA=1 SET=192`        → 明るさを192に設定
//! - `[9]  MONO=1 SET=160`           → ２階調化
//! - `[12] MOSAIC=1 SET=4`           → モザイク
//! - `[13] BLUR=1 SET=8`             → ぼかす
//! - `[14] DIFFUSION=1 SET=8`        → 拡散処理
//! - `[16] WHIRLPOOL=1 SET=3000`     → 渦巻き状に進行度 3000 で回転
//! - `[17] RASTER=1 SET=5 SET2=1`    → 横方向に 5 の幅、進行度 1
//! - `[23]/[24] MERGE / MERGE2`      → 結合(上层=ID,下层=ID2,结果留 ID2)
//!
//! 手册无页但在 yscm 表里的键(`SEPIA` / `XOR` / `AND` / `RASTER1..3` /
//! `BLUR2..6` / `ROTATION2` / `TRANSITION2`)按同族约定实现,
//! 在 `docs/divergences/DIV-GFX-062` 记为 `partial(no-manual)`。

use super::Texture;

/// 取像素(越界透明)。
fn texel(src: &[u8], w: i64, h: i64, x: i64, y: i64) -> [u8; 4] {
    if x < 0 || y < 0 || x >= w || y >= h {
        return [0, 0, 0, 0];
    }
    let i = (y as usize * w as usize + x as usize) * 4;
    [src[i], src[i + 1], src[i + 2], src[i + 3]]
}

/// Rec.601 定点亮度(`77/150/29`,与整数运算家族一致)。
fn luma(px: &[u8]) -> u32 {
    (u32::from(px[0]) * 77 + u32::from(px[1]) * 150 + u32::from(px[2]) * 29) >> 8
}

/// `CGACT[ROTATION=1 SET=deg]`:绕图层中心旋转,正值**顺时针**,单位为度。
///
/// 手册例 5「時計回りに３０度回転させる」`SET=30`。
/// 语料 `es_Sprite.yst:8100` 的 `SET=360*1000000-@RZ` 对 360 取模后 = `-@RZ`,
/// 即「抵消已应用的 roll」,与「度 + 周期性」自洽(已锚定)。
/// 画布尺寸不变(手册未描述尺寸变化),越界采样透明。
pub fn apply_rotation(texture: &mut Texture, degrees: i64) {
    let (w, h) = (texture.width as i64, texture.height as i64);
    if w == 0 || h == 0 {
        return;
    }
    let deg = (degrees % 360) as f64;
    if deg == 0.0 {
        return;
    }
    let (sin, cos) = deg.to_radians().sin_cos();
    let cx = (w as f64 - 1.0) / 2.0;
    let cy = (h as f64 - 1.0) / 2.0;
    let src = texture.rgba.clone();
    let mut out = vec![0u8; src.len()];
    for dy in 0..h {
        for dx in 0..w {
            let fx = dx as f64 - cx;
            let fy = dy as f64 - cy;
            let sx = cos * fx + sin * fy + cx;
            let sy = -sin * fx + cos * fy + cy;
            let p = texel(&src, w, h, sx.round() as i64, sy.round() as i64);
            let di = (dy as usize * w as usize + dx as usize) * 4;
            out[di..di + 4].copy_from_slice(&p);
        }
    }
    texture.rgba = out;
}

/// `CGACT[SAIDO=1 SET=sat]`:彩度。`0x80`(128)= 不变;`0` = 灰度。
///
/// 手册例 6「彩度を64に設定」⇒ 64/128 = 0.5 倍。
pub fn apply_saido(texture: &mut Texture, sat: i64) {
    let k = sat.clamp(0, 256) as f64 / 128.0;
    for px in texture.rgba.chunks_exact_mut(4) {
        let l = f64::from(luma(px));
        for c in 0..3 {
            let v = l + (f64::from(px[c]) - l) * k;
            px[c] = v.round().clamp(0.0, 255.0) as u8;
        }
    }
}

/// `CGACT[AKARUSA=1 SET=b]`:明るさ。`0x80`(128)= 不变。
///
/// 手册例 8「明るさを192に設定」⇒ 192/128 = 1.5 倍。
pub fn apply_akarusa(texture: &mut Texture, value: i64) {
    let k = value.clamp(0, 256) as u32;
    for px in texture.rgba.chunks_exact_mut(4) {
        for c in 0..3 {
            px[c] = ((u32::from(px[c]) * k) / 0x80).min(255) as u8;
        }
    }
}

/// `CGACT[MONO=1 SET=threshold]`:二值化,`luma >= threshold` 取白。
///
/// 手册例 9「２階調化（モノクロ化）」`SET=160`;语料 `es_Sprite.yst:8254` 用 128。
pub fn apply_mono(texture: &mut Texture, threshold: i64) {
    let t = threshold.clamp(0, 255) as u32;
    for px in texture.rgba.chunks_exact_mut(4) {
        let v = if luma(px) >= t { 255u8 } else { 0u8 };
        px[0] = v;
        px[1] = v;
        px[2] = v;
    }
}

/// `CGACT[MOSAIC=1 SET=n]`:马赛克,`n` = 块边长;块内取左上角像素。
///
/// 手册例 12 `SET=4`。
pub fn apply_mosaic(texture: &mut Texture, block: i64) {
    let n = block.max(1) as u32;
    if n <= 1 {
        return;
    }
    let (w, h) = (texture.width, texture.height);
    let src = texture.rgba.clone();
    for by in (0..h).step_by(n as usize) {
        for bx in (0..w).step_by(n as usize) {
            let si = ((by * w + bx) * 4) as usize;
            let p = [src[si], src[si + 1], src[si + 2], src[si + 3]];
            for y in by..(by + n).min(h) {
                for x in bx..(bx + n).min(w) {
                    let di = ((y * w + x) * 4) as usize;
                    texture.rgba[di..di + 4].copy_from_slice(&p);
                }
            }
        }
    }
}

/// `CGACT[BLUR=1 SET=r]`:可分离盒式模糊(半径 `r`,边缘夹取)。
///
/// 手册例 13「レイヤをぼかす」`SET=8`。`BLUR2..BLUR6` 无手册页,
/// 按 `BLUR` 同族以不同半径处理(调用方传入换算后的半径)。
pub fn apply_blur(texture: &mut Texture, radius: i64) {
    let r = radius.max(0);
    if r == 0 {
        return;
    }
    let (w, h) = (texture.width as i64, texture.height as i64);
    if w == 0 || h == 0 {
        return;
    }
    let src = texture.rgba.clone();
    let mut mid = vec![0u8; src.len()];
    // 水平
    for j in 0..h {
        for i in 0..w {
            let mut acc = [0u32; 4];
            let mut cnt = 0u32;
            for d in -r..=r {
                let k = (i + d).clamp(0, w - 1);
                let p = texel(&src, w, h, k, j);
                for c in 0..4 {
                    acc[c] += u32::from(p[c]);
                }
                cnt += 1;
            }
            let di = (j as usize * w as usize + i as usize) * 4;
            for c in 0..4 {
                mid[di + c] = (acc[c] / cnt) as u8;
            }
        }
    }
    // 垂直
    for j in 0..h {
        for i in 0..w {
            let mut acc = [0u32; 4];
            let mut cnt = 0u32;
            for d in -r..=r {
                let k = (j + d).clamp(0, h - 1);
                let p = texel(&mid, w, h, i, k);
                for c in 0..4 {
                    acc[c] += u32::from(p[c]);
                }
                cnt += 1;
            }
            let di = (j as usize * w as usize + i as usize) * 4;
            for c in 0..4 {
                texture.rgba[di + c] = (acc[c] / cnt) as u8;
            }
        }
    }
}

/// `CGACT[DIFFUSION=1 SET=s]`:扩散(每像素确定性伪随机位移)。
///
/// 手册例 14 `SET=8`。种子固定(xorshift),保证像素哈希可对拍。
pub fn apply_diffusion(texture: &mut Texture, strength: i64) {
    let s = strength.max(0) as u32;
    if s == 0 {
        return;
    }
    let (w, h) = (texture.width as i64, texture.height as i64);
    if w == 0 || h == 0 {
        return;
    }
    let src = texture.rgba.clone();
    let mut state: u32 = 0x9E37_79B9;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state
    };
    for y in 0..h {
        for x in 0..w {
            let dx = (next() % (s + 1)) as i64;
            let dy = (next() % (s + 1)) as i64;
            let p = texel(&src, w, h, x + dx, y + dy);
            let di = (y as usize * w as usize + x as usize) * 4;
            texture.rgba[di..di + 4].copy_from_slice(&p);
        }
    }
}

/// `CGACT[WHIRLPOOL=1 SET=progress]`:旋涡,`progress` 是「進行度」。
///
/// 手册例 16 用 `3000`,并注明「進行度が負の値なら逆回転」。
/// `progress/1000` 作为中心最大旋转弧度。
pub fn apply_whirlpool(texture: &mut Texture, progress: i64) {
    if progress == 0 {
        return;
    }
    let (w, h) = (texture.width as i64, texture.height as i64);
    if w == 0 || h == 0 {
        return;
    }
    let max_angle = progress as f64 / 1000.0;
    let cx = (w as f64 - 1.0) / 2.0;
    let cy = (h as f64 - 1.0) / 2.0;
    let radius = (cx * cx + cy * cy).sqrt().max(1.0);
    let src = texture.rgba.clone();
    for y in 0..h {
        for x in 0..w {
            let dx = x as f64 - cx;
            let dy = y as f64 - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            let angle = max_angle * (1.0 - (dist / radius).clamp(0.0, 1.0));
            let (s, c) = angle.sin_cos();
            let sx = c * dx - s * dy + cx;
            let sy = s * dx + c * dy + cy;
            let p = texel(&src, w, h, sx.round() as i64, sy.round() as i64);
            let di = (y as usize * w as usize + x as usize) * 4;
            texture.rgba[di..di + 4].copy_from_slice(&p);
        }
    }
}

/// `CGACT[RASTER=1 SET=width SET2=progress]`:拉斯特滚动。
///
/// 手册例 17:「横方向に 5 の幅、進行度 1 でラスタースクロールさせる」
/// (`SET`=幅,`SET2`=進行度)。`RASTER1` = 縦方向、`RASTER2` = 縦横(关键词表有页)。
pub fn apply_raster(
    texture: &mut Texture,
    width: i64,
    progress: i64,
    horizontal: bool,
    vertical: bool,
) {
    let band = width.max(1) as u32;
    let shift = progress;
    if shift == 0 {
        return;
    }
    let (w, h) = (texture.width, texture.height);
    if w == 0 || h == 0 {
        return;
    }
    if horizontal {
        let src = texture.rgba.clone();
        for y0 in (0..h).step_by(band as usize) {
            for y in y0..(y0 + band).min(h) {
                for x in 0..w {
                    let sx = ((x as i64 + shift).rem_euclid(w as i64)) as u32;
                    let si = ((y * w + sx) * 4) as usize;
                    let di = ((y * w + x) * 4) as usize;
                    texture.rgba[di..di + 4].copy_from_slice(&src[si..si + 4]);
                }
            }
        }
    }
    if vertical {
        let src = texture.rgba.clone();
        for x0 in (0..w).step_by(band as usize) {
            for x in x0..(x0 + band).min(w) {
                for y in 0..h {
                    let sy = ((y as i64 + shift).rem_euclid(h as i64)) as u32;
                    let si = ((sy * w + x) * 4) as usize;
                    let di = ((y * w + x) * 4) as usize;
                    texture.rgba[di..di + 4].copy_from_slice(&src[si..si + 4]);
                }
            }
        }
    }
}

/// `CGACT[XOR=1]` / `CGACT[AND=1]`:两图按位合成。
///
/// 手册无页(键在 yscm 表内)⇒ `partial(no-manual)`:RGB 按位运算、
/// alpha 保留下层。上层 = `ID`,下层 = `ID2`(与 MERGE 同向)。
pub fn apply_bitwise(lower: &mut Texture, upper: &Texture, xor: bool) {
    let w = lower.width.min(upper.width);
    let h = lower.height.min(upper.height);
    for y in 0..h {
        for x in 0..w {
            let ui = ((y * upper.width + x) * 4) as usize;
            let li = ((y * lower.width + x) * 4) as usize;
            for c in 0..3 {
                lower.rgba[li + c] = if xor {
                    lower.rgba[li + c] ^ upper.rgba[ui + c]
                } else {
                    lower.rgba[li + c] & upper.rgba[ui + c]
                };
            }
        }
    }
}

/// `CGACT[SEPIA=1]`:褐色调(手册无页 ⇒ `partial(no-manual)`,经典 sepia 矩阵)。
pub fn apply_sepia(texture: &mut Texture) {
    for px in texture.rgba.chunks_exact_mut(4) {
        let (r, g, b) = (u32::from(px[0]), u32::from(px[1]), u32::from(px[2]));
        px[0] = ((r * 393 + g * 769 + b * 189) >> 8).min(255) as u8;
        px[1] = ((r * 349 + g * 686 + b * 168) >> 8).min(255) as u8;
        px[2] = ((r * 272 + g * 534 + b * 131) >> 8).min(255) as u8;
    }
}

/// `CGACT[MERGE=1]` / `MERGE2=1`:`ID`(上层)合成到 `ID2`(下层),结果留 `ID2`。
///
/// 手册例 23/24 原文:
/// - 例 23 `MERGE`:「レイヤ「BG」の不透明度だけ反映される」⇒ **只反映上层 alpha**;
/// - 例 24 `MERGE2`:「レイヤ「BG2」とレイヤ「BG」それぞれ不透明度が反映される」
///   ⇒ **双方 alpha 都参与**;
/// - 两者都注明「レイヤ「BG」を上、レイヤ「BG2」を下に置いた状態で結合され、
///   結合した結果はレイヤ「BG2」のほうに格納されます」⇒ 输出写 `ID2`。
pub fn apply_merge(lower: &mut Texture, upper: &Texture, merge2: bool) {
    let w = lower.width.min(upper.width) as usize;
    let h = lower.height.min(upper.height) as usize;
    if w == 0 || h == 0 {
        return;
    }
    let lstride = lower.width as usize * 4;
    let ustride = upper.width as usize * 4;
    // 逐行切片 + `chunks_exact_mut(4)` 配对:去掉逐像素的下标/边界检查,
    // 保持**逐字同式**的浮点运算(像素结果与旧实现完全一致)。
    // `au == 0` 的像素两个公式都是恒等(merge: `cl*(1-0)+cu*0 = cl`;
    // merge2: `ao = al`,rgb 化简回 `cl`,alpha 写回 `al`)⇒ 直接跳过,
    // 省掉写回(精灵部件在大缓冲里大部分是透明像素)。
    for y in 0..h {
        let urow = &upper.rgba[y * ustride..y * ustride + w * 4];
        // 整行透明 ⇒ 恒等,连目标行都不用读/写(精灵部件在 2048² 缓冲里
        // 通常只占中间一块,旧实现逐行扫满整个宽度)。
        let (x0, x1) = crate::imageop::alpha_span(urow, w);
        if x0 > x1 {
            continue;
        }
        let lrow = &mut lower.rgba[y * lstride + x0 * 4..y * lstride + (x1 + 1) * 4];
        let urow = &urow[x0 * 4..(x1 + 1) * 4];
        for (lp, up) in lrow.chunks_exact_mut(4).zip(urow.chunks_exact(4)) {
            let au = f64::from(up[3]) / 255.0;
            if au == 0.0 {
                continue;
            }
            if merge2 {
                let al = f64::from(lp[3]) / 255.0;
                let ao = au + al * (1.0 - au);
                if ao > 0.0 {
                    for c in 0..3 {
                        let cu = f64::from(up[c]);
                        let cl = f64::from(lp[c]);
                        let co = (cu * au + cl * al * (1.0 - au)) / ao;
                        lp[c] = co.round().clamp(0.0, 255.0) as u8;
                    }
                }
                lp[3] = (ao * 255.0).round().clamp(0.0, 255.0) as u8;
            } else {
                // `MERGE` 的整数化(与 f64 实现**逐位一致**,见下方证明):
                //
                //   round(cl*(1-a/255) + cu*(a/255))
                // = round((cl*(255-a) + cu*a) / 255)
                //
                // 精确值是有理数 `num/255`,距任何半整数 ≥ 1/510 ≈ 0.00196,
                // 而 f64 计算的误差在 1e-13 量级 ⇒ 不可能跨过舍入边界;
                // `round`(正数 half-away)等于 `(num + 127) / 255`。
                // 旧实现每像素 6 次 u8→f64 + 3 次浮点乘 + round + clamp,
                // 在 2048² 合成缓冲上是**计算瓶颈**(实测单条 MERGE 28ms);
                // 整数版只受内存带宽限制。
                let a = u32::from(up[3]);
                let inv = 255 - a;
                for c in 0..3 {
                    let cu = u32::from(up[c]);
                    let cl = u32::from(lp[c]);
                    lp[c] = ((cl * inv + cu * a + 127) / 255) as u8;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, px: [u8; 4]) -> Texture {
        Texture {
            width: w,
            height: h,
            rgba: px.iter().copied().cycle().take((w * h * 4) as usize).collect(),
        }
    }

    /// 手册例 6:彩度 64 = 128 的一半 ⇒ 灰与纯色都向亮度收敛一半。
    #[test]
    fn saido_64_is_half_saturation() {
        let mut t = solid(1, 1, [200, 100, 0, 255]);
        apply_saido(&mut t, 64);
        // luma = (200*77 + 100*150 + 0*29) >> 8 = 118
        // out = 118 + (in - 118) * 0.5  ⇒ 159 / 109 / 59
        assert_eq!(t.rgba[0], 159);
        assert_eq!(t.rgba[1], 109);
        assert_eq!(t.rgba[2], 59);
    }

    /// 手册例 8:明るさ 192 = 1.5 倍。
    #[test]
    fn akarusa_192_is_1_5x() {
        let mut t = solid(1, 1, [100, 50, 20, 255]);
        apply_akarusa(&mut t, 192);
        assert_eq!(&t.rgba[0..3], &[150, 75, 30]);
        assert_eq!(t.rgba[3], 255, "alpha 不变");
    }

    /// 手册例 9:MONO 160 ⇒ 亮度 >=160 白,否则黑。
    #[test]
    fn mono_threshold_splits_black_white() {
        let mut bright = solid(1, 1, [255, 255, 255, 255]);
        apply_mono(&mut bright, 160);
        assert_eq!(&bright.rgba[0..3], &[255, 255, 255]);
        let mut dim = solid(1, 1, [100, 100, 100, 255]);
        apply_mono(&mut dim, 160);
        assert_eq!(&dim.rgba[0..3], &[0, 0, 0]);
    }

    /// 手册例 12:MOSAIC 4 ⇒ 4x4 块内取同色。
    #[test]
    fn mosaic_fills_blocks_uniformly() {
        let mut t = Texture {
            width: 4,
            height: 4,
            rgba: (0..16u8).flat_map(|i| [i * 10, 0, 0, 255]).collect(),
        };
        apply_mosaic(&mut t, 4);
        let first = t.rgba[0];
        for x in 0..4 {
            assert_eq!(t.rgba[x * 4], first, "整块应同色");
        }
    }

    /// 手册例 13:BLUR 8 对纯色图不改变像素(均值不变)。
    #[test]
    fn blur_preserves_uniform_color() {
        let mut t = solid(8, 8, [120, 60, 30, 255]);
        apply_blur(&mut t, 8);
        assert!(t.rgba.chunks_exact(4).all(|p| p == [120, 60, 30, 255]));
    }

    /// `0` 度旋转 / `0` 参数必须是恒等(默认值语义)。
    #[test]
    fn zero_params_are_identity() {
        let base = solid(4, 4, [10, 20, 30, 255]);
        for f in [
            (|t: &mut Texture| apply_rotation(t, 0)) as fn(&mut Texture),
            |t: &mut Texture| apply_akarusa(t, 128),
        ] {
            let mut t = base.clone();
            f(&mut t);
            assert_eq!(t.rgba, base.rgba);
        }
        let mut t = base.clone();
        apply_blur(&mut t, 0);
        assert_eq!(t.rgba, base.rgba);
        let mut t = base.clone();
        apply_mosaic(&mut t, 1);
        assert_eq!(t.rgba, base.rgba);
        let mut t = base.clone();
        apply_raster(&mut t, 5, 0, true, false);
        assert_eq!(t.rgba, base.rgba);
    }

    /// 手册例 23:`MERGE` 只反映上层 alpha ⇒ 上层全透明时下层不变。
    #[test]
    fn merge_transparent_upper_keeps_lower() {
        let mut lower = solid(1, 1, [10, 20, 30, 255]);
        let upper = solid(1, 1, [200, 200, 200, 0]);
        apply_merge(&mut lower, &upper, false);
        assert_eq!(&lower.rgba[0..4], &[10, 20, 30, 255]);
    }

    /// 手册例 24:`MERGE2` 双方 alpha 参与;上层不透明时完全覆盖。
    #[test]
    fn merge2_opaque_upper_overwrites() {
        let mut lower = solid(1, 1, [10, 20, 30, 255]);
        let upper = solid(1, 1, [200, 100, 50, 255]);
        apply_merge(&mut lower, &upper, true);
        assert_eq!(&lower.rgba[0..4], &[200, 100, 50, 255]);
        assert_eq!(lower.rgba[3], 255);
    }

    /// `XOR/AND` 按位作用在 RGB,alpha 保留下层。
    #[test]
    fn bitwise_applies_to_rgb_only() {
        let mut lower = solid(1, 1, [0b1100_0000, 0b1010_1010, 0xFF, 200]);
        let upper = solid(1, 1, [0b1111_0000, 0b0101_0101, 0x0F, 0]);
        apply_bitwise(&mut lower, &upper, false);
        assert_eq!(&lower.rgba[0..4], &[0b1100_0000, 0b0000_0000, 0x0F, 200]);
        let mut lower = solid(1, 1, [0b1100_0000, 0, 0xFF, 200]);
        let upper = solid(1, 1, [0b1111_0000, 0, 0x0F, 0]);
        apply_bitwise(&mut lower, &upper, true);
        assert_eq!(lower.rgba[0], 0b0011_0000);
    }

    /// 参考实现 = 跨度优化前的逐像素全图公式(逐字保留)。
    fn merge_reference(lower: &Texture, upper: &Texture, merge2: bool) -> Texture {
        let mut out = lower.clone();
        let w = out.width.min(upper.width);
        let h = out.height.min(upper.height);
        for y in 0..h {
            for x in 0..w {
                let ui = ((y * upper.width + x) * 4) as usize;
                let li = ((y * out.width + x) * 4) as usize;
                let au = f64::from(upper.rgba[ui + 3]) / 255.0;
                if merge2 {
                    let al = f64::from(out.rgba[li + 3]) / 255.0;
                    let ao = au + al * (1.0 - au);
                    if ao > 0.0 {
                        for c in 0..3 {
                            let cu = f64::from(upper.rgba[ui + c]);
                            let cl = f64::from(out.rgba[li + c]);
                            let co = (cu * au + cl * al * (1.0 - au)) / ao;
                            out.rgba[li + c] = co.round().clamp(0.0, 255.0) as u8;
                        }
                    }
                    out.rgba[li + 3] = (ao * 255.0).round().clamp(0.0, 255.0) as u8;
                } else {
                    for c in 0..3 {
                        let cu = f64::from(upper.rgba[ui + c]);
                        let cl = f64::from(out.rgba[li + c]);
                        let co = cl * (1.0 - au) + cu * au;
                        out.rgba[li + c] = co.round().clamp(0.0, 255.0) as u8;
                    }
                }
            }
        }
        out
    }

    /// 行跨度 + 透明跳过必须**逐位**等于旧公式:随机内容(含整行透明/窄源)。
    #[test]
    fn merge_span_optimization_is_bit_exact() {
        let mut seed = 0x0bad_f00d_dead_beefu64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for case in 0..64 {
            let lw = 1 + (next() % 31) as u32;
            let lh = 1 + (next() % 23) as u32;
            let uw = 1 + (next() % 35) as u32;
            let uh = 1 + (next() % 27) as u32;
            let mut lower = Texture {
                width: lw,
                height: lh,
                rgba: vec![0; (lw * lh * 4) as usize],
            };
            let mut upper = Texture {
                width: uw,
                height: uh,
                rgba: vec![0; (uw * uh * 4) as usize],
            };
            for byte in lower.rgba.iter_mut() {
                *byte = (next() % 256) as u8;
            }
            for byte in upper.rgba.iter_mut() {
                *byte = (next() % 256) as u8;
            }
            // 每 2 例把上层一整行清零(覆盖「跳过整行」路径)。
            if case % 2 == 0 {
                let y = (next() % uh as u64) as usize;
                for byte in &mut upper.rgba[y * uw as usize * 4..(y + 1) * uw as usize * 4] {
                    *byte = 0;
                }
            }
            for merge2 in [false, true] {
                let expected = merge_reference(&lower, &upper, merge2);
                let mut actual = lower.clone();
                apply_merge(&mut actual, &upper, merge2);
                assert_eq!(
                    actual, expected,
                    "case {case} merge2={merge2} (lower {lw}x{lh}, upper {uw}x{uh})"
                );
            }
        }
    }
}
