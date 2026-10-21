# DIV-L-001 · script 20 pc237 的 `IF` 分支不同（官方 debug_mode vs 内核默认 release）

- 状态: **fixed**（根因是启动配置注入，不是 VM 语义错误；内核代码未改）
- 优先级: 高（决定标题/场景分支）
- 发现日期 / 发现方式: 2026-10-21 / 官方事件级 L1 trace vs 内核 `emit_kernel_events` 同语料 trace
- 相关任务: 任务书 L（Phase L-3）

## 复现

- 语料: `<project-eris>/data.ypf`（官方 YSCom 0.495 **debug 构建**）
- 官方: `<side-repo>/workbench/project-eris/yu-ris.exe`（sha256 `4f5eefa3…`），
  正常启动到标题，点击「スタート」后采集。
- 内核: `cargo run -p yu-trace --example emit_kernel_events -- … --debug-mode true`
- 锚点: script 20, pc 237, `IF`

## 官方行为

trace 摘录（`target/tmp/l1-probe/run_click2/hits.jsonl`，tick 300）：

```text
60 975 RETURN
20 237 IF
20 238 GOSUB        ← 真分支: GOSUB "es.DebugTask"
5  24  GOSUB
70 359 STR
```

script 20 pc237 的条件窗口（`yst00020.ybn` 解码）：

```text
48 03 00 40 0b 00   → @11
48 03 00 40 c0 08   → @2240
5a 00 00            → >=
即 IF[@11 >= @2240]
```

YSVR 声明（`ysbin/ysv.ybn`）：

```text
slot 11    kind=1 flag=0 file=0  ty=1 dims=[] init=Int(0)
slot 2240  kind=2 flag=1 file=20 ty=1 dims=[] init=Int(1)
```

官方窗口标题（`GetWindowTextA` 原始字节 `81 79 83 66 83 6f 83 62 83 4f …`，
CP932 解码）= **【デバッグモード】**。这是官方运行期的直接证据。

## 内核行为

`debug_mode=false`（默认）时：

```text
20 237 IF  args=[{"slot":0,"value":{"t":"int","v":0}}]   ← 条件为假
20 237 JMP if_false -> 20 239
20 239 IFEND
20 240 WAIT
```

`debug_mode=true` 时：

```text
20 237 IF  args=[{"slot":0,"value":{"t":"int","v":1}}]   ← 条件为真
20 238 GOSUB args=["es.DebugTask"]
```

## 二分记录

- [x] L1 译码一致? 是：条件窗口按 `op + u16 len + payload` 解码为 `@11 >= @2240`，
      两边脚本字节同一来源（`data.ypf`）。
- [x] L2 执行/事件流一致? **配置正确后一致**：`debug_mode=true` 时 IF=1 且走
      pc238 `GOSUB es.DebugTask`，与官方逐条一致。
- [ ] L3 帧层状态一致? 不适用（该分歧在命令分支层）。
- [ ] L4 帧间编排一致? 不适用。

定位结论: 差异来自 **内核 `BootConfig.debug_mode` 默认 false**，而官方引擎运行在
debug 模式；不是 script 20 的 IF 求值/变量读取错误。

## 根因

- 官方窗口标题 = 【デバッグモード】，运行期 debug_mode = true。
- `<project-eris>/save/data/config/projectconfig.txt` 里的 `DEBUGMODE=0` **不是**
  官方运行期实际值（该副本是 exe 的解包产物，引擎实际读取的项目配置/启动方式
  另有来源；以窗口标题与事件流为准）。
- 内核 `BootConfig::default().debug_mode = false`，因此 `@11` 相关分支为假。
  `@11`/`@2240` 的具体来源（全局槽位与 debug 标志的写入路径）尚未逐条取证；
  本 DIV 只对“分支差异由 debug_mode 配置解释”这一结论负责。

## 修法

- 运行期注入：对 debug 构建语料启动内核时必须传 `debug_mode=true`
  （`emit_kernel_events --debug-mode true`；`BootConfig.debug_mode = true`）。
- 官方 trace header 的 `boot.debug_mode` 已改为 true（由 `probe_dispatch.py`
  依据窗口标题写入；`--debug-mode true` 为默认值）。
- **不改内核代码**；`debug_mode` 是宿主/工程配置，不是 VM 语义。

## 回归

- 对拍用例: `diff_traces --order-only --script 60`（见 DIV-L-002）。
- 验证命令:

```text
cargo run -q -p yu-trace --example emit_kernel_events -- \
  --root <project-eris> --out target/tmp/l1-probe/kernel_events_debug12.jsonl \
  --frames 12 --first-frame 4 --last-frame 11 --click-frame 6 --debug-mode true
```

原始结果: `debug_mode=true` 时 script 20 pc237 的 IF 事件实参 = `int 1`，下一条
命令 = `20 pc238 GOSUB "es.DebugTask"`；`debug_mode=false` 时为 `int 0` 并跳到
pc239。该分歧消失。
