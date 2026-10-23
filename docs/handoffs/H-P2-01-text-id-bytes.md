# H-P2-01 · `VmEvent::Text.id` 是 `String`,CP932 非 UTF-8 层 ID 丢失

- 状态: **待主人裁决**(跨 crate 契约变更,本任务不动 `yu-rt`)
- 来源: `crates/yu-rt/docs/coverage-matrix.md` §5 / DIV-GFX-021 / P2
- 影响面: `yu-rt::event::VmEvent::Text`、所有事件消费者(`yu-gfx`、`yu-trace`、`yu-cli`)

## 现状

```rust
VmEvent::Text { frame: u64, id: String, bytes: Vec<u8> }
```

`TEXT[ID=$x]` 是 CP932 字符串。VM 侧 `op_text` 用
`std::str::from_utf8(&b)` 转换;非 UTF-8 时**清空 id 并挂号**
(DIV-GFX-021)。于是 `TEXT[ID="あ"]` 之后的文本会落到默认层,渲染目标层
可能错位。

## P2 的处理(不改变契约)

- `yu-gfx::TextRun.id` / `TextDraw.target` 是 `LayerId(Vec<u8>)`,模型层
  不丢字节;
- 但事件里已经拿不到原始字节(VM 清空了 `String`),gfx 只能记空层 ID;
- 已有测试 `text_pipeline` 只覆盖 UTF-8/ASCII 层 ID。

## 建议的契约变更(二选一)

1. **加字段**(破坏模式匹配,但语义最清晰):
   ```rust
   VmEvent::Text { frame, id: String, id_bytes: Vec<u8>, bytes }
   ```
   `id_bytes` 恒为 CP932 原字节;`id` 保留给现有消费者(可能为空)。
2. **改类型**:`id: Vec<u8>`;所有消费者改用 `LayerId::new(id)`。
   `yu-trace` 的 JSON 需要同步(现在写 String)。

需要同步改的点:`yu-rt::event`、`yu-rt::json`、`yu-gfx::state`、
`yu-trace`(事件哈希/JSON)、`yu-cli`(诊断/WS)。工作量约半天 + golden
重生成。

## 判据(何时必须做)

- 出现任何真实脚本 `TEXT[ID=<非 ASCII>]`;
- 或自动文本层在非 UTF-8 层 ID 上出现"文字画到默认层"的可见 bug。
