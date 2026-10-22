// 引擎 effect Pass0(psD,交叉淡化,`_DXMODE≠0`)。
//
// preshader: c2.x = 1 - YS_FadeVal
// oC0 = tex1*(1-FadeVal) + tex2*FadeVal;oC0.a *= YS_AlphaVal
//
// 只在有配对层且 `TA<1` 的 HLSL 分支使用;固定管线门控(TA<1 = 单纹理)
// 不产生本 op。u8 量化与软后端一致。

struct VsIn {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) alpha: f32,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) alpha: f32,
};

@vertex
fn vs_main(in: VsIn) -> VsOut {
    return VsOut(vec4<f32>(in.position, 0.0, 1.0), in.uv, in.alpha);
}

@group(0) @binding(0) var src_texture: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(0) @binding(2) var mask_texture: texture_2d<f32>;
@group(0) @binding(3) var mask_sampler: sampler;
struct Fade {
    fade: f32,
    alpha: f32,
    pad: vec2<f32>,
};
@group(0) @binding(4) var<uniform> fade: Fade;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let a = textureSample(src_texture, src_sampler, in.uv);
    let b = textureSample(mask_texture, mask_sampler, in.uv);
    let f = clamp(fade.fade, 0.0, 1.0);
    let mixed = a * (1.0 - f) + b * f;
    let mixed8 = round(mixed * 255.0);
    let layer8 = round(fade.alpha * 255.0);
    let texel8 = mixed8.a;
    let src8 = round(texel8 * layer8 / 255.0);
    return vec4<f32>(mixed8.rgb / 255.0, src8 / 255.0);
}
