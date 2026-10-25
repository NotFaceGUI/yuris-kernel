# DIV-GFX-062 · wgpu 后端快照源解析必须读冻结 CPU 镜像(演出1 黑屏)

> 状态:`fixed`。症状「wgpu 起游戏,进入演出1 后闪一下随即全黑」。
> 软后端(soft)同场景正常 —— 差异只在 `resolve_cpu_source` 取像素的路径。

## 1. 症状与观测

`cargo run -p yu-host-desktop --example scenario_probe -- <project> \
 target/tmp/case-dir1real.json --backend wgpu`

| 帧 | soft SHA-256 | 修复前 wgpu | 修复后 wgpu |
|---|---|---|---|
| 760 | `972a15c9…` | `d8d9f702…`(skip×1) | `972a15c9…` |
| 900 | `9584b2d6…` | `a7aff0f8…`(纯黑,skip×1) | `9584b2d6…` |
| 1200 | `184568aa…` | `80556fa6…`(skip×1) | `184568aa…` |

修复前 `probe_skips` 汇总:

```text
== 逐帧 skip 汇总(按 target 分)== backend=Wgpu
  x1435    f740..f2775   image:gfx.imageop.source.missing
      target=:/W0/SPL201
```

同帧 `SPL201` 图层状态:`vis=true a=256 size=801x601 tex=SolidBlack1x1`,
即「图层可见、尺寸对、纹理是那张 1×1 黑」—— 背景层被黑图占满。

## 2. 根因

ERIS 的背景/按钮快照链是**同一帧内**三步:

```text
CG[ID="SPLBUF401" FILE="cg/bg/bg_030"]     ← 真背景进 401
COPY[ID="SPLBUF401" ID2="SPLBUF201"]        ← 快照 CopyId(345)
CGEND[ID="SPLBUF401"]                       ← 源层立刻删除
CGACT[SIZE ID="SPLBUF201" ID2="SPL201" ...] ← 用快照缩放到可见层
```

`COPY` 的源层在同一帧稍后就被 `CGEND` 删掉。两个后端对此的取像素方式不同:

| 后端 | 快照像素来源 | 源层已删时 |
|---|---|---|
| soft | `apply_copy_op` 在命令段把像素写进 `snapshots: BTreeMap<CopyId, Surface>` 并冻结,之后只读该表 | 无影响(不回看源层) |
| wgpu(旧) | `resolve_cpu_source` 按 `CopyOp.source` **惰性重建**,递归解析源层 | 找不到层 ⇒ `gfx.imageop.source.missing` |

wgpu 端其实已经在 1.4/1.6 段用 `capture_copy_mirror` 捕获了同样的
`snapshot_cpu` 镜像,但 `resolve_cpu_source` 的快照分支**没有先查它**,
而是直接去走惰性递归。于是冻结镜像白存,断链照旧。

`gfx.layer.missing ×1301`(`CGEND` 删除不存在的 `SPL2xx`/`SPLBUF2xx`)
是同一现象的另一面:这些层在 wgpu 路径下从未被成功创建/留存。

## 3. 修复

`crates/yu-render/wgpu/src/passes.rs`:

1. `resolve_cpu_source` 增加 `targets: &TargetSet` 参数;
2. `TextureRef::Snapshot(copy_id)` 分支**先查 `targets.snapshot_cpu(*copy_id)`**,
   命中即返回冻结镜像;未命中才回落到原有的惰性递归(保留给组源等
   CPU 镜像不可得的组合);
3. `apply_image_to_override` 同步接收 `targets`,把调用链贯通。

语义与 soft 的 `apply_copy_op` / `layer_surface` 一致:快照在命令时刻
冻结,之后源层生死与快照内容无关。

## 4. 验证

```text
scenario_probe --backend wgpu,case-dir1real 子集(700/760/800/900/1000/1200)
  修复前: skip_top = [image:gfx.imageop.source.missing ×1],f900 起纯黑
  修复后: skip_top = [],六帧 SHA 与 soft **逐位相同**
probe_skips --frames 1000 --wgpu
  修复前: x59  f740..f798  image:gfx.imageop.source.missing  target=:/W0/SPL201
  修复后: (无 skip)
cargo test -p yu-gfx -p yu-backend-soft -p yu-backend-wgpu
  全绿(含 parity_fixes 23 条、soft lib 43 条)
```

## 5. 残留

- 「惰性递归」分支保留给 `CopySource::Group` 等 CPU 镜像不可得的情形;
  若后续发现组源快照也需冻结,应把镜像捕获扩展到 `capture_copy_mirror`。
- 本档案只覆盖**渲染后端差异**;`CGINFO[TEX]` 与 `FILEINFO SIZE/RELATION`
  的未实现是独立台账项,不在此档案内。
