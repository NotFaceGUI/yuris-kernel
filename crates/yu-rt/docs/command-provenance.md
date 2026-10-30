# 已实现命令 × 三层出处对照表

> 任务书 C §4 的交付核心。三层 = ①官方手册页(本地 SDK HTML)
> ②真实脚本印证(官方 YSCom 编译的 ERIS 137 个 YSTB)
> ③官方引擎行为/逆向事实(0.495 `yu-ris.exe` sha256 `4f5eefa3…`,
> 旁线 `<side-repo>/docs/format-vm-opcodes.md` 的地址级证据)。
>
> 手册根:`<side-repo>/target/sdk/0.495/yu-ris_0495_032/マニュアル/YU-RIS/html\`
> 语料根:`<side-repo>/workbench/project-eris/data/ysbin`
> 引擎:0.495 官方 `yu-ris.exe`(sha256 `4f5eefa3…`);0.479 可执行版见 AGENTS §5。

## 1. 控制流

| 命令 | 频度 | ①手册 | ②语料 | ③引擎 | 状态 |
|---|---|---|---|---|---|
| `GOSUB` | 10,215 | `control/gosub.html`(参数 16 槽/嵌套 64) | ✅ 语料 10,215 次;`PINT` 4,449 / `PFLT` 4,254 / `PSTR` 4,501 | 旁线 §3.16(处理体 `0x4459c4`,参数组 A `+0x10/+0x98/+0x120`,嵌套上限 0x40) | ✅ 实现 |
| `RETURN` | 1,750 | `control/return.html` | ✅ 1,750 次;`RINT` 143 / `RFLT` 39 / `RSTR` 47 | §3.16(`0x44c0fc`,写**调用方**那层后弹栈) | ✅ 实现 |
| `GO` | 195 | `control/go.html`(跨文件、标签全局唯一) | ✅ 195 次 | §3.5/§3.6 opcode 43(`0x445830`):标签→{pc,脚本号,基址} | ✅ 实现 |
| `IF` | 3,326 | `control/if.html` | ✅ 3,326 次(窗口恒 3) | §3.13 opcode 45(`0x4462d0`):块帧 + 假分支跳 `item[2].length` | ✅ 实现 |
| `ELSE` | 887 | `control/else.html`(条件可缺省) | ✅ 887 次 | §3.13 opcode 11(`0x4404f0`):假→下一 ELSE/块出口 | ✅ 实现 |
| `IFBLEND` | 887 | `control/if.html`(IF 链说明) | ✅ 887 次 | §3.13 opcode 46(`0x4463c8`):无条件跳块出口 | ✅ 实现 |
| `IFEND` | 3,326 | `control/ifend.html` | ✅ 3,326 次 | §3.13 opcode 49(`0x446518`):depth-- 清出口 | ✅ 实现 |
| `IFBREAK` | 5 | `control/ifbreak.html`(LV) | ✅ 5 次(全部 `LV` 缺省) | §3.13 opcode 47(`0x446408`) | ✅ 实现 |
| `IFCONTINUE` | 0 | `control/ifcontinue.html` | ⛔ 语料 0 次 | §3.13 opcode 48(`0x446490`) | ✅ 实现(语料未使用) |
| `LOOP` | 522 | `control/loop.html`(SET 缺省=无限;`@_LC` 1..x) | ✅ 522 次 | §3.13 opcode 56(`0x447ef0`):帧 `{base_a,pc+1,LOOPEND pc,1,limit}` | ✅ 实现 |
| `LOOPEND` | 522 | `control/loopend.html` | ✅ 522 次 | §3.13 opcode 59(`0x4481d8`):轮数+1,`>=limit` 弹栈 | ✅ 实现 |
| `LOOPBREAK` | 159 | `control/loopbreak.html`(LV) | ✅ 159 次 | §3.13 opcode 57(`0x445b9c` 族) | ✅ 实现 |
| `LOOPCONTINUE` | 40 | `control/loopcontinue.html` | ✅ 40 次 | §3.13 opcode 58(`0x445c54`) | ✅ 实现 |
| `WAIT` | 95 | `control/wait.html`(TIME=ms / FRAME=帧) | ✅ 95 次(FRAME 81 / TIME 14) | §3.2b 门控 `+0x18`/`+0x1c`;`0x456ae4` | ✅ 实现 |
| `TASK` | 10 | `control/task.html`(ID/E/Z/#;TINT..TSTR/SCRIPTPOS/RESTART) | ✅ 10 次 | §3.15(`0x450ae9` 续帧标志;任务对象入链);REA `0x4506f4` 逐槽;A=`task+0x15`(`0x450bbf`),调度器 `A&E` 闸门 | ✅ 实现(A 按引擎证据;TEXTPOS 官方无分支,对齐 no-op) |
| `TASKEND` | 2 | `control/taskend.html` | ✅ 2 次 | §3.15(`LAB_00452070`) | ✅ 实现 |
| `TASKINFO` | 19 | `control/taskinfo.html`(FILE0..NEXTVOICE 无页) | ✅ 19 次 | REA `0x4523fc` 逐槽;`0x44c390` RETURNCODE 写 `task+0xc/+0x10`;A=`task+0x15`;SCRIPTPOS `0x452a54`/`0x450fe0` | ✅ 实现(21 槽含 A/FILE0/FILENO0/LINE0/TEXTLINE/NEXTVOICE;FILENO0→SCRIPTPOS fallthrough;GOLABEL/GOSUBLABEL/TEXTPOS 官方无分支;SCRIPTPOS 引擎字节格式) |
| `END` | 205 | `control/end.html`(RESTART) | ✅ 205 次 | `FUN_0043da44`(旧线注记) | ✅ 实现 |
| `FPS` | 2 | `system/fps.html`(1..256 / SKIP) | ✅ 2 次(`SET=60`) | `FUN_004040e8` 链的帧率源 | ✅ 实现 |

## 2. 变量

| 命令 | 频度 | ①手册 | ②语料 | ③引擎 | 状态 |
|---|---|---|---|---|---|
| `LET` | 8,136 | `variable/let.html` | ✅ 8,136 次(窗口恒 2) | §3.17 opcode 54(`0x4468e0`);复合码在 `tag>>24` | ✅ 实现(码 0..8) |
| `INT` | 1,428 | `variable/int.html`(64bit;缺省 0) | ✅ 1,428 次 | 声明是**可执行指令**(旁线 §3.6 opcode 45 的类型回调) | ✅ 实现 |
| `FLT` | 173 | `variable/flt.html` | ✅ 173 次 | 同上 | ✅ 实现 |
| `STR` | 431 | `variable/str.html` | ✅ 431 次 | 同上 | ✅ 实现 |
| `S_INT` | 767 | `variable/s_int.html`(静态) | ✅ 767 次 | REA 分发表 → `0x4634d8`(`xor eax,eax; ret`)= no-op | ✅ no-op(DIV-RT-043) |
| `S_FLT` | 17 | `variable/s_flt.html` | ✅ 17 次 | 同上 | ✅ no-op(DIV-RT-043) |
| `S_STR` | 144 | `variable/s_str.html` | ✅ 144 次 | 同上 | ✅ no-op(DIV-RT-043) |
| `VARINFO` | 119 | `variable/varinfo.html` | ✅ 119 次(TYPE/DIMNUM/DIMSIZE/LENGTH) | §3.21(`0x456050`,判定链原序) | ✅ 部分(SEARCH 等 `Unimplemented`) |
| `VARACT` | 310 | `variable/varact.html`(**无 INIT/G_* 页**) | ✅ 310 次 + `INIT+G_*`(s12) | §3.26(`0x456050` 族 + `0x455b60` 恢复体) | ✅ 实现(除 HANTOZEN/ZENTOHAN) |
| `G_INT`/`G_FLT`/`G_STR` | 0(声明)/ 2(`VARACT[INIT]`) | `variable/g_int.html`(声明)/ ⛔(`VARACT` 变体无页) | ✅ 宏重定义 + `VARACT[INIT G_*]` | §3.26 `0x455b60` | ✅ `VARACT` 侧已实现(DIV-RT-024) |
| `F_INT`/`F_FLT`/`F_STR` | 337/72/64 | ⛔ 手册 `variable/` 无页 | ✅ 高频(全在 `global_f.yst` 声明段) | REA 默认槽 `0x459e90`:报 `スクリプト実行エラー`(yse 0xdbbd2)+ 返回 -1;600 帧运行 0 次 | ✅ 脚本错误槽(DIV-RT-043) |
| `RETURNCODE` | 396 | ⛔ 手册无页(ysc/ycd 双表无参;`kind`=行计数) | ✅ 396 次;137 文件计数不变量 | REA `0x44c390`(自动 `C/R/P` + 恒 `E` + 让出);恢复判据 `FUN_004046fc` 分模式 | ✅ 实现(2026-10-21 M-2 补行尾标志;2026-10-24 R3-fix 补恢复模式分支) |
| `WORD` | 464 | ⛔ 手册无页(`basic.html` ●テキスト表示覆盖) | ✅ 464 次(载荷=CP932 原始字节) | REA `0x458d44`(每字符写 `'1'/'2'` 到 `_TEXT_FLAG`) | ✅ 实现(2026-10-21 M-2 补标志) |

## 3. 数学 / 文本 / 查询

| 命令 | 频度 | ①手册 | ②语料 | ③引擎 | 状态 |
|---|---|---|---|---|---|
| `MATH` | 21 | `math/math.html`(SIN..RND 全表) | ✅ 21 次(SQRT/ABS/POW/ATAN/RND) | §3.20 + REA `0x448571`:RND = `min + rand32 % (max-min+1)`,MT19937 | ✅ 实现(RNG 种子注入;DIV-RT-014) |
| `TEXT` | 2 | `text/text.html`(属性表) | ✅ 2 次 | REA `0x453448`(属性槽/次序/CLICK 让出) | ✅ 实现(2026-10-21 M-2;CLICK 自动模式让出) |
| `TEXTINFO` | 0 | `text/textinfo.html` | ⛔ 语料 0 次 | REA `0x4538cc`(按参数出现查询) | ✅ 实现(2026-10-21 M-2;单测覆盖) |
| `_` | 33 | `basic/basic.html` ●テキスト表示(`_[式]` 显示式值) | ✅ 33 次(YSCom 改行宏展开形态) | §3.14 + REA `0x4567c0`(每字符 `'1'/'2'`) | ✅ 实现(2026-10-21 M-2 补 `_TEXT_FLAG` 标志) |
| `LABELINFO` | 2 | `other/labelinfo.html` | ✅ 2 次 | §3.5b(标签表查名字;strcmp 字节) | ✅ 实现(EXIST) |
| `DIALOG` | 17 | `other/dialog.html` | ✅ 17 次(STR/CAPTION/YESNO/LET/DEFBUTTON/INPUT/DEFSTR/LETSTR/OKCANCEL/YESNOCANCEL) | REA `0x43fc74`:MessageBox/自绘输入框;LET=1 基按钮序号 | ✅ 实现(VM 状态机 + 宿主回答;DIV-RT-044) |
| `TRAY` | 2 | `other/tray.html` | ✅ 2 次 | REA `0x453a4c`:托盘图标 API | ➖ 域事件(宿主;DIV-RT-045) |
| `CLIPACT` | 1 | `clip/clipact.html` | ✅ 1 次 | REA `0x43e2d0`:TEXT+SET 都存在才写 CF_TEXT | ✅ 实现(镜像;DIV-RT-045) |
| `CLIPINFO` | 0 | `clip/clipinfo.html` | ⛔ 0 次 | REA `0x43e410`:TEXT 存在才读,过滤 `<0x20`,写 LET | ✅ 实现(DIV-RT-045) |
| `ERROR` | 1 | ⛔ 手册无页 | ✅ 1 次 | REA `0x46358c`:写 `[0x689b00+opcode]` 启用表;消费者未定位 | ✅ 实现(表;DIV-RT-046) |
| `CSV`/`PLUG`/`ERRORINFO` | 0 | ⛔ 无页 | ⛔ 0 次 | REA:仅参数求值后返回 0 | ✅ no-op(DIV-RT-046) |
| `TASKINFO` | 19 | `control/taskinfo.html`(含 SCRIPTPOS 页) | ✅ 19 次 | §3.2 任务记录 + REA `0x4523fc` 逐槽;`0x44c390` 写 `task+0xc/+0x10`;SCRIPTPOS `0x452a54`/`0x450fe0` | ✅ 实现(21 槽含 A/FILE0/FILENO0/LINE0/TEXTLINE/NEXTVOICE;SCRIPTPOS 引擎字节格式;DIV-RT-025) |
| `SYSTEMINFO` | 5 | ⛔ **手册无页** | ✅ 5 次 | v494 `0x450478` 逐槽反汇编 | ⚠ 实现但手册缺页(DIV-RT-023) |
| `FONT`/`FONTINFO` | 42/— | `font/font.html` | ✅ | — | ➖ 域(图形) |
| `FILEACT`/`FILEINFO` | 20/22 | `file/*.html` | ✅ | — | ➖ 域(文件;`FILEINFO` 可查询) |

## 4. 系统变量(不是命令,但同属"已实现表面")

| 变量 | id | 频度 | ①手册 | ②语料 | ③引擎 | 状态 |
|---|---|---|---|---|---|---|
| `_LC` | 48 | 18,521 | `basic/svariable.html` + `control/loop.html` | ✅ 最高频 | §3.13 循环帧 `+0x10` | ✅ |
| `_PINT/_PFLT/_PSTR` | 53/54/55 | 12,380/2,752/4,101 | `basic/svariable.html` | ✅ | §3.16 系统变量 0x35/0x36/0x37 | ✅ |
| `_PFLG` | 56 | 30 | 同上 | ✅ | §3.16 `0x38` | ✅ |
| `_RINT/_RFLT/_RSTR` | 60/61/62 | 7,302/1,065/2,668 | 同上 | ✅ | §3.16 `0x3C/0x3D/0x3E` | ✅ |
| `_RFLG` | 63 | 0 | 同上 | ⛔ 未使用 | §3.16 | ✅ |
| `_TINT/_TFLT/_TSTR` | 67/68/69 | (TASK 参数) | ⛔ **两版手册均无页**(旧注误引已勘误) | ✅ 官方源码生产者/消费者对(`es_ERIS.yst:524→533`) | §3.16 `task+0x3d0` | ✅(2026-10-19 勘误引用) |
| `_OS_TIME` | 1 | 1,463 | `basic/svariable.html` | ✅ 全部 `(1)` | — | ✅(下标 1;`(0)` 挂账) |
| `_TIME` | 0 | 336 | `basic/svariable.html`(8 字段) | ✅ | — | ✅(真实运行宿主注入本地时间;未注入回退 1970 注入时钟) |
| `_PAI` | 10 | 0 | `basic/svariable.html` | ⛔ | — | ✅ |
| `_DEBUGMODE` | 11 | 570 | 同上 | ✅ | 工程 `projectconfig.txt` | ✅ |
| `_SYSTEM_VERSION` | 108 | 0 | 同上 | ⛔ | 工程表 version | ✅ |
| `_SCREEN_SIZE_X/Y` | 115/116 | 0 | 同上 | ⛔ | 工程 `yscfg.ybn` | ✅ |
| `_SCREEN_COLOR` | 114 | 0 | 同上 | ⛔ | `BootConfig::screen_color` 注入 | ✅ |
| `_DXMODE` | 113 | 1,333 | ⛔ 手册 0 命中 | ✅ 高频 | — | ⛔ `Unimplemented`(DIV-RT-007) |
| `_CHR` | 127 | 560 | ⛔ 手册 0 命中 | ✅ | — | ⛔ `Unimplemented`(DIV-RT-007) |
| `_MENUCODE` | 132 | 41 | `basic/svariable.html`(○) | ✅ | 内核状态 + `Vm::set_menu_code`(菜单域推入) | ✅(2026-10-19) |
| `_TEXT` | 122(0.495)/127(旧注) | (读/写) | `basic/svariable.html` | ✅ | `WORD`/`_` 追加;写替换 | ✅(2026-10-19) |
| `_TEXT_FLAG` | 123(0.495)/128(旧注) | 120 读 1 写(600 帧) | `basic/svariable.html` | ✅ | REA `0x446b93`(id 0x7b → `[state+0x3c]`);字节表见 `src/text.rs::flag` | ✅ 读(2026-10-21 M-2);写仍挂(DIV-RT-032) |
| `_CODE_*(133 项)` | 415..547 | 39 读/8 帧 | `basic/svariable.html`(○) | ✅ | REA `FUN_00427350`(VK 计数表;DIV-RT-029) | ✅(2026-10-19) |
| `_INP` | 144 | (数组) | 同上(`@_INP(@_CODE_Z)` 例) | ✅ | REA case 0x90(同表;`n<=0→0`) | ✅(2026-10-19) |
| `_WINDOW_EXEC_MAX/CLOSE` | (ycd 序) | 8 读/8 帧 | `basic/svariable.html`(○) | ✅ | 内核状态,宿主窗口推入 | ✅(2026-10-19) |
| `_WINDOW_EXEC_MIN` | 118 | 多帧(`es_scenario.yst` pc246 起) | ⛔ 手册无页(0.479/0.495 字节级) | ✅ 最小化ボタン押下状態(与 MAX 同族) | REA 0.495 getter `FUN_00427350` case 118 → `MOVSX EBP,byte [0x80191c]`;写侧 `0x446fa4` `MOV byte [0x80191c],DL`;窗口过程 `FUN_0045acdc` `SC_MINIMIZE(0xF020)` → `(char)槽号+1` | ✅ `fixed(no-manual)`(2026-10-30) |
| `_WINDOW_EXEC_CAPTION` | 120 | 600 读(无输入普查) | ⛔ 手册无页(0.479/0.495 字节级) | ✅ `es_Scenario`:窗口尺寸再初始化的一次性标志 | REA `0x45a9d0`/`0x45a9e7`(WM_SYSCOMMAND 写 `[0x801924]`) | ✅ 宿主 setter(2026-10-23 P3;DIV-RT-047) |
| `_SAVE_PATH_TYPE` | 112 | 3 读/1800 帧(`es_first.yst` pc6/9/12) | ⛔ 手册无页(0.479/0.495 字节级) | ✅ 存档路径策略 0..3 | REA getter `FUN_00427350` case 112 → `MOVSX EBP,byte [0x801324]`;写侧 `FUN_00460c80` 从 `yssfs.dat` 偏移 0x54 读(CRT fread 明文,只收 0..=3) | ✅ `fixed(no-manual)`(2026-10-30) |
| `_RET` | 49 | 1 读/1800 帧 | ⛔ 手册无页(字节级) | ✅ 恒 999(「无返回值」哨兵,与 `_INT` 门限 0x3E8 相邻) | REA getter case 49 块 = `MOV EBP,0x3E7`,不读任何全局 | ✅ `fixed(no-manual)`(2026-10-30) |
| `_KEYLAST/_MOUSELAST` | 145/146 | 0 | ⛔ 手册无页(字节级核对) | ⛔ | InputState 内部记录,读取挂号 | ⛔(DIV-RT-033) |
| `_MOUSE_X/Y` | 133/138 | 232/234 | `basic/svariable.html` | ✅ | 输入层 | ✅(注入光标) |
| `_KEY_*`/`_MOUSE_L..D` | 167..275 | 高 | `basic/input.html`(带符号计数器) | ✅ | 输入层 | ✅ |
| `_KEYNUM_*`/`_MOUSENUM_*` | — | — | `basic/svariable.html`(押された回数) | ✅ | — | ✅ |
| `_KEY_ANY` | 275 | — | `basic/svariable.html` | ✅ | — | ✅ |
| `_INP` | 144 | 477 | `basic/svariable.html`(按钮码) | ✅ | — | ⛔ `Unimplemented`(`_CODE_*` 码值无证据) |
| `_KEYLAST`/`_MOUSELAST` | 145/146 | 39/41 | 同上 | ✅ | — | ⛔ 同上 |
| `_PAD*` | 276..280 | 0 | 同上 | ⛔ | — | ⛔ 输入未接手柄 |
| `_TEXT` | 122 | 68 | `basic/svariable.html` | ✅ | 文本层 | ✅(空串;`_` 未实现) |
| `_TEXT_FLAG`/`_TEXT_NUM` | 123/124 | — | 同上 | — | — | ⛔ `Unimplemented` |
| `_SYSTEM_FRAME`/`_SYSTEM_TIME` | 109/111 | — | ⛔ 手册 0 命中 | — | — | ⛔ `Unimplemented`(DIV-RT-007) |
| 路径类 | 105..107 | — | `basic/svariable.html` | — | — | ⛔ 宿主注入(DIV-RT-010) |

## 4.5 端到端(交叉验证,2026-10-19)

| 验证 | 结果 |
|---|---|
| 官方入口链 `SYSTEM_START → es.ERIS` 连跑 5 帧(2026-10-19,文本流+TINT 落地后) | 零 Fatal;帧命令数 `[1290326, 332550, 3315, 1302401, 15331]`(FADE 任务链被激活,流程深入真实场景切换);main 未结束 = 游戏主循环仍在运行 |
| 8 帧诊断普查(同日) | 62 → 33 条/8帧;剩余 = 域命令(CG/SOUND/WINDOW…17 种,归宿主)+ 63 项手册无页挂账 |
| 散盘 vs 官方 `data.ypf` 执行轨迹 | 逐帧命令数 / 错误数 / 变量域终值摘要**完全相同**(A1 执行层) |
| 商业包 <game-b> v494(无 `YSCom.ycd`) | 启动成功,12 帧零 Fatal |
| 热路径基准(4 帧 292.9 万条,events 关) | 567k cmds/s(改前 300k;`record_events=true` 时 316k,事件流为可观测性税,可关) |

复现:`cargo run -p yu-rt --example run_eris -- <project-eris> 12`、
`cargo test -p yu-rt --test corpus eris_entry_chain_runs_end_to_end`、
`cargo test -p yu-rt --test corpus loose_and_packed_eris_are_execution_equivalent`。

## 5. 统计口径

- 命令频度来自 `crates/yu-rt/tests/corpus.rs` 的全量扫描(137 文件里实存 136 个
  `ystNNNNN.ybn`,36,974 组,80,102 个窗口);测试用**精确断言**钉住上表数字,
  语料变化会直接让测试失败。
- 「✅ 实现」= 有手册页 + 语料印证 + 引擎/旁线事实,且单测覆盖;
  「✅ 部分」= 主路径实现,个别参数/分支按 DIV-RT-* 挂账;
  「⛔」= 三层缺一,运行到即 `Unimplemented` + 事件流留痕(执行继续);
  「➖」= 语义属宿主域,运行核只发事件。
- 2026-10-21(M-1/M-2):上表频度是**静态语料**;运行期频度与逐命令状态见
  `docs/coverage-matrix.md`。两者冲突时以矩阵的实测为准,并回写本表。
  M-2 实现把 `LET` 的运行期未实现从 122 次降到 2 次。
- 2026-10-23(P3):600 帧命令 7,735,679;`LET` 未实现 1(`_SAVE_PATH`);
  `ERROR`/`_WINDOW_EXEC_CAPTION` 归零;默认槽/no-op 槽按 0.495 分发表定案
  (DIV-RT-043..048)。
