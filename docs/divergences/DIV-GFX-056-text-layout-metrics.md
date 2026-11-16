# DIV-GFX-056 · 自动文本层的排版度量(换行 / 行高 / 基线)

- 状态: **open(Likely;折行/行高已 fixed 2026-11-16;基线与 GDI advance 仍 Likely)**
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
2. **换行规则(2026-11-04 勘误)**:旧口径把 `R/r/P/p/E` 都当“换行类”
   且“只在当前行已有字符时才推进”，是不完整的近似。REA
   `FUN_00404914` 逐标志处理:
   - `'R'/'r'`:`+0x14=0`、`+0x18 += +0x44[+0x6c]`(该标志行高);
   - `'P'/'p'`:清图层表面(`zero_fill(surface, w*4*h)`)并 `+0x14/+0x18=0`;
   - `'E'`:仅推进游标,无排版副作用;
   - `'C'`:置 `+0x35=1` 并**停止**,等点击/跳过。
   所以一条 `…C R P E` 不是 4 倍行距:`C` 先停,点击后 `R` 换行、`P`
   清页(默认 `AUTOPAGE=1` 时一行一页)。gfx `layout_text` 仍保留“有字
   才换行”的近似(把 `P` 当换行、不清屏),差异登记为
   `DIV-GFX-075`;不再据此宣称官方行距已对齐。
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


## 2026-11-16 更新(text-parity)

- **折行已按官方实现**:`FUN_00404c60` 的分支与禁则表已逐指令复核并落地
  (`docs/tasks/text-oracle-2026-11-16.md` §2/§3)。适配判断的每格宽
  **恒为 `SX`**(`WORD 0x458e26..0x458e3e` 把 `+0x20` 无条件写进 `+0x40`,
  半角也不折半);`x` 的实际步进来自 `FUN_004056b8` 的 GDI advance,
  内核按 `全角=SX、半角=SX/2` 建模(仍 `Likely`,归 DIV-FONT-002)。
- **行高已确认**:`'R'` 用 `+0x44[+0x6c]`(追加时 `SY`)推进 `+0x18`;
  `SY=0` 时按引擎默认 20。没有额外行距。
- **基线仍 `Likely`**:`FUN_004056b8` 以单元顶 `+0x18` + 字体 ascent 绘制;
  `yu-font` 的 ascent 取整与 GDI 的差未对拍(需官方截图 oracle)。
- 仍待 oracle:逐字 advance 差导致的行尾折行点、行内 `FONT[SX/SY]` 变化后的
  基线。采样方法见 `text-oracle-2026-11-16.md` §6(最小工程 + 屏幕截图)。
