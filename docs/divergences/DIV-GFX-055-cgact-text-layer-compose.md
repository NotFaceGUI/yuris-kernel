# DIV-GFX-055 · `CGACT[TEXT]` 的图层合成模型与 `SET` 重复次数

- 状态: **fixed(P2,2026-10-23)**,`SET` 映射 `Likely`
- 优先级: 高(ERIS 本篇对话文本的**真实绘制路径**)
- 发现日期 / 方式: 2026-10-23 / 语料 + REA

## 现象

进本篇后对话文本不可见。VM 侧文本状态正常(`_TEXT`/`_TEXT_FLAG` 有数据),
但 ERIS 的正文根本不走 `TEXT[ID]` 自动文本层,而是:

```text
FONT[SX=12 SY=12 ...]                     // es_Text.yst es.TX.BUFSET.SIZE
...
\_strmid($_TEXT_FLAG, @TX.SizeReadPos, 1)($one)
  IF[$one=="1"] @TX.BUF.SX(...) = @TX.SX/2   // 半角
  ELSE[$one=="2"] @TX.BUF.SX(...) = @TX.SX  // 全角
...
\FONT.SIZE(@TX.BUF.SB(@TX.POS), @TX.BUF.SY(@TX.POS))
\FONT.XY((4)+@TX.BUF.X(@TX.POS), (4)+@TX.BUF.Y(@TX.POS))
\BT.FONT.DRAW("ES.GAMEMAIN.TIP.MESW.TX",, $txone)
```

`es_Font.yst::es.FONT.DRAW.SET` 对每个字符发:

```text
CGACT[ID=$ID TEXT=1 SET=1 SETSTR=$TX X=.. Y=..]   // 轮廓色
CGACT[ID=$ID TEXT=1 SET=0 SETSTR=$TX X=.. Y=..]   // 填充色
```

即 `CGACT[TEXT=1]` 是正文路径;旧实现把它当“图像加工未实现”丢掉(DIV-GFX-013
的 `imageop.unsupported`)。

## 证据

| 证据 | 内容 | 等级 |
|---|---|---|
| 手册 `cg/cgact.html` | `TEXT = 値(0/1) レイヤに文字列を表示する`;例 25/26:`CGACT[ID=BG TEXT=1 SETSTR=$A X=30 Y=50]` 在图层左上 `(X,Y)` 绘制字符串 | Confirmed |
| 语料 `ERIS/Text/es_Text.yst` | 每字符按 `_TEXT_FLAG` 的 `'1'/'2'` 算 `SX/2`/`SX` 后逐字绘制(COV-M-03 的消费者) | Confirmed |
| 语料 `ERIS/es_Font.yst` | 轮廓 `SET=1`、填充 `SET=0`、影 `SET=0`;`FONT[DRAWMODE=1]` 包住整段绘制 | Confirmed |
| 语料 `ERIS/es_System.yst:311` | 同一位置先黑 `SET=1` 再白 `SET=0` ⇒ `SET=1` 必然产生“更粗/轮廓”效果,不是颜色 | Confirmed |
| REA `FUN_004056b8` @ `0x4056b8` | 取单字 mask 后按 ANTIALIASING 选 AA/非 AA 合成器;一个栈参数决定重复合成 **1 次(0)/ 9 次(1..10)/ 3 次(>=11)** | Confirmed |
| REA `FUN_004450c0`/`FUN_00445421` | 逐字符 GDI `TextOutA` → 8-bit coverage;无 kerning | Confirmed(font-provenance §3.3) |
| 映射 `SET → 重复次数` | ERIS 只发 `SET=0/1`,与 `FUN_004056b8` 的 0/9 分支一一对应 | **Likely**(无官方截图对拍,H-P2-03) |

## 实现

- `yu-gfx` 把 `CGACT[TEXT]` 记录为 `TextDraw { target, bytes, x, y, style, set, bno }`
  (`frame.text_draws`),**不**进入 `ImageOp`/`op` 计数;
- 绘制项按帧重建(ERIS 每帧重发全部字符);`CGEND`/重新 `CG(FILE)` 清空;
- `TextDraw::composite_passes()` = `0 → 1`、`1..=10 → 9`、`>=11 → 3`;
- 后端把文本合成到**图层基图副本**上,再按图层 `A/MODE/SX/SY` 走常规合成
  (soft `draw_layer` 的 override 分支;wgpu `overrides` + quad)。

## 等价性论证(为何"每帧重建"不改变可见结果)

- 引擎的命令处理器立即把字写进图层像素(手册「元のイメージには戻せません」);
- ERIS 每帧从同一基图重画全部可见字符(`es.TX.100` 循环),且文本图层是普通
  `CG` 图像层(`UserDesign/jp_0040.txt`:`TIP.MESW.TX`,Z=012);
- 因此"每帧把这一帧的绘制项叠加到基图"与引擎"在原图像素上叠加"在**每帧
  重画相同内容**的前提下逐像素等价;
- 如果将来出现"只画一次、依赖跨帧保留"的脚本,会与引擎分歧 ⇒ 若观察到
  该用法,改为跨帧累积并加去重/上限(挂账,H-P2-03)。

## 回归

- `crates/yu-gfx/tests/text_pipeline.rs::cgact_text_becomes_a_layer_draw`
  (SET=1 → 9 次;style 快照;每帧清空);
- `crates/yu-gfx/tests/text_pipeline.rs::cgact_text_does_not_enter_image_ops`;
- `crates/yu-backend-soft/tests/render.rs::cgact_text_draws_into_layer`
  (绿字画进黑色图层,`text_draws=1/text_glyphs=1`);
- ERIS `es_Text.yst` 的逐字坐标由脚本计算,后端不再自造布局。
