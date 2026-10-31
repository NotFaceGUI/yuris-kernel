// `CGACT[MERGE=1]` 的 GPU 合成,与 soft `apply_merge(lower, upper, false)`
// **逐位一致**。
//
// ## 官方口径(REA 0.495,逐字节手解,2026-10-31 定案)
//
// `CGACT` 处理体 `0x42c4a4` → MERGE 标志 `0x684182` → `CALL 0x43b33c`
// (`FUN_0043b33c`)。它是**逐行 CPU 循环**,混合内核经 `DAT_005d161c`
// 指到 `LAB_00402828`(全不透明快路径 `DAT_005d1618` = `LAB_004028f8`
// 是纯拷贝)。内核按 4 个像素展开,单个像素的算术是:
//
// ```text
//   T   = L.a·U.a/255                  ; 查表 0x62f4de(字表,行距 0x400)
//   w   = (U.a − T)·srcA >> 8            ; srcA = **上层**的 [层+0x80]
//   a'  = L.a + w
//   U'  = U.a·256/a'                   ; 查表 0x78b6e0,a'=0 的行全 0
//   co  = L.c + ((U.c − L.c)·U'·srcA) >> 16
// ```
//
// 三处细节都是官方实现自带的,必须照抄:
//
// 1. **`co` 是 lerp**,不是加权平均:`L.c + (U.c−L.c)·强度`。这条天然满足
//    「下层不可见处不泄色」—— 素材 PNG 在透明区常带残留 RGB(实测立绘缓冲
//    `SPLBUF702` 在眼睛区域有 26773 个 alpha=0 但 RGB 非零的像素,均值
//    159,117,122),`L.a=0 ⇒ w=U.a ⇒ a'=U.a ⇒ U'=256 ⇒ co=U.c`,
//    脏色永远进不了可见区。2b53786 之前按 `cl·(1−a)+cu·a` 混,差分抗锯齿边
//    (a≈128)处把脏色按 50% 拉进可见区 —— 就是眨眼时那圈眼罩伪影。
// 2. **`srcA` 是源混合强度**,`256` = 全混合、`0` = 完全不动。下层的 `dstA`
//    **不参与** MERGE(执行体根本不传这个参数;只有 MERGE2 传)。
// 3. **`U' = U.a·256/a'` 用整数除法截断**(表由 `FUN_0045c04c` 的
//    `0x45c58f` 循环 `MOV EAX,ESI; CDQ; IDIV EBX` 生成)。所以 `U.a=128`
//    且下层不透明时强度是 `floor(128·256/255)/256 = 128/256`,而不是
//    `128/255` —— 与「按 255 归一化」的直觉差一点点,但这是官方的值。
//
// `T` 与 `U'` 都直接用官方同一算式现算(WGSL 的整数除法同样是向零截断),
// 不需要把 256×512 的查找表上传成 storage buffer。
//
// ## 与 `MERGE2` 的关系
//
// 两者是**分开的执行体**,`MERGE2`(`0x43b8ec` → 内核 `0x402654`)用的是
// 另一套算术(下层自身 alpha 也乘下层层 alpha、RGB 除以 `(a'+1)`),
// 见 `composite.wgsl` mode 0。**模式层公式并不相同**,层 alpha≡256 时也
// 会差 1 LSB —— 这是官方的真实差异,不是我们的误差。
//
// 越界像素 / `U.a=0` ⇒ 原样返回下层(CPU 循环只覆盖 min 尺寸)。

@group(0) @binding(0) var t_lower: texture_2d<f32>;
@group(0) @binding(1) var t_upper: texture_2d<f32>;

// 层不透明度 `[上层的 [层+0x80], 下层的 [层+0x80]]`,各 0..256。
// `MERGE` 只读 `.src`;`.dst` 由宿主固定传 256(执行体不传这个参数)。
struct LayerAlpha {
    src: u32,
    dst: u32,
    _pad0: u32,
    _pad1: u32,
};
@group(0) @binding(2) var<uniform> u_layer: LayerAlpha;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    return vec4<f32>(pos[index], 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let lower_dim = vec2<i32>(textureDimensions(t_lower));
    let upper_dim = vec2<i32>(textureDimensions(t_upper));
    let p = vec2<i32>(floor(frag.xy));

    var lower = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    if (p.x >= 0 && p.y >= 0 && p.x < lower_dim.x && p.y < lower_dim.y) {
        lower = textureLoad(t_lower, p, 0);
    }
    if (p.x < 0 || p.y < 0 || p.x >= upper_dim.x || p.y >= upper_dim.y) {
        return lower;
    }
    let upper = textureLoad(t_upper, p, 0);
    let up = vec4<u32>(round(upper * 255.0));
    let l_raw = vec4<u32>(round(lower * 255.0));
    let ua = up.w;
    if (ua == 0u) {
        // T=0、w=0、a'=L.a、U'=0 ⇒ 四通道恒等。
        return lower;
    }
    let la = l_raw.w;
    let layer = u_layer.src;

    // T = L.a·U.a/255;w = (U.a − T)·srcA>>8;a' = L.a + w。
    let t = la * ua / 255u;
    let w = ((i32(ua) - i32(t)) * i32(layer)) >> 8;
    let a_out = u32(clamp(i32(la) + w, 0, 255));
    // U' = U.a·256/a'(a'=0 时官方查表取 0)。
    var u_w = 0u;
    if (a_out != 0u) {
        u_w = ua * 256u / a_out;
    }
    let prod = i32(u_w) * i32(layer);

    var rgb = vec3<u32>(0u, 0u, 0u);
    for (var c = 0u; c < 3u; c += 1u) {
        let delta = i32(up[c]) - i32(l_raw[c]);
        // 官方 `IMUL` + `SAR 0x10`:32 位回绕乘 + 算术右移。
        let v = i32(l_raw[c]) + ((delta * prod) >> 16);
        rgb[c] = u32(clamp(v, 0, 255));
    }
    return vec4<f32>(
        f32(rgb.x) / 255.0,
        f32(rgb.y) / 255.0,
        f32(rgb.z) / 255.0,
        f32(a_out) / 255.0,
    );
}
