// 快照上的 RECTPAINT 族(拷贝完成后在快照纹理上原地绘制)。
//
// write_alpha_only=1 对应 RECTPAINT2;否则整像素写入(颜色由 uniform 给出,
// 已含 alpha 规则)。混合为替换,不做 blend。

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

struct Paint {
    color: vec4<f32>,
};
@group(0) @binding(0) var<uniform> paint: Paint;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return vec4<f32>(paint.color.rgb, paint.color.a * in.alpha);
}
