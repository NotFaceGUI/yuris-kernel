# P5 交接件 · engine/server 挂钩点

> 本文件只列**不属于** `crates/yu-trace` / `tools/` / `yu-cli/examples/` 的接线点。
> 归 P1(桌面宿主/serve 防线)与 P4(engine/case)。P5 已把 API、测试、示例、
> 官方采集与 diff 工具全部落地;下面是宿主侧一行式调用。

## H-P5-01 · panic hook / Fatal Diag → `dump_on_fault`

### API(P5 已实现)

```rust
use yu_trace::{EventRecorder, FaultInfo, RingConfig};

// 1) 启动后、第一 tick 前:
recorder.enable_ring(RingConfig::default(), Some(header))?;
//    或 VmTrace::enable_fault_ring(config, header)
//    header 必须是 header_from_vm_with_clock(...),producer.sample_sha256 由宿主写入。
//    ring 必须在第一 tick 前挂上,否则 dump 标注 replayable=false。

// 2) 任意线程(panic hook / catch_unwind / Fatal Diag):
recorder.dump_on_fault("target/tmp/fault/fault.jsonl", FaultInfo::panic(detail))?;
//    FaultInfo::caught(detail) / FaultInfo::fatal(&diag)
```

- 写两份文件:`<path>`(schema v4 trace)+ `<path>.timeline.json`;先写伴生文件、
  最后 rename JSONL(提交标记),不写仓库内路径。
- `EventRecorder::clone()` 共享 ring 槽位;hook 里 clone 一份,不要持有 `&mut Vm`。
- ring 默认 300 tick / 单 tick 8 MiB / 总 64 MiB;淘汰与截断都有计数
  (`evicted_ticks` / `diags_dropped` / `displays_dropped` / `timeline_truncated`)。

### 桌面宿主接线(P1)

- `crates/yu-platform/host-desktop` 的 panic 防线(或 `yu-cli::Engine` 的
  `catch_unwind` 包裹处)安装 `std::panic::set_hook`;hook 内只做
  `dump_on_fault` + 调用原 hook,不再 panic。
- 每 tick:宿主已有 `engine_tick`;`VmTrace` 路径已自动 `begin_tick/record_timeline`,
  直接走 `enable_fault_ring` 即可。若走 `yu-cli::Engine`(不是 `VmTrace`),
  需要把 `EventRecorder::begin_tick(first_frame, now_ms)`、`record_timeline_entry`
  与 `note_tick_frames(last_frame)` 接到 tick 边界(一行式,顺序见
  `crates/yu-cli/examples/fault_dump.rs`)。
- 参考实现:`crates/yu-cli/examples/fault_dump.rs`(真实 `data.ypf` + 合成 panic +
  离线 replay)。该 example 的 `yu-trace` 是 **dev-dependency**,不改变
  architecture §2 的“旁挂、非运行期依赖”。

### Fatal Diag(P4)

- 运行核/域宿主产生 Fatal `Diag` 时,在把它送进 `LogSink` 之前/之后调用
  `FaultInfo::fatal(&diag)`;一条 Fatal 只 dump 一次(宿主自己去重)。

## H-P5-02 · `case --trace` 开关(case.rs,P4)

- `cases/*.json` 增加可选 `"trace": "<relative path>"`;`yu-cli case` 在用例
  开始前 `enable_fault_ring`,结束时 `dump_on_fault(path, FaultInfo::caught("case end"))`
  或正常 `finish`。
- 产物只写 `target/tmp/case-traces/`;路径不得含绝对路径进 trace 内容
  (文件名可以,内容不行)。
- 现有 `case` 的 `pulse`/输入时间线已在 `Engine` 内,`record_timeline_entry` 直接复用。

## 附:schema v4 的宿主注入(必须由 P1 填)

P5 为回放闭包把两个宿主注入放进 header(`boot.host`):

| 字段 | 来源 | 缺省 |
|---|---|---|
| `window_init_pos` | `yscfg.dat` 第 10 个 int(引擎设置,不在 data.ypf) | 0(ERIS 一致) |
| `rng_seed` | `RANDOM` 的 MT19937 种子(宿主注入) | 5489 |

宿主若注入了非缺省值,**必须**用同一个值重建 `BootConfig`,否则
`BootSummary::to_config` 不校验这两个标量(它们没有摘要,直接进 header);
回放会用 header 的值重跑,宿主侧显示值不一致属于宿主契约缺口。

## 附:官方采集器 schema 变更

`tools/official_trace/*.py` 已写 schema 4 并在 `boot.host` 写
`window_init_pos=0` / `rng_seed=5489`(官方外部不可观测,按缺省占位;
屏幕级用 `CodecSample`、事件级用 `Semantic` 时与内核缺省一致)。
v3 旧 trace 不再被解析,重采即可。
