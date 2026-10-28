// `CGACT[TRANSITION=1]` 的双源整数 lerp —— GPU 版。
//
// 与 soft `blend_transition` **逐位一致**:
//   out[c] = (from[c] * (256 - f) + to[c] * f) / 256     (整数截断,含 alpha)
// 两边按**同一整数像素坐标**取 texel(不缩放;`from` 越界按 0,与 CPU 的
// `unwrap_or(0)` 同义)。输出画布 = `to` 的尺寸(CPU 同口径)。
//
// UNORM→f32 是 `k/255`(f32 舍入);乘回 255 后 `round` 精确还原 k,因此
// 整数域计算无误差;写回目标时 `o/255` 经 UNORM 的 `round(x*255)` 还原成 o。

struct Lerp {
    // 0..=256
    f: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
};

@group(0) @binding(0) var t_from: texture_2d<f32>;
@group(0) @binding(1) var t_to: texture_2d<f32>;
@group(0) @binding(2) var<uniform> u: Lerp;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    // 全屏三角形:覆盖 [-1,1]² 的裁剪空间。
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    return vec4<f32>(pos[index], 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let to_dim = vec2<i32>(textureDimensions(t_to));
    let from_dim = vec2<i32>(textureDimensions(t_from));
    let p = vec2<i32>(floor(frag.xy));

    var to_c = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    if (p.x >= 0 && p.y >= 0 && p.x < to_dim.x && p.y < to_dim.y) {
        to_c = textureLoad(t_to, p, 0);
    }
    var from_c = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    if (p.x >= 0 && p.y >= 0 && p.x < from_dim.x && p.y < from_dim.y) {
        from_c = textureLoad(t_from, p, 0);
    }

    let b = vec4<u32>(round(to_c * 255.0));
    let a = vec4<u32>(round(from_c * 255.0));
    let inv = 256u - u.f;
    let o = (a * inv + b * u.f) >> vec4<u32>(8u);
    return vec4<f32>(o) / 255.0;
}
