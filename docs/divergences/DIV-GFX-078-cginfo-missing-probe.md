# DIV-GFX-078 · `CGINFO` 对不存在层的尺寸/像素探测(BT.MASK/SLIDER 日志)

- 状态: **fixed**(2026-11-05)
- 影响面: `crates/yu-gfx/src/state.rs`(`query_cginfo` 缺失层分支)
- 关联: `DIV-GFX-070`(CGINFO[COLOR]/资产尺寸)、`DIV-GFX-024`、
  `docs/symptom-ledger.md`(`CGINFO[TEX] SPLBUF1` 先例)

## 1. 症状

商業包启动/标题阶段大量:

```text
UNIMP [gfx] gfx.info.missing @ data\script\eris\es_button.yst pc2271:
  CGINFO[SX] 目标 `:/W0/BT.MASKES.GAMEMAIN.BTN.VOLUME.HBGM.SLIDER=0=1` 不存在
... SY / COLOR 同目标
```

同类还有 `BT.OFF1`、`BT.ON1` 等按钮状态层。

## 2. 取证

- 目标层确实不在当前 `GfxState.nodes` 中;`gfx.info.missing` 由
  `query_cginfo` 的 `self.nodes.get(&parsed.path)` 为 `None` 产生,
  不是资产加载失败,也不是渲染跳过。
- 按钮脚本的 MASK 层是**可选**的:
  `es_Button.yst`(0.495 SDK 同形)在
  `IF[$BT.MASK(@mLC)!=""]` 内才 `CG[ID=$mask FILE=$BT.MASK(@mLC)]`;
  没有 MASK 图的按钮不会创建该层。
- 随后按钮几何/重绘例程对 `"BT.MASK"+$tmp+$(@BT.ANCM(...))` 无条件做
  `CGINFO[SX]/[SY]/[COLOR]` 探测,用结果作守卫
  (`es_button.yst` s0 pc2241..2243;pc2271..2273 同形)。
- 官方语义:
  - 手册 `cg/cginfo.html` 例 4/5:`SX/SY` = 层横幅/纵幅;
  - 手册例 8..10:`ONMOUSE*` 未命中 ⇒ `-1`;
  - `DIV-GFX-070` 已取证:`CGINFO[SX/SY]` 直接读层 `+0x78/+0x7c`,
    装载失败/矩形缺省为 **0**;`COLOR` 越界返回 **-1**。
- 因此层不存在时,官方得到的是中性探测值(尺寸 0、像素 -1),
  不会把“层不在”当成命令未实现。

## 3. 修复

`GfxState::query_cginfo` 缺失层分支(保留 `EXIST` 在前):

| flag | 缺失层返回 | 依据 |
|---|---|---|
| `EXIST` | 0 | 已有语义 |
| `TEX` | 0 | 已有特例(非 E-mote) |
| `SX` / `SY` | 0 | `DIV-GFX-070`;层矩形 0×0 |
| `COLOR` / `ONMOUSE` / `ONMOUSE2` / `ONMOUSE3` | -1 | 手册例 8..10 + EVID-J-070 |
| 其它字段 | 仍 `gfx.info.missing` | 未取证,不 blanket 成 0 |

不再创建“鬼层”;`CG` 无 FILE/RID 的官方 -1 语义保持
(`DIV-GFX` GFX-016 / PERF-GFX-POLL)。

## 4. 验证

```text
cargo test -p yu-gfx --test info_query
  → cginfo_missing_layer_returns_neutral_probe_values ... ok
  → 13 passed
商業包 300 帧(--log-level info)
  → gfx.info.missing 计数 0(修复前同一入口数百条)
桌面 wgpu 環境設定截图
  → 与修复前一致:预览区无黑幕、on/off 标签完整、底栏说明可见
```

## 5. 残留

- `E/A/X/Y/Z/MODE/T/TA/TID` 对缺失层的中性值尚未逐项取证,仍显式
  `gfx.info.missing`。若后续语料出现同类刷屏,按官方 `CGINFO` 处理体
  逐字段补。
- 目前不额外发 verbose 诊断:可选层探测是脚本的正常守卫,返回中性值
  即官方行为;`EXIST` 可用于需要留痕的调用方。
