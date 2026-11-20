# DIV-GFX-086 · `CGACT[MERGE/MERGE2]` 的 `X/Y` 是落点:忽略它会把立绘差分贴到画布 `(0,0)`

- 状态:**fixed(no-manual)**(2026-11-15)
- 取证对象:v474 商业样本 `<0.474 旧运行时样本>`(YU-RIS 0.474)实机逐帧捕获;
  SDK 0.479 `マニュアル/YU-RIS/html/cg/cgact.html`
- 相关:DIV-GFX-063(图像加工族手工页与 alpha 口径)、
  DIV-GFX-076(`CGACT[TEXT]` 与图像加工的命令序)

## 现象

推进剧情、角色开口时,画面**左上角**出现一块约 41×41 的**嘴部碎片**
(微笑的嘴 + 牙齿 + 少许头发/衣领),不随立绘移动,也不属于背景。

捕获帧(1280×720,`debug-ui` 单帧):

```text
帧 (0,0)-(40,40) 与 obj/rio/rio_n_kuti1.png 顶部 41 行逐像素 MAE = 0.00
帧第 41 行以下与背景 bg1a.png 逐像素 MAE = 0.02(即完全等于背景)
⇒ 碎片 = 差分图的第一帧被原尺寸画在屏幕 (0,0)
```

## 证据链

### 1. 差分图与部件坐标(数据自证)

`obj/rio/rio_n_kuti1.png` 是 **41×123 = 3 帧 × 41×41** 的口型动画条。
`yst00162`(立绘定义表)的 `es.SPA.ADV.XY2.DEF.SET` 参数:

```text
obj/rio/rio_n_kuti1  帧宽=41 帧高=41 ... X=635 Y=309
```

即该部件应贴在立绘画布的 `(635,309)`。把第一帧贴到该坐标后,嘴部与
`rio_n_1_1.png`(基础立绘)完全对齐。

### 2. 脚本命令链(`es_SPExec.yst`,0.474 语料 s35)

```text
pc110  CG      [ID=sp.tmp  FILE=<差分条>]
pc111  CGACT   [ID=sp.tmp  SIZE=1 SX/ SY=<帧尺寸> X/Y/X2/Y2=<帧在条带中的区域>]
pc115  CGACT   [ID=sp.tmp  ID2=<立绘缓冲> X=635 Y=309 MERGE=1]
```

`SIZE` 把条带裁成 41×41 的 `sp.tmp`;随后 `MERGE` 的 `X/Y` 就是**落点**。
同一模式在 `es_spexec` 中有 5 处(MERGE)+ `es_sprite` 的
`CGACT[ID=… ID2=… X=… Y=… MERGE=1]`(自合并平铺)。

### 2b. 官方执行体(REA 0.495,sha256 `4f5eefa3…`)

`MERGE` `FUN_0043b33c` / `MERGE2` `FUN_0043b8ec` 的入口都先做剪切模式计算:

```text
ESI = X(ECX), EDI = Y(栈参)
EAX = [upper + 0x78] + X ; cmp [lower + 0x78] ; ja ⇒ clip |= 8   ; 右越界
EAX = [upper + 0x7c] + Y ; cmp [lower + 0x7c] ; ja ⇒ clip |= 2   ; 下越界
X < 0 ⇒ clip |= 4                                                ; 左越界
Y < 0 ⇒ clip |= 1                                                ; 上越界
CALL 0x43b518                                                    ; 落点/跨度设置
```

`FUN_0043b518` 按 16 种剪切位设置 `DAT_00801c84/88`(目标起点)与
`DAT_00801c7c/80`(源起点)、`DAT_00801c8c/90`(跨度/行数),例如无剪切:

```text
dest = (X, Y), src = (0, 0), span = (upper.w, upper.h)
```

左/上越界时目标夹到 0、源起点取 `-X/-Y`、跨度加回 `X/Y`;右/下越界时
跨度取 `lower - X` / `lower - Y`。即 **X/Y = 上层左上角在下层表面上的
落点**,四边裁剪,与本次实现的求交循环同义。

同一常量 `0x43b518` 的调用点共 6 处:`0x43b33c`、`0x43b8ec` 和
`0x43bad0`/`0x43bca0`/`0x43c1d8`/`0x43c388`(XOR/AND 等合成族),说明
落点语义是整个两层合成族的共同约定。

### 3. 内核旧行为

`ImageOp` 完整记录了 `x/y`,但两个后端在合并分支里显式丢弃:

```text
crates/yu-render/soft/src/render.rs:472   let _ = (image.x, image.y);
crates/yu-render/wgpu/src/plan.rs:1597    let _ = (image.x, image.y);
```

soft 随后 `apply_merge_layers`、wgpu 全屏 draw,都把上层对齐到 `(0,0)`。
立绘画布是 1280×720、内容居中,左上角本来是透明的 ⇒ 唯一可见的就是
这块被贴错位置的口型帧。

历史原因:2b53786 曾把「带非零 `X/Y` 的合并」整条跳过
(`gfx.imageop.merge.offset.unsupported`),导致 `RECTPAINT3 SET=0`
清掉的矩形补不回来(立绘轮廓缝隙、眨眼条带);后续为救症状改成
「忽略 `X/Y`、整面合成」,把定位语义整个丢掉了。

### 4. 手册

`cg/cgact.html` 只在参数表写:

```text
X = 値  文字列表示の際のX座標を指定 または レイヤの範囲指定をする際に使用
Y = 値  文字列表示の際のY座標を指定 または レイヤの範囲指定をする際に使用
```

合并族(MERGE/MERGE2)没有示例页,但执行体已由 REA 逐指令确认 X/Y 为目标
落点(见 §2b),故记 `fixed(no-manual)`(引擎证据充分,手册缺页)。

## 根因

`MERGE/MERGE2/XOR/AND` 的 `X/Y`(上层贴入下层表面的落点)在 soft/wgpu
两个后端均被丢弃,统一按 `(0,0)` 整面合成。YU-RIS 立绘系统的口型/眨眼/
表情差分全部走这条路径 ⇒ 所有差分帧都贴在画布左上角。

## 修法

- soft:`apply_merge_at_layers(..., dx, dy, ...)` /
  `apply_bitwise_at(..., dx, dy)`(`crates/yu-render/soft/src/render.rs`);
- wgpu:`merge.wgsl` / `composite.wgsl` 的 uniform 增加 `dx/dy`,目标像素
  `p` 采样上层 `p-(dx,dy)`,落点外原样返回下层;`pipelines.rs` 两个
  bind group 透传;
- `MID`(mode 3)无 `X/Y`(记录时恒 `None`),行为不变;
- 合并结果尺寸仍等于下层表面,越界按「上层矩形 ∩ 下层表面」裁剪,与
  CPU 参考实现一致。

## 残余/边界

- 官方执行体接收的 `X/Y` 是 32 位整数;脚本若给小数,引擎的定点转换
  (`cvtt` 截断 vs 舍入)未取证。本实现按 `round()` 取整,soft/wgpu 一致;
  现有语料的 `X/Y` 全是整数表达式,不触发该边界。
- 同一 `FUN_0043b518` 还有 `0x43bad0`/`0x43bca0` 两个调用点(按 CGACT
  参数表序号推断为 `TRANSITION`/`TRANSITION2`)。本内核 TRANSITION 走
  独立实现且记录 `x/y = None`,本次未改;若后续遇到带 `X/Y` 的 TRANSITION,
  需按同一锚点另案处理。

## 回归

- soft:`imagefilters::tests::merge_lands_upper_at_offset_and_clips`、
  `merge_negative_offset_skips_upper_origin`、
  `bitwise_lands_upper_at_offset`;
- wgpu:`tests/surfaces.rs::merge_lands_upper_at_x_y_offset`(落点
  `(5,4)`、越界/未覆盖断言、GPU 与 soft 逐字节一致)、
  `xor_lands_upper_at_x_y_offset`(同族落点);
- 实机:重跑同一剧情,口型差分应贴回脸部,左上角碎片消失(待主人点验)。

验证命令与原始结果见 `PROGRESS.md` 成果 `GFX-MERGE-OFFSET`。
