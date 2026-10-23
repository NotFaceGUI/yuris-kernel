# H-P2-02 · VM 侧 reveal / 已显字符数查询 API(M-D)

- 状态: **待主人裁决**(只读 API,加法式;本任务未改 `yu-rt`)
- 来源: 任务书 P2 §M-D / DIV-GFX-033
- 影响面: `yu-rt::Vm::text_state` 快照、`yu-gfx`、两个后端

## 需求

`TEXT[OUTPUTMODE=0]`(手动)与 `TEXT[WAIT]` 的逐字显示需要"当前应显示的
字符数"。VM 的 `TextState` 已有 `wait_ms`/`output_mode`/`click`,但没有
已显字符数;引擎侧是 `+0x6c`(cursor,按 `_TEXT_FLAG` 任务逐字节推进;
REA `FUN_004046fc`)。

## P2 已就绪的接口

- `GfxState::set_text_reveal(chars: Option<usize>)` — 唯一截断入口;
- `TextRun.reveal` / `TextLayoutFrame.reveal` — 传给后端;
- `TextComposer::paint_layout` 按 `take(reveal)` 只画前 N 个字形;
- 后端**不自建计时器**(禁止用墙钟猜节奏)。

## 需要的 VM 改动(建议)

```rust
// crate::text::TextState / TextSnapshot 增加:
pub reveal_chars: usize,   // 引擎 +0x6c 的已显字符数(按 flag 任务计)
```

并在 `TextSnapshot` 里透出;宿主每帧:

```rust
let snap = vm.text_state();
let reveal = (snap.output_mode == 0).then_some(snap.reveal_chars);
gfx.set_text_reveal(reveal);
gfx.sync_text_state(&snap);
```

## 未取证部分(不要实现)

- `TEXT[WAIT]` 的"每字符 N ms"计时由谁推进(引擎 `+0x04` 与帧时钟的关系)
  未逐条复核 ⇒ 不要用墙钟伪造;
- `CLICK`/`AUTOCLICK` 的让出与 reveal 清零的交互;
- 手动模式下 `TEXT[CLEAR]` 与 reveal 的关系。

在上述三项取证前,`reveal` 保持 `None`(全显示);P0 场景
(`SCENARIO_EPILOGUE`/`RP_101`)的输出模式是 1(自动),不受影响。
