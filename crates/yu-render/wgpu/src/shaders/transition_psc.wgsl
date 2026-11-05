// 引擎 effect Pass1(psC,TID/mask 转场;配对 + TA>=1 即走此路径)。
//
// 反汇编(oC0):
//   oC0.rgb = tex1.rgb
//   oC0.a   = tex1.a * saturate((YS_FadeVal - 0.5*tex2.r) * YS_FadeVal2)
//             * YS_AlphaVal
//
// mask 取配对纹理 **R** 通道;YS_FadeVal2 = TA,YS_AlphaVal = A/256。
// u8 量化链与软后端一致。
//
// 两个采样窗:`in.uv` 是**本层内容**的采样窗(共享表面上的子矩形),
// `mask_uv` 由同一层内 [0,1] 归一化坐标映射到配对遮罩自己的采样窗。
// 官方两个纹理阶段共用顶点 UV;内核的共享表面需要这层映射才能保证
// 立绘/组内容不被整张共享纹理拉伸(否则立绘在转场时被压扁)。

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
    // 本层内容的 UV 窗(顶点 UV = 该窗)。
    src_min: vec2<f32>,
    src_max: vec2<f32>,
    // 配对遮罩自己的 UV 窗。
    mask_min: vec2<f32>,
    mask_max: vec2<f32>,
};
@group(0) @binding(4) var<uniform> fade: Fade;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // 顶点 UV 是本层内容的采样窗;先还原到层内 [0,1],再映射到遮罩窗。
    // 采样窗可能**反向**(`HORIZONTAL`/`VERTICAL`、负缩放),不能强制
    // `max-min > 0`,否则归一化坐标会塌成一个点。
    let span = fade.src_max - fade.src_min;
    let safe = select(vec2<f32>(1e-6, 1e-6), span, abs(span) > vec2<f32>(1e-6, 1e-6));
    let unit = clamp((in.uv - fade.src_min) / safe, vec2<f32>(0.0), vec2<f32>(1.0));
    let mask_uv = fade.mask_min + unit * (fade.mask_max - fade.mask_min);
    let texel = textureSample(src_texture, src_sampler, in.uv);
    let mask = textureSample(mask_texture, mask_sampler, mask_uv);
    // 官方遮罩采样约定 `(texel + 1) / 256`(UNORM 值还原成 texel 后 +1),
    // 不是 /255;见 `j2-micro-trans8` 的 40 格 α 表(DIV-GFX-065 §6)。
    let mask_value = (mask.r * 255.0 + 1.0) / 256.0;
    let coverage = clamp((fade.fade - 0.5 * mask_value) * fade.fade2, 0.0, 1.0);
    let coverage8 = round(coverage * 255.0);
    let layer8 = round(fade.alpha * 255.0);
    let alpha8 = round(layer8 * coverage8 / 255.0);
    let texel8 = round(texel.a * 255.0);
    let src8 = round(texel8 * alpha8 / 255.0);
    return vec4<f32>(texel.rgb, src8 / 255.0);
}
