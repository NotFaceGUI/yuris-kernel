// 区域重采样:把 `t_src` 的闭区间像素矩形铺到 `dst` 尺寸的新表面。
//
// 与 `yu_render_core::scale_region` **同式**(CPU 参考实现,也是 soft 的
// `SIZE` 带区域路径):
//   nearest : sx = x0 + dx*cw/dw        (整数除法,向下取整)
//   bilinear: u = (dx+0.5)*cw/dw - 0.5 + x0;fx=floor(u);tx=u-fx;四邻域
//             out = round(lerp(lerp(p00,p10,tx), lerp(p01,p11,tx), ty))
// 越界 texel = 透明黑(CPU `texel()` 同义;不能用采样器的 clamp)。
//
// 这条路径只服务「需要把采样修饰**物化**成真实像素」的场合:COPY 的源带
// 区域缩放、TRIM/ROTATE 之后接像素动作、文本合成需要 CPU 基面的回退。

struct Params {
    // 源区域:闭区间 (x0, y0) + (cw, ch) 个像素。
    x0: i32,
    y0: i32,
    cw: i32,
    ch: i32,
    dst_w: u32,
    dst_h: u32,
    bilinear: u32,
    _pad: u32,
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

fn texel(p: vec2<i32>) -> vec4<f32> {
    let d = vec2<i32>(textureDimensions(t_src));
    if (p.x < 0 || p.y < 0 || p.x >= d.x || p.y >= d.y) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    return vec4<f32>(round(textureLoad(t_src, p, 0) * 255.0));
}

@fragment
fn fs_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let p = vec2<i32>(floor(frag.xy));
    let dw = max(1, i32(u.dst_w));
    let dh = max(1, i32(u.dst_h));
    if (u.bilinear == 0u) {
        let sx = u.x0 + p.x * u.cw / dw;
        let sy = u.y0 + p.y * u.ch / dh;
        return texel(vec2<i32>(sx, sy)) / 255.0;
    }
    let fcw = f32(u.cw);
    let fch = f32(u.ch);
    let uu = (f32(p.x) + 0.5) * fcw / f32(dw) - 0.5 + f32(u.x0);
    let vv = (f32(p.y) + 0.5) * fch / f32(dh) - 0.5 + f32(u.y0);
    let fx = floor(uu);
    let fy = floor(vv);
    let tx = uu - fx;
    let ty = vv - fy;
    let ix = i32(fx);
    let iy = i32(fy);
    let p00 = texel(vec2<i32>(ix, iy));
    let p10 = texel(vec2<i32>(ix + 1, iy));
    let p01 = texel(vec2<i32>(ix, iy + 1));
    let p11 = texel(vec2<i32>(ix + 1, iy + 1));
    var out = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    for (var c = 0; c < 4; c += 1) {
        let top = p00[c] * (1.0 - tx) + p10[c] * tx;
        let bot = p01[c] * (1.0 - tx) + p11[c] * tx;
        out[c] = round(clamp(top * (1.0 - ty) + bot * ty, 0.0, 255.0));
    }
    return out / 255.0;
}
