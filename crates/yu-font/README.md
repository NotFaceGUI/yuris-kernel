# yu-font

YU-RIS 字体源 + 字形栅格化 + 文本样式合成。**只管"一个字长什么样",
不管排版布局** —— 换行/对齐/ruby/逐字节奏/文本层管理归 `yu-gfx/text`
与后端。

## 它干什么

- **字体发现/加载**(`source.rs`):自带字节优先;桌面/Android 走系统枚举
  (fontdb),wasm 禁止系统枚举。引擎默认 family 回退表(ＭＳ ゴシック →
  標準ゴシック → ＭＳ Ｐゴシック → ＭＳ 明朝 → 標準明朝)与官方
  `EnumFontFamiliesExA` 行为对齐;缺失显式 `font.missing`,无静默 fallback。
- **栅格化**(`raster.rs`):swash/skrifa outline → 8-bit coverage。
  对齐官方 GDI 路径的行为:BOLD 优先 bold face、否则固定合成加粗;
  ANTIALIASING 1=灰度 / 0=阈值 128;SX/SY 每轴 0..=256px(默认 20,
  非等比用 transform);无 ClearType(显式 Unimplemented)。
- **样式管线**(`style.rs` + `render.rs`):COLOR/COLOR2、GRADATION、
  DRAWMODE(0=replace/1=blend)、outline(round 膨胀)、shadow(整数偏移、
  无模糊)、ROTATION(octant×45°)、italic、kerning。固定合成顺序:
  base → bold/italic → outline → shadow → color/gradient → rotation →
  调用方 DRAWMODE。
- **证据门禁**:`StylePolicy::Strict`(生产,默认)只接受有 0.495 证据的
  样式,未取证效果返回 `font.unverified`;`Reference` 是显式 opt-in
  (参考渲染器用)。
- **有界确定性 LRU cache**(`cache.rs`):条目 + 像素双上限;
  同 `(font, char, style)` 两次输出逐字节一致。
- **排版只留接口**(`layout.rs`):`TextLayout` 抽象 + `render_run`
  参考单行 helper(拒绝换行)。

## 取证基线(硬门禁,`docs/font-provenance.md`)

- 参考工程**无自带字体**(data.ypf 1,407 条 0 个字体扩展)→ 引擎用系统字体;
- 官方栅格路径 = GDI `CreateFontIndirectA` + `TextOutA` → 8-bit DIB;
- FONT 参数存储/clamp 由 0.495 二进制逐条钉死;默认值
  (SX/SY=20, AA=1, COLOR=0xffffff, GRADATION=1, ROTATION=0, DRAWMODE=0,
  BOLD=0)由 `FUN_00464fac` 钉死。

## 未解决(如实)

GRADATION 方向映射、ROTATION 渲染语义、BNO 字体列表分支 `0x444ce7`、
GDI `lfHeight` 与 SX/SY 的精确映射、`FONT[NAME]` 找不到时的官方回退行为
—— 均未取证,相应路径 Strict 下拒绝或仅保存值。

## 验证

```bash
cargo test -p yu-font
cargo run -p yu-font --example render_reference   # 参考 PNG → target/tmp/font-ref/
cargo check -p yu-font --target wasm32-unknown-unknown
```

字形/样式 golden 在 `tests/golden/`;接口契约冻结面见 `docs/font-contract.md`。
