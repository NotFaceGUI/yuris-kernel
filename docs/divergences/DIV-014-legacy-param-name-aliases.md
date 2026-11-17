# DIV-014 · 2xx 参数名兼容:`CG/CGEND/CGINFO/CGACT` 的 `NO` 与 `FONT/FONTINFO` 的 `SIZE`

- 状态: **fixed**(2026-11-17;按工程 ysc 表逐命令裁决)
- 优先级: 高(2xx 按钮/标题/文字)
- 发现日期 / 发现方式: 2026-11-17,官方 0.292 / 0.255 `ysc.ybn` 与 0.495
  `ysc.ybn` 逐项对照;0.292 标题按钮消失现象定位。

## 事实

官方 0.292 `ysc.ybn` 与 0.495 的同名命令参数表逐项对照:

| 命令 | 0.292 | 0.495 | 语义 |
|---|---|---|---|
| `CG` | `ID, NO, E, …, TID, TNO, TA` | `ID, IDNO, …, TID, TIDNO, TA` | `NO` = 图层号十进制后缀;`TNO` = 过渡源图层号 |
| `CGEND` | `ID, NO` | `ID, IDNO` | 同上 |
| `CGINFO` | `ID, NO, …` | `ID, IDNO, …` | 同上 |
| `CGACT` | `ID, NO, ID2, NO2, ID3, NO3` | `ID, IDNO, ID2, IDNO2, …` | 同上 |
| `FONT` | `NO, NAME, …, SIZE, BOLD` | `BNO, NO, …, SX, SY, BOLD` | `SIZE` = 单值方形字号(0.495 拆成 SX/SY) |
| `FONTINFO` | `… SIZE …` | `… SX, SY …` | 同上 |

后果(修复前):`CG[ID=BTN.OFF NO=n]` 的 `NO` 被当成未分类参数,gfx 把
所有按钮写进同一个 `:/W0/BTN.OFF` 层,最后一个覆盖前面全部 ⇒ 0.292 标题
只剩一个按钮。修复后标题按钮/背景全部出现。

## 实现

- `yu-gfx::state::alias_legacy_layer_names`:当表里**没有** `IDNO` 时,
  把 `CG/CGACT/CGEND` 的 `NO/NO2/NO3` 改名为 `IDNO/IDNO2/IDNO3`、
  `TNO` 改名为 `TIDNO`;有 `IDNO` 的表不动(不猜未来混合表)。
- `query_cginfo` 用 `idno_alias`(CGINFO 表有 `NO` 无 `IDNO`)读取 `NO`。
- `yu-rt` 的 `CGINFO[SX/SY]` 资产尺寸回退:找 `IDNO` 槽时回退到 `NO`,
  使 `CGINFO[ID=… NO=n MFILE=1]` 能取到正确图层。
- `FONT`:`SIZE` 同时写 `size_x/size_y`;`FONTINFO[SIZE]` 读回 `size_y`。
  依据 0.292 样本 `\FONT.SIZE(n)` 的存取对(`FONTINFO[SIZE=1 LET=@s]` /
  `FONT[SIZE=@_PINT(1)]`)与 0.495 的 SX/SY 手册页。

## 验证

- 0.292 headless 600 帧:标题背景 `cg/title/back` + 开始/读取/选项/结束
  四个按钮全部绘制(修复前只有最后一个"结束");
- `gfx.param.nonvisual`(CG `NO`)与 `FONTINFO[SIZE]` Unimplemented 消失;
- 0.495 项目 600 帧截图与修复前逐字节一致(参数名不冲突)。
