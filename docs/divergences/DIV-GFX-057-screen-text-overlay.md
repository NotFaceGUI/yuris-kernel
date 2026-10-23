# DIV-GFX-057 · 没有文本图层时的自动文本叠加

- 状态: **open(Likely)**,P2 实现
- 优先级: 中(仅当 `TEXT[ID]` 为空/目标层不存在时触发)
- 发现日期 / 方式: 2026-10-23 / P0 场景 boot

## 现象

`SCENARIO_EPILOGUE`/`RP_101` 直接 boot 到文本标签时,没有创建 ERIS 的
`ES.GAMEMAIN.TIP.MESW.TX` 文本层(那个层由 GAMEMAIN 场景创建)。
`TEXT[ID]` 未设置,自动文本层的排版结果没有承载图层。

## 证据与解释

- 引擎自动文本渲染器 `FUN_004046fc` 只在 `TEXT[OUTPUTMODE] == 1` 时绘制,
  逐字推进后调用 `FUN_00404914`;目标层/坐标来自文本状态(光标
  `+0x14/+0x18`)与 `TEXT[ID]`。
- 手册 `text/text.html`:`ID = テキスト自動出力モード時の出力するレイヤＩＤ`
  ⇒ 文本**画进一个图层**,不是独立窗口。目标层不存在时引擎行为未取证
  (ERIS 正文根本不走这条路径)。
- P2 的决定:目标层存在 ⇒ 文本作为该层的绘制项(与 `CGACT[TEXT]` 同一
  管线);目标层不存在 ⇒ 画成**屏幕后置覆盖层**,强制 src-over 合成,
  保证"文本可见"且不破坏背景。

## 为什么不能直接 Replace

`FONT[DRAWMODE=0]`(引擎默认)会把字形框内的透明像素也写下去;画进图层时
这是正确的(图层另有基图),但直接盖在屏幕上会挖出透明洞。因此屏幕覆盖层
固定用合成模式(`paint_layout_with(.., Some(true))`),并在文档标注
`Likely`。

## 回归

- `crates/yu-render/wgpu/tests/wgpu.rs::text_layout_frames_match_soft_exactly`
  (无图层、64×32、`AこB`):soft/wgpu RGBA 逐字节一致,3 glyph;
- `crates/yu-render/soft/tests/render.rs::synced_text_flags_rasterize_glyphs`
  (可见性 + 清空同步);
- 示例 `text_probe` / `wgpu_text_probe`(真实语料,见 PROGRESS)。

## 后续取证

- 官方引擎在"无文本层"时的行为(是否丢字/画到别处)没有证据;
- 若 YSCom attach 恢复,用最小脚本 `TEXT[OUTPUTMODE=1] + WORD`(不建层)
  对拍;若官方不显示,则本路径只在 `Reference` 模式保留。
