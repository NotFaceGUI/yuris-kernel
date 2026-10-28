// `CGACT[MERGE=1]` 的 GPU 整数合成(与 soft `apply_merge(lower, upper, false)`
// **逐位一致**):
//
//   out.rgb = (lower*(255-a) + upper*a + 127) / 255     // a = upper.alpha
//   out.a   = lower.alpha                                // MERGE 不改目标 alpha
//
// 逐项与 CPU 同式:`round(cl*(1-a/255) + cu*(a/255))` 的精确值是 num/255,
// 距半整数 ≥ 1/510,f64 误差不可能跨舍入边界 ⇒ 整数 `(num+127)/255` 等价。
// 上层越界 / alpha=0 ⇒ 原样返回下层(CPU 循环只覆盖 min 尺寸,且 au=0 恒等)。
//
// `MERGE2`(带 `/ao` 除法,精确值可能正好落在 .5)**不在这里**,仍走 CPU。

@group(0) @binding(0) var t_lower: texture_2d<f32>;
@group(0) @binding(1) var t_upper: texture_2d<f32>;

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
    let a = u32(round(upper.a * 255.0));
    if (a == 0u) {
        return lower;
    }
    let inv = 255u - a;
    let u = vec4<u32>(round(upper * 255.0));
    let l = vec4<u32>(round(lower * 255.0));
    let rgb = (vec3<u32>(l.x, l.y, l.z) * inv
        + vec3<u32>(u.x, u.y, u.z) * a
        + vec3<u32>(127u)) / vec3<u32>(255u);
    return vec4<f32>(f32(rgb.x) / 255.0, f32(rgb.y) / 255.0, f32(rgb.z) / 255.0, lower.a);
}
