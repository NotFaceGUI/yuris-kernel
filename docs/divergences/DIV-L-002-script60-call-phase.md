# DIV-L-002 · script 60 返回边界后调用点不同（官方场景任务 vs 内核菜单任务）

- 状态: **open**（多任务相位/调度对齐问题；尚未证明是脚本体语义分歧）
- 优先级: 中
- 发现日期 / 发现方式: 2026-10-21 / 官方事件级 trace vs 内核 `--debug-mode true` trace，
  `diff_traces --order-only --script 60`
- 相关任务: 任务书 L（Phase L-3）

## 复现

- 语料/输入: `<project-eris>/data.ypf`；标题画面点击「スタート」。
- 官方: `run_click2` 采集（点击 tick 284；trace 从 tick 299 开始）。
- 内核: `kernel_events_debug12.jsonl`（click_frame=6，trace 窗口 frame 4..11）。
- 锚点: script 60 的 `RETURN`(pc 975) 之后的第一次 script 60 调用。

## 官方行为

`script 60` 顺序投影的前 966 条命令与内核逐条一致；第 967 条（ordinal 966）：

```text
60 975 RETURN        ← 两条 trace 的最后一条相同命令
58 79  LOOP
58 80  GOSUB
60 1414 INT          ← 官方下一次进入 script 60 的入口
```

官方 trace 在 RETURN 之前处于 **script 66 场景任务**（`66 6228 RETURN` → `58 79`）：

```text
66 6226 LOOPEND
66 6227 LET
66 6228 RETURN
58 79  LOOP
58 80  GOSUB
60 1414 INT
```

## 内核行为

同一 RETURN 之后，内核 trace 处于 **script 20 菜单任务**：

```text
20 203 IFEND
20 204 IF
…
20 231 IF
20 231 JMP if_false -> 20 235
20 236 GOSUB
20 236 JMP gosub -> 60 820
60 820 INT           ← 内核下一次进入 script 60 的入口
```

## 二分记录

- [x] L1 译码一致? 是：同一 `data.ypf`，script 60 前 966 条 `(script,pc,name)` 完全一致。
- [ ] L2 执行/事件流一致? **未定**：分歧点是调用/返回边界，不是 script 60 体内
      的命令；官方调用者 = script 58（场景路径），内核调用者 = script 20（菜单路径）。
- [ ] L3 帧层状态一致? 未取证。
- [ ] L4 帧间编排一致? 未取证。

定位结论: 当前证据只支持“两边在**不同任务相位**”——官方 trace 已进入 script 66
场景任务，内核同一时刻仍在 script 20 菜单循环。不能据此判定 script 60 语义错误，
也不能用容差掩盖。

## 为什么暂不判为内核错误

- 官方 trace 从 tick 299 开始，点击在 tick 284，已过约 15 个 tick；
  内核 trace 从 frame 4 开始，点击在 frame 6，边界处约 3 个 frame。两边距点击的
  帧数不同，菜单任务与场景任务的让出/调度相位自然可能不同。
- 官方 trace 只有 `command` 记录，没有 `task_spawn/task_finish/wait` 记录；
  无法从当前样本还原官方的任务调度顺序。
- script 60 是共享子程序（pc 820 与 pc 1414 是两个不同入口），按“每脚本顺序”
  拼接会把不同调用者的调用串在一起；返回边界处的下一条命令取决于调用者。

## 下一步（需要新证据，不改容差）

1. **从点击的第一条命令开始采集官方 trace**：`probe_dispatch.py` 的
   `--trigger-rva 0x5ab63`（WM_LBUTTONDOWN 命中非透明 UI 像素后的写点）已实现；
   命中后自动挂分派断点，可避免漏掉转场开头。
2. **官方任务事件**：在 `FUN_004046dc`（任务列表遍历）/`FUN_00404398`（任务步进）
   加只读断点，记录 task 指针、script、pc、返回地址，得到官方的
   `task_spawn/task_finish/wait` 近似投影；再与内核 `VmEvent::TaskSpawn/Wait`
   对齐。
3. **按任务而不是按脚本序号对齐**：同一 task 的 script 60 调用序列再比较；
   若仍在此处分歧，升级为真实调度/调用分歧。
4. 未取得上述证据前，本 DIV 保持 open；对拍报告只声明“前 966 条一致，首个
   边界不一致”，不声明整段等价。

## 回归

- 验证命令:

```text
cargo run -q -p yu-trace --example diff_traces -- \
  --expected target/tmp/l1-probe/run_click2/official_events_debug.jsonl \
  --actual target/tmp/l1-probe/kernel_events_debug12.jsonl \
  --mode cross --header codec-sample --order-only --script 60 --context 5
```

- 原始结果: `order-only projection: expected 4681 commands, actual 1932 commands`;
  首分歧 `key=vm/command frame=0 ordinal=966`, `path=/payload/pc`,
  `expected=1414`, `actual=820`；前 966 条一致。

## 2026-10-23 任务相位证据(P5,600 tick,无点击)

官方探针新增 `--mode task-phase`:在 tick 入口 `0x4040f8` 只读遍历任务链表
(`0x801704` 列表对象、节点 `+0x0c/+0x34/+0x30`、上下文 `+0x18/+0x1c/+0x20/+0x3c`),
不挂命令分派断点。采集 600 tick,产物 `target/tmp/p5-phase-click5/task_phase.jsonl`
(不入库);内核 `emit_kernel_phase` 同 600 tick(`target/tmp/p5-kernel/kernel_phase.jsonl`)。

对拍方式:`tools/task_phase_diff.py --align anchor --anchor-script 66`(以场景任务 66
首次出现为相对 0),比较每 tick 启用任务的 `(script,pc)` 多重集合。

结果:

- **对齐成立**:script 66 首次出现于官方 tick 12 / 内核 frame 12;script 20 三个实例的
  `(script,pc)` 在相对 0..590 逐 tick 相同。
- **新分歧 1**:官方任务链表常驻 `script 0`(容器/主任务)与 `script 8`;内核
  `SYSTEM` 主任务(script 8)在 frame 16 左右结束并从 `Scheduler::nodes()` 消失。
- **新分歧 2(显式忽略 0/8 后)**:相对 49 起,官方 script 66 的 pc 在
  `5100/5168` 间推进(49/50/62/…),内核 600 tick 一直停在 `5100`;
  首个 `(script,pc)` 分歧 = `expected 66:5168 actual 66:5100`。
- **新分歧 3**:官方 script 66 在相对 205 之后被 `script 139` 取代(绝对 tick 218);
  内核 script 66 一直存活到 600 tick 末尾(相对 590),没有对应的结束/替换。
- 这两条是**任务生命周期/调度**差异,不是 script 60 体内命令差异;仍不能据此判定
  script 60 语义错误。DIV-L-002 保持 open。
- **点击未覆盖转场**:本会话 `SendInput` 已在窗口前台发送,但官方 `0x80157c`
  “有效 UI 点击”标志始终 0、`trigger=null`,任务相位无转场。script 60 返回后的
  相位仍缺官方样本;已把“真实用户点击/前台激活”列为下一步采集前提。
