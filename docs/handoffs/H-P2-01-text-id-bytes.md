# H-P2-01 · `VmEvent::Text.id` 是 `String`,CP932 非 UTF-8 层 ID 丢失

- 状态: **fixed(方案 B,2026-11-04;主人拍板 `id: Vec<u8>`)**
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

## 落地(方案 B,2026-11-04)

- `VmEvent::Text { frame, id: Vec<u8>, bytes }`;`TextState.layer_id: Vec<u8>`;
  `op_text` 不再 `from_utf8`,直接保存脚本字节。
- 官方图层解析:先 `FUN_004141e4` 路径规范化(相对 ID → `:/W0/<ID>`),
  再 `FUN_00414559` MurmurHash2 + **逐字节**比较;未命中走 `0x4538a7`
  错误路径并返回 -1(不自动建层)。gfx 的 `text_layer_path` 复刻前者。
- `yu-gfx`:事件与快照的层 ID 都走同一路径规范化;`LayerId(Vec<u8>)`
  字节精确。
- `yu-trace`:`Payload::VmText.id` 改 `Vec<u8>`,JSON 用 `bytes_json`
  (每字节一个 `\u00xx`,与 `bytes` 同一可逆编码);
  `SCHEMA_VERSION` 4 → 5。旧 trace 按现有协议以 `trace.schema` 拒绝;
  `crates/yu-trace/tests/golden/synthetic_frame.jsonl` 只改 schema 号
  (该 golden 无 `text` 事件),无行为差异。
- 回归:
  - `fixture::text_id_keeps_cp932_bytes_without_utf8_conversion`(事件/快照字节);
  - `text_pipeline::non_ascii_text_layer_id_targets_exact_layer`
    (CP932 `0x82A0` + **非法 CP932** `0xFFFE`,证明无解码、字节透传);
  - `yu-trace` roundtrip 用 `id: vec![0x82,0xa0]`。
- 语料缺口:`<project-eris>` 141 脚本 `TEXT[ID]` = 0 次;无商业包语料。
  上述用例均为**合成样本**;REA 已证明字节语义本身。
