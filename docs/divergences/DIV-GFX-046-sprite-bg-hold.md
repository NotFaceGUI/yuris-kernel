# DIV-GFX-046 · ERIS sprite 系统的 boot logo 保持/淡出(BG 提升)

- 状态: **fixed(J3)**
- 优先级: 高(title boot 动画与时间锚点)
- 发现日期 / 方式: 2026-10-21 / 官方 boot 逐帧截图 vs 内核逐 tick 渲染
- 相关: EVID-J3-02(`docs/title-motion-ledger.md` §9.6)、DIV-GFX-006/045

## 现象

官方启动 0.2–2.0 s 显示 Firstia logo(`cg/bg/logo`)的菱形 wipe,
随后保持 1.2 s、用 800 ms 淡出,再淡入 title。内核在修复 EVID-J3-02
后能渲染 wipe(tick 13–59),但 tick 63 起 logo 被
`CGEND SPLBUF401/SPL401/SPLTR401` 删除,屏幕全黑到 tick 224,
title 首次出现约 3.7 s(官方约 2.2–2.5 s)。

## 证据

- 官方 boot:`target/tmp/j3-official-boot/frame_0001..0020.png`
  (`capture_screen.py --warmup 0 --frames 20 --interval 0.1`);
  frame_0008 为完整 Firstia logo。
- 脚本:`ERIS/UserScript/SCENARIO_START.txt`:
  `\BG(logo, 200, 800, 23, , , 1)` → `\WA(1200,,1,1)` → `\FOUT(800, 23)`
  → `\TITLE(@L(4001), -1)`。
- sprite 缓冲约定:`es_Sprite.yst` 的 `$LID="SPL"`、`$LIDBUF="SPLBUF"`、
  `$LIDTR="SPLTR"`、`$LIDTRBUF="SPLTRBUF"`。
- 内核状态(tick 62):`SPLBUF401=cg/bg/logo`、`SPLTRBUF401=cg/tr/ysr023`、
  `SPL401` 可见 T=256;tick 63 三者被 CGEND 删除。
- 官方/内核 wipe 完成帧对齐:tick 61 vs frame_0008,4×4 采样平均通道差 0.13。

## 根因(2026-10-22 追加,EVID-J3-03/04)

不是缺 sprite 渲染器。官方 sprite 系统本身就是脚本
(`ERIS/Sprite/es_Sprite.yst`),每帧翻译成 `CG`/`CGACT` 命令;内核缺的是这些
命令的低层语义:

1. `MATH3D`(REA `FUN_00448900`,`0x448900`)未实现 ⇒ `es.SP.XYZCALC` 返回的
   缩放 `zx/zy` 恒为 0;
2. `CGINFO[SX/SY]` 对 `Asset` 纹理返回 `gfx.info.asset_size`(无资产元数据
   provider)⇒ `es.CGACT_SIZE` 读到的 `SX/SY` 为 0;
3. 于是 `SIZE<1` ⇒ `CG[ID=$dst X=999999999 Y=999999999]`(屏幕外哨兵)
   ⇒ 提升出来的 BG 层永远不可见,tick 63 后再被 `CGEND` 删除 = 黑屏。
4. `CGACT[SIZE/SIZE2]` 本身也未实现(此前标 Unimplemented)。

实装后 tick 13–59 菱形 wipe、tick 63–139 保持、tick 139–190 反向 wipe 淡出全部
由脚本自身驱动,无需新增契约。

## 结论与处置

- **已修**:可见 wipe(EVID-J3-02);保持/淡出(EVID-J3-03/04:
  MATH3D + `CGINFO[SX/SY]` 资产头 + `CGACT[SIZE]`)。
- **不需要**“过渡完成后提升为 BG”的新契约:脚本用
  `SP.BGCOPY`(`CGACT[COPY2]`)+ `CGACT[SIZE]` + `CGACT[RECTPAINT3]`
  自己完成 BG 提升,官方也没有额外机制。
- **已撤回的固定实现**:中途曾在 `yu-gfx` 硬编码 `SPL/SPLBUF/SPLTR/SPLTRBUF`
  做缓冲层配对(EVID-J3-02)。那是脚本命名,不是引擎语义(EVID-T-10);
  `CGACT[SIZE]` 落地后由脚本自己写像素,特判已成多余。现为纯状态驱动,
  600 帧对拍与 boot 时间线不变。
- 附加修复(EVID-J3-05/06/07):
  - 过渡取样必须用**图层当前像素**(脚本先对配对层做 `CGACT[REVERSE]`,
    读资产原图会方向镜像);
  - `COPY ID=:/W0/` 读**上一帧后缓冲**,不是本帧组内容;
  - wgpu 快照必须**冻结**(命令段每帧重放 `CopyOp`);
  - 主组直绘(DIV-GFX-008):主窗口组不再经离屏 RT,否则淡入淡出被再乘一次
    组 alpha(实测 100→77)。

## 复核

```bash
# 官方 boot
python tools/official_trace/capture_screen.py --exe <project-eris>/yu-ris.exe \
  --cwd <project-eris> --out target/tmp/j3-official-boot \
  --frames 20 --interval 0.1 --warmup 0 --no-send-input
# 内核 boot(soft)
YURIS_FIRST_FRAME_TICK=61 cargo run -p yu-backend-soft --example first_frame
```

---

## 补记(2026-10-24,`<game-b>` v494 片头 LOGO 黑屏回归)

### A. 架构事实复核(与本文档定案一致)

实测本作 `s66 es_sprite.yst` 里的图层名来自**脚本字符串变量**,不是内核常量:

```text
var 0x0d44 = "SPL"          ; 前缀
var 0x0d45 = "SPLBUF"
var 0x0d46 = "SPLBUFa"
var 0x0d47 = "SPLBUFb"
var 0x0d48 = "SPLTR"
var 0x0d49 = "SPLTRBUF"
var 0x0d43 = ["cg/", "cg/bg/", "cg/ev/", "cg/tr/", "cg/stand/"]   ; 资产目录表
```

⇒ **再次确认:这些是 ERIS sprite 库的命名约定,不是引擎语义**。
f0 的 `es.SPA.ST.DEF.SET`(s64 pc135)把 `yuu_1a01` 等 32 项写进 SPA 表,
同为脚本侧动作。内核只需正确执行 `CG`/`CGACT`/`CGINFO` 低层命令。

### B. 本次定位到的实际断点(与编码无关)

`cg/ev/サークル.jpg`(293,724 B,**包内实存**)已在修好 CP932 后正确解析:

```text
CG[ID="SPLBUF1" Z=1 FILE="cg/ev/サークル"]      ; s65 pc99,纹理已装载
CG[ID="SPLBUF1" E=0 A=0 X=0 Y=0 Z=1 MODE=0]    ; s66 pc3442,隐藏+透明
CG[ID="SPLBUF1" TEX=0]                          ; s66 pc3523
```

随后 `s66 pc3527 GOSUB es.SPA.DEF.SEARCH("サークル")` 返回**空**
(`pc3528 IF #0=0` 判真 → 跳到 3552),因此 `SPL1` 只执行
`CGACT[ID="SPL1" RECTPAINT3 SET=0]`(清成透明),没有任何像素来源。

`es.SPA.DEF.SEARCH`(s64 pc176–204)的查找逻辑:
- `pc187 LOOP $0e14`(表项计数,实测 32);
- `pc194 IF $fd==$cf[i][0] && $fd==$cf[i][1]` —— 用 `_strleft(目标名,1)` 的
  **首字符**与表项首字符做两字符比对;
- 表项内容实测为 `yuu_1a01`/`yu_1a01`/`yu１a`/`yu１b`/`yu２a`…(`0x0dcf`),
  全部是 `yu` 开头 ⇒ **`サークル` 不在 SPA 表里,查空是脚本数据本身的正常结果**。

### C. 因此剩下的疑点(下一个取证点)

`s66 pc3443 IFBLEND` 之后,`SPL1` 的像素来源应该是 `pc3444~3502` 那段循环
(建 `SPLBUF401..4NN` 分块层 + `COPY2` 汇入)。但实测该段**被跳过**,
因为 `pc3432 IF #0=1` 判真、内层 `pc3433 IF #0=0` 判假走了 ELSE(`pc3440 LOADCG`),
`pc3443 IFBLEND` 正是 IF 体出口 —— 即此分支下**官方同样不会建 SPLBUF401**。

⇒ 待查:此分支下 `SPL1` 的像素应由谁提供。候选:
1. `pc3523 CG[ID=SPLBUF1 TEX=0]` 的 `TEX` 槽语义(我们目前按"无纹理操作"处理);
2. `CGACT[ID=SPL1 ...]` 的后续动作(实测只有 `RECTPAINT3 SET=0`);
3. `es.SP.ANM.` 族在别帧的 `COPY2`。

这三项都属低层命令语义,需按 REA 逐个取证,不引入 sprite 专用渲染器。

### D. 已验证的低层命令清单(本作 v494)

| 命令 | 实测形态 | 内核状态 |
|---|---|---|
| `CG` | `ID/Z/E/A/X/Y/FILE/TEX/MODE` 等;`FILE` 未给 ⇒ 像素不动;`FILE=""` ⇒ 生成 1×1 像素(REA `0x40c5f8`,四字节全 0) | 部分(见 DIV-GFX-060) |
| `CGACT` | `ID`=源、`ID2`=目标(手册确认);`COPY2` 拷图像 | ✅ |
| `CGEND` | 摘层 | ✅ |
| `CGINFO` | `SX/SY` 读资产尺寸 | ✅ |

### E. `CG` 扩展槽(手册无页,需单独取证)

本作 `ysc.ybn` 的 `CG` 有 45 槽,手册 `cg/cg.html` 只列 12 个。手册**没有**的词:
`IDNO/TSX/TSY/MIPMAP/TEX/DXBUF/DXDRAW/FX..FID/F/REMALLOC/RID/MID/MFILE/T/TID/TIDNO/TA/MODE/CASH/LINT/LINT2`。

其中 s66 片头实际用到 `TEX`:

```text
CG[ID="SPLBUF1" TEX=0]     ; s66 pc3523
```

`TEX` 在 v494 的语义**未取证**;0.495 已定案为「表面类型码」(`2` = E-mote,
见 `DIV-GFX-011`/`DIV-GFX-069`),v494 是否同码未复核。
下一个取证点:在 `CG` 处理体里定位 `0x684160+12`(槽 12 = TEX)的引用,
读它写的层字段,再判断它是否参与"把 SPLBUF1 的像素交给 SPL1"。

### F. `CG[TEX]` 槽取证结论(2026-10-24,REA)

`TEX` = `CG` 槽 12 ⇒ 存在标志 `0x68416c`。

```text
0x42b072  movzx ecx, byte[0x68416c]   ; TEX 压缩序号
0x42b079  test ecx, ecx
0x42b07b  je  0x42b092                ; 未给 ⇒ 下一槽
0x42b07d  mov esi, [ecx*8+0x683740]   ; TEX lo
0x42b084  dec esi                     ; -1
0x42b085  or  esi, [ecx*8+0x683744]   ; | hi
0x42b08c  je  0x42baa5                ; ← 仅 TEX == 1 时进入
```

`0x42baa5` 分支(`TEX == 1`):

```text
0x42baa5  movsx ecx, byte[0x8012e4]   ; 全局开关
0x42baac  test ecx, ecx
0x42baae  je  0x42b092                ; 关闭 ⇒ 退回主流程
0x42bac3  mov byte[ebp+0xa4], 1       ; 层标志位置 1
0x42bacd  mov [esp+0xc], 0x100
0x42bad7  mov ecx, 0x200
0x42badc  call 0x40ae99               ; 像素合成/缩放
```

⇒ **`TEX=0` 不是特殊值**:只有 `TEX=1` 触发合成路径。
本作片头的 `CG[ID="SPLBUF1" TEX=0]`(s66 pc3523)**不产生任何像素操作**,
与内核当前"无操作"处理一致 —— **该槽不是 logo 不上屏的断点**。

### G. 剩余待查(诚实登记)

已排除:CP932 编码(DIV-RT-051 fixed)、`FILE=""`(DIV-GFX-060)、
`CG`/`CGACT` 的 ID/ID2 方向、`TEX` 槽、`SPA` 表(脚本数据本身不含 `サークル`)、
`IFBLEND` 块结构(判定为真,官方亦不建 `SPLBUF401`)、`RETURNCODE` 让出(已修)。

仍在查:`SPL1` 于该分支下的像素来源。下一步候选(按概率):
1. `es.SP.ANM.` 族(s64 `pc336~357`)在建 `SPL*` 动画层时的 `COPY2`;
2. `CGACT[ID="SPL1" RECTPAINT3 SET=0]` 之后的同帧动作(实测当前只有这一条);
3. `CGINFO[SX/SY]` 返回 0 导致后续 `IF` 全跳过(实测 `pc3508 IF` 判假)。

### H. 断点定案(2026-10-24):**同帧内 CGEND 清理循环晚于 sprite 重建**

单帧内实测顺序(f86,`s66 es_sprite.yst`):

```text
pc3399  LOOP → CGEND SPLBUF1..100     ; 清空 sprite 缓冲族
pc3437  CGACT[ID=SPLBUF401 ID2=SPLBUF1 COPY2]   ; 目标不存在(见 G,正常)
pc3442  CG[SBLBUF1 E=0 A=0 …]         ; 重建 SPLBUF1(已正确持有 cg/ev/サークル)
pc3523  CG[SPLBUF1 TEX=0]             ; 无操作(见 F)
pc3527  es.SPA.DEF.SEARCH("サークル")  ; 返回空(脚本数据本身无此项)
pc2044  CGEND SPLBUF1                 ; ← **本帧末尾又被删掉**
pc2045  CGEND SPLBUF2 …
```

`pc2043 LOOP` + `pc2044..2049` 是 `es.SP.INFO.INIT` 的清理段(同 s66,入口
`es.SP.DEL.SET`/`es.SP.DELALL.SET` 族),它对**整族** `SPLBUF*` 无条件 CGEND。

⇒ 结果:`SPLBUF1` 在 f86 帧末消失,下一帧 `CGINFO[SX/SY]` 拿不到尺寸、
`pc3508 IF [SX==1 && SY==1]` 判假、`SPL1` 只剩 `RECTPAINT3 SET=0`(清透明)。

**为什么官方没这个问题**:`pc3399` 与 `pc2044` 是同一族的两次清理,
官方必然在**清理之后**才重建;我们的执行序把「重建」放在了两次清理之间。
待查:官方这个 script 的 `LOOP` 边界 / 调用嵌套是否与我们的 `LOOP` 语义一致
(尤其 `LOOPBREAK`/`LOOPCONTINUE` 在本段的实际走向)。

下一步取证(最小):在 f86 完整打印 `pc2035..2060` 与 `pc3390..3450` 的**全部**
Jump 事件,核对两次清理各自的循环边界是否与官方同形。

### I. `pc2044` 的真实归属(更正 H 的推测)

`pc2043 LOOP SET=100` 的循环体是 `pc2044..2050`(每轮 CGEND 一族层),
其**调用者**是 `pc2066 GOSUB` 进入的 `pc2035`(`es.SP.DEL.SET`),
而 `pc2035` 又由 `pc2296 GOSUB → pc2057`(`es.SP.DELALL.SET`)串起。

⇒ 这是**显式删除 API**(`es.SP.DEL` / `es.SP.DELALL`),**不是每帧清理**。
f86 内实测次序:

```text
pc3399 → CGEND SPLBUF1..100        (pc2066/pc2035 路径? 需按 f86 编号核对)
pc3440 → LOADCG 重建 SPLBUF1
pc2296 → GOSUB es.SP.DELALL.SET → pc2057 → pc2035 → pc2044 CGEND SPLBUF1
```

即:**同一逻辑帧内,"重建"与"删除"两条路径都被执行,且删除在重建之后**。

### J. 这是不是内核缺陷 —— 尚不能断言

两种可能,需下一步区分:

1. **官方同帧也这样跑**(脚本每帧先建后删,真正显示发生在下一帧或别的缓冲),
   那就不是内核问题,`SPL1` 的像素来源在别处(如 `es.SP.ANM.` 族);
2. **我们的任务泵/循环边界让本该分帧的两段挤进了同一帧**
   (例如 `LOOP` 的 `LOOPBREAK` 未按官方语义提前退出,导致多跑一轮删除)。

区分方法(下一步,最省):在官方引擎上对本作捕一份 `--seconds 6` 的
事件级 trace(工具 `tools/official_trace/probe_dispatch.py`,已在库),
筛 `es_sprite.yst` 的 pc 序列,与内核 f86 的序列并排比对。
**在此之前不修改执行序**(无证据的"修"可能就是下一步的回归源)。

### K. 定案(2026-10-25):§H/§I/§J 全部作废,真正断点在 `VARINFO` 槽号

**先撤销错结论。** §H/§I/§J 引用的 f86 命令序列来自
`examples/probe_trace_script`(裸 `Vm`,**未挂 `GraphicsHost`**)。该形态下
`op_info` 对 `CGINFO/WINDOWINFO/FONTINFO` 一律 `report_unimplemented` 并
**不写回 `LET`**,脚本读到全 0 ⇒ 走的是假分支。挂宿主重跑后:

```text
f86  s66  pc3399  CGEND SPLBUF1..100      ← 每帧清缓冲（正常）
f86  s66  pc3440  GOSUB es.SP.PRO.LOADCG  ← 重建 SPLBUF1 = cg/ev/サークル
f86  s66  pc3442  CG SPLBUF1 E=0 A=0 Z=1  ← 缓冲层本就不可见
f86  s66  pc3523  CG SPLBUF1 TEX=0        ← 已证无操作（§F）
```

`pc2044 CGEND SPLBUF1` **一次都没有发生**。§H 的"同帧建完就删"是探针
自身造成的假象。

> 教训(工具层):凡涉及 `es_Sprite.yst` / `es_EF*` 这类**按 INFO 返回值
> 分支**的脚本,裸 `Vm` trace 不可用。库内新增
> `crates/yu-render/soft/examples/probe_host_cmds.rs`(挂宿主、按
> sid/pc 区间过滤)与 `probe_host_vars.rs`(挂宿主导出变量)、
> `probe_host_query.rs`(记录每次 INFO 提问与答案)作为唯一可信口径。

#### K.1 官方 v494 事件级 trace(定案证据)

v494 的分派点与 0.495 不同(`0x44cb`/`0x4537`,比 0.495 早 `0x10`;
可用 `ff 14 b9` = `call [ecx+edi*4]` 在 `.text` 里唯一命中定位)。
v494 的 CG handler = `0x42a4a8`、CGACT = `0x42c438`(由 hits 的
handler 直方图与 ysc 表序交叉确认)。

对同一帧(官方 t≈9.50–9.54 s)实测 CG 序列:

```text
pc245/263/264   es.EF.TASK.MAIN 的 DXBUF/CP01/CP02
pc3442          CG SPLBUF1            ← 与内核同址、同序
pc3523          CG SPLBUF1 TEX=0
pc3572/3573     CG SPL1 + RECTPAINT3  ← 与内核同址、同序
pc5533 ×100     CG[A=0]（es.SP.DRAW 内）
pc5582/5583     CG[E=1] / CG[E=0]     ← ← ← 官方执行了,内核 0 次
pc5659/5660/5714 CG 位置/尺寸
pc6026/6028/6056 CG[REMALLOC] / CGACT
```

**官方同帧里有 `es.SP.DRAW`(pc5533→pc5582 让 sprite 可见),内核一次
都没调用。** §H 的「先建后删」与 §J 的「两种可能」都不成立:差异不在
执行序,而在**进入 `es.SP.DRAW` 的判据**。

#### K.2 根因:`VARINFO[SEARCH]` 的槽号被写死

`es.SP.DRAW` 只在 `pc2925/pc2931` 判 `@3394[slot] == 1/2` 时被调用;
`@3394` 的唯一非清理写入是 `es.EF.TASK.MAIN` 的 `pc507`:

```text
pc502  LET @3414 = @48 - 1                       ; @48 = _LC
pc503  CGINFO[ID="SPLBUF"+(slot+1) EXIST LET=@3415]
pc504  IF [@3415 == 0 && @3414 != 3 && @3414 != 5]
pc505    LOOPCONTINUE
pc507  LET @3394[@3414] = 1                      ; ← 只有走到这里才登记
```

`@3415` 恒为 1,是因为 `CGINFO[EXIST]` 对**槽号未映射**时也该返回 0 ——
但真正让整条链断掉的是更上游的一步:`es.LNO.GET`。

```text
es.LNO.GET(name):
  pc1919  INT  @3755 = 0
  pc1920  VARINFO[SEARCH=1 SET=$3153 STR=@_PSTR(1) NO=1 LET=@3755]
  pc1921  IF [@3755 + 1 == 0] → 报「未找到」
  pc1924  RETURN @3755
```

`$3153` = `["BGF","LNO_FD","BG","LNO_CM","BGF",…]`,`"BGF"` 在
**下标 4**。官方 `SEARCH` 返回 4;内核返回 `-1`。

原因:`VARINFO` 的参数表在两个版本里**整体位移**:

| 参数 | SDK 0.495(ERIS 语料) | 商业包 v494 |
|---|---|---|
| SEARCH | 14 | 14 |
| INT | 17 | **15** |
| FLT | 18 | **16** |
| STR | 19 | **17** |
| NO | 20 | **18** |

旧代码在 `array_needle` 里写死 `(Str,19)|(Float,18)|(Int,17)`,
并在 `pc503` 用 `flags_get(&flags, 20)` 取起始下标。在 v494 上这两个
判断都落空 ⇒ `needle=None` ⇒ `found=-1` ⇒ `es.LNO.GET("BGF")` 判
「未找到」⇒ `@3743`(槽号)恒 0 ⇒ `es.SP.SET.SET`/`es.SP.GO.SET`
登记到槽 0 而非 3 ⇒ `@3394` 全 0 ⇒ `es.SP.DRAW` 从不调用 ⇒ **sprite
全不上屏**。

#### K.3 修复

`array_needle` 改为按**参数名**解析:`VARINFO` 的针值槽 =
`params.iter().position(|n| n == "STR"/"FLT"/"INT")`,起始下标 =
`position(|n| n == "NO")`。参数名跨版本稳定,槽号不稳定。

复核(挂宿主实测,同一帧):

| 指标 | 修复前 | 修复后 |
|---|---|---|
| `$3153` 查 `"BGF"` 的结果 | `-1` | `4` |
| `@3743`(槽号) | `0` | `4` |
| `@3270[3]`(槽→层号) | `0` | `4` |
| `es.SP.DRAW` 调用次数(f84–f86) | 0 | 9 |
| `CGINFO[EXIST "SPLBUF1"]` | 1 | 1 |

单测:`fixture::varinfo_search_resolves_slots_by_param_name`(合成工程
改用 v494 参数表,断言 SEARCH 仍命中;写死数字会让它红)。
台账:`DIV-RT-053`。

#### K.4 仍未收口(诚实登记)

修复后 `@3394` 在**帧末**仍读到 0,而 `pc507` 执行时确实写入了 ——
说明同一帧后面还有清零点(`pc2940 LET @3394[@48-1]=0` 在
`es.SP.DRAW` 的收尾段)。该清理是否该在**同一帧**发生,需与官方
`pc2940` 的时间位置对拍才能定;在此之前不动执行序(与 §J 同一纪律)。
当前可见效果:标题画面(f300)正常、无调试 UI 残留、`CGINFO` 语义正确。

### L. 收口(2026-10-25):`CGACT[SIZE/SIZE2]` 的参数必须按浮点读

§K 修好 `VARINFO` 之后 `es.SP.DRAW` 会被调用了,但画面仍全黑。渲染报告给出
最后一段:

```text
skip image x2  gfx.imageop.region.partial   :/W0/SPL401
```

同一个 `CGACT` 在内核里的实参(`s66 pc6028`,`es.CGACT_SIZE` 内部):

```text
CGACT[ID=SPLBUF401 ID2=SPL401 SIZE=1
      X=0 Y=0 X2=1100 Y2=824.5      ← X2/Y2 是**浮点**
      SX=1101 SY=825.5]             ← SX/SY 也是浮点
```

根因:`cgact_image` 用 `find_int` 读 `X/Y/X2/Y2/SX/SY`,而
`EngineValue::as_int` 要求 `fract() == 0`。`824.5`/`825.5` 因此被静默判成
「参数未给」:

1. `Y2` 丢失 ⇒ 区域只剩 3 个字段 ⇒ `gfx.imageop.region.partial`;
2. `SY` 丢失 ⇒ `gfx.imageop.size.missing`。

`SIZE` 是**缩放后的实际像素范围**,除以 2 的缩放在本作里产生 `.5` 是常态
(`es_Sprite.yst` 的 `es.CGACT_SIZE` 每次都用 `源尺寸 × 比例`,比例含 0.5)。
所以这不是脚本的怪值,是这条链的正常输入。

修法:

| 参数 | 读法 | 理由 |
|---|---|---|
| `SX`/`SY` | `find_f64` → `round() as u32` | 缩放尺寸,浮点是常态;纹理像素只能取整 |
| `X`/`Y`/`X2`/`Y2` | `find_px`(`as_f64().round() as i64`) | 像素边界;旧 `find_int` 丢 `.5` 会让区域字段数不齐 |
| `ImageOp.sx/sy` | `Option<f64>` | 记录原值必须保真(`.5` 不能丢) |
| JSON 输出 | `opt_f64` | 同上 |

后端(`yu-backend-soft` / `yu-backend-wgpu`)同步改成浮点判定
(`<= 0.0`)后取整,与 `yu-gfx` 写层尺寸同口径。

**实测(同一台机器、同一语料、逐帧截图):**

| 帧 | 修复前 | 修复后 |
|---|---|---|
| f90(LOGO 淡入中) | 全黑,`skip image x2 region.partial` | `通行止め` LOGO 可见,A≈8/256 |
| f140(LOGO 完成) | 全黑 | 灰底 + 红黑书法 LOGO,与官方 `frame_0020` 同构图同底色 |
| f300(标题) | 正常(一直正常) | 正常,`ops 79/79 drawn, 0 skipped` |

引用计数:`ops 83 total, 83 drawn, 0 skipped`(f90),此前是
`79 total, 77 drawn, 2 skipped`。

任务书中「不得以 `Unimplemented` 为终态」在此处的落地方式即:两个
`gfx.imageop.*` 跳过原因都不是「语义未取证」,而是**参数被误判为缺失**,
属实现缺陷而非证据缺口 —— 修正读取口径即可,不需要新证据。
