# 任务书 C · 运行核 yu-rt(M2)——三会话中优先级最高

> 会话定位:你独占 `crates/yu-rt`;不动根 Cargo.toml 与其它 crate。依赖:`yu-log`(A)、
> `yu-format`(B,接口契约见任务书 B §4,**已冻结**——B 落地前你按契约+手工 fixture 先行)。
> 本任务是内核语义的心脏,验收标准只有一句话:**每一条语义都可指认出处,每一条行为都对照过真实数据**。
>
> **v2 变更(2026-10-19,用户决定): 算法/常量/流程可直接采用**——从下列开源实现得到的
> 调度算法、执行位交换、分派表构建、命令实现流程等允许直接采用;采用处文件头注明
> `参考 <项目>/<文件>`,并在 `docs/references/open-source-tools.md` §4 致谢链登记。
> **但真值层级不变**: 官方手册/官方引擎行为 > 开源实现;开源算法只是加速实现,不是证据。

## 0. 开工前必读(顺序)

1. `<repo>/AGENTS.md` → 2. `docs/architecture.md`(A5 零硬编码;§2 yu-rt 行;§8 错误系统)
   → 3. `docs/sdk-eris-analysis.md`(**§2 语法 / §4 调用约定 / §5 推进模型 / §6 输入输出——这是你的语义总纲**)
   → 4. `docs/oracle.md` → 5. 官方手册目录:`<side-repo>/target/sdk/0.495/yu-ris_0495_032/マニュアル/YU-RIS/html/`
   (control/ variable/ system/ … 每个命令一页,**这是逐命令语义的一手规格**)。

**参考实现索引(用户授权:启动链与命令覆盖面可参考旧接手线 `<repo-old>`):**

> **旧线仓库根 = `<repo-old>`**(真名见 `AGENTS.md` 的占位符对照表;本机即
> `<repo-old>`)。**只读参考,禁止修改其工作区、禁止在其中执行任何 git 操作。**
> 算法/流程可直接采用并致谢(v2)。

| 旧线文件 | 可提取的事实 |
|---|---|
| `crates/yuris-vm/src/boot.rs` | 启动链编排顺序(载命令表→标签→变量→设置→入口脚本→置 PC),入口标签缺省名 |
| `crates/yuris-vm/src/exec/`(flow/vars/media/io/misc/…) | **进游戏真实用到的命令覆盖面与分组**——你的实现频度优先级直接以它为起点 |
| `crates/yuris-vm/src/{task,driver,host}.rs` | 任务/让出/交换执行位的工程形态(与手册 §5 语义互证) |
| `crates/yuris-vm/tests/{vm,diag_flow}.rs` | 对真实脚本的行为断言 = 现成验证期望 |

**开源算法直接采用来源(v2):**

| 来源 | 可直接采用的算法/流程 |
|---|---|
| 旧线 `crates/yuris-vm/src/{task,driver,boot}.rs` | 任务链/让出/执行位交换(swap_exec)/启动链编排算法 |
| 旧线 `crates/yuris-vm/src/exec/` | 命令处理器实现流程与分派表构建(按 ysc 名运行期建表) |
| `shimamura-sakura/yuri`(Rust) | 反编译结构、命令语义参照(需与手册互证) |
| `RxYuris/lib/YurisStaticLibrary/YSCM.cpp` | 命令表构建流程 |
| `docs/references/open-source-tools.md` | 其余工具与许可/致谢约定 |

纪律: 采用算法不等于采用其语义结论;每条语义仍必须有三层出处(手册/真实脚本/官方引擎观测),
缺一即 `Unimplemented` 挂账。文件头注来源,致谢条目登记到 §4。

界限:旧线是行为参照,不是真值——它的语义与你从手册/官方引擎观测到的结论冲突时,
以官方为准并在 divergences 给旧线记勘误;不整段搬运代码,注明参考出处。

## 1. 三层真值纪律(本任务的存在意义,不可妥协)

实现每一条语义前,必须拿到三层出处并**在代码 doc 注明引用**:

1. **手册页**:该命令的官方页面(相对路径写进 doc 注释)。没有手册页的命令:不实现。
2. **真实脚本印证**:该命令在真实脚本里的使用形态与**真实使用频度**——
   语料 = 官方 YSCom 编译的 ERIS 产物(`workbench/project-eris/data/ysbin/`,137 个 YSTB),
   **不使用 <game-b> 语料(用户决定)**;按频度排实现优先级,没出现的用法不预支实现。
3. **行为对照**:官方引擎 0.479(SDK 在 `<side-repo>/target/sdk/0.479/yu-ris_0479_014/システム`,
   **用户已确认可直接启动**)对最小脚本的可观测行为(输出窗口文本/画面结果/存档产物),
   作为调度与让出语义的行为 oracle。取错不准——只记可观测事实。

**拿不齐三层的:一律 `Diag::Unimplemented` 挂号**,条目进 `docs/divergences/`。
禁止"看起来应该是这样"——这是本任务第一条军规。

## 2. 范围(全部数据驱动,A5 零硬编码)

1. **表装载**:启动时从游戏工程装载 ysc 命令表(B)、ysl 标签、ysv 变量镜像、yscfg 设置;
   缺表/表不完整 ⇒ `Diag{code:"table.*", severity:Fatal}` **拒绝启动**。运行核内禁止内置任何
   命令名/序号/默认表;分派 = `table.name_of(index)` → 注册处理器。
2. **任务调度**(analysis §5):任务集合并发轮换、**让出点仅 WAIT/显示文本**、任务全让出=帧边界、
   `FPS[SET=1..256]` 由脚本设定(时钟注入,绝不读墙钟)、任务层 `TASK[ID E Z # SCRIPTPOS]`、
   RETURN 无嵌套即消亡。
3. **调用约定**:PINT/PFLT/PSTR×16 + RINT/RFLT/RSTR×16 + PFLG/RFLG + 任务局部 T*;
   编号跨类型共用;GOSUB 嵌套 64;`_PFLG` 缺席位语义按手册。
4. **表达式求值**:i64/f64/String、≤8 维数组(步长数组)、`@/$` 变量、系统变量
   (`@_KEY_*` 带符号帧计数器、`@_TIME`、`@_PINT(n)`…——系统变量表从 ycd 导出数据装载,不写死);
   运算语义(溢出回绕/整除符号/类型提升)**有官方行为才实现,否则 Unimplemented**。
5. **控制流**:GO/GOSUB/RETURN/IF 族/LOOP 族/WAIT/END;跨脚本 GO(host 换脚本)。
6. **命令处理器按真实频度排序落地**(来自 B 的语料统计),每命令一个模块+单测+出处注释。

## 3. 真实对照(验收核心)

1. **事件流 golden**:对真实脚本的一个切片(如某 yst 脚本的纯逻辑段),
   时钟/RNG/输入时间线全部注入固定值 ⇒ 事件流(命令执行序列+变量终值)golden 化;
   同配置重放必须逐字节一致(A3)。
2. **错误路径 golden**:构造"未知命令/缺槽/坏标签"输入,断言 Diag 码与继续/中止行为符合
   已确认的官方策略(官方策略未知 ⇒ 挂账,不臆造)。
3. **零硬编码审计**:对 `crates/yu-rt` grep 守卫(版本序号/内置命令名/默认路径常量),
   审计脚本入 `scripts/`。
4. **行为对照记录**:官方 0.479 引擎每次实测的最小脚本+观测输出,存
   `docs/evidence/rt-obs/`(小体量),被对应单测引用。

## 3.9 落地记录(成果 C2,2026-10-19)

见 `PROGRESS.md` 成果 C2 与 `crates/yu-rt/`:

- 实现清单与逐命令三层出处:`crates/yu-rt/docs/command-provenance.md`;
- 未决/差异:`crates/yu-rt/docs/divergences/README.md`(DIV-RT-001..020);
- 零硬编码审计:`crates/yu-rt/scripts/audit_hardcode.py`(当前 0 违规);
- 商业发行版(**无 `YSCom.ycd`**)路径已实测通过,见 DIV-RT-016。

## 4. 交付

- `cargo test -p yu-rt` 全绿;golden + 审计 + 出处注释齐备;
- `PROGRESS.md` 追加"成果 C"(验证命令+原始输出);未决逐条 divergences;
- 报告必须含一张表:**已实现命令 × 三层出处状态**(手册✓/脚本印证✓/行为对照✓/缺失项),
  这张表就是"真实"二字的交付物。
