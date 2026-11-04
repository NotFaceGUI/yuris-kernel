# H-P2-04 · 宿主每帧同步 `Vm::text_state()` 到 `GfxState`(yu-cli 一行)

- 状态: **fixed(2026-11-04;主人裁决:不重构,按现有实现统一语义)**
- 来源: 任务 P2 §M-A/§M-D;`GfxState::sync_text_state` 的契约注释
  (`yu_rt::Vm::text_state` 已标注"宿主/gfx 桥读取")
- 影响面: `crates/yu-engine/src/engine.rs::tick_inner`(以及任何自建宿主)

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

## 落地(2026-11-04)

- `TextState` / `TextSnapshot` 增 `layer_id: Vec<u8>`(权威层键)与
  `clear_seq: u64`(CLEAR 代数);`Vm::text_layer_id: String` 删除,
  `op_text` 直接保存 `TEXT[ID]` 原始字节。
- `Engine::tick_inner` 在 `vm.tick_at(...)` 之后、`frame()` 之前同步:
  `sync_text_state(&vm.text_state())` + 自动模式 `set_text_reveal(Some(reveal_chars))`。
  快照在 VM 输入同步后取(避免 `vm` 借用跨到 `self.sync_input()`)。
- `GfxState::sync_text_state`:
  - 层 ID 经 `path::normalize`(`TEXT[ID]` 与 `CG[ID]` 同一套路径语义);
  - 只在 `clear_seq` 变化时清显示层;**空 `bytes` 不再隐式当 CLEAR**;
  - `text_layout_now` 只排当前层(旧实现取 BTreeMap 第一个非空层)。
- 回归:`cargo test -p yu-engine --test text_sync`(2 项):
  - `engine_manual_mode_does_not_draw_auto_text_layer`
    (2026-11-04 实机回归:ERIS 手动模式左上角重复正文);
  - `engine_auto_mode_reveals_and_clears_text_layer`
    (自动层 reveal + CLEAR,含点击推进)。
- **模式门控**:`Engine::tick_inner` 只在 `output_mode == 1` 同步自动层;
  手动模式 `clear_text()`(官方 `FUN_004046fc` 对手动模式直接清+恢复,
  不调用 `FUN_00404914`)。`text_probe` 仍强制同步,是合成诊断探针,
  不代表官方手动路径。
- 已知分歧(已登记 `DIV-GFX-075`):官方 `+0x38` 是**待显示行缓冲**,
  行完成/手动恢复时清空且不清图层表面;内核为累积显示缓冲。`TEXT[CLEAR]`
  的“清显示层”是内核近似,不是官方像素语义。自动模式页/行表面、折行
  与每字符样式快照仍未对齐(详见任务书 C1/C2/C4/C6/C7)。
