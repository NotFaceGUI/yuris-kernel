// 固定管线双纹理过渡(`_DXMODE=0`)。
//
// 配对纹理 G 通道为 mask(EVID-T-05):
//   coverage = clamp((30 + 70*fade) * (mask.g - (1 - 2*fade)), 0, 1)
// complete=1(T>=256)直接为满。与 `yu_gfx::transition_coverage` 同式,
// 并按软后端的 u8 量化链输出 src alpha。

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
    complete: f32,
    pad: vec2<f32>,
};
@group(0) @binding(4) var<uniform> fade: Fade;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let texel = textureSample(src_texture, src_sampler, in.uv);
    let mask = textureSample(mask_texture, mask_sampler, in.uv);
    var coverage = clamp(
        (30.0 + 70.0 * fade.fade) * (mask.g - (1.0 - 2.0 * fade.fade)),
        0.0,
        1.0,
    );
    if (fade.complete > 0.5) {
        coverage = 1.0;
    }
    let coverage8 = round(coverage * 255.0);
    let layer8 = round(in.alpha * 255.0);
    let alpha8 = round(layer8 * coverage8 / 255.0);
    let texel8 = round(texel.a * 255.0);
    let src8 = round(texel8 * alpha8 / 255.0);
    return vec4<f32>(texel.rgb, src8 / 255.0);
}
