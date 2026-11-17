# DIV-GFX-090 · 同 tick `CGEND` 删除目标层后,跨层图像加工仍须重放

- 状态:**fixed**(2026-11-17)
- 样本:`natsunokusari`(YU-RIS 0.481)CG 鉴赏
- 脚本:`data\script\eris\scene\extra\cgmode\es_cgmode.yst` s54 pc513..536
- 关联:`DIV-GFX-053`(源层同帧删除)、`DIV-GFX-072`(冻结/烘焙/重放)、
  `DIV-GFX-077`(COPY 矩形)、`DIV-GFX-086`(MERGE 落点)

## 1. 现象

CG 鉴赏中点击一个 CG:脚本可正常继续、可翻页,但整屏变成纯白;
旧版本(0.3.3 之前)同样复现,不是本次 COPY 矩形修复引入。

## 2. 命令链(REA 之外由运行时 trace 实测)

```text
s54@513  CG        ID=_CGTMP999  FILE=cg/ev/ev_nat_01_01  A=256
s54@532  CG        ID=_CGTMP998  FILE=""  SX=1152 SY=720 A=0
s54@533  CGACT     ID=_CGTMP998  RECTPAINT3 SET=0xFFFFFFFF   ← 白底
s54@534  CGACT     ID=_CGTMP999  ID2=_CGTMP998  MERGE=1      ← CG 合成到白底
s54@535  CGACT     ID=_CGTMP998  ID2=_CGTMP999  COPY2=1      ← 合成结果拷回 999
s54@536  CGEND     ID=_CGTMP998                              ← 白底删除
s54@479  CGACT     ID=_CGTMP1000 ID2=_CGTMP1    COPY2=1
s54@480  CGEND     ID=_CGTMP999
```

淡入循环随后把 `_CGTMP999` 的 alpha 从 0 升到 256、把 `_CGTMP1000`
降到 0。实测白屏帧的图层/快照导出:

- `_CGTMP999` → `Snapshot(192)` = **1×1 全白**
- `_CGTMP1000` → 1152×720 的旧画面

1×1 白被拉伸到整屏,所以点击/翻页仍可交互,只是画面全白。

## 3. 根因

官方 `CGACT` 在命令处理体内立即执行(`FUN_0042f809`),随后 `CGEND`
只删层、不撤销已经写入的像素。内核把像素工作推迟到帧末回放:

1. `frame()` 的图像加工发射门禁(`crates/yu-gfx/src/state.rs`)只允许
   目标层仍存活,或原地加工且有 `source_texture` 的项重放;
   `_CGTMP998` 在同一 tick 的 `CGEND` 已删除,跨层 `MERGE` 被丢弃。
2. `ImageOp` 只冻结了源层 `source_texture`,没有冻结**目标层命令时刻**
   的基面。`RECTPAINT3` 的 1×1 白底因此丢失 `SX/SY=1152×720`,输出
   退化成 1×1。
3. wgpu `apply_copy` 在源层已删除时按 `base_token` 清掉了本帧刚由
   `MERGE` 产出的 `contents[_CGTMP998]`,只能回退到 1×1 符号表面。
   soft 后端从 `texture_overrides` 读取,没有这一步。

最终 `COPY2 998→999` 得到 1×1 白快照。

## 4. 修复

- `ImageOp` 新增 `target_base: Option<ImageTargetBase>`,在 `CGACT`
  命令时刻冻结目标层的 `TextureRef`、`fill` 与显式 `size`。
- `frame()` 发射门禁:目标层已删除但存在 `target_base` 且后续消费者
  需要该目标(COPY/MERGE 的源)时允许重放。
- wgpu `resolve_target_base` / soft `base_texture`:**目标层已不存在**时
  用命令时刻 `target_base` 重建基面;`SolidBlack1x1` 按记录的
  `fill + size` 铺开。目标层仍存活(包括随后被 `CG[FILE]` 换图)时
  一律用活层当前像素,不使用这份记录。
- wgpu `apply_copy`:源层已删除但本帧 `contents` 已有加工结果时,不再
  按基面 token 清掉它(官方 COPY 读到的就是该结果)。
- `TRANSITION`/`CG[MID]` 的 `ImageOp` 同样记录目标基面,避免同类问题。
- **边界**:`target_base` 只在目标层已被 `CGEND` 删除时使用。目标层
  仍存活时继续用活层当前纹理/fill/size —— 缩略图/hover 槽位会被
  `CG[FILE]` 逐张复用,历史加工必须作用在当前图上;用旧基面会把上一张
  图写回来。
- **复用槽位来源校验**:缩略图/hover 的 `_TMP_OF/_TMP_OV` 每张图同名
  重建。soft 的 `override_bases` / wgpu 的 `Contents::bases` 记录每份
  override/content 的命令时刻基面;COPY 读源时基面不匹配就丢弃旧结果,
  重新按 `source_texture` 解析。否则第二次 hover 会显示第一张缩略图。


## 5. 回归

- `cargo test -p yu-gfx --test parity_fixes` → 47 passed
  - `merge_into_target_deleted_same_tick_is_replayed_for_copy2`
- `cargo test -p yu-backend-soft` → 全部通过
  - `merge_into_target_deleted_same_tick_keeps_copy2_pixels`
  - `live_target_replay_uses_current_cg_file_not_recorded_base`
  - `gallery_reused_thumbnail_slots_do_not_leak_previous_image`
- `cargo test -p yu-backend-wgpu` → 39 passed
  - `merge_into_deleted_target_keeps_copy2_pixels_on_gpu`
  - `live_target_replay_uses_current_cg_file_not_recorded_base_on_gpu`
  - `gallery_reused_thumbnail_slots_do_not_leak_previous_image_on_gpu`
- `cargo build --workspace --all-targets` → 通过

## 6. 复查入口

- `crates/yu-gfx/src/op.rs` `ImageTargetBase`
- `crates/yu-gfx/src/state.rs` `image_target_base` / `frame()` 发射门禁
- `crates/yu-render/wgpu/src/plan.rs` `resolve_target_base` /
  `apply_copy` 的 `contents` 保护
- `crates/yu-render/soft/src/render.rs` `base_texture`

## 7. 后续 AI 注意（重构触发器）

若再次出现同类“命令时刻 vs 帧末回放”问题（白屏、hover 上一张图、旧
override/快照串图），**不要继续加 `target_live`/provenance 特判**：
先读 `docs/tasks/silent-gaps/task-04-layer-epoch-refactor.md`，评估
LayerEpoch 第一阶段重构。当前修复已覆盖已验证路径，未触发前不重构。
