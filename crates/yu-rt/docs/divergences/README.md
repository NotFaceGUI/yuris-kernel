# DIV-RT 系列 · yu-rt 与官方引擎的已知差异与挂账

> 制度:`docs/architecture.md` §8、任务书 C §1。每条差异/未决都必须留档,
> 含:现象、证据、处置(已实现/`Unimplemented`)、复核方式。
> 状态取值:`open`(待取证)、`fixed`(已修正)、`wontobserve`(官方不可观测)、
> `blocked`(缺样本/缺工具)。
>
> 状态对账:2026-10-23 全表复审一轮(P3/P4 收口项落账;证据见各条目
> 2026-10-23 更新行与 `PROGRESS.md` 成果 P3/P4/P6)。

| 编号 | 主题 | 状态 | 影响 |
|---|---|---|---|
| DIV-RT-001 | 引擎 0.479/0.495 的 opcode 编号整体错位 | fixed | 运行核从不内置序号表;真实 0.495 语料 corpus 实测通过 |
| DIV-RT-002 | 表达式类型提升/溢出/字符串↔数值细节 | open | 已按确定性规则实现,逐位未对齐 |
| DIV-RT-003 | 整数除零/取模 0 | open | 引擎为硬件异常;内核 Fatal。P7 续已修现语料的真实根因(`op_math` 丢弃 LET 下标 ⇒ POW2 表全 0),S2/S3d/S4/S7 的 Fatal 全部消失;「官方 Fatal 策略」本身仍待 trace |
| DIV-RT-004 | 「实参缺席」的运算符缺右操作数形态 | fixed | 语料 20 窗口统计(19 掩码 + 1 真缺席);`expr.rs` 容忍一次;语料全量跑通 |
| DIV-RT-005 | 脚本执行错误后的引擎继续策略 | open | 默认槽路径已取证(DIV-RT-043:报一次+让出,任务不终止);内核现行 = Unimplemented/Recoverable 挂号后继续、Fatal 结束+记录;官方其余错误路径仍无 trace |
| DIV-RT-006 | 输入键码 `_CODE_*` / `_INP` / `_PAD` 的码值 | fixed | REA 证结构(VK 计数表)+ 手册不变量定值;`_KEYLAST/_MOUSELAST` 已另证(DIV-RT-033) |
| DIV-RT-007 | 工程 ycd 声明但手册无页的系统变量 | open | `_WINDOW_EXEC_CAPTION` 已收口(DIV-RT-047);**`_CHR` 已收口(2026-10-25,fixed(no-manual))**:按语料用法+REA `0x446b93` 取值链实现为「单字节构造」——`$_CHR(n)` 返回字节 `n` 的 1 字节字符串(非码位再编码);证据:es_Text.yst:878 用 `$_CHR(0xEF)+$_CHR(0xF3)` 匹配文本流控制码 `C`,而编译器把 `
` 展开成 `$_TEXT(239)+$_TEXT(240)`(原始字节 0xEF 0xF0),按 Unicode 再编码恒不匹配(桌面会话实测 46,696 次挂号清零)。`_DXMODE`/`_VARTYPE`/`_SYSTEM_FRAME` 等仍 `Unimplemented` |
| DIV-RT-008 | `_`(文本输出)手册无页 | fixed | 手册 basic.html ●テキスト表示有页(`_[式]`);已实现,撤销旧挂账 |
| DIV-RT-009 | `_TIME`/`_OS_TIME` 的时区与纪元 | fixed/partial | 真实运行宿主注入本地时间基准(`_TIME` 全字段,存档现实日期);`_OS_TIME(0)` 与跨 DST 漂移仍 open |
| DIV-RT-010 | 路径类系统变量(`_SAVE_PATH` 等) | fixed | `BootConfig::paths` + yu-cli `host_paths()` 注入;`$_PROJECT_PATH` 手册无页保持 `None` |
| DIV-RT-051 | 字符串操作按 CP932 字符边界(非逐字节) | **fixed(2026-10-24)** | 根因:CP932 尾字节覆盖 `0x40..0xFC`,其中 `0x41..0x5A`/`0x61..0x7A` 与 ASCII 字母同码 ⇒ 逐字节 ASCII 化会改写双字节字符。实测 `VARACT[LOWER]("サークル")` = `83 54 81 5b 83 4e 83 8b` → `83 74 81 5b 83 6e 83 8b` = `フーハル`,厂商 LOGO `cg/ev/サークル.jpg` 因此查不到、片头黑屏。修法:新增 `crates/yu-rt/src/cp932_text.rs` 作唯一真源(`is_lead`/`char_len_at`/`spans`/`char_count`/`slice_chars`);`ascii_case`/`fullwidth_case`/`copy_chars`/`cut_chars`/`case_tables::char_len` 全部改走它。复核 `cargo test -p yu-rt --lib`(70 passed,含 4 个 cp932_text 单测) |
| DIV-RT-053 | `VARINFO` 的针值槽 / `NO=` 槽 / 查询项按写死槽号分派 | **fixed(2026-10-25)** | 根因:`VARINFO` 的参数表随版本新增条目**整体位移** —— SDK 0.495 是 `INT=17/FLT=18/STR=19/NO=20`,商业包 v494 是 `INT=15/FLT=16/STR=17/NO=18`。旧实现把后者的三个数字写死在 `array_needle` 与 `flags_get(&flags, 20)` 里,查询项分派也是 `match slot { 2=>TYPE, 4=>DIMNUM, 5..=12=>DIMSIZE, 13=>LENGTH, 14=>SEARCH }`、筛选用 `!matches!(slot, 17..=20)` ⇒ 在 v494 上 `SEARCH` 取不到针值、恒返回 -1,`TYPE`/`DIMNUM`/`DIMSIZE` 也会落到错误分支:`es.LNO.GET("BGF")` 明明存在(`$3153[4]`)却判「未找到」,`es.SP.SET.SET`→`es.SP.GO.SET` 后续的 `LET @3394[...]=1` 从不执行,`es.SP.DRAW` 一次都不调用 ⇒ sprite 全不上屏。修法:新增 `varinfo_no_slot()`,`array_needle` 与全部分派改按**参数名**(`TYPE`/`DIMNUM`/`DIMSIZEn`/`LENGTH`/`SEARCH`,针值 `INT`/`FLT`/`STR`,起始 `NO`)从工程 ysc 反查槽号;`VARACT`/`TASK`/`TASKINFO` 的同类写死槽号一并改按名解析。证据:官方 v494 事件级 trace(分派点 `0x44cb`/`0x4537`,CG handler `0x42a4a8`),同帧 pc 序列与内核逐条对拍。复核 `fixture::varinfo_search_resolves_slots_by_param_name` 与 `fixture::varinfo_query_dispatch_follows_param_names`(把参数表整体前移,断言五个查询项仍正确)。台账细节见 `docs/divergences/DIV-GFX-046-sprite-bg-hold.md` §K |
| DIV-RT-054 | `CGACT[SIZE/SIZE2]` 的 `SX/SY/X/Y/X2/Y2` 被按整数读取 | **fixed(2026-10-25)** | 根因:`cgact_image` 用 `find_int` 取这六个参数,而 `EngineValue::as_int` 要求 `fract()==0`。`es_Sprite.yst` 的 `es.CGACT_SIZE` 传的是**缩放后的实际像素范围**(实测 `CGACT[ID=SPLBUF401 ID2=SPL401 SIZE=1 X=0 Y=0 X2=1100 Y2=824.5 SX=1101 SY=825.5]`),`.5` 是正常值。旧实现把 `Y2`/`SY` 静默判成「未给」⇒ 区域只剩 3 字段 (`gfx.imageop.region.partial`)+ 尺寸缺失(`gfx.imageop.size.missing`)⇒ 整条 SIZE 被跳过 ⇒ sprite 层永远拿不到像素。修法:`SX/SY` 走 `find_f64` 后取整、`X/Y/X2/Y2` 走新增的 `find_px`(浮点四舍五入)、`ImageOp.sx/sy` 与 JSON 改浮点保值;soft/wgpu 后端同步。实测 `ops 83/83 drawn, 0 skipped`(此前 77/79,2 skipped),片头 LOGO 上屏。台账细节见 `docs/divergences/DIV-GFX-046-sprite-bg-hold.md` §L |
| DIV-RT-055 | YPF 封包版本上限 `495` 是硬编码闸门 | **fixed(2026-10-25)** | 根因:`MAX_ACCEPTED_VERSION = 495`,高于此值直接 `Unimplemented` 拒启。但 REA 证明索引装载器 `FUN_0045e3b8` **逐字节无版本分支**(起点 `0x20`、步长 `nameLen+0x1B`、长度码表 `T[0xFF-lenEnc]`、名字取反全为常量),`version` 只读入不参与分支;项目自己的 `equivalence.rs` 也早有 「version=300 必须能读」用例。真实商业作品存在 YPF v500(`<game-a>`),旧闸门会直接拒掉。修法:去掉上限,改名为 `VERIFIED_MAX_VERSION` 并注明「仅信息,非门禁」,把关交给后续结构校验(entry_count 上限 / 索引容量 / `detect_layout` 方言探测 / 名字 MurmurHash)。下限 234 保留(早期布局无样本)。复核 `equivalence::engine_layout_with_version_500_is_readable` |
| DIV-RT-056 | `FILEINFO[SIZE/RELATION]` 未实现 | **fixed(no-manual)(2026-10-25)** | REA `0x44286c`:SIZE 分支 `0x443572` = `FindFirstFileA` → 目录 0 / 文件 `nFileSizeLow`(32 位)/ 找不到 0;RELATION 分支 `0x4429e6→0x442a20` = `FindExecutableA(NULL,FILE,buf)` → 关联 exe 全路径写**字符串** LET。手册 `file/fileinfo.html` 例 3/4 有页。实现:SIZE 走宿主文件系统(`$_SAVE_PATH` 前缀解析,封包内文件与官方一致查不到=0);RELATION 内核恒空串(B7 接 `FindExecutableW` 前的宿主能力位;`FindExecutableA` 失败同样得空串,脚本判空安全)。FOLDER 查询保持挂号(手册注明封包内文件夹不可查;语料 0 次)。复核:全量用例 9/9、fixture 79 | 
| DIV-RT-057 | Save/File 域事件在帧末才应用(`FILEACT[DELETE]` 晚于同帧 `SAVE[FILE]` 刷盘) | **fixed(2026-10-27)** | 根因:`Vm::emit` 只对 Graphics/Window/Sound 同步 `apply_event`,Save/File 仅入队,由 `Engine::tick` 帧末 `apply_events`;`es_SaveLoad.yst` 的存档流程是「pc315 先删旧槽 → pc341 `SAVE[FILE]` 同步刷盘」,延迟的删除落在刷盘之后,刚写出的存档被删,`SAVEEND` 再清预约 ⇒ 无文件、无报错。修法:统一为命令时刻同步(`Vm::emit` 对 `Domain::Save|File` 也调用 `apply_event`,Engine 帧末不再重复喂),与官方 `FILEACT` 就地执行一致。复核 `fixture::fileact_is_applied_before_save_file_flush_in_same_frame`;桌面复现 `save30002.sd` 25608 字节保留 |
| DIV-RT-011 | 手册有页但语义未取证的命令(VARINFO SEARCH / VARACT INIT·G_* / F_* / ERROR) | open | 仅剩 VARINFO `STRTYPE/SJISCODE/UTF8CODE`(无手册例)、`VAR`/`CSVINFO`(无页/未取证,AU5 补录)与 `CDDA`(空桩,见 sound-ledger N0-10);SEARCH/INIT·G_*/RETURNCODE/WORD/F_*/PREP 已另证实现 |
| DIV-RT-012 | `SAVE`/`LOAD` 的盘上存档格式 | fixed | `yu-format::yssd` 官方 5 文件 269 块逐字节 roundtrip;`yu-save` 宿主消费域事件(忠实写盘受 DIV-RT-042 门禁) |
| DIV-RT-013 | `HANTOZEN`/`ZENTOHAN` 转换表 | fixed | 三张表已自 0.495 exe 取出落 `data/case-tables-0495.tsv`;算法逐条对照 REA |
| DIV-RT-014 | `MATH[RND]` 的 MT19937 种子来源 | fixed | 种子 = `GetTickCount`(REA `0x461f98`),内核 `BootConfig::rng_seed` 注入(缺省 5489);`rng.rs` MT19937 |
| DIV-RT-015 | `TASK` 的局部参数 | **fixed(no-manual)** | TINT/TFLT/TSTR/SCRIPTPOS 已按官方源码生产者/消费者对实现(带回归);**RESTART 已收口(2026-10-25)**:REA `0x450c46` 读槽 37 存在标志、`0x450c55 mov dword[0x801944],1` = 本 vsync 再跑一逻辑帧(内核 `Effect::RequestExtraFrame`);**TEXTPOS 已对齐(2026-10-26)**:0.495 TASK 处理体 `0x4506f4` 无槽 36 分支 ⇒ 官方忽略(no-op,代码不再报 Unimplemented);**A 已实现(2026-10-26)**:调度器 `0x4043dc..0x4043e6` 以 `A & E` 为步进闸门、`TASK` `0x450bbf` 写 `task+0x15`、构造器默认 1、`TASKINFO[A]` `0x4527b0` 读;手册无页 |
| DIV-RT-016 | 商业发行版不带 `YSCom.ycd` | fixed | 改 ycd 可选 + 引擎 ABI 表 + ysv 自证校验 |
| DIV-RT-017 | 变量空间前缀 `0x60` / `0x23` | fixed | = ERIS 旧式 `@G/@L`(0x60 数值)、`$G/$L`(0x23 字符串);源码↔编译产物↔YSVR 声明三方对上,`canonical_var` 归一化;其它前缀仍挂号 |
| DIV-RT-018 | `ELSE` 的 0 窗口形态 | fixed | 商业包 726 次;无条件进 else 体 |
| DIV-RT-019 | `VARACT[DIMSIZE/PUSH/POP]` 无 `LET` | fixed | 手册例 9/10 即此形态 |
| DIV-RT-020 | 数组负下标 / 越界访问 | fixed | 根因是 lvalue 解析 + 未实现的 VARINFO/VARACT |
| DIV-RT-021 | 输入系统变量可写(`@_MOUSE_L=5` 等) | fixed | 写入 = 设定帧计数器 |
| DIV-RT-022 | 系统变量的宿主注入面(色深/路径/DX/媒体只读/声音) | fixed | `BootConfig` 注入 |
| DIV-RT-023 | `SYSTEMINFO` 手册无页但启动链硬依赖 | fixed | 四槽全实现 + 引擎处理体 `0x450478` 反汇编证据;手册缺页为不可修的外部事实,保留在条目内 |
| DIV-RT-024 | `VARACT[INIT + G_*]` 手册无页 | fixed | 按引擎 §3.26 ④ 完整算法实现(掩码 `0x454d3d` + 恢复体 `0x455b60`);手册缺页为不可修的外部事实,保留在条目内 |
| DIV-RT-025 | `TASKINFO` 的数值/位置字段 | fixed(no-manual) | 2026-10-26:FILE0/FILENO0/LINE0/TEXTLINE/NEXTVOICE 按 0.495 `0x4523fc` 逐分支实现;A 槽(4)按 `task+0x15`/`A&E` 闸门实现;FILE 返回 `yst_list` 路径;GOLABEL/GOSUBLABEL/TEXTPOS 官方无分支(对齐 no-op);FILENO0→SCRIPTPOS fallthrough、完整 u16 文本行、类型诊断去重、SCRIPTPOS 引擎字节格式(`0x452a54`/`0x450fe0`,IF/GOSUB/LOOP 三栈 + text_line/pc/脚本名)均已收口 |
| DIV-RT-026 | `yst_list` 每脚本的文本数 | fixed | 记录第 3 个 u32 |
| DIV-RT-027 | `ELSE` 判定顺序 / IF 先压帧 | fixed | 引擎 §3.13 顺序 |
| DIV-RT-028 | 文本流三件套(WORD/`_`/RETURNCODE) | fixed | YSCom 合成;源码↔编译产物↔手册+RxYuris 三方证 |
| DIV-RT-029 | `_CODE_*` 的码值来源 | fixed | 引擎 BSS 字节表(REA)+ VK 规范;数据文件 button-code-abi.tsv |
| DIV-RT-030 | 手册检索的二进制误判 | fixed | 手册 html 含非文本字节,普通 grep 误报"无页";字节级 grep 重审全表 |
| DIV-RT-031 | `_TEXT`/`_MENUCODE`/`_WINDOW_EXEC_*` 状态系统变量 | fixed | 手册有页;内核状态 + 宿主 setter |
| DIV-RT-032 | `_TEXT_FLAG` 的控制字节流 | fixed | 读取、`WORD`/`_`/`TEXT`/`RETURNCODE` 写入、脚本写入(RHS 逐字节替换,REA `0x446b98..`)全部按 REA 实现 |
| DIV-RT-033 | `_KEYLAST`/`_MOUSELAST` 读取 | fixed | 手册无页,但 REA `FUN_00427350` case 0x91/0x92 直接读 `DAT_00801908`/`DAT_0080190c`;写入点 `FUN_0045a3b4`(WM_KEYDOWN / WM_LBUTTONDOWN=1 / R=2 / M=4) |
| DIV-RT-034 | panic 收口(任务书 E R7) | fixed | IFBREAK LV=0 / GOSUB·RETURN 参数名越界 / DIMSIZE·维长乘积防御上限 |
| DIV-RT-035 | 文本让出点是 `RETURNCODE`,不是 `WORD`/`_` | fixed | 任务书 E R3;REA 实证(见下) |
| DIV-RT-036 | 主任务 ID = 脚本可见 `SYSTEM` | fixed | 手册 + 引擎启动链均按名称 `SYSTEM` 建任务 |
| DIV-RT-037 | `TASKEND` 立即摘链(不等到帧尾) | fixed | REA 0x4521dc/0x452364;`EXIST` 立即为 0 |
| DIV-RT-038 | 任务链按 Z 降序;同 Z 按创建序 | fixed | 手册 Z 语义 + REA 有序插入 0x4613c0;同 Z 顺序手册称「不確定」,此处取引擎实际插入序 |
| DIV-RT-039 | GOSUB 允许 64 次嵌套 | fixed | 手册 64 回 + REA 0x4459e6(`depth+1 == 0x40` 才报错) |
| DIV-RT-040 | 帧边界事件(`FrameStart`/`FrameEnd`)与有界事件队列 | fixed | 内部可观测契约(architecture §3/A3),非官方语义 |
| DIV-RT-041 | `Diag` 的脚本定位(task/script/pc/cmd) | fixed | architecture §8;`Loc::Script.label` 承载任务 ID |
| DIV-RT-042 | `DomainCall` 的变量引用身份丢失 | fixed(B1) | `ArgValue.reference`(裁 1-A)+ `SaveHost` 同步契约(裁 2-A);SAVE 预约即值快照/LOAD 按 var_id 写回,`save-load-loop` 用例闭环 |
| DIV-RT-043 | 官方默认槽 `0x459e90` 的语义(脚本执行错误) | fixed | `F_*`/`G_*`/`MACRO`/`S_VAR`/`_IF*`/`);`/`SYSTEMMODE` 等不再静默 Unimplemented |
| DIV-RT-044 | `DIALOG` 的 VM 侧契约与宿主回答 | fixed | 任务挂起/回答、LET/LETSTR 写回、扩展参数挂账 |
| DIV-RT-045 | `TRAY`/`CLIPACT`/`CLIPINFO`/`REGINFO`/`YMV*` 的域边界 | fixed/partial | 剪贴板镜像实现;托盘/媒体归宿主;注册表挂账 |
| DIV-RT-046 | `ERROR` 的逐命令启用表 | open | 表已按 REA 写入,消费者未定位 |
| DIV-RT-047 | `_WINDOW_INIT_POS` / `_WINDOW_EXEC_CAPTION` 的宿主注入面 | fixed | 两个宿主 setter/注入值;手册仍无页 |
| DIV-RT-048 | `_TEXT_NUM` 的数据源 | fixed | `yst.ybn` 头部 text_total == `yst_list` 求和(ERIS 396) |
| DIV-RT-049 | 旗标/目标形态判定族(MATH 捕获、INIT 守卫、LABELINFO 下标)——P7 续审查轮的三个同族潜伏点 | fixed | 旁线 §3.20/§3.26 反汇编判据 + 语料形态扫描(scanmath:21 处 MATH 旗标全 =1、无 INIT=0);回归 ×3 |
| DIV-RT-050 | CGS 场景截图栈:进菜单/存档弹窗背景透出上一亮帧,官方为黑底 | open | 档案在仓库根 `docs/divergences/DIV-RT-050-cgs-scene-screenshot-stack.md`(跨 crate 编号,AU5 起在此交叉引用);候选根因含 WINDOWINFO 字段族缺口 |
| DIV-RT-052 | `END` 由**非 main 任务**执行时只 Finish 该任务,主任务继续 ⇒ 程序不退出(`\es.END` → `END[RESTART=0]` 后 `main_finished` 不亮) | open | B2 实测(`yu-cli tests/dialog.rs`):はい 后 `\es.END`→`GSD.SAVE` 落盘(执行链通),但 `END`(REA `0x441334`)语义待取证——官方是「结束整个程序」还是「结束当前任务」;影响所有退出确认流程。归 B10 rt campaign;修复前宿主以 `main_finished`/`VmEvent::End` 判退出需自行取或 |

---

## DIV-RT-001 · opcode 编号跨版本错位

- **现象**:0.495 比 0.479 多 15 条命令(`EMOTE`/`EMOTEINFO`/`F_*`/`G_*2..4`/
  `S_*2..4`),插在字母序中间 ⇒ index 12 之后整体错位。
- **证据**:`<side-repo>/docs/format-vm-opcodes.md` §3.17「版本差异」;
  本仓库实测 0.495 `ysc.ybn` 119 条与工程 `YSCom.ycd` 119 条完全同序。
- **处置**:运行核**从不使用内置序号表**;`opcode → 名字` 一律查工程 `ysc.ybn`
  (`crate::tables::YscmTable`)。跨版本 = 换数据,不换代码(A5)。
- **复核**:`cargo test -p yu-rt --test corpus` 对真实 0.495 语料逐组反查名字。
- **2026-10-23 收口**:实现即处置本体(跨版本 = 换数据不换代码,已落地);
  `cargo test -p yu-rt --test corpus -- eris_entry_chain_runs_end_to_end`
  对真实 0.495 语料 **ok**(17.75s,本机实测)。状态改 `fixed`。

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
- **2026-10-23 附记(P7 续 3 审查轮)**:浮点除法除零曾返回 `0.0`
  (`value.rs` 旧 `(Some(_), Some(0.0)) => Float(0.0)` 分支)——x87 除零
  异常默认掩蔽,IEEE-754 结果 = **±Inf(符号随被除数)**、`0/0 = NaN`;
  已按 IEEE 勘误并加 `float_div_zero_follows_ieee754` 单测。取模
  `fmod(x,0)=NaN` 原本即对。整数路径(除零 Fatal)不变。

---

## DIV-RT-003 · 整数除零 / 取模 0

- **现象**:引擎整数除法走 CRT/x87 路径,除 0 是**硬件异常**(旧线观测为
  进程级崩溃;见 `<repo-old>` PROGRESS 成果 133 的实测记录)。
- **处置**:内核返回 `Diag{code:"rt.expr.div_zero", severity:Fatal}`,**不静默
  返回 0**(静默会让脚本走错分支且无法审计)。
- **代价**:一条除零会终结该任务(策略见 DIV-RT-005)。若官方 trace 证明
  引擎在某处容忍除零,须改回并留勘误。
- **2026-10-23 取证(性能会话;未改语义)**:
  - 现场:`es_select.yst:153 LET[@2279[@2320]=(@2264[@2284]/@1118[@48])&1]`,
    `@48 = @_LC`,`@1118 = G_INT int[63]`。同一位置另有
    `es_number.yst:SYSTEM@56` 的除零(症状台账 S4)。
  - 写入者扫描:`find_var_ref <project-eris>/save 1118 0x40`
    (工具 `crates/yu-rt/examples/find_var_ref.rs`):
    ```text
    # total=19
    # var @1118 ty=Int dims=[63]
    #   [0] = 0 … [7] = 0
    kind=REF ×19(0x56/0x48 读操作数;无 0x76 / 写编码命中)
    ```
    ⇒ 表达式层面**没有** `@1118` 的写入者;启动后 63 个元素全为 0。
  - 残余可能(下一步取证,按优先级):
    1. **批量恢复路径**:`LOAD[FILE=… G_INT*]` / `SAVE` 的组恢复不经过
       表达式写操作码。需在 `LOAD` 的 G_INT 组写回处加 watchpoint
       (`VariableStore::write` 已有 `cur_script/cur_pc` 上下文),确认
       `@1118` 是否属于被恢复的组、以及 `gsd.sd`/槽文件里该数组的值。
    2. **`VARACT[INIT=1 G_INT3=1]`**(`es_GSD.yst:1258`)会把组 3 恢复成
       ysv 初值;本次投影的 ysv 初值为 0。需核对官方 `ysv.ybn` 里
       `@1118` 的 `init` 是否也为 0(若官方数据非 0,则是我们的 ysv
       投影/组号映射问题)。
    3. `@_LC` 在该 LET 前是否可能 ≥63(数组越界读);当前内核越界是 Fatal,
       不是 0 除数,故不是本现场的直接原因。
  - **禁止**:把 Fatal 改成“跳过该 LET”或伪造非 0 除数;必须等
    `LOAD`/ysv 两条取证路径有结论后再动。
- **2026-10-23 续取证(三条残余线两条已排除)**:
  1. **官方 `ysv.ybn`**:`dump_ysv_one 1118` → `id=1118 kind=1 group=1
     script=0 type=1 dims=[63] init=int 0` ⇒ 假设 2 排除(官方初值就是 0,
     不存在投影/组号映射错误;且它在 int 组 1,不在 `es_GSD.yst:1258`
     恢复的组 3)。
  2. **官方存档**:新工具 `yu-format/examples/dump_yssd_var.rs` 扫官方
     5 个 `.sd` 共 269 块,**var 1118 零命中**(gsd.sd 只带 1113/1112,
     save50001.sd 无 1118)⇒ 假设 1 的「存档里带非 0 值」排除。
  3. **语料全量引用**:`find_var_ref` 19 处全部为读操作数(无 0x76 写编码);
     读取形态 = `@1118[@_LC]` / `@1118[@_LC-1]`(`_LC`=LOOP 轮计数),
     常与 `@1119[@_LC-1]`、守卫 `@1113[91]` 并现,语义上像 2 的幂/位掩码表。
  - **新结论**:除数在内核与官方数据中都恒为 0 ⇒ 官方引擎不可能在这行
    成功执行「除以 0」(硬件异常会崩进程,与商业发行可玩矛盾)。故官方
    流程要么不执行该行(上游分支分歧),要么执行时 `_LC` ≥ 63 先触发
    数组越界 `0x1a202` 并按脚本执行错误路径吞掉(DIV-RT-043:报一次 +
    让出 + 继续)。本 Fatal 是**上游状态分歧的绊线**:内核在 `_LC < 63`
    的状态下成功读到 0 并做了除法。
  - **下一步**(替代原残余线):①采集官方 trace 在 `es_select.yst:153`
    的 `_LC` 值与是否执行;②内核 trace 里记录 Fatal 时 `_LC` 与该 LOOP 的
    轮数,对比上游哪一步分叉。
  - **现场补充(用户桌面实跑)**:`engine_frame=508` 的 Fatal 诊断显示
    **被除数与除数同为 0**(`@2264[@2284]=0 / @1118[@_LC]=0`)⇒ 该表达式
    是对全 0 状态做位提取,连被除数数组也未写入;进一步支持「上游分歧
    绊线」结论。内核侧证据已闭合,只剩官方 trace 一条路(任务归属
    P4/P3 取证)。
- **2026-10-23 收口(根因修复;推翻「上游分歧」假设 —— 勘误留痕)**:
  明文源码(`save/data/script/ERIS/`)定位:
  - 除数 `@1118` = **`@es.POW2`**(`global.yst:208` `G_INT[@es.POW2(63)]`,
    2 的幂表);写入者 = `es_First.yst:145`
    `LOOP[SET=63] MATH[POW=1 SET=2 SET2=@_LC-1 LET=@es.POW2(@_LC-1)] LOOPEND[]`
    —— 走 MATH 命令 LET 窗口,故表达式级扫描无写者;
  - **内核 bug**:`op_math` 把 LET 目标的数组下标丢弃,63 次写全落 `[0]`
    (实测 tick3 后 `POW2[0]=2^62`、其余全 0)⇒ 官方表填得对、除法正常,
    「官方不可能成功执行该除法」的前提不成立;
  - 修复:`op_math` 保留 `LValue.indices` 写回;表实证填满
    `[1,2,4,…,2^62]`;回归 `fixture::math_let_writes_indexed_array_element`;
  - 被除数 `@2264` = `@es.SELD`(選択履歴,经 `SAVE/LOAD seld.sd` 持久化,
    官方 `seld.sd` 块值 `[1]=6`;工具 `yu-format/examples/dump_yssd_var.rs`);
  - 同判 S4 的 `es_number.yst:56` 除零:同为 MATH 填表消费行,随本修复一并
    恢复(待用户桌面复验)。取证工具:`yu-rt/examples/dump_var_int.rs`、
    `dump_label_cmds.rs`(标签反汇编 / `scan:<sid>` 全脚本命令扫描)。

---

## DIV-RT-004 · 「实参缺席」形态

- **现象**:语料中 20 个窗口以 `42 01 00 00 41 00 00` 结尾(`PushInt(0)` +
  一个缺右操作数的 `&`),其中 19 个是完整的 `X & 0xff` 掩码,**只有 1 个**
  是真的「实参无值」(`es.DEBUG.DEFINE` 的分隔行)。
- **处置**:`expr::Evaluator` 只对「最后一个操作数不足」的二元运算容忍一次,
  且把栈上已有的 0 作为该参数值;其它操作数不足仍报 `rt.expr.malformed`。
- **复核**:`crates/yu-rt/src/expr.rs` 的 `step` 分支 + 语料统计脚本
  `crates/yu-rt/scripts/corpus_stats.py`。
- **2026-10-23 收口**:容忍分支落地(`expr.rs:372` 附近,仅「最后一个操作数
  不足」的二元运算容忍一次);ERIS 全量语料跑通无该挂号 ⇒ 形态处置完成,
  状态改 `fixed`。(引擎对该形态的官方处理仍无 trace;若后续 trace 与之
  冲突以官方为准并勘误。)

---

## DIV-RT-005 · 脚本执行错误后的官方继续策略

- **现象**:官方引擎处理体返回错误码后的调度行为**未取证**。
- **处置**:调度器统一「本任务让出 + 标记结束 + 记录到 `ChainReport::errors`」,
  并把 Diag 写进事件流(`VmEvent::Diag`)。这是**显式策略**,不是猜测的官方行为;
  一旦拿到官方 trace 立即核对。
- **复核**:`fixture.rs::unknown_label_is_fatal_and_recorded`、
  `fixture.rs::undeclared_variable_write_is_reported`。
- **2026-10-23 更新(P3)**:策略已细化并部分取得官方证据:
  - 默认槽(脚本执行错误)路径:**REA 实证**(DIV-RT-043)——报一次
    `[内部エラー] スクリプト実行エラー` + 返回 -1 ⇒ 本帧让出,**任务不终止**;
  - 内核现行:`Unimplemented`/`Recoverable` = 挂号留痕后**继续执行下一条**
    (`vm.rs` `step_command` 错误分支);`Fatal` = 结束任务 + 记录
    (`ChainReport::errors` + `VmEvent::Diag`);
  - 官方对其它错误路径(非默认槽)的继续策略仍无 trace ⇒ 本条**保持 open**,
    旧文「一律让出+结束+记录」已过时,以此为准。

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
- **2026-10-23(P3)**:`_WINDOW_INIT_POS` 语义已定:引擎读 `[0x80133c]`,
  该字节由 `yscfg.dat` 第 10 个 int 写入(`0x460b74/0x460b87`),由宿主经
  `BootConfig::window_init_pos` 注入(DIV-RT-047)。`_WINDOW_EXEC_CAPTION`
  按原结论实现为宿主 setter:`Vm::request_window_caption()` 置 1,脚本读后
  写回 0;VM 不自行置位。
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
- **旧处置**:全部使用**注入时钟**(`BootConfig::start_ms` + 每逻辑帧步进),
  固定 UTC 纪元 1970-01-01,星期 0=日..6=土;**不读墙钟**(A3)。
  `_OS_TIME(0)`(OS uptime)需要宿主注入 ⇒ `Unimplemented`。
- **2026-10-26 收口(存档时间错位)**:ERIS `es_SaveLoad.yst:194-200` 在存档
  时用 `@_TIME(0,0..6)` 写「現実の日付」进存档块,桌面实测落盘时间显示为
  1970-01-01(注入纪元),根因即本条。
  - 新增 `BootConfig::local_time: Option<HostLocalTime>`(年月日时分秒毫秒
    = 宿主启动时的**系统本地时间**);`@_TIME(1,*)` 返回该基准,
    `@_TIME(0,*)` 在基准上叠加逻辑流逝(`now_ms - start_ms`),跨日/月/年
    自然进位。内核仍不直接读墙钟(A3)。
  - `yu-cli` 在**真实运行宿主**(`run` / `serve` / desktop)启动时用
    `time::OffsetDateTime::now_local()` 读一次并注入;用例/trace/golden
    保持 `None`,继续走 1970 确定性纪元。
  - 复核:`cargo test -p yu-rt --lib`(73 passed,含跨午夜进位/公历往返);
    `cargo test -p yu-rt --test fixture time_sysvar_uses_injected_local_clock`
    (1 passed)。
- **残留**:`_OS_TIME(0)`(OS uptime)仍未注入 ⇒ `Unimplemented`;`_TIME(0,*)`
  用逻辑流逝近似墙钟,长会话跨 DST 切换时不会自动改偏移(存档时间戳场景
  偏差可忽略)。golden 用同一注入时钟即可复现。

---

## DIV-RT-010 · 路径类系统变量

- `$_SYSTEM_PATH` / `$_SAVE_PATH` / `$_APPDATA_PATH` / `$_PERSONAL_PATH` /
  `$_PROJECT_PATH`:官方是真实文件系统路径。内核遵守 A2(数据只读、存档只写
  SaveDir),不猜宿主布局 ⇒ `Unimplemented`,由 CLI 装配层注入后解除。
- **2026-10-23 收口(P4 M-C)**:`BootConfig::paths`(`vm::HostPaths`,五项
  `Option<Vec<u8>>`)+ `yu-cli` 装配层注入(`engine.rs::host_paths`):
  `$_SYSTEM_PATH`=工程根、`$_SAVE_PATH`=存档根(显式重定向或工程根
  `save/`)、`$_APPDATA_PATH`/`$_PERSONAL_PATH`=`USERPROFILE`(+Documents);
  `$_PROJECT_PATH` 手册无页 ⇒ 保持 `None`(不猜值)。P3 census 时未实现
  `LET` 仅剩 `_SAVE_PATH` 1 次,注入后归零;P6 门禁 `yu-cli case --all`
  **8 passed**。状态改 `fixed`。

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
- **2026-10-23 复核(状态收窄)**:原表多数子项已另证实现 ——
  `VARINFO[SEARCH]` 按手册例 5/6/7 实现(`handlers.rs`,针值 `INT/FLT/STR`);
  `VARACT[INIT+G_*]` 见 DIV-RT-024;`RETURNCODE`/`WORD`/`_` 见 DIV-RT-028;
  `RETURNBREAK`/`PREP` = 官方 no-op 槽(REA `0x7acf58`/`0x7acf4c`,
  `NOOP_COMMANDS_0495`);`F_INT/F_FLT/F_STR/F_VAR` = 默认槽 ScriptError
  (DIV-RT-043:静态命中全在 `global_f.yst` 声明段,600 帧运行期 0 次);
  `CSV`/`PLUG`/`ERRORINFO` = 官方「参数求值+返回 0」(`NoopEval`,
  REA `0x43e640`/`0x44bd10`/`0x46356c`);`ERROR` 启用表见 DIV-RT-046。
  **仍挂号**:`VARINFO` 的 `STRTYPE/SJISCODE/UTF8CODE`(手册无例,引擎
  §3.21 只证到分支地址)与 `VAR`(无页、无官方体)⇒ 本条保持 open。

---

## DIV-RT-012 · 存档容器

手册 `file/save.html`/`load.html` 给出参数表(12 个脚本自备槽),但**盘上
`.sd` 字节布局未取证**。内核把 `SAVE`/`LOAD` 归入 DomainCall(Save 域),
不写任何文件;存档字节级对齐属 `yu-save`(任务书后续)。

**2026-10-23 收口(P4 M-A/M-B)**:盘上布局已按官方产物自证 ——
`yu-format::yssd`(头 `YSSD` + 块偏移表 `0x400×u32` + 块结构 + type1/type2
块 + zlib level-1 载荷;libz-sys 钉版 `=1.1.22` 保证 deflate 输出与官方
一致),回归 `yu-format/tests/yssd_roundtrip.rs::official_sd_files_roundtrip_byte_exact`
对官方 5 文件 269 块 **逐字节一致**(P4 实测)。宿主侧 `yu-save`(`SaveHost`)
消费 SAVE/LOAD/SAVEEND/FILEACT 域事件;忠实写盘在 `SET/LET` 变量引用身份
取证前显式拒绝(`sv.write.refused`,DIV-RT-042 门禁)。内核「不写文件、
只发域事件」的分层不变。状态改 `fixed`。

---

## DIV-RT-013 · 全角/半角转换表

手册 `variable/varact.html` 写了 HANTOZEN/ZENTOHAN 的**行为**;引擎用三张
二进制表(§3.26:`0x808840` 63 项 / `0x5B5170` 95 项 / `0x5B5230` 126 对)。
表未移植 ⇒ `Unimplemented`(不手抄、不近似)。

**2026-10-23 收口(P3 §4)**:三张表已自 0.495 exe 取出,落
`data/case-tables-0495.tsv`;`src/case_tables.rs` 按 `0x4546aa..`/
`0x4548cf..`/`0x46b327`/`0x46b395` 逐条对照实现(`hantozen`/`zentohan`,
单字节原样、双字节查表),`VARACT` 分支接线(`handlers.rs`)。
**复核**:`fixture::varact_hantozen_and_zentohan_use_official_tables`
(ASCII/半角片假名/汉字直通/反向;2026-10-23 本机实测 ok)。状态改 `fixed`。

---

## DIV-RT-014 · MATH[RND]

手册 `math/math.html` 例 17 给出 `RND` 的**范围**语义;引擎随机源是
**MT19937**(§3.20),但种子来源未取证。确定性要求(A3)⇒ 必须注入 RNG;
当前 `Unimplemented`。

**2026-10-23 收口(P3 §4)**:REA 实证 —— 初始化 `0x465f64`(`mt[0]=seed`,
624 项递推常数 `0x6c078965` = MT19937 `init_genrand`),种子来自
`GetTickCount`(`0x461f98`);`RND` 体 `0x448571..0x4485b7` =
`min + rand32 % (max-min+1)`、`min>max` 时交换。内核 `src/rng.rs`
标准 MT19937 + `BootConfig::rng_seed`(缺省 5489 = 参考实现缺省)。
**复核**:`fixture::math_rnd_uses_injected_mt19937_seed`(3499211612 % 9
→ 首抽 3;2026-10-23 本机实测 ok)、`rng.rs::reference_sequence_seed_5489`。
逐位复现官方仍需采集官方 trace(种子随机),状态改 `fixed`(语义+注入面齐)。

---

## DIV-RT-015 · TASK 的局部参数

手册 `control/task.html` 列出 `TINT..TSTR`、`SCRIPTPOS`、`TEXTPOS`、
`RESTART`。本实现:`ID`/`E`/`Z`/`#`(生成与启停);其余参数见下。
- **2026-10-23 更新**:`TINT..10`/`TFLT..10`/`TSTR..10` 已按官方源码
  生产者/消费者对接线(`es_ERIS.yst:524` `TASK[…TINT=@_PINT(1)]` →
  `:533` 任务体 `@_TINT(1)`;引擎槽位 `+0x3d0/+0x3d4`=TINT、
  `+0x348/+0x34c`=TFLT、`+0x458`=TSTR;未传槽为 0,不继承调用者);
  `SCRIPTPOS` 已按 0.495 `0x452a54` 序列化 / `0x450fe0` 装载的**完整执行位**
  实现(魔数 `01..08`、IF/GOSUB/LOOP 三栈、`text_line`、`pc`、脚本名),
  `TASK[SCRIPTPOS=…]` 对既有/新建任务做位置ロード(手册例 4)。
  **复核**:`fixture::task_without_tint_does_not_inherit_caller_registers`
  (2026-10-23 本机实测 ok)。
- **2026-10-26 收口**:`RESTART`(槽 37)已按 `0x450c46/0x450c55` 实现
  (`fixed(no-manual)`,2026-10-25);`TEXTPOS`(槽 36)在 0.495 TASK 处理体
  `0x4506f4` 的参数链里**没有分支**——只检查 `0x684160/161/162/163/182/
  183/185`(ID/E/A/Z/#/SCRIPTPOS/RESTART),无 `0x684184` ⇒ 官方忽略
  `TEXTPOS`。内核按同型对齐(不写任务字段、不报 Unimplemented)。
  TASK 的参数面(含 `TINT..10/TFLT..10/TSTR..10`、`SCRIPTPOS`、`RESTART`)
  至此全部有官方依据;手册无页项按 `fixed(no-manual)` 归档。
  `TASKINFO` 的 9..13 槽另见 DIV-RT-025(2026-10-26 更新)。
- **2026-10-26 A 槽**:`A`(槽 2)手册无页;`TASK` 处理体
  `0x450bbf..0x450bd1` 按参数写 `task+0x15`,构造器
  `0x465630/0x465633` 默认 E=A=1;调度器 `0x4043dc..0x4043e6`
  (`movsx E; movsx A; test A, E`)以 `A & E != 0` 为步进闸门。
  `TASKINFO[A]`(`0x4527b0`)原样读回。内核新增 `TaskSeed/TaskNode/TaskView`
  的 `active`、`TASK` 的 A 参数与 `Effect::SetActive`;`TASK[E/A]` 改为
  只在参数**存在**时写(修掉旧实现 `TASK[ID=X Z=…]` 无条件
  `SetEnabled(true)` 把已禁用任务重新打开的 bug)。
  回归:`fixture::taskinfo_a_reads_default_and_task_a_toggles_it`、
  `task_e_a_changes_are_visible_to_taskinfo_in_same_pump`、
  `task_textpos_is_aligned_noop`。

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
- **2026-10-23 收口**:语义已由「源码 ↔ 编译产物 ↔ YSVR 声明」三方对上:
  ERIS 0.495 源码 `\DBG.VAR("…", @G(0001))` 编译成
  `56 03 00 60 89 04 …`(前缀 0x60、id 1161),YSVR 声明 1161 =
  INT 数组 [8192];`@L(0101)` 同前缀(id 1163, INT [8192]);
  `$G(0001)` → `56 03 00 23 8a 04 …`(id 1162, STR [8192])。
  ⇒ 0x60 = ERIS 旧式**数值**空间(`@G/@L`)、0x23 = 旧式**字符串**空间
  (`$G/$L`),id 全局唯一、类型由工程 `ysv.ybn` 声明决定。
  实现:`VarSpace::LegacyNum/LegacyStr`(`value.rs`)+
  `Vm::canonical_var` 按 id 声明归一化(`vm.rs`);真实语料 corpus
  (`eris_entry_chain_runs_end_to_end`)跑通。**其余未知前缀**仍按原处置
  `Unimplemented` 挂号(`vm.rs` VarAccess 读路径)。状态改 `fixed`。

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
- **2026-10-23 续(存在性判定收口)**:上述分支虽已前置,但守卫用的是
  **「旗标值 ≠ 0」**——ERIS 语料 s33(`es_logset.yst`)pc46..49 的
  `VARACT[SET=$… PUSH=0]`(`PUSH` 位置 0)被误判为无参,掉进字符串加工
  分支报 `rt.cmd.slot 缺 LET`(用户桌面 Fatal,`es.ScenarioScript@46`)。
  官方语义 = **参数存在性判定不看值**(DIV-RT-024 INIT「不在场才不做」、
  DIV-RT-044 DIALOG「`OKCANCEL=0` 也算存在」同族)。已把 `op_varact` 全部
  旗标分支(TYPE/COPY/CUT/UPPER/LOWER/HANTOZEN/ZENTOHAN/DIMSIZE/PUSH/POP)
  统一改为 `is_some()` 存在性判定。回归:
  `fixture::varact_push_zero_is_presence_not_value`(s33 pc46 形态)。

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
- **2026-10-23 状态**:实现与引擎证据均已完备(四槽 + 处理体反汇编 +
  corpus),条目可处置的部分没有遗留 ⇒ 状态改 `fixed`;「手册缺页」作为
  不可修的外部事实保留在本条目内备查。

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
- **2026-10-23 状态**:算法按引擎反汇编逐条落地(含「缺 `INIT` 整条不做」
  守卫、「不读 `SET`」特例),ERIS 语料 `es_SoundDefine.yst` 等初始化段
  跑通、census 无回退 ⇒ 状态改 `fixed`;「手册缺页」作为不可修的外部
  事实保留在本条目内备查。

## DIV-RT-025 · `TASKINFO` 的数值/位置字段

- **手册** `control/taskinfo.html`:EXIST/E/Z/FILE/LINE/IFNEST/GOSUBNEST/
  LOOPNEST/SCRIPTPOS。0.495 页**不含** `FILE0/FILENO0/LINE0/TEXTLINE/
  NEXTVOICE`(无页,按引擎证据实现)。
- **0.495 处理体 `0x4523fc` 逐槽**(REA,2026-10-26):
  - 分派顺序固定:`EXIST/E/A/Z/FILE/FILE0/LINE/LINE0/TEXTLINE/NEXTVOICE/
    IFNEST/GOSUBNEST/LOOPNEST/FILENO/FILENO0/SCRIPTPOS`;存在性判定**不看值**
    (`FILE0=0` 也算请求)。
  - `FILE` = `0x801850[task+0x3c]` 名字表字符串(`0x43f0d8` 要求 LET 为 STR),
    不是脚本号数字;`FILENO` = `task+0x3c`。
  - `LINE` = `task+0x38[task+0x20]`(数据区 B 逐 pc 源码行);
    `LINE0` = `frame[0]+0xc`(第一层 GOSUB 的调用点行),无 GOSUB 时回退 `LINE`。
  - `FILE0`/`FILENO0` 同上取 `frame[0]+8`(调用方脚本号 → 名字表)。
    `task+0x144` 是 GOSUB 帧数组,`[0]` = 第一层调用的返回位
    (`0x4459c4` 写入 `frame+4`=返回 pc、`+8`=调用方脚本、`+0xc`=调用点行)。
  - `TEXTLINE`/`NEXTVOICE` = `task+0xc/+0x10`;由 `RETURNCODE`(`0x44c390`)
    从当前指令首窗口记录抄入(首 dword = `id|type<<16`,次 dword = `length`)。
    语料 `ystdlib.yst` 的 RETURNCODE 窗口 = `{id=1..N, type=0, length=0}`,
    因此 `TEXTLINE` = 文本行计数、`NEXTVOICE` = 0(ERIS 语料无 voice);
    引擎原样读,内核同型保存。
  - `GOLABEL`/`GOSUBLABEL`/`TEXTPOS` 在 0.495 处理体**无分支** ⇒ 默认
    `return 0` 且不写 LET(本实现对齐,不挂号)。
  - `A` 槽(4)读 `task+0x15`;值来源与 `TASK[A]` 见 DIV-RT-015(同一条
    `A & E` 调度闸门)。
  - ID 未命中:`0x452918` 按 LET 目标类型写 0 / 0.0 / ""。
- **本实现(2026-10-26 复核修复)**:以上全部落地;另修:
  - `FILENO0` 与 `SCRIPTPOS` 同时请求时的引擎 fallthrough
    (`0x4525fa/0x4528fa` → `0x452628`):先写 FILENO0,再写 SCRIPTPOS
    (旧实现只取第一个查询,漏第二次写入);
  - `SCRIPTPOS` 从内核 8 字节 blob 换成引擎线格式
    (`0x452a54`/`0x450fe0`):magic `01..08`、IF 18 字符/帧、
    GOSUB 20 字符/项(返回脚本号线上 4 字节:引擎把运行期 u16 零扩展写出,
    装载只取低 16 位)、LOOP 34 字符/项、`text_line`/`pc`/脚本名;
    线格式已跨五版核对(2026-10-26 勘误):0.292/0.466/0.479/0.495/v494
    的序列化器与装载器推进量均为 18/20/34 字符,各版只有任务结构体内偏移
    不同、线上字节一致;`TASKINFO` 产引擎串,`TASK` 完整恢复三栈与位置
    (见 `crate::scriptpos`);
  - `ReturnCode.line` 改用完整 u16 窗口 id(`kind|flag<<8`),不再被
    u8 `kind` 截断(单脚本文本行 >255);
  - `TASK` 重配置继承 `task+0xc/+0x10`(引擎 TASK 处理体不清这两个字段);
  - LET 类型不符诊断按 `query+目标类型` 去重(引擎每次打印,内核只留一条,
    避免热循环刷屏);消息补上 query 名;
  - ID 未命中 + LET 目标未声明时保留 `write_target` 的 Fatal,不再静默。
  回归:`fixture::taskinfo_text_line_and_next_voice_*`、`taskinfo_file0_*`、
  `taskinfo_query_presence_*`、`taskinfo_missing_id_*`、
  `taskinfo_a_reads_default_and_task_a_toggles_it`、
  `taskinfo_fileno0_falls_through_to_scrip_pos`、
  `returncode_line_keeps_full_u16_id`、
  `task_reconfiguration_preserves_text_line_and_voice`、
  `taskinfo_type_mismatch_reports_once`、
  `taskinfo_missing_id_with_undeclared_target_is_reported`。

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
- **2026-10-23 收口(P3 §4)**:脚本**写入**路径已取证 —— REA
  `0x446b98..0x446bb5`(id `0x7b`):RHS 字符串**逐字节拷进 `[state+0x3c]`**,
  不重建文本、不重算逐字符标志(与 `$_TEXT` 的 `+0x38` 写入并列)。
  实现:`LET[$v = …]` 目标为 `_TEXT_FLAG` 时 `set_text_flags` 整串替换
  (`handlers.rs`)。旧文「写入保持 `Unimplemented` + 无副作用」作废。
  **复核**:`fixture::text_flag_write_replaces_stream_without_rebuilding_text`
  (2026-10-23 本机实测 ok)。状态改 `fixed`。

## DIV-RT-033 · `_KEYLAST`/`_MOUSELAST`(2026-10-19;2026-10-22 fixed)

- 手册无页(0.479/0.495 字节级核对);`InputState` 内部照常记录最近按下
  按钮码(按下沿,码 = 引擎按钮码)。
- **2026-10-22 取证**:REA 0.495 `FUN_00427350` case 0x91/0x92 直接返回
  `DAT_00801908` / `DAT_0080190c`;写入点是窗口过程 `FUN_0045a3b4`
  (WM_KEYDOWN → VK;WM_LBUTTONDOWN/RBUTTONDOWN/MBUTTONDOWN → 1/2/4)。
  已实现为可读系统变量;ERIS `es_Text` 用写 0 取消 skip。
- **复核**:`input::tests::last_key_and_mouse_codes_follow_engine`。

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
- **恢复条件**(2026-10-24 勘误:原文漏了模式分支,把「等输入」当成唯一判据):
  引擎 `FUN_004046fc`(`0x4046fc..0x404910`)先判**文本输出模式**:
  - `0x404700 MOVSX EDX,[EBX+9]` + `0x404706 JE 0x404831` —— `task+9` 未置位即返回;
  - `0x404711 CMP byte[state+0x1c],1` + `0x404715 JNE 0x404828` ——
    **`[state+0x1c] != 1`(手动/其它值)直达 `0x40482d MOV byte[task+9],0`,
    不经任何按钮表**;
  - `== 1`(自动)才走 `0x40471b..0x4047ec` 的 SKIPBUTTON/CLICKBUTTON 判定:
    推进按钮表 `0x5b48ac..0x5b48b8`(MOUSE_L=1 / ENTER=13 / PAD=0x3FF /
    PAD=0x3FF)与 `0x5b48bc`(CTRL=17);SKIP 侧计数 `DEC` 判当前帧按下沿,
    CLICK 侧 `OR` 判零。
  `[state+0x1c]` = `TEXT[OUTPUTMODE]`(手册 `text/text.html`:0=手動 / 1=自動,
  默认自动)。**v494 同构**:`<game-b>` 的 v494 可执行体
  `RETURNCODE` = `0x44c20c`,头部逐指令对齐(`[0x802ad8]` vs `[0x801800]`),
  尾部 `C64609` = `MOV byte[ESI+9],…`。
- **处置**:`op_returncode` 返回 `StepOutcome::TextYield`;调度器置 `text_rest`,
  恢复按上述模式分支:`TaskExecutor::text_auto_mode()` 为真才要求推进输入,
  否则下一逻辑帧直接恢复(手动模式 = 一帧休憩,手册「一旦休憩します」)。
  `InputTimeline` 注入的点击只对自动模式有意义。
- **为什么不修就起不来**(2026-10-24,商业包实测):ERIS 框架 `es_first.yst`
  首指令 `TEXT[OUTPUTMODE=0]` 切手动、自己用 `INPUT`/`MOUSE` 做点击等待。
  若把手动模式也当"等输入",`ES.CHARNAME.DEFINE`(s84,256 组)这类定义链
  会永久停在第一条 `RETURNCODE`;`es_first` pc4..pc54 的 `.DEFINE` 链走不完、
  `es.S.GO "ES.GAMEMAIN"` 到不了,画面只剩已建的 GAMEMAIN 按钮栏 +
  `CG[FILE=""]`(手册 `cg/cg.html`:1×1 黑图)被 `COPY2` 铺满 ⇒ 表现为
  「黑底 + 日期框 + 一排 SAVE/LOAD」且必须点击才动。
- **未取证**:`TEXT[WAIT=…]` 等文本命令对同一标志的影响(手册 `text/text.html`
  有 `WAIT`/`RETURN` 关键字,语义待取证)。
- **复核**:`cargo test -p yu-rt --test fixture one_text_line_per_frame_until_advance_input`
  (自动模式,行为未变)与 `manual_outputmode_returncode_resumes_without_input`
  (手动模式,零输入连续推进)。

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
- **2026-10-23 登记**:下游 `yu-save` 已按本条设硬门禁 —— `SAVE[FILE]` 刷盘
  **显式拒绝**(`sv.write.refused`;引用身份拿不到就写不出与官方一致的块),
  读侧照常;容器写路径本身已逐字节就绪(DIV-RT-012)。解除条件 = 本条关闭
  (`ArgValue` 加引用字段 + `yu-trace` 穷举匹配同步,跨 crate 契约变更,
  登记为 H-P3-03)。状态维持 `blocked`。
- **2026-10-24 收口(B1,任务书 `docs/tasks/task-b1-save-loop.md`)**:
  - **取证**(0.495 `yu-ris.exe` sha256 `4f5eefa3…`;capstone 线性+递归下降,
    产物 `target/tmp/b1/*.asm`):SAVE 处理体 = `0x44c45c`(opcode 84,
    分发表 `0x7ace20[84]`;**旧锚 `0x44e1c8` 实为 SAVEINFO,勘误**)——
    ①`SET`/`LET` 变量身份经 `0x683a60/0x683c00/0x683da0[slot]` 三表传递
    (0x44c583/0x4471eb);②**预约即快照**:`0x44d73d` 在 `SAVE[DNO]` 时刻
    写 btype/strict/var/clen/ulen 并取当前值 zlib 压缩入预约表
    (`0x7fad1c`/`0x7fbe1c`),`SAVE[FILE]`(`0x44cd5a..0x44ceab`)只写文件头
    +偏移表(容量 `0x400`,版本 `0x1e0`)按 DNO 升序流出**预约时**字节,
    预约表不清(SAVEEND `0x44e1a0` 才清;语料 es_GSD 每次刷盘后显式
    SAVEEND);③同 DNO 重复预约 = 替换(`0x44c740`);④组形态
    `G_<族><组>=1` 按**首个在场旗标**(级联序 G_INT→…→G_STR4)定
    (类型,组),枚举全局变量表逐变量子块化(块类型 2,
    `0x44ca8a..0x44cc57`);⑤LOAD `0x44708c`:偏移寻址 `0xC+DNO*4`
    (`0x44735a`),块类型 2 逐子块**按块内 var_id 直接恢复**
    (`0x447460..0x4474de`,不读 `LET`/`G_*` 在场位),块类型 0 写 `LET`
    目标整组(`0x447b3d`);⑥官方 golden:`seld.sd`=1 块 var 2264
    Int[2001](值 `[1]=6`)、`gsd.sd` DNO 3/4 = btype2 子块 var 1161/1162
    (旧式空间 `@G(0001)`/`$G(0001)`,DIV-RT-017)。
  - **裁 3(既读色)**:启动链 `es_First.yst:139 \GSD.INIT` →
    `es_GSD.yst:130 es.GSD.INIT` → `:142 GOSUB[#=ES.SEL.FILELOAD]` →
    `es_Select.yst:1068 LOAD[FILE="seld.sd" DNO=1 LET=@es.SELD()]` ——
    **全是脚本命令,零内核新机制**;LOAD[LET] 落地即自然接通。
  - **实施**:①`ArgValue.reference: Option<VarRef>`(裁 1-A;旧事件流
    缺字段=None);②`yu-trace` `TraceArg.reference` 序列化/解析(旧 trace
    兼容);③`SaveHost` 同步契约(`reserve/reserve_group/flush/save_end/
    load_block`);④`op_save/op_load/op_saveend` + FILEINFO/SOUNDINFO/INFO
    的 LET 引用槽;⑤`yu-save` 重写:预约=值快照、flush 不清表、只读根仍拒
    (原因改为「根不可写」);⑥`save-load-loop` 用例 + 回归 ×3 +
    官方 seld.sd golden 结构对拍。`sv.write.refused` 语义保留但正常路径
    零出现(2026-10-24 主人令后**默认可写**,只读为显式构造态)。
  - **桌面实测追加修(同日)**:存档界面槽列表空的根因 = `FILEINFO`
    缺「散盘」回退——槽存在判定链
    `es.SaveDataCheck → \_file.info.exist($es.SAVEPATH+名) →
    FILEINFO[EXIST]`(ystdlib:989)只查 VFS ⇒ save 根文件恒「不存在」,
    同闸门挡住启动恢复。已按 REA `0x44316c` 文件系统支补
    (`save_root_fs_exists`;`fixture::fileinfo_fs_fallback_checks_save_root`)。
  - **复核**:`fixture::save_load_loop_records_reserve_flush_and_writes_back`
    / `save_group_form_enumerates_matching_globals` /
    `load_group_block_restores_each_var_by_id`;
    `yu-save::official_seld_block_reads_as_var_snapshot`;
    用例 `cases/save-load-loop.json`(PASS,gsd/extra/seld/kidoku 落盘)。


---

## DIV-RT-043 · 官方默认槽 `0x459e90` = 脚本执行错误(2026-10-23,P3)

- **取证**(0.495 `yu-ris.exe` sha256 `4f5eefa3…`):
  - `FUN_0045cf90` `0x45d1ae..0x45d1c0`:`MOV [EAX+0x7ace20], 0x459e90`,
    循环上限 `0x1dc` ⇒ 119 槽先全部填默认体,再逐槽覆盖;
  - 默认体原始字节(REA `read_bytes 0x459e90`):
    `6a 01 68 d2 bb 0d 00 e8 6c be 00 00 83 c4 08 b8 ff ff ff ff c3`
    = `push 1; push 0xdbbd2; call 0x465d08; add esp,8; mov eax,-1; ret`;
  - `0x465d08` → `0x465dec` 解析消息 id → `0x465e30` 查 `ysbin/yse.ybn`;
    工程 yse 解出 id `0xdbbd2` = `「[内部エラー]

スクリプト実行エラーが
    発生しました。」`;
  - `FUN_004612b0` 用全局 `[0x5e4f84]` 抑制重复错误框 ⇒ 只报一次;
    处理体返回 -1 ⇒ 本帧让出(`task+9` 语义之外的另一条让出路径),
    **任务不终止**,pc 在取指后已前进。
- **no-op 槽**:`0x4634d8` = `xor eax,eax; ret`。覆盖写地址:
  `0x7ace44`(DEBUGLIST)/`0x7acef0`(LABEL)/`0x7acf4c`(PREP)/
  `0x7acf58`(RETURNBREAK)/`0x7acf60`(S_FLT)/`0x7acf64`(S_INT)/
  `0x7acf68`(S_STR)。
- **静态 vs 运行**:`F_INT/F_FLT/F_STR` 静态 337/72/64 处,全部位于
  `global_f.yst`(Sprite/Sound/Backlog)的声明段(group 0..N);入口标签
  指向声明段之后,600 帧运行期命中 0。静态扫描器不改,两口径并列记录。
- **处置**:`HandlerKind::ScriptError` 报一次
  `rt.script.error`(Recoverable)+ 返回 `StepOutcome::Yield`;后续命中
  只计数(`Vm::script_slot_hits`)。`F_*`/`G_*`/`F_VAR`/`S_VAR`/`G_VAR`/
  `MACRO`/`_IFDEF`/`_IFNDEF`/`_IFEND`/`);`/`SYSTEMMODE`/`COMPILEMODE`/
  `RELEASEMODE`/`PROJECTFOLDER` 归此类。`S_INT`/`S_FLT`/`S_STR` 改 no-op
  (全局初值仍由 `ysv` 预装载)。
- **复核**:`fixture::default_slot_reports_once_and_yields_without_terminating`、
  `fixture::noop_slots_continue_in_the_same_pump`。

## DIV-RT-044 · DIALOG 的 VM 状态机(2026-10-23,P3)

- **三层**:手册 `other/dialog.html`;语料 17 处(只用
  STR/CAPTION/YESNO/LET/DEFBUTTON/INPUT/DEFSTR/LETSTR/OKCANCEL/
  YESNOCANCEL);REA `0x43fc74`、输入模板 `0x8083c4`、对话框过程 `0x4403d0`。
- **契约要点**:
  - 参数存在性判定**不看值**(`OKCANCEL=0` 也算存在);
  - 非 INPUT = `MessageBoxA`;按钮组按存在性
    YESNOCANCEL > YESNO > OKCANCEL > 缺省 OK;`OK` 槽官方从不读取;
  - `DEFBUTTON` 1..4 → MB_DEFBUTTON1..4;`ICON` 1=Info/2=Question/
    3=Warning/4=Error;
  - `LET` = 1 基按钮序号(OK/Yes=1, No/Cancel=2, YesNoCancel-Cancel=3);
  - `INPUT` = 自绘对话框,`INPUTLIMIT`>0 时 EM_SETLIMITTEXT,OK 后把
    控件文本写 `LETSTR`、`LET=1`;
  - 官方是同步模态(计时器暂停);内核改为「任务挂起 + 宿主回答」。
- **内核**:`Vm::pending_dialogs()` / `Vm::answer_dialog(id, button, text)`;
  `DomainCall{System,DIALOG}` 是通知,权威状态在 pending 队列;回答前
  调度器不推进该任务;回答后下一逻辑帧从 DIALOG 的下一条继续。
- **未取证参数**:`STR2..8`/`A*`/`E*`/`CHECK*`/`FILEOPEN`/`FILEEXT`/
  `FOLDER`/`CHECKBOX`/`SLIDER`/`NOWAIT` ⇒ `Unimplemented`,不弹窗、不写回。
- **宿主呈现面**:交接件(P1/P4)。与 `_WINDOW_EXEC_CLOSE` 的交互:
  DIALOG 挂起期间宿主应把窗口关闭请求当作对话框取消(最后按钮),
  不要再推 `_WINDOW_EXEC_CLOSE`。
- **复核**:`fixture::dialog_blocks_task_until_answered_and_writes_let`、
  `dialog_input_writes_letstr_and_let`、`dialog_unsupported_param_is_unimplemented`。

## DIV-RT-045 · 托盘/剪贴板/注册表/YMV 的域边界(2026-10-23,P3)

| 命令 | 处理体 | 处置 | 证据 |
|---|---|---|---|
| `TRAY` | `0x453a4c` | `Domain::System` 事件(宿主托盘图标) | 手册 `other/tray.html`;语料 2 处 |
| `CLIPACT` | `0x43e2d0` | 内核镜像 + 事件;需 TEXT+SET 都存在 | 手册;REA(CF_TEXT 写入) |
| `CLIPINFO` | `0x43e410` | 读镜像写 LET;仅 TEXT 存在时动作;过滤 `<0x20` | 手册;REA `0x43e4d2..` |
| `REGINFO` | `0x44bd30` | `Unimplemented`(需宿主注册表) | 手册 `reg/reginfo.html` |
| `YMV`/`YMVEND` | `0x458ee0`/`0x459a64` | `Domain::Movie` 事件 | 分发表;无手册页 |
| `YMVINFO` | `0x459b98` | `Unimplemented`(需宿主媒体元数据) | 分发表;写 LET |

- 剪贴板镜像 API:`Vm::clipboard()` / `Vm::set_clipboard(bytes)`;宿主可
  注入外部剪贴板内容(内核不访问 OS 剪贴板)。
- **复核**:`fixture::clipact_and_clipinfo_round_trip_with_control_filtering`、
  `tray_is_a_domain_event`。

## DIV-RT-046 · `ERROR` 的逐命令启用表(2026-10-23,P3)

- **REA**:`0x46358c` 求值参数后,对参数槽 2.. 的 `COM_<命令名>`,把值
  低字节写进 `[0x689b00 + 命令序号]`;引擎初始化 `0x462124` 把 128 字节
  全置 1。
- **未取证**:该表的**消费者未定位**(xref 只找到初始化),因此内核只
  保存(`Vm::command_enable()`),不据此改变分派。
- `CSV`(`0x43e640`)、`PLUG`(`0x44bd10`)、`ERRORINFO`(`0x46356c`)
  官方体都是「参数求值 + 返回 0」⇒ 实现为 no-op(保留求值诊断)。
- **复核**:`fixture::error_sets_the_per_command_enable_table`。

## DIV-RT-047 · `_WINDOW_INIT_POS` 与 `_WINDOW_EXEC_CAPTION`(2026-10-23,P3)

- `_WINDOW_INIT_POS`:int getter `0x4276f3` ← `[0x80133c]`;写点
  `0x460b74/0x460b87` 在 `FUN_00460924` 里,源是 `yscfg.dat` 第 10 个 int
  (`FUN_00469df2` 打开 `\yscfg.dat` 后按 0x20 个 int 读入)。该文件属于
  「エンジン設定.exe」,不在 `data.ypf`。⇒ `BootConfig::window_init_pos`
  宿主注入;ERIS 工程值为 0。
- `_WINDOW_EXEC_CAPTION`:手册仍无页;语料语义 = 一次性「窗口尺寸再初始化」
  标志。内核 `Vm::request_window_caption()` 置 1,脚本读后写回 0;
  触发条件(`WM_SYSCOMMAND` 路径 `*ecx` 来源)未取证 ⇒ VM 不自行置位。
- **复核**:`fixture::window_init_pos_is_host_injected`、
  `window_exec_caption_is_a_host_setter_round_trip`。

## DIV-RT-048 · `_TEXT_NUM`(2026-10-23,P3)

- **REA**:int getter `0x4276e3` 读 `[0x80174c]`;该全局在装载
  `ysbin/yst.ybn` 时由记录 `+0xc` 写入(`0x45d73f`,同一路径写
  `[0x801728] = count+1`)。
- **实测**:ERIS `yst.ybn` 头部 = `YSTD, v495, count=4600, text_total=396`;
  `yst_list` 141 条逐脚本 `text_count` 求和同为 **396**。
- **处置**:内核用 `yst_list` 求和(`Vm::text_total()`),不新增必需表
  `yst.ybn`;两者在其它工程若分歧,以 `yst.ybn` 为准的接入留待
  `yu-format` 提供 YSTD 解码后再改。
- **复核**:`corpus::text_num_matches_yst_header_total`。


---

## DIV-RT-049 · 旗标/目标形态判定族收口(2026-10-23,审查轮)

> P7 续的审查(三类病根 × 全处理器 + 真实 yscm 全表 + 语料形态扫描)发现
> 三个与 DIV-RT-019/003 同族的潜伏点;本轮按旁线反汇编判据一次对齐。

- **A1 `op_math` 旗标捕获**(曾按「值 ≠ 0」记参):官方判定链测**窗口在场**
  (每分支形状 = 直接读 SET 载荷,不读旗标值;旁线 §3.20),且**判定链原序
  与槽位序不同**(`4,7,10,5,8,6,9,13,11,12,14..21`)⇒ 捕获改 `as_int().is_some()`,
  选择改按官方链序首个在场旗标;补齐 `HASH`(引擎表 `0x5b41c0` = 标准
  CRC-32 表,检查值 `"123456789"→0xCBF43926`,新增 `src/hash.rs`)、
  `ABS` 整数路(`0x448412`:SET 整数 ⇒ 整数写)与 `SETSTR`=HASH 输入
  (旧 `Unimplemented` 作废;语料实测 SETSTR 窗口 0 处,零回归风险)。
  语料 21 处 MATH 旗标全为 1 ⇒ 本修复对现语料零行为差,纯潜伏收口。
- **A2 `VARACT[INIT=0 …]` 守卫**(曾按「值 == 0 ⇒ 不做」):官方判据 =
  载入器预写的**参数在场字节表**(`0x454d3d movzx eax,byte ptr [0x684171]
  ; G_INT 在场?` + `test/je`;`0x684160+序号`;INIT 缺席 = `je 0x454e05`
  函数尾)⇒ INIT 与 G_* 掩码构造全部改 `is_some()`。语料无 INIT=0/G_*=0,
  潜伏收口。
- **A3 `op_labelinfo` LET 写回丢下标**(曾 `store.write(t, &[], …)` 标量化)
  ⇒ 改保留 `LValue` + `write_target`(与 LET/FILEINFO/MATH 同口径)。
- **无旗标 MATH 的缺省行为**(写出 SET 值)官方未取证,保留原内核缺省并
  在源码注释标明,不另造语义。
- **复核**:`fixture::math_flag_zero_value_still_selects_function`、
  `varact_init_zero_value_still_restores`、`labelinfo_let_writes_indexed_element`;
  `cargo test -p yu-rt` 全绿(65 lib + 11 corpus + 68 fixture + 3 golden)。
  审查工具:`examples/dump_label_cmds.rs`(标签反汇编 / `scan:<sid>` /
  `scanmath` 全语料 MATH/VARACT 旗标形态统计)。

---

## DIV-RT-057 · Save/File 域事件在帧末才应用(2026-10-27)

- **现象**:点击菜单存档后目标 `.sd` 被删除且不再生成;日志只有
  `FILEACT[DELETE] 已删 …` 与可恢复告警(VARINFO STRTYPE、`SAVE[ID]`、
  `SAVE[SET]` 系统变量),**没有 Fatal**。
- **根因**:`Vm::emit` 只对 Graphics/Window/Sound 域同步调用
  `host.apply_event`;Save/File 域仅入队,由 `Engine::tick` 在整帧脚本
  执行完后 `host.apply_events(&events)`。而存档脚本
  `es_SaveLoad.yst` 的顺序是:
  1. `pc315 FILEACT[FILE=$es.SAVEPATH+$SFNAME DELETE=1]` → 删除事件**入队**;
  2. `pc341 SAVE[FILE=$SFNAME]` → `op_save` **同步** `host.flush` 写盘;
  3. 帧末 drain → 才执行第 1 步的删除 ⇒ 刚写出的存档被删;
  4. `pc342 SAVEEND[]` → 清空预约 ⇒ 最终既无文件也无错误。
- **临时探针证据**(已还原):同一帧内依次打印
  `AUDIT FILEACT script=19 pc=315` → `AUDIT SAVE flush pc=341 file=save30002.sd`
  → `AUDIT FLUSH ok exists=true`(25602 字节)→ `AUDIT FILEACT DELETE`
  → `final exists=false`。把 Save/File 事件临时改为 `emit` 同步应用后,
  删除排到 flush 之前,文件保留(但引擎随后仍 drain 一次,事件被重复
  应用;故修复必须"同步 + 只消费一次")。
- **引入点**:该顺序问题是 SaveHost 接线(B1,`16cef47`,2026-10-24)以来
  的结构性问题,不是近期 TASKINFO/TASK/SCRIPTPOS/VARINFO 改动;`Vm::emit`
  自 `d66ed41` 起从未同步处理 Save/File。修复前的最后一次内核菜单存档
  `save00001.sd`(2026-10-25)缺少 DNO 3(缩略图)与 DNO 90(ICON),
  与内核两个既有缺口一致;说明当时存档成功过,但写入/删除时序未覆盖。
- **修法(统一,不打补丁)**:所有域宿主统一为**命令时刻同步**——
  `Vm::emit` 对 `Domain::Save | Domain::File` 也调用
  `save_host.apply_event`;`Engine::tick` 帧末只保留事件统计/trace,
  不再把同一批事件喂给 `SaveHost`。事件仍入队供 `yu-trace`/golden 记录。
  这与官方 `FILEACT` 处理体就地执行一致,避免"同步写、延迟删"。
- **复核**:`fixture::fileact_is_applied_before_save_file_flush_in_same_frame`
  (顺序 = FILEACT → SAVE → flush → SAVEEND);`cargo test -p yu-rt`
  (82 lib / 4 consistency / 11 corpus / 4 corpus_golden / 95 fixture /
  3 golden);`cargo check --workspace --all-targets` 通过;桌面复现
  (临时 save 根)菜单存档后 `save30002.sd` 25608 字节保留。
