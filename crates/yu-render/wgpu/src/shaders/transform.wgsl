// `CGACT` 逐层原地滤镜族的 GPU 实现(读一张源纹理,写同尺寸新表面)。
//
// 逐项与 soft `imagefilters.rs` 同式(该文件是像素参考实现):
//   0 REVERSE   : rgb = 255-rgb,alpha 不变
//   1 HORIZONTAL: 左右镜像(按像素坐标精确取样)
//   2 VERTICAL  : 上下镜像
//   3 MEIDO     : rgb[c] = min(255, rgb[c]*gain[c]/0x80),SET = 0xRRGGBB
//   4 SAIDO     : l = luma(rgb);rgb[c] = round(l + (rgb[c]-l)*k),k=clamp(SET,0,256)/128
//   5 AKARUSA   : rgb[c] = min(255, rgb[c]*clamp(SET,0,256)/0x80)
//   6 MONO      : luma >= SET ? 255 : 0
//   7 SEPIA     : 经典矩阵(整数,>>8,min 255)
//   8 MOSAIC    : 块内取左上角像素(SET = 块边长)
//   9 ROTATION  : 绕中心旋转 SET 度,正值顺时针,越界透明(画布尺寸不变)
//  10 BLUR-X    : 水平盒式平均(半径 = max(1,SET)),窗口在左右边界**收缩**
//  11 BLUR-Y    : 垂直盒式平均(同上,窗口在上下边界收缩)
//
// 10/11 逐条对齐官方 0.495 `FUN_00431d28`(action 0x02 = BLUR)与
// `FUN_00434b7c`(action 0x07 = BLUR6):R/G/B 各自整数除法求平均、
// 除数 = 实际窗口像素数(不是夹取重复边缘)、**alpha 从源图原样拷贝**。
// 两遍由调用方串联(先 X 再 Y),与官方"水平 → 临时缓冲 → 垂直"同序。
// luma = (r*77 + g*150 + b*29) >> 8(Rec.601 定点,与 CPU 一致)。

struct Params {
    mode: u32,
    amount: i32,
    set2: i32,
    _pad: u32,
    // ROTATION 的 (sin, cos, cx, cy);其余模式不用。
    m: vec4<f32>,
    // 需要额外整数参数的模式:旋涡的 maxr、扩散的 (方向, it, 基数, 每行块数)。
    n: vec4<u32>,
};

@group(0) @binding(0) var t_src: texture_2d<f32>;
@group(0) @binding(1) var<uniform> u: Params;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    return vec4<f32>(pos[index], 0.0, 1.0);
}

fn dims() -> vec2<i32> {
    return vec2<i32>(textureDimensions(t_src));
}

fn load(p: vec2<i32>) -> vec4<u32> {
    let d = dims();
    if (p.x < 0 || p.y < 0 || p.x >= d.x || p.y >= d.y) {
        return vec4<u32>(0u, 0u, 0u, 0u);
    }
    return vec4<u32>(round(textureLoad(t_src, p, 0) * 255.0));
}

fn luma(rgb: vec3<u32>) -> u32 {
    return (rgb.x * 77u + rgb.y * 150u + rgb.z * 29u) >> 8u;
}

// 0.495 `0x5b4780` 的 24×4 置换表(= 原始字节 / 4 的像素序号)。
const PERM: array<u32, 96> = array<u32, 96>(
    3u, 2u, 1u, 0u, 2u, 3u, 1u, 0u, 3u, 1u, 2u, 0u, 1u, 3u, 2u, 0u,
    2u, 1u, 3u, 0u, 1u, 2u, 3u, 0u, 3u, 2u, 0u, 1u, 2u, 3u, 0u, 1u,
    3u, 0u, 2u, 1u, 0u, 3u, 2u, 1u, 2u, 0u, 3u, 1u, 0u, 2u, 3u, 1u,
    3u, 1u, 0u, 2u, 1u, 3u, 0u, 2u, 3u, 0u, 1u, 2u, 0u, 3u, 1u, 2u,
    1u, 0u, 3u, 2u, 0u, 1u, 3u, 2u, 2u, 1u, 0u, 3u, 1u, 2u, 0u, 3u,
    2u, 0u, 1u, 3u, 0u, 2u, 1u, 3u, 1u, 0u, 2u, 3u, 0u, 1u, 2u, 3u,
);

// 官方 `_rand()`(MSVC LCG,种子 1)跳 `steps` 步;`s -> a*s+c` 的二进制幂复合。
fn lcg_jump(steps: u32) -> u32 {
    var a: u32 = 0x343fdu;
    var c: u32 = 0x269ec3u;
    var ra: u32 = 1u;
    var rc: u32 = 0u;
    var left: u32 = steps;
    loop {
        if ((left & 1u) == 1u) {
            ra = ra * a;
            rc = rc * a + c;
        }
        let na = a * a;
        let nc = c * (a + 1u);
        a = na;
        c = nc;
        left = left >> 1u;
        if (left == 0u) { break; }
    }
    return ra + rc;
}

// 官方 SEPIA 的「向零截断除以 2^20」(`sar 0x13` + `shr 0xc` 的符号修正)。
fn trunc20(x: i32) -> i32 {
    let corr = select(0, 0xfffff, x < 0);
    return (x + corr) >> 20;
}

fn pack(px: vec4<u32>) -> vec4<f32> {
    return vec4<f32>(f32(px.x) / 255.0, f32(px.y) / 255.0, f32(px.z) / 255.0, f32(px.w) / 255.0);
}

@fragment
fn fs_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let d = dims();
    let p = vec2<i32>(floor(frag.xy));
    if (p.x < 0 || p.y < 0 || p.x >= d.x || p.y >= d.y) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }

    if (u.mode == 1u) {
        return pack(load(vec2<i32>(d.x - 1 - p.x, p.y)));
    }
    if (u.mode == 2u) {
        return pack(load(vec2<i32>(p.x, d.y - 1 - p.y)));
    }
    if (u.mode == 9u) {
        let fx = f32(p.x) - u.m.z;
        let fy = f32(p.y) - u.m.w;
        let sx = u.m.y * fx + u.m.x * fy + u.m.z;
        let sy = -(u.m.x) * fx + u.m.y * fy + u.m.w;
        // `round` 取整与 CPU `sx.round()` 同义(正负都远离零)。
        return pack(load(vec2<i32>(i32(round(sx)), i32(round(sy)))));
    }

    var px = load(p);
    let rgb = vec3<u32>(px.x, px.y, px.z);
    if (u.mode == 0u) {
        return pack(vec4<u32>(255u - rgb.x, 255u - rgb.y, 255u - rgb.z, px.w));
    }
    if (u.mode == 3u) {
        let packed = u32(u.amount);
        let gain = vec3<u32>((packed >> 16u) & 0xFFu, (packed >> 8u) & 0xFFu, packed & 0xFFu);
        let out = min(rgb * gain / vec3<u32>(0x80u), vec3<u32>(255u));
        return pack(vec4<u32>(out, px.w));
    }
    // SAIDO(4):官方 `FUN_00436bdc` —— 灰度 = (r+g+b)/3(整数截断,
    // 不是 Rec.601);SET<128 用 k=128-SET,SET>=128 先按原始 k 试算再
    // 按溢出/下溢收缩(上溢 R 比 k0、G/B 比已收缩的 k;下溢都比 k)。
    if (u.mode == 4u) {
        let sa = u.amount;
        let p0 = vec3<i32>(rgb);
        let l = (p0.x + p0.y + p0.z) / 3;
        let dp = p0 - vec3<i32>(l);
        let dl = vec3<i32>(l) - p0;
        var out = p0;
        if (sa < 0x80) {
            let k = 0x80 - sa;
            out = p0 + ((dl * vec3<i32>(k)) >> vec3<u32>(7u));
        } else {
            let k0 = sa - 0x80;
            let trial = p0 + ((dp * vec3<i32>(k0)) >> vec3<u32>(7u));
            var k = k0;
            for (var c = 0; c < 3; c = c + 1) {
                if (trial[c] > 0xff && dp[c] != 0) {
                    let limit = (0xff - p0[c]) * 0x80 / dp[c];
                    let base = select(k, k0, c == 0);
                    if (limit < base) { k = limit; }
                }
            }
            for (var c = 0; c < 3; c = c + 1) {
                if (trial[c] < 0 && dp[c] != 0) {
                    let limit = p0[c] * -0x80 / dp[c];
                    if (limit < k) { k = limit; }
                }
            }
            out = p0 + ((dp * vec3<i32>(k)) >> vec3<u32>(7u));
        }
        return pack(vec4<u32>(vec3<u32>(out), px.w));
    }
    // AKARUSA(5):官方 `FUN_00431b40` 是**加法**亮度偏移 px+(SET-128),
    // 夹到 [0,255](SET>=0x80 查的 DAT_00689e60 逐项 = clamp(i-128))。
    if (u.mode == 5u) {
        let off = vec3<i32>(u.amount - 0x80);
        let out = clamp(vec3<i32>(rgb) + off, vec3<i32>(0), vec3<i32>(255));
        return pack(vec4<u32>(vec3<u32>(out), px.w));
    }
    if (u.mode == 6u) {
        let v = select(0u, 255u, luma(rgb) >= u32(clamp(u.amount, 0, 255)));
        return pack(vec4<u32>(v, v, v, px.w));
    }
    // SEPIA(7):官方 `FUN_004371cc` —— l10 = 117B+601G+306R、base=l10<<10、
    // 三通道 trunc((base+C)/2^20) 后夹 [0,255]。
    if (u.mode == 7u) {
        let r = i32(rgb.x);
        let g = i32(rgb.y);
        let b = i32(rgb.z);
        let base = (117 * b + 601 * g + 306 * r) << 10;
        let out = vec3<i32>(
            trunc20(base + 0x11e1400),
            trunc20(base - 0x46361),
            trunc20(base - 0x2c33311),
        );
        return pack(vec4<u32>(vec3<u32>(clamp(out, vec3<i32>(0), vec3<i32>(255))), px.w));
    }
    if (u.mode == 8u) {
        let n = max(1u, u32(u.amount));
        let q = vec2<i32>((p.x / i32(n)) * i32(n), (p.y / i32(n)) * i32(n));
        return pack(load(q));
    }
    // BLUR-X / BLUR-Y:窗口在边界收缩,除数 = 实际像素数;alpha 保留。
    if (u.mode == 10u || u.mode == 11u) {
        let r = max(1, u.amount);
        var acc = vec3<u32>(0u, 0u, 0u);
        var cnt = 0u;
        if (u.mode == 10u) {
            let lo = max(0, p.x - r);
            let hi = min(d.x - 1, p.x + r);
            var k = lo;
            loop {
                if (k > hi) { break; }
                let q = load(vec2<i32>(k, p.y));
                acc = acc + vec3<u32>(q.x, q.y, q.z);
                cnt = cnt + 1u;
                k = k + 1;
            }
        } else {
            let lo = max(0, p.y - r);
            let hi = min(d.y - 1, p.y + r);
            var k = lo;
            loop {
                if (k > hi) { break; }
                let q = load(vec2<i32>(p.x, k));
                acc = acc + vec3<u32>(q.x, q.y, q.z);
                cnt = cnt + 1u;
                k = k + 1;
            }
        }
        let n = max(1u, cnt);
        return pack(vec4<u32>(acc / vec3<u32>(n), px.w));
    }
    // RASTER 族(16/17/18):`SET`=幅度、`SET2`=相位(度);位移向零截断;
    // 水平下标夹到 x1-1(官方原样)。`FUN_0042f809` 的 0x15/0x16 内联支
    // 与 `FUN_00431658`(variant 1/2/3)。
    if (u.mode >= 16u && u.mode <= 18u) {
        let amp = f32(u.amount);
        let phase = f32(u.set2);
        let horiz = (u.mode == 16u || u.mode == 18u);
        let vert = (u.mode == 17u || u.mode == 18u);
        let row_angle = (f32(p.y) + phase) * 0.017453292;
        let col_angle = (f32(p.x) + phase) * 0.017453292;
        var shift_x = 0;
        var shift_y = 0;
        if (horiz) { shift_x = i32(sin(row_angle) * amp); }
        if (vert) { shift_y = i32(cos(col_angle) * amp); }
        let sx = clamp(p.x + shift_x, 0, max(0, d.x - 2));
        let sy = clamp(p.y + shift_y, 0, d.y - 1);
        return pack(load(vec2<i32>(sx, sy)));
    }
    // WHIRLPOOL(19):`FUN_00437368`;定点 cos/sin 表 + 算术右移 8 位。
    if (u.mode == 19u) {
        let cx = d.x / 2;
        let cy = d.y / 2;
        let maxr = max(1, i32(u.n.x));
        let dx = p.x - cx;
        let dy = p.y - cy;
        let r = clamp(i32(sqrt(f32(dx * dx + dy * dy))), 0, maxr - 1);
        let deg = f32(u.amount) / 10.0 * (1.0 - f32(r) / f32(maxr));
        let rad = deg * 0.017453292519444445;
        let cs = i32(cos(rad) * 256.0);
        let sn = i32(sin(rad) * 256.0);
        let rx = (((-dy) * sn + dx * cs) >> 8) + cx;
        let ry = ((dx * sn + cs * dy) >> 8) + cy;
        let sx = clamp(rx, 0, d.x - 1);
        let sy = clamp(ry, 0, d.y - 1);
        return pack(load(vec2<i32>(sx, sy)));
    }
    // DIFFUSION(20):`FUN_004358d4` 的单遍;`n = (方向, it, 基数, 每行块数)`。
    if (u.mode == 20u) {
        let x1 = d.x - 1;
        let y1 = d.y - 1;
        let it = i32(u.n.y);
        let reverse = (u.n.x == 1u);
        var bx = 0;
        var by = 0;
        var covered = false;
        var row = 0;
        var col = 0;
        if (!reverse) {
            if (p.x >= it) {
                bx = it + ((p.x - it) / 2) * 2;
                by = (p.y / 2) * 2;
                covered = (bx < x1) && (by < y1);
                row = by / 2;
                col = (bx - it) / 2;
            }
        } else {
            let x_start = x1 - 1 - it;
            let y_start = y1 - 1;
            if (p.x <= x_start + 1 && p.y <= y_start + 1) {
                bx = p.x - ((p.x - x_start) & 1);
                by = p.y - ((p.y - y_start) & 1);
                covered = (bx >= 0) && (by >= 0);
                row = (y_start - by) / 2;
                col = (x_start - bx) / 2;
            }
        }
        if (!covered) { return pack(px); }
        let k = u.n.z + u32(row) * u.n.w + u32(col);
        let state = lcg_jump(k + 1u);
        let idx = (((state >> 16u) & 0x7fffu) % 24u) * 4u;
        let role = u32((p.y - by) * 2 + (p.x - bx));
        let src_role = PERM[idx + role];
        let sx = bx + i32(src_role & 1u);
        let sy = by + i32(src_role >> 1u);
        let q = load(vec2<i32>(sx, sy));
        return pack(vec4<u32>(q.x, q.y, q.z, px.w));
    }
    return pack(px);
}