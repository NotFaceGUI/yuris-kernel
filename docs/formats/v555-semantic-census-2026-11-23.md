# v555 语义差异普查(2026-11-23)

> 证据纪律:555 只认 555 商业 exe 反编译/反汇编、555 语料自证;
> 0.495 只作对照。每条结论锚定「构建哈希 + VA + 指令/字段 + 语料计数」。
> 没有取证的地方显式写 Unknown,不猜。
>
> 原始产物:`<side-repo>/workbench/tmp/v555-decomp/<build>/functions/`;
> 0.495 对照:`<side-repo>/workbench/analysis/{handlers.txt,decomp,asm}/`;
> 6 构建分发表:`docs/formats/v555-command-handlers-all.tsv`。
>
> 关联:`docs/formats/v555-vs-495-command-diff.md`、
> `docs/formats/v555-truth-2026-11-21.md`、
> `docs/divergences/DIV-RT-069-returncode-clear-semantics.md`、
> `docs/divergences/DIV-GFX-092-webp-asset.md`。

---

## 0. 结论

**有,而且不止 TEXT/RETURNCODE 一处。** 但先要纠正既有审计的方法盲区:

1. `v555-truth-2026-11-21.md` §3 用**处理体字节大小**比较 6 个 555 构建。
   大小相同不能证明语义相同:`RETURNCODE` 在三个 555 语义族里都是
   **78 字节**,却和 0.495 的 204 字节实现语义不同(只写 `'E'` + 无条件
   清缓冲请求)。DIV-RT-069 就是从这个盲区漏出去的。
2. `v555-vs-495-command-diff.md` §4 用**参数表变化**挑同名异构项。
   参数表相同的命令也可能通过**隐式状态协议**改变行为:
   `TEXT` 的 CLEAR 执行时机、`WORD`/`_` 的逐字符表、`FILEINFO[CGFILE]`
   的扩展名表、`LET` 的系统变量写入分支,都不体现在参数表上。
3. 本次对 88 个 `real` 处理体做了 **0.495 ↔ 555 归一化反汇编相似度普查**
   (归一化寄存器/立即数/绝对地址后取前 110 条指令),
   并用全量反编译逐条核对文字/查询/文件/任务域的语义。
   结果不是"555 整体重写":15 条命令的指令形态 1.000 一致,
   大量命令 ≥0.9;差异集中在 text、查询、文件、任务边界。

本次**新确认**的语义差异/实现缺口(按影响排序):

| 级别 | 项 | 状态 |
|---|---|---|
| P0 | `FILEINFO[CGFILE]` 的 v555 扩展名表含 `.webp`,当前内核没有 | **已修(DIV-RT-070)** |
| P1 | `_` 的控制字节折空格 + 长度上限,当前内核没有 | **已修(DIV-RT-072)** |
| P1 | `TEXT[CLEAR]` 在 v555 里只于文件**成功装载后**执行;当前内核先清后开 | **已修(DIV-RT-073)** |
| P1 | `FONT[NAME/BNO/NO]` 的字体名/字体栈,当前内核只应用数值槽 | **已核对:gfx 已实现**;VM 数值镜像另议 |
| P1 | `LET` 对 `@_MOUSE_L(147)` 等系统变量的写入,当前内核挂号忽略 | **已修(DIV-RT-074)** |
| P2 | `WAIT` 的零参/零值/FRAME+TIME 优先级(0.495 与 555 相同,当前内核不同) | 待定 |
| P2 | `TASKEND`/顶层 `RETURN` 的 `task+8` 错误分支与"结束当前任务"语义 | 待定 |
| P2 | `WORD`/`_` 的 0x1FFE/0x400 边界错误 | **已修(DIV-RT-072)** |
| — | `RETURNCODE` 清缓冲请求 | **已修(DIV-RT-069)** |

---

## 1. 方法:怎样避免"同尺寸不同语义"的漏检

### 1.1 证据源

| 源 | 内容 |
|---|---|
| 0.495 SDK `yu-ris.exe` | SHA-256 `4f5eefa3544c7244a907477d89ccee97286f559df5e38b1b1fa674206981138f`;处理体入口见 `<side-repo>/workbench/analysis/handlers.txt` 与 `opcode_handlers.json` |
| v555 六构建 | `b02a4dc04ae`(2.1MB 族)、`b8772c246fe`、`baed7a05232`、`b3c02ab23bf`、`b1e3d85507b`(PC)、`b1baf88a871`(样本 1);全量 Ghidra 反编译 |
| v555 分发表 | `docs/formats/v555-command-handlers-all.tsv`(6 构建 × 121 槽) |
| 语料计数 | `docs/formats/v555-command-usage-all.tsv`、`v555-usage-*.tsv` |

### 1.2 两层比对

1. **指令形态相似度**:用 Capstone 反汇编 0.495 与 v555 的处理体入口,
   把寄存器保留、立即数归一为 `I`、绝对地址归一为 `D`、跳转/调用目标归一为 `T`,
   取前 110 条指令做序列比对。它只能证明"代码形态相同或不同",
   **不能**单独证明语义;用于缩小范围。
2. **反编译语义阅读**:对相似度低或语料高频的命令,逐字段读处理体,
   把写入的全局/任务字段列出来,再和内核实现对照。

### 1.3 全局字段穷举(防止再漏"隐式协议")

对 `b02a4dc04ae` 全量 3,681 个函数做字段引用统计:

```text
DAT_008726f4 + 0x15   4 处:RETURNCODE(0x44b824)、TEXT(0x452a0c)
                             、FUN_00469f9c(消费者)、以及 FUN_00469f9c 内的清零
DAT_00872584 + 9      1 处:RETURNCODE(0x44b824) 置 1
```

即 v555 文本缓冲的"清请求"只有 **RETURNCODE** 与 **TEXT[CLEAR]** 两个生产者,
消费者只有 `FUN_00469f9c`(清 `+0x18/+0x1c` 两块 0x2000 缓冲与
`+0x20..+0x2c` 四个游标)。当前内核两条路径都已覆盖,文本域可视为闭合。

---

## 2. 新确认:555 特有的语义差异

### 2.1 `RETURNCODE`(已修,作为方法校准)

- 三个 555 语义族处理体:`b02a4dc04ae 0x44b824`、`b1baf88a871 0x44b920`、
  `b1e3d85507b 0x44b9e4`,**均 78 字节**,指令同型:
  1. 抄当前指令首窗口的 `id/flag/slot/length` 到 `task+0xc/+0x10`;
  2. `state+0x15 = 1`(无条件清缓冲请求);
  3. 追加 `0x45`(`'E'`);
  4. `task+9 = 1`(让出),返回 `-1`。
- 0.495 `0x44c390` 是 204 字节:自动模式追加 `'C'/'R'/'P'` 再 `'E'`,
  手动模式才置清请求。
- 修法/回归见 `DIV-RT-069`。这条证明:**同签名、同尺寸、跨构建一致**
  仍可能是版本语义差异。

### 2.2 `TEXT[CLEAR]` 的执行时机(v555 特有)

- v555 `b02a4dc04ae 0x452a0c`:先打开 FILE(`FUN_0040bcdd`),
  **失败直接 `return -1`**(反编译 `00452a0c_V555H_452a0c.c:61-64`);
  成功切行写 LET 之后,才检查第三个参数 `CLEAR`:
  `*(u64*)&args[CLEAR] == 1` → `state+0x15=1; FUN_00469f9c()`(同文件 `:134-139`)。
- v555-1 变体 `b1baf88a871 0x4529d4` 同型:4 源打开/读取成功后,
  才在 `LAB_00452b85` 执行同一 CLEAR 分支。
- **已修(2026-11-23,DIV-RT-073)**:`other/handlers/text.rs` 改为
  打开失败不清;成功打开后、LET 校验前执行 CLEAR;无 `FILE` 的
  `CLEAR` 形态仍清。回归 `text_555_missing_file_does_not_clear_text_buffer`。
- 修复前后果:同一调用若 `FILE` 打不开且 `CLEAR=1`,官方保留旧文本缓冲,
  内核会先清空。语料里 `TEXT` 共 30 组,带 `CLEAR` 的只有极少数
  (`<v555-1>` s205 pc78 一类)。

### 2.3 `FILEINFO[CGFILE]`:v555 扩展名表含 `.webp`,当前内核没有

这条同时纠正 `v555-vs-495-command-diff.md` §4.1 的"最大证据空洞"。

**(a) 0.495 已经读 CGFILE。** `0x44286c`:

- 参数 0/1/2/3(FOLDER/FILE/CGFILE/SOUNDFILE)分别把 `ebx` 置 1/2/3/4;
  kind 2(CGFILE)在 `0x4428db-0x4428ed` 把 `ebx=3`;
- EXIST 分支 `0x44298a: cmp ebx,3; je 0x442c7f`;
- `0x442c7f` 先试**原路径**(追加空串 `0x80795c`),再按
  `0x8079ec ".png"` / `0x8079f4 ".bmp"` / `0x8079fc ".jpg"` /
  `0x807a04 ".gif"` / `0x807a0c ".psb"` 依次拼扩展名,
  用 `0x4436b5` 查存在性,命中写 `LET=1`,否则写 `0`。

因此 `crates/yu-rt/src/vm/handlers/file_io.rs:371` 的
`yscm.version == 555` 门控**没有语义依据**,是残留特化。

**(b) v555 把 FILE/CGFILE/SOUNDFILE 的 EXIST 统一到同一个辅助函数。**
`b02a4dc04ae 0x43eca8`(反编译 `0043eca8_V555H_43eca8.c`):

- kind 2(CGFILE)→ `cVar5='\x03'`;
- EXIST 分支对 `cVar5==2/3/4` 一律调 `FUN_0043f770`;
- `FUN_0043f770`(`0x43f770`,3355 B)先探测 6 个文件源,
  再按图像扩展名表拼名;扩展名表实测:
  `0x878a00 ".png"`、`0x878a08 ".bmp"`、`0x878a10 ".jpg"`、
  `0x878a18 ".gif"`、`0x878a20 ".psb"`、**`0x878a28 ".webp"`**。
  (0.495 同表 `0x8079ec..0x807a0c` 没有 `.webp`。)

**(c) 当前内核的缺口。**
`cg_file_exists`(`file_io.rs:497-537`)只按 `yscfg` 码
1..5(`png/bmp/jpg/gif/psb`)探测,**没有 `.webp`**;
WebP 能力只加在资产解码(`TextureStore`/`resolve_asset_size`),
没有加到 `FILEINFO`。v555 语料以 `.webp` 为主(29,502 个),
纯 WebP 按钮资产(`btn_*_off/_on/_over/_na.webp`)会在这里被判不存在。

> **已修(2026-11-23,DIV-RT-070)**:删除 `version == 555` 门控
> (0.495 同语义),并在扩展名链末尾通用追加 `.webp`
> (与 DIV-GFX-092 的通用能力口径一致);回归见
> `fileinfo_cgfile_probes_webp_without_version_gate`。

### 2.4 `WORD` / `_`:v555 删除了 0.495 的逐字符属性表

- 0.495 `_`(`0x4567c0`)在写完一个字符后,除文本与 `_TEXT_FLAG` 外,
  还写 `+0x4c/+0x50/+0x54/+0x58` 四张逐字符表与 `+0x30/+0x31`
  宽/高值(`0x4568ee..0x45693e`);
- v555 `_`(`b02a4dc04ae 0x455cd8`,478 B)只写:
  `state+0x18` 文本、`state+0x1c` 标志、`state+0x20/+0x24` 游标、
  末尾 NUL;没有上述逐字符表。
- `WORD` 同型:0.495 处理体约 412 B(入口 `0x458d44` 到下一入口),
  v555 236 B(`0x458524`),差量就是表的写入。
- 当前内核 `TextState::cells` 仍保留 0.495 风格的 width/height/delay,
  这是**内核内部渲染模型**;这些字段不在 `_TEXT`/`_TEXT_FLAG` 暴露面,
  脚本不可见。因此不构成本轮黑屏/文本错误的直接原因,但说明
  "555 构建间同尺寸"不等于"与 0.495 同状态布局"。

### 2.5 `<v555-1>` 的 `TEXT` 是 4 源回退,v555 其它族是单源

- `b1baf88a871 0x4529d4`(1404 B)在 `cVar3 < 4` 循环里按
  `stack0x1144[cVar3]` 选 4 个打开函数:
  `FUN_00440980` / `FUN_00440928` / `FUN_00440638` / `FUN_00440614`;
  成功后按 `FUN_0040c62c` / `FUN_0040bcd0` / `FUN_0040bc58` / `FUN_0040bbd8`
  读取。
- `b02a4dc04ae 0x452a0c`(837 B)只走一个 `FUN_0040bcdd`。
- 当前内核 `TEXT[FILE]` 走 `open_asset` 的通用源顺序,**没有**这 4 路
  逐步回退。4 个函数各自代表哪一层源(补丁/封包/散盘/增量)未在本次取证中
  解码,按 Unknown 保留;`v555-truth-2026-11-21.md` §4/§6.9 已挂账。

---

## 3. 已核对:参数相同、语义未变(或当前内核已对齐)

### 3.1 指令形态 1.000 一致的命令

归一化后前 110 条指令完全一致(短 handler 即整个处理体):

```text
CLIPACT  FLT  FONT  FPS  GO  IF  IFBLEND  IFBREAK  IFCONTINUE  IFEND
INPUT  LOOP  LOOPBREAK  LOOPCONTINUE  STR
```

其中 `IF/ELSE/IFEND/IFBLEND`、`LOOP/LOOPEND/LOOPBREAK/LOOPCONTINUE`、
`GO/GOSUB/RETURN`(帧内路径)与当前内核的栈/帧模型逐项对得上;
`INT/FLT/STR` 声明、`FONT` 的结构也与 0.495 相同(见 §4.3 的 NAME 缺口)。

### 3.2 高相似度(≥0.9)且已抽查语义

`TASK` 0.927、`TASKEND` 0.945、`TASKINFO` 0.945、`MATH` 0.955、
`CGEND` 0.964、`MOVIEEND/MOVIEINFO/PLUG/REGINFO/YMV/YMVEND/YMVINFO` 0.964、
`FLASH` 0.973、`LOOPEND` 0.973、`SAVEEND` 0.973、`FONTINFO` 0.982、
`MENUINFO` 0.982、`SOUNDINFO` 0.982、`LABELINFO/SOUNDEND/SYSTEMINFO/
WINDOWEND/WINDOWINFO` 0.991。差异多为编译器寄存器分配/栈帧形状,
未发现脚本可见语义变化;`TASKINFO` 的分派顺序与 v555 处理体逐项一致。

### 3.3 `VARINFO`:v555 的新增项已对齐

- `LENGTH`:v555 按前导表数**字符**,不是字节;内核
  `str_char_count_with(codec)` 同型。
- `SEARCH`:v555 未命中时在 `0x45584c-0x45585e` 显式写 `-1`;
  内核 `search_1d` 未命中返回 `-1`,一致。
- `STRFIRST/NO2`:v555 新增;内核按 `NO..NO2` 选字典序最小项,
  语料 `es.LNO.GET` 自证。
- `SJISCODE`:v555 `0x455b9c` 的 `(lead<<8)|trail`;
  0.495 处理体不读该槽;内核按 `SemanticProfile` 门控。
- `STRTYPE`:空串错误 `0x18c4a`,单字节 1、双字节 2;内核一致。

### 3.4 `TEXT` 其它细节

- 文件字节后追加 CRLF、按 CRLF/CR/LF 切行、每行尾加 `0x0D`、
  末尾两个空元素:内核 `text_file_lines` 与 `0x452a0c` 一致。
- 路径规范化只按前导表跳双字节、`A..Z` 小写、`/`→`\`:内核
  `normalize_text_file_path` 一致。
- `CLEAR` 的值判定是**精确整数 1**(低 32=1、高 32=0);
  内核 `Value::as_int()==1` 只接受 `Int`,一致。
- 缺 `FILE` 只带 `CLEAR` 时,官方清缓冲并返回 0;内核同型。

### 3.5 `TASKINFO` / `TASKEND` / `RETURN` 的帧内路径

- `TASKINFO`:v555 `0x4519bc` 的检查顺序 =
  `EXIST,E,A,Z,FILE,FILE0,LINE,LINE0,TEXTLINE,NEXTVOICE,IFNEST,
  GOSUBNEST,LOOPNEST,SCRIPTPOS`,FILENO/FILENO0 在 SCRIPTPOS 之前
  的 fallthrough 与 0.495 相同;内核 `ORDER` 与逐项取值一致。
- `TASKEND`/`RETURN` 的**有帧路径**(写返回参数、弹帧、恢复脚本位)
  与 0.495 同构;差异只在 §4.4 的顶层边界。

---

## 4. 跨版本实现缺口(0.495 与 v555 同语义,当前内核不同)

这些不是 555 新改的语义,而是本次普查顺带确认的**内核与两代官方都不一致**,
按影响列出。

### 4.1 `WAIT`(确认,语料未触发差异形态)

官方 0.495 `0x456ae4` 与 v555 `0x455eb8` 语义完全相同:

```text
FRAME 存在且 64 位值 ≠ 0 → task+0x18 = 低 32 位;return -1(让出)
否则 TIME 存在且 64 位值 ≠ 0 → task+0x1c = timeGetTime() + 低 32 位;return -1
否则 → return 0(不置任何等待,继续执行下一条)
```

当前内核 `crates/yu-rt/src/vm/handlers/control.rs:531-588`:

- 裸 `WAIT` → `wait_frames=1` + `Yield`(官方 `return 0`);
- `FRAME=0`/`TIME=0` → 仍 `Yield`(官方继续);
- `FRAME` 与 `TIME` 同时给 → 两者都设置(官方 FRAME 非 0 时**不再看 TIME**);
- 负数被 `.max(0)` 钳到 0(官方把低 32 位原样写入)。

语料值级普查(静态常量):`<v555-1>` 为 `FRAME=1` 101 组、`FRAME=2/10/60...`、
`TIME=20..1500`;`<v555-pc>` 与抽查的一个 2.1MB 样本同型。**没有**裸 `WAIT`、
`FRAME=0`、FRAME+TIME 同组、负数。故这不是当前游戏黑屏/文本错误的
原因,但 1:1 对齐仍需修。

### 4.2 `_` 的控制字节折空格与长度上限(确认,语料使用 507 组)

v555 `b02a4dc04ae 0x455cd8`(0.495 `0x4567c0` 同型):

- 单字节 `<0x20` 或 `>0x7E` → 文本缓冲写 `0x20`(空格),标志仍写 `'1'`;
  双字节前导 → 原样写 2 字节,标志写 `'2'`;
- 总长 `state+0x20 + len >= 0x1FFE` → 报 `0x18e70` 并 `return -1`;
- 源字符串长度 `>= 0x401` → 报 `0x18e8e` 并 `return -1`。

> **已修(2026-11-23,DIV-RT-072)**:`op_text_out` 先折叠再追加;
> `_`/`WORD` 按官方阈值 `0x1FFE`(WORD 允许到 `0x1FFE`、`_` 到
> `0x1FFD`)与 `_` 源串 `0x400` 报 `rt.text.out.limit` 并让出。
> 0.495 与 v555 同语义,不按版本门控。

### 4.3 `FONT` 的 NAME/BNO/NO(已核对:gfx 已实现)

v555 `0x4414b8`(0.495 `0x444428`,前 110 条指令 1.000 一致):

- `BNO` → `FUN_004039d0(值)`(文字样式槽 0..7 选择);
- `NO` → 校验 1..字体数,`FUN_00403a64/FUN_004039e0` 设置当前字体序号;
- `NAME` → `FUN_00403a6c(字符串指针)` 按名字选字体;
- 之后才是 `COLOR/SX/SY/...` 写当前样式槽(槽 0 镜像到文本状态字段)。

**复核结论(2026-11-23)**:图形层已经实现这组语义——
`crates/yu-gfx/src/state.rs::apply_font` 处理 `NAME/NO/BNO`,
`select_style_slot` 维护 8 个 `TextStyle` 槽,`TextStyle::font_request`
把 `NAME`/`NO` 交 `yu-font` 发现链;REA 注释引 `FUN_004039d0/4039e0/403a6c`。
先前"未应用"的判断只读了 VM 的 `apply_text_font` 数值镜像,漏看了 gfx 路径。
VM 的 `TextFontState` 只服务 `_`/`WORD` 的逐字符 cell 快照;它是否需要按
8 槽分离(以及 `+0x34` 默认 1 与样式槽 0 的对应)尚未取证,故本轮**不改**。

语料实证:`<v555-1>` s104 pc73 `FONT NAME=&$1164[113]`;gfx 路径接住该名字。

### 4.4 `TASKEND` 与顶层 `RETURN`(确认,2 / 20,780 组)

- v555 `TASKEND 0x45171c`(0.495 `0x4521dc` 同型):
  - 无 `ID` 且 `task+8==0` → `FUN_004518a8` 销毁**当前任务**,返回 `-1`;
  - 无 `ID` 且 `task+8!=0` → 报 `0x18c2c`,返回 `-1`;
  - 有 `ID` → 与 `"SYSTEM"` 比较、查找目标任务、与当前任务名比较;
    之后仍调用 `FUN_004518a8` 销毁**当前任务**,返回值 `-1`(同名)或 `0`。
- v555 `RETURN 0x44b59c`(0.495 `0x44c0fc` 同型)在 GOSUB 深度 0 时:
  `task+8==0` → `FUN_004518a8` + `-1`;否则报 `0x18bb4` + `-1`。
- 当前内核 `op_taskend`(`variables.rs:1468`)在 `ID` 指向其它任务时
  发 `FinishTask(target)`(结束**目标**任务),顶层 `op_return`
  (`control.rs:54`)直接 `Finish`,都不检查 `task+8`。
- 语料 `TASKEND` 仅 2 组、`RETURN` 的顶层无帧路径只在任务结束出现,
  当前游戏未暴露;但语义不是 1:1。

### 4.5 `LET` 的系统变量写入分支(确认,`_MOUSE_L` 语料有写)

v555 `LET 0x443988`(0.495 `0x4468e0` 同型)的系统变量特例:

```text
0x0f _DROPFILE_NUM   -> DAT_008724ac
0x32 _INT            -> 目标变量表写入
0x35 _PINT           -> 当前 GOSUB 帧的 int 参数数组 +0x10
0x75..0x78 _WINDOW_EXEC_MAX/MIN/CLOSE/CAPTION
0x84 _MENUCODE
0x90 _INP / 0x91 _KEYLAST / 0x92 _MOUSELAST
0x93 _MOUSE_L        -> FUN_00444750 写 64 位计数表 DAT_00663cc0
0x114..0x118 _PAD_*
0x19a..0x19e _PADNUM_*
```

当前内核 `op_let`(`variables.rs:15`)已覆盖 `_MENUCODE`、
`_WINDOW_EXEC_*`、`_INP`、`_KEYLAST`、`_MOUSELAST`、`_PAD_*`、
`_PADNUM_*`、`_CODE_*`、`_TEXT`、`_TEXT_FLAG` 与按钮计数;
`_DROPFILE_NUM`、`_INT`、`_PINT`、`_MOUSE_L` 已按 v555 分支实现
(DIV-RT-074)。语料实证:<v555-1> s70 pc410 / pc502 `LET @147 = 3`
(`@147 = _MOUSE_L`)。本次未在 s60/s61/s64/s65/s70/s104/s105/s113
中发现 `_PINT` 在 LET 左侧的写入;不能据此认为其它脚本没有。

> **已修(2026-11-23,DIV-RT-074)**:按钮族写入接 `Counter::write`
> (`FUN_00444750` 同型钳制);`_DROPFILE_NUM` 写 `drop_file_num` 镜像;
> `_INT`/`_PINT` 按变量表/GOSUB 帧写,带复合赋值;`_INT` 读侧补标量
> 元素 0。`_DROPFILE_NUM`/`_INT`/`_PINT` 只在 `other` 运行面启用。

### 4.6 `TEXT` 缺 `LET` / 打开失败时的执行顺序(部分)

- 正式语义见 §2.2(打开失败不执行 CLEAR)。
- 缺 `LET` 时官方如何处理未逐字节取证(语料 `TEXT` 30 组全部带 `LET`),
  当前内核报可恢复诊断后继续;按 Unknown 保留,不猜。

### 4.7 `LOAD[FILE DNO=1024]` 的 500 槽打包(确认,存档路径)

v555 `LOAD 0x4447c8` 文件分支对 `DNO == 0x400` 独立处理:

```text
seek 偏移表[1024] → block_off
if block_off == 0: error 0x18ede; return 0
seek block_off + 4
FUN_004453f4(): 12 字节头 + 500×(raw_len|comp_len|zlib);填内存槽
```

- 脚本实证:`es_saveload.yst` pc412
  `LOAD FILE=$2216 DNO=1024 LET=@2218`(save7xxxx / `@2213<0` 路径)。
- 该分支**不写 LET**;DNO=1024 缺失时官方报错但 `return 0`。
- 12 字节头由外层 YSSD 块头承载;`FUN_004453f4` 读到的槽表从槽 0
  的 `raw_len|comp_len` 开始,载荷里没有第二份 12 字节头。
- 该块就是 `SAVE[MEM=99999]` 的 500 槽内存包(空槽 8 字节,全空 =
  4000 字节),不是“嵌套 YSSD 容器”或“历史环”。

> **已修(2026-11-23,DIV-RT-075/DIV-RT-056)**:`SaveHost::load_mem_pack`
> + `SaveState::load_mem_pack` 按 500 槽表读取;写侧
> `SaveState::pack_mem` 生成完整块体,下一次 `SAVE[FILE]` 原样写
> DNO=1024,写盘成功后清除待写标记。系统存档不调用 `MEM=99999`,
> 不会误带该块。

---

## 5. 未闭合(明确不做结论)

1. `VARACT`(2,102 组):相似度 0.564,体量 5,290 vs 8,360 B,
   编译器差异与语义差异混在一起;仅按已知子命令(COPY/CUT/INIT/
   UPPER/LOWER)回归,未逐 29 槽对拍。
2. `CGINFO` 的 v555 新查询 `SEARCH/NUM/NAME/LSET/NO`(36 槽中 28-32):
   内核只实现 `LINT2` 等;`v555-truth` 记 31,405 条 unsupported。
3. `CG`/`CGACT` 的 v555 新参数(`SD/MD`、`SLX..RIP`、`GRADATION`):
   gfx 域是否逐项消费未取证;不影响 VM 文本语义。
4. `SOUNDINFO` 的播放游标/循环分支、`FILEACT[WEBP]` 的缓存代码 4、
   `SYSTEMMODE[WEBP]` 的全局开关:当前内核已按通用能力处理或忽略,
   未逐分支对拍。
5. `MOVIE2/MOVIE2END/MOVIE2INFO`:v555 新增,语料 0 组,未路由。
6. `MEM=99999` 的 0..499 打包槽:**已实现(DIV-RT-071)**;
   `LOAD[FILE DNO=1024]` 的 500 槽打包读取:**已实现(DIV-RT-075)**;
   `SAVE[MEM=99999]` → 下一次 `SAVE[FILE]` 写 DNO=1024:**已实现
   (DIV-RT-056/075)**;`LOAD[MEM=99999]` 官方无分支,保持未支持。
7. `EMOTE`:主人 `deferred(master)`。
8. `FUN_0043f770` 的 6 个文件源的完整优先级未逐函数解码;
   §2.3 只确认"扩展名表含 `.webp`"与"CGFILE 走该函数"。
9. `TASK`/`WAIT` 的**续帧口径**(`v555-truth` §11.6.8 的"本机多跑一帧"):
   本次未改动共享调度器;仍按既有挂账。

---

## 6. 对既有文档的勘误

1. `v555-vs-495-command-diff.md` §4 的 8 组同名异构**不完整**:
   至少缺 `RETURNCODE`(已由 DIV-RT-069 补)。
2. 同文档 §4.1/§6.2 的"`FILEINFO[CGFILE]` 是当前最大证据空洞":
   **已闭合**。0.495 `0x44286c` 明确读 CGFILE 并按图像扩展名探测;
   `file_io.rs` 的 `yscm.version == 555` 门控应删除。
3. `v555-truth-2026-11-21.md` §3 的"处理体大小相同 ⇒ 实质相同"只适用于
   **555 各构建之间**的粗略分类,不能推出"与 0.495 同语义";
   `RETURNCODE` 是反例。后续审计必须以字段/分支为单位。
4. `WORD`/`_` 的 v555 逐字符表删除未在既有文档记录;见 §2.4。

---

## 7. 复现

反编译文件(全部在 `<side-repo>/workbench/tmp/v555-decomp/`):

```text
b02a4dc04ae/functions/0044b824_V555H_44b824.c   RETURNCODE
b02a4dc04ae/functions/00452a0c_V555H_452a0c.c   TEXT
b02a4dc04ae/functions/00469f9c_FUN_00469f9c.c   CLEAR consumer
b02a4dc04ae/functions/00455cd8_V555H_455cd8.c   _
b02a4dc04ae/functions/00458524_V555H_458524.c   WORD
b02a4dc04ae/functions/00455eb8_V555H_455eb8.c   WAIT
b02a4dc04ae/functions/0044b59c_V555H_44b59c.c   RETURN
b02a4dc04ae/functions/0045171c_V555H_45171c.c   TASKEND
b02a4dc04ae/functions/0043eca8_V555H_43eca8.c   FILEINFO
b02a4dc04ae/functions/0043f770_FUN_0043f770.c   file-exists helper
b02a4dc04ae/functions/00443988_FUN_00443988.c   LET
b02a4dc04ae/functions/004414b8_V555H_4414b8.c   FONT
b02a4dc04ae/functions/004519bc_V555H_4519bc.c   TASKINFO
b1baf88a871/functions/004529d4_V555H_4529d4.c   <v555-1> TEXT(4 源)
```

0.495 对照(反汇编入口):

```text
RETURNCODE 0x44c390      RETURN 0x44c0fc      TASKEND 0x4521dc
WAIT       0x456ae4      _      0x4567c0      WORD    0x458d44
FONT       0x444428      LET    0x4468e0      TASKINFO 0x4523fc
FILEINFO   0x44286c(CGFILE 分支 0x442c7f;扩展名表 0x8079ec..0x807a0c)
VARINFO    0x456050
```

指令级复核命令(本次一次性工具,未入库):

```bash
capstone: CS_ARCH_X86 / CS_MODE_32, 从构建 exe 的 handler VA 起反汇编,
把寄存器保留、立即数归一 I、绝对地址归一 D、跳转/调用目标归一 T。
```

> 若要长期回归,建议把「归一化处理体相似度 + 字段穷举」做成
> `tools/v555/` 下的脚本;本次只保留结论与地址锚点。
