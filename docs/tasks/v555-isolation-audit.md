# v555 独立模块抽离 · 第一轮只读审计（2026-11-21）

> **状态：只读审计。** 本文件是 untracked 文档；本轮未修改任何代码，未执行任何
> git 写操作（无 checkout/apply/pop/drop/commit/stash/worktree add）。
>
> 依据：`AGENTS.md`、`docs/tasks/v555-isolation-task-book.md`（下称“任务书”）、
> `docs/tasks/v555-isolation-startup-prompt.md`。
>
> 现场：`master` @ `c1549eb`；工作区已恢复 `stash@{0}` 的 v555 改动，
> stash 仍在、未 pop/drop；`msic/`、`yu-emote` 未触碰。

---

## 需求基线（主人 2026-11-21 确认）

本轮只有两条需求；其他都是实现约束/证据要求，不是新增目标：

1. **旧版 = 0.4.2 行为**：495（以及所有 `version != 555` 的既有版本）的默认
   行为、命令表、系统变量 ABI、存档布局、金标、诊断必须与 0.4.2 / `c1549eb`
   基线一致；0.4.2 之后出现的淡入延迟/突然黑闪必须消失。
2. **555 = 特殊版本**：555 的商业语义是独立运行面，只对精确
   `yscm.version == 555` 生效；不得写进共享层、不得让 556/600 继承，也不得用
   495 手册补 555 的洞。

验收只看这两条：**旧版零回归 + 555 隔离**。审计表里的 REVERT/MOVE/KEEP/DEFER
都是为这两条服务；D1–D5 是落地时的最小选择，不是额外需求。

---

## 0. 结论（先给主人看的 5 行）

1. 现场与任务书 §1 一致：`stash@{0}` 基于 `c1549eb`，39 个 tracked + 68 个
   untracked；68 个 untracked 与工作区**逐字节一致**。tracked 侧唯一漂移是
   `yu-emote` 子模块指针（不在 stash 内、不得触碰）。
2. 旧版回归的首要嫌疑不是“某一个文件”，而是 **7 类共享行为改动**：
   `GosubFrame` 三组拆分、`_PINT/_RSTR` 读取改三组、`apply_script_statics`
   对 `S_*` 的无条件装载期声明、`text_reveal_complete` 无窗口即完成、
   `yu-gfx` 两处无条件过渡回退、`yu-save` 的 DNO 0 基、desktop 的
   `main_finished && live_tasks==0` 退出条件。
3. 其余 v555 业务面（`PRODUCT`、`TEXT[FILE]`、`FILEINFO[CGFILE]`、
   `VARINFO` 变体、`MEM` 存档、词法器、`_STR/_INT/_DROPFILE`）基本可按
   “参数名 / 精确 555”迁移到独立运行面；`yu-gfx` 的 `LINT/LINT2` 需要
   一个中性图型策略或扩展点。
4. **模块落点已由主人裁决（2026-11-21）**：用 `crates/yu-rt/src/other/`
   中性目录（不带 `v555` 字样），同 crate 直接使用内部 API，成本最低；
   新 crate 作为后续迁移目标，待 `RuntimeExtension` facade 稳定后再评估。
   落地硬条件仍是精确 `yscm.version == 555` 安装、556/600 不继承。
5. 本轮**未运行** `cargo test`：master 工作区正在复测；Step 3 前需要主人提供
   独立 worktree/分支或明确授权，再跑旧版金标与 bisect。

---

## 1. 现场核对

### 1.1 硬事实（只读）

| 项 | 实测值 | 与任务书 §1 一致性 |
|---|---|---|
| 仓库 | `<repo>` | ✅ |
| 分支 / HEAD | `master` / `c1549ebded5d4502448d5ebe8c86412c0e7f9b35` | ✅ |
| `stash@{0}` | `f9f354f5d10e12550f6b6ef9eac8d9c9d3176b96` | ✅ |
| stash 消息 | `v555-worktree-2026-11-21-before-baseline` | ✅ |
| stash 基提交 `^` | `c1549ebded5d4502448d5ebe8c86412c0e7f9b35` | ✅（reflog 标签写 `On main`，以基提交为准） |
| stash index `^2` | `cdbdb3687cc889e89ad27a4e4a8c76d71d6bc332` | 记录 |
| stash untracked `^3` | `6ce0c41aad7c2ba00183abedf3fe5670c81b35e2` | 记录 |
| tracked 改动 | 39 文件，+2683 / −94 | ✅ |
| untracked 改动 | 68 文件 | ✅ |
| 工作区 tracked 漂移 | 仅 `yu-emote`（`1b46604` → `a54cbd3`，不在 stash 内） | ✅ 任务书 §1 已列 |
| 工作区 untracked 额外项 | 本审计文档、`v555-isolation-startup-prompt.md`、`v555-isolation-task-book.md`、`msic/0.4.2.exe` | 预期/不得触碰 |

### 1.2 “工作区已恢复 stash”校验

只读导出并比对：

```text
stash-tracked.patch   sha256 = a46d7687d451d38ccbfa82b916dbffb6133ee7a9851d6d7355ab0346cb32d42d
worktree-tracked.patch sha256 = 4a9b226c5a61219b9c5c53b10933b9703fa971c6c45437cf6ef38e7d6d5b5162
```

差异只有一处：`yu-emote` 子模块指针。即 **39 个 tracked 改动与 stash 完全一致**；
`yu-emote` 属于进入本轮前的既有状态，收盘时不得带进任何提交。

68 个 untracked 逐个用 `git hash-object` 对 `stash@{0}^3:<path>` 校验：

```text
checked=68 missing=0 diff=0
```

### 1.3 stash 备份的漂移（重要）

`<side-repo>/workbench/tmp/v555-decomp/stash-backup/status-before.txt` 比
`stash@{0}` 多出 3 项：

| 备份里的项 | 是否在 stash | 当前工作区 | 结论 |
|---|---|---|---|
| `M crates/yu-cli/src/desktop.rs` | ❌ 不在 39 tracked 内 | 无 diff | 备份与 stash 非同点；**不得从 stash 恢复** |
| `M crates/yu-platform/host-desktop/Cargo.toml` | ❌ 不在 39 tracked 内 | 无 diff | 同上 |
| `?? NUL` | ❌ 不在 68 untracked 内 | 不存在 | 忽略（Windows 重定向残留） |

`diff-stat.txt` 还包含 `yu-emote | 2 +−`，同样不在 `stash show` 的 39 文件内。
**结论**：备份文件是“stash 之前的现场快照”，不是 stash 内容的精确清单；本轮以
`git stash show --name-status stash@{0}` + `ls-tree stash@{0}^3` 为唯一清单。
若主人认为上述两个文件里有 v555 改动需要保留，请另行提供内容，不能从 stash 反推。

### 1.4 工作树清单

```text
<repo>                    master @ c1549eb   ← 当前
<repo>/target/tmp/wt-1031  detached @ 5584621
<repo>-v555               v555-support @ 6e47cd2
<side-repo>/workbench/tmp/yk-head detached @ 44024638
```

- `v555-support`：早期实现，只作语义参考，**不整分支合并**。
- `target/tmp/wt-1031`、`yk-head`：任务书未提及；本轮不使用、不修改。
- 当前仍在 master 工作区；**Step 3 改码前必须换独立 worktree/分支或获得明确授权**。

### 1.5 本轮未运行的检查（显式挂账）

| 检查 | 状态 | 原因 |
|---|---|---|
| `cargo build --workspace --all-targets` | 未运行 | 避免在主人复测的 master 工作区产生构建副作用；留给独立工作树 |
| `cargo test --workspace` / 各 crate 金标 | 未运行 | 同上 |
| 旧版进游戏手工复测（淡入/黑闪） | 未运行 | 需要主人复测；本轮只读 |
| v555 headless/desktop 验收 | 未运行 | 属于 Step 6，不在审计轮 |

> 引用主人已确认的基线：`c1549eb` 是旧版复测基线；若该基线本身不干净，
> Step 3 的门禁将无法区分“旧版回归”与“基线既有问题”。

---

## 2. 证据与分类口径

### 2.1 证据锚点

| 证据 | 用途 |
|---|---|
| `git show stash@{0}` / `stash-unified0.patch` | hunk 级改动本身 |
| `c1549eb:<path>` | 旧版行为基线（逐函数核对） |
| `docs/formats/v555-truth-2026-11-21.md` §5/§6/§10/§11 | 555 语义与已落地项 |
| `docs/divergences/DIV-RT-065-gosub-param-groups.md` | GOSUB 三组拆分的自述证据（任务书 §5.1 明确不足以证明旧版无回归） |
| `docs/divergences/DIV-RT-066-v555-runtime-surface.md` | v555 运行期面清单 |
| 任务书 §3（目标架构）/§4（职责表）/§5（初始判定）/§6（门禁） | 分类与处置口径 |
| 旁线 `format-vm-opcodes.md` §3.16 / `vm-commands.md` | 引擎三组参数数组等事实（只读） |

### 2.2 分类定义

| 分类 | 含义 | 处置 |
|---|---|---|
| **REVERT** | 改动共享行为/共享结构，且旧版路径可观察地变了（或证据不足判“未知”） | 恢复 `c1549eb` 行为；不要反向补 stash |
| **MOVE** | v555 业务逻辑当前落在共享层，但可迁到独立运行面；迁走后旧版路径保持/恢复原样 | 迁入 `crates/yu-rt/src/other/`（中性目录） |
| **KEEP** | 行为中性：新诊断码/事件字段/端口/中性 API/精确 555 数据派生；旧版不调用时行为不变 | 保留；补旧版中性证明 |
| **DEFER** | 文档、工具、探针、示例、PROGRESS；不参与行为判定 | 最后收口；不随代码提交 |

“旧版影响”三档：**会**（可观察行为/诊断/时序改变）、**不会**（旧版路径逐字节不变）、
**未知**（证据不足，必须先补测试/对拍，禁止猜）。标“未知”的项不得直接实现。

---

## 3. Hunk 级审计表

> 行号约定：`base:` 指 `c1549eb` 的行；`new:` 指 stash 应用后的工作区行。
> “hunk”指 `git stash show -p --unified=0` 的对应块。八个重点文件逐 hunk；
> 其余文件按逻辑块列出（块内所有 hunk 均已覆盖）。
> 证据缩写：`TB`=任务书，`TR`=真值台账，`D65/D66`=两份 divergence。

### 3.1 `crates/yu-rt/src/vm.rs`（重点，逐 hunk）

| 文件:符号/base 行 | 暂存改动 | 分类 | 旧版影响 | 证据 | 处置 |
|---|---|---|---|---|---|
| `vm.rs:BootConfig.disk_free_bytes` base:175 / new:176-181 | 新增 `Option<u64>` 宿主端口字段 | KEEP | 不会 | TR §6.4；`engine.rs:3192` 只在 v555 PRODUCT COM=34 消费 | 保留；旧版不读 |
| `vm.rs:BootConfig::default` base:220 / new:227 | 默认 `None` | KEEP | 不会 | 同上 | 保留 |
| `vm.rs:GosubFrame.args_*` base:297-298 / new:304-315 | `args[16]` → `args_int/args_flt/args_str` | **REVERT** | **会/未知** | `c1549eb:vm.rs:297-301`；TB §3.3；D65 未覆盖旧版 `RINT/RFLT/RSTR` 与过渡宏 | 共享恢复 `args: [Option<Value>;16]`；三组帧放 v555 `gosub.rs` |
| `vm.rs:GosubFrame.ret_*` base:300-301 / new:317-320 | `ret[16]` → `ret_int/ret_flt/ret_str` | **REVERT** | **会/未知** | 同上 | 同上 |
| `vm.rs:Vm.disk_free_bytes` base:585 / new:605-606 | 新增字段 | KEEP | 不会 | 只在 v555 COM=34 读 | 保留 |
| `vm.rs:Vm.drop_files` + `last_text_lines` base:694 / new:716-722 | v555 专属状态 | MOVE | 不会（旧 handler 不触碰） | TR §6.3/§6.4；D66 §1 | 移入 `other::State` |
| `vm.rs:Vm::boot 赋值 disk_free_bytes` base:893 / new:922 | 装配端口 | KEEP | 不会 | 同上 | 保留 |
| `vm.rs:Vm::boot 初始化 drop_files/last_text_lines` base:936 / new:966-967 | v555 状态初始化 | MOVE | 不会 | 同上 | 随状态移走 |
| `vm.rs:Vm::apply_script_statics` base:1130 / new:1162-1256 | 新增装载期静态声明扫描；`S_INT/S_FLT/S_STR` **无条件**执行，`INT/FLT/STR` 仅 `LoadTime` | **REVERT + MOVE** | **会**（旧版 `S_*` 从运行时声明变成装载期声明） | `c1549eb` 无此函数；`op_declare` base:variables.rs:209-220 处理 `S_*`；TB §5 | 共享删掉无条件 `S_*`；v555 的 `LoadTime` 逻辑放 `on_script_loaded` 钩子/模块 |
| `vm.rs:Vm::drop_files_len/drop_file/last_text_lines` new:1481-1497 | v555 sysvar/PRODUCT 访问器 | MOVE | 不会 | D66 §1；`sysvar.rs` 新 `_DROPFILE` 只在 v555 语料 | 移入 v555 状态/访问器 |
| `vm.rs:Vm::live_tasks` new:1499-1501 | 新增中性任务计数 | KEEP | 不会 | 旧调度器行为不变；消费者 desktop 需回退 | 保留 API；desktop 回退后暂不消费 |
| `vm.rs:Vm::scenario_token_vars` new:1511-1599 | v555 PRODUCT COM=22 token 数组定位；带 `v555: bool` 参数 | MOVE | 不会（仅 v555 PRODUCT 调用） | TR §11.4；D66 §1 | 移入 v555 `product.rs`；去掉 bool 参数 |
| `vm.rs:Vm::enumerate_drop_files` new:1601-1651 | VFS 目录枚举 → `drop_files` | MOVE | 不会（仅 v555 PRODUCT COM=21/24） | TR §6.3/§6.4；D66 §1 | 改为中性 VFS 列举 API + v555 模块自持结果 |
| `vm.rs:Vm::disk_free_bytes` 访问器 base:1418 / new:1691-1695 | 只读访问器 | KEEP | 不会 | 同上 | 保留 |
| `vm.rs:Vm::report_unimplemented_variant` base:2242 / new:2520-2539 | 按变体只报一次的诊断辅助 | KEEP | 不会（新方法；旧路径不调用） | D66 §2 | 保留 |
| `vm.rs:load_script` 调 `apply_script_statics` base:2276 / new:2574 | 每次装载脚本后调用 | **REVERT + MOVE** | **会** | 与上一行同源 | 共享恢复原调用序列；v555 用 `on_script_loaded` |
| `vm.rs:TaskExecutor::text_reveal_complete` base:3038-3040 / new:3337-3345 | `line_complete()` → `window().is_none() || line_complete()` | **REVERT** | **会**（无窗口任务旧版语义改变） | `c1549eb:vm.rs:3039-3041`；D66 §1 自称官方 `FUN_00404914` 同判，但未给旧版回归 | 恢复原判；v555 行为放扩展钩子/精确 555 策略 |

### 3.2 `crates/yu-rt/src/vm/handlers/control.rs`（重点，逐 hunk）

| 文件:符号/base 行 | 暂存改动 | 分类 | 旧版影响 | 证据 | 处置 |
|---|---|---|---|---|---|
| `control.rs:op_gosub` base:26-27 / new:26-59 | 新增 `RefIntParamMode::SlotId` 的“字符串数组引用传槽号”；三组参数落位 | **REVERT + MOVE** | **会/未知** | `c1549eb:control.rs` 单 `frame.args[idx]`；TB §3.3/§5.1 | 共享恢复单 `args`；SlotId 与三组都进 v555 `gosub.rs` |
| `control.rs:op_return` base:59 / new:91-93 | `ret[16]` → `ret_int/ret_flt/ret_str` | **REVERT** | **会/未知** | 同上；D65 未覆盖旧版 `R*` | 共享恢复 `ret` |
| `control.rs:op_return` 写组 base:69-70 / new:103-108 | 按 `ParamTy` 分三组 | **REVERT** | **会/未知** | 同上 | 恢复 `ret[idx]` |
| `control.rs:op_return` 写调用方 base:79 / new:117-119 | `caller.ret_*` | **REVERT** | **会/未知** | 同上 | 恢复 `caller.ret` |

### 3.3 `crates/yu-rt/src/sysvar.rs`（重点，逐 hunk）

| 文件:符号/base 行 | 暂存改动 | 分类 | 旧版影响 | 证据 | 处置 |
|---|---|---|---|---|---|
| `sysvar.rs:import VarSpace` base:22 / new:22 | 新增 import | KEEP | 不会 | 仅为 v555 访问器服务 | 迁走后可删 |
| `sysvar.rs:_PINT/_PFLT/_PSTR` base:62-69 / new:62-72 | 读 `frame.args_*` 三组 | **REVERT** | **会/未知** | `c1549eb:sysvar.rs` 单 `frame.args[n]`；TB §3.3 | 恢复单组；v555 模块的 `sysvar_get` 读自己的帧 |
| `sysvar.rs:_RINT/_RFLT/_RSTR` base:90-97 / new:90-100 | 读 `frame.ret_*` 三组 | **REVERT** | **会/未知** | 同上 | 同上 |
| `sysvar.rs:_DROPFILE_NUM/_DROPFILE` base:243 / new:244-256 | 新增系统变量访问器 | MOVE | **会/未知**（旧版此前是 `undoc` 错误，现在得到 0/空串） | `c1549eb:sysvar.rs` 无这两个 case | 移入 v555 `sysvar_get`；旧路径恢复错误 |
| `sysvar.rs:_STR` 动态访问器 base:382 / new:396-490 | `_STR(槽[,元素])`；槽 <1000 且版本 !=555 时返回空串 | MOVE | **会/未知**（旧版从错误变成空串） | TR §5.3；D66 §1；代码内 `version != 555` 分支 | 移入 v555 `sysvar.rs`；共享恢复原 `undoc` |
| `sysvar.rs:_INT` 动态访问器 base:387 / new:496-515 | `_INT(槽[,元素])`；无版本门控 | MOVE | **会/未知**（旧版从错误变成取值） | TR §5.2 | 移入 v555；共享恢复原样 |
| `sysvar.rs:dynamic_slot_index` new:1031-1049 | `_STR/_INT` 下标解析辅助 | MOVE | 不会（仅上面两个 case 用） | 同上 | 随访问器移走 |

### 3.4 `crates/yu-rt/src/vm/handlers/file_io.rs`（重点，逐 hunk）

| 文件:符号/base 行 | 暂存改动 | 分类 | 旧版影响 | 证据 | 处置 |
|---|---|---|---|---|---|
| `file_io.rs:FILEINFO CGFILE` base:291/315/354/461 / new:292-295,320-327,367-388,496-544 | 新增 `CGFILE` 参数解析、存在性探测、扩展名序、`version==555` 分支 | MOVE | 不会（旧 ysc 的 FILEINFO 无 `CGFILE`；且显式 555 门控） | TR §11.3；D66 §1；`c1549eb:file_io.rs` 无 CGFILE | 移入 v555 `fileinfo.rs`；共享 handler 恢复原样 |
| `file_io.rs:SAVE MEM` base:635/674/717/721/815 / new:719-722,762-767,810-818,911-949 | 新增 `MEM` 参数、MEM 优先、DNO 0 基、内存槽写入、`MEM=99999` 挂号 | MOVE | 不会（旧 ysc 的 SAVE 无 `MEM`；非 MEM 文本对旧版相同） | TR §6.4；D66 §1；TB §4.1 | 移入 v555 `save_mem.rs`；共享 `op_save` 恢复原样 |
| `file_io.rs:LOAD MEM` base:849/886/915/931/935/938/940 / new:984-986,1024-1029,1059-1065,1081-1084,1088,1092-1112,1114-1127 | 新增 `MEM` 参数、MEM 优先、DNO 0 基、`load_block_mem/load_image_mem`、`MEM=99999` 挂号 | MOVE | 不会（旧 ysc 的 LOAD 无 `MEM`） | 同上 | 移入 v555 `save_mem.rs` |
| `file_io.rs:LOAD 非 MEM 错误路径` base:923/927/962/989 / new:1073,1077,1148,1191-1204 | 去掉前置 `file` 必需检查；错误文案改成 `缺 DNO/ID`、`缺 FILE/MEM` | **REVERT** | **会**（旧版诊断文本/分支改变） | `c1549eb:file_io.rs` 原文案 `LOAD[ID] 缺 FILE/DNO/ID`、`LOAD 缺 FILE` | 共享恢复 c1549eb 流程与文案；MEM 分支只在 v555 模块 |

> 注：`SAVE/LOAD` 的 `DNO` 范围校验在 `yu-save` 还有一处共享改动（见 §3.6），
> 两处必须一起回退，否则旧版 `DNO=0` 仍会被接受。

### 3.5 `crates/yu-rt/src/vm/handlers/text.rs`、`variables.rs`、`handlers/mod.rs`、`helpers.rs`（重点，逐 hunk）

| 文件:符号/base 行 | 暂存改动 | 分类 | 旧版影响 | 证据 | 处置 |
|---|---|---|---|---|---|
| `text.rs:op_text` base:59 / new:60-65 | 参数表含 `FILE` 时转 `op_text_file` | MOVE | 不会（旧 TEXT 无 `FILE` 参数） | TR §6.9；D66 §1；TB §4 | 移入 v555 `text.rs`；共享 TEXT 恢复原样 |
| `text.rs:TEXTINFO 空签名` base:216 / new:223-228 | 空参数 + `ysc_version==555` → no-op | MOVE | 不会（精确 555） | TR §6.7；D66 §1 | 移入 v555；共享恢复“缺 LET”诊断 |
| `text.rs:op_text_file` + `text_file_lines` + 单测 base:372 / new:385-639 | v555 `TEXT[FILE,LET,CLEAR]` 整文件切行 | MOVE | 不会（仅 v555 参数表可达） | TR §7；D66 §1；`c1549eb` 无此函数 | 整体迁入 v555 `text.rs`（含测试） |
| `helpers.rs:varinfo_no_slot/varinfo_slot` base:190-198 / new:191-200 | 抽出通用 `varinfo_slot(name)` | KEEP | 不会（旧 `NO` 行为不变） | 重构；旧调用点同义 | 保留（中性重构） |
| `helpers.rs:str_less_ci` new:202-213 | v555 STRFIRST 的 ASCII 大小写不敏感比较 | MOVE | 不会（仅 STRFIRST 用） | TR §6/§7；D66 §1 | 随 STRFIRST 移入 v555 |
| `handlers/mod.rs:HandlerKind Debug` base:86 / new:86 | `derive(Debug)` | KEEP | 不会 | 仅测试/诊断 | 保留 |
| `handlers/mod.rs:HandlerKind::Product` base:137 / new:138-139 | 新增枚举变体 | MOVE | 不会（旧表不选） | TR §2；D66 §1 | 移入 v555 dispatch |
| `handlers/mod.rs:DEFAULT/NOOP/GUARD_*_0555` base:232 / new:235-293 | v555 默认槽/空操作/guard 表 | MOVE | 不会（仅 `version==555` 选） | TR §2；TB §4 | 移入 v555 的 121 槽覆盖表；共享保留 0.495 清单 |
| `handlers/mod.rs:build_handler_table` 版本选择 base:236 / new:298-306 | `let v555 = yscm.version == 555`，选 555 表 | MOVE | 不会（精确 555） | 同上 | 共享建表恢复 0.495 唯一路径；v555 install 时覆盖 |
| `handlers/mod.rs:default/noop 选择` base:243-245 / new:313-316 | 按 v555 选清单 | MOVE | 不会 | 同上 | 同上 |
| `handlers/mod.rs:PRODUCT 路由` base:283 / new:356-357 | `PRODUCT if v555` → `HandlerKind::Product` | MOVE | 不会（精确 555；旧 ysc 无 PRODUCT） | 同上 | 同上 |
| `handlers/mod.rs:CSV 路由` base:300-304 / new:375-382 | `v555 || has_line || i != 7` → Csv | MOVE | 不会（精确 555；0.495 保持 NoopEval） | TR §6.1；D66 §1 | 移入 v555 表；共享恢复 `has_line || i != 7` |
| `handlers/mod.rs:mod product` base:353 / new:432 | 新增子模块 | MOVE | 不会 | — | 移入 `other/handlers/` |
| `handlers/mod.rs:Product dispatch` base:546 / new:626 | 新分支 | MOVE | 不会 | — | 移入 `other/handlers/` |
| `handlers/mod.rs:测试` base:718 / new:799-865 | `v555_routes_alias_and_csv...`、`legacy_routes_keep_0495...` | MOVE / KEEP | 不会 | — | v555 测试移入模块；`legacy_routes_keep_0495` 保留为旧版回归 |
| `variables.rs:op_declare 字符串数组引用` base:227 / new:227-259 | `RefIntParamMode::SlotId` 时把字符串数组引用转槽号 | MOVE | 不会（profile 仅 555 为 SlotId） | TR §11.4；D66 §1 | 移入 v555 的声明/变量面 |
| `variables.rs:VARINFO SJISCODE 辅助参数` base:259-289 / new:291-336 | `SJISCODE` 在 555 才是查询项；`NO2` 从属参数 | MOVE | 不会（495 仍 `Inert`，行为同 c1549eb） | TR §6.8；D66 §1 | 移入 v555 `varinfo.rs`；共享保留 0.495 惰性路径 |
| `variables.rs:SJISCODE 分支` base:437 / new:486-534 | 首字符码（空/单字节/lead<<8|trail） | MOVE | 不会（旧 profile Inert） | TR §5.2/§5.3；D66 §1 | 移入 v555 |
| `variables.rs:STRFIRST/NO2` base:468 / new:566-624 | `[NO,NO2]` 闭区间字典序最小 | MOVE | 不会（旧 ysc 无 STRFIRST） | TR §6/§7；D66 §1 | 移入 v555 |
| `variables.rs:VARINFO helper 名单/detail` base:279-307 / new:318-356 | `NO2`、SJISCODE 门控后的名单与文案 | MOVE | 不会（旧路径结果不变） | 同上 | 随 v555 分支迁走 |

### 3.6 `crates/yu-gfx/src/state.rs`、`yu-save/src/state.rs`（重点，逐 hunk）

| 文件:符号/base 行 | 暂存改动 | 分类 | 旧版影响 | 证据 | 处置 |
|---|---|---|---|---|---|
| `gfx/state.rs:GfxConfig.v555` base:91 / new:92-94 | 新增精确 555 配置字段，默认 false | MOVE | 不会（默认 false） | TR §11.3；TB §3.2/§4 | 改成中性策略名（如 `button_number_info`）或 gfx 扩展；引擎只对精确 555 置位 |
| `gfx/state.rs:GfxConfig::new` base:105 / new:109 | 默认 false | MOVE | 不会 | 同上 | 随字段 |
| `gfx/state.rs:with_v555` base:124 / new:129-134 | 链式设置 | MOVE | 不会 | 同上 | 随字段改名 |
| `gfx/state.rs:CGINFO 缺层 LINT/LINT2` base:2194 / new:2205-2207 | v555 时缺层返回 0 | MOVE | 不会（旧 flag false） | TR §11.3；D66 §1 | 中性策略/扩展点 |
| `gfx/state.rs:CGINFO 图层 LINT/LINT2` base:2222 / new:2236-2247 | v555 时回读 `extra["LINT"/"LINT2"]` | MOVE | 不会（旧 flag false） | 同上 | 同上 |
| `gfx/state.rs:过渡两端全黑` base:6599 / new:6625-6643 | 无条件把全黑过渡降级为 `alpha=0` + Unimplemented | **REVERT** | **会**（所有版本的全黑过渡渲染改变） | 证据只覆盖 v555 标题帧；`c1549eb` 无此分支 | 恢复 c1549eb；若 v555 确需，做成显式 555 策略并补证据 |
| `gfx/state.rs:source_texture 回退` base:6688/6692 / new:6732-6745 | 命令时刻为空/黑时回退源层当前纹理 | **REVERT** | **会**（所有版本的过渡纹理解析改变） | 同上；无旧版回归 | 恢复 c1549eb；v555 单独策略 |
| `gfx/state.rs:YU_CG_FILE_TRACE` base:7312 / new:7366-7370 | 环境变量触发的 `eprintln!` 追踪 | DEFER | 不会（未设环境变量时） | 探针代码 | 从共享移除；需要时放 `tools/` 探针 |
| `yu-save/state.rs:ReservedFact.dno` 文档 base:50 / new:50-51 | 注释改成 0..=1024 | KEEP/DEFER | 不会 | — | 回退 DNO 校验后把注释改回 1..=1024（或写“由 VM 层分来源校验”） |
| `yu-save/state.rs:mem_slots` base:101 / new:103-113 | 稀疏内存槽字段 | KEEP | 不会（旧版不调用） | TB §4.1；D66 §1 | 保留为通用能力 |
| `yu-save/state.rs:mem_slots 初始化` base:134 / new:147 | 空表 | KEEP | 不会 | 同上 | 保留 |
| `yu-save/state.rs:mem_slots_len/mem_slot` base:168 / new:182-194 | 只读访问器 | KEEP | 不会 | 同上 | 保留 |
| `yu-save/state.rs:reserve DNO` base:331/334 / new:357/360 | `1..=1024` → `0..=1024` | **REVERT** | **会**（旧版 DNO=0 从报错变成接受；错误文案变） | `c1549eb:state.rs:331-334`；TB §4 | 恢复 1..=1024；给 v555 加中性 `reserve_with_dno_min`/`reserve_mem` |
| `yu-save/state.rs:reserve_group DNO` base:361/364 / new:387/390 | 同上 | **REVERT** | **会** | 同上 | 同上 |
| `yu-save/state.rs:reserve_image DNO` base:389/392 / new:415/418 | 同上 | **REVERT** | **会** | 同上 | 同上 |
| `yu-save/state.rs:save_mem` base:496 / new:522-538 | 内存槽序列化 | KEEP | 不会（旧版不调用） | D66 §1；TB §4.1 | 保留；命令面由 v555 模块调用 |
| `yu-save/state.rs:load_block_mem` base:631 / new:674-695 | 内存槽读块 | KEEP | 不会 | 同上 | 保留 |
| `yu-save/state.rs:load_image_mem` base:727 / new:792-813 | 内存槽读图像 | KEEP | 不会 | 同上 | 保留 |
| `yu-save/state.rs:mem 单测` base:1782 / new:1869-1914 | 内存槽往返测试 | KEEP | 不会 | — | 保留为通用能力测试 |

### 3.7 `crates/yu-engine/src/engine.rs`、`host-desktop`、`yu-trace`、`Cargo`（重点，逐 hunk）

| 文件:符号/base 行 | 暂存改动 | 分类 | 旧版影响 | 证据 | 处置 |
|---|---|---|---|---|---|
| `engine.rs:Engine::live_tasks` base:2054 / new:2055-2068 | 新增中性任务计数 API | KEEP | 不会 | TB §3.2 允许 runtime 选择外的中性 API | 保留；旧版不消费 |
| `engine.rs:boot disk_free_bytes` base:3179 / new:3190-3192 | 启动时查询存档盘剩余并注入 | KEEP | 不会（旧版不读该字段；多一次只读查询） | TR §6.4；D66 §1 | 保留；补全平台构建验证 |
| `engine.rs:boot with_v555` base:3218 / new:3231-3233 | `yscm.version == 555` 设置 gfx 精确门控 | KEEP | 不会（精确 555） | TB §3.2/§7.1 | 保留为 runtime 选择；随 gfx 中性策略改名 |
| `host-desktop/lib.rs:ScriptAction::Close` base:1349 / new:1349 | `main_finished()` → `main_finished() && live_tasks()==0` | **REVERT** | **会/未知**（旧版主任务结束后若有子任务，窗口不再退出） | `c1549eb:host-desktop:1349`；D66 §1 只述 v555 | 恢复 c1549eb；v555 用中性 engine exit policy |
| `host-desktop/lib.rs:窗口关闭事件` base:2706-2707 / new:2706-2709 | 同上 | **REVERT** | **会/未知** | 同上 | 同上 |
| `host-desktop/lib.rs:每帧 exit 判定` base:3534 / new:3536-3538 | 同上 | **REVERT** | **会/未知** | 同上 | 同上 |
| `yu-trace/record.rs:BootSummary` base:676 / new:677-679 | `disk_free_bytes=None` | KEEP | 不会（trace 回放默认未注入） | TR §6.4 | 保留 |
| `Cargo.lock` base:4211/4307 / new:4212/4309-4316 | 新增 `yu-host-disk` 依赖/包 | KEEP | 不会（构建层；旧版不读） | — | 保留；全平台 `cargo check` |
| `Cargo.toml` base:16 / new:17 | workspace 成员 | KEEP | 不会 | — | 保留 |
| `crates/yu-engine/Cargo.toml` base:23 / new:24 | 引擎依赖 host-disk | KEEP | 不会 | — | 保留；确认 wasm/android 目标可编译 |

### 3.8 语义、事件、诊断、中性 API（KEEP 为主）

| 文件:符号/base 行 | 暂存改动 | 分类 | 旧版影响 | 证据 | 处置 |
|---|---|---|---|---|---|
| `semantics.rs:头部文档` base:40 / new:40 | 说明 SJISCODE 已按 profile 门控 | KEEP/DEFER | 不会 | — | 随实现更新 |
| `semantics.rs:ScriptDeclMode/RefIntParamMode` base:59 / new:60-88 | 新增两个 v555 语义模式枚举 | KEEP（精确 555 数据派生） | 不会（旧版 runtime/value） | TB §3.2 明确允许 SemanticProfile 精确 555 数据派生 | 保留为数据；消费方移入 v555 模块，共享 handler 不分支 |
| `semantics.rs:SemanticProfile 字段` base:69 / new:99-102 | 新增 `script_decls`/`ref_int_param` | KEEP | 不会 | 同上 | 同上 |
| `semantics.rs:from_yscm_version` base:76-86 / new:109-135 | `>=555` → `==555`，并派生新模式 | KEEP | 不会（495 不变；556/600 按未取证走 legacy，符合任务书） | TB §2.2/§7.1；TR §6.8 | 保留；补 556/600 回归 |
| `semantics.rs:测试` base:108-113 / new:155-170 | 555 true；556/600 legacy | KEEP | 不会 | 同上 | 保留 |
| `code.rs:TEXT_FILE/TEXT_FILE_LINES` base:95/136 / new:96-99/141-142 | 新诊断码 | KEEP | 不会（旧路径不产生） | D66 §1/§2 | 保留；v555 模块使用 |
| `diag.rs:verbose` base:25 / new:26-30 | 新日志级别辅助 | KEEP | 不会 | — | 保留 |
| `event.rs:SaveHost::save_mem/load_block_mem/load_image_mem` base:748 / new:749-776 | trait 默认未实现方法 | KEEP | 不会（旧 host 不调用；默认报未实现） | TB §4.1；D66 §1 | 保留为通用能力端口 |
| `task.rs:Scheduler::live_tasks` base:350 / new:351-363 | 中性任务计数 | KEEP | 不会 | — | 保留 |
| `value.rs:VariableStore::find_by_id` base:668 / new:669-708 | 按槽号查找 | KEEP | 不会（新方法；v555 访问器使用） | TR §5.3 | 保留为通用能力；若 v555 移走仍可保留 |
| `yu-save/error.rs:MEM_MISSING` base:21/58 / new:22-23/61 | 新错误码 | KEEP | 不会 | D66 §2 | 保留 |
| `yu-save/host.rs:SaveHost 内存槽转发` base:97 / new:98-113 | 转发到 SaveState | KEEP | 不会 | TB §4.1 | 保留 |

### 3.9 新文件与测试

| 文件 | 暂存内容 | 分类 | 旧版影响 | 证据 | 处置 |
|---|---|---|---|---|---|
| `crates/yu-rt/src/scenario.rs` | v555 剧本 DSL 词法器（219 行，含单测） | MOVE | 不会（仅 v555 PRODUCT COM=22） | TR §11；D66 §1 | 整体迁入 `crates/yu-rt/src/other/scenario.rs` |
| `crates/yu-rt/src/vm/handlers/product.rs` | v555 PRODUCT（466 行） | MOVE | 不会（仅 v555 命令表） | TR §6；D66 §1 | 迁入 `crates/yu-rt/src/other/handlers/product.rs` |
| `crates/yu-rt/tests/fixture/product.rs` | v555 PRODUCT 测试（259 行） | MOVE | 不会 | — | 随 product 迁移 |
| `crates/yu-platform/host-disk/` | 通用磁盘剩余端口（Windows/Unix/其它） | KEEP | 不会（旧版不读；构建层新增） | TR §6.4 | 保留；全平台编译验证 |
| `crates/yu-save/examples/dump_sd*.rs` | 存档探针 | DEFER | 不会 | — | 工具最后收口 |
| `crates/yu-rt/tests/fixture/control.rs` | `gosub_keeps_pint_and_pstr_groups_apart` | MOVE/REWRITE | **会/未知** | D65；TB §3.3/§5.1 | 共享 `GosubFrame` 回退后该测试不能留在共享；迁入 v555 gosub 测试；另补旧版 GOSUB 回归 |
| `crates/yu-rt/tests/fixture/file_save.rs` | CSV 555、SAVE/LOAD MEM 测试与 `SaveRecorder` 内存槽字段 | MOVE | 不会（v555 参数表） | D66 §1 | 迁入 v555 测试；共享 fixture 恢复原样 |
| `crates/yu-rt/tests/fixture/system.rs` | `_STR` 动态访问器测试 | MOVE | 不会（v555 访问器） | TR §5.3 | 迁入 v555 测试 |
| `crates/yu-rt/tests/fixture/text.rs` | TEXT[FILE]、TEXTINFO 555 测试 | MOVE | 不会 | D66 §1 | 迁入 v555 测试 |
| `crates/yu-rt/tests/fixture/variables.rs` | SJISCODE 495 负用例（KEEP）；SJISCODE 555 正用例、STRFIRST（MOVE） | KEEP / MOVE | 不会（负用例锚定旧版；正用例仅 555） | D66 §1 | 负用例留共享；正用例迁 v555 |
| `crates/yu-rt/tests/support/mod.rs` | `ysc_version` + `build_yscm_with_version` | KEEP | 不会（默认 495；仅测试基础设施） | — | 保留；若 v555 测试独立 crate，评估抽公共测试支持 |
| `crates/yu-engine/examples/probe_*.rs`、`dump_*.rs`、`v555_*.rs` | 探针/普查 | DEFER | 不会 | — | 最后收口 |
| `crates/yu-platform/movie-plmpeg/examples/probe_mpg.rs` | 探针 | DEFER | 不会 | — | 最后收口 |
| `tools/probe555/`、`tools/v555/` | 逆向/探针工具 | DEFER | 不会 | — | 最后收口；不入产品构建 |
| `docs/formats/v555-*.tsv/md`、`docs/divergences/DIV-RT-065/066`、`docs/tasks/v555-*.md` | v555 证据/旧任务书 | DEFER | 不会 | — | 最后收口；D65 需按本轮裁决更新（见 §4） |
| `PROGRESS.md`、`docs/architecture/optimization-2026-11-20.md`、`yu-rt/docs/*` | 成果/计划/覆盖表 | DEFER | 不会 | — | 最后收口 |

### 3.10 明确不属于本轮（不得触碰）

| 项 | 状态 | 处置 |
|---|---|---|
| `yu-emote` 子模块指针 `1b46604 → a54cbd3` | 不在 stash；工作区既有 | 不修改、不提交；若主人需要，单独处理 |
| `msic/`（含 `0.4.2.exe`） | 主人的构建产物 | 不修改、不删除、不提交 |
| 备份多出的 `yu-cli/src/desktop.rs`、`host-desktop/Cargo.toml`、`NUL` | 不在 stash/当前工作区 | 不从 stash 恢复；若含 v555 改动请主人另给 |

---

## 4. 未知与待主人裁决

> 任务书 §6 Step 2：标“未知”的项先补测试/对拍，不许猜。以下每项在改码前需要
> 主人裁决或补证据。

### 4.1 必须裁决的架构项

| # | 项 | 现状 | 选项 | 建议 |
|---|---|---|---|---|
| D1 | GOSUB 三组拆分是否回退共享 | `GosubFrame` 已拆三组；D65 自称“全版本修复” | A. 回退共享，v555 模块自持三组帧；B. 保留共享，补旧版 `R*`/过渡宏回归并请主人裁决 | **A**（任务书 §3.3 已定；D65 证据不足，见 §5.1） |
| D2 | 独立模块落点 | **已裁决（2026-11-21）**：B，`crates/yu-rt/src/other/` 中性目录 | A. 新 crate + 中性 facade（后续迁移目标）；B. `yu-rt/src/other/` 同 crate 模块 | 按 B 落地；不建 `yu-rt-v555`。`other` 只是中性目录名，激活仍精确 `version == 555`；迁移计划见 §6.5 |
| D3 | `SemanticProfile` 新字段归属 | `script_decls`/`ref_int_param` 在共享 profile | A. 保留为精确 555 数据，消费者移入 v555；B. 移入 v555 profile | **A**（TB §3.2 允许；避免重复 profile） |
| D4 | `yu-gfx` LINT2 落点 | `GfxConfig.v555` + 两处 `if v555` | A. 中性策略位（`button_number_info`，引擎精确 555 置位）；B. `InfoQueryExtension` trait | A 为最小中性改动；B 更严格但需要 layer 视图 API |
| D5 | `yu-gfx` 两处过渡回退 | 无条件全黑降级/源纹理回退 | A. 直接 REVERT；B. 保留为 v555 专属策略（需补 v555 证据与旧版回归） | **A**（任务书最高优先级：旧版不变；当前证据只覆盖 v555） |

### 4.2 需要补测试/对拍的“未知”

| # | 项 | 为什么未知 | 需要的探针/测试 |
|---|---|---|---|
| U1 | `apply_script_statics` 的 `S_*` 装载期声明 | 旧版从运行时声明变装载期；是否改变旧语料可观察状态未证 | 旧版 fixture：`S_INT` 在入口前/后声明 + GOSUB 跳过；对拍 c1549eb |
| U2 | `text_reveal_complete` 无窗口即完成 | 旧版无窗口任务的调度行为未测 | 旧版 fixture：无窗口 + `WORD/RETURNCODE`；对拍 c1549eb |
| U3 | desktop `live_tasks` 退出条件 | 旧版主任务结束但子任务存活的场景未测 | 旧版 headless/desktop 生命周期用例；主人手工复测 |
| U4 | GOSUB 单 `args` 的旧版混用行为 | D65 只称“旧语料未混用”，未覆盖 `R*`/过渡宏 | 旧版 fixture：同条 `GOSUB[PINT=.. PSTR=..]` + `RETURN[RINT=.. RSTR=..]`；锚定 c1549eb 实际行为 |
| U5 | `_STR/_INT` 旧版可达性 | 旧 ysv 是否有这两个 id 未逐一核对 | 6 个旧样本 ysv 的 id/name 扫描；旧版 corpus 是否引用 |
| U6 | `S_*` / 过渡宏时序与黑闪的因果 | 黑闪可能不止 GOSUB 一个来源 | 任务书 §8 bisect 分组逐组应用，主人复测记录（见 §6.2） |
| U7 | `yu-host-disk` 全平台构建 | 新 workspace 成员/引擎依赖尚未在 wasm/android 目标验证 | `cargo check --workspace --all-targets` + Android/ wasm 目标构建 |

### 4.3 需要主人确认的现场项

1. **工作区隔离**：我当前在主人复测的 master 工作区；Step 3 改码前请给独立
   worktree/分支或明确授权。审计轮未改码。
2. **基线是否干净**：`c1549eb` 的 `cargo test --workspace` 与旧版进游戏复测
   是否已由主人确认？若否，Step 3 前需在独立工作树补跑。
3. **备份漂移**：`yu-cli/src/desktop.rs`、`host-desktop/Cargo.toml` 的备份改动
   是否需要另行恢复？当前工作区/ stash 都没有它们。
4. **D65 的处置**：确认按 A 回退共享；更新 `DIV-RT-065` 状态为“部分结论：
   v555 需要三组帧；共享布局按 c1549eb 保留”，避免后续又按“全版本修复”改回。
5. **模块落点**：D2 已裁决 B——`crates/yu-rt/src/other/` 中性目录；请确认
   目录/类型均不带 `v555`、激活仍精确 `version == 555`、556/600 不继承。

---

## 5. 为什么 GOSUB 拆分不能直接保留（D65 复核）

D65 的核心事实（手册例子 + 引擎三组数组）成立，但“全版本正确性修复，旧版行为不变”
这一步**证据不足**：

1. 它没有覆盖旧版 `RETURN` 的 `RINT/RFLT/RSTR` 混用；本次 diff 里
   `RETURN` 同样改了三组（`control.rs`），这是 D65 未验证的面。
2. 它没有覆盖旧版过渡宏（任务书 §5.1 点名 `es.SS.FADE` / `es.CGTA` / `CGACT`）
   的参数组合；这些宏正是旧版黑闪嫌疑链。
3. 它没有解释“淡入延迟 + 突然黑闪”的时序现象；三组拆分最多解释“值被覆盖”，
   解释不了时序。
4. 任务书 §3.3 已明确：在旧版闪黑未查明前，不能把“通用正确”当作“旧版不受影响”
   的证明；要保留共享拆分，必须先复现旧版闪黑、证明与拆分无关、补旧版
   GOSUB/RETURN 回归，再请主人裁决。

因此本审计的处置是：**共享层恢复 `args`/`ret`；v555 的三组帧放
`crates/yu-rt/src/other/gosub.rs`；旧版新增 GOSUB/RETURN 回归锚定 `c1549eb` 的实际行为**
（即使该行为是“同编号覆盖”，也作为旧版金标，不改）。

---

## 6. v555 独立模块方案

### 6.1 落地布局（D2 已裁决：`crates/yu-rt/src/other/` 中性目录）

```text
crates/yu-rt/src/other/
  mod.rs                     # install / 状态入口 / other_* 钩子方法
  runtime.rs                 # Runtime 状态（drop_files、last_text_lines、GOSUB 帧栈、选项）
  gosub.rs                   # 三组参数帧、GOSUB/RETURN 拦截、_PINT/_PFLT/_PSTR/_RINT/_RFLT/_RSTR
  sysvar.rs                  # _STR/_INT/_DROPFILE/_DROPFILE_NUM、VARINFO 变体
  scenario.rs                # 剧本 DSL 词法器（从共享 scenario.rs 迁入）
  startup.rs                 # 装载期声明、启动编排
  handlers/mod.rs            # 命令拦截分派（按命令名/参数名）
  handlers/product.rs        # PRODUCT 全部 COM
  handlers/text.rs           # TEXT[FILE]、TEXTINFO 空签名
  handlers/fileinfo.rs       # FILEINFO[CGFILE]
  handlers/varinfo.rs        # VARINFO[SJISCODE/STRFIRST/NO2]
  handlers/save_mem.rs       # SAVE/LOAD[MEM]
```

命名约定：目录/类型/函数名使用 `other`，不出现 `v555`；`555` 只允许出现在
证据注释、测试数据、引擎安装处的精确版本判断。共享 `yu-rt` 只保留通用 VM 核、
旧版 handler 原样、`other_*` 钩子调用；共享 `yu-gfx`/`yu-save` 只保留中性能力/
策略，不出现 v555 业务分支。

### 6.2 共享钩子契约（行为中性，默认 None）

目录内直接 `impl Vm`，用同 crate 的 `pub(crate)` 内部面；不需要 trait object/
facade。`Vm` 增加一个中性字段 `other: Option<other::State>`（默认 None），并增加
一个中性安装入口 `Vm::install_other_runtime()`；`other` 模块本身保持私有，
引擎不接触其内部类型。共享入口只调用 `other` 模块定义的钩子方法：

```rust
// crates/yu-rt/src/other/mod.rs（私有模块）
pub(crate) struct State { /* drop_files / last_text_lines / gosub 帧栈 / 选项 */ }

pub(crate) fn install(vm: &mut Vm) {
    vm.other = Some(State::new(/* from vm.tables() */));
}

impl Vm {
    pub(crate) fn other_intercept_command(
        &mut self, st: &mut ExecState, cmd: &Cmd,
    ) -> Option<Result<StepOutcome, VmError>> { /* None = 交回共享 handler */ }

    pub(crate) fn other_sysvar_get(
        &self, st: &ExecState, var: VarRef, indices: &[i64],
    ) -> Option<Result<Value, Diag>> { /* None = 共享 sysvar::read */ }

    pub(crate) fn other_sysvar_set(
        &mut self, st: &ExecState, var: VarRef, indices: &[i64], value: &Value,
    ) -> Option<Result<(), Diag>> { /* None = 共享写路径 */ }

    pub(crate) fn other_on_script_loaded(
        &mut self, script_id: u32,
    ) -> Option<Result<(), Diag>> { /* 装载期声明 */ }

    pub(crate) fn other_text_reveal_complete(&self) -> Option<bool> { /* 无窗口推进 */ }

    pub(crate) fn other_should_exit(&self) -> Option<bool> { /* 宿主退出策略 */ }
}
```

共享 `vm.rs`/`sysvar.rs`/`task.rs`/host 只在入口处调用这些 `other_*` 钩子；
`other` 为 `None` 时执行 `c1549eb` 原代码。所有钩子名中性，不含 `v555`。
引擎只在 `yscm.version == 555` 时调用 `vm.install_other_runtime()`；556/600 不安装。

```rust
// crates/yu-rt/src/vm.rs（共享层唯一新增的公开入口）
impl Vm {
    /// 中性运行面安装入口；默认不安装，旧版行为不变。
    pub fn install_other_runtime(&mut self) {
        crate::other::install(self);
    }
}
```

**借用处理**：钩子作为 `impl Vm` 方法，直接访问 `self.other` 与同 crate 内部
字段；若某钩子需要同时可变借用 `self.other` 和 `self` 的其它字段，用
`Option::take`/`as_mut` 拆借，避免双重借用。旧版 `None` 分支不改变任何可观察
行为。

### 6.3 同 crate 可直接使用的内部面

因为 `other/` 在 `yu-rt` 内部，以下方法**不需要先 pub 化**，直接在 `other`
模块内调用（当前可见性照旧）：

| 面 | 方法 | 用途 |
|---|---|---|
| 求值 | `eval_window`, `eval_lvalue_window` | TEXT/FILEINFO/VARINFO/PRODUCT/SAVE/LOAD |
| 写回 | `write_target` | PRODUCT/LOAD |
| 变量 | `read_var`, `canonical_var`, `var_shape`, `store`, `store_mut` | VARINFO/STRFIRST/TEXT[FILE] |
| 参数 | `param_plan`, `script_text_codec` | GOSUB/RETURN/SJISCODE |
| 表 | `tables`, `sysvars`, `semantics` | 命令表/系统变量/语义 profile |
| 诊断/事件 | `report`, `report_aligned_stub`, `report_unimplemented_variant`, `emit`, `emit_domain` | 挂号/事件流 |
| 资源 | `open_asset` / `list_asset_dir` | TEXT[FILE]/PRODUCT[COM=21/24] |
| 文本 | `text_clear` / `text_state` | TEXT[FILE] CLEAR |
| 存档 | `save_host_mut` / `with_save_host` | SAVE/LOAD[MEM] |
| 生命周期 | `live_tasks`, `disk_free_bytes` | exit policy / PRODUCT COM=34 |

只有跨 crate 的 `yu-gfx`/`yu-save` 能力需要中性公开 API（例如
`SaveState::reserve_mem`/`reserve_with_dno_min`、`GfxConfig` 的中性策略位）。
这比新 crate 少约 18 个 facade 方法，是本轮选 `other/` 的主要理由。

### 6.4 引擎装配（唯一版本判断）

```rust
// yu-engine::boot，装载表之后
if vm.tables().boot.yscm.version == 555 {
    vm.install_other_runtime();
    // gfx：中性策略（或扩展）
    gfx_config = gfx_config.with_button_number_info(true);
}
```

- 版本判断只允许在 `yu-engine` 的 runtime 选择处；共享 handler 不再出现
  `if version == 555`。
- `556/600` 没有样本：不安装扩展、不继承 555，保持 legacy 路径（semantics 已改
  精确 555，测试覆盖）。
- `yu-gfx` 的 LINT2 若采用中性策略位，引擎只对精确 555 置位；旧版默认 false，
  行为不变。若采用 `InfoQueryExtension`，同样只在 555 安装。

### 6.5 后续迁移：独立 crate（目标 A，非本轮）

本轮的边界由“同 crate 的 `other/` 目录”承担；迁移到独立 crate 的触发条件：
`other` 与共享内部的耦合稳定、钩子集合不再频繁变化、需要编译期隔离或复用给
其它运行面。迁移步骤：

1. 在 `yu-rt` 定义中性 `RuntimeExtension` + `VmAccess`/`VmView` facade
   （§6.3 的内部面按最小集合公开）；
2. `other/` 整体搬到中性名 crate（如 `crates/yu-other`），实现 trait；
3. 引擎安装点从 `vm.install_other_runtime()` 改为 `vm.install_extension(...)`；
4. 旧版测试证明未安装 = `c1549eb`；运行面 fixture 随模块迁移；
5. 共享层只保留 hook 调用与 facade，不出现业务分支。

迁移前不需要建空 crate；本轮不产生新的 workspace 成员/依赖。

### 6.6 迁移/落地顺序（确认后）

1. **Step 3 旧版零回归**：从 `c1549eb` 干净树重落 KEEP 项；回退所有 REVERT；
   跑金标 + 主人手工复测。未过门禁不建模块。
2. **扩展点**：在 `yu-rt` 加 `RuntimeExtension`/`VmAccess`/`VmView` 默认 None；
   旧版全量测试证明未安装 = 不变。
3. **`other/` 模块**：先落 `mod.rs`/`runtime.rs` + `gosub.rs` + `sysvar.rs`，
   再搬 `product.rs`/`text.rs`/`fileinfo.rs`/`varinfo.rs`/`save_mem.rs`/
   `scenario.rs`/`startup.rs`。
4. **gfx/save/lifecycle**：中性策略/能力接入；引擎精确 555 安装。
5. **测试**：v555 fixture 迁入模块；补 `corpus_v555`；旧版 GOSUB/过渡回归留在共享。
6. **验收**：headless 1500 帧到 `ES.TITLE.*`、0 Fatal；启动链顺序；desktop 180 帧；
   `gfx.info.missing/unsupported` 稳定帧为 0；`_STR/@3165/CGINFO[LINT2]` 噪声归零。
7. **收口**：更新 `PROGRESS.md`/真值台账/D65/D66/coverage；**不提交 git**，交主人审阅。

---

## 7. 旧版零回归门禁

### 7.1 金标命令（任务书 §4.1）

```bash
cargo build --workspace --all-targets
cargo test --workspace
cargo test -p yu-rt --lib
cargo test -p yu-rt --test fixture
cargo test -p yu-gfx --test info_query
cargo test -p yu-save
```

外加主人手工复测：0.4.2 之后的旧版进游戏不再出现淡入延迟/突然黑闪。
**不能用“全量测试太重”跳过。**

### 7.2 Step 3 落地顺序（从干净树重落，不反向补 stash）

1. 构建层：`yu-host-disk` crate、`Cargo.toml/lock`、`yu-engine/Cargo.toml`；
   `code/diag/event/trace` 中性项。
2. 中性 API：`Engine::live_tasks`、`disk_free_bytes`、`report_unimplemented_variant`、
   `VariableStore::find_by_id`、`Scheduler::live_tasks`、`semantics` 精确 555。
3. `yu-save` 内存槽能力（`mem_slots`/`save_mem`/`load_*_mem`/`MEM_MISSING`/
   `SaveHost` 默认方法），**不含 DNO 范围改动**。
4. 测试基础设施 `ysc_version`（默认 495）。
5. 跑金标 + 手工复测；全绿后再建 v555 模块。

### 7.3 bisect 分组（黑闪不是单一根因时）

任务书 §8 要求按组应用到干净 `c1549eb`，逐组复测：

| 组 | 内容 | 观察点 |
|---|---|---|
| A | GOSUB/`sysvar` 三组 | 宏参数/命令名是否被覆盖；淡入时序 |
| B | TEXT/text 推进 | 文本缓冲/黑闪时序 |
| C | FILEINFO/CGINFO/gfx（含两处过渡回退） | 过渡/黑屏/按钮层 |
| D | SAVE/LOAD/MEM | 存档路径/加载时序 |
| E | PRODUCT/scenario | 启动剧本/token 链 |
| F | engine/host/lifecycle | 退出/主循环/窗口 |
| G | 中性端口/诊断 | 预期无行为差异，作对照 |

每组的构建/日志由 agent 准备，**旧版进游戏复测由主人执行并记录**；未定位到
触发组前，不得开始 v555 模块。

---

## 8. 本轮不做 / 不做的事

- 不 pop/drop/clear stash；不 apply。
- 不修改 `msic/`、`yu-emote`、游戏数据。
- 不整分支 merge `v555-support`。
- 不在 master 工作区改码；未授权不做任何 git 写操作。
- 不继续加 v555 功能；不实现 E-mote（`deferred(master)`）。
- 不用 495 手册补 555 的洞；无证据保持 `Unimplemented`/过渡态。
- 不把“通用正确”当旧版无回归的证明（D65 的教训）。

---

## 9. 待主人确认（审计表签收项）

请主人逐项确认/裁决：

- [ ] 现场核对无误（§1）；备份漂移按 §1.3 处理。
- [ ] 分类表认可（§3），特别是 7 类 REVERT：`GosubFrame`、`_PINT/_RSTR`、
  `apply_script_statics`、`text_reveal_complete`、gfx 两处过渡、save DNO、
  desktop 退出条件。
- [ ] D1：GOSUB 共享回退、v555 自持三组帧（建议 A）。
- [x] D2：已裁决 B——`crates/yu-rt/src/other/` 中性目录；新 crate 为后续迁移目标。
- [ ] D3：`SemanticProfile` 保留精确 555 数据、消费者移入 v555（建议 A）。
- [ ] D4：gfx LINT2 用中性策略位（建议 A）或扩展 trait（B）。
- [ ] D5：gfx 两处过渡回退（建议 A），v555 专属策略需另补证据。
- [ ] 基线：`c1549eb` 的旧版金标/手工复测是否已确认干净；否则需独立工作树补跑。
- [ ] 工作区：提供独立 worktree/分支或明确授权，之后才进入 Step 3。
- [ ] 备份多出的 `yu-cli/src/desktop.rs`、`host-desktop/Cargo.toml` 是否需要另给。

---

## 附录 A：39 个 tracked 文件与初判汇总

| 文件 | 初判 | 说明 |
|---|---|---|
| `Cargo.lock`, `Cargo.toml`, `crates/yu-engine/Cargo.toml` | KEEP | 构建层 |
| `PROGRESS.md`, `docs/architecture/optimization-2026-11-20.md`, `crates/yu-rt/docs/*` | DEFER | 文档 |
| `crates/yu-engine/examples/probe_*.rs` | DEFER | 探针 |
| `crates/yu-engine/src/engine.rs` | KEEP（runtime 选择） | live_tasks/disk_free/with_v555 |
| `crates/yu-gfx/src/state.rs` | REVERT + MOVE | 过渡回退 REVERT；LINT2 MOVE |
| `crates/yu-gfx/tests/info_query.rs` | MOVE | v555 LINT2 测试 |
| `crates/yu-platform/host-desktop/src/lib.rs` | REVERT | 退出条件 |
| `crates/yu-rt/src/{code,diag,event,lib,task,value}.rs` | KEEP / MOVE | code/diag/event/task/value 中性；lib `pub mod scenario` MOVE |
| `crates/yu-rt/src/semantics.rs` | KEEP | 精确 555 数据派生 |
| `crates/yu-rt/src/sysvar.rs` | REVERT + MOVE | 三组读取 REVERT；v555 访问器 MOVE |
| `crates/yu-rt/src/vm.rs` | REVERT + MOVE + KEEP | 见 §3.1 |
| `crates/yu-rt/src/vm/handlers/control.rs` | REVERT + MOVE | 见 §3.2 |
| `crates/yu-rt/src/vm/handlers/file_io.rs` | REVERT + MOVE | 见 §3.4 |
| `crates/yu-rt/src/vm/handlers/helpers.rs` | KEEP + MOVE | `varinfo_slot` KEEP；`str_less_ci` MOVE |
| `crates/yu-rt/src/vm/handlers/mod.rs` | MOVE + KEEP | v555 表/路由 MOVE；legacy 测试 KEEP |
| `crates/yu-rt/src/vm/handlers/text.rs` | MOVE | TEXT[FILE]/TEXTINFO |
| `crates/yu-rt/src/vm/handlers/variables.rs` | MOVE | VARINFO 变体/声明 slot |
| `crates/yu-rt/tests/fixture.rs` | MOVE | `mod product` |
| `crates/yu-rt/tests/fixture/{control,file_save,system,text,variables}.rs` | MOVE / KEEP | v555 测试 MOVE；495 负用例 KEEP |
| `crates/yu-rt/tests/support/mod.rs` | KEEP | 测试基础设施 |
| `crates/yu-save/src/{error,host,state}.rs` | KEEP + REVERT | 内存槽 KEEP；DNO 范围 REVERT |
| `crates/yu-trace/src/record.rs` | KEEP | 默认 None |

## 附录 B：68 个 untracked 文件与初判

| 组 | 文件 | 初判 |
|---|---|---|
| v555 源码 | `crates/yu-rt/src/scenario.rs`、`crates/yu-rt/src/vm/handlers/product.rs` | MOVE |
| v555 测试 | `crates/yu-rt/tests/fixture/product.rs` | MOVE |
| 通用端口 | `crates/yu-platform/host-disk/{Cargo.toml,src/lib.rs}` | KEEP |
| 探针/示例 | `crates/yu-engine/examples/*`、`crates/yu-platform/movie-plmpeg/examples/probe_mpg.rs`、`crates/yu-save/examples/*` | DEFER |
| 逆向工具 | `tools/probe555/*`、`tools/v555/*` | DEFER |
| 证据文档 | `docs/formats/v555-*`、`docs/divergences/DIV-RT-065/066` | DEFER（D65 需更新） |
| 旧任务书 | `docs/tasks/v555-task-book.md`、`v555-startup-prompt.md` | DEFER（已被新任务书取代） |

---

## 附录 C：本轮只读产物（scratch，不在仓库）

```text
<side-repo>/workbench/tmp/v555-isolation-audit-20261121/
  stash-tracked.patch        # git stash show -p
  worktree-tracked.patch     # git diff HEAD
  stash-unified0.patch       # hunk 级清单
  stash-untracked.txt        # 68 项
  worktree-untracked.txt
  v555-truth.md              # 从 stash ^3 提取
  DIV-065.md
  DIV-066.md
  old-task-book.md
  old-startup-prompt.md
```

这些文件是审计证据，不是仓库内容；未复制进工作区。

---

## 一句话复述

> 我在做 v555 的**抽离**，不是继续加功能，也不是改 495。
> v555 是独立商业分支；555 没有手册，495 手册不是真值。
> 共享层只保留通用能力与行为中性扩展点；旧版默认一律不动。
> 没有 555 证据就不实现；未经主人授权不做任何 git 写操作。
