# DIV-RT 系列 · yu-rt 与官方引擎的已知差异与挂账

> 制度:`docs/architecture.md` §8、任务书 C §1。每条差异/未决都必须留档,
> 含:现象、证据、处置(已实现/`Unimplemented`)、复核方式。
> 状态取值:`open`(待取证)、`fixed`(已修正)、`wontobserve`(官方不可观测)、
> `blocked`(缺样本/缺工具)。

| 编号 | 主题 | 状态 | 影响 |
|---|---|---|---|
| DIV-RT-001 | 引擎 0.479/0.495 的 opcode 编号整体错位 | open | 跨版本脚本必须用各自的 ysc 表 |
| DIV-RT-002 | 表达式类型提升/溢出/字符串↔数值细节 | open | 已按确定性规则实现,逐位未对齐 |
| DIV-RT-003 | 整数除零/取模 0 | open | 引擎为硬件异常;内核 Fatal |
| DIV-RT-004 | 「实参缺席」的运算符缺右操作数形态 | open | 已按语料唯一形态容忍 |
| DIV-RT-005 | 脚本执行错误后的引擎继续策略 | open | 内核一律「让出+结束+记录」 |
| DIV-RT-006 | 输入键码 `_CODE_*` / `_INP` / `_PAD` 的码值 | fixed | REA 证结构(VK 计数表)+ 手册不变量定值;`_KEYLAST/_MOUSELAST` 读取仍挂号 |
| DIV-RT-007 | 工程 ycd 声明但手册无页的系统变量 | open | 一律 `Unimplemented` |
| DIV-RT-008 | `_`(文本输出)手册无页 | fixed | 手册 basic.html ●テキスト表示有页(`_[式]`);已实现,撤销旧挂账 |
| DIV-RT-009 | `_TIME`/`_OS_TIME` 的时区与纪元 | open | 注入时钟固定 UTC |
| DIV-RT-010 | 路径类系统变量(`_SAVE_PATH` 等) | blocked | 需宿主注入 |
| DIV-RT-011 | 手册有页但语义未取证的命令(VARINFO SEARCH / VARACT INIT·G_* / F_* / ERROR) | open | `Unimplemented`(RETURNCODE/WORD 已另证实现) |
| DIV-RT-012 | `SAVE`/`LOAD` 的盘上存档格式 | blocked | 交宿主,内核不猜 |
| DIV-RT-013 | `HANTOZEN`/`ZENTOHAN` 转换表 | open | 需移植引擎三张表 |
| DIV-RT-014 | `MATH[RND]` 的 MT19937 种子来源 | open | 需注入 RNG |
| DIV-RT-015 | `TASK` 的 TINT..10/TFLT..10/TSTR..10/SCRIPTPOS 参数 | open | TINT/TFLT/TSTR/SCRIPTPOS 已按官方源码生产者/消费者对实现;TEXTPOS/RESTART/A 仍挂号 |
| DIV-RT-016 | 商业发行版不带 `YSCom.ycd` | fixed | 改 ycd 可选 + 引擎 ABI 表 + ysv 自证校验 |
| DIV-RT-017 | 变量空间前缀 `0x60` / `0x23` | open | 5+2 处,挂号继续 |
| DIV-RT-018 | `ELSE` 的 0 窗口形态 | fixed | 商业包 726 次;无条件进 else 体 |
| DIV-RT-019 | `VARACT[DIMSIZE/PUSH/POP]` 无 `LET` | fixed | 手册例 9/10 即此形态 |
| DIV-RT-020 | 数组负下标 / 越界访问 | fixed | 根因是 lvalue 解析 + 未实现的 VARINFO/VARACT |
| DIV-RT-021 | 输入系统变量可写(`@_MOUSE_L=5` 等) | fixed | 写入 = 设定帧计数器 |
| DIV-RT-022 | 系统变量的宿主注入面(色深/路径/DX/媒体只读/声音) | fixed | `BootConfig` 注入 |
| DIV-RT-023 | `SYSTEMINFO` 手册无页但启动链硬依赖 | open | 按引擎处理体实现 + 手册缺页挂账 |
| DIV-RT-024 | `VARACT[INIT + G_*]` 手册无页 | open | 按引擎 §3.26 算法实现 |
| DIV-RT-025 | `TASKINFO` 的数值字段(LINE/FILE/嵌套数) | fixed | 用任务记录 + 数据区 B 行号 |
| DIV-RT-026 | `yst_list` 每脚本的文本数 | fixed | 记录第 3 个 u32 |
| DIV-RT-027 | `ELSE` 判定顺序 / IF 先压帧 | fixed | 引擎 §3.13 顺序 |
| DIV-RT-028 | 文本流三件套(WORD/`_`/RETURNCODE) | fixed | YSCom 合成;源码↔编译产物↔手册+RxYuris 三方证 |
| DIV-RT-029 | `_CODE_*` 的码值来源 | fixed | 引擎 BSS 字节表(REA)+ VK 规范;数据文件 button-code-abi.tsv |
| DIV-RT-030 | 手册检索的二进制误判 | fixed | 手册 html 含非文本字节,普通 grep 误报"无页";字节级 grep 重审全表 |
| DIV-RT-031 | `_TEXT`/`_MENUCODE`/`_WINDOW_EXEC_*` 状态系统变量 | fixed | 手册有页;内核状态 + 宿主 setter |
| DIV-RT-032 | `_TEXT_FLAG` 的控制字节流 | open | 读取 + `WORD`/`_`/`TEXT`/`RETURNCODE` 写入已按 REA 实现;脚本写入 `_TEXT_FLAG` 仍挂号 |
| DIV-RT-033 | `_KEYLAST`/`_MOUSELAST` 读取 | open | 手册无页(两版核对);InputState 内部照常记录 |
| DIV-RT-034 | panic 收口(任务书 E R7) | fixed | IFBREAK LV=0 / GOSUB·RETURN 参数名越界 / DIMSIZE·维长乘积防御上限 |
| DIV-RT-035 | 文本让出点是 `RETURNCODE`,不是 `WORD`/`_` | fixed | 任务书 E R3;REA 实证(见下) |
| DIV-RT-036 | 主任务 ID = 脚本可见 `SYSTEM` | fixed | 手册 + 引擎启动链均按名称 `SYSTEM` 建任务 |
| DIV-RT-037 | `TASKEND` 立即摘链(不等到帧尾) | fixed | REA 0x4521dc/0x452364;`EXIST` 立即为 0 |
| DIV-RT-038 | 任务链按 Z 降序;同 Z 按创建序 | fixed | 手册 Z 语义 + REA 有序插入 0x4613c0;同 Z 顺序手册称「不確定」,此处取引擎实际插入序 |
| DIV-RT-039 | GOSUB 允许 64 次嵌套 | fixed | 手册 64 回 + REA 0x4459e6(`depth+1 == 0x40` 才报错) |
| DIV-RT-040 | 帧边界事件(`FrameStart`/`FrameEnd`)与有界事件队列 | fixed | 内部可观测契约(architecture §3/A3),非官方语义 |
| DIV-RT-041 | `Diag` 的脚本定位(task/script/pc/cmd) | fixed | architecture §8;`Loc::Script.label` 承载任务 ID |
| DIV-RT-042 | `DomainCall` 的变量引用身份丢失 | blocked | `SAVE[SET]`/`LOAD[LET]`/`FILEINFO[LET]` 只带求值结果;跨 crate 契约变更 |

---

## DIV-RT-001 · opcode 编号跨版本错位

- **现象**:0.495 比 0.479 多 15 条命令(`EMOTE`/`EMOTEINFO`/`F_*`/`G_*2..4`/
  `S_*2..4`),插在字母序中间 ⇒ index 12 之后整体错位。
- **证据**:`<side-repo>/docs/format-vm-opcodes.md` §3.17「版本差异」;
  本仓库实测 0.495 `ysc.ybn` 119 条与工程 `YSCom.ycd` 119 条完全同序。
- **处置**:运行核**从不使用内置序号表**;`opcode → 名字` 一律查工程 `ysc.ybn`
  (`crate::tables::YscmTable`)。跨版本 = 换数据,不换代码(A5)。
- **复核**:`cargo test -p yu-rt --test corpus` 对真实 0.495 语料逐组反查名字。

---

## DIV-RT-002 · 表达式类型提升 / 溢出 / 字符串↔数值

- **现象**:引擎分「整数路径 / 浮点路径」两套(旁线 §3.9.3:只要还有未消费的
  变量引用帧,一律走整数路径),且 x87 舍入为「就近到偶数」;字符串→数值扫描
  后若有剩余字符报错。
- **已实现**(确定性):
  - INT+INT 用 i64 环绕;C 的 `%` 语义用 `wrapping_rem`;
  - 混合数值提升 f64;
  - `$()` 浮点用 `%f`(六位小数)—— 有引擎证据(§3.22);
  - `@()` 支持 `0x` 十六进制与十进制/浮点,非法输入 → `Unimplemented`。
- **未对齐**:整数/浮点「路径选择」的完整判据、浮点→字符串的 x87 舍入细节、
  `-0.0` 等边界。**处置**:`docs/divergences` 挂账;逐位对拍需官方 trace。
- **复核**:`crates/yu-rt/tests/fixture.rs::arithmetic_*`、
  `crates/yu-rt/src/value.rs` 单测。

---

## DIV-RT-003 · 整数除零 / 取模 0

- **现象**:引擎整数除法走 CRT/x87 路径,除 0 是**硬件异常**(旧线观测为
  进程级崩溃;见 `<repo-old>` PROGRESS 成果 133 的实测记录)。
- **处置**:内核返回 `Diag{code:"rt.expr.div_zero", severity:Fatal}`,**不静默
  返回 0**(静默会让脚本走错分支且无法审计)。
- **代价**:一条除零会终结该任务(策略见 DIV-RT-005)。若官方 trace 证明
  引擎在某处容忍除零,须改回并留勘误。

---

## DIV-RT-004 · 「实参缺席」形态

- **现象**:语料中 20 个窗口以 `42 01 00 00 41 00 00` 结尾(`PushInt(0)` +
  一个缺右操作数的 `&`),其中 19 个是完整的 `X & 0xff` 掩码,**只有 1 个**
  是真的「实参无值」(`es.DEBUG.DEFINE` 的分隔行)。
- **处置**:`expr::Evaluator` 只对「最后一个操作数不足」的二元运算容忍一次,
  且把栈上已有的 0 作为该参数值;其它操作数不足仍报 `rt.expr.malformed`。
- **复核**:`crates/yu-rt/src/expr.rs` 的 `step` 分支 + 语料统计脚本
  `crates/yu-rt/scripts/corpus_stats.py`。

---

## DIV-RT-005 · 脚本执行错误后的官方继续策略

- **现象**:官方引擎处理体返回错误码后的调度行为**未取证**。
- **处置**:调度器统一「本任务让出 + 标记结束 + 记录到 `ChainReport::errors`」,
  并把 Diag 写进事件流(`VmEvent::Diag`)。这是**显式策略**,不是猜测的官方行为;
  一旦拿到官方 trace 立即核对。
- **复核**:`fixture.rs::unknown_label_is_fatal_and_recorded`、
  `fixture.rs::undeclared_variable_write_is_reported`。

---

## DIV-RT-006 · 输入键码与手柄

- **现象**:手册 `basic/svariable.html` 说 `@_KEY_*` 是「押され続けている
  フレーム数」(带符号计数器,已实现);`@_INP(n)` 的 `n` 是「对应按钮码」,
  而按钮码由 `@_CODE_*` 给出。`@_CODE_*` 在工程 ycd 里声明、但**初值为 0**,
  真实码值是引擎运行期填的 —— 无一手证据说明具体数值。
- **处置**:`_KEY_*`/`_MOUSE_L..D` 全部按 `ycd` 名字绑定并实现计数器;
  `_INP`/`_KEYLAST`/`_MOUSELAST`/`_PAD*` 一律 `Unimplemented`(不猜码表)。
- **2026-10-21(P0)修正**:按下沿必须从 `1` 重新起算(手册「押された瞬間から
  1 → 2 → 3」)。旧实现 `+= 1` 使长期松开后的短按从负数往上加,第二次点击
  永远到不了正值;已改为按下沿 `value = 1`,并修正
  `text_advance_requested` 的键名(`KEY_ENTER`/`KEY_CTRL`,不是
  `ENTER`/`CTRL`)。复核:`input::tests::repress_after_release_restarts_at_one`、
  `fixture::one_text_line_per_frame_until_advance_input`。
- **复核**:`crates/yu-rt/src/input.rs` 单测(手册三个例子的形态)+
  `sysvar.rs::read_button`。

---

## DIV-RT-007 · ycd 有声明、手册无页的系统变量

- **实测**:0.495 工程 `YSCom.ycd` 声明 548 项;手册 `basic/svariable.html`
  覆盖其中大部分。对 `_DXMODE`/`_CHR`/`_VARTYPE`/`_SYSTEM_FRAME` 等在手册
  HTML 里 0 命中(`grep -arl` 全目录验证)。
- **处置**:`Unimplemented`,即使语料高频使用(`_CHR` 560 次、`_DXMODE` 1,333 次)。
  语料频度只决定**优先级**,不构成语义证据(任务书 C §1)。
- **2026-10-21(M-1 普查)**:`_WINDOW_EXEC_CAPTION`(id 120)在 600 帧里
  被 `es_scenario.yst:231` 每帧 `IF` 轮询 **600 次**,是当前运行期最大的
  单点缺口;手册仍无页,故继续保持挂号,不凭 ycd 名字猜“窗口标题按钮状态”。
  引擎 int getter case 0x78(`0x427704` → `[0x801924]`) 只证明“读一个全局”，
  不足以定义写入/宿主事件语义。
- **2026-10-21(P1)有界取证**:
  - 0.479/0.495 全部手册 HTML 字节级搜索无 `_WINDOW_EXEC_CAPTION`
    (older 0.292/0.466 也无);三层仍缺①。
  - 语料语义(`es_Scenario.yst`):与 `@_MENUCODE==30001` 并列的
    `IF[@_WINDOW_EXEC_CAPTION==1]` → 写回 0 → `WINDOW[NO=0 WSX=… WSY=…]`;
    注释掉的旧路径是 `WINDOW[NO=0 MAXIMIZE=1]`。即「窗口尺寸再初始化」
    的一次性宿主事件标志。
  - REA:脚本写 `0x446f70`(`MOV byte [0x801924], DL`);引擎写在
    `WM_SYSCOMMAND(0xA3)` 路径 `FUN_0045a9d0`(`0x45a9d0`),`0x45a9e7`
    把 `*ecx + 1` 写进 `[0x801924]`(条件 `[0x801338]==1` 且
    `[0x8015e0]==0`)。`*ecx` 的来源与两个条件全局仍未取证。
  - 结论:它是宿主窗口事件标志,不是 VM 可计算语义;保持 `Unimplemented`,
    若放宽门禁应做宿主 setter(同 `_WINDOW_EXEC_MAX/CLOSE` 形态)。
- **复核**:`crates/yu-rt/src/sysvar.rs::undoc`;频度见 `docs/coverage-matrix.md` §3。

---

## DIV-RT-008 · `_`(文本输出通道)手册无页

- **证据**:手册 `text/` 分类只有 `text.html`/`textinfo.html`;旁线 §3.14 只证到
  「按类型格式化后追加全局缓冲 `[0x801800]+0x60`,上限 0x1ffe」,没有给出
  「何处触发上屏/换行/翻页」的可观测契约。
- **处置**:命令可执行但**不产出可见文本**,挂 `Unimplemented` 一次;
  文本层 `$_TEXT` 读到的仍是空串。
- **复核**:`fixture.rs` 未实现命令用例。

---

## DIV-RT-009 · 时间与日期

- **现象**:手册给出 `@_TIME(0,k)` 当前日期/`(1,k)` 启动时刻(k=年/月/日/
  星期/时/分/秒/毫秒)与 `@_OS_TIME(0/1)`。引擎读系统本地时间。
- **处置**:全部使用**注入时钟**(`BootConfig::start_ms` + 每逻辑帧步进),
  固定 UTC 纪元 1970-01-01,星期 0=日..6=土;**不读墙钟**(A3)。
  `_OS_TIME(0)`(OS uptime)需要宿主注入 ⇒ `Unimplemented`。
- **后果**:跨时区/夏令时的逐位一致性未对齐;golden 用同一注入时钟即可复现。

---

## DIV-RT-010 · 路径类系统变量

- `$_SYSTEM_PATH` / `$_SAVE_PATH` / `$_APPDATA_PATH` / `$_PERSONAL_PATH` /
  `$_PROJECT_PATH`:官方是真实文件系统路径。内核遵守 A2(数据只读、存档只写
  SaveDir),不猜宿主布局 ⇒ `Unimplemented`,由 CLI 装配层注入后解除。

---

## DIV-RT-011 · 手册有页但语义未取证的命令/参数

| 命令/参数 | 手册 | 缺什么 |
|---|---|---|
| `VARINFO[SEARCH]` / `STRTYPE` / `SJISCODE` / `UTF8CODE` | `variable/varinfo.html` | 引擎 §3.21 只证到分支地址,未给「起始下标/未找到」的精确返回 |
| `VARACT[INIT]` / `G_INT..G_STR4` | `variable/varact.html`(未列 INIT/G_*) | 引擎 §3.26 给了掩码位算法,但**手册无对应页**;三明治判据缺① |
| `RETURNCODE` / `RETURNBREAK` | 手册 `control/` 无独立页 | 语料 396 次高频使用,但零一手规格 |
| `F_INT` / `F_FLT` / `F_STR` / `F_VAR` | 手册 `variable/` 无页 | 同上(F_INT 337 次) |
| `WORD` / `VAR` / `PREP` / `PLUG` | 无页 | 不实现 |

处置:运行到即 `Unimplemented` + 事件流留痕,执行继续。

---

## DIV-RT-012 · 存档容器

手册 `file/save.html`/`load.html` 给出参数表(12 个脚本自备槽),但**盘上
`.sd` 字节布局未取证**。内核把 `SAVE`/`LOAD` 归入 DomainCall(Save 域),
不写任何文件;存档字节级对齐属 `yu-save`(任务书后续)。

---

## DIV-RT-013 · 全角/半角转换表

手册 `variable/varact.html` 写了 HANTOZEN/ZENTOHAN 的**行为**;引擎用三张
二进制表(§3.26:`0x808840` 63 项 / `0x5B5170` 95 项 / `0x5B5230` 126 对)。
表未移植 ⇒ `Unimplemented`(不手抄、不近似)。

---

## DIV-RT-014 · MATH[RND]

手册 `math/math.html` 例 17 给出 `RND` 的**范围**语义;引擎随机源是
**MT19937**(§3.20),但种子来源未取证。确定性要求(A3)⇒ 必须注入 RNG;
当前 `Unimplemented`。

---

## DIV-RT-015 · TASK 的局部参数

手册 `control/task.html` 列出 `TINT..TSTR`、`SCRIPTPOS`、`TEXTPOS`、
`RESTART`。本实现:`ID`/`E`/`Z`/`#`(生成与启停),其余参数运行到即
`Unimplemented`(任务局部寄存器已建模但未接参数传递;`SCRIPTPOS` 属存档锚点,
待 `yu-save`)。

---

## DIV-RT-016 · 商业发行版不带编译器定义(`YSCom.ycd`)

- **实测**:<game-b> `data.ypf`(v494,12,212 条目)里
  `ycd` / `yscom` / `ysconfig` 命中数均为 **0**;但 `ysbin/` 下
  7 张元数据表(`ysc/yscfg/yse/ysl/yst/yst_list/ysv`)+ 160 个脚本齐全。
  而官方 SDK 0.495(`project-eris`)带 `system/YSCom/YSCom.ycd`。
- **为什么不能"没有就拒绝启动"**:A5 的缺表拒启只适用于**游戏必须自带**的表;
  编译器定义是**开发工具链**的一部分,发行版不会带。把它当必需项 ⇒
  所有商业游戏都无法启动(与目标相反)。
- **为什么不能"内置命令名表"**:命令名/参数表逐工程不同(用户明确要求动态),
  所以 `ysc.ybn` 仍是唯一命令来源,`ycd` 只提供**系统变量名/类型**与
  参数属性(后者在无 ycd 时缺失,见下)。
- **处置(已实现)**:
  1. `YSCom.ycd` 改为**可选**:`src` 或 `aux` 根下找到就用;
  2. 找不到 ⇒ 用 `crates/yu-rt/data/sysvar-abi-0495.tsv`(由脚本
     `scripts/gen_sysvar_abi.py` 从官方 0.495 ycd 生成,带来源 SHA-256,
     `include_str!` 进二进制);
  3. **每次启动都用工程自己的 `ysv.ybn` 逐项校验** ABI 表
     (`SysVarTable::verify_against_ysvr`):前 548 条 `kind==1` 记录的
     `(var_id, value_type, dims)` 必须逐项相等,否则 `table.version` 拒启。
- **证据(本机实测)**:
  - 0.495 SDK ycd:548 项;`_TIME`=0、`_PAI`=10、`_LC`=48、`_PINT`=53、
    `_SCREEN_SIZE_X`=115、`_MOUSE_X`=133、`_INP`=144、`_KEY_A`=167;
  - 0.494 商业包 ysv 前 548 条 kind=1:与上面 ycd 的 (type, dims)
    **548/548 逐项一致,0 差异** ⇒ id 空间是引擎 ABI,不随游戏变化;
  - ERIS 0.495 语料同样 548/548 一致。
- **残余风险**:若某作品把引擎魔改到改变系统变量 id 空间,校验会拒启
  (这是期望行为:显式失败优于静默错位)。ycd 存在时以 ycd 为准。

## DIV-RT-017 · 变量空间前缀 `0x60` 与 `0x23`

- **实测**:ERIS 0.495 语料只用 `@`(0x40)与 `$`(0x24);
  商业 v494 语料出现 `0x60`(**5 处**,全在 `yst00091`)与
  `0x23`(**2 处**)。例:`yst00091 g45` 的
  `56 03 00 60 89 04 42 01 00 01 29` = 引用 `[0x60][0x0489]` 后取下标 1。
- **缺什么**:手册只定义 `@` 数值 / `$` 字符串;引擎侧旧笔记把 0x60 记为
  「罕见,语义 Unknown」。没有任何一手材料说明这两个前缀的寻址语义
  (是否等价于全局/局部/静态空间,或不同的变量域基址)。
- **处置**:运行到即 `Unimplemented` + 事件流留痕;**该次读写作废**,
  执行继续(不猜内存布局、不静默当 `@`)。

## DIV-RT-018 · `ELSE` 的 0 窗口形态

- **实测**:ERIS 0.495 的 887 个 `ELSE` **全部 3 窗口**;
  商业 v494 的 1,430 个 `ELSE` 中 **726 个是 0 窗口**。
  结构例(v494 s52 g59..g65):`IFBLEND` → `ELSE wc=0` → 体 → `IFEND`。
- **解释**:0 窗口 = 「无条件进入 else 体」(条件缺省为真),与 3 窗口形态的
  `ELSE[条件缺省]` 同义。手册 `control/else.html` 明确允许 `ELSE[]` 无条件形态。
- **处置**:`windows.len()==0` ⇒ 顺序执行下一条;`==3` ⇒ 原逻辑;
  其它 ⇒ `rt.cmd.windows` Fatal。

## DIV-RT-019 · `VARACT[DIMSIZE/PUSH/POP]` 不需要 `LET`

- **手册** `variable/varact.html` 例 9/10:
  `VARACT[DIMSIZE=500 SET=@A()]`、`VARACT[DIMSIZE+=1 SET=@A()]`
  —— **没有 `LET`**,直接改 `SET` 指向的数组。
- **实测**:商业 v494 出现无 `LET` 的 `VARACT`(`DIMSIZE=104` 次中多数无 LET)。
- **处置**:DIMSIZE/PUSH/POP 分支在要求 `LET` **之前**处理,目标 = `SET`。

## DIV-RT-020 · 数组负下标 / 越界访问

- **引擎判据**(旁线 §3.10):`index[k] > dims[k]-1` → 报错 `0x1a202`;
  **负下标没有写判据**,报错后是否中止脚本也未取证。
- **实测**:商业 v494 出现 `-1` 下标(常见于「空数组读末元素」)与
  `index=1 / dim=1` 的越界。
- **处置**:负下标与越界都按 `Unimplemented` 挂号并**作废该次访问**
  (读不返回值 → 上层命令按未实现处理;写不落地),不越界读写内存。
  这与旧线「钳制到 0..=n-1」不同:钳制会**静默产生一个假值**,
  比显式挂号更难审计。

---

## DIV-RT-021 · 输入系统变量可写(`@_MOUSE_L=5` / `@_KEY_ENTER=5`)

- **手册**:`basic/svariable.html` 用 ○ 标出「値を代入する事が出来るシステム変数」,
  输入族(`@_KEY_*` / `@_MOUSE_*`)全部带 ○。
- **语料**(ERIS `es_ERIS.yst` 的 `es.S.GO`):
  ```
  ///xシーン変わるときは入力一旦無効に.
  @_MOUSE_L=5
  @_KEY_ENTER=5
  ```
  注释写明意图:**切换场景时让输入一次性失效**。
- **引擎语义(旁线 §3.10 结论三 + 输入通路)**:输入系统变量**没有存储**,
  读写都落到输入子系统;写入即设定该按钮的帧计数器。
- **处置**:`LET` 目标是输入系统变量时,按名字(来自工程 ABI 表)反查按钮,
  调 `InputState::set_counter`。其它系统变量写入(如 `@_PINT`)仍按 DIV-RT-007
  挂号 —— 那些没有「写入 = 设定计数器」的证据。

## DIV-RT-022 · 系统变量的宿主注入面

启动链会读一批**宿主环境属性**;它们是手册有页的正式系统变量,但内核自己
不探测环境(A3:确定性)。这些值必须由 `BootConfig` 注入:

| 变量 | 手册页 | 语义 | 缺省 |
|---|---|---|---|
| `@_SCREEN_COLOR` | `basic/svariable.html` | 現在の色数(bpp) | 32 |
| `@_MEDIA_READONLY` | 同上 | 読み込み専用メディア(0/1) | `false` |
| `@_OS_SOUND` | 同上 | サウンド機能が利用可能(0/1) | `true` |
| `@_DXMODE` | **无页**(工程 ycd 有声明) | DirectX 缓冲图层通路 | 0 |
| `$_SYSTEM_PATH` 等 5 个路径 | 同上 | 启动/存档/用户路径 | `None` ⇒ `Unimplemented` |

`@_DXMODE` 的判定依据是语料而不是猜测:`es_first.yst` s10 pc81 的
`IF[@_DXMODE] { … CG[ID=DXBUF …] }` —— 非零才走 DirectX 缓冲图层分支,
所以它是「DX 缓冲可用」的宿主能力开关。

## DIV-RT-023 · `SYSTEMINFO`(手册无页,但启动链硬依赖)

- **手册**:`system/` 分类只有 `fps/input/mouse`;全手册 HTML 目录
  `grep -arl 'SYSTEMINFO'` **零命中**。⇒ 三层缺①。
- **为什么仍实现**:ERIS `text/es_Text.yst` 用
  `\_task.info(...)` → `TASKINFO` 取文本行号,而 `TASKINFO` 的 `LINE`
  依赖 `SYSTEMINFO` 建立的文本数表;缺它 ⇒ `@mTLine = 0` ⇒
  `(@mTNum)/@mTLine` **整数除零** ⇒ 主线终止(实测:帧 1 即 Fatal)。
- **引擎证据**:v494 处理体 `0x450478` 逐槽反汇编(旧线
  `yuris-vm/src/exec/sysinfo.rs` 记录):`SCRIPTFILE`=NO(1 基)→`yst_list` 文件名;
  `SCRIPTNUM`=脚本总数;`TEXTNUM`=NO → 文本数表;`EMOTE`=E-mote 运行时标志。
- **本实现**:四槽全实现(文本数见 DIV-RT-026)。**仍挂账**:手册缺页这一事实
  无法消除,若将来拿到手册补充或官方 trace 与之冲突,以官方为准。

## DIV-RT-024 · `VARACT[INIT + G_INT..G_STR4]`(手册无页)

- **手册**:`variable/varact.html` 的参数表**没有** `INIT` / `G_*`
  (只有 SET/LET/CUT/COPY/TYPE/UPPER../DIMSIZE/PUSH/POP)。
- **语料**:ERIS `es_SoundDefine.yst` 等处的初始化段出现
  `VARACT[INIT=1 G_INT=1]` / `VARACT[INIT=1 G_STR3=1]`(实测 s12 pc862/863)。
- **引擎证据(完整算法)**:旁线 §3.26 ④(处理体 `0x454d3d` 掩码构造 +
  `0x455b60` 恢复体,逐条反汇编):
  1. `INIT` 是守卫:不在场 ⇒ **整条指令什么都不做**;
  2. 17..28 各置一位掩码(INT/FLT/STR × 组 1..4);
  3. 恢复只收 `ysv` 里 `kind == 1`(全局);按 `(值类型, 组号)` 查掩码,位为 0 则否决;
     组号超出 1..=4 时没有否决项 ⇒ 照常恢复;
  4. 重建是「整槽按声明重建」(类型 + 维数 + 初值),不是逐元素赋值;
  5. 这一组**不读 `SET`**(所以 `VARACT` 的「缺 SET」检查必须让位于它)。
- **处置**:按上述算法实现。**仍挂账**:手册缺页(DIV 保留 open)。

## DIV-RT-025 · `TASKINFO` 数值字段

- **手册** `control/taskinfo.html`:EXIST/E/Z/FILE/LINE/IFNEST/GOSUBNEST/
  LOOPNEST/SCRIPTPOS。
- **取值来源(引擎任务记录,旁线 §3.2)**:`+0x20` pc、`+0x3c` 脚本号,
  加上本内核建模的三种栈深度;**LINE 用编译产物里的「数据区 B 逐 pc 源码位置」**
  (`ScriptFile::source_position`,0.495 实测 `part4_len == part1_len == 组数×4`)。
- **未实现**:`SCRIPTPOS`(存档锚点串,归 `yu-save`)、`TEXTPOS`。

## DIV-RT-026 · `yst_list` 的逐脚本正文数

- `yst_list.ybn` 每条记录 = `u32 index + u32 nameLen + name + 16 字节元数据 + u32 fileNo`。
- **实测**:16 字节元数据的**第 3 个 u32** 与脚本正文数一致:
  s71=222(88,861 字节)、s64=125(25,311)、s11=116(30,261)、s70=109(36,664)、
  s12=0(该脚本无正文)、s85=0。
- **处置**:解析进 `ScriptEntry::text_count`,`SYSTEMINFO[TEXTNUM]` 直接用它
  (引擎同源:处理体查 `0x802b2c[n-1]`)。

## DIV-RT-027 · IF/ELSE 的判定顺序

- **引擎 §3.13 opcode 45**:「**压块帧**(`base_a++`、出口 = `item[3].length`)
  → 求值 `item[1]`」—— **先压帧,再求值**。
- 本实现起初反过来,后果:条件求值一旦返回 `Unimplemented`(例如未取证的系统变量),
  块帧没压上,后续 `IFEND` 直接失配(实测 s10 pc87 `IFEND 无配对 IF`)。
- **处置**:按引擎顺序;并且**可恢复的条件求值失败按「假」处理 + 记诊断**,
  Fatal 才终止任务。

## 本轮端到端结论

```text
$ cargo run -p yu-rt --example run_eris -- <project-eris> 12
frame 0: cmds=1290322 errors=0
frame 1: cmds=329227  errors=0
frame 2: cmds=26      errors=0
frame 3: cmds=249     errors=0
frame 4..11: cmds=8765 errors=0   (稳态)
```

```text
$ cargo test -p yu-rt --test corpus eris_entry_chain_runs_end_to_end
E2E frames: [1290322, 329227, 26, 249, 8765, 8765, 8765, 8765]
E2E total commands: 1654884                                         ... ok

$ cargo test -p yu-rt --test corpus loose_and_packed_eris_are_execution_equivalent
loose  = ([1290322, 329227, 26, 249, 8765, 8765], 0, "cd1790…4bd2e")
packed = ([1290322, 329227, 26, 249, 8765, 8765], 0, "cd1790…4bd2e")  ... ok
```

**ERIS 主循环连续 12 帧零 Fatal**,且散盘与官方 `data.ypf` 的执行轨迹
(逐帧命令数 / 错误数 / 变量域终值摘要)**完全相同** —— A1 从 vfs 层推进到执行层。

---

## DIV-RT-028 · 文本流三件套(2026-10-19)

- **现象**:ERIS 源码 grep 不到 `WORD`/`RETURNCODE`/`_` 的直接使用,
  但编译语料 464/396/33 处高频出现。
- **取证**:一行剧本文本被 YSCom 编译为
  `WORD(前半)→[_($_TEXT(239)+$_TEXT(240))]×行内改行数→WORD(后半)→RETURNCODE(n)`;
  `RETURNCODE` 无参数声明(ysc/ycd 双表),唯一窗口 `kind` = **本脚本内文本行
  计数**(1 起逐行 +1;137 文件 396 处全量成立,回归
  `returncode_kinds_are_per_script_line_counters`)。手册 `basic/basic.html`
  ●テキスト表示:`_[式]` 显示式值;RxYuris `YSTB_TextEditor_V5.cpp` 以 WORD
  为文本提取单元(YSTL `text_count` == WORD 数,与本仓库解析互证)。
- **处置**:全部实现(`op_word`/`op_text_out`/`op_returncode`);
  `$_TEXT` 缓冲同步维护。

## DIV-RT-029 · `_CODE_*` 码值(2026-10-19,REA)

- **取证**(0.495 `yu-ris.exe` sha256 `4f5eefa3…`,Ghidra `FUN_00427350`):
  ①`_INP(n)`(case 0x90)读 VK 计数表 `DAT_00685800[n*8]`,`n<=0 → 0`;
  ②`_KEY_*`/`_MOUSE_*`(case 0x93..0x113)经字节表 `0x68996d[id]` 读同表;
  ③`_CODE_*`(case 0x19f..0x21e)= 字节表 `0x689861[id]`(BSS 运行期填充,
  静态不可读);④手柄段纯算术(case 0x114/0x118/0x21f/0x223):
  `384 + n*36 + 0..3`、`388 + n*36 + m%32`(每手柄 36 槽)。
  值层:手册不变量 `@_INP(@_CODE_Z) ≡ @_KEY_Z`(svariable ※3)⇒ `_CODE_*`
  = VK;日文符号键 = JP106 OEM 映射;与旧线独立逆向互证。
- **处置**:数据文件 `data/button-code-abi.tsv`(113 基名;鼠标 1..3 号变体
  同基码)+ 手柄算术;脚本可写(手册 ○ 键位重映射);
  回归 `inp_and_code_constants_share_the_engine_counter_table`。

## DIV-RT-030 · 手册检索的二进制误判(2026-10-19)

- **现象**:普通 grep 把 SDK 手册 html 判为 binary 跳过,导致
  `_WINDOW_EXEC_CLOSE`/`_MENUCODE` 等被误分类为「手册无页」。
- **处置**:全部系统变量名做字节级(`grep -a`)重审;63 项非族谱名确认
  手册无页(挂账);错误引用(如 `_TINT` 误引 svariable、`_KEYLAST`
  误引手册)全部勘误。

## DIV-RT-031 · 状态系统变量(2026-10-19)

- `$_TEXT`(内容;`WORD`/`_` 追加,写替换)、`○@_MENUCODE`(菜单域宿主
  经 `Vm::set_menu_code` 推入)、`○@_WINDOW_EXEC_MAX/CLOSE`(窗口域;
  `window_exec` 状态)——手册有页,内核持有状态,宿主可推入。

## DIV-RT-032 · `_TEXT_FLAG` 控制字节流(2026-10-19,2026-10-21 更新)

- **旧状态**:手册有页但逐字符标志定义未取证 ⇒ 读取/写入均挂号。
- **2026-10-21(M-2)取证**(0.495 `yu-ris.exe` sha256 `4f5eefa3…`):
  - 字符串系统变量取值 `0x446b93`:id `0x7a`(`_TEXT`)→ `[state+0x38]`,
    id `0x7b`(`_TEXT_FLAG`)→ `[state+0x3c]`;
  - `WORD` 处理体 `0x458d44`:文本字节进 `+0x38`,每字符一个标志进
    `+0x3c`;CP932 首字节(`0x5b40c0[byte]=1`)写 `'2'`,否则 `'1'`;
  - `_` 处理体 `0x4567c0` 同样写 `'1'`/`'2'`(0x456999/0x456a9e);
  - `TEXT` 处理体 `0x453448`:RETURN `'R'`、PAGE `'P'`、CLICKRETURN `'r'`、
    CLICKPAGE `'p'`、CLICK `'C'`;
  - `RETURNCODE` 处理体 `0x44c390`:自动模式按 AUTOCLICK/AUTORETURN/
    AUTOPAGE 追加 `C/R/P`,最后恒追加 `'E'`(0x45)。
- **已实现**:读取(120 次/600 帧的 LET 不再挂号)、`WORD`/`_`/`TEXT`/
  `RETURNCODE` 的写入点、`TEXTINFO` 查询。
- **仍挂账**:脚本**写入** `$_TEXT_FLAG`(语料 1 次)的语义未取证 ——
  引擎写路径未定位,且“写标志是否重建文本/光标”无证据。当前写入保持
  `Unimplemented` + 无副作用。
- **复核**:`cargo test -p yu-rt --test fixture text_flag_sysvar_reads_the_control_stream`、
  `text_flags_track_cp932_width_and_returncode_auto_defaults`。

## DIV-RT-033 · `_KEYLAST`/`_MOUSELAST`(2026-10-19)

- 手册无页(0.479/0.495 字节级核对);`InputState` 内部照常记录最近按下
  按钮码(按下沿,码 = 引擎按钮码),系统变量读取挂号。

---

## DIV-RT-034 · panic 收口(2026-10-19,任务书 E R7)

- **现象**(成果 16 审查清单):四条脚本可触发的硬崩溃路径——
  ①`IFBREAK[LV=0]`:`if_stack[len-0]` 越界 panic;
  ②GOSUB/RETURN 参数名 `PINT0`/`PINT17+`(工程 ysc 表异常):`args[idx-1]`
  usize 回绕/越界 panic;
  ③`VARACT[DIMSIZE=<巨型>]`:`Vec::resize` 分配失败 abort;
  ④数组声明维长乘积:乘法回绕。
- **处置**:①LV=0 与 LOOPBREAK 同口径 = no-op;②③范围检查/防御上限
  (2^26 元素,非引擎语义、内核自保)→ Fatal 诊断;④`checked_mul` + 上限。
- **回归**:`ifbreak_lv0_is_a_no_op_not_a_panic`、
  `gosub_param_name_out_of_range_is_fatal_not_a_panic`、
  `dimsize_huge_is_fatal_not_an_abort`(修复前 abort 复现于
  STATUS_STACK_BUFFER_OVERRUN,修复后为 Fatal Diag)。

---

## DIV-RT-035 · 文本让出点是 `RETURNCODE`

- **手册措辞**:`basic/task.html`「「ウェイト命令」か「表示テキスト」が来ると、
  タスクはそれを実行したあと、一旦休憩します(約0.016秒)」——字面上「表示文本」
  可被读成每个文本段(`WORD`/`_`)都让出。
- **真实脚本/编译形态**:一行剧本文本 = `WORD(段) → _[式] → WORD(段) → RETURNCODE(n)`
  (C3 已证,137 文件 396 处 RETURNCODE 逐行递增)。
- **引擎证据**(REA,0.495 `yu-ris.exe` sha256 `4f5eefa3…`):
  - `RETURNCODE` 处理体 `0x44c390..0x44c45b`:
    `0x44c451 MOV byte ptr [ESI+9],1`、`0x44c455 MOV EAX,0xffffffff` —— 置任务让出标志并返回非 0;
  - 泵 `FUN_00404398` `0x40442e`: `MOVSX ECX,[ESI+9]; JNZ next_task` —— 该任务本 vsync 不再泵;
  - `WORD` 处理体 `0x458ee0` 成功路径 `0x459054: XOR EAX,EAX; RET`;
  - `_` 处理体 `0x4567c0` 成功路径 `0x4569cc: XOR EAX,EAX; RET`。
  ⇒ **只有 `RETURNCODE` 让出**;一行内多个 `WORD`/`_` 在同一次泵内跑完。
- **恢复条件**:`FUN_004046fc` 在推进输入为正时清 `task+9`;推进按钮表为
  `0x5b48ac..0x5b48b8`(MOUSE_L=1 / ENTER=13 / PAD=0x3FF / PAD=0x3FF)与
  `0x5b48bc`(CTRL=17)。
- **处置**:`op_returncode` 返回 `StepOutcome::TextYield`;调度器置 `text_rest`,
  下一次逻辑帧若无推进输入则跳过该任务。宿主/测试用 `InputTimeline` 注入点击。
- **未取证**:`TEXT[WAIT=…]` 等文本命令对同一标志的影响(手册 `text/text.html`
  有 `WAIT`/`RETURN` 关键字,语义待取证)。
- **复核**:`cargo test -p yu-rt --test fixture one_text_line_per_frame_until_advance_input`。

---

## DIV-RT-036 · 主任务 ID = `SYSTEM`

- **手册**:`basic/task.html`「起動時に生成されるタスクをシステムタスクと呼び、
  「SYSTEM」というIDが名づけられています」。
- **引擎证据**(REA):启动链 `FUN_00465384` `0x4652f0: MOV EDX,0x807b00`("SYSTEM")
  → `CALL 0x414559`(按名查任务)→ `[EAX+0x34]` 写脚本号/pc;任务以 `+0x28=0x64`(Z=100)
  插入有序链表(`FUN_004621d0` @0x4623f4)。
- **处置**:`MAIN_TASK_ID = "SYSTEM"`;`TaskSeed::main` 的 Z = 100。
  `TASK`/`TASKEND`/`TASKINFO` 无 ID 时目标 = **执行者自身**(手册;REA 0x45074a
  `MOV EBP,[0x801720]` 取当前任务记录),不再是主线的硬编码别名。
- **复核**:`cargo test -p yu-rt --test fixture main_task_id_is_system_and_script_visible`、
  `taskend_without_id_ends_only_self_and_main_continues`。

---

## DIV-RT-037 · `TASKEND` 立即摘链

- **手册**:`control/taskend.html`「タスクを終了します」。
- **引擎证据**(REA `0x4521dc`):
  - 无 `ID`:`MOV EAX,[0x801720]`(当前任务)→ `CALL 0x452364`(从双向链表解链,
    `DEC [pool+0x20]`)→ `MOV EAX,0xffffffff`(返回非 0 = 让出 ⇒ 同一条命令之后的
    指令不执行);
  - 指定 `ID`:查名字表 `0x414559`;命中其他任务 → 摘链 + 返回 0(继续);
    命中自身 → 返回 -1。
- **处置**:自身目标返回 `StepOutcome::Finish`(执行位丢弃、节点帧尾移除、
  发 `TaskFinish`);他人目标投 `Effect::FinishTask`,调度器立即摘链并回调
  `TaskExecutor::task_removed`(丢弃执行位 + 落事件)。`TASKINFO EXIST` 立即为 0。
- **复核**:`cargo test -p yu-rt --test fixture taskend_stops_neighbor_commands_and_rebuild_uses_new_entry`。

---

## DIV-RT-038 · Z 优先级与同 Z 次序

- **手册**:`basic/task.html`「その優先度の高い順から実行されます」「優先度の値が
  同じだった場合の実行順序は不確定となります」;系统任务默认 100。
- **引擎证据**(REA):
  - 插入 `FUN_004613c0`:以 `+0x28/+0x2c` 组成 64 位键,`jl` 前插(
    即降序);等键时走到链表尾(插在既有同键节点之后);
  - 重配置走 `FUN_0042f7bc`:先 `0x452364` 摘链,再按新 Z 重新插入。
- **处置**:`Scheduler::nodes` 为按 `(priority desc, seq asc)` 排序的有序表;
  `seq` = 创建序号。手册称同 Z 不確定,这里取引擎实际行为(创建序)并钉死,
  以保证 A3 确定性。
- **复核**:`cargo test -p yu-rt --test fixture higher_z_task_runs_before_lower_z`、
  `cargo test -p yu-rt --lib task::tests::higher_priority_runs_first_and_equal_priority_keeps_creation_order`。

---

## DIV-RT-039 · GOSUB 嵌套 64 次

- **手册**:`control/gosub.html`「GOSUBのネストは64回まで可能です」。
- **引擎证据**(REA `FUN_004459c4`):`0x4459e6 MOVSX ESI,[task+0x140]`;
  `0x4459ed LEA EAX,[ESI+1]`;`0x4459f0 CMP EAX,0x40; JE 报错`。
  即新深度等于 64 才报错 ⇒ 允许 64 次调用(深度字段从 0 起,基帧不计)。
- **处置**:内核 `frames` = 基帧 + 调用帧;允许 `frames.len() <= 64` 时压栈,
  第 65 次调用 Fatal(`rt.nesting`)。旧实现用 `>= 64` 判基帧在内 ⇒ 实际只允许 63 次。
- **复核**:`cargo test -p yu-rt --test fixture gosub_allows_64_nested_calls`、
  `gosub_65_nested_calls_is_fatal`。

---

## DIV-RT-040 · 帧边界事件与有界事件队列

- **性质**:内部可观测契约(`docs/architecture.md` §3/A3),**不是**官方语义;
  官方引擎没有对应物,故不进 golden 之外的任何语义断言。
- **处置**:每逻辑帧发 `FrameStart` / `FrameEnd`;事件 Vec 默认上限
  `DEFAULT_EVENT_CAPACITY`(1<<20),超出丢最旧并计数 `events_dropped()`;
  新增 `drain_events(&mut Vec)` 供长跑宿主消费。`record_events=false` 行为不变。
- **复核**:`cargo test -p yu-rt --test fixture frame_events_are_bounded_by_start_and_end`、
  `event_queue_is_bounded_and_reports_drops`。

---

## DIV-RT-041 · `Diag` 的脚本定位

- **契约**:`architecture.md` §8「每个 Fatal 报告自带『哪条边界、哪个表、哪个脚本位置』」。
- **处置**:`Vm` 在每步记录 `cur_script/cur_pc/cur_cmd/cur_task`;`report()` 与
  `step_command` 错误路径在 `where_ == Loc::None` 时补
  `Loc::Script { file, label: Some(task_id), pc, cmd }`。处理器内部的
  `Unimplemented` 也计入 `unimplemented_cmds` 挂号表。
- **未取证**:引擎自己的错误文案/位置格式(不可比);此处只保证内核可用性。
- **复核**:`cargo test -p yu-rt --test fixture script_diag_carries_task_script_and_pc`。

---

## DIV-RT-042 · `DomainCall` 的变量引用身份丢失(2026-10-21,M-2)

- **事实**:`SAVE[SET=@X]`、`LOAD[LET=@Y]`、`FILEINFO[LET=@Z]`、
  `SOUNDINFO[LET=@W]` 在引擎里把 `SET`/`LET` 当**变量引用**执行;yu-rt 的
  `eval_arg_windows` 对所有窗口调用 `eval_value`,事件里只剩**求值结果**。
- **影响**:宿主能按参数名知道“要保存/写回哪个槽位语义”,但拿不到目标变量;
  把 `SET=2` 当字面量会错误地写坏存档。
- **为什么不本轮修**:给 `ArgValue` 加引用字段会破坏既有结构体字面量;
  新增事件变体会让 `yu-trace` 的穷举匹配编译失败(该 crate 故意没有 `_` 分支)。
  两者都是跨 crate 契约变更,超出任务书 M 的独占范围。
- **缓解**:`VmEvent::domain_args(&yscm)` 提供数据驱动的参数名视图;
  `docs/domain-event-contracts.md` §6 明确标注“事件是事实记录,不是可重放命令”。
- **复核**:`cargo test -p yu-rt --test fixture domain_args_map_slots_through_project_yscm`。
