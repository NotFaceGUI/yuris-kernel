# yu-render-core

两个渲染后端共享的**数据契约与平台设施**。这里不放任何后端行为:soft 的
`apply_*` 像素算法、wgpu 的管线与目标管理都留在各自 crate。

## 负责

| 模块 | 内容 |
|---|---|
| `surface` | `Surface`(RGBA8 + 尺寸)、`blit` / `blit_rgba` |
| `report` | `RenderReport`、`SkippedOp`(按 kind/reason/target 聚合)、`RenderPhases`(阶段微秒) |
| `texture` | `TextureStore` 资产解码与探测序、`Texture`、`AssetIssue` |
| `text` | `TextComposer` 与两条文本路径共用的栅格化/合成,`ComposedLayer`、增量脏区 |
| `scale` | 共享区域重采样原语(GPU 后端只在 CPU 读回路径使用) |
| `png` | RGBA8 → PNG 确定性编码;`png_libpng` 仅 Android |
| `cache` | 图像加工缓存键(与后端无关的纯数据) |
| `tuning` | `RenderTuning` / `PngBackend`:预取、缓存预算、文本 GPU、PNG 后端等运行参数 |

关键契约:

- `RenderReport` 参与 soft / wgpu 逐计数对账,必须 `Eq`;运行时耗时拆在
  `RenderPhases`,不得混入报告。
- `SnapshotSource` 是 `SAVE[ID]` 命令时刻读回快照的按需来源,由后端实现;
  引擎注入 `GfxCapture`。
- `RenderTuning` 是引擎参数(`EngineOptions.tuning`),渲染层只读字段,
  不再读 `YURIS_*` 环境变量。

## PNG 解码

- 桌面默认 `image`(纯 Rust 解码;x86 上比 libpng 的标量 Paeth 快);
- Android 可选 `libpng`(官方 C 库,arm64 NEON 反滤波)。两者输出逐字节
  一致,由 parity 测试守卫;选择入口是 `PngBackend` / `--png-backend`。

## 不负责

- 窗口、GPU 设备、上屏(归宿主与 `yu-backend-wgpu`);
- 渲染语义(`yu-gfx`)与任何后端像素算法;
- 游戏数据读取(`yu-vfs`)。

## 验证

```bash
cargo test -p yu-render-core
cargo check -p yu-render-core --target aarch64-linux-android   # libpng 分支
```

`tests/png_libpng_parity.rs` 在 Android 目标上对拍两套 PNG 解码;桌面上
`png_libpng` 模块不参与编译。

边界与证据:`docs/architecture/backends.md`、`docs/render-parity.md`;
性能相关决策见 `PROGRESS.md` 成果 PERF-RENDER / PERF-VM-TRIM。
