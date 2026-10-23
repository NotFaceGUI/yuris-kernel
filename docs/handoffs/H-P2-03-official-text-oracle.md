# H-P2-03 · 官方文本截图对拍(oracle 挂账)

- 状态: **blocked**(YSCom attach 恢复后执行)
- 来源: 任务书 P2 §4 / DIV-GFX-055/056/057
- 影响面: 文本行距/基线、`SET` 重复次数、字体栅格

## 需要的官方观测

1. **自动文本层**:最小脚本
   ```text
   TEXT[OUTPUTMODE=1 AUTORETURN=1 AUTOPAGE=0 AUTOCLICK=0]
   TEXT[WAIT=0]
   WORD("あいうえお")  RETURNCODE
   WAIT[FRAME=60]
   ```
   截图逐行测:行高、基线、`SX/SY` 放大后的墨迹框;
2. **`CGACT[TEXT]`**:同字符 `SET=0` / `SET=1` / `SET=5` / `SET=11`
   各画一次(同色、同位置),对比墨迹质量/厚度,验证
   `1 / 9 / 3` 重复次数的映射;
3. **无文本层**(`TEXT[ID]` 为空且不建层):官方是否显示文本。

## 现在的临时锚点

- P0 场景 `SCENARIO_EPILOGUE`/`RP_101`:soft/wgpu 同帧逐像素一致
  (100 帧 0 mismatch),文本可见(ink > 0);
- 标题 600 帧:soft/wgpu 与改动前基线各 **0 差异**(文本代码对无文本帧
  完全惰性);
- `text_probe` 会把每帧 `display.json`(含 `text_layout.glyphs`/`reveal`)
  与 PNG 一起写到 `target/tmp/`,可直接做对拍输入。

## 做完后要回填

- DIV-GFX-055 的 `SET` 映射从 `Likely` → `Confirmed`(或改实现);
- DIV-GFX-056 的 `line_height`/基线取整规则;
- DIV-GFX-057 的"无图层"行为(保留或删除屏幕覆盖路径);
- `docs/font-contract.md` §8 DIV-FONT-002(swash vs GDI)的 diff 数值。
