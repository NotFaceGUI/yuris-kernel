# DIV-GFX-061 · 动画立绘/背景不上屏:`CGACT[TRANSITION]` 与固定点尺寸键

- 状态:**fixed(no-manual)** —— 2026-10-25 实装,两条回归测试 + 像素单测在册
- 取证对象:官方 `0.495` `yu-ris.exe`(sha256 `4f5eefa3…`)、语料
  `data/script/ERIS/Sprite/es_Sprite.yst`(`<project-eris>` 官方全量编译)
- 现象(用户口径):**有动画的立绘/背景不显示,静止的 CG 正常显示**;
  同时出现 render spike

## 1. 结论

症状不是「CG 没解析」,而是**动画分支独有的两条 `CGACT` 写法没被实现**:

| # | 写法 | 出现处 | 旧行为 | 后果 |
|---|---|---|---|---|
| A | `CGACT[TRANSITION=1 SET=f ID=$LB1 ID2=$LB2 ID3=$LB3]` | `es_Sprite.yst:7978` | 归 `ImageAction::Other("transition")` ⇒ `Unimplemented` | 输出层 `$LB3` 永远全透明 ⇒ 后续 `SIZE` 拿到空图 ⇒ 立绘/背景消失 |
| B | `CGACT[ID=src ID2=dst SIZE2=1 FX=… FY=… FX2=… FY2=… FSX=… FSY=…]` | `es_Sprite.yst:8665/8690`(中画质分支) | `SX/SY` 缺失 ⇒ `gfx.imageop.size.missing` ⇒ 图层被标 `Unimplemented` | 同上,且逐帧累积 skip

**静止 CG 不走这两条分支**(`es.SP.DRAW` 里 `IF[@L.ST(@LNO)!=0]` 才发 TRANSITION;
`quo==0` 的低画质分支才发整数 `SX/SY`),所以静止图正常 —— 与用户观察一致。

## 2. 证据

### 2.1 `TRANSITION` 的三源角色

语料唯一用法(`es_Sprite.yst:7978`,动画立绘/背景逐帧采样):

```text
CGACT[TRANSITION=1 SET=@L.F2(@LNO) ID=$LB1 ID2=$LB2 ID3=$LB3]
```

同段(`:7915..7985`)里 `$LB1=$LIDBUF+…`(上一帧采样)、
`$LB2=$LIDBUF2+…`(目标图)、`$LB3=$LIDBUF3+…`(本帧采样源);
产物紧接着被 `es.CGACT_SIZE`(`:7980/8005`)缩放到可见层。

⇒ **`ID2` 是第二输入,`ID3` 是输出** —— 与 `COPY/SWAP` 的「`ID2`=目标」相反。
`SET` 与 `A` 同量纲(0..256):同段 `@A` 走 `_range(@A, 0, 256)`。

手册 `cg/cgact.html` 只有一行「画像Ａから画像Ｂへの遷移途中画像を生成する」,
**没有 `SET` 量纲、没有 `ID3` 说明、没有示例** ⇒ 按 §0 政策 `fixed(no-manual)`。

实现:`out = from*(256-f)/256 + to*f/256`,逐通道含 alpha;画布取新图尺寸,
旧图越界按透明。

### 2.2 `FX/FY/FX2/FY2/FSX/FSY` 是整数键的百分位定点

`es_Sprite.yst:8665`(中画质,`quo!=0`):

```text
CGACT[ID=$src ID2=$dst SIZE2=1 FX=@X1*100+@FLX FY=@Y1*100+@FLY
       FX2=@X2*100+@FLX FY2=@Y2*100+@FLY FSX=@SIZE_X*100 FSY=@SIZE_Y*100]
```

同函数里的注释原文:

> 表示位置 @LEFT_X,@LEFT_Y の小数点値を掃い、それを CGACT[SIZE2=1 ～] に伝える

即「把小数折进百分位再传给 CGACT」。`@FLX` 的构造(`:8656..8660`)是
`@FLX = @LEFT_X*100` 后取百分位余数并取负 —— 纯粹是定点化,没有别的语义。

绘制侧的读法已在本仓 `docs/render-parity.md` §7 锚定:`+0x1b0/1b8/1c0/1c8/1d0/1d8`
按 `÷100.0` 读,`+0x1f8/1fc/200` 的 f32 同样 `÷100.0`。

⇒ `FX→X`、`FY→Y`、`FX2→X2`、`FY2→Y2`(÷100 后取整)、`FSX→SX`、`FSY→SY`(÷100)。
手册 `cg/cgact.html` 无 `FX/FSX` 页 ⇒ `fixed(no-manual)`。

### 2.3 `SIZE3/SIZE4/SIZE5`

语料用法与 `SIZE` 完全同形(均 `SX/SY`,可带区域):

- `es_ERIS.yst:631` `CGACT[ID=$id SIZE4=1 SX=@sx/2 SY=@sy/2]`(CG 半尺寸)
- `es_System.yst:261` `CGACT[ID=$SRCID ID2=$DSTID SIZE4=1 SX=@SSX SY=@SSY]`
  (二分降采样,与 `SIZE2` 交替出现在同一循环)

手册无页;几何语义有语料锚点,**重采样滤波相对 `SIZE2` 的差异未取证** ⇒
按双线性执行,DIV-GFX-039 由 `unimplemented` 降为 `partial(no-manual)`。
关键点:不实现会让整层被标 `Unimplemented`,而 `SIZE4` 就用在 CG 缩放路径上。

## 3. 顺带修掉的两条放大链路

### 3.1 `image_log` 逐帧累积(render spike 的直接原因)

`frame()` 为 `image_log`/`copy_log` 的**每一条**产出一个 texture pass。
缩放动画每帧追加一条同 `(源, 目标)` 的 `SIZE`,于是:

```text
实测(passes):103 → 139(逐帧 +1),skip 累计 336
实测(image_log):66 条,其中 50 条是同一层的重复 SIZE
```

引擎的 `CGACT` 是**立即执行的破坏性写入**(REA `0x42f809`:分配新缓冲 →
从层当前纹理读 → 写回层),旧版本像素不会再被看到。故按下列条件折叠
(`GfxState::supersede_image_op`):

- 只折叠**缩放族**(`SIZE/SIZE2/SIZE3/SIZE4/SIZE5`)—— 后端对它们只读源层;
- `RECTPAINT`(RGB 填充保留目标 alpha)、`REVERSE/HORIZONTAL`(变换目标自身)
  都以目标旧像素为输入,**一律不折叠**;
- 源 = 目标(原地缩放会叠加)不折叠;
- 旧项之后有操作把该目标当源读过(含 COPY 源)⇒ 历史仍被引用,不折叠。

折叠后:`image_log` 66 → 15,`passes` 稳定 84–88;像素 SHA 与折叠前**逐位相同**
(`menu2g` / `menu2g_nocollapse` 两次 A/B,8 帧采样全等)。

### 3.2 `VARACT` 缺 `LET` 的 Fatal

桌面实测 FATAL:`rt.cmd.slot at es_text.yst:SYSTEM@1354 VARACT`(进入演出样例后
`es_Text` 任务被杀,此后输入不再推进)。

语料形态(`es_Text.yst:2024/2040/2568/2586`):

```text
VARACT[CUT=1 SET=$TX POS=1 LENGTH=@lenA]
VARACT[CUT=1 SET=$TX.TASK POS=@mPos LENGTH=@lenA]
```

**没有 `LET`**。侧线已取证:引擎 `0x4542ab` 按 `[0x684161]`(LET 在场)分两支,
**两支都调 `0x43f0d8`** 写结果;缺省时回写 `SET` 指向的变量。
手册 `variable/varact.html` 例 5/6 只示范带 `LET` 的形态 ⇒ `fixed(no-manual)`。

## 4. 回归

| 测试 | 覆盖 |
|---|---|
| `yu-gfx/tests/parity_fixes.rs::cgact_size_accepts_fixed_point_fx_fsx_keys` | `FX/FY/FX2/FY2/FSX/FSY` ÷100 解析 + 可执行 |
| `…::cgact_size4_executes_as_scale` | `SIZE4` 按缩放执行,不再毒化层 |
| `…::cgact_transition_blends_two_sources_into_id3` | `TRANSITION` 的 `ID2`=源2 / `ID3`=目标 |
| `…::repeated_scale_ops_collapse_but_in_place_ops_stay` | 折叠规则 + 原地不折叠 |
| `yu-backend-soft/src/imageop.rs::blend_tests::*` | 插值端点/中点、画布取新图尺寸、越界透明 |
| `yu-rt/tests/fixture.rs::varact_cut_without_let_writes_back_to_set` | `LET` 缺省回写 `SET` |

## 5. 仍未闭环

- `SIZE3/5` 与 `SIZE2` 的**滤波差异**(`partial(no-manual)`,按双线性代用);
- `TRANSITION` 的 `SET` 在动画端取值范围实测确认(当前按 0..256 夹紧);
- `CGINFO[TEX]`(`es_Sprite.yst:3454/4155/5848/7770` 的 E-mote 判定)仍未实现,
  属 E-mote 家族,维持 `deferred(master)`。
