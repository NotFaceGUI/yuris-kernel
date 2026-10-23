# yu-trace

旁挂对拍基建:把内核的可观测行为录成确定性 JSONL,支撑回放与 diff。
**旁挂定位** —— 可依赖 `yu-log`/`yu-vfs`/`yu-rt`,但任何 crate 都不得依赖它。

## 四条边界(当前状态)

| 边界 | sub/kind | 状态 |
|---|---|---|
| ① vfs 读 | `vfs/*` | 已实现:`TracingSource` 包装任意 `DataSource`,记录 `open/exists/list` 事实与结果身份 |
| ② 事件流 | `vm/*` | 已实现:`EventRecorder` 拉 `Vm::take_events`,穷尽 match(新增变体即编译失败) |
| ③ 帧描述 | `frame/frame_description` | 已实现:调用方传入规范化 display JSON 或摘要,本 crate 只做协议校验/内容寻址,不硬绑 yu-gfx 字段 |
| ④ 平台命令 | `platform/platform_call` | **占位并显式拒绝**(等平台后端) |

## schema v4(producer-neutral + 回放闭包)

- 协议唯一定义:crate 根 `SCHEMA.md`;字段增删必须递增 `schema`,
  旧版本文件直接拒绝、不迁移。
- header 携带 `producer`(kernel/official + 版本 + 样本 sha256)与
  `clock`(逐 tick `now_ms` 序列);`boot.host` 含 `window_init_pos` /
  `rng_seed`(宿主注入且影响脚本行为,不记录就无法重放)。
- 跨 producer 语义 diff 只投影 `codec`/`boot`/`input`/双方
  `producer.sample_sha256`,内核版本/人类 prose/producer 名不参与。
- `seq` 是同一 producer 内的录制序;跨 producer 对齐用
  `(sub, kind, frame, ordinal)`。
- 内容身份 = 本地 SHA-256(`hash`),增量版见 `Sha256`(故障快照用)。
- 帧描述有显式字段投影:`FrameFieldPolicy::Screen` 只比较
  `frame/level/display`,不拿屏幕级占位计数冒充实测值。

## 故障快照(fault dump)

- `EventRecorder::enable_ring` / `VmTrace::enable_fault_ring`:保留最近 N
  tick 的 {命令摘要, Diag(含 Unimplemented), 输入, frame_description};
- `dump_on_fault(path)`:panic hook / catch_unwind / Fatal Diag 时原子写
  `<path>`(schema v4 trace)+ `<path>.timeline.json`(工具级伴生文件,
  含原始输入时间线、clock、每 tick 摘要);
- `FaultDump::read` 校验 `trace_sha256` 与 header input 摘要;
  `FaultDump::replay` 离线重演到故障 tick,逐 tick 精确比较运行核观测,
  任何差异 = `trace.replay`。
- 示例:`crates/yu-cli/examples/fault_dump.rs`(真实语料 + 合成 panic)。

## 回放与 diff

- `replay`:按记录重放 vfs 读与事件流,校验往返一致;
- `diff`:两份 trace 的投影对齐报告(首分歧位置/字段/期望/实际);
- 官方引擎侧采集器不在本 crate:见 `tools/official_trace/`
  (屏幕级 + 事件级 L1 + 任务相位采样)。

## 确定性

输出中不存在墙钟、线程、HashMap 迭代序、绝对路径;`frame` 只来自注入值。

## 验证

```bash
cargo test -p yu-trace
YURIS_REQUIRE_SAMPLES=1 cargo test -p yu-trace --test real_sample
cargo run -p yu-trace --example diff_traces -- --help
```

进度与证据:`PROGRESS.md` 成果 D/19/H1;协议变更史见 `SCHEMA.md` 文末。
