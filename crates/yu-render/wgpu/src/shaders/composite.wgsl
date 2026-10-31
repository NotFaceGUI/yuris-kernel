// `CGACT[MERGE2/XOR/AND]` 的 GPU 合成(整面,读 lower+upper 写新表面)。
//
// 三者都只在两图**重叠区**改像素,其余像素按 lower 原样返回(CPU 参考实现
// `apply_bitwise`/`apply_merge` 用 `min(width)`/`min(height)` 作循环上界)。
//
// - `MODE=1` XOR / `MODE=2` AND:RGB 位运算,alpha 不变。
// - `MODE=0` MERGE2 —— 下面详述。
//
// ## MERGE2 官方口径(REA 0.495,逐字节手解,2026-10-31 定案)
//
// `CGACT` 处理体 `0x42c4a4` → MERGE2 标志 `0x684183` → `CALL 0x43b8ec`
// (`FUN_0043b8ec`),逐行 CPU 循环,混合内核经 `DAT_005d1624` 指到
// `LAB_00402654`(全不透明快路径 `DAT_005d1620` = `LAB_00402758` 纯拷贝)。
// 内核也是 4 像素展开,单像素算术:
//
// ```text
//   a1 = L.a·srcA >> 8                   ; dstA = **下层**的 [层+0x80]
//   t  = (U.a+1)·a1 >> 8               ; `+1` 在乘法左操作数上
//   w  = (U.a − t)·srcA >> 8             ; srcA = **上层**的 [层+0x80]
//   a' = a1 + w
//   co = L.c + ((U.c − L.c)·srcA >> 8)·U.a / (a'+1)      ; idiv,向零截断
// ```
//
// 与 MERGE(`0x402828`)的三处**真实差异**(不是同一内核的变体):
//
// 1. 下层的层 alpha 是乘在**下层自己的像素 alpha** 上(`a1`),所以手册说的
//    「MERGE2 双方不透明度都反映」;MERGE 完全不看下层的层 alpha。
// 2. RGB 分母是 `(a'+1)`(`+1` 防除零),MERGE 是查表 `U' = U.a·256/a'`。
// 3. 除法是 `idiv`(向零截断),MERGE 是查表后的 `>>16` 算术右移。
//
// ⇒ **两族在层 alpha≡256 时也不是逐位相同**:全不透明上层、不透明下层时
// MERGE 给 `U.c` 原值,MERGE2 给 `L.c + (U.c−L.c)·255/256`,差 1 LSB。
// 这是官方实现自带的量化,`DIV-GFX-063` 的对拍用例按此放宽为 ≤1。
//
// 「下层不可见处不泄色」照样成立:`L.a=0 ⇒ a1=0 ⇒ t=0 ⇒ w=0 ⇒ a'=0 ⇒
// den=1 ⇒ co = L.c`;叠 `dstA` 也不会把残色放大。

struct Params {
    mode: u32,
    // 上层的 `[层+0x80]`(0..256)。MERGE2 里作为 srcA = 源混合强度。
    src_layer_alpha: u32,
    // 下层的 `[层+0x80]`(0..256)。MERGE2 里作为 dstA,乘进下层像素 alpha。
    dst_layer_alpha: u32,
    _pad0: u32,
};

@group(0) @binding(0) var t_lower: texture_2d<f32>;
@group(0) @binding(1) var t_upper: texture_2d<f32>;
@group(0) @binding(2) var<uniform> u: Params;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    return vec4<f32>(pos[index], 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let lower_dim = vec2<i32>(textureDimensions(t_lower));
    let upper_dim = vec2<i32>(textureDimensions(t_upper));
    let p = vec2<i32>(floor(frag.xy));

    var lower = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    if (p.x >= 0 && p.y >= 0 && p.x < lower_dim.x && p.y < lower_dim.y) {
        lower = textureLoad(t_lower, p, 0);
    }
    if (p.x < 0 || p.y < 0 || p.x >= upper_dim.x || p.y >= upper_dim.y) {
        return lower;
    }
    let upper = textureLoad(t_upper, p, 0);
    let l_raw = vec4<u32>(round(lower * 255.0));
    let up_raw = vec4<u32>(round(upper * 255.0));

    if (u.mode == 1u) {
        return vec4<f32>(
            f32(l_raw.x ^ up_raw.x) / 255.0,
            f32(l_raw.y ^ up_raw.y) / 255.0,
            f32(l_raw.z ^ up_raw.z) / 255.0,
            lower.a,
        );
    }
    if (u.mode == 2u) {
        return vec4<f32>(
            f32(l_raw.x & up_raw.x) / 255.0,
            f32(l_raw.y & up_raw.y) / 255.0,
            f32(l_raw.z & up_raw.z) / 255.0,
            lower.a,
        );
    }

    // ---- MERGE2 ----
    let la = l_raw.w;
    let ua = up_raw.w;
    let p3 = i32(u.dst_layer_alpha);
    let p4 = i32(u.src_layer_alpha);

    let a1 = (i32(la) * p3) >> 8;
    let t = ((i32(ua) + 1) * a1) >> 8;
    let w = ((i32(ua) - t) * p4) >> 8;
    let a_out = u32(clamp(a1 + w, 0, 255));
    // 分母 `(a'+1)`:官方 idiv,向零截断。
    let den = i32(a_out) + 1;

    var rgb = vec3<u32>(0u, 0u, 0u);
    for (var c = 0u; c < 3u; c += 1u) {
        // `(U.c − L.c)·srcA >> 8` 先收缩(算术右移),再乘 `U.a` 后整除 den。
        let delta = ((i32(up_raw[c]) - i32(l_raw[c])) * p4) >> 8;
        let v = i32(l_raw[c]) + delta * i32(ua) / den;
        rgb[c] = u32(clamp(v, 0, 255));
    }
    return vec4<f32>(
        f32(rgb.x) / 255.0,
        f32(rgb.y) / 255.0,
        f32(rgb.z) / 255.0,
        f32(a_out) / 255.0,
    );
}
