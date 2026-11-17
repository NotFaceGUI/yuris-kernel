# 任务 01 · COPY/COPY2 目标矩形与查询/命中尺寸分离

- 状态：fixed（实机待验，2026-11-17）
- 优先级：P1（最可能影响真实 UI 命中与嵌套快照）
- 关联：`DIV-GFX-077`、`DIV-GFX-089`、
  `docs/tasks/silent-state-audit-2026-11-17.md` §2.1/2.3
- 官方锚点：0.495 `FUN_0042f809`（CGACT 分派）在动作后无条件写目标
  `+0x78/+0x7c = X2-X+1 / Y2-Y+1`；`ONMOUSE/ONMOUSE2/ONMOUSE3` 处理体
  `0x43de01/0x43dec8/0x43df7f` 读该矩形。

## 目标

让 `COPY/COPY2` 目标的**显示/查询矩形**与官方 `+0x78/+0x7c` 一致：

- `CGINFO[SX/SY]` 在复制后返回区域尺寸；
- `CGINFO[ONMOUSE*]/[COLOR]` 按该矩形做命中和像素采样；
- 绘制仍按**表面像素尺寸**，不因区域矩形把源合图压扁
  （`DIV-GFX-077` 已确认的取舍）。

## 现状

- `GfxState::cgact_copy` 把目标纹理换成 `Snapshot`，把区域尺寸记录在
  `snapshot_sizes`（原点 `snapshot_origins`、源 `snapshot_files`），
  **故意不写** `LayerKind::Image.size`；注释见 `state.rs` 的
  `COPY/COPY2 局部区域` 段。
- `query_layer_size` 先读 `layer.group_size()/image_size()`，再按纹理类型
  回退；`query_onmouse` 也先读 `layer.image_size()`。
- 目标层若在 COPY 前已有显式尺寸（`CG[SX/SY]`、`SIZE`、`RESIZE` 等），
  复制后查询会先命中旧 `Image.size`，得到过期矩形。
- 现有 `snapshot_sizes` 已能给出正确区域尺寸，但查询优先级把它排在
  旧 `Image.size` 后面。

## 设计约束

不要直接把 `LayerKind::Image.size` 改成区域尺寸：后端把它当整张表面的
绘制缩放，改小会压扁源合图（DIV-GFX-077）。

推荐方向（实现前先验证并写清取舍）：

1. 在 `Layer` 或 `GfxState` 增加独立的**显示/命中矩形**
   （如 `display_rect: Option<SizeU32>` + 原点 + 来源代次），与
   `LayerKind::Image.size`（表面/绘制尺寸）分离。
2. `COPY/COPY2` 写入该矩形；`CG[ID SX/SY]`、`SIZE`、`RESIZE`、`TRIM`
   的显式尺寸写入要**覆盖/失效**它，并递增代次。
3. `CGEND`/重建层清理该矩形。
4. `query_layer_size`/`query_onmouse` 按“显式尺寸代次 > COPY 矩形 >
   纹理/资产尺寸”的顺序取值；绘制仍用 `Image.size`/纹理尺寸。
5. 嵌套 COPY：下一次 COPY 的源尺寸按官方表面/区域语义取，不能把
   `display_rect` 误当表面尺寸（见 `DIV-GFX-077` §2.1）。
6. `snapshot_display_size` 与 `snapshot_regions` 保持现状，除非能证明
   需要统一；不要引入第二套区域换算。

## 必做回归

1. 已存在目标层（先 `CG[FILE=cg/x SX=800 SY=600]`）→
   `COPY2` 局部 100×50 → `CGINFO SX/SY = 100/50`；
   `ONMOUSE` 在 (99,49) 命中、在 (100,50) 返回 -1。
2. COPY 后显式 `CG[ID=target SX=200 SY=150]` → 查询/命中回到 200×150。
3. 全幅 COPY（无区域）→ 目标矩形 = 源表面尺寸。
4. 组源 COPY（`ID=:/W0/`）→ 现有按钮/首屏快照行为不变。
5. 嵌套 `COPY2`：从局部快照再复制，尺寸/原点不双重换算。
6. `cargo test -p yu-gfx`、`cargo test -p yu-backend-soft`、
   `cargo test -p yu-backend-wgpu --test plan_scaling`、
   `cargo build --workspace --all-targets`。

## 验收

- 上述测试全绿，且现有按钮/快照 golden 不回归。
- 更新 `DIV-GFX-077`（记录“显示矩形与表面尺寸分离”的最终口径）、
  `DIV-GFX-089`（若涉及）、`PROGRESS.md`。
- 实机若可跑：至少验证一处按钮 hover 命中与一处局部快照。

## 禁止

- 不得把 `LayerKind::Image.size` 直接写成 COPY 区域尺寸。
- 不得给具体按钮/SPID/窗口命名加特判；必须是通用层语义。
- 不得改脚本数据来绕过。

## 提交建议

```text
fix(gfx): COPY 目标矩形独立于绘制表面,修正 CGINFO/ONMOUSE 命中
```

## 完成记录（2026-11-17）

- 实现 `Layer.display_rect: Option<CopyId>`（来源标记，查询期动态解析）；
  `query_layer_size`/`query_onmouse` 优先级 = 组尺寸 → `display_rect` →
  `Image.size` → 纹理/资产；显式 `CG SX/SY`（含单轴）、`SIZE`、`RESIZE`、
  `CG[FILE]` 换图、`YMV` 表面重写与层重建清除；`SWAP` 不交换矩形。
- 绘制/后端零改动：`LayerKind::Image.size` 仍是表面缩放尺寸；
  `snapshot_sizes`/`snapshot_regions`/`snapshot_display_size` 保持现状。
- 回归 6 条见 `crates/yu-gfx/tests/info_query.rs`（含单轴 `CG SX` 的
  缺省轴取 COPY 矩形）；全量验证结果见 `PROGRESS.md` 成果 COPY-RECT-SPLIT。
- 0.486 实机验收（按钮 hover 命中 + 局部快照）待主人执行。
