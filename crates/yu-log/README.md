# yu-log

错误 + 日志一体包,所有 crate 的错误唯一出口。零第三方依赖。

## 负责

- [`Diag`](src/lib.rs):结构化错误 —— 稳定命名空间错误码 + 严重级 + 边界归属 +
  位置上下文 + 因果链。错误是数据,不是字符串。
- 严重级语义对齐官方:`Fatal` / `Recoverable` / `Unimplemented`(已知语义未实装,
  挂号留痕)/ `Info`。
- 稳定码表(`src/code.rs`):各子系统码域与唯一性有单测约束。
- [`LogSink`](src/sink.rs):显式注入,无隐藏全局(确定性不变式 A3);
  `JsonlSink` / `MemorySink` / `NullSink`。JSONL 输出确定性、可逐字节往返。

## 不负责

- 不做任何 IO 策略决策(落盘位置、轮转由调用方装配);
- 不承载任何子系统语义;其它 crate 不得自造错误通道或 `println!`。

## 约定

- 新错误码必须进码表并配单测;禁止调用方拼接字符串错误。
- `clippy::result_large_err` 在本 crate 显式放行:`Diag` 的结构化上下文是契约。

## 验证

```bash
cargo test -p yu-log
```

进度与证据:`PROGRESS.md` 成果 A;架构契约 `docs/architecture.md` §8。
