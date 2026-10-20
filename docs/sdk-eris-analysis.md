# SDK 脚本系统一手分析(ERIS 范本)

> 依据: 官方 SDK 0.495(`yu-ris_0495_032`)一手资料 —— `data/script/**/*.yst` 71 个源脚本、
> `マニュアル/YU-RIS/html/` 官方手册、`system/YSCom/YSCom.ycd` 编译器语言定义表(二进制实读)、
> 以及 REA/Ghidra 12.1.4 对 `YSCom.exe` 的取证。旁线笔记只作索引,结论以一手来源为准。
> 证据标注: [yst]=源脚本原文 [手册]=官方手册页 [ycd]=语言定义表 [REA]=二进制取证。

## 1. 工程与文件结构

```text
<project>/                      ← 含 yu-ris.exe 的目录即项目根
├── yu-ris.exe  yscfg.dat       ← 引擎 + 引擎配置(128B i32 数组)
├── system/ysconfig.txt         ← 系统级设置(纯文本,只允许设置项)
├── <data>/                     ← PROJECTFOLDER[](默认 ./data)
│   ├── script/
│   │   ├── system_start.yst    ← 入口: 标签 #SYSTEM_START → GO[#=es.ERIS] [yst]
│   │   ├── ERIS/               ← 71 个 .yst: global.yst / macro.yst / es_*.yst / stdlib/
│   │   ├── UserDefine/ UserDesign/ UserScript/
│   │   └── ysbin/              ← 编译产物(未编译时不存在)
│   └── bgm/ cg/ cgsys/ cgsys2/ config/ pac/ se/ voice/
```

编译产物布局(YSCom.exe 内字符串模板 [REA]): `ysbin\{yscfg,yse,ysc,ysl,ysv,yst,yst_list,ysi%05d,yst%05d}.ybn`;
报错文案确证 `ysl.ybn`=标签信息、`ysv.ybn`=变量信息。

## 2. 语法(一手样例归纳)

| 构造 | 形式 | 实例 [yst] |
|---|---|---|
| 命令 | `CMD[KEY=VAL KEY2=VAL2]` | `CG[ID=":/W3/" SX=720 Z=100]` |
| 标签 | 行首 `#NAME`,全局唯一跨文件 | `#SYSTEM_START` `#=es.ERIS` |
| 块 | `{` … `}` 与 IF/LOOP 连用 | `IF[@_DEBUGMODE>=2] { … } IFEND[]` |
| 注释 | `//`;`///x` 为标记注释 | `///xあとで纏める` |
| 宏调用 | `\NAME` / `\NAME(args)` / `\NAME[...]` | `\es.S.GO("ES.FIRST",0,0)` |
| 数值/串变量 | `@name` / `$name` | `@es.STATE` `$S.LBL.INIT` |
| 赋值糖 | `@A=5` `@A+=10` `@A/=2` `@A%=5`;串拼接 `+` | `$S.LBL.INIT = $es.S.LBL+".INIT"`(可带空格) |
| 字面量 | 十进制/负数/`0x` 十六进制;i64 最值直接出现 | `0x808080` `9223372036854775807` |

变量种类([手册] variable 页 + global.yst):全局 `G_INT/G_FLT/G_STR`(带 2/3/4 恢复组变体)、
局部 `INT/FLT/STR`、静态 `S_INT/S_FLT/S_STR`(**声明本身是可执行指令**,作用域=声明点→文件末尾,
es_ERIS.yst 开头 `{}` 块即静态声明区)、系统变量 `_` 前缀。数组 ≤8 维 `G_STR[$es.VOICE.FILE(11)]`、
`G_INT[@es.SELAUTO(200,100)]`,可带初值 `G_INT[@es.SMV.SX=400]`。`INT`=i64、`FLT`=f64。

控制流命令(control/ 分类):`GO GOSUB RETURN LOOP LOOPBREAK LOOPCONTINUE LOOPEND WAIT
IF ELSE IFEND IFBREAK IFCONTINUE TASK TASKEND TASKINFO END`。
`WAIT[FRAME=n]` 或 `WAIT[TIME=ms]`;`END[RESTART=@_PINT(1)]`。

函数式调用面(ERIS 大量使用):`\_gosub(label)`、`\_label.info.exist($s)`(结果进 `@_RINT(1)`)、
`\return[]`、用户函数 `\es.S.GO(...)` —— 本质是宏/GOSUB 包装,返回值走 `@_RINT/@_RFLT/$_RSTR`。

## 3. 宏系统(other/macro.html [手册] + macro.yst [yst])

- 定义**仅允许在 `macro.yst`**:`MACRO[NAME~NAME8 STR="…" DEF~DEF10 DEFB~DEFB10 PRE~PRE10
  PREB~PREB10 SUF~SUF10 SUFB~SUFB10]`(NAME2..8 一名多义;DEF 族=参数缺省;PRE/SUF 族=前后缀缺省)。
- **编译期文本替换**——替换产物语法错即编译错误 ⇒ `.ybn` 内不存在宏。
- 参数:`\NAME(a,b)` 按位置填 `$_M`,`$_M2`…(上限 `$_M16`);B 参数族 `$_MB~`。
  省略规则:`\ABC(,20)` 空位、`\ABC` 全省;缺省填 DEFn。
- 调用形式:`\NAME[...]`;**后随全角字符或行尾时可省括号**([手册] basic/macro)。
- 系统宏:`\r`(换行) `\p`(翻页) `\c`(点击等待)——文本演出基础件。
- ERIS 的宏层=游戏高层 API:`SAVE/LOAD/END/DATE/CHARNAME/SS.GO/…` 全是 `GOSUB` 包装宏;
  连 `G_INT` 都被重定义(`macro[name=G_INT str=";G_INT2[$_M];"]` ⇒ global.yst 写 G_INT,
  编译产物实为 G_INT2 [yst]);`name2=` 提供小写别名(如 `PAUSE name2=pause`)。
- 编译器如何存宏:仅 `macro.yst` 参与展开 ⇒ 展开发生在单文件上下文,顺序敏感。

## 4. 命令集与参数模型(YSCom.ycd [ycd] + GOSUB 页 [手册])

- `YSCom.ycd`:魔数 `YSCD`,版本 `0x1EF(=495)`,头部计数 `119`(含义未定),内含
  **105 个 `COM_*`** 命令名与参数表(参数名 + 2×u16 元数据,如 `X:(256,0)` `FILE:(1,1)`);
  另有系统变量 535 项、设置项 34 项、参数关键字 886 个(与旁线导出数一致,本次自一手二进制复核)。
- **GOSUB 传参**(调用约定核心):`PINT/PFLT/PSTR` 1..16,**编号跨类型共用**
  (`PFLT=` 与 `PSTR=` 同为 1 号 ⇒ 冲突编译错);接收端系统变量 `@_PINT(n)/@_PFLT(n)/$_PSTR(n)`,
  `@_PFLG(n)`=该号是否传过;`RETURN[RINT=…]` → `@_RINT(n)`/`@_RFLG(n)`;每层 GOSUB 独立保存;
  嵌套上限 **64**。`_TINT/_TFLT/_TSTR`=任务局部寄存器(ycd)。
- 命令语义的权威=手册逐条页(18 分类),编译器 COM_* 是全集(105 ⊇ 78 文档化)。

## 5. 推进模型 = 任务与帧(basic/task.html [手册] + es_ERIS.yst [yst])

- 启动生成**系统任务 `SYSTEM`**,自入口脚本起执行;任务=「脚本执行者」。
- **让出点只有两个:WAIT 命令、显示文本**;让出后休 ≈0.016s;
  **所有任务都休眠时才画一帧**;任一任务运行期间完全不渲染。
- 任意时刻实际只有一个任务在跑(高速轮换,非并行)。
- `TASK[ID E Z # SCRIPTPOS]`:任务层=可持久启停的执行流(E=0/1),RETURN(无 GOSUB 嵌套)自动消亡,
  `SCRIPTPOS=$var` 即脚本位置的存/取(=剧本推进的存档锚点)。
- `FPS[SET=1..256 SKIP=0/1]`:**帧率由脚本设定**。
- ERIS 主循环骨架(es_ERIS.yst):全局状态串 `$es.S.LBL` → 约定标签 `.INIT/.LOOP/.END`;
  LOOP 里每帧 `GOSUB[#=es.BT.GOSUB.EXEC]`(按钮轮询)+ 状态 `.LOOP` + `WAIT[FRAME=1]`;
  `$es.S.LBL` 非空即状态切换。**这是"ERIS 引擎侧"的推进循环,游戏剧本是它的被调方**。

## 6. 输入/输出(basic/input.html + svariable.html [手册])

- **按键状态=带符号帧计数器**:`@_KEY_A` 按下当帧起 1,2,3,… 逐帧递增;松开当帧起 -1,-2,-3,… 递减。
  `==1` 即"本帧按下";`>0` 持续按住;`>=60` 按住一秒。另有 `@_KEYNUM_*(累计次数)`、
  `@_MOUSE_L/R/M`、`@_KEY_ANY`、`@_INP(n)` 数组版、手柄钮。键位表覆盖 A–Z/0–9/小键盘/F1–F24/
  编辑键区/符号键(svariable.html 全表)。
- 文本输出:`TEXT[OUTPUTMODE=0]` 手动模式(es_First.yst 要求最先执行);`\r \p \c` 系统宏;
  `$_TEXT`/`$_TEXT_FLAG` 读当前显示内容。
- 文件:`FILEACT[CREATE=1 FOLDER=$es.SAVEPATH]` 建目录;`FILEINFO` 查询;路径系统变量
  `$_SAVE_PATH/$_SYSTEM_PATH/$_APPDATA_PATH/$_PERSONAL_PATH`;命令行 `@_ARGC/$_ARGV(0..8)`。
- 系统:`@_TIME(t,f)` 日期时间、`@_OS_TIME`、`@_OS_VERSION`、`@_SCREEN_SIZE_X/Y`、
  `@_DEBUGMODE`、`@_SYSTEM_VERSION`(×1000) 等。

## 7. 对 yuris-kernel 的架构结论

1. **宏是必需前端**:ERIS 的整个 API 面建立在宏上(G_INT→G_INT2 证明必须展开后才见真命令)。
   形态:编译期展开器(单遍、顺序敏感、DEF 缺省、全角省括号规则),产物=无宏的命令流。
2. **VM 调用帧=16 槽寄存器模型**(PINT/PFLT/PSTR × 16 + RINT/RFLT/RSTR × 16 + PFLG/RFLG +
   任务局部 TINT/TFLT/TSTR),参数编号跨类型共用——与 ycd 证据一致,不要自创。
3. **帧调度器**:任务集合,每任务跑到 WAIT/TEXT 让出;全员让出→渲染→推进一帧;
   FPS 由脚本决定。ERIS 主循环证明"每帧按钮轮询+状态机+WAIT[FRAME=1]"是标准用法。
4. **输入层对齐目标=带符号帧计数器**,不是 bool,也不是事件流——这是与旧实现最关键的差异点。
5. **命令分派按名字**(ycd 表),跨版本(0.479/0.495/商业 v555)只对表不对号。
6. 脚本位置存取(`TASK[SCRIPTPOS]`)= 剧本推进存档的原生机制,存档设计围绕它。

## 8. 附:YSCom 启动链与编译产物(本次实测打通,2026-10-19)

**已成功**:官方 YSCom 0.495 编译 ERIS 全部 143 个产物,`project-eris/data/ysbin/` 就绪,
官方引擎直接启动并显示标题画面(验收通过)。

协议(逆向 + 实测双证):

- 映射1 `"YU-RISCompilerFileMapObject"`(≥0x200,无空格):
  `+0x00 state0`(编译器附着后写 1)、`+0x04 state1`(启动器建好映射2后写 1)、
  `+0x08/+0x0C state2/3`(完成标志)、`+0x10 产物数`、`+0x14 总字节数`、
  `+0x80 u32 引擎版本`、`+0x84 项目路径`。
- **项目路径必须带尾部反斜杠**(实测:缺了会拼成 `project-erissystem\ysconfig.txt`)。
- 映射2 `"YU-RISCompilerFileMapObject2"`(大小=+0x14)由**启动器**创建;条目格式
  `{u32 nameSize(含NUL,带路径), name, u32 fileSize, data}`。
- 启动:`CreateProcess(YSCom.exe ys)`,CWD=项目根;**PowerShell `Start-Process`(ShellExecuteEx)
  不会把 argv 传给这个 VC6 程序 → 静默退出;必须 CreateProcess 语义**。
- 非日文系统下 YSCom 读取 CP932 文件名会损坏(`キ?ラ...` → 读文件失败):
  要么用 Locale Emulator(`LEProc.exe -runas <guid> <exe> ys`,且 CWD 设为 YSCom 所在目录以找到
  `YSCom.ycd`),要么把语料文件名改为 ASCII(本次采用后者,映射清单在 workbench/rename-manifest.json)。

**磁盘 .ybn 形态**(RxYuris 参考实现 + 实测):

- `yst*.ybn`:魔数明文 `"YSTB"`+版本(本次 495);**代码/参数段各自从密钥下标 0 重启异或**,
  密钥 = CRC32("Yu-ris") = `0x96AC6FD3`,字节序 LE(`D3 6F AC 96`):
  V5(>300)四段 = inst index / args index / args data / line numbers;
  V2(200<v<300)= code seg + args seg。
- 元数据文件各有独立魔数:`YSCM`(ysc 命令表)/`YSCF`(yscfg)/`YSER`(yse)/`YSLB`(ysl 标签)/
  `YSTD`(yst)/`YSTL`(yst_list)/`YSVR`(ysv 变量镜像),版本同为 0x1EF。

**发布形态(已打通)**:官方 YSPac 打 `data/` 的**内容**(不带 `data\` 前缀)→ `data.ypf` 放游戏根;
`SYSTEMMODE[DEBUGMODE=0]` 下引擎走单包挂载、直接读包内 ysbin、无编译。完整流程与实测见
`docs/eris-corpus.md`;YPF 布局实证见 `docs/formats/ypf.md`。
