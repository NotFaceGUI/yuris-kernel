# DIV-GFX-056 · 自动文本层的排版度量(换行 / 行高 / 基线)

- 状态: **open(Likely)**,P2 实现并按此挂账
- 优先级: 中(自动文本层:非 ERIS 正文,但接受 SCENARIO_EPILOGUE/RP_101 用)
- 发现日期 / 方式: 2026-10-23 / 语料 + REA + P0 场景

## 已确证的部分

| 项 | 证据 | 结论 |
|---|---|---|
| 逐字宽度 | ERIS `es_Text.yst::es.TX.BUFSET.SIZE`:读 `$_TEXT_FLAG` 的 `'1'/'2'` ⇒ `SX/2`/`SX` | **Confirmed**,与 COV-M-03 同源 |
| 字体/字号/颜色 | `FONT` 参数与 REA `0x444428..0x444b1c` 的存储/clamp | **Confirmed** |
| 字形覆盖 | GDI `TextOutA` 8-bit mask(REA `0x4450c0`) | **Confirmed**,本实现用 swash(不宣称字节一致) |
| 字距 | 引擎逐字符 `TextOutA`,无 `kern` 消费 | **Confirmed** |

## 未确证、P2 采用的可替换解释

1. **行高 = `SY`**(`SY=0` → 20),`TextStyle::line_spacing` 可覆盖。
   引擎文本状态 `+0x60/+0x64/+0x68/+0x6c` 的具体行距字段未逐条复核。
2. **换行规则**:`R/r/P/p/E` 都是"换行类"控制字节,但**只在当前行已有字符时**
   才推进一行。理由:P0 实测一行 `_TEXT_FLAG` 是 `'2'*n + C R P E`
   (coverage-matrix §7),若每个标志各换一行会得到 4 倍行距;ERIS 的
   `es.TX.101` 注释也说明 `C/R/P` 是任务码、`E` 是行尾。
3. **基线 = 单元顶边 + 字体 ascent**(`yu-font::FontSource::ascent_px`,
   `hhea/OS2` ascent 按 `SY` 缩放并四舍五入)。GDI 用 `lfHeight` 单元,
   整数舍入细节未取证。
4. **字符串内步进**(`CGACT[TEXT SETSTR=多字符]`)= 字形 advance
   (`1/256 px` 四舍五入)。ERIS 只发单字符,多字符仅调试浮层使用。

## 影响

- 自动文本层(本轮验收):`SCENARIO_EPILOGUE` 6 字/1 行、`RP_101` 10 行
  逐行可见,soft/wgpu 同帧逐像素一致(见 PROGRESS 成果 P2);
- 与官方的行距/基线差最多 1–2 px;没有官方文本截图对拍前不宣称一致
  (H-P2-03)。

## 后续取证

- 官方引擎文本窗口截图(YSCom attach 恢复后):同一文本、同一 `SX/SY`,
  逐行拟合 `line_height` 与基线;若偏差固定,改 `line_height()`/ascent
  取整规则即可,不影响 gfx/后端契约。
