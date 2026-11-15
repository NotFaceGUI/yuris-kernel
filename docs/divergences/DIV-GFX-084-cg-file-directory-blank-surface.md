# DIV-GFX-084 · `CG[FILE=<目录>]` 必须建空白表面,不是资产

- 状态: **fixed**
- 优先级: 高(本篇对话正文与按钮文字全灭的根因)
- 发现日期 / 方式: 2026-11-14 / v474 商业样本实机 + 组件侧 `YU_TEXT_TRACE`

## 1. 现象

进本篇后**所有 `CGACT[TEXT]` 文本不可见**(对话正文、按钮标签),换字体无效。
VM 侧正常:`WORD=1 _=1 RETURNCODE=1` 实测出现,`CGACT` 帧内数十次。

## 2. 证据链(逐条可复核)

1. `[text-trace]`(`YU_TEXT_TRACE=1`)显示:
   `draws=26 rep(draws=0 glyphs=0 missing=0) diags="text.layer_base: asset.missing"`。
   → 绘制项已经登记进 `CompositorFrame.text_draws`,但**后端一条都没栅格化**。
2. 该 trace 把每个 draw 的目标层与基图种类一并打出:
   `tgt=:/W0/BT.OFFES.GAMEMAIN.TIP.MESW.TX=0=1 size=771x95
    tex=Asset("cgsys/../system/window/")`。
   → 基图键是**一个目录**(以 `/` 结尾,没有文件名)。
3. 包内 `pac/system.ypf` 的 `system/window/` 下只有
   `dialogbg.png`/`windowbg.png`/`select.png`/`windowbotan.png`/
   `message_select.png`/`key1.png` —— 没有任何东西能解析成目录键。
4. 该键**不以字面量出现在任何脚本**里(全 `ysbin/yst*.ybn` grep 无命中),
   由运行时拼接:脚本 `CG[FILE = $prefix[$n] + $_RSTR(2)]`
   (`es_button.yst` s9 pc1744 等)。`$prefix` 系列由配置表读入,
   值可以是目录前缀(实测另一样本出现 `../system/config/fader`)。
5. 我们的后端把 `TextureRef::Asset(key)` 一律交给资产解码
   (`soft::layer_base` → `textures.resolve`;wgpu `resolve_source`
   → `resolve_ref`),目录键必然 `asset.missing` ⇒ `layer_base` 失败
   ⇒ **整层文本被 `return` 丢弃**(soft `render.rs` / wgpu `plan.rs`
   的 `text.layer_base: {reason}` 分支)。

## 3. 官方语义

手册 `cg/cg.html` 只规定 `FILE=""` 生成 1×1 黑图(DIV-GFX-060);
对"路径不指向任何发行文件"的情形,官方 `CG` 处理体把装载失败当作
**该层没有图片**,但**层对象本身照样存在,表面尺寸取 `SX/SY`**
(DIV-GFX-080 的"官方静默"是同一机制的另一面:缺资产不报错、不中止)。

因此:
- `FILE` 给空串 ⇒ `SX/SY` 尺寸的实色表面(`Layer.fill`/1×1 铺开);
- `FILE` 给不可解析路径 ⇒ **同一尺寸的空白(全透明)表面**;
- 两种都不改变"这一层可以被 `CGACT[TEXT]` 写入"的事实。

我们此前把"层表面"与"资产解码成功"绑定,是本条的实现错误。

## 4. 修法

`layer_base` / `layer_base_dims`(soft)与 wgpu 的文本基面解析:
当 `TextureRef::Asset(_)` 解码失败、且该层有显式 `SX/SY`(`Layer::image_size`
返回正尺寸)时,**退化为同尺寸全透明表面**,并保留原诊断
(`text.layer_base.missing-asset-fallback`)以便继续统计缺资产。

不改变:
- 资产查找逻辑(仍按原始字节查,查不到就是查不到);
- 非空 `FILE` 且能解析时的行为(照常用图片像素做基面);
- 层是否存在、`E`/`A`/`Z` 等属性语义;
- `RenderReport.missing_assets` 的计数口径。

## 5. 回归

- `crates/yu-render/soft/tests/render.rs`:新增
  `text_layer_with_unresolvable_asset_still_draws_glyphs`
  —— 层 `TextureRef::Asset("cgsys/../system/window/")` + `SX/SY`
  + 一条 `CGACT[TEXT]`,`assert text_glyphs > 0`。
- `crates/yu-render/wgpu/tests/`:同形用例,并保持
  soft/wgpu `text_glyphs` 逐帧相等。
- 实机:v474 样本进本篇,对话正文应出现字形
  (`text-trace` 的 `rep(glyphs=…)` 由 0 变为正数)。

## 6. 复核入口

```bash
YU_TEXT_TRACE=1 yu-cli run "<game>" --mode desktop --log-file trace.jsonl
# 期望:diags 不再出现 text.layer_base: asset.missing 导致的 draws>0/glyphs=0
cargo test -p yu-render-soft --test render text_layer_with_unresolvable_asset
cargo test -p yu-render-wgpu --test wgpu text_
```
