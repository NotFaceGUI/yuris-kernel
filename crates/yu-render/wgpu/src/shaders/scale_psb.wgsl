// 引擎 effect Pass2(psB,4-tap 锐利双线性,`DAT_00801334 == 2`)。
//
// 字节码 `0x18cf58`(604 B)反汇编要点:
//
//   f  = frc(t0 * YS_TexSize)
//   w  = saturate((f - 0.5) * SharpN + 0.5)
//   r2 = lerp(tex(t0), tex(t1), w.x)        // 上排
//   r1 = lerp(tex(t2), tex(t3), w.x)        // 下排
//   oC0 = saturate(lerp(r2, r1, w.y))
//
// 4 组 UV 由 `FUN_0041a308`(`DAT_00801334 == 2` 分支)生成:
// t0=(u,v)、t1=(u+1/W,v)、t2=(u,v+1/H)、t3=(u+1/W,v+1/H)。
// 采样器在 `FUN_004195ec` 里被硬编码为 D3DTEXF_POINT(1/1/1),
// 所以这里必须用 point 采样器,单 texel 取值再由权重插值。
//
// `SharpN` 引擎从未写入:取 FXO 参数初值 1.25(`0x3FA00000`)。

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// 全屏三角形对;uv 的 v=0 在屏幕上方(与 `push_quad` 一致)。
@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VsOut {
    var positions = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(1.0, 1.0),
    );
    let position = positions[index];
    var out: VsOut;
    out.clip = vec4<f32>(position, 0.0, 1.0);
    out.uv = vec2<f32>((position.x + 1.0) * 0.5, (1.0 - position.y) * 0.5);
    return out;
}

@group(0) @binding(0) var src_texture: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
struct Scale {
    tex_size: vec2<f32>,
    sharp_n: vec2<f32>,
};
@group(0) @binding(2) var<uniform> scale: Scale;

fn texel_at(x: i32, y: i32) -> vec4<f32> {
    let size = vec2<i32>(textureDimensions(src_texture));
    let clamped = clamp(vec2<i32>(x, y), vec2<i32>(0), size - vec2<i32>(1));
    return textureLoad(src_texture, clamped, 0);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // uv 是输出像素中心的归一化坐标;减去半个 texel 得到源 texel-center
    // 坐标 q 的归一化值(FUN_0041a308 的 u/v 输入)。
    let base = in.uv - 0.5 / scale.tex_size;
    let position = base * scale.tex_size;
    // D3D9 point 采样按 floor(u*W) 取 texel;显式 floor 避免 WebGPU
    // nearest 在整 texel 边界上的 tie-break 差异(1:1 必须逐像素一致)。
    let origin = floor(position);
    let f = position - origin;
    let ix = i32(origin.x);
    let iy = i32(origin.y);
    let w = clamp(
        (f - vec2<f32>(0.5)) * scale.sharp_n + vec2<f32>(0.5),
        vec2<f32>(0.0),
        vec2<f32>(1.0),
    );
    let t0 = texel_at(ix, iy);
    let t1 = texel_at(ix + 1, iy);
    let t2 = texel_at(ix, iy + 1);
    let t3 = texel_at(ix + 1, iy + 1);
    let top = mix(t0, t1, w.x);
    let bottom = mix(t2, t3, w.x);
    return clamp(mix(top, bottom, w.y), vec4<f32>(0.0), vec4<f32>(1.0));
}
