# DIV-GFX-076 · `CGACT[TEXT]` 与图像加工的命令序(外字链根因)

- 状态: **open(已定位;修复需主人拍板架构)**
- 影响面: `crates/yu-gfx/src/state.rs`、`crates/yu-render/{core,soft,wgpu}`、
  ERIS 外字链(`es_font.yst` s11)、`BT.CGCLEAR` 标签清屏
- 关联: `DIV-GFX-072` §9、`DIV-GFX-055`、`DIV-GFX-058`
- 证据基线: 官方 0.495 `yu-ris.exe` sha256 `4f5eefa3…`(Ghidra 12.1.4)、
  手册 `cg/cgact.html`、`<project-eris>` s11/s135

## 1. 官方语义(命令时刻写表面)

- `CGACT` 的动作分派 `FUN_0042f809`(`0x42f809`)一进来就取
  `iVar2 = [in_EDX + 0x34]`(目标层对象)、`iVar9 = [iVar2 + 0x90]`
  (目标层**像素表面**),在原表面上执行动作,成功后
  `[iVar2 + 0x90] = local_38` 换新表面、`[+0x78/+0x7c]` 更新尺寸。
  证据: 伪代码 `undefined4 FUN_0042f809(void)` 的
  `iVar2/iVar9/local_38` 段;动作码 `0x01..0x1d` 的分派。
- `CGACT[TEXT]` 走同一层表面: `FUN_004056b8`(`0x4056b8`)按字符取
  8-bit mask 后合成到层表面(`0x404a1b..0x404ac7` 读 AA/DRAWMODE;
  手册 `cg/cgact.html`「元のイメージには戻せません」)。
- 因此官方状态里**没有“文字绘制项列表”**:文字像素在命令返回时已经在
  `[+0x90]`;后续 `MERGE2/SIZE2/MEIDO/RECTPAINT*` 读写的都是**含文字
  的表面**。脚本命令序 = 像素序。

ERIS 外字链(s11 `es_font.yst`,s135 文本):
`CGACT[TEXT]` 画前缀 → `CG[FILE=g01_48]` → `CGACT[SIZE2]` →
`CGACT[MEIDO]` → 多次 `CGACT[MERGE2]` 把外字图叠进文本层 → 继续
`CGACT[TEXT]` 画后缀。官方每一步都改同一层表面。

## 2. 内核现状(近似模型)

- `CGACT[TEXT]` 只记录 `TextDraw { target, x, y, bytes, style, set }` 到
  `text_draws`;后端在**层绘制时**把整层文字合成到基图副本(`apply_layer_text`)。
- `cgact_image` 对任何可执行图像加工都 `clear_layer_texts(target)`——
  用“清文字”避免延迟文字出现在图像加工之后。
- 这是**保守近似**:它保证不会把旧文字错误叠在新图像上,但会丢掉
  “先文字、后图像加工”的官方像素。
- 2026-11-04 实机:外字页第一行从 `」` 开始,`」` 之前的 `CGACT[TEXT]`
  前缀被后续 `MERGE2` 的 `clear_layer_texts` 清掉;外字图也可能因同样
  原因/源链状态不可见。

## 3. 为什么不能按动作类型开例外

“`MERGE2` 不清、其他清”是补丁,语义不闭合。若不清理,以下已取证用法
会坏:

| 用法 | 官方 | 不清理的后果 |
|---|---|---|
| `RECTPAINT3 SET=0`(`BT.CGCLEAR`,`es_Button.yst:4547`) | 把标签面填透明/黑,旧文字被覆盖 | 旧标签文字残留(鬼影) |
| `COPY`/`COPY2` 整面覆写目标层 | 目标层像素整面替换 | 旧文字残留 |
| `SIZE/RESIZE/TRIM/ROTATION` 作用于文本层 | 文字随表面一起缩放/旋转 | 文字停在旧坐标,错位 |
| `MEIDO/BLUR*/MONO/...` 作用于文本层 | 文字像素一起被过滤/着色 | 文字不参与滤镜,颜色/模糊不一致 |
| `MERGE/MERGE2` 上层覆盖文字 | 上层像素盖住下层文字(含重叠区) | 文字被后端画在上层图像之上,重叠区顺序反了 |
| `CG[FILE]` 重建 / `CGEND` 删除层 | 表面重建/释放 | 文字挂到新层或已删层 |

结论: **动作类型例外不成立**。正确语义只有一条:文字必须在命令时刻
成为层表面的一部分,后续图像加工按脚本顺序作用于它。

## 4. 修复选项(需主人拍板)

### A(推荐)· 命令时刻烘焙文字

- `TextDraw` 增加 `seq`(命令序)。
- `GfxState` 在图像/复制加工落到“有未烘焙文字”的层之前,向宿主发
  **bake 请求**:把该层 `text_draws` 用 `TextComposer` 栅格化进层基图/
  override 表面;成功后清空 `text_draws`,再执行图像加工。
- 宿主端口由 `yu-engine` 注入(渲染器持有 `TextComposer`);gfx 不引入
  字体依赖。
- 与官方一一对应:文字成为表面像素,后续动作读同一表面。
- 代价: gfx↔renderer 新端口(公共契约)、`TextDraw.seq`、烘焙缓存失效
  规则;需要主人批准契约。

### B · 后端按 `seq` 重放

- `TextDraw` 增加 `seq`;后端按层把 text/image/copy 操作按 `seq` 排序,
  逐项写入层表面。
- soft 可做;wgpu 图像加工是 GPU pass,文字是 CPU override,混序需要
  render-target 往返/CPU 镜像,复杂度高。
- 不动 gfx 状态,但 backend plan 要大改。

### C · 保持现状(保守清文字)

- 不外字显示;前缀丢失;DIV-GFX-072 §9 保持 open。
- 只在“外字必须显示”不是验收项时可接受;当前主人已指出必须修。

## 5. 验收(实现 A/B 后)

- 合成微场景: `CGACT[TEXT]` 在 (10,10) 画 `A` → `CGACT[MERGE2]` 在
  (20,10) 叠图 → `CGACT[TEXT]` 在 (40,10) 画 `B`;三者都必须可见,
  且 `RECTPAINT3 SET=0` 覆盖 (0,0)-(31,31) 时 `A` 被清掉。
- 真实语料: ERIS s135 外字页,截图对照:
  行首必须是 `あ、ちなみにこの「` + 外字 `＃`/`＠` + `」などの…`;
  不是从 `」` 开始。
- 现有 `text_pipeline` / soft / wgpu 文本测试不回归。

## 6. 当前动作

- 已撤销“MERGE2 例外”补丁,`cgact_image` 恢复保守清文字。
- 2026-11-04 曾按方案 A 接线(gfx 烘焙队列 + Engine 逐 tick 执行 +
  soft/wgpu `bake_text_layer`),**已整体回退**:实机主文本层的绘制项位于
  `99999+4/5` 隐藏哨兵坐标且层基图常为 `None`;烘焙把待合成文字移出
  `text_draws` 后没有产出可见像素,导致整块文本框空白。回退后 desktop
  wgpu 文本恢复(截图「それではまず、背景を拡大してみます。」)。
- 临时 `YURIS_TRACE_TEXT_OPS=1` 诊断钩子已删除(不属于行为改动)。
- 等主人选 A/B/C;选 A 需先拍板 gfx↔renderer bake 端口契约,并解决
  “隐藏哨兵坐标/空基图层”这一类真实文本层的处理,不能复用本次失败实现。
