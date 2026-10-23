# font-contract · `yu-font` 接口与样式契约

> 面向后续桥接：`yu-gfx/text`、后端、存档/脚本层。证据来源见
> `docs/font-provenance.md`。本文件是 API/语义的冻结面；改动必须同步
> `PROGRESS.md` 与测试 golden。

## 1. 边界

`yu-font` 负责：

- 字体发现/加载（自带字体优先、系统字体、显式缺失诊断）；
- 单字形栅格化（coverage）；
- 字体槽状态（`FONT` 参数）与样式合成；
- 有界 glyph cache；
- **排版接口**（`TextLayout`）。

`yu-font` **不负责**：换行、对齐、ruby、逐字节奏、行距、文本层管理、最终
屏幕合成。这些归 `yu-gfx/text` / 后端。

补充(P2):CP932 字节 → `char` 的**唯一**解码器在 `yu_font::cp932`
(`decode` / `decode_char`,WHATWG `shift_jis`);两个渲染后端共用它,保证
字符流一致。

## 2. 证据等级与策略

```rust
pub enum Support { Confirmed, Likely, Unverified, Unimplemented }
pub enum StylePolicy { Strict, Reference }
```

- `Strict`（默认，生产）：只接受有 0.495 证据的样式。任何未取证效果返回
  `font.unverified`（`Severity::Unimplemented`）。
- `Reference`：显式 opt-in，允许已实现但未取证的效果（参考渲染器使用）。
- 永远 `Unimplemented`：`AaMode::ClearType`、`OutlineJoin::Miter`、
  `Shadow.blur > 0`；即使 `Reference` 也返回 `font.unimplemented`。

能力矩阵（同 `yu_font::CAPABILITIES`）：

| 能力 | 等级 | 说明 | 实现位置 | 测试 |
|---|---|---|---|---|
| SX/SY | Confirmed | 0..=256 px；默认 20；ERIS 12/12、14/15 | `raster.rs` | `styles.rs::anisotropic_sx_sy_changes_box` |
| BOLD | Confirmed | `lfWeight 700/400`；优先 bold face，否则固定合成 | `source.rs::resolve_style`、`raster.rs` | `styles.rs::bold_increases_ink` |
| ANTIALIASING | Confirmed | 1=灰度 coverage，0=阈值 128；默认 1 | `raster.rs` | `styles.rs::antialias_off_is_binary` |
| COLOR | Confirmed | `0xRRGGBB`，默认 `0xffffff` | `color.rs`、`render.rs` | `style_golden.rs` |
| COLOR2 | Confirmed | 渐变第二色 | `render.rs` | `style_golden.rs::gradient...` |
| GRADATION | Likely | 0..=8，默认 1；方向映射未取证 | `render.rs::gradation_t` | `styles.rs::vertical_gradient...` |
| DRAWMODE | Confirmed | 0=replace（含透明），1=blend | `image.rs::draw_onto`、`render.rs` | `image.rs` 单测 |
| ROTATION | Unverified | 0..=8，默认 0；octant×45° | `render.rs::rotate_image` | `styles.rs::rotation...` |
| italic | Unverified | 无 FONT 参数；face 优先，否则 shear 12° | `source.rs`、`raster.rs` | `styles.rs::italic...` |
| outline | Unverified | 无 FONT 参数；round coverage 膨胀 | `render.rs::dilate` | `styles.rs::outline...` |
| shadow | Unverified | 无 FONT 参数；整数偏移；无模糊 | `render.rs::compose_glyph` | `styles.rs::shadow...` |
| kerning | Unverified | 引擎逐字符 `TextOutA`；仅显式 `Kerning::Font` | `layout.rs`、`source.rs` | `layout` 未纳入 golden |
| ClearType | Unimplemented | 引擎无 ClearType | — | `style.rs` 单测 |
| 位图字体 | Confirmed 不适用 | 参考工程无位图字体 | — | 扫描报告 |

## 3. 公开 API（冻结面）

### 3.1 字体源

```rust
pub struct FontSource { /* fontdb::Database + source-local face table */ }
pub struct FaceId(pub(crate) u32);                   // source-local
pub enum FaceOrigin { Embedded, System }
pub enum FaceStyle { Normal, Italic, Oblique }
pub struct FaceInfo { id, families, post_script_name, weight, style, index, origin }
pub struct ResolvedFace { face, synthetic_bold, synthetic_italic }

impl FontSource {
    pub fn from_bytes(bytes: Vec<u8>, origin: FaceOrigin) -> Result<Self, Diag>;
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, Diag>;
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_system_fonts() -> Result<Self, Diag>;
    #[cfg(target_arch = "wasm32")]
    pub fn load_system_fonts() -> Result<Self, Diag>;   // font.system_unavailable
    pub fn default_face(&self) -> Result<FaceId, Diag>;
    pub fn find_family(&self, name: &str) -> Option<FaceId>;
    pub fn face_by_no(&self, no: usize) -> Option<FaceId>;
    pub fn face_info(&self, face: FaceId) -> Result<FaceInfo, Diag>;
    pub fn family_names(&self) -> Vec<String>;
    pub fn resolve_style(&self, face: FaceId, bold: bool, italic: Italic)
        -> Result<ResolvedFace, Diag>;
    pub fn with_face_data<T>(&self, face: FaceId, f: impl FnOnce(&[u8], u32) -> T)
        -> Result<T, Diag>;
    pub fn glyph_index(&self, face: FaceId, ch: char) -> Result<Option<u16>, Diag>;
    pub fn advance_px(&self, face: FaceId, gid: u16, sx: u16, sy: u16) -> Result<f32, Diag>;
    /// 单元顶边 → baseline 的距离(`sy` px;`hhea/OS2` ascent 缩放)。
    /// 文本后端用它把 `CGACT[TEXT Y]`(单元顶边)换成字形基线。
    /// `Likely`:GDI `lfHeight` 的整数舍入未取证(`DIV-GFX-056`)。
    pub fn ascent_px(&self, face: FaceId, sy: u16) -> Result<f32, Diag>;
    pub fn units_per_em(&self, face: FaceId) -> Result<u16, Diag>;
    pub fn kerning_units(&self, face: FaceId, left: u16, right: u16) -> Result<i16, Diag>;
}
```

- `FaceId` 是 **source-local**；不同 `FontSource` 的 `FaceId` 不能混用。
- 自带字体优先：调用方先 `from_bytes/from_file`，失败再 `load_system_fonts`。
- 缺失字体**显式失败**；`default_face` 只按引擎回退表，不静默换 Noto/sans。
- `NO` 的列表顺序是本 crate 的确定性排序（family 名 + face index），**不是**
  GDI `EnumFontFamiliesExA` 顺序（见 §8 DIV-FONT-001）。
- `NAME` 支持全角 ASCII/空格/连字符折叠；`ＭＳ ゴシック` 与 fontdb 的
  `ＭＳ ゴシック` 直接匹配。

### 3.2 栅格化

```rust
pub struct RasterParams { sx, sy, bold, italic, aa }
pub struct Rasterizer { /* swash::ScaleContext */ }
impl Rasterizer {
    pub fn rasterize(&mut self, source: &FontSource, face: FaceId, ch: char,
                     params: RasterParams) -> Result<GlyphBitmap, Diag>;
}

pub struct GlyphBitmap {
    width, height, bearing_x, bearing_y, advance /* 1/256 px */,
    coverage: Vec<u8>,       // 8-bit, top-down
}
```

坐标：`bearing_x` = 左缘相对 pen（右正）；`bearing_y` = 上缘相对 baseline
（y-up，正上）。`advance` 为 1/256 px。

算法：`swash` + `skrifa` outline → `zeno` coverage（Alpha）。非等比 SX/SY 用
`Render::transform { xx: sx/sy, yy: 1 }` 在最终尺寸栅格化，而不是位图拉伸
（`Likely`，GDI 的 `lfWidth=0` 细节未完全可观测）。

BOLD：

1. `FontSource::resolve_style` 先在相同 family 内找 weight ≥ 600 的 face；
2. 找不到时 `Render::embolden((sy*0.04).clamp(0.5, 2.0))`（固定算法，`Likely`；
   GDI 合成量未取证）。

Italic：

1. `Italic::Face` 先找 italic/oblique face；
2. 找不到时 shear 12°；`Synthetic{degrees}` 始终 shear。

### 3.3 样式与渲染

```rust
pub struct TextStyle {
    face: FaceId,
    sx: u16, sy: u16,                 // 0..=256
    bold: bool,
    italic: Italic,
    aa: AaMode,                       // Grayscale | None | ClearType
    color: Rgb, color2: Rgb,          // 0xRRGGBB
    gradation: Gradation,             // None | Direction{octant:1..8}
    gradation_scope: GradationScope,  // PerGlyph | WholeRun
    draw_mode: DrawMode,              // Replace | Blend
    rotation: Rotation,               // None | Octant(1..8)
    outline: Option<Outline>,         // width 1..=16, round
    shadow: Option<Shadow>,           // dx/dy, blur=0
    kerning: Kerning,                 // None | Font
}

impl TextStyle {
    pub fn engine_default(face: FaceId) -> Self;
    pub fn validate(&self, policy: StylePolicy) -> Result<(), Diag>;
    pub fn validate_all(&self, policy: StylePolicy) -> Vec<Diag>;
}
```

引擎默认（`FUN_00464fac`）：`SX=SY=20`、`bold=false`、`AA=Grayscale`、
`color=color2=#ffffff`、`gradation=Direction{1}`、`rotation=None`、
`draw_mode=Replace`、无 outline/shadow/italic/kerning。

```rust
pub struct FontRenderer { /* FontSource + Rasterizer + GlyphCache */ }
impl FontRenderer {
    pub fn new(source: FontSource, limits: CacheLimits) -> Self;
    pub fn render_glyph(&mut self, ch: char, style: &TextStyle, policy: StylePolicy)
        -> Result<Arc<GlyphImage>, Diag>;
    pub fn render_run(&mut self, text: &str, style: &TextStyle, policy: StylePolicy)
        -> Result<GlyphRun, Diag>;       // reference helper, no line breaking
    pub fn cache_stats(&self) -> CacheStats;
    pub fn clear_cache(&mut self);
}

pub struct GlyphImage {
    width, height, left /* pen x */, top /* baseline y-down */,
    advance /* 1/256 px */, rgba: Vec<u8>,
}
pub struct GlyphRun {
    width, height, baseline_x, baseline_y, rgba, advance,
}
```

`render_run` 是**参考渲染器的便利函数**，不是引擎排版：

- 只做单行水平步进（可选 legacy `kern`）；
- 遇到 `\n`/`\r` 返回 `font.unimplemented`；
- 不做换行、对齐、ruby、逐字节奏。

### 3.4 缓存

```rust
pub struct CacheLimits { max_entries: usize, max_pixels: u64 } // 默认 4096 / 8M px
pub struct GlyphKey {
    face, ch, sx, sy, bold, italic_q, aa,
    outline_width/color, shadow_dx/dy/color/blur/outline,
    rotation_octant, color, color2, gradation_octant, draw_mode,
}
pub struct GlyphCache { /* HashMap + BTreeMap LRU tick */ }
```

- key 含全部影响最终像素的字段；`FaceId` 是 source-local。
- 双上限（条目数 + 像素数）；淘汰严格按单调 tick 的 LRU，确定性。
- `CacheStats { hits, misses, insertions, evictions, entries, pixels, memory_bytes }`。
- `memory_bytes` = `Σ(width*height*4 + 128)`（近似；不含 allocator 开销）。
- `GradationScope::WholeRun` 不进入 per-glyph cache（需要 run 范围），参考路径
  会重新栅格化；生产应优先 `PerGlyph`。

### 3.5 FONT 槽

```rust
pub struct FontCommand { bno, no, name, antialiasing, color, color2,
                         gradation, rotation, drawmode, sx, sy, bold }
pub struct FontSlot { bno, no, name, antialiasing, color, color2,
                      gradation, rotation, drawmode, sx, sy, bold }
impl FontSlot {
    pub fn engine_default(name: impl Into<String>) -> Self;
    pub fn apply(&mut self, cmd: &FontCommand, source: &FontSource) -> Result<(), Diag>;
    pub fn to_style(&self, face: FaceId) -> TextStyle;
}
```

`apply` 复刻引擎 clamp（而不是报错）：`SX/SY 0..=256`、`COLOR/COLOR2
0..=0xffffff`、`GRADATION/ROTATION 0..=8`、`AA/DRAW/BOLD 0..=1`。`NO` 越界是
`font.missing`；`NAME` 找不到是 `font.missing`；`BNO` 仅保存。

### 3.6 排版接口（只定义，不实现）

```rust
pub struct PlacedGlyph { ch: char, x: f32, y: f32 }
pub trait TextLayout {
    fn layout(&mut self, source: &FontSource, style: &TextStyle, text: &str)
        -> Vec<PlacedGlyph>;
}
pub struct HorizontalLayout { kerning: Kerning } // reference-only helper
```

引擎换行/对齐/ruby/逐字节奏由 `yu-gfx/text` 实现；本 crate 不提供。

## 4. 样式合成顺序（固定）

```text
base coverage
  → bold/italic（在 rasterizer 内完成：face 选择 / embolden / shear）
  → outline coverage（round dilation，1 px AA 带）
  → shadow coverage（整数偏移；模糊未实现）
  → color / COLOR2 / GRADATION 着色（per-glyph 或 whole-run）
  → 图层自底向上：shadow → outline → fill
  → ROTATION（绕 pen 原点旋转整个 glyph 图层，bilinear premultiplied）
  → 调用方按 DRAWMODE 合成到目标
```

- `DRAWMODE=0`（Replace）：目标像素被新 RGBA 替换（含 alpha=0）；在
  `GlyphImage::draw_onto` 中逐像素 replace。
- `DRAWMODE=1`（Blend）：source-over。
- outline 的 round join 由距离膨胀天然产生；miter 未实现。
- shadow 的 `include_outline=true` 时基于 outline 剪影，否则基于 base。
- GRADATION 颜色插值在 sRGB 字节空间；色标是 `COLOR`→`COLOR2`。

## 5. 诊断码（`yu_font::code::font`）

| 码 | 场景 |
|---|---|
| `font.missing` | face/family/字体文件不存在；系统枚举 0 个 face |
| `font.missing_style` | 显式请求的 style 无法解析（当前实现会走合成，保留码） |
| `font.glyph_missing` | 字符在 face 中无 glyph |
| `font.bad_face` | 非 sfnt/TTC、face index 越界、coverage 截断 |
| `font.io` | 桌面读取字体文件失败 |
| `font.system_unavailable` | wasm32 上请求系统字体 |
| `font.unverified` | `Strict` 下请求未取证效果 |
| `font.unimplemented` | ClearType、miter、blur、换行 |
| `font.style_range` | 超出引擎范围（如 SX>256、outline width 0/17） |

全部不 panic；测试覆盖缺失字体、缺失 glyph、坏字体、越界。

## 6. 跨平台

- 主依赖 `swash`（std+scale+render）与 `skrifa` 可在
  `wasm32-unknown-unknown` / `aarch64-linux-android` 编译。
- `fontdb` 是 `cfg(not(target_arch = "wasm32"))` 的 target dependency；
  wasm 构建不编译它。
- `std::fs` 只出现在 `FontSource::from_file` / `load_system_fonts`
  （`cfg(not(wasm32))` 且 fontdb 的 `fs` feature）。
- wasm 调用方必须用 `from_bytes` 提供字体字节；`load_system_fonts` 返回
  `font.system_unavailable`。
- Windows GDI 参考栅格器未进入默认依赖（`gdi-reference` feature 保留位，
  当前无实现；GDI 事实只在证据文档）。

## 7. 性能基准（参考机，release 构建）

命令：`cargo run --release -p yu-font --example render_reference`
（1000 个所选 face 实际覆盖的 CJK 表意文字，1 px outline，
`StylePolicy::Reference`）：

```text
12px outline=1: first 1799.87 ms (1000 glyphs, 1.800 ms/glyph),
                cache hit 0.38 ms (0.0004 ms/glyph),
                entries=1000 pixels=185217 mem=848 KiB, hits=1000 misses=0
24px outline=1: first 2054.27 ms (1000 glyphs, 2.054 ms/glyph),
                cache hit 0.42 ms (0.0004 ms/glyph),
                entries=1000 pixels=580669 mem=2393 KiB, hits=1000 misses=0
```

说明：首栅格成本主要在 swash outline/coverage + 1 px outline 的距离膨胀；
命中路径只做 key 构造与 LRU 更新（约 0.4 µs/glyph）。字体只解析一次
（fontdb/bytes 在 `FontSource` 生命周期内常驻）；基准的 `font source`
枚举成本不计入首栅格。debug 构建同样可通过 `render_reference` 复现，数值约为
release 的 1.0–1.2 倍（受 CPU/热状态影响）。

## 8. 已知分歧与未解决项

| 编号 | 分歧/未知 | 处理 |
|---|---|---|
| DIV-FONT-001 | `FONT[NO]` 的列表顺序：引擎 = GDI 枚举顺序；本 crate = family 名排序 | `face_by_no` 文档标注；需要 1:1 时用 `NAME` |
| DIV-FONT-002 | 栅格器：GDI outline/hinting vs swash/skrifa | 不宣称字节级一致；diff 报告归因 |
| DIV-FONT-003 | GRADATION 方向映射（0..8）未取证 | 默认 1=竖直上→下；`Likely`；`Strict` 只允许纯色 |
| DIV-FONT-004 | ROTATION 的 0..8 语义/是否参与 0.495 文本渲染未观察到 | octant×45°；`Unverified`；`Reference` only |
| DIV-FONT-005 | outline/shadow/italic/kerning 不是 0.495 FONT 参数 | API 扩展；`Unverified`；`Reference` only |
| DIV-FONT-006 | 合成粗体强度是固定公式，GDI 实际量未知 | `(sy*0.04).clamp(0.5,2.0)`；`Likely` |
| DIV-FONT-007 | BNO 的字体列表语义未解析 | 仅保存值；`font.unverified`/报告 Unknown |
| DIV-FONT-008 | ClearType、miter join、shadow blur | 显式 `font.unimplemented` |
| DIV-FONT-009 | `FONT[NAME]` 若同一 family 有多个 localized 名称，匹配使用全部别名 | 与 GDI 的 family 选择可能不同 |
| DIV-FONT-010 | 引擎的字符宽度/字距使用 GDI 度量；本实现用 swash advance | 行宽可能有亚像素差 |
| DIV-FONT-011 | 单元顶边→基线的 ascent 取整(GDI `lfHeight` 单元 vs hhea ascent) | `ascent_px`(`Likely`)；官方文本截图对拍(H-P2-03) |

需要 REA 复核的项：

1. `0x444ce7` 的 BNO 分支（字体列表编号来源、与 NO 的优先级）；
2. 文本合成器是否在任何路径读取 `GRADATION`/`ROTATION`（本会话未在
   `FUN_00406184`/`FUN_00407cf0` 找到直接偏移读取）；
3. GDI `lfHeight` 与 SX/SY 的精确映射（`0x4450c0` 的 EDX/ECX 参数语义）；
4. GDI 合成粗体的精确像素量（建议用官方截图逐像素拟合）；
5. `FONT[NAME]` 找不到时引擎的回退/报错行为（当前 crate 显式失败）。

## 9. 验收命令

```bash
export TEMP="<side-repo>/workbench/tmp"; TMP="$TEMP"
export YURIS_REQUIRE_SAMPLES=1
export YURIS_SAMPLE_ERIS_ROOT="<side-repo>/workbench/project-eris"   # 数据入口:data.ypf 优先,data/ 为备
export YURIS_SAMPLE_ERIS_YPF="<side-repo>/workbench/project-eris/data.ypf"
cargo test -p yu-font
cargo check -p yu-font --target wasm32-unknown-unknown
cargo check -p yu-font --target aarch64-linux-android
cargo clippy -p yu-font --all-targets
cargo run -p yu-font --example render_reference
grep -rn "println!\|eprintln!\|dbg!" crates/yu-font/src    # 必须为 0
```
