// 把引擎产出的 RGBA8 帧贴到窗口 surface。
//
// uv 约定:纹理 v=0 在顶部;NDC y=+1 是顶部,所以 v = (1 - y) / 2。
// 不做 sRGB 转换:引擎按存储值混合,宿主只负责搬运。
//
// # 为什么用四边形而不是"全屏三角形"
//
// 全屏三角形(顶点 -1,-1 / 3,-1 / -1,3)只在**恰好铺满屏幕**时成立:它的
// 斜边在 NDC 的 x+y=2 处,正好贴着可见区角落。一旦顶点被缩放(letterbox
// 需要的等比收缩)斜边就切进可见区,画面出现对角黑边(实机已复现)。
//
// 这里改用 4 顶点四边形:缩放只影响矩形范围,覆盖性永远完整。
//
// # 等比缩放(letterbox)
//
// 游戏视口固定 4:3,窗口可能是 20:9。按 min(scaleX, scaleY) 等比缩放并居中,
// 多余的两侧/上下留黑边 —— 与桌面宿主一致,不做拉伸填充。

struct Params {
    /// xy = 游戏视口尺寸(像素);zw = 窗口尺寸(像素)。
    viewport_and_window: vec4<f32>,
};

@group(0) @binding(0) var frame_texture: texture_2d<f32>;
@group(0) @binding(1) var frame_sampler: sampler;
@group(0) @binding(2) var<uniform> params: Params;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) index: u32) -> VertexOut {
    // 四边形两三角形:0,1,2  2,1,3
    var corners = array<vec2<f32>, 4>(
        vec2<f32>(-1.0, -1.0), // 左下
        vec2<f32>( 1.0, -1.0), // 右下
        vec2<f32>(-1.0,  1.0), // 左上
        vec2<f32>( 1.0,  1.0), // 右上
    );
    var indices = array<u32, 6>(0u, 1u, 2u, 2u, 1u, 3u);
    let p = corners[indices[index]];

    // 等比因子:两维共用同一个缩放,较小的一维决定可见范围。
    let viewport = max(params.viewport_and_window.xy, vec2<f32>(1.0, 1.0));
    let window = max(params.viewport_and_window.zw, vec2<f32>(1.0, 1.0));
    let uniform_scale = min(window.x / viewport.x, window.y / viewport.y);
    // 几何体在 NDC 里的占屏比例(<=1;小于 1 的那一维就是黑边所在)。
    let fit = vec2<f32>(
        viewport.x * uniform_scale / window.x,
        viewport.y * uniform_scale / window.y,
    );

    var out: VertexOut;
    // 四边形是矩形:缩放后 UV 仍恰好铺满 0..1。
    out.uv = vec2<f32>((p.x + 1.0) * 0.5, (1.0 - p.y) * 0.5);
    out.position = vec4<f32>(p * fit, 0.0, 1.0);
    return out;
}

@fragment
fn fs(input: VertexOut) -> @location(0) vec4<f32> {
    return textureSample(frame_texture, frame_sampler, input.uv);
}
