// `CGACT[TRANSITION=1]` 的双源整数 lerp —— GPU 版。
//
// 与 soft `blend_transition` **逐位一致**,且对齐官方 SSE2 内核 `0x402d7c`:
//
//   k = SET + 1                       (执行体 `0x43bad0` 的 `iVar6 + 1`;范围 1..=256)
//   out[c] = a + (b - a) * k / 256    (四舍五入,饱和到 0..255,含 alpha)
//
// 官方用 `PSUBW`/`PMULHW`(16.16 定点)/`PADDUSW`(饱和加)算同一条式子。
// **不是** `(a*(256-k) + b*k) >> 8` 的截断式:那会系统性偏低 1,渐变中段
// 成片偏暗,与现场「过渡形态不一致」吻合。
//
// 两边按**同一整数像素坐标**取 texel(不缩放;`from` 越界按 0,与 CPU 的
// `unwrap_or(0)` 同义)。输出画布 = `to` 的尺寸(CPU 同口径)。
//
// UNORM→f32 是 `k/255`(f32 舍入);乘回 255 后 `round` 精确还原 k,因此
// 整数域计算无误差;写回目标时 `o/255` 经 UNORM 的 `round(x*255)` 还原成 o。

struct Lerp {
    // 权重 k = SET + 1,范围 1..=256
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

    let b = vec4<f32>(round(to_c * 255.0));
    let a = vec4<f32>(round(from_c * 255.0));
    let k = f32(u.f);            // 1..=256
    let kf = 256.0 - k;

    // **按 alpha 加权**(预乘语义)的插值。
    //
    // 缓冲里存在「纯白但全透明」的像素 `(255,255,255,0)`:来自 `RECTPAINT2`
    // (只写 alpha、保留 RGB)作用在白色底图,以及纹理池复用。逐通道直接插值
    // 会让这些像素的 RGB 参与混合 ⇒ 淡出过程**越淡越亮**。
    //
    //   out = (a.rgb*a.a*(1-k) + b.rgb*b.a*k) / (a.a*(1-k) + b.a*k)
    //
    // 两源都不透明时退化为 `a + (b-a)*k/256`,与官方 SSE 内核(`0x402d7c`)
    // 逐位同口径;只在半透明/透明区域修正。
    let wf = a.a * kf;
    let wt = b.a * k;
    let den = wf + wt;
    if (den <= 0.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    // **必须与 soft 的整数式逐位一致**:`(num + den/2) / den` 截断除、
    // `(den + 128) >> 8` 取整。`num` 上界 255*65536 ≈ 1.67e7 < 2^24,
    // f32 可精确表示整数,所以 `floor` 之后逐位相同。
    let num = a.rgb * wf + b.rgb * wt;
    let rgb = floor((num + den * 0.5) / den);
    let alpha = floor((den + 128.0) / 256.0);
    return clamp(vec4<f32>(rgb, alpha), vec4<f32>(0.0), vec4<f32>(255.0)) / 255.0;
}
