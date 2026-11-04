# 任务书 · END 全程序退出 / DIALOG 扩展参数 / 仿射绘制公式

> 状态:**END / DIALOG 已落地;仿射 scale(含浮点尺寸)已落地;推送前审查已过,半像素/官方像素对拍待补**(2026-11-04)。
> 主人指示「添加功能」后,按任务书推荐方案实现:
> `StepOutcome::ExitProgram` / `ChainReport.program_exit` /
> `Engine::restart_requested()`,以及 DIALOG 扩展参数的官方 no-op +
> `DialogRequest.ignored_params`。
> 仿射续证结论(§3.1-3.5):普通图层四角 = `(X-0.5, Y-0.5)` +
> `(FBX/10000)*tileWidth` / `(FBY/10000)*tileHeight`;`FCX/FCY` 的
> `-C/+C` 平移相消;**`FRX/FRY/FEZ/FQU` 在 0.495 绘制里零读取**;
> `FRZ/FEX/FEY/FMD` 只属主窗口/`+0xa4==2` 分支;`FSD` 属滤镜。
> 单位已定:`+0x78/+0x7c` 是**像素**宽高(`FUN_0040ae99` 直接
> `width*0.5` 写 FCX/FCY),tile 表同单位;有效缩放 =
> `FBX/10000`(默认 10000 = 1.0)。soft `draw_image` 已改 f64 尺寸 +
> 亚像素采样,soft/wgpu 共同消费 `Layer::affine_scale()`;仍未落地的是
> 官方 `-0.5` 半像素/16.16 最终像素口径(需官方截图对拍)和主窗口
> `+0xa4==2` 分支。
> 真值层级:官方 0.495 可观测行为 > SDK 0.495 REA > 原版数据自证 >
> 旧线 golden。证据对象:`<side-repo>/target/sdk/0.495/yu-ris_0495_032/システム/yu-ris.exe`
> (sha256 `4f5eefa3544c7244a907477d89ccee97286f559df5e38b1b1fa674206981138f`);
> 对照版本:`<side-repo>/target/sdk/0.479/yu-ris_0479_014/システム/yu-ris.exe`
> (sha256 `479f6bf712edeb34ed3e5d0b1d3e6287309c01d58f30cda700751fe10b6a1cc0`)。
> 参数表:ERIS 工程 0.495 全编译 `project-eris/data/ysbin/ysc.ybn`。

---

## 0. 结论摘要

| 编号 | 结论 | 证据等级 |
|---|---|---|
| A | `END` 是**全程序退出请求**:写全局退出字节 + 可选 restart 字节,不结束单个任务、不让出、不碰任务链;帧泵在下一轮直接停,窗口由宿主层 `WM_CLOSE` 关闭;restart 在进程退出时用 `CreateProcessA` 重启自身 | REA 0.495 + 0.479 静态;手册 `control/end.html` 文字一致 |
| B | 0.495/0.479 的 `DIALOG` **只读取** `CAPTION/STR/DEFSTR/LETSTR/LET/OKCANCEL/YESNO/YESNOCANCEL/INPUT/INPUTLIMIT/DEFBUTTON/ICON`;**`STR2..8`、`A*`、`E*`、`CHECK*`、`FILEOPEN`、`FILEEXT`、`FOLDER`、`CHECKBOX`、`SLIDER`、`NOWAIT` 没有任何读取点** ⇒ 官方行为 = 接受命令并忽略这些参数,不弹新控件、不写回 | REA 0.495 `0x43fc74` 全函数 + 对话框过程 `0x4403d0`;0.479 `0x439cd8` 同构;ysc 参数表 |
| C | 普通图层仿射 = `X-0.5/Y-0.5` + `(FBX/10000)*tileW` / `(FBY/10000)*tileH`;tile 表与 X/Y 同为像素,默认 10000=1.0;`FCX/FCY` 平移相消;**`FRX/FRY` 全二进制无读取点,`FRZ` 只在主窗口分支**;`FEX/FEY/FMD` 只主窗口,`FSD` 滤镜,`FEZ/FQU` 绘制不读;`F` 只 MODE 3 fade。16.16 为截断(非 round),半像素 `-0.5` | REA 0.495 全 `.text` 偏移扫描 + `FUN_00414ba4`/`FUN_0040af1c`/`FUN_0040ae99` 逐指令 + 常量池 |

---

## 1. END 全程序退出(DIV-RT-052)

### 1.1 手册

`control/end.html`(0.495/0.479 同):

- 「ユーリスの実行を終了します。」
- `RESTART=1`:「ユーリスを再起動します。」

手册语义是**整个程序**,不是「结束当前任务」。REA 与手册不冲突。

### 1.2 0.495 处理体 `0x441334`(逐条)

Ghidra 未把该入口建成 Function(只经分发表间接到达);按字节反汇编:

```text
00441334  56                   push esi
00441335  b8 0e000000          mov  eax, 0xE              ; 命令声明序号 14 = END
0044133a  e8 928cfeff          call 0x429fd1             ; 参数预扫:建 [0x684160+i] 索引
0044133f  0fbec0               movsx eax, al
00441342  85c0                 test eax, eax
00441344  7522                 jne  0x441368             ; 预扫失败 → return -1
00441346  0fb605 60416800      movzx eax, byte [0x684160] ; 参数 0(RESTART)是否存在
0044134d  85c0                 test eax, eax
0044134f  740c                 je   0x44135d
00441351  8b04c5 40376800      mov  eax, [eax*8 + 0x683740] ; 求值后的低 32 位
00441358  a2 2c158000          mov  byte [0x80152c], al   ; restart 字节(= 参数低 8 位)
0044135d  c605 6c188000 01     mov  byte [0x80186c], 1    ; 全程序退出标志
00441364  33c0                 xor  eax, eax
00441366  59                   pop  ecx
00441367  c3                   ret
00441368  b8 ffffffff          mov  eax, 0xffffffff
0044136d  59                   pop  ecx
0044136e  c3                   ret
```

结论:

1. `RESTART` 从**声明下标 0** 取(`[0x684160+0]`),值取 `[slot*8+0x683740]` 的
   **低 8 位**写入 `[0x80152c]`;参数缺席 = 0。
2. 退出请求写 `[0x80186c] = 1`。处理体**不**清任务链、**不**停泵、**不**让出,
   也**不**直接关窗;它只置全局标志并返回 0。
3. 预扫失败返回 `-1`(命令执行错误路径)。

### 1.3 退出标志的消费链(0.495)

- 主循环 `FUN_00403bfc`(`0x403bfc`)每轮读 `[0x80186c]`:
  - 为 0 → 调 `FUN_00403ee0`(输入轮询)与 `FUN_004040f8`(帧泵/渲染);
  - 非 0 → 只 `Sleep(1)`,不再调帧泵。
- `FUN_004040f8`(`0x4040f8`)开头再查 `[0x80186c]`;非 0 立即返回 1,
  **同一帧剩余逻辑帧/绘制全部跳过**。主循环收到返回值 1 后调
  `FUN_0041ae3c`(`0x41ae3c`):

  ```text
  DAT_0080186c = 1;
  *(layer+0x1a) = 2;
  PostMessageA(hwnd, 0x10 /* WM_CLOSE */, 0, 0);
  ```

  即**宿主层关窗**,不是 VM 内的任务操作。
- 进程收尾 `0x403e78`:

  ```text
  movsx eax, byte [0x80152c]
  test eax,eax
  je   0x403e8d
  mov  eax, 0x67fe00          ; 命令行/自身路径
  call 0x41cce4               ; CreateProcessA(NULL, eax, ...) → 重启自身
  ```

  ⇒ `RESTART=1` 的官方实现是**进程级重启**,不是引擎内重置;内核只应暴露
  restart 标志,不得自行拉起进程。

### 1.4 与 `TASKEND` 的区别

`TASKEND` 处理体 `0x4521dc`:

- 预扫后清 `DAT_005e05c0`;
- `ID` 缺席 → 错误 `0x18c2c`(除非 `[0x80171c+8]` 允许);
- `ID == "SYSTEM"` 或等于当前任务 → 错误 `0x18c2c`;
- 否则 `FUN_00452364` 摘链,返回 0/-1。

**没有任何全局退出写入**。因此 `END` 与 `TASKEND` 必须在调度层分开:
`TASKEND` = 任务结束;`END` = 全程序结束。

### 1.5 0.479 对照(opcode 位移)

0.495 比 0.479 多 `EMOTE`/`EMOTEINFO` 等命令,插在 `END` 之前 ⇒ 0.479 的
`END` 声明序号是 **12**,不是 14。0.479 `END` 处理体在 `0x43a619`:

```text
0043a619  b8 0c000000          mov  eax, 0xC
0043a61e  e8 be68feff          call 0x420ee1             ; 预扫
0043a62a  0fb605 20e56500      movzx eax, byte [0x65e520] ; RESTART 存在性
0043a635  8b04c5 00db6500      mov  eax, [eax*8 + 0x65db00]
0043a63c  a2 24db7d00          mov  byte [0x7ddb24], al   ; restart
0043a641  c605 9cde7d00 01     mov  byte [0x7dde9c], 1    ; 退出标志
```

与 0.495 指令形态逐条同构(仅地址/版本差异)。0.479 `0x45b484` 的
`mov eax,0xE` 空桩是**另一条命令**,不能当 END 证据。

### 1.6 实现边界(提议)

- `StepOutcome` 增 `ExitProgram { restart: bool }`(名字表达「全程序结束」,
  不叫 `Finish`)。
- `TaskChain::step_frame` 收到后:**立即停止本帧及后续 extra 帧**,把所有任务
  置 finished(不是只把当前任务当 main),`ChainReport` 增
  `program_exit: Option<ProgramExit { restart: bool }>`,并令
  `main_finished = true`(兼容现有宿主退出判断)。
- `Vm::op_end` 返回 `ExitProgram { restart }`;`restart` 按官方**低 8 位非 0**
  判定(不是整个 i64 非 0)。`VmEvent::End { restart }` 保留,仍只发一次。
- `yu-engine` 把 `program_exit` 映射到 `main_finished` 并保存
  `restart_requested()`;宿主可读,内核不重启进程。
- 现有 `taskend_without_id_ends_only_self_and_main_continues` 必须保持。
- 不采用「把当前任务标记成 main」「宿主猜 `VmEvent::End`」等补丁。

**待拍板**:`ExitProgram` 放 `StepOutcome` 还是 `Effect`;`ChainReport` 的
字段名/`main_finished` 是否同时置位(提议同时置位)。

---

## 2. DIALOG 扩展参数(DIV-RT-044 残项)

### 2.1 参数表(0.495 `ysc.ybn`,ERIS 工程)

`DIALOG` 共 50 个声明参数:

```text
 0 CAPTION     1 STR        2..8 STR2..STR8
 9 DEFSTR     10 LETSTR    11 LET        12 OK
13 OKCANCEL   14 YESNO     15 YESNOCANCEL
16 INPUT      17 INPUTLIMIT 18 DEFBUTTON 19 ICON
20 FILEOPEN   21 FILEEXT   22 FOLDER     23 CHECKBOX   24 SLIDER
25..32 A..A8  33..40 E..E8 41..48 CHECK..CHECK8 49 NOWAIT
```

槽号不写死:运行核继续按名字消费,表只作证据。

### 2.2 处理体 `0x43fc74` 全函数(Ghidra 反编译)

非 INPUT 路径:

```c
lpCaption = (CAPTION 存在) ? 字符串 : "";
lpText    = (STR 存在)     ? 字符串 : "";
uVar11    = OKCANCEL ? 1 : 0;
if (YESNO)       uVar11 = 4;
if (YESNOCANCEL) uVar11 = 3;
MessageBoxA(hwnd, lpText, lpCaption, icon | defbutton | uVar11);
/* 按 IDOK/IDCANCEL/IDYES/IDNO 写 LET:1/2/1/2;YESNOCANCEL 的 Cancel=3 */
```

INPUT 路径:

```c
DAT_00801ce4 = INPUTLIMIT 值(槽 17);
DAT_00801cd4 = CAPTION; DAT_00801cd8 = DEFSTR; DAT_00801cdc = STR;
DAT_00801ce0 = malloc(0x1000);
DialogBoxParamA(hInstance, "IDD_YSI", hwnd, FUN_004403d0, 0);
/* OK → LET=1;LETSTR 由 FUN_0043f0d8 写入(DEFSTR 只在模板初始化时用) */
```

槽位读取点(绝对寻址,全函数):

```text
0x43fc9f idx 11 LET        0x43fcc6 idx 10 LETSTR
0x43fce5 idx  0 CAPTION    0x43fcf7 idx  1 STR
0x43fd09 idx  9 DEFSTR     0x43fd1b idx 16 INPUT
0x43fd2a idx 18 DEFBUTTON  0x43fd70 idx 19 ICON
0x43fdce idx 13 OKCANCEL   0x43fde2 idx 14 YESNO
0x43fdf2 idx 15 YESNOCANCEL 0x43fefb idx 17 INPUTLIMIT
0x43fe7a/0x43ff6d idx 11(LET 写回) 0x440001 idx 10(LETSTR 写回)
```

- **槽 12 `OK` 从不读取**(缺省 MB_OK),与现有实现一致。
- **槽 2..8、20..49 零读取**。
- 对话框过程 `0x4403d0` 只读 `0/1/16/15/18/19/17/14`(控件 1000/1001/1002
  与 `INPUTLIMIT`),同样不读扩展参数。

### 2.3 0.479 对照

0.479 `DIALOG` 处理体 `0x439cd8`(声明序号 10),读取点集合与 0.495 相同:
`0,1,9,10,11,13,14,15,16,17,18,19`;`STR2..8`/`20..49` 零读取。

### 2.4 陷阱记录(避免误判)

在 `0x4403d0..0x441334` 区间做槽位扫描,会看到下标 `2..8、20..23` 的读取;
这些属于紧随其后的 **`EMOTE` 处理体 `FUN_004405a4`**(coverage-matrix 的
EMOTE 行;它读自己的 ID/层属性/网格参数),**不是** DIALOG 的扩展参数。
判断必须以「该命令自己的处理体 + 其对话框过程」为边界,不能用整段地址
区间的汇总。

### 2.5 结论与实现边界(提议)

- 官方 0.495/0.479 对扩展参数的行为 = **接受命令、完全忽略**:
  - 不求值(预扫只建索引,不执行表达式);
  - 不生成控件、不写回、不改变按钮/`LET`/`LETSTR`;
  - `NOWAIT` 不产生非模态行为;`STR2..8` 不产生多行文本;
  - `FILEOPEN/FOLDER/CHECKBOX/SLIDER/A*/E*/CHECK*` 不产生文件框/复选/滑杆。
- 内核不得再因这些参数整条 `Unimplemented`;改为识别名字后跳过求值。
- **不需要新增宿主端口**(文件选择/复选/滑杆)。B7 的文件选择框是独立宿主
  能力,不由 DIALOG 参数驱动。
- 事件事实:建议在 `DialogRequest` 增 `ignored_params: Vec<String>`(仅记录
  名字,不含值);不新增 `VmEvent` 变体。若主人希望 trace 可见,可改为
  `DialogRequest` + 一条 `Diag`(非 `Unimplemented`)。
- 更新 `domain-event-contracts.md §8.1` 的 2..8/20..49 行为
  `unsupported-by-official`;`DIV-RT-044` 残项归零;coverage-matrix DIALOG 行
  改为「基础参数 + 扩展参数对齐 no-op」。

**待拍板**:`ignored_params` 放 `DialogRequest` 还是新增 trace 事件;是否
接受「0.495 无扩展参数 UI」这一结论(即不实现 STR2..8/文件框等假功能)。

---

## 3. 仿射绘制公式(DIV-GFX-069 §5.1)

### 3.1 字段消费图(全二进制偏移扫描,2026-11-04 续证)

初版把 `FRX/FRY/FRZ` 当成普通图层的旋转矩阵来源。对 0.495 全 `.text`
做「层偏移 → 读取点」扫描(capstone,disp32)后推翻:

| 字段 | 偏移 | 读取点 | 结论 |
|---|---|---|---|
| X / Y | `+0x1b0/1b8` | `0x414bed/0x414bf9`(FUN_00414ba4);`0x4140a2/0x4140ae`(主窗口分支) | 普通图层位置 |
| FBX / FBY | `+0x1c0/1c8` | `0x414c05/0x414c18`(网格);`0x414062`(主窗口 scale=FBX/10000) | 普通图层缩放 / 主窗口缩放 |
| FRX / FRY | `+0x1f8/1fc` | **只有** `0x414c24/0x414c37` 的载入,之后无任何读取 | 0.495 绘制**不消费**;普通图层与主窗口都不用 |
| FRZ | `+0x200` | `0x414c4a` 载入 + `0x414075`(主窗口) | 只有主窗口/`+0xa4==2` 分支按角度用 |
| FCX / FCY | `+0x1d0/1d8` | `0x414c5d/0x414c70` + 网格平移调用 `0x415462/0x41551b` | 网格里只作平移枢轴(见 §3.4) |
| FEX / FEY | `+0x1e0/1e8` | `0x414031/0x41403b`(主窗口分支) | 只主窗口;普通图层不读 |
| FEZ | `+0x1f0` | 无绘制读取(仅 COPY 属性块) | 0.495 绘制不消费 |
| FSD | `+0x204` | `0x40e109`(图像加工/滤镜函数) | 滤镜,不是几何 |
| FMD | `+0x208` | `0x413f6f`(主窗口 effect) | 主窗口/滤镜 |
| FQU | `+0x20c` | 无绘制读取(仅 COPY 属性块) | 0.495 绘制不消费 |
| F | `+0x214` | `0x416bcb`(MODE 3 HLSL `YS_FadeVal`) | 效果 fade,不是几何 |

⇒ **普通图像图层的仿射只有 X/Y + FBX/FBY + FCX/FCY 平移枢轴**;
`FRX/FRY/FEZ/FQU` 在 0.495 绘制里是死字段;`FRZ/FEX/FEY/FMD` 属于
主窗口/`+0xa4==2` 分支;`FSD` 属于图像加工/滤镜。

### 3.2 常量池(0x495 只读)

```text
0x58eb98 = 100.0        0x58eba0 = 0.5
0x58eba8 = 10000.0      0x58ebc0 = 100.0
0x58ebc8 = 0.0          0x58ebd0 = 0.5
0x58ebd8 = 65536.0      0x58ebe0 = 1/65536
0x58ebe8 = 0.25         0x58ebf0 = 1.0
0x58ebf8 = 0.017453292519943295(π/180)
0x58ec00 = 1/256        0x58ec08 = 256.0
0x58ec10 = 128.0
0x58ef40 = π(f32)       0x58ef44 = 180.0(f32)
```

### 3.3 16.16 与半像素(已确认)

- 位置/缩放/中心一律 `i64 / 100.0`;角度 `f32` 在主窗口分支按
  `*π/180/100` 转弧度。
- 顶点坐标先做 `-0.5`(半像素),再 `ROUND(v * 65536)` 后拆成 16.16。
- 反编译里的 `ROUND` 实际是 **FPU 控制字 OR 0xC00(截断/朝零)**,
  随后对负数 `+0xFFFF` 修正,再 `shrd/shld/shl` 拆高低位;不是
  round-to-nearest。实现必须用「截断 + 负数修正」,不能用 `f32.round()`。
- UV 反算:`fixed * (1/65536) / 尺寸 * 0.25`(0.25 是引擎纹理图集
  4×4 的采样缩放,不是几何)。

### 3.4 普通图层网格:四角公式(已逐指令确认)

`FUN_0040af1c` 是网格表构造器:
- `+0xa6` = **X 方向 tile 数**,`+0xb0` = 每 tile 宽度表;
- `+0xa7` = **Y 方向 tile 数**,`+0x130` = 每 tile 高度表;
- 按 0x2000/0x1000/0x800/0x400/0x200/0x100/0x80/0x40/0x20/0x10 阈值
  切分;主窗口/`+0xa4==2` 时直接写屏宽/屏高单 tile。
(旧口径「部件×分块」应更名为「X tile × Y tile」。)

`FUN_00414ba4` 的网格循环(逐指令 + x87 栈修正后):

```text
x0 = X/100 - 0.5
y0 = Y/100 - 0.5
对每个 Y tile:
  对每个 X tile:
    x1 = x0 + (FBX/10000) * tileWidth
    y1 = y0 + (FBY/10000) * tileHeight
    // 4 个顶点:(x0,y0) (x1,y0) (x0,y1) (x1,y1),triangle strip 5 顶点
    // 坐标先按 ROUND(v*65536) 拆 16.16,再 DrawPrimitiveUP(5,2,0x54)
    x0 = x1  // 同行右移
  y0 = y1    // 下一行
```

- 平移矩阵只做 `-FCX/-FCY` 再 `+FCX/+FCY`(0x415462 / 0x41551b);
  没有旋转矩阵参与 → 两段平移**相消**,`FCX/FCY` 在普通图层上无净效果。
- `0x4ca3fd` 的实参是 `(Y-0.5, FCX, FCY)`(x87 栈修正后),不是
  `(FRY,FRZ,FRX)`;初版「D3DXMatrixRotationYawPitchRoll」读法作废。
  `0x4ca416` 是 7 参的矩阵 compose 函数(scale/center/rotation/offset),
  不在普通网格路径上被调用。
- 主窗口/`+0xa4==2` 分支(`0x413f6f..0x4140f0`)才是完整 2D 变换:
  `FBX/10000` 缩放、`FRZ*π/180/100` 旋转、`X/100`/`Y/100` 平移、
  `FEX/FEY` 半像素偏移、`FMD` effect;该分支不写普通 CG 层。

### 3.5 仍未取证 / 落地缺口

**单位已定**(2026-11-04 追加):`FUN_0040ae99` 把 `layer+0x78/+0x7c`
直接写 `DAT_00801dd0/dd4`,并 `FCX = width*0.5`、`FCY = height*0.5`;
`FUN_0040af1c` 用同一对数切 tile。⇒ `+0x78/+0x7c`、`+0xb0/+0x130`、
`FCX/FCY` 都是**像素**;有效缩放 = `FBX/10000`(10000 = 1.0)。

| 子项 | 现状 | 补证方式 |
|---|---|---|
| soft 渲染器的整数绘制尺寸 | **已落地**:`draw_image` 改 f64 尺寸 + 亚像素采样;整数尺寸仍走原快路径 | 已完成(2026-11-04) |
| `+0xa4` 层类型(`==2` 主窗口分支) | 只确认该分支用 FRZ/FEX/FEY/FMD;普通 CG 不走 | 运行时层类型 dump |
| 主窗口变换 `0x414854..0x4148e4` 的矩阵合成顺序 | 函数族已定位,参数已见;顺序待逐函数确认 | 反编译 `0x414854/488/8b4/8e4` + 微场景 |
| FSD 滤镜(`0x40e109`)语义 | 只确认读取点 | 反编译该函数 |
| 官方像素对拍 | 未做(本机官方引擎无可枚举窗口,运行时 dump 不可用) | 主人桌面/官方采集环境 |

### 3.6 实现边界(按续证修正)

- **已落地(2026-11-04)**:soft/wgpu 共用
  `w=(FBX/10000)*baseW`、`h=(FBY/10000)*baseH`;soft `draw_image`
  已改 f64 尺寸 + 亚像素采样,任意比例不再取整;wgpu 的 quad 同步。
  夹具:`affine_scale_scales_layer_quad`、`affine_scale_matches_soft`。
- **未落地**:`-0.5` 半像素(官方顶点初值)与 16.16 截断的最终像素口径
  需官方截图对拍;当前先保证 scale 与既有默认画面不变。`FCX/FCY` 不参与
  普通图层(平移相消),`FRX/FRY` 不参与(全二进制无读取)。
- `FCX/FCY` 普通图层无净效果(平移相消),不因非默认值丢弃图层。
- `FRX/FRY/FEZ/FQU` 按 `unsupported-by-official`(0.495 绘制不读)归档;
  `FRZ/FEX/FEY/FMD` 归主窗口分支,另案;`FSD` 归滤镜,另案。
- **移除**「非默认 affine → 整层 `LayerStatus::Unimplemented`」;
  未实现字段单独出诊断,已实现部分照常绘制。
- 默认值必须逐像素不变;`FX/FY` 仍写 `transform.x/y`;`X/FX` 后写覆盖先写
  的顺序保持;不碰 `+0x68/+0x1b0` 双字段(§5.2)。

---

## 4. 契约裁决点(2026-11-04 已按推荐项落地)

> 主人指示「添加功能」,裁-A/裁-B 采用推荐形状;裁-C 已按续证落地
> `FBX/FBY` 缩放(soft/wgpu 共用),半像素/16.16 仍 open。以下保留原
> 裁决文本供回查。

### 裁-A · END 的退出结果

提议:

1. `StepOutcome` 增 `ExitProgram { restart: bool }`;
2. `ChainReport` 增 `program_exit: Option<ProgramExit>`(含 `restart`),
   同时 `main_finished = true`;
3. `yu-engine` 暴露 `restart_requested()`,不自行重启;
4. `VmEvent::End` 保留,不新增宿主侧「猜 End」逻辑。

需要主人确认:是否同意上述形状;`ExitProgram` 放 `StepOutcome`(推荐,
因为它是命令返回值)还是 `Effect`。

### 裁-B · DIALOG 扩展参数的宿主契约

提议:

1. 官方不消费的 2..8/20..49 一律「接受 + 跳过求值 + 不报错」;
2. `DialogRequest` 增 `ignored_params: Vec<String>`(只记名字);
3. **不新增**文件框/复选/滑杆/非模态宿主端口;
4. `DIV-RT-044` 残项按 `unsupported-by-official` 归档。

需要主人确认:是否接受「0.495 无扩展参数 UI」;`ignored_params` 放
`DialogRequest` 还是改为 trace 事件。

### 裁-C · 仿射逐字段状态

提议:`Affine` 增 `AffineFields` 位标记;后端按字段应用,不再整层丢弃。
此项只动 `yu-gfx` 公共结构与 display_list golden,不动宿主;若主人认为
也应先拍板,一并确认。

---

## 5. 验收命令(实施后执行)

```bash
cargo build --workspace --all-targets
cargo test --workspace --all-targets
cargo test -p yu-rt --test fixture end_
cargo test -p yu-rt --test fixture dialog_
cargo test -p yu-gfx --test transform
```

新增用例(先红后绿,注释写证据锚点):

- `end_in_child_task_stops_program`:主任务建子任务,子任务
  `END[RESTART=0]` ⇒ 同帧后不再执行任何命令,`main_finished=true`,
  `program_exit.restart=false`;后续 tick 命令数为 0。
- `end_restart_uses_low_byte`: `END[RESTART=1]` / `RESTART=256` 的 restart
  分别为 true / false(低 8 位语义)。
- `taskend_without_id_ends_only_self_and_main_continues`:保持通过。
- `dialog_extended_params_are_ignored_without_eval`:命中 `STR2/FILEOPEN/
  FOLDER/CHECKBOX/SLIDER/A/E/CHECK/NOWAIT` 时不报 `Unimplemented`、不弹窗、
  `LET/LETSTR` 不变;`ignored_params` 记录名字,扩展槽不求值。
- 仿射微场景(已落地部分):`CG` 图像给 `FBX/FBY` 缩放、负 `FBX` 镜像;
  soft/wgpu 逐像素一致;默认值零差异。`FCX/FCY`、`FRX/FRY/FRZ`、
  `FEX/FEY` 的像素对拍仍归 DIV-GFX-070 半像素/主窗口分支。
- 原 `gfx.affine.unverified` 整层丢弃的层恢复绘制,无新 `Unimplemented`。

已知环境红:Android 音频若仍失败,注明「非本轮引入」,不掩盖。

---

## 6. 台账变更清单(实施时)

- `crates/yu-rt/docs/divergences/README.md`:DIV-RT-052 → `fixed`;
  DIV-RT-044 扩展参数行 → `unsupported-by-official`/aligned no-op。
- `crates/yu-rt/docs/coverage-matrix.md`:END 行、DIALOG 行。
- `docs/divergences/DIV-GFX-069-cg-affine-binding.md`:§5.1 状态与
  字段拆分结论。
- `docs/gfx-unimplemented-todo.md`:DIV-GFX-003/069 相关行。
- `docs/render-parity.md` §7:绘制公式口径、FEX..F 的「非绘制字段」结论。
- `crates/yu-rt/docs/domain-event-contracts.md` §8.1/§8.2:扩展参数
  `unsupported-by-official`;`ignored_params` 契约。
- `PROGRESS.md`:每项完成追加「成果 N」,含命令与原始输出、跳过计数。

---

## 7. 复跑取证(只读)

```bash
# 0.495/0.479 END、DIALOG 全函数反编译(本任务产出的 Ghidra headless 设施)
# 见 target/tmp/ghidra/out/func_441334* / func_43fc74.c / func_4521dc.c / func_414ba4.c
# 槽位扫描(0.495 基址 0x684160 / 0x683740;0.479 基址 0x65e520 / 0x65db00)
python <repo>/tools/rea/scan_cg_slots.py <sdk495>/システム/yu-ris.exe 0x43fc74 0x4403d0
# 参数表
cargo run -q -p yu-rt --example probe_ysc_params -- <project-eris> DIALOG,END,TASKEND
```
