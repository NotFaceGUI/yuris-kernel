# DIV-GFX-068 · `SAVE[ID]` 命令时刻抓帧(同 tick `COPY2→SAVE`)

> 状态:`fixed`(2026-10-29)。范围:`yu-engine::capture` + `engine` 的逐帧
> `last_screen` 注入。关联:`PROGRESS.md`「成果 SAVE-THUMB」/
> 「成果 PERF-ASSET-PRELOAD」、`DIV-GFX-066`(帧契约)。

## 1. 现象

`<game-b>` 手动存档后,存档槽缩略图为空;`.sd` 的 DNO3 块退化为
`ulen=1612 / B=4`(1×1),官方产物是 `ulen=31608 / B=30000`(100×75)。
HUD 存档界面刷新后仍无图。

## 2. 官方行为与脚本序列(真值 1/3)

`data/script/eris/es_saveload.yst`(sid 19,**同一 tick**):

| pc | 命令 |
|---|---|
| 312 | `CG ID="SLTEMP2" Z=1 A=0 SX=1 SY=1 FILE=""`(先建 1×1 占位层) |
| 313 | `CGACT COPY2=1 ID="SLTEMP" ID2="SLTEMP2"` |
| 319 | `SAVE DNO=3 ID="SLTEMP2"` |
| 341 | `SAVE FILE=...`(刷盘) |
| 343 | `CGEND ID="SLTEMP2"` |

官方 `CGACT` 在命令处理器内**立即写层表面**(EVID-T-15),`SAVE[ID]`
(`0x4477c0`)读同一块表面 ⇒ 同 tick 能拿到刚拷贝的像素。`SLTEMP` 是打开
存档界面时冻结的截图快照(前几帧已存在)。

## 3. 内核根因

帧契约(66)后,wgpu 把写表面推迟到帧末渲染;`GfxCapture` 只有“上一帧
已渲染快照”的注册表/镜像。pc319 在**本帧渲染之前**执行,新 `CopyId`
不在注册表里,`capture` 返回 `None`;若 `COPY2` 的源在命令时刻不可用,
目标层仍是 `CG FILE=""` 的 1×1 占位 ⇒ 写出 1×1 缩略图。

## 4. 修法

`GfxCapture::capture` 对 `TextureRef::Snapshot(copy_id)` 三级取像素:

1. **注册表/镜像**(`snapshot_pixels`):上一帧已渲染快照,GPU 按需读回
   一次并缓存(`1–8ms`);
2. **源链裁剪**(`chain_snapshot`):沿 `CopyOp.source_texture` 回溯源快照/
   资产,按 `CopyRegion` 闭区间裁剪;源快照已在注册表时零回放
   (`SLTEMP→SLTEMP2` 走这条,实测无整帧回放);
3. **命令时刻软回放**(`replay_snapshot`):组/后缓冲/带文本的源才回放
   `state.frame()`(soft 参考实现,`last_screen` 由引擎每帧渲染后注入)。

`TextureRef::SolidBlack1x1` 现在显式告警:`SAVE[ID]` 落到 1×1 占位说明
拷贝缺失,不再静默写空图。

## 5. 验证(原始结果)

```text
# soft headless(kotoneiro 实机脚本:START→SAVE→槽 1)
[caption] SAVE[ID] replay snapshot=336 1100x825 436088us   # 首次(整帧回放)
[caption] LOAD[ID] PNG 100x75 19340B encode=603us
DNO3 btype=1 ulen=31608 A=1600 B=30000

# 加入源链裁剪后:无 replay 行;DNO3 仍 ulen=31608 / 100×75
# 桌面 wgpu(--mode desktop --backend wgpu --frames 640,同一脚本)
[caption] LOAD[ID] PNG 100x75 12241B encode=548us
DNO3 btype=1 ulen=31608 A=1600 B=30000(存档界面截图可见缩略图)

cargo test -p yu-engine --test capture_snapshot → 4 passed
```

## 6. 残余

- 整帧软回放(仅组/后缓冲源)约 0.4s(1100×825、188 层);低频存档可接受,
  后续若需降低可缓存回放结果或按图层子集回放。
- 同帧 `COPY` 后立即 `SAVE` 且源快照**也**同帧生成时,源链与注册表都
  不可用 ⇒ 仍整帧回放(正确但慢);不会静默丢图。
