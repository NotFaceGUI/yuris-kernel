# 任务书 Q1 · 日志收口:统一格式 + 级别颜色 + 默认可见

> 仓库:`<repo>`。先读 `AGENTS.md`、`docs/architecture.md` §8、
> `crates/yu-log/src/`(契约与现状)、`docs/todo-silent-handling.md`(S1–S9,
> 与本任务互为表里)、`docs/todo-inventory.md` §0(姊妹台账)、
> `crates/yu-cli/src/desktop.rs` 的 `init_logging`、
> `crates/yu-platform/host-desktop/src/diagnostics.rs`。
>
> 独占新建:`docs/logging-contract.md`(如格式定稿需要单页契约)。
> **加法式修改**:`crates/yu-log`(ConsoleSink/颜色/统一格式化)、
> `crates/yu-engine/src/output.rs`(降级为薄壳)、`crates/yu-cli`
> (desktop/run/server 的输出点改走统一通道)、
> `crates/yu-platform/host-desktop`(诊断面板日志页签)。
> 其余 crate 只读。
>
> **tyn 参照**:旧线 `<repo-old>/crates/yuris-log` 的行格式/颜色表/文件汇
> 行为是**事实参照**(注明出处),**禁止整段搬代码**;按本仓库架构自写。

## 0. 任务定位(主人诉求 2026-10-25)

主人原话归纳成三条:

1. **默认行为不输出日志,没法排查** —— 内核诊断(`yu-log::Diag`)在默认
   运行方式下根本不到控制台(见 §1 现状盘点);
2. **日志没有颜色** —— 想要 tyn 那样按级别着色;
3. **记录 + 清除 + 统一输出格式** —— 现在大量散装输出没有统一前缀/级别,
   文件记录链路不完整。

本任务是**工程基础设施**,不涉及官方行为对齐;证据纪律(A4/A5)不约束
格式设计,但**一切结论仍要写锚点**(现状 grep 结果、tyn 出处行号)。

### 与 `todo-silent-handling.md` 的关系(不是替代)

- **S1(FrameDiag 聚合丢 pc)是本任务的前置**:控制台/文件行要能显示
  「哪条脚本哪个 pc」;没有 S1,统一格式也只是把不可定位的计数印得更漂亮。
  **建议 S1 并入 Q1 第一个阶段做掉**。
- S2(as_int 类型不符挂号)、S5(域空分支挂号)产出**新诊断**;
  Q1 落地后这些诊断自动获得可见性,无需重复接线。
- S3/S4/S6 与本任务无依赖,独立排期。

## 1. 现状盘点(2026-10-25 实测锚点)

| # | 事实 | 锚点 |
|---|---|---|
| 1 | VM 把 Diag 攒进内部 `MemorySink`,宿主经 `take_diags()` 取走 | `crates/yu-rt/src/vm.rs:306,828` |
| 2 | 桌面模式每帧被 `DiagnosticsHost::observe` 取走 → 只进 egui 面板/横幅,**控制台零输出** | `crates/yu-platform/host-desktop/src/diagnostics.rs:351`、`src/ui.rs:228` |
| 3 | headless `run` 从不取 diags → **静默丢弃** | `crates/yu-cli/src/run.rs`(全文无 `take_diags`) |
| 4 | `serve` 无 `/diag` 订阅者时 `drain_diags` 丢弃 | `crates/yu-engine/src/engine.rs:905..913` |
| 5 | JSONL 文件(desktop 默认 `%TEMP%/yu-cli-desktop.jsonl`)只含 `log` 门面行(`sub:"log"`),**内核 Diag 不落盘** | `crates/yu-cli/src/desktop.rs:185..212`、`crates/yu-log/src/facade.rs:108` |
| 6 | 门面桥(wgpu/winit)控制台格式 `[log:warn] target: msg`,默认 warn,无颜色 | `crates/yu-log/src/facade.rs:135..144` |
| 7 | 产品输出 `output::line/error/raw` 无级别、无时间戳、无颜色;调用点 ad-hoc 前缀各写各的:`[sound]`、`yu-cli --mode desktop:`、`warning:`、`restart:`、`step:`、`paused`、`shutdown` … | `crates/yu-engine/src/output.rs`、`crates/yu-cli/src/desktop.rs:89..210`、`run.rs:122..212`、`server.rs:211..745` |
| 8 | 全链路无一处 ANSI 颜色 | `grep -rn 'x1b\|033\[' crates/*/src` 零命中 |
| 9 | `src/` 零直接 stdout 宏(AU1 C2 已清)——这条纪律要保住 | `docs/todo-inventory.md` §6.5 |

**tyn 事实参照**(出处 `<repo-old>/crates/yuris-log/src/`,行号 2026-10-25 核对):

- 行格式 `[{secs:>9.3}s] {LEVEL:<5} {target}: {message}`(`record.rs::format_line`);
- 级别色:Trace=`\x1b[90m` 灰 / Debug=`\x1b[36m` 青 / Info=`\x1b[32m` 绿 /
  Warn=`\x1b[33m` 黄 / Error=`\x1b[1;31m` 亮红 / Fatal=`\x1b[1;37;41m` 白底红
  (`level.rs:71..76`);
- stderr 汇,多行消息**逐行包色、空行不包、每行 `\x1b[0m` 收尾**(`sink.rs:38..80`);
- 文件汇追加写,路径 `YURIS_LOG_FILE`(`sink.rs:84`);
- **已知坑**:tyn 曾收到实机反馈「控制台日志全是白的」—— 由宿主/IDE 拉起、
  或 stderr 被管道转发时 TTY 检测变量缺失,颜色被**静默关掉**(`sink.rs:154..158`)。
  本任务必须提供显式开关,不允许静默无色后无提示。

## 2. 目标行为(待主人拍板的裁决点标 ⚖)

### 2.1 唯一控制台格式

所有**日志性质**的控制台行(内核诊断、门面桥、宿主生命周期)经同一个
格式化器输出,行格式:

```text
[  +12.345s] WARN  [gfx] imageop.region.partial @ s66 pc6028 x2: region partly outside source (:/W0/SPL401)
[   +0.102s] INFO  [host] data.ypf mounted, files=1424
```

- 前缀三段:相对启动秒(`+s`)、级别词、`[频道]`(频道 = vm/gfx/sound/
  save/host/log-facade;频道表进码表契约,新增要挂号);
- ⚖ 级别集合沿用 `yu-log::Severity`(Fatal/Recoverable/Unimplemented/Info)
  还是引入 tyn 式 Trace..Fatal 六级 —— 倾向**沿用 Severity + 给 Info 细分
  `Info/Lifecycle`**,避免同一包里两套级别语义;主人定。
- 机器可读输出(case runner 的 JSON、report JSON、benchmark 表)**永不经
  此通道**,不加色不加前缀(grep 守卫随 R5 落地)。

### 2.2 颜色(主人诉求 2)

- 色表直接采用 tyn 同表(§1);多行消息逐行包色、每行复位(tyn 行为);
- 新开关 `--color on|off|auto`,默认 auto:auto 检测失败(stderr 非终端)
  时**必须**在文件日志里留一行「color auto→off 原因」,并在 `--help` 里
  写明 `--color on` 可强制(tyn 的静默无色教训);
- 遵守业界约定:`NO_COLOR` 非空 → 强制 off;`CLICOLOR_FORCE=1` → 强制 on;
- Windows:目标是 Windows Terminal/ConPTY 与 Git Bash(mintty);不引入
  winapi 依赖,enable-virtual-terminal 由运行环境保证(两处实测均支持)。

### 2.3 默认可见性(主人诉求 1 的核心)

| 级别 | 控制台默认 | 说明 |
|---|---|---|
| Fatal | 恒显示(红) | 现状:Fatal 走 `Err` 路径已有;保持 |
| Unimplemented / Recoverable(挂号) | **显示**(黄),同 key 聚合去重、带位置与计数 | 「没法排查」的主诉;依赖 S1 |
| Info(生命周期:挂载/启动摘要/回退/关闭) | **显示**(绿) | tyn 同口径;`--log-level quiet` 可压掉 |
| 门面桥 Debug/Trace | 默认关 | 维持 `YU_LOG`/`--log-level` 语义 |
| wgpu/naga 门面 Warn | 显示(带 `log-facade` 频道) | 维持 |

- headless `run` 结束时打印**诊断汇总**:`{级别} {code} x{count} @ {位置列表}`
  按频次排序;`--diag-summary off` 可关(用例流水线不想看)。
- 桌面模式:控制台与 egui 面板**同源**(同一 drain,两路消费),面板行为不变。

### 2.4 记录 + 清除(主人诉求 3)

- JSONL 文件**同时**落内核 Diag(`sub:"diag"`,即 `Diag::to_json_line` 现有
  schema)与门面行(`sub:"log"`),同文件同写者:
  ⚖ 实现取「一个 Sink 拥有文件,内部 Tee」还是「FacadeLogger 持 JsonlSink」,
  实现期定,约束是**禁止两个写者各自持有同一文件句柄**;
- 「一次运行 = 一个现场」的**每轮截断**保持(desktop `init_logging` 现状,
  `desktop.rs:194..201`),并补 headless/serve 同口径;
- egui 诊断面板加「日志」页签:滚动缓冲 + 清空按钮(面板内清除不影响
  文件记录);默认帧上限沿用现有 CAP 思路,截断区间显式标注
  (与 silent-handling S6 同方向,本任务只做页签,S6 的 seq 区间记账归 S6)。

## 3. 阶段划分

| 阶段 | 内容 | 验收 |
|---|---|---|
| R0(=S1 并入) | `FrameDiag` key 加 `where_`,`FrameDiag.at: Vec<Loc>`(cap N);渲染报告/`game_diag_probe` 输出位置列表 | `<game-a>` 片头 f86 复现:报告出现 `region.partial @ s66 pc6028` 而非仅层名(todo-silent-handling §S1 原验收) |
| R1 | `yu-log` 新增统一格式化 + ConsoleSink(色表/NO_COLOR/`--color`);`yu-engine::output` 保留 raw 产品通道,日志行全部改走新通道 | 单测:格式化确定性(同输入同字节);色开关三态;NO_COLOR 生效 |
| R2 | 默认可见性接线:桌面 observe 后同步控制台行;headless 末尾汇总;`--log-level quiet/info` 门槛 | headless 跑 `<game-a>` 120 帧:控制台出现挂号行(带 pc);`--diag-summary off` 时干净 |
| R3 | 前缀清理:desktop/run/server 的 ad-hoc 前缀改频道化(`[host]`/`[sound]`/`[serve]`…);文案保留 | `grep -rn 'output::error(format!' crates/*/src` 每处首段都是 `[频道]` 或走 Diag |
| R4 | 文件统一:Diag 落 JSONL(`sub:"diag"`);headless/serve 截断口径对齐;egui 日志页签 + 清空 | 用 `Diag::from_json_line` 回读混合文件,`sub:"diag"`/`sub:"log"` 都可解析 |
| R5 | 守卫:`src/` 零 stdout 宏(已有)+ **stdout 无 ANSI**(新 grep)+ `silent_fallback_audit`(S3 的扫描测试,如主人同意一并挂) | 三条 grep 全绿且进提交前手跑清单(与 `case --all` 同级) |

## 4. 风险与边界

1. **性能**:逐帧格式化有成本。铁律 = 级别过滤在格式化**之前**(tyn 的
   阈值语义,`level.rs:6..8`);聚合 key 命中已有条目时只 `count += 1`,
   不重新格式化。
2. **洪水**:wgpu/naga 在某些驱动上 Debug/Trace 洪水 —— 门面桥默认级别
   维持 warn 不变,不因本任务放大。
3. **wasm 宿主**:门面桥 wasm 不装(facade.rs 现状);ConsoleSink 颜色在
   web 控制台走 `%c` 样式是**后续任务**,本任务不做,接口留 `color: bool`。
4. **不碰的东西**:`Diag` schema/JSONL 往返契约、`LogSink` trait 语义、
   `yu-trace` 的取证通道(那是与官方对齐的 oracle,不是运维日志,两码事
   ——合并会污染对拍)。
5. **A3**:格式化器无全局状态;颜色开关经构造注入,`init` 只在 main 入口
   一次;测试并行安全(MemorySink 断言,不依赖终端)。

## 5. 开放裁决清单(开工前找主人确认)

- ⚖ §2.1 级别集合:沿用 Severity vs 引入 Trace..Fatal;
- ⚖ §2.3 Info(生命周期)默认显示是否可接受(会改变现有用例/脚本对
  stdout/stderr 行数的期望;`--log-level quiet` 是否足够兜底);
- ⚖ R0(S1)并入本任务,还是按 silent-handling §3 的顺序独立先行;
- ⚖ 诊断汇总打 stderr(建议)还是 stdout(若下游把 stdout 当唯一现场)。
