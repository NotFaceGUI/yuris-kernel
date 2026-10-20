# 任务书 E · 修复审查确认缺陷(A/B/C/D 审查后续)

> 会话定位: 你独占工作区做修复。**先读 `PROGRESS.md` 的「成果 16」**(四线审查结论),
> 再按本任务书执行。证据纪律与 A/B/C/D 相同: 缺证据 → `Diag::Unimplemented` + divergences,
> 禁止猜。每个行为修复必须带出处注释 + 回归测试。
>
> **并发警告(必读 §3)**: 启动时 C 会话仍在改 `yu-rt`。**C 活跃期间禁止编辑
> `yu-rt` / `yu-vfs` / `yu-log`**。先做 `yu-format` 与 `yu-trace`(当前无活跃写者),
> 再检查 mtime; 仍活跃就停手报告 blocked, 不要抢写。

## 0. 开工前必读

1. `PROGRESS.md` 成果 16(本次审查的完整结论与证据)
2. 本任务书
3. 官方手册(逐条核对,不要只信审查报告):
   - `control/task.html`、`control/taskend.html`、`control/taskinfo.html`
   - `control/end.html`、`control/gosub.html`、`control/ifcontinue.html`
   - `basic/task.html`(让出点、SYSTEM 任务、优先级 Z、默认 100)
   - `basic/svariable.html`、`system/fps.html`、`control/wait.html`
   - 路径: `<side-repo>/target/sdk/0.495/yu-ris_0495_032/マニュアル/YU-RIS/html/`
4. REA 已直接复核的官方事实(0.495 `yu-ris.exe`,sha256 `4f5eefa3…`):
   - `FUN_00462a10`:恒等表 + **恰好 12 对交换**,无版本分支;生成 CRC32("Yu-ris") 四字节。
   - `FUN_0045e3b8`:索引起点恒 `0x20`,步长 `nameLen+0x1B`,`T[0xFF-lenEnc]`,名字取反,无版本分支。
   - `FUN_00404398`:任务泵按等待计数器(+0x18)/`timeGetTime` 期限(+0x1c)门控;处理器返回非 0 = 让出。
   - `FUN_004046dc`:按索引取任务链表节点,不是优先级排序本身。
   - **勘误**:旧笔记的 TASK 地址 `0x4506f4/0x4521dc/0x4567c0` 在 0.495 不是函数入口,
     不能引用为 0.495 证据;TASK 语义以手册为准。
5. 旧线参考 `<repo-old>`(只读,禁止修改/禁止 git);真名见 `AGENTS.md`。

## 1. 铁律

- 不猜语义。拿不齐"手册页 / 真实脚本 / 官方行为(REA 或可复现观测)"三层的,
  保持 `Unimplemented`,并写 `crates/*/docs/divergences/`。
- 每个修复:代码 doc 注明出处;新增回归测试;报告贴原始输出。
- 禁止任何 git 写操作;禁止修改 `<repo-old>`;被跟踪文件只用占位符。
- 缺样本测试必须显式可见:引入 `YURIS_REQUIRE_SAMPLES=1`,设了就必须跑真样本,
  没设也至少打印 `SKIP[...]` 并计数;禁止静默 `ok`。
- 禁止盲目 `UPDATE_GOLDEN`/`YURIS_REGEN_GOLDEN`:先人工确认新事件流/结构符合手册,
  再更新,并在报告里列出改了哪些 golden、为什么。

## 2. 修复清单(按优先级;P0 必须完成)

### P0-A · `yu-format`(安全区,立即做)

| 编号 | 修复 | 证据/验收 |
|---|---|---|
| F1 | `collect_label_refs` 用 **窗口 `slot`** 而不是发射位置去索引 ysc 参数名;非首位 `#`(如 TASK 的 slot 34)必须收进 `DecodedScript.labels` | 真实 `yst00008.ybn` 第 232 条 TASK 的 `es.SS.FADE.TASK` / `.MAIN` 必须出现;与 `ysl.ybn` 的 `script_no=8,field_b=232/234` 对齐;加回归测试 |
| F2 | YSTB 用**头里的 count**迭代,不用 `inst.len()/4`;`encrypt` 保留原 header count/sizes 与 trailing;若做不到通用回封,就明确只支持"自洽且无尾部"并拒绝其它输入 | 复现:追加 4 字节尾部、或把 count 改成 1,当前会静默丢数据/改文件;修复后必须字节一致或显式 Err;引擎 `FUN_004518e9` 用头 count |
| F3 | YSCF 宽/高按 **u32** 读,零值/范围校验与引擎一致(两者都做无符号范围检查) | 反编译 `0x45dbb0`(`MOV dword ptr [+0x10]/[+0x14]`,asm `0x45dcc8/0x45dccb`);加 65536/单边 0 的用例 |
| F4 | 32 位 `usize` 上的未检查算术改 `checked_*`:YSTB 段和/offset+size、YSTL name_at+len、YSTD var_count+1 | 构造溢出输入返回 `fmt.truncated`/`fmt.limit`,不 panic;`armv7` 目标可编译 |
| F5 | 错误密钥诊断:decrypt 后做结构自检(opcode 分布、argc、窗口偏移),失败产出 `fmt.ybn.key`;`fmt.count` 要么实现要么从 `ALL_CODES` 删除 | 错 key 的 fixture 必须得到 `fmt.ybn.key`,不是 `fmt.ybn.opcode` |
| F6 | 未验证的 V2 版本带(200,300):保留可以,但必须挂 `fmt.ybn.unsupported` 警告并写 DIV;或按引擎无版本检查的行为处理 | 文档与实现一致;不得静默 |

### P0-B · `yu-vfs`(等 C 空闲;改动尽量保持 API 兼容)

| 编号 | 修复 | 证据/验收 |
|---|---|---|
| V1 | **删除 NUL-XOR 的 `Ok` 回退**:未验证方言一律 `Err(Unimplemented)`;不再把结构不匹配的包读成垃圾名字 | 复现包 `v300_wrong12_hash0.ypf` 必须失败;`stats.name_hash_matches` 不得再被挪用 |
| V2 | 版本表对齐 REA:**只保留 0x20 起点 + 12 对表 + 22 字节 meta**(对 234..=495 全部接受版本);删除 0x24、8/10 对表、18/26 字节 meta,或收进显式 `Unverified` 分支并返回 Err | `FUN_00462a10`/`FUN_0045e3b8`;官方 ERIS 与 v494 真包回归通过;`v300_engine_layout.ypf`(0.495 布局但 version=300)应能读 |
| V3 | `dataOffset` 按 **u64** 读;拒绝高位非 0 的未知语义(或按 u64 使用);更新文档里"宽度未验证"的错误说法 | `FUN_0043fb9c` 读 8 字节;`FUN_00404580` 写 8 字节 |
| V4 | `payload_hash == 0` 不再跳过校验;hash 是数据,不是哨兵 | `FUN_0043f71c` 无条件比较 |
| V5 | 重复名字语义与引擎对齐(后者生效、前者禁用)或直接 Fatal;不得静默 first-wins | `FUN_0045e3b8` 的重复 hash 处理;加合成包测试 |
| V6 | `is_verified()` 要求 `index_start==0x20 && name_key==0xFF`;非 0xFF key / 其它起点必须 false + 警告 | 现有测试更新 |
| V7 | `entry_count` 上界:引擎 99,999 + `index_bytes/27`;删除 `with_capacity(1_000_000)` 的放大分配,用 `try_reserve` 或按实际 | 36 字节恶意头不得再产生 100MB+ 分配 |
| V8 | key 探测在 `score == entries.len()` 时提前退出 | 官方包从 256 轮降到 1 轮;测试断言调用次数或耗时上限 |
| V9 | Diag 的索引错误把实际失败偏移写进 `Loc::Data.offset`,不只写 detail | 自动定位可用 |

### P0-C · `yu-rt`(**只在 C 空闲时做**;这是本轮重点)

| 编号 | 修复 | 证据/验收 |
|---|---|---|
| R1 | 主任务 ID 改 `SYSTEM`(手册);`TASK/TASKEND` 无 ID 时目标是**执行者自身**,不是 `@main` | 手册 `basic/task.html`、`control/task.html`、`control/taskend.html`;回归:非主任务 `TASKEND[]` 只结束自己,主线继续写变量 |
| R2 | `TASKEND` 立即生效:停止当前泵、移除节点与 `ExecState`、发 `TaskFinish`,`TASKINFO EXIST` 变 0;同 ID 再建从新标签开始 | 手册;回归:`TASKEND` 后的 `LET` 不执行;重生成不复用旧 pc |
| R3 | 显示文本 `_` 恢复为**让出点**并发 `VmEvent::Text`(字节原样) | 手册 `basic/task.html`;ERIS s130;回归:一帧只出一行,宿主能收到 Text |
| R4 | 优先级 Z:就绪任务按 Z 降序执行;系统任务默认 100;`TASK` 的 Z 参数生效 | 手册;若引擎插入点无法取证,在 doc 标注 `Likely` 并加对照测试;不得继续完全忽略 |
| R5 | ERIS 的 TASK 阻塞项:`TINT`(间隔)、`SCRIPTPOS`(存档锚点)、`RESTART` 按手册与真实脚本实现;拿不齐证据的单项保持 Unimplemented,但**TASK 不得整条静默失败**——至少发带参数名的 Diag | ERIS 10 处 TASK(s008/s019/s020/s070/s071)必须能生成任务或明确挂号;`TASKINFO EXIST` 可观测 |
| R6 | `END` 置全局 halt(不再让其它任务继续跑);`RESTART=1` 需要证据,拿不到就 Unimplemented,不得假装 | 手册 `control/end.html`;回归:END 后 tick 不再执行命令 |
| R7 | 修 panic:`IFBREAK[LV=0]` → Diag;GOSUB 参数名 `PINT0`/`PINT17+` → `checked_sub`/范围检查;`VARACT[DIMSIZE]` 与数组维长乘积 → `checked_*` + 上限 + Diag | 每个都用最小 fixture 复现"修复前 panic / 修复后 Err" |
| R8 | 语义修正:`MATH/LABELINFO` 保留左值下标;`TASKINFO FILE` 返回文件名;`SYSTEMINFO` 多查询不互相覆盖;`%0`/浮点除零按已掌握证据统一;CP932 大小写不破坏多字节尾字节 | 每条加回归;无证据的保持 Unimplemented |
| R9 | GOSUB 嵌套上限改 64(基帧不算);`IFCONTINUE` 不再累积 IF 帧 | 手册 `gosub.html`/`ifcontinue.html`;回归 |
| R10 | 事件/诊断有界:提供 `drain`/容量上限;发 `FrameEnd`;所有任务结束路径都发 `TaskFinish` | 8 帧事件不得无限增长;事件流有完整帧边界 |
| R11 | Diag 补 `task/script/pc`;删除 `TEXT/_` 里的 `let _ =`、ID 非字符串等静默回退;`unimplemented_cmds` 收录处理器内部挂号 | 日志能定位到具体脚本位置 |
| R12 | **统一解析器**:`yu-rt` 改为依赖 `yu-format` 的类型与解析,删除 `tables.rs` 里的重复实现 | 若工作量超出本次会话:先写迁移计划与差异清单,不要半迁移;现有 corpus/golden 必须保持通过 |

### P1 · `yu-trace`(可与 F 并行)

- T1 `hex4` 字符边界 panic → `trace.parse`;加坏 `\u`/深嵌套负例。
- T2 缺样本改为可见失败:`YURIS_REQUIRE_SAMPLES=1` 时必须跑;否则打印并计数。
- T3 header 增加 producer-neutral 字段;记录 `tick_at` 的 now_ms 序列;`header.codec` 与数据源一致性校验。
- T4 diff 支持按 `sub/kind/frame` 投影与对齐,解除"seq=物理行号"对跨生产者比较的绑定。
- T5 流式写入/读取,`finish` 不再深拷贝;给大 trace 设上限。
- T6 aux 源帧句柄与多逻辑帧输入摘要修正。

### P2 · 性能(最后做)

- RT 每命令 String/Vec 克隆、exec remove/insert、表达式重复解码;
- format `index_of` 线性查找改映射、`encrypt` 去 clone;
- vfs 解压按 expected 预分配但防 4GB 声明;
- 32 位目标 checked arithmetic 全量收口。

## 3. 并发守则(最重要)

1. 启动先跑:
   ```bash
   find crates -type f -newermt "-10 minutes" | sort
   ```
2. 任何 **10 分钟内被改过** 的文件所属 crate 视为"活跃写者",**只读不写**。
3. 执行顺序:
   - Phase 1: `yu-format`(F1-F6)
   - Phase 2: `yu-trace`(T1-T2,可选 T3-T6)
   - Phase 3: 再查 mtime;`yu-rt` 仍活跃 → 跳过 R 阶段,报告 blocked;
     `yu-vfs`/`yu-log` 仍活跃 → 跳过 V 阶段。
   - Phase 4: C 空闲后做 R(V 可在 R 之前)。
4. 若 `cargo` 因另一会话的 target 锁等待,属正常;不要 kill 进程。
5. `PROGRESS.md` 只在结束时追加"成果 E1",不重写别人条目。

## 4. 验收

- `cargo test --workspace --no-fail-fast` 全绿;设 `YURIS_REQUIRE_SAMPLES=1` 再跑一遍。
- 每个 P0 修复都有新增回归;报告给出"修复前失败/修复后通过"的原始输出。
- `cargo clippy --workspace --all-targets` 零新增告警。
- `grep -rn "println!\|eprintln!\|dbg!" crates` 零命中(测试里显式打印 skip 除外,需说明)。
- `git diff --stat` 只包含预期文件;无任何 git 写操作。
- 报告:改动文件清单、证据出处、修复前后输出、blocked 项、未做项。

## 5. 交付

结束时在 `PROGRESS.md` 追加"成果 E1(日期)· 审查缺陷修复",列出:
完成编号 / 未完成编号 / blocked 原因 / 验证命令与原始结果。
