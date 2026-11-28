# DIV-GFX-102 · `CG` 无 FILE 但带状态参数时建空白层

- 状态: **fixed**(2026-11-28)
- 影响面: `yu-gfx::state` 的 `CG` 缺层分支
- 关联: DIV-GFX-016(鬼层)、DIV-GFX-099/100

## 事实

- 0.495 REA(`0x42a63d..0x42bbef`):`CG[ID]` 纯存在性探测在层不存在时
  返回 -1,**不建节点**;这是既有 `DIV-GFX-016` 的依据。
- 2xx 转场脚本在缺层时用**状态参数**建空白缓冲:
  `<0.263 商业>` s226 pc121/122
  `CG[ID=SRCCG A=0]`、`CG[ID=DESTCG A=0]`(无 `FILE`);
  随后 pc299 `CGACT[SWAP ID=DESTCG ID2=SRCCG]` 需要两层存在。
  旧实现把这类命令也当探测 ⇒ 两层永远不存在 ⇒ 标题转场 `SWAP` 报
  `gfx.layer.missing`,过渡整段缺失。
- 语义区分不是版本分支:命令是否携带**状态参数**(`A/E/Z/X/Y/SX/SY/
  MODE/T/TA/TID/TNO/MID/MNO/MFILE/CASH/FRAME/TEX/TRIM`)决定它是
  “在空白层上设属性”还是“纯探测”。

## 决策

- 无 `FILE/RID` 且层不存在:
  - 只给 `ID`(及 `NO`)⇒ 保持官方探测语义,返回 -1、不建节点、只留一条
    Verbose;
  - 带任一状态参数 ⇒ 建 `SolidBlack1x1` 空白层,再交给统一的
    `apply_layer_params`(A=0 等照常生效)。
- `FILE` 路径不变;`RID` 未实现分支不变。

## 验证

- `parity_fixes::cg_missing_layer_with_state_param_creates_blank`
  (`CG[ID=SRCCG A=0]` 建层且 alpha=0);
- `parity_fixes::cg_missing_layer_poll_is_reported_once_as_verbose`
  继续通过(纯 `CG[ID]` 探测仍不建层);
- `<0.263 商业>` 重跑后 s226 `CGACT SWAP` 不再报源/目标不存在。
