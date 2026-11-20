# v555 支持期优化计划（行为保持优先）

- 状态：计划（未执行）
- 日期：2026-11-20
- 基线：`a976d41`（`chore(release): 版本号升至 0.4.1`）
- 关联：`docs/formats/v555-commercial.md`、`docs/formats/v555-gap-2026-11-19.md`

## 0. 一句话

v555 已经推进到「标题画面前」边界，架构骨架是稳的；现在要做的是把
「能力判断、命令实现、测试、探针」四层分开，并补上 v555 原生的
「三份文本变体 + 配置语言号」多语言层，让后续 P0/P1 和 M2–M4
有稳定落点，而不是继续往一个 8k 行文件里加分支。

## 1. 为什么现在做

v555 已落地：启动链、剧本 DSL 词法/宏、GOSUB 三组参数、
`VARINFO[SJISCODE/STRFIRST]`、`TEXT[FILE]`、`PRODUCT` 最小面、
`SAVE/LOAD[MEM]`。剩余游戏缺口在 gap 文档的 P0-1..P0-4。

v555 的多语言是**数据驱动**的：同一引擎读 `scenario/`、`sc_cn_scenario/`、
`sc_tw_scenario/` 三份文本，由配置里的语言号选一份；当前内核对多语言
只用全局 `--patch gbk`，会把基准日文/繁中一起按 GBK 解，不能作为长期方案。

同时结构开始吃紧（实测）：

| 位置 | 规模 | 问题 |
|---|---:|---|
| `crates/yu-rt/src/vm/handlers.rs` | 7,945 行 | 命令表/分派/103 个 `impl Vm` 方法/6 个内联测试模块混在一个文件 |
| `crates/yu-rt/tests/fixture.rs` | 7,650 行 | 136 个测试，无子模块，v555/CSV/IDOM 等主题混排 |
| 版本/能力判断 | 1 处生产分支 | 目前只有 `handlers.rs:1818` 的 SJISCODE；`_STR`、TEXT/SAVE、PRODUCT 会继续增加 |
| 剧本词法器 | `scenario.rs` 192 行 | 与官方 `PRODUCT[COM=22]`/s59 的契约还有未钉死差异 |
| v555 多语言 | 3 份 scenario + `config.sd` 语言号 | 当前用 `--patch gbk` 全局覆盖；base/cn/tw 编码不同 |
| 探针 | 20+ 个 `examples/` | 稳定/一次性/调试 API 未分类 |

目标：**除 O4 词法器差分与 O5 多语言层确认后的修正外，所有批次行为不变**；
不重写架构，不做大爆炸重构。

## 2. 边界（非目标）

- 本计划不直接修 P0；P0 仍按 gap 文档推进。
- 不引入新框架/宏/插件；不做 speculative 字段。
- 不动 `yu-gfx`/`yu-engine` 的既有分层；不重命名公开 API。
- 商业语料、反编译产物、NEKO 包内容不入库；词法器 golden 只用合成行。
- 不执行 git 写操作；每批独立可编译，提交仍等主人明确指令。
- `Error::Unimplemented` 只能作为过渡态；实现处必须有证据锚点。
- E-mote 家族继续 `deferred(master)`。

## 3. O0：先钉基线（小）

目的：后面任何重构都能判断「行为有没有变」。

1. 记录当前测试基线：
   - `cargo test -p yu-rt -p yu-save`
   - `cargo test -p yu-rt --test fixture --test golden`
2. 记录 0.495 ERIS 基线：
   - 136 个脚本、命令覆盖 census、VARINFO 使用面；
   - `logic_slice/input_counter/error_paths` 三个事件 golden 不变。
3. 记录 v555 基线：
   - 启动链到 `es_scenario_macro_変数n.yst` pc24 的 Fatal；
   - 诊断码计数（`rt.sysvar.unimplemented _STR` 等）；
   - 桌面/headless 画面状态。
4. 记录 v555 多语言基线（O5）：
   - 语言号 0/1/2 下 base/简中/繁中同标签的 `\LC` 行；
   - `config.sd` DNO1/2 的语言号与字节指纹；
   - 缺 `sc_cn_`/`sc_tw_` 时的回退结果。
5. 新增 `docs/architecture/optimization-baseline.md`，只写命令、数字、诊断码，
   不写商业文本。
6. 不运行 `UPDATE_GOLDEN`。

**出口**：同一命令可复现同一基线；之后任何差异都当回归处理。

## 4. O1：SemanticProfile（小）

### 4.1 为什么不是再叫一个 ProjectProfile

现有「版本工具」各管一段，不是缺，而是没有「已装载表 → 语义形态」的
单一派生快照：

| 现有类型 | 位置 | 回答的问题 | 不负责 |
|---|---|---|---|
| `AbiProfile` | `crates/yu-rt/src/sysvar_abi.rs` | 系统变量 ABI 前缀/槽位来源 | 工程命令语义 |
| `PatchProfile` | `crates/yu-engine/src/patch.rs` | 补丁文件集/覆盖顺序 | 脚本语义 |
| `YpfProfile` | `crates/yu-vfs/src/ypf.rs` | 封包格式/key/入口布局 | 表语义 |
| `Identity` | `crates/yu-engine/src/identity.rs` | 引擎版本、标题、窗口信息 | 语义门控 |
| **`SemanticProfile`（新）** | `crates/yu-rt/src/semantics.rs` | 本工程已装载表的语义形态 | 宿主/路径/补丁事实 |

`Identity.engine_version` 是展示字段，不应被 handler 反查；
`SemanticProfile` 是行为门控的唯一来源。

### 4.2 设计

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SemanticProfile {
    pub ysc_version: u32,
    pub varinfo_sjis: SjisCodeMode, // Inert | FirstChar
}
```

- 只放**当前有门控需求**的字段；TEXT/SAVE/PRODUCT 的字段等各自门控落地时
  再加，避免空字段。
- `SemanticProfile::from_tables(&TableSet)` 在 `Vm::boot_inner`
  （`vm.rs:797` 的 `TableSet::load_from` 之后）计算一次，存入 `Vm` 新字段；
  提供只读 `Vm::semantics()`。
- `handlers.rs:1818` 的 `self.tables.boot.yscm.version < 555` 改为
  `self.semantics.varinfo_sjis == SjisCodeMode::Inert`；
  删除该处直接版本比较。
- `yu-engine` 的启动诊断/`Identity` 只做只读展示（可选），不把
  文件系统/补丁事实塞进 profile。
- 测试：`ProjectBuilder`（`tests/support/mod.rs:180`）495/555 两档断言
  profile；保留现有 SJISCODE 正/负用例；加一条审查规则/脚本检查
  「handler 内不得新增直接 `yscm.version` 比较」。
- 命名刻意避开 `ProjectProfile`：后者与 `PatchProfile`/`AbiProfile`
  太容易混。

**出口**：SJISCODE 只有一个门控来源；0.495 仍走 aligned-stub，
555 仍计算首字符码。

## 5. O2：handlers 拆分（中）

`src/vm/handlers.rs` → `src/vm/handlers/mod.rs` + 子模块。
**只搬运，不改函数体**：

| 模块 | 内容 | 源行段（约） |
|---|---|---|
| `handlers/mod.rs` | `required_commands`、`build_handler_table`、分派、`HandlerKind`、共享 helper | 1–754、6781–末 |
| `handlers/control.rs` | GOSUB/RETURN/GO/IF/ELSE/IFBLEND/IFEND/IFBREAK/IFCONTINUE/LOOP/LOOPEND/LOOPBREAK/LOOPCONTINUE/WAIT/FPS/END | 755–1377 |
| `handlers/variables.rs` | LET/DECLARE/VARINFO/VARACT/MATH/TASK/TASKEND/TASKINFO | 1378–3251 |
| `handlers/file_io.rs` | CSV/CSVINFO/FILEINFO/SAVE/LOAD/SAVEEND | 3441–4659 |
| `handlers/media_info.rs` | MOVIEINFO/MOVIE/MOVIEEND/YMV/MATH3D/INFO/SOUNDINFO | 4660–5398 |
| `handlers/text.rs` | TEXT/TEXT_FILE/TEXTINFO/TEXT_OUT/WORD/RETURNCODE | 5399–5715、5914–6094 |
| `handlers/product.rs` | PRODUCT（v555 热区，单独放） | 5716–5913 |
| `handlers/system.rs` | SYSTEMINFO/INPUT/MOUSE/CLIPACT/CLIPINFO/REGINFO/ERROR/DIALOG/LABELINFO | 3252–3440、6095–6780 |

规则：

- 固有 `impl Vm` 可跨同 crate 子模块；子模块是 `vm` 的后代，
  可访问 `Vm` 私有字段。
- 跨模块调用的方法改 `pub(super)`（必要时 `pub(crate)`），
  按编译器提示最小化，不提前全公开。
- `handlers.rs` 内 6 个 `#[cfg(test)]` 块随所属命令族搬走。
- 共享 helper（`build_varact_plan`、`param_index_of`、
  `str_char_count_with`、`copy_chars` 等）留在 `mod.rs` 或
  `handlers/helpers.rs`，保持原可见性/再导出。
- 分三小批：A=`product`+`text`；B=`file_io`+`media_info`；
  C=`control`+`variables`+`system`。每批单独编译、跑测试，
  不把 P0 改动混进搬运批。
- 验证 `git diff` 以移动为主；函数体逐字不变。

**出口**：`handlers/mod.rs` 只剩分派/表/helper（目标 ≤1,200 行），
各子模块职责清晰；测试与 golden 全绿。

## 6. O3：fixture 拆分（中）

`tests/fixture.rs` 保留为测试入口，新增子模块目录：

```text
tests/fixture.rs
tests/fixture/common.rs      // boot/no_input/user_var/push_var/boot_with_files
tests/fixture/control.rs     // IF/LOOP/GOSUB/WAIT/FPS
tests/fixture/variables.rs   // DECLARE/LET/VARINFO/VARACT/MATH/TASK
tests/fixture/text.rs        // TEXT/TEXT_FILE/TEXTINFO/PRODUCT
tests/fixture/file_save.rs   // FILEINFO/CSV/SAVE/LOAD
tests/fixture/media.rs       // MOVIE/YMV/MOVIEINFO
tests/fixture/system.rs      // SYSTEMINFO/INPUT/MOUSE/CLIPINFO/REGINFO/DIALOG
```

- 顶部共享 helper 移到 `common.rs`，`pub(crate)`；
  各测试保持名称与断言，只改变模块路径。
- 标准模块解析，不用 `#[path]`；`support` 保持现状。
- 136 个测试按域搬迁，搬完 `cargo test -p yu-rt --test fixture`
  数量与结果不变。

**出口**：根文件 ≤300 行，测试可按域定位；无测试语义变化。

## 7. O4：剧本词法器差分契约（中–大）

这是本计划允许改行为的批次之一：O4 先钉词法器，O5 再钉语言选择/编码；
其余批次保持行为不变。

1. **取 oracle**：用官方引擎 + 现有 `probe_trace555`/REA，抓
   `PRODUCT[COM=22]` 对合成 `scenario/*.txt` 行的输入/输出
   （整数数组、字符串数组、token 数、返回偏移）；记录 RE 地址
   （`0x45a9dc`）或 trace 编号。
2. **golden**：`tests/golden/scenario_tokens.jsonl`，每行
   `{input_hex, expected:[{code,text_hex}], count, source, anchor}`；
   只用合成行，不含商业文本。
3. **测试**：`tests/scenario_tokens.rs` 逐例跑 `scenario::tokenize`；
   未解决 oracle 的用例显式标 `unresolved` 并计数，另设断言固定该数量
   （新增未知必须让测试失败），禁止空跑。
4. **待确认差异清单**（以 oracle 为准，不先猜）：
   - 字符串 token 码：当前 `0x22`，记录中疑为 `0x1f`；
   - CR 是否产出 token；
   - `//` 是否只吞行、是否保留 token、文本内容；
   - `/* */` 块注释是否跨行；
   - 多字符运算符（`==`/`!=`/`<=`/`>=`）、`;`、`:` 等；
   - `PRODUCT[COM=22]` 写入整数数组的基址/偏移与越界行为。
5. 若 oracle 证明跨行状态存在：新增 `ScenarioLexer { state }` +
   `feed_line`，`tokenize` 保留为单行无状态包装，调用方迁移；
   这是唯一允许的 API 扩展。
6. 每处修正单独建 `docs/divergences/DIV-RT-0xx-*.md`，注明 oracle 锚点；
   0.495 不经过该路径，必须不受影响。
7. 补 `PRODUCT[COM=22]` 集成测试：正常、数组越界、空行、未知字节、
   缺省参数回退。
8. golden 同时覆盖 CP932/GBK/Big5 合成行；多语言变体与语言选择见 O5。

**出口**：每个已知差异要么被 oracle 修正并有测试，要么显式 unresolved
+ DIV；`scenario_tokens` 成为 M2 的前置门。

## 8. O5：v555 多语言变体与语言选择（中）

### 8.1 已取证事实（2026-11-20 补）

v555 的多语言不是“每个语言一份 exe”，而是**同一引擎 + 三份文本变体
+ 配置里的语言号**。

- 文本资产（`<v555-2/3/4>` 样本）：
  - `scenario/*.txt`：基准（日文）
  - `sc_cn_scenario/*.txt`：简中
  - `sc_tw_scenario/*.txt`：繁中
  - 三份同标签、同行结构；`es_scenario_load.yst`（s60）pc0–9 用
    `TEXT[FILE]` 分别读入 `$2868/$2869/$2870`（第一组）与
    `$2871/$2872/$2873`（第二组）。
- 选择在脚本里，不硬编码：
  - `es_scenario_parser.yst`（s61）pc129–137：
    `@1002 == 1` → `$2872`（简中）；否则 `@1001 == 1` → `$2873`（繁中）；
    否则 `$2871`（基准/日文）。
  - `ES.LANG.SET`（s26 pc0）按 `@1177[110,19]` 的语言号设置：

    | 语言号 | @1000 | @1002 | @1001 | UI 资源前缀 |
    |---:|---:|---:|---:|---|
    | 0 | 1 | 0 | 0 | `cgsys/` |
    | 1 | 0 | 1 | 0 | `cgsys_cn/` |
    | 2 | 0 | 0 | 1 | `cgsys_tw/` |

    并 `GOSUB es.BT.TASK.INIT` 重建按钮任务；`@1000` 的消费点待确认
    （疑似日文/字体模式）。
- 配置持久化：`config.sd` DNO=1 → `@1177`（2D 配置数组），
  DNO=2 → `$1176`（s25 pc20–23）；`ES.CONFIG` 菜单写回；
  `@1177[141,1..3]` 是版本/迁移旗标。
- 编码（对真实非 ASCII 行逐行解码的成功数）：

  ```text
  scenario/        CP932 5720/5720   GBK 5720/5720   Big5 0/5720
  sc_cn_scenario/  GBK   6228/6228   Big5 5811/6228  CP932 641/6228
  sc_tw_scenario/  Big5  6207/6228   CP932 5909/6228 GBK 23/6228
  ```

  结论：基准 = CP932、简中 = GBK、繁中 = Big5。全局 `--patch gbk`
  会把基准/繁中一起按 GBK 解，不能作为多语言方案。
- 缺变体：官方 `TEXT[FILE]` 打开失败返回 -1，脚本继续（不是 fatal）。
- `lm_res/gui/lang/*.po` + `lm_gui_config.json` 是 DRM/激活对话框的
  本地化层（hans/hant/en/jp），不是剧情文本；不实现，避免和游戏语言
  混为一谈。

### 8.2 当前内核差距

- `TEXT[FILE]` 已能读三份原始字节；缺口在“选哪份 + 用什么编码”。
- `--patch gbk` 是全局执行 codec：能让简中过，但会破坏基准日文/繁中；
  多语言 v555 不应依赖它。
- `@1000/@1001/@1002` 目前没有封装；语言号从 `config.sd` 读入后，
  要保证脚本能拿到、UI 能按 `cgsys_*` 前缀解析。
- `config.sd` 的 DNO 1/2 形状和 `ES.CONFIG` 菜单/写回需要与官方对拍；
  不能只改内存变量不落盘。
- P0-2 的按钮纹理 `cgsys_cn/main/button/...` 与语言前缀直接相关；
  当前缺失可能同时是“路径前缀没接上”和“状态机没接上”。

### 8.3 设计

- 新增**运行时** `LanguageProfile`（区别于表派生的 `SemanticProfile`）：
  - `selected: Base | Hans | Hant`
  - `variants: { base: bool, cn: bool, tw: bool }`（启动时探测）
  - `codec: { base: Cp932, cn: Gbk, tw: Big5 }`
  - `ui_prefix: "cgsys/" | "cgsys_cn/" | "cgsys_tw/"`
- 内核不替脚本决定语言：`@1000/@1001/@1002` 由 `ES.LANG.SET` 写；
  内核负责：
  1. `TEXT[FILE]` 读三份并写 `LET` 数组（已有）；
  2. `LOAD/SAVE config.sd` 正确保留 `@1177`/`$1176`；
  3. `cgsys_*` 资源按当前语言前缀解析；
  4. 文本渲染按当前变体 codec 解码，而不是全局 `--patch gbk`。
- `--patch gbk` 保留给“汉化补丁/旧工程”兼容；多语言 v555 不再要求。
- 切换语言 = 脚本调用 `ES.LANG.SET` → 重建按钮任务 + 重载当前场景；
  内核按 `LanguageProfile` 取 codec，不做全局热换。
- `.po`/`lm_res` 明确不实现（DRM 激活 UI，非游戏内容）。

### 8.4 调研/实现清单

1. 用官方 trace/REA 确认 `@1177[110,19]` 的 0/1/2 语义与 `@1000`
   的消费点；记录 `ES.LANG.SET` 的 RE 地址或 trace 编号。
2. 用真实 `\LC`/文本行确认三份变体的编码；补合成 golden
   （CP932/GBK/Big5 各一组）。
3. 对拍 `config.sd` DNO 1/2 的块形状、`ES.CONFIG` 写回后的字节差异；
   目标：官方切换语言后，内核能读出同一语言号。
4. 对拍 `sc_cn_`/`sc_tw_` 缺失或短文件时 s60 的返回值与 s61 行为；
   明确“缺变体 → 基准”的回退。
5. 对拍 `cgsys_cn/`/`cgsys_tw/` 资源清单与 `es.BT.TASK.INIT` 重建路径；
   这同时是 P0-2 的输入。
6. 合成测试工程：base/cn/tw 同标签、不同 `\LC` 文本，语言号 0/1/2；
   断言 parser 取到对应行；缺变体走 base；三种编码往返。
7. 0.495 回归：不经过 `sc_cn_`/`sc_tw_`，行为不变。

### 8.5 出口

- 多语言 v555 样本不再需要 `--patch gbk` 才能读简中/繁中；
- 语言号从 `config.sd` 读入、切换后 UI/文本一致；
- 三份变体缺失/短文件有显式行为与测试；
- P0-2 的按钮纹理路径按语言前缀解析；
- 0.495 与单语 v555 不回归。

## 9. O6：探针与文档卫生（小）

- 按实际清单分类现有 `examples/` 探针：稳定复现的
  （trace/vars/asset/pack walk/movie）保留并补 `--help`/用途注释；
  一次性探针不混入产品入口。
- 统一探针产物路径到 `crates/target/` 或工作区临时目录；
  不提交样本/截图/反编译产物。
- 新增 `docs/tools/probes.md` 索引：探针名、用途、输入、输出、是否稳定。
- 删除任何文件前逐项列清单确认，不批量清空。

**出口**：探针可发现、可复现、产物不污染工作区。

## 10. O7：诊断与性能（小–中，最后）

- 汇总 `rt.cmd.unimplemented` / `rt.sysvar.unimplemented` 按命令族计数，
  形成 capability ledger（实现/对齐空桩/未实现），数据来自 handler table
  + 运行事件，不新增手工表。
- release 构建做一次 boot/每帧/分配热点测量；只修有数据的热点，
  不做提前优化。
- 该批不改变语义，测量结果写入 `docs/architecture/`。

## 11. 推荐执行顺序

1. **O0 + O1**（低风险，先给后续门控一个家）
2. **P0-1..P0-4**（按 gap 文档 → M1，标题可见）
3. **O4 + O5**（M2 正文前必须完成：词法器 + 多语言选择/编码；
   O5 的 RE 可与 P0 并行，代码落地排在这里）
4. **O2 + O3**（M1 验证基线稳定后做纯搬运；不与 P0 混批）
5. **O6 + O7**
6. 回到 M2–M4

> 如果更在意「先清结构再继续 P0」，把 O2/O3 整体移到第 2 步前即可；
> O1 之后两边无依赖。推荐仍是先 O0/O1，再 P0，避免重构期间画面基线
> 不可比。

## 12. 验证

每批结束执行：

```bash
cargo test -p yu-rt -p yu-save
cargo test -p yu-rt --test fixture --test golden
cargo build --workspace --all-targets
```

本机 mimalloc 链接问题按既有办法：headless 用 `--no-default-features`，
桌面用 `--no-default-features --features desktop`；不因构建环境问题改
产品默认特性。

跨版本回归：

- 0.495 ERIS：136 脚本、命令 census、VARINFO 使用面、事件 golden
  逐字节不变；
- v555：O0 基线（Fatal 位置、诊断计数、画面）在 O1/O2/O3 后不变；
  P0 批次完成后建立新基线；
- v555 多语言（O5）：
  - 语言号 0/1/2 分别取到 base/简中/繁中同一标签的对应 `\LC` 行；
  - 不再要求 `--patch gbk`；base=CP932、cn=GBK、tw=Big5 往返；
  - `config.sd` DNO1/2 读入/写回后语言号一致；
  - 缺 `sc_cn_`/`sc_tw_` 或短文件时回退 base，不是 fatal；
  - `cgsys/`、`cgsys_cn/`、`cgsys_tw/` 前缀按语言解析；
- 不运行 `UPDATE_GOLDEN`；golden 任何变化都必须解释。

结构验收：

- `handlers/mod.rs` ≤1,200 行，各命令族独立文件；
  `fixture.rs` 根 ≤300 行；
- 生产代码中 `yscm.version` 直接比较只允许出现在 `semantics.rs`
  （SJISCODE 及后续门控）；
- `LanguageProfile` 是运行时配置/文件事实，不塞进 `SemanticProfile`；
  多语言路径不以全局 `--patch` 作为唯一 codec 来源；
- 词法器：oracle 用例全过，unresolved 数量固定且可见；
- 探针索引存在，产物路径统一；
- `PROGRESS.md` 追加成果（含命令与原始结果），语义修正补 DIV。

## 13. 风险与回滚

- **搬运批**：只移动、不改函数体；每小批编译+测试；borrow/可见性问题
  按编译器最小化 `pub(super)` 解决。出问题按批回退，不跨批修。
- **profile 变成上帝对象**：只允许「当前有门控」的字段；
  宿主/路径/补丁事实一律不进。
- **词法器改行为**：先 oracle 后代码；每处差异单独 DIV；
  0.495 路径不经过，必须回归验证。
- **多语言编码**：不要把 `--patch gbk` 当默认；base/cn/tw 三种 codec
  必须由 `LanguageProfile` 选择，并用真实 `\LC` 行做 golden。
- **config.sd 兼容**：语言切换必须落盘/读回；只改内存 flag 会造成
  “本次生效、重启回日文”的假象。
- **P0 与重构混批**：禁止；P0 改动与搬运批分开验证，避免画面基线不可比。
- **git**：全程不执行写操作；每批完成后等主人审阅/授权再提交。
