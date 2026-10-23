# H-P2-04 · 宿主每帧同步 `Vm::text_state()` 到 `GfxState`(yu-cli 一行)

- 状态: **待主人裁决**(`yu-cli` 属其他任务线,本任务未改)
- 来源: 任务 P2 §M-A/§M-D;`GfxState::sync_text_state` 的契约注释
  (`yu_rt::Vm::text_state` 已标注"宿主/gfx 桥读取")
- 影响面: `yu-cli::engine::Engine::tick_inner`(以及任何自建宿主)

## 背景

`VmEvent::Text` 只带 `bytes`,不带 `_TEXT_FLAG`。自动文本层(`TEXT[ID]` +
`WORD`/`_`/`RETURNCODE`)的逐字宽度只能来自 VM 的 `_TEXT_FLAG`
(`Vm::text_state()`)。`TEXT[CLEAR]` 在 VM 内部执行,**没有事件**;同样只能
经快照传播。

ERIS 正文(`CGACT[TEXT]`)不依赖快照;但:

- 其它游戏/最小脚本走自动文本层;
- `TEXT[CLEAR]` 的跨帧持久语义(DIV-GFX-034)需要清空信号。

## 建议的改动(1 处,3 行)

`crates/yu-cli/src/engine.rs::tick_inner`,在 `vm.tick_at(...)` 之后、
`state.borrow().frame()` 之前:

```rust
let snapshot = vm.text_state();
self.state
    .as_ref()
    .expect("engine started")
    .borrow_mut()
    .sync_text_state(&snapshot);
```

(若将来 H-P2-02 落地,再按 `output_mode` 调 `set_text_reveal`。)

## 为什么本任务不做

- 任务书 §0 明确 `yu-rt` / `yu-cli` 不碰;
- `yu-gfx` 的 API 是加法式,不接线时行为 = 旧行为(自动文本不显示),
  不会破坏现有功能;
- 已用 `crates/yu-render/*/examples/text_probe.rs` 验证接线后的可见结果。

## 回归(接线后跑)

```bash
cargo run -p yu-cli -- <project-eris>   # 进本篇后文本可见
# 或 headless:
YURIS_ENTRY_LABEL=RP_101 cargo run -p yu-backend-soft --example text_probe -- 130
```
