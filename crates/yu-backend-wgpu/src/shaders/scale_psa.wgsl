// 引擎 effect Pass3(psA,16-tap 双三次,`DAT_00801334 == 3`)。
//
// 字节码 `0x18c9a8`(1432 B)反汇编要点(ps_2_0):
//
//   f  = frc(t0 * YS_TexSize);f2 = f*f;f3 = f2*f
//   w0 = -f3 + 2f2 - f
//   w1 =  f3 - 2f2 + 1
//   w2 = -f3 + f2 + f
//   w3 =  f3 - f2
//   oC0 = Σ_r wY[r] · Σ_c wX[c] · tex(Tap[r][c])     (无最终 saturate)
//
// 8 组 UV 由 `FUN_0041a308`(`DAT_00801334 == 3` 分支)生成:
//   t0=(u-1/W, v-1/H) t1=(u-1/W, v) t2=(u-1/W, v+1/H) t3=(u-1/W, v+2/H)
//   t4=(u,     v-1/H) t5=(u,     v) t6=(u,     v+1/H) t7=(u,     v+2/H)
// 像素 shader 再把 `YS_TexDiffU = (2/W, 0)` 加给 t0..t7,得到第 3/4 列。
// 所以 16 个 tap 是 origin=(u-1/W, v-1/H) 起的 4×4、步长 (1/W, 1/H) 网格。
// 采样器硬编码为 D3DTEXF_POINT(见 `FUN_004195ec` 的 SetSamplerState 1/1/1)。

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

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
    // 引擎真实参数 `YS_TexDiffU` 恒为 (2/W, 0);WGSL 直接用网格步长
    // `du=(1/W,0)` 生成第 0..3 列,与之等价。保留字段以对齐 bind 布局。
    tex_diff_u: vec2<f32>,
};
@group(0) @binding(2) var<uniform> scale: Scale;

// 一行的 4 个 tap 按 ps_2_0 的累加顺序:w1·c1 + w0·c0 + w2·c2 + w3·c3。
fn row(c0: vec4<f32>, c1: vec4<f32>, c2: vec4<f32>, c3: vec4<f32>, weights: vec4<f32>) -> vec4<f32> {
    return c1 * weights.y + c0 * weights.x + c2 * weights.z + c3 * weights.w;
}

fn tap(x: i32, y: i32) -> vec4<f32> {
    let size = vec2<i32>(textureDimensions(src_texture));
    let clamped = clamp(vec2<i32>(x, y), vec2<i32>(0), size - vec2<i32>(1));
    return textureLoad(src_texture, clamped, 0);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let base = in.uv - 0.5 / scale.tex_size;
    let position = base * scale.tex_size;
    // D3D9 point 采样 = floor(u*W);显式 floor 保证 1:1 与边界确定性。
    let origin = floor(position);
    let f = position - origin;
    let ix = i32(origin.x) - 1;
    let iy = i32(origin.y) - 1;
    let f2 = f * f;
    let f3 = f2 * f;
    let wx = vec4<f32>(
        -f3.x + 2.0 * f2.x - f.x,
        f3.x - 2.0 * f2.x + 1.0,
        -f3.x + f2.x + f.x,
        f3.x - f2.x,
    );
    let wy = vec4<f32>(
        -f3.y + 2.0 * f2.y - f.y,
        f3.y - 2.0 * f2.y + 1.0,
        -f3.y + f2.y + f.y,
        f3.y - f2.y,
    );

    // 每行 4 个 tap(列序 c0..c3):origin 已是 (u-1/W, v-1/H)。
    let r0 = row(
        tap(ix, iy),
        tap(ix + 1, iy),
        tap(ix + 2, iy),
        tap(ix + 3, iy),
        wx,
    );
    let r1 = row(
        tap(ix, iy + 1),
        tap(ix + 1, iy + 1),
        tap(ix + 2, iy + 1),
        tap(ix + 3, iy + 1),
        wx,
    );
    let r2 = row(
        tap(ix, iy + 2),
        tap(ix + 1, iy + 2),
        tap(ix + 2, iy + 2),
        tap(ix + 3, iy + 2),
        wx,
    );
    let r3 = row(
        tap(ix, iy + 3),
        tap(ix + 1, iy + 3),
        tap(ix + 2, iy + 3),
        tap(ix + 3, iy + 3),
        wx,
    );
    // 垂直累加顺序与反汇编一致:wy1·r1 → +wy0·r0 → +wy2·r2 → +wy3·r3。
    return r1 * wy.y + r0 * wy.x + r2 * wy.z + r3 * wy.w;
}
