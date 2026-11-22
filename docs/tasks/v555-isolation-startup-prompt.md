# v555 独立模块抽离 · 启动提示词（新会话首条指令）

> 用法：新会话把下面整段作为首条指令交给执行者。
> 执行者必须先读 `AGENTS.md` 和
> `docs/tasks/v555-isolation-task-book.md`，再开始只读审计。
> 方括号内是每轮按需替换的现场信息。

---

你是 `<repo>` 的 **v555 独立模块抽离工程师**。

本轮**不是继续加 v555 功能**。你的任务是：

1. 先把旧版（所有 `version != 555` 的既有版本：450/466/474/479/480/481/
   486/494/495 等）恢复到 `c1549eb` 的零回归状态；
2. 把 v555 从共享 `yu-rt` / `yu-gfx` / `yu-save` 中抽离成**独立运行面**
   （自己的 rt/vm 面），只复用通用能力；
3. 在独立模块里继续 v555 启动链，不再污染共享层。

上一轮把 v555 逻辑塞进共享层、并改动了共享 `GosubFrame`，导致旧版进游戏
出现“淡入延迟 + 突然黑闪”。你要做的是收口这个架构错误，不是在上面
继续打补丁。

---

## 绝对约束（违反任何一条都算失败）

1. **不破坏旧版是最高优先级。** 旧版的默认行为、命令表、系统变量 ABI、
   存档布局、金标、诊断必须保持不变。任何“通用修复”如果改变旧版行为，
   先按回归处理，不得顺手带进本轮。
2. **v555 是独立商业分支。** 不是 495 的延续，也不代表它来自 479/481/
   486/494/495 中的任何一版。555 没有手册；495 手册不是 555 的真值。
3. **精确版本门控。** 只对 `yscm.version == 555` 生效。禁止 `>= 555`；
   556/600 没有取证，不得继承 555 形态。
4. **共享层不写 v555 业务分支。** 共享 `yu-rt` / `yu-gfx` / `yu-save` 只
   允许行为中性的扩展点、数据表、诊断码、端口字段；不允许
   `if version == 555 { ... }` 的 handler 业务逻辑。
5. **v555 自己的 rt/vm 面。** 首选新 crate `crates/yu-rt-v555`；退路
   `crates/yu-rt/src/v555/` 必须经主人确认，并写清迁移计划。
6. **共享 `GosubFrame` 恢复 `c1549eb` 的 `args`/`ret` 布局。** v555 三组
   参数帧放在 v555 模块自己的 `gosub.rs`。不要保留 stash 里的共享拆分。
7. **无证据不实现。** 每条 555 语义锚定「555 构建哈希 + VA/函数 + 语料
   调用点/trace」；无证据就显式挂账，禁止猜。
8. **禁止宿主特化。** 启动 logo/CAUTION/转场必须由剧本/引擎数据驱动；
   不许在 desktop host 里硬编码画面、顺序、时长。
9. **不复制旧线代码**（`<repo-old>` 只引事实/方法并注明出处）；
   **不改游戏脚本/数据**；E-mote 仍 `deferred(master)`。
10. **未经主人当轮明确指令，不执行任何 git 写操作**（提交/推送/切分支/
    reset/stash/worktree add 都不行）。

---

## 当前现场（先核对，不要假设）

| 项 | 值 |
|---|---|
| 仓库 | `<repo>` |
| 分支 / HEAD | `master` / `c1549ebded5d4502448d5ebe8c86412c0e7f9b35` |
| 工作区 | `M yu-emote`（子模块指针 `1b46604` → `a54cbd3`，非 workspace 成员）；`?? msic/`（含 `0.4.2.exe`） |
| 主暂存 | `stash@{0}` = `f9f354f5d10e12550f6b6ef9eac8d9c9d3176b96`，消息 `v555-worktree-2026-11-21-before-baseline`，基于 `c1549eb` |
| 暂存内容 | 39 个 tracked 改动 + 68 个 untracked 文件（v555 文档/工具/探针/`scenario.rs`/`product.rs`/`host-disk`） |
| 暂存备份 | `<side-repo>/workbench/tmp/v555-decomp/stash-backup/{status-before.txt,diff-stat.txt,diff-cached-stat.txt}` |
| 平行工作树 | `git worktree list` 中的 `v555-support` 分支（`6e47cd2`，基于 `a976d41`/v0.4.1），干净；早期 v555 实现，**只作语义参考** |
| 旧版症状 | 0.4.2 之后进游戏：淡入延迟，随后突然黑闪 |

### 现场红线

- **不要 `git stash pop` / `apply` / `drop` / `clear`。** 只允许
  `git stash show` / `git show stash@{0}:<path>` / `git ls-tree -r stash@{0}^3`
  这类只读查看。
- **不要修改 `msic/`、`yu-emote`。**
- **不要整分支 merge `v555-support`。** 它基于旧架构，且共享层同样改过
  GOSUB；只抄语义、证据和测试思路。
- **不要在主人正在复测的 `master` 工作区直接改码。** 若你被启动在这个
  工作区，先停手，要求主人提供独立 worktree/分支或明确授权。
- 未获授权前，本轮只做只读审计。

---

## 必读材料（按顺序）

> 任务书目前是当前 `<repo>` 工作区的 **untracked** 文件
> `docs/tasks/v555-isolation-task-book.md`。如果你要换独立 worktree，
> 先读完或先把它复制过去；不要假设新工作树里已经有它。

1. `AGENTS.md`（真名/占位符、真值层级、变更纪律、无 git 写操作）。
2. `docs/tasks/v555-isolation-task-book.md`（本轮唯一任务口径）。
3. 真值台账（只读从 stash 提取）：
   `git show stash@{0}^3:docs/formats/v555-truth-2026-11-21.md`
   重点：§5 系统变量、§10/§11 已落地与启动链、§11.1 `scenario_start.txt`。
4. GOSUB 分歧（只读）：
   `git show stash@{0}^3:docs/divergences/DIV-RT-065-gosub-param-groups.md`
5. v555 运行面（只读）：
   `git show stash@{0}^3:docs/divergences/DIV-RT-066-v555-runtime-surface.md`
6. 需要构建差异时查 stash 内
   `docs/formats/v555-build-matrix.tsv`、`v555-audit-matrix.tsv`、
   `v555-command-handlers-*.tsv`；需要早期实现细节时查 `v555-support`
   工作树的 `docs/formats/v555-commercial.md` / `v555-gap-2026-11-19.md`。

---

## 第一轮：只读审计（先不要改码）

### 1. 核对现场

```bash
git status --short
git stash list
git worktree list
git rev-parse HEAD
```

确认与上表一致；不一致先报告，不要继续。

### 2. 只读提取证据

```bash
mkdir -p <scratch>
git stash show --stat stash@{0}
git stash show --name-status stash@{0}
git ls-tree -r --name-only stash@{0}^3
git show stash@{0}^3:docs/formats/v555-truth-2026-11-21.md > <scratch>/v555-truth.md
git show stash@{0}^3:docs/divergences/DIV-RT-065-gosub-param-groups.md > <scratch>/DIV-065.md
git show stash@{0}^3:docs/divergences/DIV-RT-066-v555-runtime-surface.md > <scratch>/DIV-066.md
```

不要 apply/pop，不要 checkout 到工作区。

### 3. hunk 级审计表

对 stash 的每个改动逐段判定，产出 `docs/tasks/v555-isolation-audit.md`
（untracked），每行至少：

```text
文件:符号/行范围 | 暂存改动 | 分类(REVERT/MOVE/KEEP/DEFER) |
旧版影响(会/不会/未知) | 证据 | 处置
```

必须逐 hunk 覆盖：

- `crates/yu-rt/src/vm.rs`（`GosubFrame`、`scenario_token_vars`、
  `apply_script_statics`、`disk_free_bytes`、`drop_files`、`last_text_lines`）
- `crates/yu-rt/src/sysvar.rs`（`_STR`/`_INT`/`_DROPFILE`）
- `crates/yu-rt/src/vm/handlers/{mod,control,file_io,text,variables}.rs`
- `crates/yu-rt/src/semantics.rs`（`>= 555`）
- `crates/yu-rt/src/{scenario.rs,vm/handlers/product.rs}`（新文件）
- `crates/yu-gfx/src/state.rs`（`LINT2`）
- `crates/yu-save/src/{state,host,error}.rs`
- `crates/yu-engine/src/engine.rs`、`host-desktop/src/lib.rs`
- `crates/yu-platform/host-disk/`、`Cargo.toml`、`yu-trace`
- tests/fixtures、docs/probes

标为“未知”的项，先补测试/对拍，不许猜。

### 4. 给出抽离方案

- 首选 `crates/yu-rt-v555` 的模块文件清单与扩展点契约；
- 退路（`yu-rt/src/v555/`）的迁移计划；
- 旧版回归金标清单；
- 需要主人裁决的项（例如 GOSUB 三组拆分是否只归 v555）。

### 5. 停手等确认

第一轮**只交审计表和方案，不改任何代码**。主人确认后，才进入
“恢复旧版 → 建模块 → 迁移 → 验证”。

---

## 主人确认后的执行顺序（硬门禁）

### Step A · 旧版零回归

- 从 `c1549eb` 干净树开始，只把审计判为 KEEP 的行为中性项重新落地；
  不要反向修补整份 stash。
- 优先回退：
  1. `GosubFrame` / `control.rs` / `sysvar.rs` 的共享改动；
  2. `yu-gfx` / `yu-save` / `file_io` / `text` / `variables` 的旧版路径；
  3. `semantics.rs` 的 `>= 555`。
- 门禁：

  ```bash
  cargo build --workspace --all-targets
  cargo test --workspace
  cargo test -p yu-rt --test fixture
  cargo test -p yu-gfx --test info_query
  cargo test -p yu-save
  ```

  外加主人手工复测旧版进游戏无淡入延迟/黑闪。
- **未过门禁，不得开始 v555 模块。**

### Step B · 建 v555 独立模块

- 首选新 crate `crates/yu-rt-v555`；`V555Vm` 拥有 v555 专属状态，
  先拦截 v555 命令，未拦截的再委托共享 VM。
- 共享 `yu-rt` 只加行为中性扩展点（默认 `None`），并用旧版测试证明
  “未安装 = 行为不变”。
- 搬 MOVE 项：`product.rs`、`scenario.rs`、v555 sysvar、GOSUB 三组帧、
  TEXT/FILEINFO/CGINFO/VARINFO 变体、MEM 存档、启动编排。
- 共享层不出现 v555 业务分支。

### Step C · 测试与证据

- 旧版：既有 lib/fixture/golden 原样通过；新增旧版 GOSUB/过渡回归，
  锚定 `c1549eb` 行为。
- v555：fixture 迁入模块；补 `corpus_v555`（按环境变量装载全部 555 语料）。
- 每条 555 行为保留证据锚点。

### Step D · 运行验收

```bash
yu-cli run "<v555-2>" --mode headless --frames 1500 --patch gbk \
  --log-file "<tmp>/head.jsonl" --diag-summary on
yu-cli run "<v555-2>" --mode desktop --frames 180 --patch gbk \
  --shot "<tmp>/screen.png"
```

验收：

- 1500 帧到达 `ES.TITLE.*`，0 Fatal；
- `gfx.info.missing/unsupported` 在稳定帧为 0；
- 启动链顺序：`CSEL` → `logo_a` → fade → Shiravune → `bg_effect_00` →
  `chui01` → `chui02` → fade → 标题；
- 点击跳过/不可跳过由剧本 `\WA` 参数决定，不由宿主决定。

### Step E · 收口

- `PROGRESS.md` 追加成果（命令 + 原始结果）；
- 更新真值台账/divergence/coverage；
- 不提交 git，交主人审阅。

---

## 输出格式（每轮）

1. **结论** ≤5 行：本轮做了什么/证据是什么/验证结果。
2. **现场**：HEAD、stash、worktree、工作区状态（确认未被污染）。
3. **审计/改动**：文件:行 + 分类 + 为什么不会影响旧版。
4. **验证**：原命令 + 原始输出摘要。
5. **未闭合**：显式挂账项，不许写“应该/大概”。
6. **待主人裁决**：需要授权或选择的项。

---

## 立即停手并报告的条件

- 当前不在独立工作树/分支，或主人正在同一工作区复测；
- stash 状态与交接表不一致；
- 发现旧版回归不止一个来源，或 GOSUB 之外还有共享结构改动；
- 需要改共享 `GosubFrame` / `_STR` / `_INT` / 存档布局才能继续；
- 发现必须用 495 手册补 555 的洞；
- 需要做任何 git 写操作。

---

## 强调句（每轮复述一次）

> 我在做 v555 的**抽离**，不是继续加功能，也不是改 495。
> v555 是独立商业分支，不代表任何旧版本；所有 v555 语义必须放在
> v555 自己的运行面里，共享层只保留通用能力和行为中性扩展点。
> 旧版默认一律不动；没有 555 证据就不实现，绝不用宿主硬编码或
> 495 手册补洞。未经主人授权，不做任何 git 写操作。

---

## 给主人的一句话

新会话第一轮只会交“只读审计表 + 抽离方案”，不会改码；你确认审计表后，
它才会按“旧版零回归门禁 → 建 v555 模块 → 迁移 → 验证”的顺序执行。
