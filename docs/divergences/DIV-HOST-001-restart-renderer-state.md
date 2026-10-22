# DIV-HOST-001 · `Engine::restart` 不重置渲染器状态,跨重启快照/后缓冲泄漏

- 状态: open
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

## 修法(二选一,需裁决)

1. **`reset()`**:两个后端各加 `fn reset(&mut self)`,清 `last_screen`/
   `backbuffer`/`snapshots`/`image_cache`/`uploaded_overrides`;
   `Engine::restart` 在 `boot()` 之后调用。假设:引擎重启 = 新设备。
2. **代际隔离**:`CopyId` 带上重启代际(`CopyId { epoch, seq }`),后端
   按代际丢弃旧快照。改动更大,但不需要假设“重启 = 新设备”。

倾向 1(与 `restart` 现有的“重建 `Vm`/`GfxState`”语义一致)。

## 回归

- 需要一条用例:跑到含 COPY 的帧 → Restart → 再跑到同帧 → 两帧哈希相同;
- 现有可能覆盖不到:`cases/until-visible.json` 只跑到首个可见帧(首个
  可见帧的 COPY 恰好是 A1 的失败路径)。
