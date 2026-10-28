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
// luma = (r*77 + g*150 + b*29) >> 8(Rec.601 定点,与 CPU 一致)。

struct Params {
    mode: u32,
    amount: i32,
    set2: i32,
    _pad: u32,
    // ROTATION 的 (sin, cos, cx, cy);其余模式不用。
    m: vec4<f32>,
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
    if (u.mode == 4u) {
        let k = f32(clamp(u.amount, 0, 256)) / 128.0;
        let l = f32(luma(rgb));
        let out = vec3<u32>(
            u32(round(clamp(l + (f32(rgb.x) - l) * k, 0.0, 255.0))),
            u32(round(clamp(l + (f32(rgb.y) - l) * k, 0.0, 255.0))),
            u32(round(clamp(l + (f32(rgb.z) - l) * k, 0.0, 255.0))),
        );
        return pack(vec4<u32>(out, px.w));
    }
    if (u.mode == 5u) {
        let k = u32(clamp(u.amount, 0, 256));
        let out = min(rgb * k / vec3<u32>(0x80u), vec3<u32>(255u));
        return pack(vec4<u32>(out, px.w));
    }
    if (u.mode == 6u) {
        let v = select(0u, 255u, luma(rgb) >= u32(clamp(u.amount, 0, 255)));
        return pack(vec4<u32>(v, v, v, px.w));
    }
    if (u.mode == 7u) {
        let r = rgb.x;
        let g = rgb.y;
        let b = rgb.z;
        let out = vec3<u32>(
            min((r * 393u + g * 769u + b * 189u) >> 8u, 255u),
            min((r * 349u + g * 686u + b * 168u) >> 8u, 255u),
            min((r * 272u + g * 534u + b * 131u) >> 8u, 255u),
        );
        return pack(vec4<u32>(out, px.w));
    }
    if (u.mode == 8u) {
        let n = max(1u, u32(u.amount));
        let q = vec2<i32>((p.x / i32(n)) * i32(n), (p.y / i32(n)) * i32(n));
        return pack(load(q));
    }
    return pack(px);
}
