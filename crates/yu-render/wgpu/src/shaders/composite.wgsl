// `CGACT[MERGE2/XOR/AND]` 的 GPU 合成(整面,读 lower+upper 写新表面)。
//
// 三者都只在两图**重叠区**改像素,其余像素按 lower 原样返回(CPU 参考实现
// `apply_merge`/`apply_bitwise` 用 `min(width)`/`min(height)` 作循环上界)。
//
// - `MODE=0` MERGE2(手册例 24:双方 alpha 都参与):
//     au = U/255, al = L/255
//     ao = au + al*(1-au)
//     co = (cu*au + cl*al*(1-au)) / ao
//   CPU 用 f64 计算后 round;这里用**精确整数**:
//     设 U=upper.a、L=lower.a(0..255):
//     D   = U*255 + L*(255-U)        (= ao * 255²)
//     num = cu*U*255 + cl*L*(255-U)
//     co  = round(num / D) = (2*num + D) / (2*D)     [正数 round-half-up]
//     out.a = round(D / 255) = (D + 127) / 255
//   精确有理数落在 .5 时 CPU 的 f64 除法可能略小于 .5(向下),整数式向上;
//   这是**已知分歧点**,回归以 soft 逐像素对拍为准(`docs/divergences`)。
// - `MODE=1` XOR / `MODE=2` AND:RGB 位运算,alpha 不变。

struct Params {
    mode: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
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
    let l = vec4<u32>(round(lower * 255.0));
    let up = vec4<u32>(round(upper * 255.0));

    if (u.mode == 1u) {
        return vec4<f32>(
            f32(l.x ^ up.x) / 255.0,
            f32(l.y ^ up.y) / 255.0,
            f32(l.z ^ up.z) / 255.0,
            lower.a,
        );
    }
    if (u.mode == 2u) {
        return vec4<f32>(
            f32(l.x & up.x) / 255.0,
            f32(l.y & up.y) / 255.0,
            f32(l.z & up.z) / 255.0,
            lower.a,
        );
    }

    // MERGE2
    let au = up.w;
    let al = l.w;
    if (au == 0u) {
        // 上层全透明:CPU 的 ao = al、rgb 化简回 cl、alpha 写回 al ⇒ 恒等。
        return lower;
    }
    let inv = 255u - au;
    let d = au * 255u + al * inv;
    if (d == 0u) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    var rgb = vec3<u32>(0u, 0u, 0u);
    for (var c = 0u; c < 3u; c += 1u) {
        let num = l[c] * al * inv + up[c] * au * 255u;
        rgb[c] = (num * 2u + d) / (d * 2u);
    }
    let a = (d + 127u) / 255u;
    return vec4<f32>(f32(rgb.x) / 255.0, f32(rgb.y) / 255.0, f32(rgb.z) / 255.0, f32(a) / 255.0);
}
