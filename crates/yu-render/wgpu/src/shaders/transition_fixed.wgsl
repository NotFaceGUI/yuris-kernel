// 固定管线双纹理过渡(`_DXMODE=0`)。
//
// 配对纹理 G 通道为 mask(EVID-T-05)。覆盖度 = 等值线上的**过渡带**:
//
//   c          = 1 - 2*fade                等值线(随推进从 1 扫到 0)
//   half_band  = 2 / TA                    TA 越大越硬(脚本「くっきり」)
//   coverage   = clamp((mask.g - c)/half_band + 0.5, 0, 1)
//   complete   = 1(T>=256)直接为满
//
// 等值线上 α=0.5,与官方 `j2-micro-trans6/8` 的 `mask=128,T=128 → 0.498`
// 一致;`w≈2/TA` 由 `j2-micro-trans8` 四个 TA 逐格拟合(DIV-GFX-065 §12)。
// 与 `yu_gfx::transition_coverage` 同式,并按软后端的 u8 量化链输出 src alpha。

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
    // 剧情边界柔带 `0.16/TA`(mask 单位);TA 越大越硬(「くっきり」)。
    wide: f32,
    pad: f32,
    // 本层有效 UV 矩形(层上 `SIZE`/`SIZE2` 的子矩形)。
    uv_min: vec2<f32>,
    uv_max: vec2<f32>,
    // 配对层(mask)自己的有效 UV 矩形。
    mask_min: vec2<f32>,
    mask_max: vec2<f32>,
};
@group(0) @binding(4) var<uniform> fade: Fade;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let texel = textureSample(src_texture, src_sampler, in.uv);
    // 源纹理可能是共享表面上的子矩形:先把顶点 UV 反归一化回层内 [0,1],
    // 再映射到 mask 自己的有效矩形(配对层也可能挂 `SIZE` 修饰)。
    let span = max(fade.uv_max - fade.uv_min, vec2<f32>(1e-6, 1e-6));
    let unit = clamp((in.uv - fade.uv_min) / span, vec2<f32>(0.0), vec2<f32>(1.0));
    let mask_uv = fade.mask_min + unit * (fade.mask_max - fade.mask_min);
    let mask = textureSample(mask_texture, mask_sampler, mask_uv);
    // 单一公式:亚像素带 `1/K` + 官方「一格」带 `0.16/TA`。
    let threshold = 1.0 - 2.0 * fade.fade;
    let slope = 30.0 + 70.0 * fade.fade;
    let band = 1.0 / slope + fade.wide;
    var coverage = clamp((mask.g - threshold) / band, 0.0, 1.0);
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
