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
//! 手册无页但在 yscm 表里的键(`SEPIA` / `XOR` / `AND` / `RASTER2/3` /
//! `BLUR2..6` / `ROTATION2` / `TRANSITION2`):`BLUR2..6` 与
//! `DIFFUSION`/`WHIRLPOOL`/`RASTER2/3` 已逐个反编译取证(见各自的
//! `FUN_*` 锚点),其余按同族约定实现,在 `docs/divergences/DIV-GFX-063`
//! 记为 `partial(no-manual)`。

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
    let set = sat as i32;
    for px in texture.rgba.chunks_exact_mut(4) {
        // 官方灰度 = `(r+g+b)/3`(整数截断,查表 `0x436c1c` 一带的
        // `i*0x55555556>>32`),**不是** Rec.601 加权。
        let l = (i32::from(px[0]) + i32::from(px[1]) + i32::from(px[2])) / 3;
        if set < 0x80 {
            let k = 0x80 - set;
            for c in 0..3 {
                let d = l - i32::from(px[c]);
                px[c] = (i32::from(px[c]) + ((d * k) >> 7)) as u8;
            }
        } else {
            // 官方(`0x436d7x..0x436e4x`)先用**原始 k** 逐通道试算,再按
            // 溢出/下溢把 k 收缩;前三条比较的是**原始 k**、后三条比较的是
            // 已收缩的 k(官方原样,不是统一的 min)。除法是有符号截断,
            // `d == 0` 时试算值恒等于 p、两条判断都不会成立 ⇒ 等价于跳过。
            let k0 = set - 0x80;
            let ch = [0usize, 1, 2];
            let p = [i32::from(px[0]), i32::from(px[1]), i32::from(px[2])];
            let d = [p[0] - l, p[1] - l, p[2] - l];
            let trial = [p[0] + ((d[0] * k0) >> 7), p[1] + ((d[1] * k0) >> 7), p[2] + ((d[2] * k0) >> 7)];
            let mut k = k0;
            // 官方六条判断的**比较基准**并不统一(逐指令):
            // 上溢 R 比 `k0`,上溢 G/B 比**已收缩**的 `k`;下溢三条比 `k`。
            // 统一用 `k0` 会让 k 被后面的通道放大回去,破坏前面已成立的约束。
            for &c in &ch {
                if trial[c] > 0xff && d[c] != 0 {
                    let limit = (0xff - p[c]) * 0x80 / d[c];
                    let base = if c == 0 { k0 } else { k };
                    if limit < base {
                        k = limit;
                    }
                }
            }
            for &c in &ch {
                if trial[c] < 0 && d[c] != 0 {
                    let limit = p[c] * -0x80 / d[c];
                    if limit < k {
                        k = limit;
                    }
                }
            }
            for &c in &ch {
                px[c] = (p[c] + ((d[c] * k) >> 7)) as u8;
            }
        }
    }
}

/// `CGACT[AKARUSA=1 SET=b]`:明るさ。`0x80`(128)= 不变。
///
/// 手册例 8「明るさを192に設定」⇒ 192/128 = 1.5 倍。
pub fn apply_akarusa(texture: &mut Texture, value: i64) {
    // 官方 `FUN_00431b40`(action `0x01`)是**加法**亮度偏移,不是乘法:
    // `SET < 0x80` 分支 `out = (px - 0x80) + SET` 并夹下界 0;
    // `SET >= 0x80` 分支查 `DAT_00689e60[SET + px]`,该表逐项等于
    // `clamp(i - 0x80, 0, 255)`(运行时读官方进程内存 400 项零偏差)。
    let off = (value as i32) - 0x80;
    for px in texture.rgba.chunks_exact_mut(4) {
        for c in 0..3 {
            px[c] = (i32::from(px[c]) + off).clamp(0, 255) as u8;
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

/// 盒式模糊的通道口径 —— 官方六个模糊变体里出现的三种写法。
#[derive(Clone, Copy, PartialEq, Eq)]
enum BlurChannels {
    /// 只平均 R/G/B;alpha 保持源值(官方 `flag=0` 分支)。
    Rgb,
    /// R/G/B/A 四通道都平均(官方 `flag=1` 分支)。
    Rgba,
    /// 只平均 alpha,RGB 输出 0(官方 BLUR5,以及 BLUR6 的 `flag=1` 分支)。
    AlphaMask,
}

/// 单遍盒式平均(`horizontal` 选轴)。
///
/// 逐条对齐官方 0.495 的六个模糊体:
/// - 半径 = `max(1, SET)`(由调用方保证);
/// - 窗口在区域边界**收缩**:`lo = max(0, i - r)`、`hi = min(n - 1, i + r)`,
///   除数 = `hi - lo + 1`(不是夹取重复边缘像素);
/// - 每个参与通道各自 `acc / cnt` 整数除法。
fn box_blur_pass(
    src: &[u8],
    w: i64,
    h: i64,
    r: i64,
    horizontal: bool,
    ch: BlurChannels,
) -> Vec<u8> {
    let mut out = vec![0u8; src.len()];
    for y in 0..h {
        for x in 0..w {
            let (lo, hi) = if horizontal {
                ((x - r).max(0), (x + r).min(w - 1))
            } else {
                ((y - r).max(0), (y + r).min(h - 1))
            };
            let mut acc = [0u32; 4];
            for k in lo..=hi {
                let p = if horizontal {
                    texel(src, w, h, k, y)
                } else {
                    texel(src, w, h, x, k)
                };
                match ch {
                    BlurChannels::Rgb | BlurChannels::Rgba => {
                        for c in 0..3 {
                            acc[c] += u32::from(p[c]);
                        }
                        if ch == BlurChannels::Rgba {
                            acc[3] += u32::from(p[3]);
                        }
                    }
                    BlurChannels::AlphaMask => acc[3] += u32::from(p[3]),
                }
            }
            let cnt = (hi - lo + 1) as u32;
            let di = (y as usize * w as usize + x as usize) * 4;
            match ch {
                BlurChannels::Rgb => {
                    for c in 0..3 {
                        out[di + c] = (acc[c] / cnt) as u8;
                    }
                    // 官方 `flag=0` 分支不写目标缓冲的 alpha(BLUR2 连拷贝都
                    // 没有):该层无有效 alpha、alpha 不参与绘制。这里统一带回
                    // 源 alpha,保证内核的缓冲始终是确定值。
                    out[di + 3] = src[di + 3];
                }
                BlurChannels::Rgba => {
                    for c in 0..4 {
                        out[di + c] = (acc[c] / cnt) as u8;
                    }
                }
                BlurChannels::AlphaMask => out[di + 3] = (acc[3] / cnt) as u8,
            }
        }
    }
    out
}

fn blur_pass_in_place(texture: &mut Texture, radius: i64, horizontal: bool, ch: BlurChannels) {
    let r = radius.max(0);
    if r == 0 {
        return;
    }
    let (w, h) = (texture.width as i64, texture.height as i64);
    if w == 0 || h == 0 {
        return;
    }
    texture.rgba = box_blur_pass(&texture.rgba, w, h, r, horizontal, ch);
}

fn blur_two_pass(texture: &mut Texture, radius: i64, ch: BlurChannels) {
    let r = radius.max(0);
    if r == 0 {
        return;
    }
    let (w, h) = (texture.width as i64, texture.height as i64);
    if w == 0 || h == 0 {
        return;
    }
    let mid = box_blur_pass(&texture.rgba, w, h, r, true, ch);
    texture.rgba = box_blur_pass(&mid, w, h, r, false, ch);
}

/// `CGACT[BLUR=1 SET=r]`(action `0x02`)/ `BLUR4`(action `0x05`):
/// **水平遍 → 临时缓冲 → 垂直遍**。
///
/// 手册例 13「レイヤをぼかす」`SET=8`。算法逐条取自官方 0.495:
/// - `FUN_00431d28`(BLUR)与 `FUN_00433718`(BLUR4)是同一种可分离盒式的
///   两种实现(BLUR 用预计算夹取表 + 直接求和,BLUR4 用运行和),像素结果
///   相同;`FUN_00434b7c`(BLUR6)的 `flag=0` 分支也是同一算法;
/// - 半径 = `max(1, SET)`(六个变体 prologue 一致);
/// - **alpha 口径由 `[层+0x86]` 选**:`flag=0` 时只平均 R/G/B、alpha 从源图
///   原样拷贝;`flag=1` 时四通道一起平均。
///
/// `alpha_aware` 即 `[层+0x86]`(见 `yu_gfx::Layer::alpha_aware`)。
pub fn apply_blur(texture: &mut Texture, radius: i64, alpha_aware: bool) {
    blur_two_pass(
        texture,
        radius,
        if alpha_aware {
            BlurChannels::Rgba
        } else {
            BlurChannels::Rgb
        },
    );
}

/// `CGACT[BLUR2=1 SET=r]`(action `0x03`):**只有水平遍**。
///
/// 证据 `FUN_0043265c`:外循环按行推进、内层只沿 x 做运行和;函数开头
/// `malloc` 的临时缓冲**从未被写入**(只 free),即官方确实只做了水平遍。
/// `flag=0` 只写 R/G/B;`flag=1` 四通道平均。
pub fn apply_blur_h(texture: &mut Texture, radius: i64, alpha_aware: bool) {
    blur_pass_in_place(
        texture,
        radius,
        true,
        if alpha_aware {
            BlurChannels::Rgba
        } else {
            BlurChannels::Rgb
        },
    );
}

/// `CGACT[BLUR3=1 SET=r]`(action `0x04`):**只有垂直遍**。
///
/// 证据 `FUN_00432e58`:外循环按列推进、内层只沿 y 做运行和。`flag=0` 分支
/// 在每段写回时都带 `*(dst+3) = *(src+3)`,即 alpha 从源图拷贝;
/// `flag=1` 四通道平均。
pub fn apply_blur_v(texture: &mut Texture, radius: i64, alpha_aware: bool) {
    blur_pass_in_place(
        texture,
        radius,
        false,
        if alpha_aware {
            BlurChannels::Rgba
        } else {
            BlurChannels::Rgb
        },
    );
}

/// `CGACT[BLUR5=1 SET=r]`(action `0x06`)与 BLUR6 的 `flag=1` 分支:
/// **alpha-only 两遍盒式,RGB 归零**。
///
/// 证据 `FUN_00434748`(BLUR5):函数**没有** flag 分支,两遍都只对 alpha
/// 求和,写回 `(0, 0, 0, avg)`;第一遍写进临时缓冲,第二遍从临时缓冲的
/// alpha 再垂直平均。`FUN_00434b7c`(BLUR6)的 `flag=1` 分支逐行同式
/// ⇒ 对话阴影层就是这样变成「黑底 + 模糊 alpha」的。
pub fn apply_blur_alpha_mask(texture: &mut Texture, radius: i64) {
    blur_two_pass(texture, radius, BlurChannels::AlphaMask);
}

// ---------------------------------------------------------------- 扩散/旋涡/拉斯特
//
// 这三族的动作码与执行体(0.495 `yu-ris.exe`,sha256 `4f5eefa3…81138f`):
//
// | 动作 | 声明下标 | action | 执行体 |
// |---|---|---|---|
// | `DIFFUSION` | 53 | `0x0a` | `FUN_004358d4` |
// | `RASTER` | 54 | `0x15` | 分派 `FUN_0042f809` 内联 |
// | `RASTER2` | 55 | `0x16` | 分派内联 |
// | `RASTER3` | 56 | `0x17` | `FUN_00431658`(`variant=3`) |
// | `WHIRLPOOL` | 61 | `0x27` | `FUN_00437368` |
//
// wgpu 侧是同一套公式的 `transform.wgsl` 模式 16..20(真 GPU pass),
// 两个后端逐像素对拍;本文件是像素参考实现。
//
// **取整口径**:这三族里所有「浮点 → 整数」在官方都是 `fistp` + FPU 控制字
// `or 0xC00`(RC=11)= **向零截断**,不是四舍五入(`0x43174b`/`0x4317a3`
// 的 RASTER 位移、`0x4373ed`/`0x437490`/`0x4374af`/`0x437592` 的旋涡
// 表与半径)。Rust 的 `as i64/i32`、WGSL 的 `i32(f32)` 都是向零截断,同义;
// 早期实现误用了 `round_ties_even`,会系统性偏 1 像素。

/// 一次官方 `_rand()`(MSVC LCG),推进 `seed`。
///
/// 证据 `0x46ac0c`:`seed = seed*0x343fd + 0x269ec3; return (seed>>16) & 0x7fff;`,
/// 状态在 CRT 线程局部区 `[td+0x14]`,**默认种子 1**(全 exe 无 `srand`);
/// 全 exe 只有 `FUN_004358d4`(DIFFUSION)两处调用它 ⇒ 该流只被扩散消耗。
fn msvc_rand(seed: &mut u32) -> u32 {
    *seed = seed.wrapping_mul(0x34_3fd).wrapping_add(0x26_9ec3);
    (*seed >> 16) & 0x7fff
}

/// 0.495 `0x5b4780` 的 24 行 × 4 字节置换表,按**像素序号**记
/// (原始字节是 0/4/8/12 的字节偏移,除以 4 即像素序号)。
///
/// 目标像素顺序 = `[(x,y), (x+1,y), (x,y+1), (x+1,y+1)]`。
const DIFFUSION_PERM: [[u8; 4]; 24] = [
    [3, 2, 1, 0],
    [2, 3, 1, 0],
    [3, 1, 2, 0],
    [1, 3, 2, 0],
    [2, 1, 3, 0],
    [1, 2, 3, 0],
    [3, 2, 0, 1],
    [2, 3, 0, 1],
    [3, 0, 2, 1],
    [0, 3, 2, 1],
    [2, 0, 3, 1],
    [0, 2, 3, 1],
    [3, 1, 0, 2],
    [1, 3, 0, 2],
    [3, 0, 1, 2],
    [0, 3, 1, 2],
    [1, 0, 3, 2],
    [0, 1, 3, 2],
    [2, 1, 0, 3],
    [1, 2, 0, 3],
    [2, 0, 1, 3],
    [0, 2, 1, 3],
    [1, 0, 2, 3],
    [0, 1, 2, 3],
];

/// 把 `(x,y)` 起的 2×2 块按一次随机置换重排(**只动 RGB,alpha 原样保留**)。
fn permute_block(rgba: &mut [u8], width: usize, x: usize, y: usize, seed: &mut u32) {
    let perm = DIFFUSION_PERM[(msvc_rand(seed) % 24) as usize];
    let base = (y * width + x) * 4;
    let next_row = base + width * 4;
    // 先快照 4 个像素的 RGB(写入会互相覆盖)。
    let px = |i: usize| -> [u8; 3] { [rgba[i], rgba[i + 1], rgba[i + 2]] };
    let src = [px(base), px(base + 4), px(next_row), px(next_row + 4)];
    let dst = [base, base + 4, next_row, next_row + 4];
    for (k, at) in dst.iter().enumerate() {
        let from = src[perm[k] as usize];
        rgba[*at] = from[0];
        rgba[*at + 1] = from[1];
        rgba[*at + 2] = from[2];
    }
}

/// `CGACT[DIFFUSION=1 SET=s]`:扩散。
///
/// 证据 `FUN_004358d4`(0x4358d4..0x435d5e):
/// 1. 先把整面 `memcpy` 到新缓冲(`0x435912`,MMX `memcpy`),再在新缓冲上就地
///    置换 —— 结果与本实现的就地置换等价(同一遍内 2×2 块互不重叠);
/// 2. `SET/2`(C 的向零取整)轮,每轮两遍:
///    - 正向:`y = y0, y0+2, … < y1`,`x = x0+it, x0+it+2, … < x1`;
///    - 反向:`y = y1-1, y1-3, … >= y0`,`x = x1-1-it, x1-3-it, …`
///      (次数 = `ceil((x1-x0-it)/2)`,与正向同数);
/// 3. 每个 2×2 块取一次 `rand()%24`,按 [`DIFFUSION_PERM`] 重排四个像素,
///    **只写 RGB**(汇编只写 `+0/+1/+2/+4/+5/+6`,alpha 字节不动)。
///
/// 整层调用时 `x0 = y0 = 0`、`x1 = w-1`、`y1 = h-1`。
pub fn apply_diffusion(texture: &mut Texture, strength: i64, seed: &mut u32) {
    let width = texture.width as usize;
    let height = texture.height as usize;
    if width < 2 || height < 2 {
        return;
    }
    let passes = strength / 2;
    if passes <= 0 {
        return;
    }
    let (x0, y0) = (0i64, 0i64);
    let (x1, y1) = (width as i64 - 1, height as i64 - 1);
    let rgba = &mut texture.rgba;
    for it in 0..passes {
        // 正向:y 从 y0 步进 2;x 从 x0+it 步进 2。
        let mut y = y0;
        while y < y1 {
            let mut x = x0 + it;
            while x < x1 {
                permute_block(rgba, width, x as usize, y as usize, seed);
                x += 2;
            }
            y += 2;
        }
        // 反向:y 从 y1-1 步进 -2;x 从 x1-1-it 步进 -2,次数与正向同。
        let mut y = y1 - 1;
        while y >= y0 {
            let mut x = x1 - 1 - it;
            let mut counter = x0 + it;
            while counter < x1 {
                permute_block(rgba, width, x as usize, y as usize, seed);
                x -= 2;
                counter += 2;
            }
            y -= 2;
        }
    }
}

/// `CGACT[WHIRLPOOL=1 SET=progress]`:旋涡。
///
/// 证据 `FUN_00437368`(0x437368..0x4376e0):
/// - 中心 `(w/2, h/2)`(整数除法);
/// - 半径表长度 `maxr = trunc((hypot(w,h)+1)*0.5 + 1)`;
/// - 角度表 `angle(r) = progress/10 * (1 - r/maxr)` 度,
///   `cos_tab[r] = trunc(cos(angle)·256)`、`sin_tab[r] = trunc(sin(angle)·256)`
///   (常量 `0x58ed38` = `0.017453292519444445`,**不是** `π/180`);
/// - 每个像素取 `r = trunc(hypot(x-cx, y-cy))`,再按
///   `sx = ((-(y-cy)·sin + (x-cx)·cos) >> 8) + cx`、
///   `sy = (((x-cx)·sin + cos·(y-cy)) >> 8) + cy` 采样(`>>` 为算术右移),
///   最后各自夹取到 `[0, w-1]` / `[0, h-1]`。
pub fn apply_whirlpool(texture: &mut Texture, progress: i64) {
    let width = texture.width as i64;
    let height = texture.height as i64;
    if width == 0 || height == 0 {
        return;
    }
    let cx = width / 2;
    let cy = height / 2;
    let maxr = whirlpool_maxr(width, height);
    if maxr <= 0 {
        return;
    }
    // 官方常量 `0x58ed38`(double),与 `std::f64::consts::PI/180` 差在末位。
    const DEG2RAD: f64 = f64::from_bits(0x3f91_df46_a250_6b91);
    let step = -(progress as f64) / (maxr as f64 * 10.0);
    let mut angle = progress as f64 / 10.0;
    let mut cos_tab = Vec::with_capacity(maxr as usize);
    let mut sin_tab = Vec::with_capacity(maxr as usize);
    for _ in 0..maxr {
        let radians = angle * DEG2RAD;
        // 官方是 `fistp` + FPU 控制字 `0xC00`(RC=11)**向零截断**,
        // 不是四舍五入(`0x4373ed`/`0x437490`/`0x4374af` 的 `or eax,0xc00`)。
        cos_tab.push((radians.cos() * 256.0) as i32);
        sin_tab.push((radians.sin() * 256.0) as i32);
        angle += step;
    }
    let src = texture.rgba.clone();
    let stride = width as usize;
    for y in 0..height {
        for x in 0..width {
            let dx = x - cx;
            let dy = y - cy;
            let dist = (((dx * dx + dy * dy) as f64).sqrt()) as usize;
            let r = dist.min(cos_tab.len() - 1);
            let (sin, cos) = (i64::from(sin_tab[r]), i64::from(cos_tab[r]));
            let mut sx = ((-dy * sin + dx * cos) >> 8) + cx;
            let mut sy = ((dx * sin + cos * dy) >> 8) + cy;
            sx = sx.clamp(0, width - 1);
            sy = sy.clamp(0, height - 1);
            let from = (sy as usize * stride + sx as usize) * 4;
            let to = (y as usize * stride + x as usize) * 4;
            texture.rgba[to..to + 4].copy_from_slice(&src[from..from + 4]);
        }
    }
}

/// `FUN_00437368` 的半径表长度 `trunc((hypot(w,h)+1)*0.5 + 1)`。
///
/// wgpu 侧要把同一个值当标量传给 shader(着色器里没有 f64,
/// 大尺寸时 `f32(w*w+h*h)` 会丢精度),所以这里单独给出。
fn whirlpool_maxr(width: i64, height: i64) -> i64 {
    let hypot = ((width * width + height * height) as f64).sqrt();
    ((hypot + 1.0) * 0.5 + 1.0) as i64
}

/// `RASTER` 族的轴选择(官方 `FUN_00431658` 的 `variant` 参数 1/2/3)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RasterAxis {
    /// `RASTER`(action `0x15`):只有水平位移(正弦按**行**)。
    Horizontal,
    /// `RASTER2`(action `0x16`):只有垂直位移(余弦按**列**)。
    Vertical,
    /// `RASTER3`(action `0x17`):两者叠加。
    Both,
}

/// `CGACT[RASTER=1 SET=amp SET2=phase]`:拉斯特滚动。
///
/// 证据:分派 `FUN_0042f809` 内联的 `0x15`/`0x16` 两支,以及
/// `FUN_00431658`(variant 1/2/3)。三者像素公式一致:
///
/// ```text
/// 行 y 的位移  shift_x = trunc(sin((y + SET2)°) · amp_x)
/// 列 x 的位移  shift_y = trunc(cos((x + SET2)°) · amp_y)
/// 采样        src[clamp(y + shift_y, y0, y1)][clamp((x-x0) + shift_x, 0, x1-1)]
/// ```
///
/// - `amp` = `SET`、`phase` = `SET2`;
/// - 角度 = `(行号或列号 + SET2)` 的**绝对**序号,先乘 f32 常量
///   `0.017453292` 再交给 `sin`/`cos`(x87 `fsin`/`fcos`);
/// - 水平采样下标是**区域相对**的 `(x-x0)+shift_x`,夹取上界却用绝对
///   `x1-1`(官方原样:末列永远复用倒数第二列);
/// - `RASTER` 只用 `sin`、`RASTER2` 只用 `cos`、`RASTER3` 两者都用。
pub fn apply_raster(texture: &mut Texture, amplitude: i64, phase: i64, axis: RasterAxis) {
    let width = texture.width as i64;
    let height = texture.height as i64;
    if width == 0 || height == 0 {
        return;
    }
    // 官方 f32 常量 `0.017453292`(不是 f64 的 π/180)。
    const DEG2RAD_F32: f32 = 0.017_453_292;
    let (x0, y0) = (0i64, 0i64);
    let (x1, y1) = (width - 1, height - 1);
    let amp = amplitude as f64;
    let amp_x = if axis == RasterAxis::Vertical { 0.0 } else { amp };
    let amp_y = if axis == RasterAxis::Horizontal { 0.0 } else { amp };
    let sx_max = (x1 - 1).max(0);
    let src = texture.rgba.clone();
    let stride = width as usize;
    for y in y0..=y1 {
        let angle_y = (y + phase) as f32 * DEG2RAD_F32;
        // 官方 `fistp` + 控制字 `0xC00` = **向零截断**(`0x4317a3`/`0x43174b`
        // 的 `or eax,0xc00`),Rust 的 `as i64` 同义。
        let shift_x = (f64::from(angle_y).sin() * amp_x) as i64;
        for x in x0..=x1 {
            let angle_x = (x + phase) as f32 * DEG2RAD_F32;
            let shift_y = (f64::from(angle_x).cos() * amp_y) as i64;
            let sx = ((x - x0) + shift_x).clamp(0, sx_max);
            let sy = (y + shift_y).clamp(y0, y1);
            let from = (sy as usize * stride + sx as usize) * 4;
            let to = (y as usize * stride + x as usize) * 4;
            texture.rgba[to..to + 4].copy_from_slice(&src[from..from + 4]);
        }
    }
}

/// `CGACT[XOR=1]` / `CGACT[AND=1]`:两图按位合成。
///
/// 手册无页(键在 yscm 表内)⇒ `partial(no-manual)`:RGB 按位运算、
/// alpha 保留下层。上层 = `ID`,下层 = `ID2`(与 MERGE 同向)。
pub fn apply_bitwise(lower: &mut Texture, upper: &Texture, xor: bool) {
    apply_bitwise_at(lower, upper, xor, 0, 0);
}

/// 同 [`apply_bitwise`],上层按 `(dx, dy)` 落点贴到下层(见 [`apply_merge_at`])。
pub fn apply_bitwise_at(lower: &mut Texture, upper: &Texture, xor: bool, dx: i64, dy: i64) {
    let (uw, uh) = (upper.width as i64, upper.height as i64);
    if uw == 0 || uh == 0 || lower.width == 0 || lower.height == 0 {
        return;
    }
    let x_start = dx.max(0);
    let y_start = dy.max(0);
    let x_end = (dx + uw).min(i64::from(lower.width));
    let y_end = (dy + uh).min(i64::from(lower.height));
    let w = (x_end - x_start).max(0) as u32;
    let h = (y_end - y_start).max(0) as u32;
    let ux0 = (x_start - dx) as u32;
    let uy0 = (y_start - dy) as u32;
    for y in 0..h {
        for x in 0..w {
            let tx = x_start as u32 + x;
            let ty = y_start as u32 + y;
            let ui = (((uy0 + y) * upper.width + (ux0 + x)) * 4) as usize;
            let li = ((ty * lower.width + tx) * 4) as usize;
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
    // 官方 `FUN_004371cc`(action `0x26`),逐指令复刻:
    //   l10 = 117·B + 601·G + 306·R      (内存序 BGRA,权重即 Rec.601)
    //   base = l10 << 10
    //   R' = trunc((base + 0x11E1400) / 2^20)
    //   G' = trunc((base - 0x46361)   / 2^20)
    //   B' = trunc((base - 0x2C33311) / 2^20)
    // `trunc` 由 `(x>>19)>>12`(负数得 0xFFFFF)实现,即向零截断;
    // 末尾各自夹到 [0,255]。
    const C_R: i64 = 0x0011_e1400;
    const C_G: i64 = -0x4_6361;
    const C_B: i64 = -0x2c3_3311;
    fn trunc_div_2p20(x: i64) -> i64 {
        let corr = if x < 0 { (1 << 20) - 1 } else { 0 };
        (x + corr) >> 20
    }
    for px in texture.rgba.chunks_exact_mut(4) {
        let (r, g, b) = (
            i64::from(px[0]),
            i64::from(px[1]),
            i64::from(px[2]),
        );
        let base = (117 * b + 601 * g + 306 * r) << 10;
        px[0] = trunc_div_2p20(base + C_R).clamp(0, 255) as u8;
        px[1] = trunc_div_2p20(base + C_G).clamp(0, 255) as u8;
        px[2] = trunc_div_2p20(base + C_B).clamp(0, 255) as u8;
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
///
/// 官方 MERGE 内核的两张查找表(运行时构造,`FUN_0045c04c`)。
///
/// - `t[la<<8 | ua] = la·ua/255`——REA 0.495 `0x62f4de`(字表;同一构造循环
///   在 `0x45c60a` 生成同式的字节表 `0x68a2e0`,magic `0x80808081>>7` 即除以 255)。
/// - `u[a<<8 | j] = j·256/a`——`0x78b6e0`,构造循环在 `0x45c58f`:
///   `MOV EAX,ESI; CDQ; IDIV EBX`(ESI 以 256 步进,EBX = 行号 1..255)。
///   行 0 从不写入(全 0),对应 `a'=0` 时权重 0。
///
/// 官方在启动时把这两张表铺成 256×256;这里首次合并时惰性构造一次,
/// 热循环里只剩乘法,与官方「不用除法的内层循环」同形。
struct MergeTables {
    t: Vec<u16>,
    u: Vec<u16>,
}

static MERGE_TABLES: std::sync::OnceLock<MergeTables> = std::sync::OnceLock::new();

fn merge_tables() -> &'static MergeTables {
    MERGE_TABLES.get_or_init(|| {
        let mut t = vec![0u16; 1 << 16];
        let mut u = vec![0u16; 1 << 16];
        for la in 0u32..256 {
            for ua in 0u32..256 {
                t[((la << 8) | ua) as usize] = (la * ua / 255) as u16;
            }
        }
        for a in 1u32..256 {
            for j in 0u32..256 {
                u[((a << 8) | j) as usize] = (j * 256 / a) as u16;
            }
        }
        MergeTables { t, u }
    })
}

pub fn apply_merge(lower: &mut Texture, upper: &Texture, merge2: bool) {
    apply_merge_at(lower, upper, merge2, 0, 0);
}

/// [`apply_merge`] 的**层不透明度**版本(REA 0.495 完全对齐)。
///
/// 官方把图层不透明度 `[层+0x80]`(0..256)作为独立乘数传给混合内核:
///
/// ```text
/// MERGE  FUN_0043b33c : blend(dst, src, srcLayerAlpha, span)
/// MERGE2 FUN_0043b8ec : blend(dst, src, dstLayerAlpha, srcLayerAlpha, span)
/// ```
///
/// ⇒ `MERGE` **只用上层的**,`MERGE2` 两层都用 —— 与手册例 23/24 一致。
/// 传 `256` 表示「不透明」(恒等乘数),`apply_merge` 即其特例。
///
/// 乘数口径:两个层 alpha 都按 `/256` 归一化,先乘进**像素** alpha 得到
/// 各自的「有效 alpha」,再做归一化 over:
///
/// ```text
/// a_eff  = round(a_src  * srcLayerAlpha / 256)
/// la_eff = round(a_dst  * dstLayerAlpha / 256)     // MERGE 时 dstLayerAlpha=256
/// ao     = a_eff + la_eff·(1 − a_eff)
/// co     = (cu·a_eff + cl·la_eff·(1−a_eff)) / ao
/// ```
///
/// 这一步的**精确取整口径未经 RE 证实**(查找表在 BSS,静态不可读);采用
/// `round(x·L/256)` 的理由是它在本仓库已知语料上退化正确:
/// `L=256` 时 `a_eff ≡ a`,与原实现**逐位相同**,
/// `MERGE` 的 `dstLayerAlpha=256` 也保住「下层透明不泄色」这条硬约束。
pub fn apply_merge_layers(
    lower: &mut Texture,
    upper: &Texture,
    merge2: bool,
    src_layer_alpha: u32,
    dst_layer_alpha: u32,
) {
    if src_layer_alpha == 256 && (dst_layer_alpha == 256 || !merge2) {
        // 恒等乘数 ⇒ 走原路径(零额外开销,且保证逐位一致)。
        apply_merge_at(lower, upper, merge2, 0, 0);
        return;
    }
    apply_merge_at_layers(
        lower,
        upper,
        merge2,
        0,
        0,
        src_layer_alpha,
        if merge2 { dst_layer_alpha } else { 256 },
    );
}

/// 同 [`apply_merge`],但上层按 `(dx, dy)` **落点**贴到下层(`CGACT[X= Y=]`)。
///
/// 语义按「一次带纹理 quad」建模:上层 `(ux,uy)` 写目标的 `(ux+dx, uy+dy)`,
/// 目标像素 `p` 取上层 `p-(dx,dy)`;越界部分不改目标(quad 只覆盖自己的
/// 矩形,与 CPU 循环上界一致)。
///
/// **勘误(2026-10-31)**:此处曾把落点语义归到官方 `FUN_004405a4`
/// (0x4405a4)——那是 `EMOTE` 域处理体(依赖 YSEmote.dll),**不是** CGACT
/// 的 MERGE。落点建模本身仍合理,但目前**没有 RE 锚点**,只剩手册文字与
/// 像素实测支撑。
pub fn apply_merge_at(lower: &mut Texture, upper: &Texture, merge2: bool, dx: i64, dy: i64) {
    apply_merge_at_layers(lower, upper, merge2, dx, dy, 256, 256)
}

/// [`apply_merge_at`] 的完整形态:额外吃两个**图层不透明度**(0..256)。
///
/// `src_layer_alpha` = 上层(`ID`)的 `[层+0x80]`;
/// `dst_layer_alpha` = 下层(`ID2`)的 `[层+0x80]`,**只在 `merge2` 时生效**
/// (MERGE 的执行体 `FUN_0043b33c` 根本不传这个参数)。
///
/// 乘数如何进入算术:先把层 alpha 折成**有效像素 alpha**
/// `a_eff = round(a_px · L / 256)`,再走与 [`apply_merge_at`] 完全相同的
/// 归一化 over。`L=256` 时 `a_eff ≡ a_px`,逐位退化为原式 —— 这就是
/// 下面那个 `L==256` 快路径能直接调用原循环的原因。
///
/// 取整口径(`round(x·L/256)`)未获 RE 证实(官方查找表在 BSS),但它是
/// 唯一同时满足「L=256 恒等」与「下层透明不泄色」两条硬约束的简单形式;
/// 残差已记 `DIV-GFX-063` §2.5。
pub fn apply_merge_at_layers(
    lower: &mut Texture,
    upper: &Texture,
    merge2: bool,
    dx: i64,
    dy: i64,
    src_layer_alpha: u32,
    dst_layer_alpha: u32,
) {
    let src_la = src_layer_alpha.min(256);
    let dst_la = if merge2 { dst_layer_alpha.min(256) } else { 256 };
    let (uw, uh) = (upper.width as i64, upper.height as i64);
    if uw == 0 || uh == 0 || lower.width == 0 || lower.height == 0 {
        return;
    }
    // 落点矩形与下层表面求交:只有这段目标像素会被写。
    let x_start = dx.max(0);
    let y_start = dy.max(0);
    let x_end = (dx + uw).min(i64::from(lower.width));
    let y_end = (dy + uh).min(i64::from(lower.height));
    let w = (x_end - x_start).max(0) as usize;
    let h = (y_end - y_start).max(0) as usize;
    if w == 0 || h == 0 {
        return;
    }
    let lstride = lower.width as usize * 4;
    let ustride = upper.width as usize * 4;
    let ux0 = (x_start - dx) as usize;
    let uy0 = (y_start - dy) as usize;
    // 逐行切片 + `chunks_exact_mut(4)` 配对:去掉逐像素的下标/边界检查。
    //
    // **上层像素 alpha 为 0 时是否恒等**,两族不同,决定下面的跨度裁剪能不能用:
    // - `MERGE`:T=0、w=0 ⇒ `a' = L.a`、`U' = 0` ⇒ 四通道恒等;
    // - `MERGE2`:只有 `dstA(下层 [层+0x80]) == 256` 才恒等。`dstA < 256` 时
    //   `a' = a1 = L.a·dstA>>8 ≠ L.a`,alpha 会被压暗 —— 这时**不能**跳过,
    //   否则透明区的 alpha 与官方不一致(官方按交集矩形整块扫,不看 alpha 跨度)。
    let skip_transparent_upper = !merge2 || dst_la == 256;
    // 查表一次性取出:热循环里只剩乘法(官方内层循环也没有除法)。
    let tb = merge_tables();
    for y in 0..h {
        let uy = uy0 + y;
        let urow = &upper.rgba[uy * ustride + ux0 * 4..uy * ustride + (ux0 + w) * 4];
        // 整行透明且恒等 ⇒ 连目标行都不用读/写(精灵部件在 2048² 缓冲里
        // 通常只占中间一块,旧实现逐行扫满整个宽度)。
        let (x0, x1) = if skip_transparent_upper {
            let span = crate::imageop::alpha_span(urow, w);
            if span.0 > span.1 {
                continue;
            }
            span
        } else {
            (0, w - 1)
        };
        let dy_row = (y_start as usize) + y;
        let dx_col = (x_start as usize) + x0;
        let lrow = &mut lower.rgba
            [dy_row * lstride + dx_col * 4..dy_row * lstride + (dx_col + (x1 - x0) + 1) * 4];
        let urow = &urow[x0 * 4..(x1 + 1) * 4];
        for (lp, up) in lrow.chunks_exact_mut(4).zip(urow.chunks_exact(4)) {
            // 官方两个内核都按「**下层 dst / 上层 src**」的 lerp 写:
            //   co = L.c + ((U.c − L.c) · 权重) >> k
            // 权重随族不同(MERGE 走查表,MERGE2 走 idiv),见下两分支。
            //
            // 这条 lerp 形式天然满足「下层不可见处不泄色」:
            // 素材 PNG 在透明区常带残留 RGB(kotoneiro 立绘缓冲 `SPLBUF702`
            // 实测 26773 个 alpha=0 但 RGB 非零的像素,均值 159,117,122)。
            // 下层 alpha=0 ⇒ 权重=0 ⇒ `co = L.c`,脏色永远进不了可见区。
            // 2b53786 之前的 `cl·(1−a) + cu·a` 没有这一步预乘,差分抗锯齿边
            // (a≈128)处把脏色按 50% 拉进可见区 —— 就是眨眼时的眼罩伪影。
            let la_px = u32::from(lp[3]);
            let ua_px = u32::from(up[3]);
            if ua_px == 0 && !merge2 {
                // MERGE:a=0 ⇒ T=0、w=0、a'=la、U=0 ⇒ 四个通道恒等。
                continue;
            }
            if merge2 {
                // MERGE2(`0x43b8ec` → 内核 `0x402654`,逐字节手解):
                //
                //   a1 = L.a · dstA >> 8                ; dstA = **下层**的 [层+0x80]
                //   t  = (U.a + 1) · a1 >> 8            ; `+1` 在乘法左操作数上
                //   w  = (U.a − t) · srcA >> 8          ; srcA = **上层**的 [层+0x80]
                //   a' = a1 + w
                //   co = L.c + (((U.c − L.c) · srcA) >> 8) · U.a / (a'+1)
                //
                // 注意三处与 MERGE 不同:①下层的层 alpha 乘进**下层自己的**
                // 像素 alpha(所以 MERGE2 才叫「双方都反映」);②RGB 用
                // `(a'+1)` 做分母而不是查表;③除法是 `idiv`(**向零截断**,
                // 不是向下取整)。因此两者在层 alpha≡256 时也**不是**逐位
                // 相同,差 1 LSB 属正常,`DIV-GFX-063` 的用例按此放宽。
                let a1 = (la_px * dst_la) >> 8;
                let t = ((ua_px + 1) * a1) >> 8;
                let w = (((ua_px as i32) - (t as i32)) * (src_la as i32)) >> 8;
                let a_out = (a1 as i32 + w).clamp(0, 255);
                let den = a_out + 1;
                for c in 0..3 {
                    let delta = (((i32::from(up[c])) - (i32::from(lp[c])))
                        * (src_la as i32))
                        >> 8;
                    // `idiv` 向零截断 ⇒ Rust 的 `/` 同义(负数也一致)。
                    let v = i32::from(lp[c]) + delta * (ua_px as i32) / den;
                    lp[c] = v.clamp(0, 255) as u8;
                }
                lp[3] = a_out as u8;
            } else {
                // MERGE(`0x43b33c` → 内核 `0x402828`,逐字节手解):
                //
                //   T   = L.a · U.a / 255               ; 查表 0x62f4de
                //   w   = (U.a − T) · srcA >> 8         ; srcA = **上层**的 [层+0x80]
                //   a'  = L.a + w
                //   U'  = U.a · 256 / a'                ; 查表 0x78b6e0
                //   co  = L.c + ((U.c − L.c) · U' · srcA) >> 16
                //
                // 两张表都是运行时构造(`FUN_0045c04c`):
                //   T[la][ua] = la·ua/255               (`0x68a2e0` 同式字节表)
                //   U'[a][j]  = j·256/a                 (`0x45c58f` 的 idiv 循环)
                // 这里用同一组值预建缓存(`merge_tables`,循环外取一次)。
                // `srcA=256` 时 `U' = U.a·256/a'`,`co = L + (U−L)·U.a/a'`。
                let t = u32::from(tb.t[((la_px << 8) | ua_px) as usize]);
                let w = (((ua_px as i32) - (t as i32)) * (src_la as i32)) >> 8;
                let a_out = (la_px as i32 + w).clamp(0, 255) as u32;
                lp[3] = a_out as u8;
                let u_w = i32::from(tb.u[((a_out << 8) | ua_px) as usize]);
                // `prod ≤ 256·256`、`delta ≤ 255` ⇒ 乘积 ≤ 1.7e7,不会溢出
                // 官方的 32 位 `IMUL`(用普通乘法即可,截断由 `>>16` 负责)。
                let prod = u_w * src_la as i32;
                for c in 0..3 {
                    let delta = i32::from(up[c]) - i32::from(lp[c]);
                    let v = i32::from(lp[c]) + ((delta * prod) >> 16);
                    lp[c] = v.clamp(0, 255) as u8;
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

    /// 手册例 6 的 `SET=64`(官方 `FUN_00436bdc`)。
    ///
    /// 官方灰度是 `(r+g+b)/3` = (200+100+0)/3 = 100(整数截断,
    /// **不是** Rec.601);`SET<128` ⇒ `k = 128-64 = 64`,
    /// `out = px + ((l-px)*k >> 7)`:
    ///   R: 200 + (-100*64 >> 7) = 200 - 50 = 150
    ///   G: 100 + (0*64 >> 7)    = 100
    ///   B: 0   + (100*64 >> 7)  = 50
    #[test]
    fn saido_64_lerps_toward_plain_average() {
        let mut t = solid(1, 1, [200, 100, 0, 255]);
        apply_saido(&mut t, 64);
        assert_eq!(&t.rgba[0..3], &[150, 100, 50]);
        assert_eq!(t.rgba[3], 255, "alpha 不变");
    }

    /// `SET=192` 的增饱和分支:先按原始 k 试算,再按越界收缩 k。
    ///
    /// `l = (200+100+0)/3 = 100`,`k0 = 192-128 = 64`;
    /// 试算 R = 200 + (100*64>>7) = 250(不越界)、G = 100、B = -50(下溢)
    /// ⇒ B 把 k 收到 `0*-128/(0-100) = 0` ⇒ 输出 [200, 100, 0] 不变。
    #[test]
    fn saido_192_above_neutral_shrinks_k_on_overflow() {
        let mut t = solid(1, 1, [200, 100, 0, 255]);
        apply_saido(&mut t, 192);
        assert_eq!(&t.rgba[0..3], &[200, 100, 0]);
    }

    /// 手册例 8 的 `SET=192`:官方是**加法**偏移 `SET-128 = +64`
    /// (`FUN_00431b40`),不是乘法。
    #[test]
    fn akarusa_192_adds_64() {
        let mut t = solid(1, 1, [100, 50, 20, 255]);
        apply_akarusa(&mut t, 192);
        assert_eq!(&t.rgba[0..3], &[164, 114, 84]);
        assert_eq!(t.rgba[3], 255, "alpha 不变");
        // 上溢夹到 255
        let mut hi = solid(1, 1, [250, 200, 10, 255]);
        apply_akarusa(&mut hi, 192);
        assert_eq!(&hi.rgba[0..3], &[255, 255, 74]);
        // SET=128 = 恒等
        let mut mid = solid(1, 1, [12, 34, 56, 255]);
        apply_akarusa(&mut mid, 128);
        assert_eq!(&mid.rgba[0..3], &[12, 34, 56]);
        // 下溢夹到 0
        let mut lo = solid(1, 1, [10, 20, 30, 255]);
        apply_akarusa(&mut lo, 64);
        assert_eq!(&lo.rgba[0..3], &[0, 0, 0]);
    }

    /// SEPIA(官方 `FUN_004371cc` 逐指令)。
    ///
    /// `[200,100,0]`:`l10 = 117*0 + 601*100 + 306*200 = 121300`,
    /// `base = l10<<10 = 124211200`;
    ///   R' = (base + 0x11E1400) >> 20 = 142955520 >> 20 = 136
    ///   G' = (base - 0x46361)   >> 20 = 123923615 >> 20 = 118
    ///   B' = (base - 0x2C33311) >> 20 =  77880303 >> 20 = 74
    #[test]
    fn sepia_uses_official_bgra_weights() {
        let mut t = solid(1, 1, [200, 100, 0, 255]);
        apply_sepia(&mut t);
        assert_eq!(&t.rgba[0..3], &[136, 118, 74]);
        assert_eq!(t.rgba[3], 255, "alpha 不变");
        // 灰阶:l10 = 1024*px ⇒ 三通道 = px + 各自偏移(R+17.9/G-0.3/B-44.2)。
        let mut g = solid(1, 1, [128, 128, 128, 255]);
        apply_sepia(&mut g);
        assert_eq!(&g.rgba[0..3], &[145, 127, 83]);
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
        apply_blur(&mut t, 8, false);
        assert!(t.rgba.chunks_exact(4).all(|p| p == [120, 60, 30, 255]));
    }

    /// `BLUR2`(action 0x03)= **只有水平遍**:中心脉冲只向左右摊开,
    /// 上下行保持 0(`FUN_0043265c` 的临时缓冲从未被写入)。
    #[test]
    fn blur2_is_horizontal_only() {
        let mut t = Texture {
            width: 3,
            height: 3,
            rgba: vec![0; 36],
        };
        t.rgba[(1 * 3 + 1) * 4] = 90;
        t.rgba[(1 * 3 + 1) * 4 + 3] = 200;
        apply_blur_h(&mut t, 1, false);
        let px = |x: usize, y: usize| &t.rgba[(y * 3 + x) * 4..(y * 3 + x) * 4 + 4];
        assert_eq!(&px(0, 0)[0..3], &[0, 0, 0], "上排不参与水平遍");
        assert_eq!(&px(0, 1)[0..3], &[45, 0, 0], "左: (0+90)/2");
        assert_eq!(&px(1, 1)[0..3], &[30, 0, 0], "中: 90/3");
        assert_eq!(&px(2, 1)[0..3], &[45, 0, 0], "右: (90+0)/2");
        assert_eq!(px(1, 1)[3], 200, "flag=0:alpha 保持源值");
    }

    /// `BLUR3`(action 0x04)= **只有垂直遍**,且 flag=0 时 alpha 从源图拷贝
    /// (`FUN_00432e58` 每段写回都带 `*(dst+3) = *(src+3)`)。
    #[test]
    fn blur3_is_vertical_only_and_keeps_source_alpha() {
        let mut t = Texture {
            width: 3,
            height: 3,
            rgba: vec![0; 36],
        };
        t.rgba[(1 * 3 + 1) * 4 + 1] = 90;
        t.rgba[(1 * 3 + 1) * 4 + 3] = 200;
        apply_blur_v(&mut t, 1, false);
        let px = |x: usize, y: usize| &t.rgba[(y * 3 + x) * 4..(y * 3 + x) * 4 + 4];
        assert_eq!(&px(0, 1)[0..3], &[0, 0, 0], "左列不参与垂直遍");
        assert_eq!(px(1, 0)[1], 45, "上: (0+90)/2");
        assert_eq!(px(1, 1)[1], 30, "中: 90/3");
        assert_eq!(px(1, 2)[1], 45, "下: (90+0)/2");
        assert_eq!(px(1, 1)[3], 200, "flag=0:alpha 从源图拷贝");
    }

    /// `BLUR4`(action 0x05)= 与 BLUR 同核的 H→V 两遍:
    /// 中心脉冲摊成菱形(水平遍后再垂直遍)。
    #[test]
    fn blur4_is_two_pass_box() {
        let mut t = Texture {
            width: 3,
            height: 3,
            rgba: vec![0; 36],
        };
        t.rgba[(1 * 3 + 1) * 4] = 90;
        apply_blur(&mut t, 1, false);
        let r = |x: usize, y: usize| t.rgba[(y * 3 + x) * 4];
        // 水平遍:row1 = [45, 30, 45];垂直遍再平均(整数除法)。
        assert_eq!(r(1, 1), 10, "中心: (0+30+0)/3");
        assert_eq!(r(0, 1), 15, "左: (0+45+0)/3");
        assert_eq!(r(1, 0), 15, "上: (0+30)/2");
        assert_eq!(r(0, 0), 22, "角: (0+45)/2");
    }

    /// `BLUR5`(action 0x06)/ BLUR6 的 flag=1 分支:alpha-only 两遍,
    /// **RGB 全 0**(`FUN_00434748` 每段都写 `(0,0,0,avg)`)。
    #[test]
    fn blur5_is_alpha_only_mask() {
        let mut t = Texture {
            width: 3,
            height: 3,
            rgba: vec![0; 36],
        };
        t.rgba[(1 * 3 + 1) * 4] = 200;
        t.rgba[(1 * 3 + 1) * 4 + 1] = 200;
        t.rgba[(1 * 3 + 1) * 4 + 2] = 200;
        t.rgba[(1 * 3 + 1) * 4 + 3] = 255;
        apply_blur_alpha_mask(&mut t, 1);
        let px = |x: usize, y: usize| &t.rgba[(y * 3 + x) * 4..(y * 3 + x) * 4 + 4];
        assert!(t.rgba.chunks_exact(4).all(|p| p[0] == 0 && p[1] == 0 && p[2] == 0));
        // 水平遍:127 / 85 / 127;垂直遍:63 / 42 / 63 与 42 / 28 / 42。
        assert_eq!(px(1, 1)[3], 28, "中心: 85/3");
        assert_eq!(px(0, 1)[3], 42, "左: 127/3");
        assert_eq!(px(0, 0)[3], 63, "角: 127/2");
    }

    /// flag=1(层 `[+0x86]` 置位)时四通道一起平均。
    #[test]
    fn alpha_aware_blur_averages_alpha_too() {
        let mut t = Texture {
            width: 3,
            height: 1,
            rgba: vec![0, 0, 0, 10, 100, 0, 0, 20, 200, 0, 0, 30],
        };
        apply_blur_h(&mut t, 1, true);
        assert_eq!(t.rgba[0], 50);
        assert_eq!(t.rgba[3], 15, "alpha 也参与水平平均");
        assert_eq!(t.rgba[7], 20, "三像素窗口: (10+20+30)/3");
        assert_eq!(t.rgba[11], 25);
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
        apply_blur(&mut t, 0, false);
        assert_eq!(t.rgba, base.rgba);
        let mut t = base.clone();
        apply_mosaic(&mut t, 1);
        assert_eq!(t.rgba, base.rgba);
        let mut t = base.clone();
        apply_raster(&mut t, 0, 0, RasterAxis::Horizontal);
        assert_eq!(t.rgba, base.rgba);
        let mut t = base.clone();
        apply_whirlpool(&mut t, 0);
        assert_eq!(t.rgba, base.rgba);
        let mut seed = 1u32;
        let mut t = base.clone();
        apply_diffusion(&mut t, 1, &mut seed);
        assert_eq!(t.rgba, base.rgba, "SET/2 = 0 ⇒ 不置换");
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
    fn merge2_opaque_upper_keeps_official_one_lsb_darkening() {
        // 全不透明时 MERGE2 仍比上层**暗 1 LSB**:官方内核 `0x402654` 用
        // `(a'+1)` 做分母(避免 a'=0 除零),而分子用 `U.a` 本身 ——
        // a'=U.a=255 时比值 255/256,即 `L.c + (U.c−L.c)·255/256`。
        // 这是官方实现自带的量化,不是我们的偏差;照抄才能逐字节一致。
        let mut lower = solid(1, 1, [10, 20, 30, 255]);
        let upper = solid(1, 1, [200, 100, 50, 255]);
        apply_merge(&mut lower, &upper, true);
        assert_eq!(&lower.rgba[0..4], &[199, 99, 49, 255]);
        assert_eq!(lower.rgba[3], 255);
        // 对照:MERGE 走查表 `U' = U.a·256/a'`,全不透明时 U'=256 ⇒ 恒等。
        let mut lower = solid(1, 1, [10, 20, 30, 255]);
        apply_merge(&mut lower, &upper, false);
        assert_eq!(&lower.rgba[0..4], &[200, 100, 50, 255]);
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
    /// 独立参考实现:逐像素**照抄官方两个内核的整数式**(不走快路径的
    /// 行跨度/跨度裁剪),用来验证快路径与官方口径逐字节一致。
    fn merge_reference(lower: &Texture, upper: &Texture, merge2: bool) -> Texture {
        merge_reference_layers(lower, upper, merge2, 256, 256)
    }

    /// 同 [`merge_reference`],但带两个层不透明度(`src_la` 上层、`dst_la` 下层)。
    fn merge_reference_layers(
        lower: &Texture,
        upper: &Texture,
        merge2: bool,
        src_la: u32,
        dst_la: u32,
    ) -> Texture {
        let p3 = src_la;
        let p4 = if merge2 { dst_la } else { 256 };
        let mut out = lower.clone();
        let w = out.width.min(upper.width);
        let h = out.height.min(upper.height);
        let tb = merge_tables();
        for y in 0..h {
            for x in 0..w {
                let ui = ((y * upper.width + x) * 4) as usize;
                let li = ((y * out.width + x) * 4) as usize;
                let la_px = u32::from(out.rgba[li + 3]);
                let ua_px = u32::from(upper.rgba[ui + 3]);
                if merge2 {
                    // MERGE2(`0x402654`):下层自身 alpha 乘下层层 alpha,
                    // 上层贡献乘上层层 alpha,RGB 除以 `(a'+1)`。
                    let a1 = (la_px * p4) >> 8;
                    let t = ((ua_px + 1) * a1) >> 8;
                    let w2 = (((ua_px as i32) - (t as i32)) * p3 as i32) >> 8;
                    let a_out = (a1 as i32 + w2).clamp(0, 255);
                    let den = a_out + 1;
                    for c in 0..3 {
                        let delta = (((i32::from(upper.rgba[ui + c]))
                            - (i32::from(out.rgba[li + c])))
                            * p3 as i32)
                            >> 8;
                        let v = i32::from(out.rgba[li + c]) + delta * (ua_px as i32) / den;
                        out.rgba[li + c] = v.clamp(0, 255) as u8;
                    }
                    out.rgba[li + 3] = a_out as u8;
                } else {
                    // MERGE(`0x402828`):查表 T/U,RGB 为 lerp。
                    let t = u32::from(tb.t[((la_px << 8) | ua_px) as usize]);
                    let w2 = (((ua_px as i32) - (t as i32)) * p3 as i32) >> 8;
                    let a_out = (la_px as i32 + w2).clamp(0, 255) as u32;
                    out.rgba[li + 3] = a_out as u8;
                    let u_w = u32::from(tb.u[((a_out << 8) | ua_px) as usize]);
                    let prod = u_w as i32 * p3 as i32;
                    for c in 0..3 {
                        let delta =
                            i32::from(upper.rgba[ui + c]) - i32::from(out.rgba[li + c]);
                        let v = i32::from(out.rgba[li + c]) + (delta.wrapping_mul(prod) >> 16);
                        out.rgba[li + c] = v.clamp(0, 255) as u8;
                    }
                }
            }
        }
        out
    }

    /// `MERGE` 的 alpha 口径(归一化 over ⇒ `a + la·(1−a)`;依据是像素实测,
    /// 不是 RE —— 见 `merge.wgsl` 头注的勘误):
    /// 目标被清空(alpha=0)时,不透明上层必须把 alpha 写回 255 —— 这是
    /// 「立绘换差分眨眼闪屏」的回归锚点。
    #[test]
    fn merge_writes_alpha_so_cleared_target_stays_visible() {
        let mut lower = Texture {
            width: 2,
            height: 1,
            rgba: vec![0, 0, 0, 0, 0, 0, 0, 0],
        };
        let upper = Texture {
            width: 2,
            height: 1,
            rgba: vec![255, 0, 0, 255, 255, 0, 0, 255],
        };
        apply_merge(&mut lower, &upper, false);
        assert_eq!(&lower.rgba[..4], &[255, 0, 0, 255]);
        assert_eq!(&lower.rgba[4..8], &[255, 0, 0, 255]);
    }

    /// **眨眼缝隙的主回归锚点**:下层 alpha=0 但**RGB 非零**(素材 PNG 在
    /// 透明区的残留色)+ 上层抗锯齿边,合并后必须只看到上层颜色。
    ///
    /// kotoneiro 立绘缓冲实测:眼睛区域 26773 个像素 alpha=0 但 RGB 非零
    /// (均值 159,117,122),差分图在该区域有 15426 个 alpha 1..254 的边缘。
    /// 旧式 `cl*(1−a)` 会在 a=128 处把这些不可见的脏色按 50% 混进可见区。
    #[test]
    fn merge_ignores_hidden_lower_rgb_where_lower_is_transparent() {
        // 下层:全透明,但 RGB 是「脏」的 159,117,122。
        let mut lower = Texture {
            width: 1,
            height: 1,
            rgba: vec![159, 117, 122, 0],
        };
        // 上层:抗锯齿边 alpha=128,颜色纯红。
        let upper = Texture {
            width: 1,
            height: 1,
            rgba: vec![255, 0, 0, 128],
        };
        apply_merge(&mut lower, &upper, false);
        assert_eq!(
            &lower.rgba[..3],
            &[255, 0, 0],
            "下层 alpha=0 ⇒ 它的 RGB 一个权重都不该占"
        );
        assert_eq!(lower.rgba[3], 128, "结果 alpha 就是上层 alpha");
    }

    /// 下层不透明时仍是标准 over(与 2b53786 之前的 `out.a = lower.a` 一致)。
    #[test]
    fn merge_over_opaque_lower_is_plain_interpolation() {
        let mut lower = Texture {
            width: 1,
            height: 1,
            rgba: vec![40, 120, 200, 255],
        };
        let upper = Texture {
            width: 1,
            height: 1,
            rgba: vec![220, 30, 20, 128],
        };
        apply_merge(&mut lower, &upper, false);
        assert_eq!(lower.rgba[3], 255, "下层不透明 ⇒ 结果不透明");
        assert_eq!(
            &lower.rgba[..3],
            &[130, 75, 110],
            "(220·128 + 40·127)/255 = 130 等"
        );
    }

    /// 半透明上层 + 半透明下层:归一化 over,`ao = a + la·(1−a)`。
    #[test]
    fn merge_alpha_uses_normalized_over() {
        let mut lower = Texture {
            width: 1,
            height: 1,
            rgba: vec![0, 0, 0, 128],
        };
        let upper = Texture {
            width: 1,
            height: 1,
            rgba: vec![255, 0, 0, 160],
        };
        apply_merge(&mut lower, &upper, false);
        let expected = ((160u32 * 255 + 128 * (255 - 160) + 127) / 255) as u8;
        assert_eq!(lower.rgba[3], expected);
        // a=160/255、la=128/255 ⇒ over = 0.627 + 0.502·0.373 = 0.8147 ⇒ 208。
        assert_eq!(expected, 208);
    }

    /// **层不透明度**(`[层+0x80]`,0..256)的语义门禁。
    ///
    /// REA 0.495 混合内核 `0x402828` 的 alpha 递推(已手解):
    ///
    /// ```text
    /// T[d][s] = d·s/255                        (magic 0x80808081 >> 7)
    /// dst.a'  = s + ((d − T[d][s]) · layerAlpha) >> 8
    ///         = s + d·(1 − s/255) · layerAlpha/256
    /// ```
    ///
    /// ⇒ layerAlpha 乘在**下层露出量 `(1−s)`** 上,不是折进像素 alpha。
    /// 两个推论,本用例逐条钉住:
    ///
    /// 1. **两层都不透明时层 alpha 无效**:`(1−s)=0` ⇒ 缩放乘 0 仍是 0,
    ///    结果恒为纯上层色。这不是 bug,是公式的必然;
    /// 2. **上层半透明时才显形**:`(1−s)≠0`,层 alpha 越小,下层露得越少,
    ///    结果越接近纯上层。
    #[test]
    fn layer_alpha_is_the_source_blend_strength() {
        // 官方 MERGE 内核 `0x402828`:
        //   T  = L.a·U.a/255
        //   a' = L.a + ((U.a − T)·srcA)>>8
        //   U' = U.a·256/a'
        //   co = L.c + ((U.c − L.c)·U'·srcA)>>16
        // ⇒ srcA(=上层的层 alpha)是**源混合强度**:256 = 全混合,0 = 完全不动。
        // 下层的层 alpha **不参与** MERGE(执行体根本不传)。
        let mk = || {
            (
                Texture {
                    width: 1,
                    height: 1,
                    rgba: vec![40, 120, 200, 255],
                },
                Texture {
                    width: 1,
                    height: 1,
                    rgba: vec![220, 30, 20, 128],
                },
            )
        };
        let run = |l: u32| {
            let (mut lo, up) = mk();
            apply_merge_layers(&mut lo, &up, false, l, 256);
            lo.rgba
        };

        let full = run(256);
        let half = run(128);
        let none = run(0);
        // srcA=256:下层不透明 ⇒ a'=255,U'=floor(128·256/255)=128,
        //        co = L + (U−L)·128/256 = 40+90=130。
        assert_eq!(full[..3], [130, 75, 110], "srcA=256 应为标准 over");
        // srcA=0:U'·0 ⇒ co = L.c 原样,alpha 也不变。
        assert_eq!(none, [40, 120, 200, 255], "srcA=0 ⇒ 下层原样");
        // srcA=128:强度减半 ⇒ co = L + (U−L)·128/512。
        assert_eq!(half[..3], [85, 97, 155], "srcA=128 ⇒ 强度减半");
        assert_eq!(half[3], 255, "下层不透明 ⇒ alpha 恒 255");
        // 单调性:强度越大越接近上层。
        assert!(
            full[1] < half[1] && half[1] < none[1],
            "绿色分量应随层 alpha 单调: full={} half={} none={}",
            full[1],
            half[1],
            none[1]
        );

        // alpha 递推独立复算(官方整数式,`>>8` 为 sar 截断):
        //   L.a=255 & U.a=255 ⇒ T=255 ⇒ w=0 ⇒ a'=255,与 srcA 无关。
        let (mut lo, up) = mk();
        apply_merge_layers(&mut lo, &up, false, 128, 256);
        assert_eq!(lo.rgba[3], 255, "下层不透明 ⇒ a' 恒 255");
        // 下层清空后 a' 直接等于上层贡献项:srcA·U.a/256。
        let mut cleared = Texture {
            width: 1,
            height: 1,
            rgba: vec![0, 0, 0, 0],
        };
        apply_merge_layers(&mut cleared, &up, false, 128, 256);
        assert_eq!(cleared.rgba[3], 64, "a' = ((128−0)·128)>>8 = 64");
    }

    /// `layerAlpha == 256` 必须与不带层 alpha 的原路径**逐位一致**(恒等乘数)。
    #[test]
    fn layer_alpha_256_is_bit_identical_to_plain_merge() {
        for (lower_a, upper_a) in [(255u8, 255u8), (255, 128), (128, 200), (0, 137), (37, 219)] {
            for merge2 in [false, true] {
                let lower = Texture {
                    width: 1,
                    height: 1,
                    rgba: vec![41, 122, 203, lower_a],
                };
                let upper = Texture {
                    width: 1,
                    height: 1,
                    rgba: vec![219, 33, 21, upper_a],
                };
                let mut plain = lower.clone();
                apply_merge(&mut plain, &upper, merge2);
                let mut layered = lower.clone();
                apply_merge_layers(&mut layered, &upper, merge2, 256, 256);
                assert_eq!(
                    plain.rgba, layered.rgba,
                    "lower_a={lower_a} upper_a={upper_a} merge2={merge2}"
                );
            }
        }
    }

    /// 行跨度 + 透明跳过必须**逐位**等于旧公式:随机内容(含整行透明/窄源)。
    /// **跨度裁剪必须与层 alpha 组合后仍然逐位一致**。
    ///
    /// 直接回归一个真实缺陷:`alpha_span` 会跳过「上层整行/边缘全透明」的区域,
    /// 这要求 `U.a=0` 时结果恒等。`MERGE` 恒满足;`MERGE2` **只在
    /// `dstA(下层 [层+0x80]) == 256` 时**满足 —— `dstA < 256` 时
    /// `a' = L.a·dstA>>8` 会把 alpha 压暗,跳过就会漏写。
    /// 本用例在带层 alpha 的前提下把「有透明空洞的上层」对拍参考实现。
    #[test]
    fn merge_span_optimization_holds_with_layer_alpha() {
        let mut seed = 0x5eed_1234_9876_abcdu64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for case in 0..48 {
            let (lw, lh) = (6 + (next() % 7) as u32, 5 + (next() % 5) as u32);
            let (uw, uh) = (8 + (next() % 5) as u32, 6 + (next() % 4) as u32);
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
            // 上层:只有中间一块有内容,四周(含整行/整列)**完全透明**,
            // 这正是 `alpha_span` 会裁掉的形态。
            for y in 0..uh {
                for x in 0..uw {
                    let i = ((y * uw + x) * 4) as usize;
                    let inside = y >= uh / 3 && y < uh - uh / 3 && x >= uw / 3 && x < uw - uw / 3;
                    if inside {
                        upper.rgba[i] = (next() % 256) as u8;
                        upper.rgba[i + 1] = (next() % 256) as u8;
                        upper.rgba[i + 2] = (next() % 256) as u8;
                        upper.rgba[i + 3] = (next() % 256) as u8;
                    }
                }
            }
            for (src_la, dst_la) in [(256u32, 256u32), (256, 128), (128, 256), (200, 64), (0, 96)] {
                for merge2 in [false, true] {
                    let expected =
                        merge_reference_layers(&lower, &upper, merge2, src_la, dst_la);
                    let mut actual = lower.clone();
                    apply_merge_layers(&mut actual, &upper, merge2, src_la, dst_la);
                    assert_eq!(
                        actual, expected,
                        "case {case} merge2={merge2} src_la={src_la} dst_la={dst_la}"
                    );
                }
            }
        }
    }

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
    /// 每个像素唯一:`R = x*10`、`G = y*10`、`B = 7`、`A = 200`。
    fn grid(w: u32, h: u32) -> Texture {
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                rgba.extend_from_slice(&[(x as u8) * 10, (y as u8) * 10, 7, 200]);
            }
        }
        Texture { width: w, height: h, rgba }
    }

    fn px_at(t: &Texture, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * t.width + x) * 4) as usize;
        [t.rgba[i], t.rgba[i + 1], t.rgba[i + 2], t.rgba[i + 3]]
    }

    /// `SET=2` ⇒ 1 轮、两遍(正向 + 反向),2×2 图上每遍各命中同一块。
    ///
    /// 官方 `rand()` 前两个值(MSVC LCG,种子 1)= 41 / 18467;
    /// `41%24 = 17 → [0,1,3,2]`,`18467%24 = 11 → [0,2,3,1]`;
    /// 两次复合 ⇒ `[P00, P11, P01, P10]`。
    #[test]
    fn diffusion_applies_two_pass_permutation_with_msvc_rand() {
        let mut seed = 1u32;
        let mut t = grid(2, 2);
        apply_diffusion(&mut t, 2, &mut seed);
        assert_eq!(px_at(&t, 0, 0), [0, 0, 7, 200], "P00 原位");
        assert_eq!(px_at(&t, 1, 0), [10, 10, 7, 200], "P11 落到 (1,0)");
        assert_eq!(px_at(&t, 0, 1), [0, 10, 7, 200], "P01 原位");
        assert_eq!(px_at(&t, 1, 1), [10, 0, 7, 200], "P10 落到 (1,1)");
        assert!(
            t.rgba.chunks_exact(4).all(|p| p[3] == 200),
            "DIFFUSION 只置换 RGB,alpha 不动"
        );
    }

    /// `SET/2` 为 0 时不置换(官方 `if (0 < SET/2)`),`SET=1` 即恒等。
    #[test]
    fn diffusion_below_two_is_identity() {
        let base = grid(3, 3);
        let mut seed = 1u32;
        let mut t = base.clone();
        apply_diffusion(&mut t, 1, &mut seed);
        assert_eq!(t.rgba, base.rgba);
        let mut t = base.clone();
        apply_diffusion(&mut t, 0, &mut seed);
        assert_eq!(t.rgba, base.rgba);
    }

    /// 5×5、`SET=900` ⇒ 中心 90°,按半径线性衰减到 0(官方角度表)。
    ///
    /// 手算(`maxr = trunc((hypot(5,5)+1)*0.5+1) = 5`;表值全部**向零截断**):
    /// - `r=0`:cos=0/sin=256;`r=1`:79/243;`r=2`:150/207;`r=3`:207/150;
    /// - `(3,2)`:`dx=1,dy=0,r=1` ⇒ `sx=(79)>>8+2=2`、`sy=(243)>>8+2=2`;
    /// - `(4,2)`:`dx=2,dy=0,r=2` ⇒ `sx=(300)>>8+2=3`、`sy=(414)>>8+2=3`;
    /// - `(2,3)`:`dx=0,dy=1,r=1` ⇒ `sx=(-243)>>8+2=1`、`sy=(79)>>8+2=2`;
    /// - `(0,0)`:`dx=dy=-2,hypot=2.83` ⇒ `r=trunc=2` ⇒
    ///   `sx=(-300+414)>>8+2=2`、`sy=(-414-300)>>8+2=-1→0`。
    #[test]
    fn whirlpool_rotates_by_radius_table() {
        let mut t = grid(5, 5);
        apply_whirlpool(&mut t, 900);
        let base = grid(5, 5);
        assert_eq!(px_at(&t, 2, 2), [20, 20, 7, 200], "圆心不动");
        assert_eq!(px_at(&t, 3, 2), px_at(&base, 2, 2), "(3,2) ← (2,2)");
        assert_eq!(px_at(&t, 4, 2), px_at(&base, 3, 3), "(4,2) ← (3,3)");
        assert_eq!(px_at(&t, 2, 3), px_at(&base, 1, 2), "(2,3) ← (1,2)");
        assert_eq!(px_at(&t, 0, 0), px_at(&base, 2, 0), "(0,0) ← (2,0)");
    }

    /// `progress=0` ⇒ 角度 0 ⇒ `cos=256/sin=0` ⇒ 恒等。
    #[test]
    fn whirlpool_zero_progress_is_identity() {
        let base = grid(5, 5);
        let mut t = base.clone();
        apply_whirlpool(&mut t, 0);
        assert_eq!(t.rgba, base.rgba);
    }

    /// `RASTER`:`SET=2 SET2=60` ⇒ `sin(60°)=0.866`、`trunc(1.732)=1`
    /// ⇒ 每行整体右移 1;水平下标夹取到 `x1-1`(官方上界就是 `x1-1`,
    /// 末列复用倒数第二列)。
    ///
    /// 注意不能用 `SET2=90`:`0.017453292f * 90` 略小于 π/2,
    /// `sin` 得 `0.99999999…`,官方 `fistp` 向零截断后是 **0**(不是 1)。
    #[test]
    fn raster_horizontal_shifts_row_by_sin() {
        let mut t = grid(5, 1);
        apply_raster(&mut t, 2, 60, RasterAxis::Horizontal);
        let base = grid(5, 1);
        assert_eq!(px_at(&t, 0, 0), px_at(&base, 1, 0));
        assert_eq!(px_at(&t, 1, 0), px_at(&base, 2, 0));
        assert_eq!(px_at(&t, 2, 0), px_at(&base, 3, 0));
        assert_eq!(px_at(&t, 3, 0), px_at(&base, 3, 0), "夹取到 x1-1");
        assert_eq!(px_at(&t, 4, 0), px_at(&base, 3, 0), "末列同样夹到 x1-1");
    }

    /// `RASTER2`:`SET=1 SET2=0` ⇒ `cos(0°)=1` ⇒ 每列整体下移 1,
    /// 垂直下标夹取到 `[y0, y1]`(含端点,末行复用末行)。
    #[test]
    fn raster_vertical_shifts_column_by_cos() {
        let mut t = grid(1, 5);
        apply_raster(&mut t, 1, 0, RasterAxis::Vertical);
        let base = grid(1, 5);
        assert_eq!(px_at(&t, 0, 0), px_at(&base, 0, 1));
        assert_eq!(px_at(&t, 0, 1), px_at(&base, 0, 2));
        assert_eq!(px_at(&t, 0, 2), px_at(&base, 0, 3));
        assert_eq!(px_at(&t, 0, 3), px_at(&base, 0, 4));
        assert_eq!(px_at(&t, 0, 4), px_at(&base, 0, 4), "夹取到 y1");
    }

    /// `RASTER3` 同时叠加两个方向的位移(`SET2=45` ⇒ sin=cos=0.7071;
    /// 幅度 2 时 `trunc(1.414)=1`,两个方向都是 +1。幅度 1 会截断成 0)。 
    #[test]
    fn raster3_combines_both_axes() {
        let mut t = grid(5, 5);
        apply_raster(&mut t, 2, 45, RasterAxis::Both);
        let base = grid(5, 5);
        assert_eq!(px_at(&t, 0, 0), px_at(&base, 1, 1));
        assert_eq!(px_at(&t, 2, 2), px_at(&base, 3, 3));
    }
}
