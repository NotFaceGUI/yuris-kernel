# DIV-HOST-001 · `Engine::restart` 不重置渲染器状态,跨重启快照/后缓冲泄漏

- 状态: **fixed(AU1,2026-10-22)**
- 优先级: 高(Restart 后首帧含 COPY 时像素直接错)
- 发现日期 / 方式: 2026-10-22 / 代码审查(`yu-cli` + 两个后端)

## 现象

`yu-cli serve` / `bench` 的 Restart 路径(`crates/yu-cli/src/engine.rs`)只重置:

```text
vm / state / timeline / held / cursor / frame /
last_frame / last_surface / last_report / last_render /
tick_history / render_history / events_drained
```

**保留** `Renderer`,`SoftRenderer` / `WgpuRenderer` 内部状态原样继续:

| 状态 | 位置 | 后果 |
|---|---|---|
| `last_screen` | soft `SoftRenderer` | 重启后第一帧的 `COPY ID=:/W0/` 截到重启前画面 |
| `snapshots` | soft | 旧 `CopyId` 仍在 |
| `backbuffer` | wgpu `WgpuRenderer` | 同上 |
| `targets`(snapshot 表) | wgpu `TargetSet` | 旧快照纹理仍在 |
| `image_cache` | 两后端 | 缓存键只含源/参数,不含帧代际 |

叠加问题:`GfxState.copy_seq` 在重启时随新 `GfxState` 归零,新 `CopyId`
会与旧快照同 id;soft 的 `apply_copy_op` 有

```rust
if snapshots.contains_key(&copy.id) { return Ok(()); }   // 快照冻结
```

的早退 ⇒ **新复制命中旧快照,不执行复制**。

## 官方行为

未取证。引擎重启(进程内或进程外)时纹理/快照是否保留没有 trace;
`capture_screen.py` 的每次采集都是新进程,无法回答进程内重启。

## 内核行为

K1 的 Restart 验收只比对了 title 稳定帧哈希(`gfx frame 303`),该帧的
COPY 已结束、快照已稳定;没有覆盖“重启后首帧含 COPY”的路径。因此
现有验收通过不代表重启正确。

## 修法(AU1 已落地)

采用方案 1(`reset()`),与 `restart` 现有的“重建 `Vm`/`GfxState`”语义一致:

- `SoftRenderer::reset()`:清 `snapshots`、`last_screen`、`image_cache`;
- `WgpuRenderer::reset()`:清 `backbuffer`/`backbuffer_gpu`/
  `uploaded_overrides`/`image_cache`,并 `TargetSet::clear_snapshots` +
  `clear_offscreen`(同时 `forget_texture` 作废旧 bind group);
- `Engine::restart` 在 `boot()` 成功后调用 `renderer.reset()`;
- 纹理解析缓存(`TextureStore`)与 GPU 管线保留:只依赖游戏数据,与运行代际无关。

假设写明:**引擎重启 = 新设备**(快照不跨进程保留)。这条没有引擎 trace 证据,
但 `restart` 本来就在重建 `Vm`/`GfxState`,保留旧快照没有任何语义依据。

## 回归(AU1 已补)

`crates/yu-cli/tests/restart.rs`(`#[ignore]`,需真实语料):

```text
参照组(全新 Engine):跑到 3 tick,记录 (frame, RGBA sha256, display sha256)
重启组:先跑到 300 tick(title),restart,再跑 3 tick,记录同一组值
断言:frame / RGBA sha256 / display sha256 全部相等
```

**为什么必须先跑到 title**:只跑 3 帧时重启前后后缓冲都是黑的,泄漏不可见
(第一版用例就是这样,去掉 `reset()` 仍然通过)。改到 title 后,旧 `last_screen`
里有 title 画面,泄漏会让首帧 `COPY ID=:/W0/` 截到它。

另有一条纯单元回归(不依赖样本):
`crates/yu-backend-soft/tests/render.rs::reset_drops_snapshots_so_new_copy_ids_do_not_alias`
—— 去掉 `snapshots.clear()` 立即失败(旧快照 `[255,0,0]` 冻结)。
