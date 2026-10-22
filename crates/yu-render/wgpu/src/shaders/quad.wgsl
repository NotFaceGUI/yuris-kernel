// 普通图层 quad。
//
// SRC_MODE(管线常量,见 `yu_gfx::BlendMode` 的固定管线状态表):
//   0 = MODE 0/1:stage0 SEL1(TEXTURE) => 源色 T
//   1 = MODE 2/5/6/9:stage0 MOD(D,T) => T*A(A = 顶点 DIFFUSE 灰度)
//   2 = MODE 3/7:stage1 BLENDCURRENTALPHA + 白纹理/白 TFACTOR
//       => T*A + (1 - C.a)
//
// alpha 链与软后端逐位一致:texel8 = round(a*255),
// src8 = round(texel8*layer8/255)。SRC_MODE=0 时 RGB 保持未量化的采样值,
// 以维持既有 600 帧基线(线性过滤边缘不被额外量化)。

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

override SRC_MODE: u32 = 0u;

@vertex
fn vs_main(in: VsIn) -> VsOut {
    return VsOut(vec4<f32>(in.position, 0.0, 1.0), in.uv, in.alpha);
}

@group(0) @binding(0) var src_texture: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let texel = textureSample(src_texture, src_sampler, in.uv);
    let layer8 = round(in.alpha * 255.0);
    let texel8 = round(texel.a * 255.0);
    let src8 = round(texel8 * layer8 / 255.0);
    let frag = src8 / 255.0;
    if (SRC_MODE == 0u) {
        return vec4<f32>(texel.rgb, frag);
    }
    var stage8 = texel.rgb;
    if (SRC_MODE == 1u) {
        stage8 = round(texel.rgb * layer8);
    } else {
        stage8 = round(texel.rgb * layer8 + (1.0 - frag) * 255.0);
    }
    return vec4<f32>(stage8 / 255.0, frag);
}
