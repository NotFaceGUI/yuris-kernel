# DIV-GFX-087 · 快照尺寸缺失:组快照按视口、资产快照按晚到资产头回填

- 状态:**fixed(no-manual)**(2026-11-15)
- 取证对象:v474 商业样本 `<0.474 旧运行时样本>` 实跑 trace;soft/wgpu
  `group_surface_size`;官方 0.495 行为按层矩形 `+0x78/+0x7c`
- 相关:DIV-GFX-024(资产尺寸 provider)、DIV-GFX-061(快照/过渡)、
  DIV-GFX-086(合并落点)

## 现象

实跑日志出现 6 条:

```text
gfx.info.unsupported  @ es_sprite.yst:SYSTEM@6519 CGINFO
gfx.info.unsupported  @ es_sprite.yst:SYSTEM@6520 CGINFO
  快照纹理 76 / 80 尺寸未记录(源层无尺寸元数据)
```

脚本用 `CGINFO[SX/SY]` 做立绘/转场的裁剪与缩放布局。`LET` 目标在查询
失败时保留旧值(通常 0),后续 `es.SP.SIZE` 会按 0 计算源矩形/目标尺寸。

## 证据链

### 1. 组快照的真实表面尺寸 = 组尺寸或视口

soft / wgpu 同一实现:

```text
group_surface_size(frame, id) =
  layer.group_size()            // CG 显式 SX/SY
  .unwrap_or((viewport.w, viewport.h))
```

但 `copy_snapshot_size` 原来只给**主窗口组**回退视口;子组(无显式尺寸)
→ `snap_size=None` → `snapshot_sizes` 无条目 → `CGINFO` 报错。

### 2. 资产快照的尺寸晚到

`COPY2` 时若 `asset_sizes` 尚无该键(VM 还没解析图像头),同样不写
`snapshot_sizes`,但 `snapshot_files` 已记录资产键。VM 的 `CGINFO` 回退
路径会解析资产头并 `note_asset_size`,随后重试;旧 `query_layer_size`
对 `TextureRef::Snapshot` 只查 `snapshot_sizes`,不查
`snapshot_files → asset_sizes`,所以重试仍然失败。

## 修法

- 组源:显式尺寸 → **视口**,主组/子组同口径;
- 新增 `snapshot_regions`:COPY 区域随快照保存;
- `set_asset_size`:资产尺寸晚到时,给引用该资产、尚无尺寸的快照回填
  显示矩形(局部区域按 `resolve` 含端点裁剪,并补 `snapshot_origins`);
  **0×0 不回填**,避免把失败尺寸钉死后挡住后续真实尺寸;
- 新增 `snapshot_sources`:`COPY2` 自另一个快照时记录源快照;
  `snapshot_display_size` 沿源链递归解析尺寸(派生快照没有资产键);
- `query_layer_size` 的 Snapshot 分支统一走 `snapshot_display_size`
  (自身尺寸 → 资产键 → 源快照链),覆盖同一 tick 内 COPY → CGINFO 的时序;
- 裁剪函数抽成 `snapshot_region_display`,与 `copy_snapshot_size` 共用。

## 回归

```text
cargo test -p yu-gfx --test info_query late_asset_size_backfills
  → late_asset_size_backfills_snapshot_size / late_asset_size_backfills_partial_snapshot_block
    2 passed
cargo test -p yu-gfx --test info_query subgroup_copy
  → subgroup_copy_snapshot_reports_viewport_size  1 passed
cargo test -p yu-gfx --test info_query late_zero
  → late_zero_asset_size_does_not_poison_snapshot  1 passed
cargo test -p yu-gfx --test info_query snapshot_of_late
  → snapshot_of_late_snapshot_resolves_size_through_source_chain  1 passed
cargo test -p yu-gfx
  → 全部 19 组 test result ok(含既有 main_group/partial/0×0 用例)
```

- `partial_copy2_zero_source_size_keeps_zero_rect` 仍保持 0×0 矩形(官方
  矩形为 0,不是“无尺寸错误”)。
- <0.474 样本> 的 6 条运行期诊断应由本修复消除(待实机复验)。
