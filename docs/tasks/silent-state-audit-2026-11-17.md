# 静默状态缺口普查(2026-11-17)

- 背景:`CGACT[RESIZE]` 曾“动作可执行、像素也改，但没有回写图层矩形”，
  脚本 `CGINFO` 读到看似合法但过期的 1×1，导致 0.486 立绘过渡静默截断。
  本档按同一模式普查：**返回 `Executable`/成功、却漏掉官方同一执行体内
  的状态写入**。
- 真值层级：0.486/0.495 官方二进制 REA > 原版数据/手册 > 旧线 golden。
- 范围：`yu-gfx` 的 `CG`/`CGACT`/`CGINFO` 状态路径 + soft/wgpu 后端对
  这些状态的消费；不含 E-mote（主人暂缓）与 iOS。

## 1. 已闭合

| 项 | 官方证据 | 内核处置 |
|---|---|---|
| `RESIZE` 不改层矩形 | 0.495 `FUN_0043c971`：`newW=srcW+X+X2`、`newH=srcH+Y+Y2`，写 `+0x78/+0x7c` | `cgact_image` 命令时刻更新 `LayerKind::Image.size`；`ImageOp.resize_base` 冻结回放基数；回归 `resize_updates_layer_rect_for_cginfo`。DIV-GFX-089 |
| `CG[FILE=""]` 对已存在层保留旧矩形 | 0.486 `FUN_0042b144(EAX=层, EDX=名)`：空串 → `FUN_0040c5f4` 无条件写 1×1；非空 → `FUN_0040c7c5` 按新图重建 | `apply_cg_inner` 换图分支：空串重置 1×1；资产换图按新图尺寸（未知则置空，由 `asset_sizes` 回退）；回归 `empty_file_on_existing_layer_resets_rect_to_1x1`。DIV-GFX-060/085 |
| `COPY/COPY2` 目标矩形被旧显式尺寸遮住 | 0.495 `FUN_0042f809` 动作后无条件写目标 `+0x78/+0x7c = X2-X+1 / Y2-Y+1`（0x4305f4/0x4305fe；单层 0x42fa0a/0x42fa14）；`CGINFO[SX/SY]` 读该矩形（0x43d86d/0x43d87d） | `Layer.display_rect: Option<CopyId>` 记来源，查询/命中按 `display_rect → Image.size → 纹理/资产`；显式尺寸/换图/RESIZE/SIZE/重建清除；绘制仍按 `Image.size`。回归 `copy2_rect_overrides_stale_explicit_size_and_yields_to_new_cg_size` 等 5 条。DIV-GFX-077 §2.4 |

## 2. 仍 open(按风险排序)

### 2.1 `COPY/COPY2` 目标矩形与 `LayerKind::Image.size`（fixed 2026-11-17）

- 官方 `FUN_0042f809` 在动作后**无条件**写目标
  `+0x78/+0x7c = X2-X+1 / Y2-Y+1`。
- 内核 `cgact_copy` 把目标纹理换成 `Snapshot`，显示矩形只记在
  `snapshot_sizes`；**故意不改** `LayerKind::Image.size`，因为后端把
  `Image.size` 当整张表面缩放，改小会把源合图压扁（DIV-GFX-077）。
- 后果：目标层若在 COPY 前已有显式 `Image.size`，`CGINFO[SX/SY]` 先读
  `layer.image_size()`，会返回旧矩形；`ONMOUSE/COLOR` 同样优先读它。
  按钮/局部快照链可能拿到错误命中矩形（发布审计 0.3.3 第 1/4 项）。
- 需要：把“表面尺寸”和“显示/命中矩形”拆开，或在 COPY 时记录矩形
  代次，查询只读 COPY 后的矩形；不能简单改 `Image.size`（会改绘制缩放）。

### 2.2 `TRIM` 改表面尺寸但不回写矩形

- 手册例 22：`TRIM=1 SET=15` 裁掉完全透明边缘；soft `apply_trim` 已按
  位掩码裁纹理。
- 内核 `cgact_image` 只在 `SIZE`/`RESIZE` 更新层矩形，`TRIM` 没有；
  `ImageAction::changes_extent()` 已存在但无消费者。
- 与 RESIZE 不同：TRIM 的新尺寸依赖**像素 alpha 包围盒**，gfx 在命令
  时刻没有像素（资产解码/前序图像加工都在后端回放），不能直接算。
  需要后端回报裁后尺寸，或把 TRIM 纳入表面/矩形两阶段提交。
- 当前 `.yst` 源语料未见 `TRIM` 调用（动态脚本未普查），风险中低；
  未凭手册猜公式改状态。

### 2.3 `CGINFO[ONMOUSE*/COLOR]` 的尺寸优先级（fixed 2026-11-17,与 2.1 同修）

- `query_onmouse` 依次取：`layer.image_size()` → `snapshot_sizes` →
  纹理实测 → `asset_sizes`。
- 与 2.1 同根：COPY 后 `Image.size` 可能是旧值，会先于快照矩形命中。
  未单独改，避免影响“COPY 后脚本显式 `CG[SX/SY]` 覆盖矩形”的合法用法。
- 需要与 2.1 一起做“矩形代次/来源”标记，不能只看类型。

### 2.4 `CG[FILE=<目录形键>]` 的失败矩形

- 目录形 `FILE` 按 DIV-GFX-084 退化成透明空白表面；官方装载失败时
  `+0x78/+0x7c` 是否保持 0/旧值，尚未逐分支取证。
- 当前实现保留旧尺寸（未纳入本轮空串/资产两条已证分支）；风险低，
  需要官方失败路径 REA 或微场景。

### 2.5 `TRIM` 全透明 / `SIZE` 单轴

- 全透明 TRIM：`apply_trim` 保守保持原样，官方“裁到 0”未取证。
- `SIZE=1` 只给 `SX` 或只给 `SY`：当前另一轴取当前尺寸/1；源语料
  0 次单轴用法，官方未逐分支取证。低风险，登记不猜。

## 3. 已核对、暂不算缺口

- `ROTATION`：soft 在原地同尺寸画布内旋转；官方旋转画布扩展由脚本
  `es.CGACT_SIZE` 的 `WSX.BIG` 负责，命令本身不改矩形。
- `MERGE/MERGE2/XOR/AND`：只改目标像素，不改层矩形。
- `SWAP/SWAP2`：官方模式 1/2 换纹理 + X/Y（+属性），未换矩形；查询按
  纹理类型取尺寸，暂未发现同类静默错误。
- `TRANSITION`：官方只用目标 `+0x78/+0x7c` 与源求交，不扩容目标；
  0.486/0.495 一致，已回退旧扩容补丁。
- `CGINFO[SX/SY]` 快照无矩形返回 0、`CG[FILE=""]` 1×1 四零像素等已
  按 REA/微场景裁定。

## 4. 建议

1. 先做 2.1 + 2.3 的“矩形来源/代次”设计：COPY 写显示矩形，绘制仍按
   表面尺寸；这是最可能影响真实 UI 命中/嵌套快照的一项。
2. 2.2 TRIM 需后端回报裁后尺寸，单独任务，不与 COPY 混修。
3. 2.4/2.5 只在有官方证据或实际样本复现时动；不得用“看起来合理”的
   公式猜。

## 5. 本轮验证

- `cargo test -p yu-gfx --test parity_fixes` → 42 passed
  （含 RESIZE 与 `CG[FILE=""]` 已存在层两条新回归）。
- `cargo test -p yu-gfx`、`cargo test -p yu-backend-soft` 全绿。
- 0.486 样本实机 1614 帧未再出现 `gfx.info.unsupported`。
