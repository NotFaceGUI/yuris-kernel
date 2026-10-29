// GPU 文本 pass:把一个字形写到目标表面,逐像素复刻 CPU 的
// `Rgba::over` / `replace_with`(`yu-font/src/color.rs`)。
//
// 目标表面是 `Rgba8Unorm` storage read_write;字形位图以 packed u32
// (`r | g<<8 | b<<16 | a<<24`)放在 storage buffer。整数运算,不用浮点
// 混合,保证与 soft 后端逐像素一致。

struct Draw {
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    src: u32,
    blend: u32,
    pad0: u32,
    pad1: u32,
};

@group(0) @binding(0) var dst_tex: texture_storage_2d<rgba8unorm, read_write>;
@group(0) @binding(1) var<storage, read> glyphs: array<u32>;
@group(0) @binding(2) var<storage, read> draws: array<Draw>;
@group(0) @binding(3) var<uniform> draw_count: u32;

fn u8_of(x: f32) -> u32 {
    return u32(clamp(x, 0.0, 1.0) * 255.0 + 0.5);
}

fn over_channel(s: u32, d: u32, sa: u32, da: u32, oa: u32) -> u32 {
    let sv = s * sa;
    let dv = d * da * (255u - sa) / 255u;
    return min(255u, (sv + dv) / oa);
}

@compute @workgroup_size(64)
fn main(@builtin(local_invocation_index) tid: u32) {
    // 单个 workgroup 按 draw 顺序串行:CPU `paint_draw_refs` 是有序的,
    // 多个 workgroup 并行写重叠字形会让最终像素取决于调度顺序。
    let dims = textureDimensions(dst_tex);
    for (var di = 0u; di < draw_count; di = di + 1u) {
        let d = draws[di];
        let n = d.w * d.h;
        for (var i = tid; i < n; i = i + 64u) {
            let sx = i % d.w;
            let sy = i / d.w;
            let dx = d.x + i32(sx);
            let dy = d.y + i32(sy);
            if (dx < 0 || dy < 0 || u32(dx) >= dims.x || u32(dy) >= dims.y) {
                continue;
            }
            let s = glyphs[d.src + i];
            let sa = (s >> 24u) & 0xffu;
            let sr = s & 0xffu;
            let sg = (s >> 8u) & 0xffu;
            let sb = (s >> 16u) & 0xffu;
            var orr = sr;
            var og = sg;
            var ob = sb;
            var oa = sa;
            if (d.blend != 0u) {
                // `Rgba::over`: a==0 保持 dst;a==255 直接 src;否则按公式。
                if (sa == 0u) {
                    continue;
                }
                if (sa != 255u) {
                    let dv = textureLoad(dst_tex, vec2<i32>(dx, dy));
                    let da = u8_of(dv.a);
                    oa = sa + da * (255u - sa) / 255u;
                    if (oa == 0u) {
                        orr = 0u;
                        og = 0u;
                        ob = 0u;
                    } else {
                        let dr = u8_of(dv.r);
                        let dg = u8_of(dv.g);
                        let db = u8_of(dv.b);
                        orr = over_channel(sr, dr, sa, da, oa);
                        og = over_channel(sg, dg, sa, da, oa);
                        ob = over_channel(sb, db, sa, da, oa);
                    }
                }
            }
            textureStore(
                dst_tex,
                vec2<i32>(dx, dy),
                vec4<f32>(
                    f32(orr) / 255.0,
                    f32(og) / 255.0,
                    f32(ob) / 255.0,
                    f32(oa) / 255.0,
                ),
            );
        }
        workgroupBarrier();
    }
}
