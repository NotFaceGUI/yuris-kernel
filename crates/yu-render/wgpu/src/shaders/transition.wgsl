// `CGACT[TRANSITION/TRANSITION2]` 的**带落点** GPU 形态。
//
// 无 `X/Y` 的 `TRANSITION` 仍走 `lerp.wgsl`(整层、画布 = to 尺寸,已实证);
// 本 shader 只处理:
// - `TRANSITION` 带 `X/Y`:目标层求交矩形内两源 lerp,矩形外保留底面;
// - `TRANSITION2`:三源 + 遮罩,目标层求交矩形内逐像素过渡。
//
// ## 官方几何(REA 0.495)
//
// `0x43bad0`(TRANSITION)/`0x43bca0`(TRANSITION2)入口按
// `src.width+X`、`src.height+Y`、`X<0`、`Y<0` 计算四边剪切位,再调
// `FUN_0043b518`:目标起点 `(X,Y)`、源起点 `(0,0)`、跨度 = 源尺寸 ∩
// 目标表面。目标矩形外**不写像素** ⇒ 本 shader 返回 base 原值。
//
// ## TRANSITION2(`0x43bca0` + `FUN_0043c008`)
//
// `ID`=源 A、`ID2`=源 B、`ID3`=遮罩、`ID4`=目标;`SET`=进度、
// `SET2`=遮罩对比:
//
// ```text
// SET = 0        → 原样拷贝源 A
// SET >= 255     → 原样拷贝源 B
// 其余:mask 字节 = 内存序 BGRA 的 B(本纹理 RGBA 的蓝通道)
//       w = table[mask]; w<2 ⇒ 源 B
//       否则 out[c] = B[c] − ((B[c]−A[c])·w >> 8)
// table = FUN_0043c110(k = 256−SET, SET2):
//   SET2 == 256: mask < SET ⇒ 0,否则 256
//   否则: acc=m·SET2; v=(acc>>1)+((SET2>>1)·k)+SET2·(−128)+k;
//         v=min(v,255); table[m]=v+1
// ```
//
// 8 位回绕照抄官方 `char` 截断(不 clamp)。

struct Params {
    // 0 = TRANSITION,1 = TRANSITION2
    mode: u32,
    dx: i32,
    dy: i32,
    progress: i32,
    contrast: i32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
};

@group(0) @binding(0) var t_base: texture_2d<f32>;
@group(0) @binding(1) var t_from: texture_2d<f32>;
@group(0) @binding(2) var t_to: texture_2d<f32>;
// TRANSITION2 的 ID3;mode 0 时绑定 `to` 作占位,不读取。
@group(0) @binding(3) var t_mask: texture_2d<f32>;
@group(0) @binding(4) var<uniform> u: Params;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    return vec4<f32>(pos[index], 0.0, 1.0);
}

fn load_or_zero(t: texture_2d<f32>, p: vec2<i32>) -> vec4<f32> {
    let d = vec2<i32>(textureDimensions(t));
    if (p.x < 0 || p.y < 0 || p.x >= d.x || p.y >= d.y) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    return textureLoad(t, p, 0);
}

@fragment
fn fs_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let p = vec2<i32>(floor(frag.xy));
    let base = textureLoad(t_base, p, 0);
    let from_dim = vec2<i32>(textureDimensions(t_from));
    let sp = p - vec2<i32>(u.dx, u.dy);
    // 落点/源矩形外:官方不写目标。
    if (sp.x < 0 || sp.y < 0 || sp.x >= from_dim.x || sp.y >= from_dim.y) {
        return base;
    }

    if (u.mode == 0u) {
        // TRANSITION:与 lerp.wgsl 同一 alpha 加权整数式(k = SET+1)。
        let from_c = textureLoad(t_from, sp, 0);
        let to_c = load_or_zero(t_to, sp);
        let a = vec4<f32>(round(from_c * 255.0));
        let b = vec4<f32>(round(to_c * 255.0));
        let set0 = clamp(u.progress, 0, 255);
        let k = f32(set0 + 1);
        let kf = 256.0 - k;
        let wf = a.a * kf;
        let wt = b.a * k;
        let den = wf + wt;
        if (den <= 0.0) {
            return vec4<f32>(0.0, 0.0, 0.0, 0.0);
        }
        let num = a.rgb * wf + b.rgb * wt;
        let rgb = floor((num + den * 0.5) / den);
        let alpha = floor((den + 128.0) / 256.0);
        return clamp(vec4<f32>(rgb, alpha), vec4<f32>(0.0), vec4<f32>(255.0)) / 255.0;
    }

    // TRANSITION2 端点:原样拷贝(含 alpha)。
    if (u.progress <= 0) {
        return textureLoad(t_from, sp, 0);
    }
    if (u.progress >= 255) {
        return load_or_zero(t_to, sp);
    }

    let from_c = vec4<u32>(round(textureLoad(t_from, sp, 0) * 255.0));
    let to_c = vec4<u32>(round(load_or_zero(t_to, sp) * 255.0));
    let mask = vec4<u32>(round(load_or_zero(t_mask, sp) * 255.0));
    let m = i32(mask.z);
    let progress = u.progress; // 1..254
    let k = 256 - progress;
    let s2 = clamp(u.contrast, 0, 256);
    var w: i32;
    if (s2 == 256) {
        w = select(0, 256, m >= progress);
    } else {
        let acc = m * s2;
        var v = (acc >> 1) + ((s2 >> 1) * k) + (s2 * -128) + k;
        v = min(v, 255);
        w = v + 1;
    }
    if (w < 2) {
        return vec4<f32>(vec4<f32>(to_c) / 255.0);
    }
    var out: vec4<u32>;
    for (var c = 0u; c < 4u; c += 1u) {
        let d = i32(to_c[c]) - (((i32(to_c[c]) - i32(from_c[c])) * w) >> 8);
        out[c] = bitcast<u32>(d) & 0xFFu;
    }
    return vec4<f32>(vec4<f32>(out) / 255.0);
}
