# DIV-GFX-097 · `COPY2` 源快照不可物化时仍覆写目标,立绘在过渡中消失

- 状态: fixed(残余:源暂存层为何为空未对齐)
- 优先级: 高(立绘/背景过渡整段不可见)
- 发现日期 / 发现方式: 2026-11-25 / `<game-c>` debug-ui 逐帧抓取 + `frames.jsonl`

## 复现

- 游戏位置: `<game-c>` 藤花台词换行处
- 锚点: engine frame 1238 有立绘 → 1239 空白 → 1255 恢复;
  用户抓帧 1782 有 / 1842 无

## 官方行为

`COPY/COPY2` 在命令处理体内**立即**读源表面并写目标(`EVID-T-15`,
`FUN_0042f809`);命令时刻源没有像素时不会把目标换成“未来才可能建出来”的
表面。

## 内核行为

帧 1239–1254 的逐帧层图:

```text
pass target=:/W0/SPLBUF601  op=copy2  source=:/W0/SPLBUFa601
pass target=:/W0/SPLBUFb601 op=transition set=0..245
render skipped: copy  target=SPLBUF601  reason=gfx.copy.source.snapshot_missing
render skipped: image target=SPLBUFb601 reason=gfx.copy.source.snapshot_missing
```

`INFO.CHANGE` 的提交 `SPLBUF601 ← SPLBUFa601` 先于 `INFO.INIT` 填充 `a`;
`cgact_copy` 仍把 `SPLBUF601` 改指向 `Snapshot(本次 copy)`。帧末
`resolve_source` 发现源快照的复制链已断,跳过复制;过渡输出 `b`/`SPL601`
全空,直到 1255 帧复制顺序变正(先填充 `a` 再提交)才恢复。

## 根因

- `CopyOp.source_texture` 记录的快照在帧末可能已不可物化(生产者被同帧
  的后续复制裁掉/未烘焙);
- `cgact_copy` 无条件把目标层改指向该快照,目标上一帧仍可见的像素被
  一个永远建不出来的空表面替换。

## 修法

- `CopyOp.dest_texture_before`:命令时刻记录覆写前的目标纹理;
- `frame()` 时若 `copy_source_unresolvable`(按后端 `resolve_source`
  同口径递归检查)成立:不发射该复制,并在投影层把目标回退到
  `dest_texture_before`;同时发一条 Verbose
  `gfx.copy.source.snapshot_missing` 记账,不静默;
- 正常源(Asset/可物化快照)行为不变。

## 回归

- `crates/yu-gfx/tests/transition.rs::copy2_from_asset_source_still_repoints_destination`
- `cargo test -p yu-gfx --tests` → 全部通过(含 12 transition 用例)。
- 反例测试(空源不清目标)已删除:实测 `CG[FILE=""]` 是 1×1
  `SolidBlack1x1`,不是 `TextureRef::None`;该断言前提不成立,保留会
  误报。当前 skip 路径由实机 1239–1254 帧图取证,尚无脚本级可复现夹具。

## 残余

- 为什么 `SPLBUFa` 在 1239 命令时刻是空的(官方此时仍保留上一姿态):
  `INFO.CHANGE/INFO.INIT` 的执行顺序与 `a/b` 暂存层生命周期尚未 1:1;
- 若被跳过的目标是直接 `Op::Layer` 绘制(而非经 image op 解析),投影
  回退只改 `frame.layers`,该 op 内嵌的 `Arc<Layer>` 仍是旧纹理;本次
  立绘链走 image op,不受影响,通用化待补。
