# DIV-FONT-012 · 宿主默认字体与游戏内字体选择器

- 状态: **fixed(2026-11-16;逐字形缺字回退提供可选开关,默认关)**
- 影响面: `crates/yu-font/src/source.rs`、`crates/yu-render/core/src/text.rs`、
  `crates/yu-gfx/src/state.rs`、`crates/yu-engine/src/engine.rs`、
  `crates/yu-cli/src/{args,run,desktop,server}.rs`
- 证据基线: 官方 0.495 `yu-ris.exe` sha256 `4f5eefa3…`;ERIS 编译脚本
  `es_Font.yst` / `ES_CONFIG_FONT.yst` / `es_Text.yst`(Ghidra 12.1.4 旁线
  笔记与脚本源码);手册 `font/fontinfo.html`(只有 NUM/LANG/NAME 的存在,
  **无页**说明枚举序/选择语义,按 2026-10-24 政策以 REA/引擎证据实现)。

## 1. 官方/游戏侧事实

ERIS 自带完整字体选择器,内核只需把**数据**喂对:

- `es_Font.yst:139` `FONTINFO[LET=@mFontNum NUM=1]` 取总数;`:145-160`
  循环 `FONTINFO[LET=@mFontLang LANG=1 NO=@_LC]`(非 0 跳过)、
  `FONTINFO[LET=$mFontName NAME=1 NO=@_LC]` 建 `$FONT.LIST`;
  `NO` 在脚本里是 **1 基**。
- `es_Font.yst:166-167` `$FONT.LIST(0)=$T(36,11)`;
  `メイン定義.txt:22` `$T(36,11)="ＭＳ ゴシック"`。
- 选择应用只走 `FONT[NAME=$FONT.LIST(...)]`(`es_Font.yst:266/381`),
  从不使用 `FONT[NO]`。
- 提交(`ES_CONFIG_FONT.yst:112-121`)写 `@es.GSD(113,01)`(1 基过滤后
  下标)与 `$es.GSD(113)`(字体名);`es_GSD.yst:536-563` 存 `gsd.sd`。
- 正文初始化 `es_Text.yst:218-220` `FONT[NAME=$es.GSD(113)]`;
  首次为空时 `es_Font.yst:176-181` 尝试用 `$T(36,11)` 播种。

## 2. 旧实现的问题

- `GfxState` 只注入 **1 条**字体(引擎默认 family),游戏选择器永远只有
  一项,无法切换。
- `--font`/Android 导入字体走 `TextComposer::set_force_font`,把所有
  `FONT[NAME]` 短路;游戏内选中其它字体也不会生效。
- `family_names` / `face_by_no` / `find_family` 每次调用全表扫描/排序;
  `default_face` 最多 5 次扫描。

## 3. 现实现(2026-11-16)

1. `FontSource` 构造时一次建 `catalog: Vec<FamilyEntry{name,face,origin}>`、
   `normalize(alias)→face` 索引与默认 family 缓存;`family_names` /
   `face_by_no` / `find_family` / `default_face` 均复用。
2. 宿主默认字体走 `TextComposer::set_default_font`(非 force):
   - 空 `NAME` / 引擎默认 family(含 `MS Gothic` 本地化别名)→ 宿主默认;
   - 其它明确 family 原样解析 → 游戏内切换立即生效;
   - `--font-lock` 保留旧 force 语义。
3. `TextComposer::catalog_cp932`:宿主默认名第一项;目录中引擎默认 family
   的条目改名并去重;其余系统字体照常列出。`FONTINFO[NUM/NAME/LANG]` 直接
   读该缓存。
4. `FONTINFO[NAME]`(无 `NO`):脚本名称为空或属于引擎默认 family 时回报
   宿主默认名;明确其它字体原样回报。
5. 字体切换失效:`set_default_font` 清 faces/ascent/compose/base + 字形
   缓存;`FONT`/`BNO` 变更清布局投影缓存;会话重启
   (`Renderer::reset`)清合成/基面缓存 —— 新 `GfxState` 的文本修订号从
   0 重计,不清会与旧会话同键复用旧字体/旧像素。
6. `FONT[NO]` 与 `FONTINFO[NAME NO=n]` 共用 CP932 过滤后的同一张投影表
   (编不出 CP932 的 system family 不占序号),避免“列表第 n 项”与
   “按 NO 选中”错位。

## 4. 验证

```text
cargo test -p yu-font
cargo test -p yu-gfx --test info_query
cargo test -p yu-gfx --test text_pipeline
cargo test -p yu-render-core --lib
cargo test -p yu-backend-soft --test render font_switch -- --nocapture
cargo test -p yu-backend-wgpu --test wgpu text_
```

- `fontinfo_lists_catalog_and_rewrites_engine_default_name`:
  `NUM`/`NAME NO=1..N`/`LANG=0`,引擎默认名回报宿主默认名,显式其它字体原样。
- `host_default_font_maps_engine_default_and_explicit_switch_wins`:
  `FONT[NAME=ＭＳ ゴシック]` → 宿主 face;`FONT[NAME=其它]` → 其它 face;
  目录第一项为宿主名且引擎默认条目已改名。
- `font_switch_changes_pixels_and_restores_on_switch_back`:A→B→A,
  两次 A 逐字节一致、A/B 像素不同。
- `host_injected_duplicate_family_wins_over_system`:同名宿主注入 face 优先,
  普通 `find_family` 保持系统序。
- 真实 ERIS 启动日志(2026-11-16,`--font "Yu Gothic"`):
  `host font list for FONTINFO: 125 entries (in-game selector enabled)`。

## 5. 残余

- **逐字形缺字回退默认关**:官方 GDI font linking 的候选顺序由注册表
  `SystemLink` 决定,无法 1:1;开启 `font_fallback`(CLI
  `--font-fallback` / Android `run-config.json` 同名键)后,按「宿主注入字体
  → 系统字体」的确定序补字形,回退 face 自己的 advance/bearing 生效。
  未复刻 GDI 的候选顺序;关掉时保持官方「缺字即不画」的行为。
- 游戏硬编码的当前字体名标签(`$T(36,11)`)/默认列表下标仍由脚本与 GSD
  决定;首次运行且 GSD 为空时,列表首项已是宿主字体,但部分游戏可能仍把
  “默认”标签画成旧名。按游戏单独播种 GSD 不在本轮。
- `FONT[NO]` 与 GDI 枚举顺序仍不一致(DIV-FONT-001 继续挂账);本轮只保证
  `FONTINFO[NAME NO=n]` 与 `FONT[NO=n]` 共用本 crate 目录。
- 选中比例字体时脚本硬编码的居中假设仍会偏移(`fullwidth_is_monospace`
  警告),归 DIV-FONT-001/渲染度量。
- 脚本字体名拼写与 fontdb 主名不同(全/半角空格等)时,核心按折叠规则
  映射到宿主默认,但 `FONTINFO[NAME]` 的显示名仍可能是脚本原名(仅显示,
  不影响渲染)。
