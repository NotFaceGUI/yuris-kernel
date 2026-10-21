# 官方 trace 采集器（任务 H/L）

本目录只放**仓库外运行**的采集工具；截图/语料/trace 一律写到仓库外或 `target/`，
不入库。采集器不修改官方二进制、不写游戏文件。

## 1. 当前等级

| 等级 | 状态 | 证据 |
|---|---|---|
| **屏幕级（screen）** | 已达成 | `capture_screen.py` 启动官方 0.495 + ERIS 数据，截取客户区 800×600，写 schema v3 `frame/frame_description`（`level=screen`） |
| **事件级（L1）** | **已达成（窗口级）** | `probe_dispatch.py` 在 `0x4044db` / `0x404547` 两个分派点下只读断点；命中记录 `(script, pc, handler → cmd/name)`，写 schema v3 `vm/command` |

事件级 L1 的**明确边界**（不得外推）：

- 官方侧只观测到**命令分派**；参数求值结果不可观测，trace 里 `args=[]`（不是“无参数”）；
- handler 被多个 opcode 共享时，`cmd`/`name` **显式留空**（见 `command_table.json`），不猜；
- `frame` = 引擎每 tick 计数器 `[0x801268]`（u64），**不等于**内核逻辑帧；
  跨 producer 对拍用顺序投影（§5）；
- 官方任务创建/结束/`WAIT`/跳转事件尚未采集（挂账，见 §6）。

## 2. 屏幕级采集

```bash
python tools/official_trace/capture_screen.py \
  --exe <官方 yu-ris.exe> --cwd <工程根> \
  --out <仓库外输出目录> --frames 6 --interval 0.4 --warmup 8
```

输出：

```text
<out>/frame_0001.png ...       客户区截图(不入库)
<out>/official_screen.jsonl    schema v3 trace(producer=official, level=screen)
<out>/input_timeline.json      帧号 -> 输入(首屏为空)
<out>/manifest.json            引擎/样本 hash、窗口尺寸、等级声明
```

屏幕摘要定义（官方与内核必须一致）：

```text
pixels_sha256 = SHA-256(b"RGBA8" + width:u32 LE + height:u32 LE + RGBA bytes)
display JSON  = {"width":W,"height":H,"pixels_sha256":...}
display_sha256 = SHA-256(display JSON 规范文本,无空白,键序 width/height/pixels_sha256)
```

屏幕级记录的 `pass_count/op_count/executable_ops/skipped_ops` 在引擎外部不可观测，
按 0/默认值记录；**不是实测 0**。`level=screen` 已表达这一点。

## 3. 运行环境与安全

- 64 位 Python（本机 3.12）、Pillow；Windows 桌面会话。
- 采集器是只读观测：启动进程、读内存、按需 `PostMessage`/真实输入；结束时
  `terminate` 引擎。**唯一的内存写是断点字节 `cc` 与恢复原字节**（观测所需）。
- **不做反调试绕过**：旧版本的 PEB `BeingDebugged`/`NtGlobalFlag` 清零已删除；
  H4 复查也未发现 `IsDebuggerPresent`/`CheckRemoteDebuggerPresent`/
  `NtQueryInformationProcess` 导入，engine 在调试器下正常运行。
- 截图不在探针内：断点暂停时 `PrintWindow` 会等待被挂起的 UI 线程而死锁；
  屏幕级截图继续由 `capture_screen.py` 单独采集。
- 截图与 trace 不提交；`manifest.json` 可能含本机绝对路径，只用于复核。

## 4. 事件级 L1

### 4.1 锚点（REA 复核，官方 exe sha256 `4f5eefa3…`）

| 项 | 值 | 证据 |
|---|---|---|
| 命令分派（路径 A） | `0x4044db: call dword ptr [ecx + edi*4]` | Ghidra 反汇编 `ff 14 b9`；`0x8015b0 == 0` 时走这条 |
| 命令分派（路径 B） | `0x404547: call dword ptr [ecx + edi*4]` | 同上；`0x8015b0 != 0` 时走这条（标题实测走 B） |
| 任务步进函数 | `0x404398` | 入口 `sub esp,0x24`；`EAX` = 任务/计数对象 |
| 当前任务 | `[0x80171c]` | 分派前写入；pc 在 `task+0x20`（取指后自增） |
| 脚本 ID | `task+0x3c`（u16） | 命中实测 script 8/20/60/66… |
| 处理器表 | `0x7ace20`，119 项 | 运行时 87 个不同 handler；两个共享组（见下） |
| tick 计数 | `[0x801268]`（u64） | 每 `FUN_004040f8` 一次；实测 ~60/s |
| 有效 UI 点击写点 | `0x45ab63: MOV [0x80157c], AL` | WM_LBUTTONDOWN 命中非透明像素后；trigger 模式用 |
| WM_LBUTTONDOWN 入口 | `0x45aa44` | WndProc `FUN_0045a358 → FUN_0045a3b4` 分发 |

运行时 handler 表（`command_table.json`）：

```text
unique handlers = 87
shared 0x459e90 = 27 slots（变量赋值族 F_*/G_*/S_* 等）
shared 0x4634d8 = 7 slots（默认 stub）
```

共享 handler 的命中：`cmd=null`、`name=""`，并在 `hits.jsonl` 保留
`cmd_candidates`；不按“第一个匹配”猜。

### 4.2 error 299 根因（勘误，假设表终态）

旧版本在标题画面 25 s 内 0 命中，日志出现
`ReadProcessMemory(0x400000/0x401000/0x4044db) → error 299`。逐条假设实验结果：

| 假设 | 实验 | 结论 |
|---|---|---|
| H1 句柄权限 | CREATE_PROCESS 句柄 / `OpenProcess(VM_READ)` / `VM_READ|VM_WRITE|VM_OPERATION` / `ALL_ACCESS` 各读 `0x4044db` | **证伪**：四种句柄均成功读到 `ff 14 b9`；写需要 `VM_WRITE`（旧版本已用 ALL_ACCESS） |
| H2 时机 | 进程创建瞬间 / +1 s / +3 s / +6 s 各读一次 | **证伪**：正确地址在所有阶段都可读；`0x0`/`0x1000` 的 299 只是未映射 |
| H3 附加方式 | DEBUG_ONLY_THIS_PROCESS / 普通启动外部读 / `DebugActiveProcess` | **证伪**：三种方式读 `0x4044db` 都成功 |
| H4 反调试 | REA 查导入 + 运行时 32 位 PEB（class 26）/ DebugPort/DebugObject | **无证据**：无相关导入；debugged 时 `BeingDebugged=1` 但引擎正常运行；未做任何绕过 |
| H5 地址/重定位 | PE 头、模块基址、文件字节 | **成立**：PE32，ImageBase `0x400000`，无重定位目录、`DllCharacteristics=0`；实际 base `0x400000` |

**根因**：旧代码把 `--bp-rva 0x44db` 直接当绝对地址，实际落下的是 `0x44db`
（未映射）；正确地址是 `模块基址 + RVA = 0x4044db`。`ReadProcessMemory(0x44db)`
→ 299，`WriteProcessMemory(0x44db)` → 998，断点从未落下。
旧日志里成功读之后的 `err 299` 还是**未清零的陈旧 GetLastError**——本版本每次
API 调用前 `SetLastError(0)`，并用 `use_last_error=True` 读取。

原始实验输出：`target/tmp/l1-probe/l1_experiments_*.json`（不入库）。

### 4.3 泵验证（Phase L-2）

在 `0x404398` 下断点，标题启动后 **t≈0.68 s 首次命中**，`EAX` 为任务对象；
命中序列覆盖 script 8/10/12/20 等（随后 script 0/5/20/52/60/66/70/71/72）。
说明任务泵在跑，且标题/场景都由同一泵驱动。

分派实测：标题阶段走 `0x404547`（`0x8015b0 != 0` 的分支）；`0x4044db` 是另一条
分支。因此探针**同时**挂两个分派点，而不是只挂一个。

### 4.4 用法

自动输入（引擎必须在**真实前台**；见 §4.5）：

```bash
python tools/official_trace/probe_dispatch.py \
  --exe <官方 yu-ris.exe> --cwd <工程根> --ysc <ysc.ybn> \
  --out-dir target/tmp/official/events \
  --seconds 20 --arm-after 6.5 --max-hits 200000 \
  --trace-out target/tmp/official/events/official_events.jsonl
```

- `--arm-after`：先让引擎自由跑到标题，再挂分派断点，避免把启动期 100 万级
  命令也逐条停下来；
- 自动点击（可选）：
  `--input-at 9 --input click --click-xy 210,281 --input-method sendinput`。
  已布防时 `sendinput` 只检查前台、不调用 `SetForegroundWindow`（调试暂停期间
  该调用会等待被挂起的 UI 线程）；鼠标按住直到引擎 tick 前进再松开，避免
  DirectInput 采样漏掉瞬时限点击。
- **手动触发（推荐）**：默认 `--trigger-rva 0x5ab63`，只挂“有效 UI 点击”写点。
  标题阶段引擎全速运行、不卡；用户点中「スタート」后，探针才自动挂分派断点，
  从转场第一条命令开始记录。

输出（全部写 `--out-dir`，不入库）：

```text
hits.jsonl             逐命中原始记录（含 handler/ecx/tick/线程/歧义候选）
command_table.json     运行时 119 项 handler 表与共享分组
manifest.json          引擎/样本/ysc hash、参数、计数、失败原因、debug_mode 证据
<trace-out>            schema v3 trace（producer=official, sub=vm/kind=command）
```

### 4.5 输入/前台约束（实测）

- 官方引擎的键鼠状态来自 DirectInput 风格的原始按键表（`0x685000/0x685400`
  一族），不是 `GetAsyncKeyState`；`PostMessage(WM_KEYDOWN/WM_LBUTTONDOWN)` 在
  窗口非前台时**不被采纳**（`WM_ACTIVATE`/`WM_ACTIVATEAPP` 也不能替代真实前台）。
- 真实 `SendInput`/`mouse_event` 需要引擎窗口是前台；当前台被其他进程占用、
  且 `AttachThreadInput` 被拒（error 5）时，探针**显式记录
  `input_skipped=not_foreground` 并放弃输入**，不盲点。
- 因此交互式采集提供了两条路：自动输入（前台可控时）与手动触发（用户真实点击，
  最可靠）。

### 4.6 trace 语义与命名

- `schema=3`；记录字段顺序固定 `schema,seq,frame,sub,kind,payload`。
- 事件记录：`sub="vm"`, `kind="command"`,
  `payload={"script":u32,"pc":u32,"name":str,"args":[]}`。
  `args` 恒为空数组：官方参数求值不可从分派点观测，未伪造。
- `name`：由 handler 反查 `0x7ace20` 下标，再用工程 `ysc.ybn` 映射命令名；
  共享 handler 留空。
- `frame`：命中时读 `[0x801268]` 的 u64；是引擎 tick，不是内核逻辑帧。
- `debug_mode`：官方窗口标题 CP932 解码 = **【デバッグモード】**（原始字节
  `81 79 83 66 83 6f 83 62 83 4f …`）。探针默认 `--debug-mode true` 写入
  header，并在 manifest 记录证据；`save/data/config/projectconfig.txt` 的
  `DEBUGMODE=0` 不是运行期实际值。

### 4.7 2026-10-21 采集结果（样本）

| 项 | 值 |
|---|---|
| 官方 exe | sha256 `4f5eefa3…`（0.495，PE32） |
| 语料 | `<project-eris>/data.ypf`，sha256 `5744a7be…` |
| `ysc.ybn` | 11156 B，sha256 `741d4caf…`，119 命令；与 `save/data` 官方解包副本逐字节一致 |
| 点击后事件 trace | 101,580 条命中，tick 299–308；trace sha256 `0e980b36…`（run_click2）/ 修正 header 后 `run_click2/official_events_debug.jsonl` |
| 内核同语料 trace | 876,096 条记录 / 632,805 条命令，frame 4–11（`--debug-mode true`） |
| 顺序对拍 | script 60 前 **966 条 `(script,pc,name)` 完全一致**；首分歧见 DIV-L-002 |

## 5. 与内核 trace 对齐

跨 producer 不能按 `seq`/物理行号对齐，也不能直接按 `frame` 对齐（官方 tick 与
内核逻辑帧不是同一时钟）。当前使用**顺序投影**：

```bash
cargo run -q -p yu-trace --example diff_traces -- \
  --expected target/tmp/l1-probe/run_click2/official_events_debug.jsonl \
  --actual target/tmp/l1-probe/kernel_events_debug12.jsonl \
  --mode cross --header codec-sample --order-only --script 60 --context 5
```

- `--order-only`：保留 header + 指定 script 的 `vm/command`，把 `frame` 置 0、
  `seq` 重编号（`TraceLog::command_sequence_projection`）；这是比较投影，
  不是观测到的帧号；
- 默认只比较 `script/pc/name`（官方 `args` 不可观测）；
- header 只比较 `codec` + 双方 `producer.sample_sha256`；
- 首分歧报告 `key/path/expected/actual`，不排序、不去重、不做容差。

内核侧采集：

```bash
cargo run -q -p yu-trace --example emit_kernel_events -- \
  --root <工程根(含 data.ypf)> --out <trace.jsonl> \
  --frames 12 --first-frame 4 --last-frame 11 --click-frame 6 --debug-mode true
```

## 6. 已知限制/挂账

- 官方 `args` 不可观测；共享 handler 的命令名留空。要补参数，需要在具体 handler
  入口读运行时窗口/栈，属下一步（不在 L1 范围）。
- 官方任务事件（spawn/finish/wait/jump）尚未采集；当前只有命令事件。
- 官方 tick 与内核逻辑帧的映射未证明；对拍只做命令顺序（§5）。
- 点击转场的完整起点依赖手动触发或前台可控的自动输入；`run_click2` 的官方窗口
  从 tick 299 开始，已错过转场开头约 15 tick（DIV-L-002 说明）。
- 屏幕截图与事件采集不能在同一断点会话内安全并行（PrintWindow 死锁）。
- 首屏 vs 内核 `first_frame` 的比较仍按 `DIV-H-001` 撤回，不作结论。
