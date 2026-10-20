# 任务书 D · 对拍基建 `yu-trace`(旁挂,可与 A/B/C 并行)

> 会话定位: 你**独占 `crates/yu-trace`**;不修改其它任何 crate、根 `Cargo.toml`、共享文档。
> 依赖:`yu-log`(A,已完成)、`yu-vfs`(A,已完成)、`yu-rt`(C,进行中——**只使用其公开 API**)。
> 一句话目标: 把"同一输入时间线 ⇒ 逐字节一致的事件流"变成一条 `cargo test` 能自动执行的回归。
>
> **并行结论(开工前先看 §4)**: 现在就能开工,不必等 B/C 完工。边界①已完全解锁;
> 边界②的代码形态已存在但 API 未冻结;边界③④要等 M3。D 永远不阻塞别人,也不许别人依赖 D。

## 0. 开工前必读(顺序)

1. `<repo>/AGENTS.md`(占位符规则、语料、纪律)
2. `docs/architecture.md` §2(crate 表:`yu-trace` 是**旁挂**,依赖全部但不得被依赖)、
   §3(一帧数据流:①②③④ 四条边界)、§8(错误系统)
3. `docs/oracle.md`(**schema 草案与 diff 工作流的权威来源**)
4. `docs/sdk-eris-analysis.md` §5(任务让出 = 帧边界;"帧"的定义)
5. 现有代码(只读):
   - `crates/yu-vfs/src/lib.rs` → `DataSource::{open,exists,list}`、`DataSource::source_name`、`LookupKey`
   - `crates/yu-rt/src/event.rs` → `VmEvent` 全部变体(**边界②当前形态**)
   - `crates/yu-rt/src/vm.rs` → `Vm::{events,take_events,to_jsonl,table_provenance}`、`BootConfig`
   - `crates/yu-log/src/` → `Diag`/`Severity`/`Boundary`/`code`
6. 方法论参考(只读,重写不搬运): `<repo-old>/scripts/engine_trace.py`、
   `docs/windows-handoff.md`(WOW64 采集思路)。**本任务不实现官方采集器**,见 §1 最后一条。

## 1. 范围(只做 ①②;③④ 只留接口)

| 边界 | 内容 | 本次 |
|---|---|---|
| ① vfs 读 | 包装任意 `DataSource`,记录 `open/exists/list` 的调用事实与结果身份 | **做** |
| ② 事件流 | 消费 `yu_rt::VmEvent`,帧锚定,稳定 JSONL 落盘/读回/diff | **做** |
| ③ 帧描述 | `FrameDescription`(M3 的 `yu-gfx` 产物) | 只留枚举变体与文档,不实现 |
| ④ 平台命令 | 后端呈现/音频/输入等平台调用 | 只留枚举变体与文档,不实现 |

明确**不做**:

- 不做官方引擎 trace 采集器(WOW64/Python)。它属于独立工具任务;D 只负责让内核侧输出
  与官方采集**同一 schema**,将来采集器直接对接。
- 不做 gfx/snd/inp/save/cli,不实现任何游戏语义。
- 不在任何其它 crate 里加埋点。边界②的接入方式是**拉取 `Vm::take_events()`**,
  不要求 C 在 `yu-rt` 内写一行 trace 代码。

## 2. 接口契约(冻结;改动须在报告里留痕)

### 2.1 crate 依赖

```toml
[dependencies]
yu-log = { path = "../yu-log" }
yu-vfs = { path = "../yu-vfs" }
yu-rt  = { path = "../yu-rt" }
```

- `yu-trace` **不得**出现在任何其它 crate 的依赖里(旁挂的硬约束)。
- 哈希: 允许引入 `sha2`(仅本 crate,用于记录负载身份);若坚持零新依赖,
  可用 `len + FNV-1a64`,但须在报告里说明取舍。**不可**只记长度不记内容身份。
- 时间: 不允许 `SystemTime`/`Instant` 进任何输出;所有时间来自注入的帧号/毫秒。

### 2.2 记录封套(与 `docs/oracle.md` 同构,字段顺序固定)

```json
{"schema":1,"seq":0,"frame":42,"sub":"vfs","kind":"open","payload":{...}}
```

- `schema`: u32,从 1 起;任何字段增删/语义变化必须递增并在报告记录。
- `seq`: 单调递增 u32/u64,录制顺序的**唯一序**;不接受任何"按时间排序"的推断。
- `frame`: 事件的逻辑帧号;边界①发生在帧外时为 `null`。
- `sub`: `"vfs" | "vm"`(③④ 启用后加 `"frame" | "platform"`)。
- `payload`: 按 `kind` 定形;所有键排序固定,**禁止 HashMap 迭代序**。

建议的 payload 形状(可实现时微调,但须写进 `crates/yu-trace/SCHEMA.md`):

```text
vfs/open     {"source":"YpfSource","key":"ysbin/ysc.ybn","bytes":11156,"sha256":"…","diag":null}
vfs/missing  {"source":"…","key":"…","diag":"vfs.missing"}
vm/<event>   直接映射 VmEvent 的可观测字段(frame/task/script/pc/name/args/code/detail…)
header       {"schema":1,"codec":"…evidence_line…","tables":["ysc:vfs","ysl:vfs",…],
              "boot":{"entry":"SYSTEM_START","fps":60,"start_ms":0,"frame_ms":16},
              "input":{"events":N,"sha256":"…"},"kernel":"<crate 版本>"}
```

- **header 也是一条记录**(`seq=0`,`sub="meta"`,`kind="header"`),不是带外注释;
  diff 时先比 header,再比流。`header` 里允许出现路径/编码决策等元数据,
  但它们必须来自**确定性来源**(不得含绝对路径、主机名、墙钟)。
- trace 文件写入 `target/tmp/trace/` 或 `YURIS_TRACE_DIR`;
  **trace 语料不入库**。入库的只有 `crates/yu-trace/tests/golden/` 下 <32KB 的合成 fixture。

### 2.3 `VmEvent` 映射 = 穷尽匹配

- 用 `match` 穷尽列出**当前全部** `VmEvent` 变体,逐条映射到记录。
- 这是设计意图: C 将来新增变体 ⇒ D **编译失败** ⇒ 强制做一次 schema 评审,
  而不是新事件悄悄漏出 trace。禁止 `_ => {}` 兜底。
- D **不得**直接复用 `yu_rt::json` 的内部序列化当协议;协议由 D 拥有。
  可以让测试断言"同一事件在两边都可读",但 golden 以 D 的输出为准。
- 参数值(`ArgValue`)按可观测形式记录(i64/f64/字符串字节);浮点用 `{:?}` 或
  最短往返表示,禁止格式化丢精度;CP932 文本按字节记(与 `VmEvent::Text` 一致)。

### 2.4 diff 语义

- 逐行比较(按 `seq`),输出**首个分歧**:
  `{line, path, expected, actual}`(path 用 JSON Pointer)。
- **禁止容差/归一化/模糊匹配**。唯一允许的例外: `header` 中显式声明为
  `"volatile":true` 的字段(本任务不允许新增此类字段,除非有真实需求并留档)。
- 行数不等、末尾截断、非法 JSON 都是分歧(分别给独立错误码),不做"前缀相同即通过"。
- `replay` 的定义: 用录制时的 header(编码决策、启动配置、输入时间线摘要)
  重跑同一数据,生成新流,与录制流 diff 为零。
  若 `yu-rt` 的运行入口尚未支持"注入时间线后确定性跑完"一段,先做离线部分
  (把 `Vec<VmEvent>` 喂给 recorder),把在线回环标 `blocked` 并在报告写明等待的 API,
  **不要**为此修改 `yu-rt`。

### 2.5 错误

- 错误码命名空间 `trace.*`(如 `trace.io`、`trace.parse`、`trace.schema`、
  `trace.truncated`、`trace.diff`),全部走 `yu-log::Diag`。
- 本 crate 内**禁止** `println!/eprintln!/dbg!`;专断审计见 §5。
- 解析 trace 时: 未知 `schema` 版本 → `trace.schema`(Fatal,拒绝猜测);
  未知 `kind` → `trace.schema`(Fatal);缺字段 → `trace.parse`。

## 3. 实现顺序(每步可独立验收)

| 步骤 | 内容 | 验收 |
|---|---|---|
| D1 | crate 骨架 + 记录类型 + 稳定 JSONL writer/reader | 写→读→再写**逐字节一致** |
| D2 | `TracingSource<D: DataSource>` 包装器 + 等价性测试 | 包装前后 `open/exists/list` 结果逐字节一致;每次调用都有记录 |
| D3 | `EventRecorder`: `take_events()` → 记录;穷尽映射 `VmEvent` | 事件流 JSONL 可读回;全部变体有覆盖测试 |
| D4 | `diff` + first-divergence + 回放闭环 | 相同输入 0 差异;人为改一字节在正确行报错 |
| D5 | 真实包冒烟(env gate)+ clippy + 报告 | 见 §5 |

建议目录:

```text
crates/yu-trace/
├─ Cargo.toml
├─ SCHEMA.md              ← 记录格式的唯一定义(2.2 落地版)
├─ src/{lib,record,json,source,event,diff,error}.rs
└─ tests/{roundtrip,equivalence,vfs_trace,event_trace,diff,real_sample}.rs
```

## 4. 并行规则(用户最关心的问题)

**可以现在开工,与 A/B/C 并行。** 依赖状态与约束:

| 对话 | 状态 | D 怎么用 |
|---|---|---|
| A(`yu-log`/`yu-vfs`) | **已完成** | 公开 API 直接用;发现缺 API 只记录,不改 A |
| B(`yu-format`) | 进行中 | **无依赖**,不等待、不引用 |
| C(`yu-rt`) | 进行中 | 只用 `pub` 的 `VmEvent`/`Vm::take_events`/`table_provenance`/`BootConfig`;API 变化时 **D 适配**,不改 C |
| ③④(M3) | 未开始 | 只留占位;等 `yu-gfx`/后端落地再实现 |

操作约束:

- 只改 `crates/yu-trace/**`;不碰根 `Cargo.toml`(`members=["crates/*"]` 自动纳入)。
- 不在 `yu-rt`/`yu-vfs`/`yu-log`/`yu-format` 里加任何 trace 代码或依赖。
- C 正在编译时 `cargo` 会等文件锁,属正常现象;不要为此 kill 进程。
- `PROGRESS.md` 只在完成时**追加**"成果 D1",不重写、不整理别人条目。
- 禁止任何 git 写操作(含本仓库),与 AGENTS §4 一致。

## 5. 验收(全部可复跑,报告须贴原始输出)

1. `cargo test -p yu-trace` 全绿;缺样本用例显式 skip 并计数,禁止空跑。
2. roundtrip: `record → jsonl → parse → jsonl` 两次字节相等。
3. 等价性: 同一组 key 在 `TracingSource<S>` 与裸 `S` 上,`open/exists/list`
   返回值/错误码逐字节一致;trace 中调用条数 = 实际调用条数。
4. 真实包冒烟(env gate `YURIS_SAMPLE_ERIS_YPF=<official data.ypf>`):
   `open("ysbin\ysc.ybn")` 记录 `bytes/sha256`,并与直接 `yu-vfs` 读取结果一致;
   没有 env 时打印"sample not found, skip"并计数(不得静默通过)。
5. 事件流: 用 `yu-rt` 现有 fixture 跑一帧 → recorder → 落盘 → 读回 → diff 为 0;
   手工改动某条记录一个字段 → diff 在**该行**报 first divergence。
   若 C 的在线入口不可用,离线路径必须通过,在线回环标 `blocked` 并写明缺的 API。
6. 审计:
   - `grep -rn "println!\|eprintln!\|dbg!" crates/yu-trace` → 0 命中;
   - `git diff --stat` 只出现 `crates/yu-trace/**`(以及 `PROGRESS.md` 追加);
   - `grep -rn "SystemTime\|Instant" crates/yu-trace` → 0 命中(测试时间戳除外)。
7. `cargo clippy -p yu-trace --all-targets` 零告警。

## 6. 交付报告

- 改动文件清单(逐文件一句话)。
- 验证命令 + 原始输出(贴完整段落,不要只写"通过")。
- `SCHEMA.md` 的最终字段表;与 `docs/oracle.md` 草案的差异(若有,说明原因)。
- 未做项/阻塞项(③④、官方采集器、在线回放等)。
- 在 `PROGRESS.md` 末尾追加"成果 D1(日期)· yu-trace";不执行 git。

## 7. 禁止清单

- 禁止把 trace 语料或任何游戏数据入库。
- 禁止让 `yu-trace` 成为运行时/其它 crate 的依赖;禁止打印到 stdout。
- 禁止墙钟、线程调度、HashMap 迭代序、绝对路径进入录制内容。
- 禁止"前缀相同/容差/归一化"式的假通过;diff 必须严格。
- 禁止修改 A/B/C 的工作区;禁止任何 git 写操作。
- 被跟踪文件只写占位符(真名见 `AGENTS.md`)。
