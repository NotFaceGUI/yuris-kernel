# DIV-GFX-089 · 立绘切换的目标缓冲为空,过渡输出被 1×1 占位截断

- 状态:**fixed(实机待验)** — 根因 = `CGACT[RESIZE]` 未回写图层矩形
- 目标样本:`<game-d>`(YU-RIS 0.486,
  `pac/data.ypf`)
- 症状:剧情过渡后画面跳一下;角色立绘淡出直接消失。
- 日志锚点(修复前):
  `UNIMP [gfx] gfx.info.unsupported @ data\script\eris\sprite\es_sprite.yst pc6134:
   快照纹理 32 尺寸未记录(源层无尺寸元数据)`

## 1. 复现与取证

`YU_GFX_TRACE=1` 记录 `COPY/TRANSITION/CGINFO` 与 `$L.FCG`/`$S.FCG`
(取证后已删除):

```text
frame=1219 CG  SPLBUFb1001 size_changed None -> 1×1
frame=1219 TRANSITION dst=SPLBUFb1001 dst image=1×1
          from=SPLBUF1001 Asset(st_kno_06_bk0_07)
          to  =SPLBUFa1001 SolidBlack1x1
frame=1219 COPY id=30 dest=SPLBUF1001 source=SPLBUFa1001
          src_tex=SolidBlack1x1 => display=1×1
frame=1260 COPY id=32 dest=SPLBUF1001 source=SPLBUFa1001 => display=1×1
frame=1260 CGINFO snapshot=32 SX/SY resolved=1×1
```

变量探针(s58 = `es_sprite.yst`):

```text
LET s58@1349 var=Dollar id=3121 idx=[10] = Str("Char1")   ; $L.SPID
LET s58@1350 var=Dollar id=3122 idx=[10] = Str([])        ; $L.FCG
STR s58@3935  var=Dollar id=3934 = Str([])                ; $lp.fn
LET s58@3976  var=Dollar id=3942 = Str("SPLBUFa1001")     ; LOADCG 目标
```

⇒ 切换命令只带 **SPID=`Char1`**,没有 CG 文件名;`$L.FCG(10)` 为空,
`es.SP.LOADCGF` 的 `$lp.fn` 为空,`$LIDBUF2`(目标缓冲)保持 1×1。
0.486 `es.SP.LOADCGF` 命中 1×1 空目标后只做
`CGACT[ID=$LIDBUF2 RECTPAINT3=1 SET=0]`(清成透明),**不扩容**;随后
`es.SP.RESIZE` 逐边发 `CGACT[RESIZE=1]`,把 1×1 扩到旧立绘尺寸
(1139×720),再进入过渡。

## 2. 官方语义(REA/编译产物已证部分)

- `CG[FILE=""]` 空串分支(0.486 `FUN_0040c5f4`,0.495 `0x40c5f8`)
  无条件把层 `+0x78/+0x7c` 写 1、`+0x90` 表面重分配 1×1。
  ⇒ 1×1 占位本身是官方行为。
- `CGINFO[SX/SY]` 直接读层 `+0x78/+0x7c`(0.495 `0x43d86d/0x43d87d`),
  无显示矩形时返回 0,不是错误。
- `TRANSITION`(0.486 `FUN_0043a950` / 0.495 `0x43bad0`)+ 裁剪
  `FUN_0043a3a4`/`FUN_0043b518`:目标起点/跨度由**目标层**
  `+0x78/+0x7c` 与源 A 尺寸求交,像素循环用目标 `+0x90` 表面;
  **没有“目标不够大就按源 A 扩容”的分支**。
- `CGACT[RESIZE]` 执行体(0.495 `FUN_0043c971`)：
  `newW = srcW + X + X2`、`newH = srcH + Y + Y2`，并写图层矩形
  `+0x78/+0x7c`（同时置 `+0x86=1`）。这就是 `es.SP.RESIZE` 让空目标
  恢复旧尺寸的机制。

## 3. 根因

0.486 编译脚本(`pac/data.ypf` 内 `ysbin/yst00058.ybn`，不是散盘
`ysbin/`）的 `es.SP.RESIZE` 在 1×1 空目标上连续发四条
`CGACT[RESIZE=1]`，每条之后用 `CGINFO[SX/SY]` 读回尺寸再算下一条边：

```text
pc4052 CGINFO[ID=$LIDBUF2+...] SX → @0x0f6f
pc4054 CGINFO[ID=$LIDBUF2+...] SY → @0x0f70
pc4056 IF[both == 1] CGACT[ID=$LIDBUF2 RECTPAINT3=1 SET=0]
...
pc4081 CGINFO[ID=$LIDBUF2+...] SX → @0x0f67
pc4082 CGINFO[ID=$LIDBUF2+...] SY → @0x0f68
pc4089 GOSUB[#=es.SP.RESIZE ...]
```

内核 `cgact_image` 只在 `SIZE` 族更新 `LayerKind::Image.size`，
`RESIZE` 只记录像素加工项，**没有回写图层矩形**。于是 `CGINFO` 在四条
`RESIZE` 之间一直返回 1×1，`es.SP.RESIZE` 的边距计算全部按 1×1 进行；
过渡输出 `$LIDBUF3` 也按 1×1 创建。过渡后可见层从 1139×720 清成 1×1，
再按 1×1 图放大 ⇒ 画面跳变、立绘瞬间消失。

## 4. 修复

- `yu-gfx::cgact_image`：执行 `RESIZE` 时按官方公式更新目标层
  `LayerKind::Image.size = (srcW+X+X2, srcH+Y+Y2)`；基数取命令前尺寸，
  非法尺寸（<=0 / 宽 > 0x2000）不更新。
- `ImageOp` 新增 `resize_base`：冻结每条 `RESIZE` 执行前的表面尺寸。
  后端回放 1×1 符号表面时用这份基数，避免按已更新的矩形把增量再加一遍。
- soft 后端 `base_texture` 的 `SolidBlack1x1` 分支接受显式基数
  (`solid_fill_texture_sized`)；其余调用仍按图层矩形铺开。

保留此前两处 REA 对齐修复：

- `cgact_copy`:`SolidBlack1x1` 无显式尺寸时,快照显示尺寸按引擎
  层构造默认 **1×1** 记录；
- `query_layer_size`(Snapshot 分支):无显示矩形时返回 **0**,
  不再 `Unimplemented`。

**已撤销**的兼容补丁(2026-11-16 二轮):

- `cgact_transition` 曾把 1×1 占位目标按源 A 尺寸恢复;REA 证明官方
  `TRANSITION` 不扩容目标,该补丁是错误语义,已回退。

## 5. 验证

- `cargo test -p yu-gfx --test parity_fixes` → 41 passed;
  新增 `resize_updates_layer_rect_for_cginfo`:1×1 空目标连续两次
  `RESIZE` 后 `CGINFO SX/SY = 1039×720`,两条 `ImageOp` 的
  `resize_base` 分别为 `(1,1)`、`(1139,720)`。
- `cargo test -p yu-gfx`、`cargo test -p yu-backend-soft`、
  `cargo build --workspace --all-targets` 通过。
- 实机:待启动 0.486 样本走同一剧情过渡确认（不再跳变、立绘按官方
  过渡淡出）。

## 6. 残余

- `SolidBlack1x1` 作为过渡/遮罩取样源时仍按手册呈黑
  (`DIV-GFX-077` 残留);是否应在空目标淡出里改透明,属于后续取证范围。
- 临时 trace 与变量探针已删除。

## 7. 补记(2026-11-17):COPY 矩形成为 RESIZE 基数

`RESIZE` 的基数一直走 `query_layer_size`(等价官方读目标 `+0x78/+0x7c`)。
此前 COPY/COPY2 只写 `snapshot_sizes`,而查询先读 `LayerKind::Image.size`,
所以“局部 COPY 后再 RESIZE”会把 COPY 前的旧显式尺寸当基数。

本次 `Layer.display_rect: Option<CopyId>` 落地后,查询优先级变为
`display_rect → Image.size → 纹理/资产`;`RESIZE` 的基数随之与官方
`FUN_0043c971` 一致(读复制后的区域矩形)。回归
`resize_after_copy_uses_copy_rect_as_base`:800×600 目标局部 COPY 100×50
后 `RESIZE X=5 X2=5` → 110×50(旧实现为 810×600)。

## 8. 补记(2026-11-17):wgpu 回放接入 `resize_base`

`dc545ae` 只让 soft 消费了 `ImageOp::resize_base`。wgpu 的
`resolve_target_base` 对活层仍按帧末 `LayerKind::Image.size` 重建 1×1
符号基面,`Resize` 分支再加 `X/X2` ⇒ 增量重复叠加。

实测(临时探针,已删除):1×1 空层 `RESIZE X=1 Y=0 X2=1 Y2=0`,
wgpu 物化表面为 **5×1**,官方/soft 为 **3×1**。

修复:wgpu `resolve_target_base` 增加 `solid_size` 参数,`RESIZE` 调用点
传 `image.resize_base`;新增
`resize_empty_layer_uses_command_base_on_gpu` 与 soft 对拍
`resize_empty_layer_uses_command_base_for_followup_copy`。资产/快照基面
不受影响,仅在 `SolidBlack1x1` 符号面使用命令时刻基数。

## 9. 补记(2026-11-19):COPY/COPY2 覆盖目标矩形与表面(读档黑幕回归)

官方 0.495 `FUN_0042f809` 复核(REA Ghidra 12.1.4,产物
`4f5eefa3…`,evidence `ev_ff12409d…`):

- 入口无条件算区域:`local_2c = X2-X+1`、`local_28 = Y2-Y+1`
  (`0x42f85f..0x42f876`);
- 单层路径(0x42f9ef..0x42fa14)与双层 COPY/COPY2 路径
  (0x4305f4/0x4305fe)在动作分派后都执行同一段:
  `FUN_00468e10([dst+0x90])` 释放旧表面 → `[dst+0x90] = 新分配的
  区域尺寸表面` → `[dst+0x78] = local_2c`、`[dst+0x7c] = local_28`
  (`MOV [EAX+0x78],ESI` @0x4305f4、`MOV [EAX+0x7c],EDI` @0x4305fe)。
- ⇒ `CG[FILE=""]`(0.495 `FUN_0040c5f4`,写 `+0x78/+0x7c = 1`、表面
  重分 1×1)留下的 1×1 矩形,在随后的 COPY/COPY2 之后**不再代表目标
  层**:表面与矩形都被复制区域替换。

内核快照表面保持**源尺寸**(`DIV-GFX-077` 的既有残留:区域块在
(0,0),其余透明),与官方“区域尺寸表面 1:1 绘制”逐像素等价。因此
`cgact_copy` 现在把目标 `LayerKind::Image.size` 清成 `None`(后端按
快照自然尺寸绘制),`display_rect` 仍记录复制区域供
`CGINFO`/`ONMOUSE*` 查询;COPY 之后脚本写 `SIZE/RESIZE/SX/SY` 会重新
写 `Image.size` 并清 `display_rect`。

回归:`dc545ae` 给**复用**的 `CG[FILE=""]` 层回写 1×1 矩形后,读档
`es.CGTSS` 的 CGS 幕布层(菜单打开时已创建)再次 `CG CGS<n> FILE=""`
被重置成 1×1,`COPY2 ID=:/W0/` 的整幅冻结屏幕被缩成 1 个像素 ⇒ 读档
淡出直接黑屏(<game-b> 2026-11-19 实测)。

回归测试:`copy2_after_reused_fileless_layer_draws_snapshot_at_natural_size`
(`crates/yu-gfx/tests/info_query.rs`),同时断言 `CGINFO[SX/SY]` 仍按
复制区域(视口)回答。
