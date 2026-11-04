# H-P2-02 · VM 侧 reveal / 已显字符数查询 API(M-D)

- 状态: **fixed(2026-11-04;官方表面/页语义见 DIV-GFX-075)**
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

## 落地与勘误(2026-11-04)

上述“未取证”三项已逐指令复核,结论与旧假设不同:

1. **`+0x6c` 不是“已显字符数”**,是 `_TEXT_FLAG` 消耗游标(含
   `C/R/P/E`);`+0x68` 才是文本字节游标。字符数 = 前缀里 `'1'/'2'` 的
   个数。
2. **时间基**:`FUN_00404c0c` `0x404c21` 调 `timeGetTime()`(毫秒),
   `now - last`/`last += delay` 均按 u32 回绕;一行的第一个字符立即显示
   (`+0x68 == 0` 门)。内核用 `tick_at(now_ms)` 同单位,不读墙钟。
3. **WAIT 全/半角不同权**:`WORD 0x458e78 idiv`、`_ 0x45697f sar` ⇒
   全角 = `WAIT`,半角 = `WAIT/2`;延迟在字符追加时写入 `+0x48` 表。
4. **CLICK/自动恢复**:`FUN_004046fc` 遇 `'C'` 停在 `+0x35=1`;无 `'C'`
   的行(如 `AUTOCLICK=0`)在游标消费完 `+0x64` 后直接恢复
   (`0x404818`)。内核 `text_reveal_complete` 复刻后者。
5. **手动模式不推进 auto reveal**:`output_mode != 1` 时
   `FUN_004046fc` 直接清+恢复,不调用 `FUN_00404914`。Engine 只在
   `output_mode == 1` 同步自动层并传 `Some(reveal_chars)`;手动模式
   `clear_text()`,不调用 `set_text_reveal`(旧文里的
   `output_mode == 0 → Some` 是反的,已勘误)。
6. **CLEAR**:`0x4638e4` 清 `+0x60/+0x64/+0x68/+0x6c` 与十块堆数组,
   **不清图层表面**;内核显示缓冲以 `clear_seq` 显式清层,属近似。

实现:
- `TextState` 增 `reveal_flags/reveal_bytes/line_reveal_chars/committed_chars/
  delays/last_char_ms/timer_reset/skip_mode`;
  `advance_reveal(now_ms)` 复刻 `FUN_00404c0c`/`FUN_00404914`;
- `TextSnapshot` 透出 `reveal_flags/reveal_bytes/reveal_chars/skip_mode`;
- `Vm::tick_at` 只在 `Scheduler::text_waiting()` 时推进;
- 调度器 gate 增 `text_reveal_complete`,自动模式无 `'C'` 时无需输入;
- `finish_line` 提交当前显示段(点击/跳过/行尾);
- Engine 自动模式每帧 `sync_text_state` 后 `set_text_reveal`;手动模式
  `clear_text()`。

回归:
- `text::tests::reveal_uses_full_half_delays_and_resets_on_clear`,
  `text::tests::manual_mode_does_not_advance_reveal`;
- `fixture::text_reveal_advances_by_wait_and_full_half_delay`,
  `fixture::text_reveal_autoclick_off_resumes_without_input`;
- soft `synced_text_flags_rasterize_glyphs`(reveal=0 不画);
  wgpu `text_layout_frames_match_soft_exactly`(reveal=1 两端各 1 glyph)。

未对齐(登记 `DIV-GFX-075`):官方行完成清待显示缓冲、`'P'` 清页、
窗口宽度折行/标点禁则、每字符样式快照。内核当前为累积显示缓冲 +
`layout_text` 的 flag 换行。
