# DIV-GFX-102 · `CG` 无 FILE 但带状态参数时建空白层

- 状态: **fixed**(2026-11-28)
- 影响面: `yu-gfx::state` 的 `CG` 缺层分支
- 关联: DIV-GFX-016(鬼层)、DIV-GFX-099/100

## 事实

- 0.495 REA(`0x42a63d..0x42bbef`):`CG[ID]` 纯存在性探测在层不存在时
  返回 -1,**不建节点**;这是既有 `DIV-GFX-016` 的依据。
- 2xx 转场脚本在缺层时用 `A=0` 建透明缓冲:
  `<0.263 商业>` s226 pc121/122
  `CG[ID=SRCCG A=0]`、`CG[ID=DESTCG A=0]`(无 `FILE`);
  随后 pc299 `CGACT[SWAP ID=DESTCG ID2=SRCCG]` 需要两层存在。
  旧实现把这类命令也当探测 ⇒ 两层永远不存在 ⇒ 标题转场 `SWAP` 报
  `gfx.layer.missing`,过渡整段缺失。
- **反证(0.495 ERIS 首屏)**:483 条 `CG[ID E=0 X Y MODE=0]` 的禁用
  按钮探测全部落在缺层上,官方不建节点(golden 199 层)。它们都不带
  `A`。若把“任一状态参数”都当建层信号,首屏会多出 72 个鬼层,并破坏
  `snapshot_reads` 索引一致性(实测 271 vs 199)。
- 因此语义判据收窄为 **`A`(alpha)在场**,不是版本分支:
  `A` 是“新建缓冲的初始透明度”,`E/X/Y/MODE` 只是属性设置/探测。

## 决策

- 无 `FILE/RID` 且层不存在:
  - 不带 `A`(纯 `ID`/`NO`,或只有 `E/X/Y/MODE` 等属性)⇒ 保持官方
    探测语义,返回 -1、不建节点、只留一条 Verbose;
  - 带 `A` ⇒ 建 `SolidBlack1x1` 空白层,再交给统一的
    `apply_layer_params`(A=0 等照常生效)。
- `FILE` 路径不变;`RID` 未实现分支不变。

## 验证

- `parity_fixes::cg_missing_layer_with_state_param_creates_blank`
  (`CG[ID=SRCCG A=0]` 建层且 alpha=0;`CG[ID E X Y MODE]` 不建层);
- `parity_fixes::cg_missing_layer_poll_is_reported_once_as_verbose`
  继续通过(纯 `CG[ID]` 探测仍不建层);
- `<0.263 商业>` 重跑后 s226 `CGACT SWAP` 不再报源/目标不存在。
