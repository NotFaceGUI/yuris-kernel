# v555 独立模块抽离 · 任务书（交接版）

> 版本：2026-11-21
> 适用：接手 v555 抽离的新会话 / 新执行者。
> 关系：本文件**取代** stash 内 `docs/tasks/v555-task-book.md` 的架构口径。
> 旧任务书里的 v555 事实台账仍可参考，但其中“共享代码 + exact-555 gate”
> 的做法已被判定不符合边界，不得再作为实现依据。
>
> 现场提示：本文件目前是当前 `<repo>` 工作区的 **untracked** 文档。
> 如果换独立 worktree 执行，先把本文件复制过去，不要假设新树里已有。

---

## 0. 一句话结论

上一轮把 v555 逻辑塞进了共享 `yu-rt` / `yu-gfx` / `yu-save`，并改动了
共享数据结构（尤其是 `GosubFrame`）。旧版因此出现“淡入延迟 + 突然黑闪”。
本轮**不是继续加 v555 功能**，而是：

1. 先把旧版恢复到 `c1549eb` 的行为，零回归；
2. 把 v555 抽离成**独立运行面**（自己的 rt/vm 面），只复用通用能力；
3. 在独立模块里继续 v555 启动链，不再污染共享层。

---

## 1. 交接现场（硬事实，先核对再动手）

| 项 | 值 |
|---|---|
| 仓库 | `<repo>` |
| 当前分支 / HEAD | `master` / `c1549ebded5d4502448d5ebe8c86412c0e7f9b35` |
| 工作区 | `M yu-emote`（子模块指针 `1b46604` → `a54cbd3`，非 workspace 成员）；`?? msic/`（含 `0.4.2.exe`，19,961,856 字节） |
| 主暂存 | `stash@{0}` = `f9f354f5d10e12550f6b6ef9eac8d9c9d3176b96`，消息 `v555-worktree-2026-11-21-before-baseline`，基于 `c1549eb` |
| 暂存内容 | 39 个 tracked 文件改动 + 68 个 untracked 文件（含 v555 文档、工具、探针、`scenario.rs`、`product.rs`、`host-disk`） |
| 暂存备份 | `<side-repo>/workbench/tmp/v555-decomp/stash-backup/{status-before.txt,diff-stat.txt,diff-cached-stat.txt}` |
| 平行工作树 | `git worktree list` 中的 `v555-support` 分支（`6e47cd2`，基于 `a976d41` / v0.4.1），工作区干净；是早期 v555 实现，**只作语义参考** |
| 旧版症状 | 0.4.2 之后进游戏：淡入延迟，随后突然黑闪；主人将用 `c1549eb` 基线复测 |

> 注：`git stash list` 的 reflog 标签显示 `On main`，但
> `git rev-parse stash@{0}^` 实测基提交是 `c1549eb`（与当前 `master` 同点）。
> 以基提交为准，不要被标签误导。

### 1.1 绝对不能碰

- **不要 `git stash pop` / `apply` / `drop` / `clear`。** 主暂存是主人回滚和
  对照的唯一现场；只允许 `git stash show` / `git show stash@{0}:<path>` /
  `git ls-tree -r stash@{0}^3` 这类只读查看。
- **不要修改 `msic/`、`yu-emote`。** 前者是主人的 0.4.2 构建产物，后者是
  子模块指针，均不属于本轮。
- **不要整分支 merge `v555-support`。** 它基于旧架构（`vm/handlers.rs` 还是
  单体文件），且同样改了共享 `GosubFrame`；只能抄语义、证据和测试思路。
- **不要在主人正在复测的 `master` 工作区直接改码。** 先让主人给独立
  worktree / 分支；未获授权前只读审计。
- **未经主人当轮明确指令，不执行任何 git 写操作**（提交、推送、切分支、
  reset、stash、worktree add 都算）。

---

## 2. 不可违反的边界

1. **v555 是独立商业分支。** 不是 0.495 的延续，也不代表它来自
   0.479/0.481/0.486/0.494/0.495 中的任何一版。555 没有手册；495 手册
   不是 555 的真值。
2. **精确版本门控。** 只对 `yscm.version == 555` 生效。禁止 `>= 555`；
   556/600 没有取证，不得继承 555 形态。
3. **无证据不实现。** 每条 555 语义锚定「555 构建哈希 + VA/函数 +
   语料调用点/trace」。无证据就显式挂账，禁止猜。
4. **禁止宿主特化。** 启动 logo / CAUTION / 转场必须由剧本/引擎数据
   驱动；不许在 desktop host 里硬编码画面、顺序、时长。
5. **不改游戏脚本/数据。** E-mote 维持 `deferred(master)`。
6. **不复制旧线代码。** `<repo-old>` 只可引用事实与方法，注明出处。
7. **最高优先级：不破坏旧版。** 旧版的默认行为、命令表、系统变量 ABI、
   存档布局、金标和诊断必须保持不变。任何“通用修复”如果改变旧版行为，
   先按回归处理，不得顺手带进本轮。

---

## 3. 目标架构（按此收口）

```text
crates/yu-rt/                通用 VM 核（旧版行为冻结）
  ├─ tables / expr / value / task / diag / event / ports
  ├─ 现有 handler 语义（所有 `version != 555` 的既有版本）保持原样
  └─ RuntimeExtension 扩展点（新增、默认 None、缺席时行为不变）

crates/yu-rt-v555/           v555 独立运行面（新 crate；v555 自己的 rt/vm 面）
  ├─ V555Runtime / V555Vm    v555 运行门面，持有 v555 专属状态
  ├─ handlers/               PRODUCT、TEXT[FILE]、FILEINFO[CGFILE]、
  │                          CGINFO[LINT2]、VARINFO 变体、MEM 存档
  ├─ gosub.rs                v555 三组参数帧（PINT/PFLT/PSTR、RINT/RFLT/RSTR）
  ├─ sysvar.rs               v555 的 _STR/_INT/_DROPFILE 访问器
  ├─ scenario.rs             v555 场景 DSL 词法/解析/宏入口
  └─ startup.rs              启动剧本链的 v555 编排

crates/yu-engine/            只做装配
  └─ if yscm.version == 555 { V555Runtime::install(&mut vm) }
```

### 3.1 “自己的 rt/vm”怎么落地

- **首选**：新 crate `crates/yu-rt-v555`。`V555Vm` 拥有 v555 的命令拦截、
  参数布局、系统变量、场景解释器和启动编排；通用命令可以委托给共享
  `yu_rt::Vm`，但 **v555 语义不得写回共享代码**。
- **不要求**在本轮复制整个 121 命令 VM。`V555Vm` 可以是组合式门面：
  先拦截 v555 专属命令，未拦截的再交给共享 VM。
- **退路**：如果本轮评估后认为独立 crate 的接口成本过高，可先用
  `crates/yu-rt/src/v555/` 子模块过渡，但必须满足：
  1. 独立目录、独立文件、独立测试；
  2. 共享文件里不出现 v555 业务分支（只允许扩展点注册）；
  3. 任务书里写清升级到独立 crate 的迁移计划；
  4. 先经主人确认，不能自行降级。

### 3.2 共享层扩展点的硬要求

- 共享 `yu-rt` 只能新增**行为中性**的扩展点，例如：

  ```rust
  pub trait RuntimeExtension {
      fn intercept_command(/* ... */) -> Option<Result<StepOutcome, VmError>>;
      fn sysvar_get(/* ... */) -> Option<Value>;
      fn sysvar_set(/* ... */) -> Option<()>;
      fn on_boot(/* ... */) -> Result<(), Diag>;
  }
  ```

- 扩展点默认 `None`；未安装时，旧版路径的执行结果、结构布局、诊断文本
  必须与 `c1549eb` 完全一致。
- **不允许**在共享 handler 里写 `if version == 555 { ... } else { ... }`
  的业务分支。共享层只允许两种版本判断：
  1. `yu-engine` 启动时的 runtime 选择；
  2. `SemanticProfile` 的精确 555 数据派生（且不得 `>= 555`）。
- 静态审计命令：

  ```bash
  rg -n "v555|555" crates/yu-rt/src crates/yu-gfx/src crates/yu-save/src crates/yu-engine/src
  ```

  每一处命中都要能归入：runtime 选择 / 数据表 / 证据注释 / 测试。
  出现业务分支即判失败。

### 3.3 `GosubFrame` 的硬处理

- 共享 `crates/yu-rt/src/vm.rs` 的 `GosubFrame` **恢复为 `c1549eb` 的
  `args: [Option<Value>; 16]` / `ret: [Option<Value>; 16]` 布局**。
- v555 的三组参数帧（`args_int/args_flt/args_str`、`ret_int/ret_flt/ret_str`）
  放在 `yu-rt-v555` 自己的 `gosub.rs`，由 v555 runtime 拦截 GOSUB/RETURN。
- 理由：DIV-RT-065 把三组拆分称为“全版本通用修复”，但它确实改变了共享
  结构；在旧版闪黑未查明前，不能把“通用正确”当作“旧版不受影响”的证明。
  如果新执行者要用证据推翻这一点，必须先补旧版回归、再请主人裁决，
  不能直接保留共享拆分。

---

## 4. 共享层职责表

| 面 | 共享 `yu-rt` / `yu-gfx` / `yu-save` | v555 独立模块 |
|---|---|---|
| 表装载 / YSCM / YSTB / 表达式 | 保留 | 复用 |
| 任务 / 调度 / 控制流骨架 | 保留（旧语义） | 复用 + 自己的 GOSUB 帧 |
| 命令分派表 | 旧版 495 清单原样 | v555 自己的 121 槽表与覆盖 |
| `PRODUCT` | 旧版不新增业务分支 | 全部 COM 语义 |
| `TEXT` / `TEXTINFO` | 旧版路径原样 | `TEXT[FILE]`、无窗口推进等 |
| `FILEINFO` | 旧版原样 | `CGFILE` 存在性探测 |
| `CGINFO` | 旧版字段语义原样 | `LINT2` 按钮号 |
| `VARINFO` | 旧版惰性参数原样 | `SJISCODE` / `STRFIRST` 变体 |
| `_STR` / `_INT` / `_DROPFILE` | 旧版访问器原样 | v555 访问器 |
| `SAVE` / `LOAD` | 旧版容器原样 | `MEM` 槽 |
| 场景 DSL / 启动链 | 不新增 | `scenario.rs` / `startup.rs` |
| 图形宿主 | `GraphicsHost` 契约不变 | v555 查询包装 / 扩展 |

### 4.1 跨 crate 通用能力的边界

`yu-save` 的内存槽、`yu-gfx` 的图层 extra 查询、`host-disk` 的磁盘空间
这类能力，可以以**行为中性**的通用接口保留或新增：

- 旧版不调用时，行为不变；
- 旧版调用时，默认值/路径/容器布局不变；
- v555 的命令语义（`MEM=...`、`CGINFO[LINT2]` 的按钮号、`PRODUCT[COM=34]`
  的磁盘剩余）仍放在 v555 模块，由 v555 runtime 调用这些通用接口。

判定标准不是“代码在哪个 crate”，而是“**旧版路径是否被改变**”。

### 4.1 旧版回归金标

至少包括：

```bash
cargo build --workspace --all-targets
cargo test --workspace
cargo test -p yu-rt --lib
cargo test -p yu-rt --test fixture
cargo test -p yu-gfx --test info_query
cargo test -p yu-save
```

外加主人手工复测：0.4.2 之后的旧版进游戏不再出现淡入延迟/黑闪。
**不能用“全量测试太重”跳过旧版回归。**

---

## 5. 暂存改动审计清单（起点，不是结论）

> 以下按 `git stash show --stat stash@{0}` 和
> `git ls-tree -r --name-only stash@{0}^3` 列出。新执行者必须做 **hunk 级**
> 审计，逐段判定，不能按文件一刀切。

| 文件/目录 | 暂存改动 | 初始判定 | 说明 |
|---|---|---|---|
| `crates/yu-rt/src/vm.rs` | +315 | **REVERT/MOVE** | `GosubFrame` 三组拆分必须回退共享；`scenario_token_vars` v555 分支、`apply_script_statics`、`disk_free_bytes`、`drop_files`、`last_text_lines` 逐段判 |
| `crates/yu-rt/src/sysvar.rs` | +142 | **REVERT/MOVE** | 旧版 `_STR`/`_INT` 语义回退；v555 访问器移入模块 |
| `crates/yu-rt/src/vm/handlers/mod.rs` | +155 | **REVERT/MOVE** | 非 555 的 `PRODUCT` 分派回退；v555 121 槽表移入模块 |
| `crates/yu-rt/src/vm/handlers/file_io.rs` | +251 | **REVERT/MOVE** | 旧版 SAVE/LOAD 路径回退；`CGFILE`/`MEM` 移入模块 |
| `crates/yu-rt/src/vm/handlers/text.rs` | +267 | **REVERT/MOVE** | 旧版 TEXT/TEXTINFO 路径回退；`TEXT[FILE]` 移入模块 |
| `crates/yu-rt/src/vm/handlers/variables.rs` | +184 | **REVERT/MOVE** | `VARINFO` 旧版惰性路径回退；555 变体移入模块 |
| `crates/yu-rt/src/vm/handlers/control.rs` | +52 | **REVERT/MOVE** | GOSUB/RETURN 三组参数在共享层的改动回退 |
| `crates/yu-rt/src/semantics.rs` | +81 | **REVERT** | 去掉 `>= 555`；只保留精确 555 的数据派生 |
| `crates/yu-rt/src/{code,event,task,value,diag,lib}.rs` | 小量 | **KEEP/审计** | 只允许新增诊断码、事件字段、端口字段等行为中性项 |
| `crates/yu-rt/src/scenario.rs` | 新文件 | **MOVE** | v555 场景 DSL，归模块 |
| `crates/yu-rt/src/vm/handlers/product.rs` | 新文件 | **MOVE** | PRODUCT 处理体，归模块 |
| `crates/yu-gfx/src/state.rs` | +62 | **REVERT/MOVE** | 非 555 的 `LINT2` 语义回退；v555 查询走扩展/包装 |
| `crates/yu-save/src/{state,host,error}.rs` | +167 | **REVERT/MOVE** | 旧版容器布局回退；`MEM` 槽归 v555 |
| `crates/yu-engine/src/engine.rs` | +17 | **审计** | 只保留“按精确 555 选 runtime”的装配；不得改变旧版构造路径 |
| `crates/yu-platform/host-desktop/src/lib.rs` | +12 | **REVERT/审计** | 生命周期改动需证明对旧版中性 |
| `crates/yu-platform/host-disk/` | 新 crate | **KEEP/审计** | 通用磁盘剩余空间端口；可保留，但不得强迫旧版依赖 |
| `crates/yu-trace/src/record.rs` | +3 | **KEEP** | 新增字段，默认值不影响旧 trace |
| `Cargo.toml` / `Cargo.lock` | +1/+9 | **KEEP** | 新 crate/依赖，构建层 |
| `crates/yu-rt/tests/fixture/*` | 新增 | **KEEP/MOVE** | 旧版 fixture 保留；v555 fixture 归模块；必须补 GOSUB/过渡回归 |
| `docs/*`、`PROGRESS.md`、探针 | 新增 | **DEFER** | 文档/工具最后收口，不参与行为判定 |

### 5.1 特别说明：DIV-RT-065

stash 内 `docs/divergences/DIV-RT-065-gosub-param-groups.md` 声称三组拆分是
“全版本正确性修复”，并称旧语料没有同编号 `PINT/PSTR` 混用。这个结论
**不能直接当作旧版无回归的证明**：

- 它没有覆盖 `RINT/RFLT/RSTR` 的旧版使用；
- 它没有覆盖旧版过渡宏（`es.SS.FADE` / `es.CGTA` / `CGACT`）的参数组合；
- 它没有解释“淡入延迟 + 黑闪”的时序现象。

因此本轮处置是：**共享层恢复旧布局，v555 用自己的三组帧**。如果新执行者
要用证据保留共享拆分，必须：
1. 复现旧版闪黑并证明与拆分无关；
2. 补旧版 GOSUB/RETURN 回归；
3. 请主人裁决。

---

## 6. 执行顺序（硬门禁）

### Step 0 · 冻结现场

- 只读核对：`git status`、`git stash list`、`git worktree list`、HEAD。
- 让主人提供独立 worktree / 分支；未获授权前不改码。
- 在独立工作树里先跑基线：

  ```bash
  cargo build --workspace --all-targets
  cargo test --workspace
  ```

- 记录原始输出；旧版手工复测（若主人已确认基线干净，直接引用其结论）。

### Step 1 · 只读恢复证据

```bash
git stash show --stat stash@{0}
git stash show --name-status stash@{0}
git ls-tree -r --name-only stash@{0}^3
git show stash@{0}^3:docs/formats/v555-truth-2026-11-21.md > <scratch>/v555-truth.md
git show stash@{0}^3:docs/divergences/DIV-RT-065-gosub-param-groups.md > <scratch>/DIV-065.md
git show stash@{0}^3:docs/divergences/DIV-RT-066-v555-runtime-surface.md > <scratch>/DIV-066.md
```

- 不 apply、不 pop、不 checkout 到工作区。
- 需要某个文件内容时，用 `git show stash@{0}:<path>` 输出到 `<scratch>`，
  再手工挑段。

### Step 2 · hunk 级审计（先交表，后改码）

产出 `docs/tasks/v555-isolation-audit.md`（untracked），每行至少：

```text
文件:符号/行范围 | 暂存改动 | 分类(REVERT/MOVE/KEEP/DEFER) |
旧版影响(会/不会/未知) | 证据 | 处置
```

- 对 `vm.rs`、`sysvar.rs`、`control.rs`、`file_io.rs`、`text.rs`、
  `variables.rs`、`yu-gfx/state.rs`、`yu-save/state.rs` 必须逐 hunk 写。
- 标为“未知”的项，先补测试/对拍，不许猜。
- **审计表交给主人确认后，才进入 Step 3。**

### Step 3 · 恢复旧版零回归

- 最稳的路径：从 `c1549eb` 干净树开始，只把 Step 2 判为 KEEP 的行为中性
  项重新落地；不要反向修补整份 stash。
- 优先顺序：
  1. 回退 `GosubFrame` / `control.rs` / `sysvar.rs` 的共享改动；
  2. 回退 `yu-gfx` / `yu-save` / `file_io` / `text` / `variables` 的旧版路径；
  3. 去掉 `semantics.rs` 的 `>= 555`；
  4. 保留扩展点/诊断/端口等中性项。
- 门禁：`cargo test --workspace` 全绿 + 旧版手工复测无闪黑。
  **未过门禁，不得开始 v555 模块。**

### Step 4 · 建 v555 独立模块

- 首选新 crate `crates/yu-rt-v555`；退路见 §3.1。
- 先落扩展点（默认 None），用旧版测试证明“未安装 = 行为不变”。
- 再搬 MOVE 项：`product.rs`、`scenario.rs`、v555 sysvar、GOSUB 三组帧、
  TEXT/FILEINFO/CGINFO/VARINFO 变体、MEM 存档、启动编排。
- 共享 `yu-rt` 只保留扩展点注册与通用能力，不出现 v555 业务分支。

### Step 5 · 测试与证据

- 旧版：既有 lib/fixture/golden 原样通过；新增一个旧版 GOSUB/过渡回归，
  锚定 `c1549eb` 行为。
- v555：把 stash 内对应 fixture 迁到模块；补 `corpus_v555`（按环境变量
  装载全部 555 语料）。
- 每条 555 行为保留证据锚点（构建哈希 + VA/函数 + 语料调用点）。

### Step 6 · v555 运行验收

```bash
# 示例；实际路径按 AGENTS.md 的占位符替换
yu-cli run "<v555-2>" --mode headless --frames 1500 --patch gbk \
  --log-file "<tmp>/head.jsonl" --diag-summary on
yu-cli run "<v555-2>" --mode desktop --frames 180 --patch gbk \
  --shot "<tmp>/screen.png"
```

验收：

- 1500 帧到达 `ES.TITLE.*`，0 Fatal；
- `gfx.info.missing/unsupported` 在稳定帧为 0；
- 启动链可见且顺序正确：`CSEL` → `logo_a` → fade → Shiravune →
  `bg_effect_00` → `chui01` → `chui02` → fade → 标题；
- 点击跳过/不可跳过由剧本 `\WA` 参数决定，不由宿主决定。

### Step 7 · 收口

- `PROGRESS.md` 追加成果（命令 + 原始结果）；
- 更新 `docs/formats/v555-truth-2026-11-21.md` / divergence / coverage；
- 不提交 git；交主人审阅。

---

## 7. 验收标准

### 7.1 旧版（硬门禁）

- [ ] `cargo build --workspace --all-targets` 通过；
- [ ] `cargo test --workspace` 全绿；
- [ ] `cargo test -p yu-rt --test fixture` 全绿；
- [ ] `cargo test -p yu-gfx --test info_query` 全绿；
- [ ] `cargo test -p yu-save` 全绿；
- [ ] 主人手工复测：旧版进游戏无淡入延迟/黑闪；
- [ ] `git diff c1549eb -- crates/yu-rt/src/vm.rs` 中 `GosubFrame` 仍是
      `args`/`ret`，无三组拆分；
- [ ] `rg -n "v555|555" crates/yu-rt/src crates/yu-gfx/src crates/yu-save/src`
      无共享业务分支。

### 7.2 v555（功能门禁）

- [ ] 独立模块可编译，`yu-engine` 只在 `yscm.version == 555` 时安装；
- [ ] 可达命令 0 `rt.cmd.unimplemented`（剩余项必须是台账显式挂账）；
- [ ] 启动链/标题截图可见，0 Fatal；
- [ ] `_STR`/`@3165`/`CGINFO[LINT2]` 诊断噪声归零；
- [ ] 所有 v555 结论有证据锚点。

### 7.3 结构门禁

- [ ] 共享层没有 v555 业务分支；
- [ ] v555 模块有自己的 dispatch、GOSUB 帧、sysvar、scenario、startup；
- [ ] 旧版结构/默认值/诊断与 `c1549eb` 一致；
- [ ] 独立 crate（或经批准的过渡模块）边界写进文档。

---

## 8. 风险与禁止事项

1. **不要把 GOSUB 当唯一根因。** 它是首要嫌疑，但必须 bisect：
   按组（GOSUB/sysvar、TEXT/text、FILEINFO/CGINFO/gfx、SAVE/MEM、
   PRODUCT/scenario、engine/host）逐组应用到旧版，复测闪黑，记录哪组触发。
2. **不要用宿主侧的 sleep/淡入补丁“修好画面”。** 这是行为对齐任务。
3. **不要用 495 手册补 555 的洞。** 手册缺页但 555 证据足够时标
   `fixed(no-manual)`；官方空桩标 `aligned-stub`；官方不支持标
   `unsupported-by-official`。
4. **不要把 `v555-support` 分支整体合并。** 它基于旧架构，且共享层同样
   改过 GOSUB；只抄语义/证据/测试。
5. **不要 drop/clear stash。** 它是主人的回滚现场。
6. **不要修改 `msic/`、`yu-emote`、游戏数据。**
7. **不要未经授权做 git 写操作。**
8. **无证据的语义保持 `Unimplemented`/过渡态。** `Unimplemented` 不得
   作为终态，但也不能用猜测替代证据。

---

## 9. 参考资料（只读）

- stash 内旧任务书：`git show stash@{0}^3:docs/tasks/v555-task-book.md`
- stash 内旧启动提示词：`git show stash@{0}^3:docs/tasks/v555-startup-prompt.md`
- 真值台账：`git show stash@{0}^3:docs/formats/v555-truth-2026-11-21.md`
- GOSUB 分歧：`git show stash@{0}^3:docs/divergences/DIV-RT-065-gosub-param-groups.md`
- v555 运行面：`git show stash@{0}^3:docs/divergences/DIV-RT-066-v555-runtime-surface.md`
- 早期 v555 分支：`v555-support` 工作树内的 `docs/formats/v555-commercial.md`、
  `v555-gap-2026-11-19.md`、`v555-sysvar-abi-measured.tsv`
- 启动剧本顺序：真值台账 §11.1（`scenario/scenario_start.txt`）
- 旁线笔记：`<side-repo>/docs/format-vm-opcodes.md`、`vm-commands.md`、
  `script-keywords.md`、`render-semantics.md`
- 旧线参考：`<repo-old>` 的 v555 记录（只引事实，不复制代码）

---

## 10. 交接检查表（新执行者第一轮必须回答）

- [ ] 我确认当前 HEAD / stash / worktree 状态与 §1 一致；
- [ ] 我没有 pop/drop stash，没有改 `msic/`、`yu-emote`；
- [ ] 我已完成 hunk 级审计表，并标出所有“未知”；
- [ ] 我说明了旧版基线测试结果（或主人已确认基线干净）；
- [ ] 我提出了独立模块的落点（crate 或经批准的过渡模块）；
- [ ] 我列出了本轮不做的项（E-mote、新功能、文档收口）；
- [ ] 我等主人确认后再改码。
