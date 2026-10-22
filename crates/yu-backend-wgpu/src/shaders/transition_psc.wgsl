// 引擎 effect Pass1(psC,TID/mask 转场,`_DXMODE≠0`)。
//
// 反汇编(oC0):
//   oC0.rgb = tex1.rgb
//   oC0.a   = tex1.a * saturate((YS_FadeVal - 0.5*tex2.r) * YS_FadeVal2)
//             * YS_AlphaVal
//
// 注意 mask 取配对纹理 **R** 通道(固定管线模型取 G);YS_FadeVal2 = TA,
// YS_AlphaVal = A/256。u8 量化链与软后端一致。

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
    fade2: f32,
    alpha: f32,
    pad: f32,
};
@group(0) @binding(4) var<uniform> fade: Fade;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let texel = textureSample(src_texture, src_sampler, in.uv);
    let mask = textureSample(mask_texture, mask_sampler, in.uv);
    let coverage = clamp((fade.fade - 0.5 * mask.r) * fade.fade2, 0.0, 1.0);
    let coverage8 = round(coverage * 255.0);
    let layer8 = round(fade.alpha * 255.0);
    let alpha8 = round(layer8 * coverage8 / 255.0);
    let texel8 = round(texel.a * 255.0);
    let src8 = round(texel8 * alpha8 / 255.0);
    return vec4<f32>(texel.rgb, src8 / 255.0);
}
