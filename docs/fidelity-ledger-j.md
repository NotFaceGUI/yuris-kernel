# 首帧保真台账 J（取证强制版）

> 任务书 J。**本文件先于任何实现代码产生。**
> 每一行语义点必须四件套齐全才可标 `Confirmed`；缺任意一件 ⇒ `Uncertain`，
> 只挂账、不实现。代码注释用 `EVID-J-xx` 反查本表。
>
> 真值层级：官方引擎可观测行为 > SDK 0.495 二进制逆向 > 原版数据自证 >
> 旧接手线 golden（只作回归参考）。

## 0. 证据源与哈希

| 项 | 值 |
|---|---|
| 官方引擎 | SDK 0.495 `yu-ris.exe`，sha256 `4f5eefa3544c7244a907477d89ccee97286f559df5e38b1b1fa674206981138f` |
| 官方主数据 | `<project-eris>/data.ypf`，sha256 `5744a7bea69c0daa1e4559dfbebf25a937d1132154083c64fb217af3f0694690` |
| 官方首屏基准 | `target/tmp/fidelity-ref/frame_0001..0006.png`（800×600，6 帧逐像素相同；`official_screen.jsonl`） |
| 官方微场景 A/B | `target/tmp/fidelity-micro4/`（源 `data/script/micro.yst`，YSCom 0.495 编译）；截图 `target/tmp/fidelity-micro4-ref/frame_0004.png`；编译产物/截图 sha256 清单 `target/tmp/fidelity-micro-manifest.json` |
| 官方微场景 alpha | 同上；R2/R4 = A=128 半透明层，R1/R3/R5 为不透明对照 |
| 反编译树 | `<side-repo>/workbench/analysis/decomp/`（Ghidra 12.1.4） |
| 手册 | `<side-repo>/target/sdk/0.495/yu-ris_0495_032/マニュアル/YU-RIS/html/`（CP932） |
| 真实脚本 | `<project-eris>/save/data/script/`（CP932；grep 需 `-a`，文件含 0x85 行分隔） |
| D3D9 头 | `C:/Program Files (x86)/Windows Kits/10/Include/10.0.22621.0/shared/d3d9types.h` |

### 0.1 官方微场景编译/采集命令（可复现）

```bash
# 1) 复制官方 SDK 工程到仓库外工作区（不入库）
#    <repo>/target/tmp/fidelity-micro4 由 project-eris 的 save/data + system 复制而来
# 2) 写 data/script/micro.yst（内容见 §4），并在 system_start.yst 把 GO[#=es.ERIS] 改为 GO[#=MICRO]
# 3) YSCom 编译（项目协议；输出 data/ysbin）
<side-repo>/workbench/tmp/cargo-target/release/yscom_harvest.exe \
  <repo>/target/tmp/fidelity-micro4 \
  <repo>/target/tmp/fidelity-micro4/system/YSCom/YSCom.exe 495
# 4) 官方引擎截图
python tools/official_trace/capture_screen.py \
  --exe <repo>/target/tmp/fidelity-micro4/yu-ris.exe \
  --cwd <repo>/target/tmp/fidelity-micro4 \
  --out target/tmp/fidelity-micro4-ref --frames 4 --interval 0.4 --warmup 5
```

> 注意：该工程副本保留 `data/script` 源码时，官方引擎启动期会再做一次脚本解析；
> `CG[Z=0]` 会触发解析错误窗口（已实测）。微场景的 `Z` 必须 ≥1。

---

## 1. Phase A · soft alpha（EVID-J-01）

| ID | 语义点 | ① REA 地址 + 原文摘录 | ② 手册/文档句 | ③ 脚本样本 | ④ 截图路径 | 结论 | 决策 |
|---|---|---|---|---|---|---|---|
| EVID-J-01 | D3D9 未开 `SEPARATEALPHABLENDENABLE` 时，alpha 通道使用与 RGB 同一组因子；`SRCALPHA` 因子是 `(As,As,As,As)`，故 MODE 0 alpha 源项 = `As²` | `FUN_00414ba4`(0x414ba4) MODE 0：`iVar10 = (**(code **)(*DAT_00801d5c + 0xe4))(DAT_00801d5c,0x13,5);` / `...,0x14,6);` / `...,0xab,1);`（反编译行 445/450/455）；MODE 1：`0x13,5` / `0x14,2` / `0xab,1`（行 492/497/502）；MODE 2：`0x13,5` / `0x14,2` / `0xab,3`（行 543/548/553）。`FUN_00462e40` 行 160-166：`puStack_68 = (undefined *)0x1b;` + 错误串 `"D3DRS_ALPHABLENDENABLE Error."`。反编译全树无 `SetRenderState(...,0xce,...)`（`grep -rn ",0xce,"` 0 命中） | D3D9 头 `d3d9types.h`：`D3DBLEND_SRCALPHA = 5`、`D3DBLEND_INVSRCALPHA = 6`、`D3DBLEND_ONE = 2`；`D3DRS_SEPARATEALPHABLENDENABLE = 206` 注释 `TRUE to enable a separate blending function for the alpha channel`（默认 FALSE，未写入即同一组因子） | `es_ERIS.yst:1061` `IF[@Mode==0] CG[ID=$IDName A=@mA]`（A 写入路径）；受控 `micro.yst` R2/R4 用 `RECTPAINT2=1 SET=0x80` 产生 A=128 纹理 alpha | `target/tmp/fidelity-micro4-ref/frame_0004.png` R2 中心 `(190,70)=(16,160,48)`、R4 `(430,70)=(144,160,48)`（与 MODE 0 RGB `src·As+dst·(1−As)` 逐值吻合） | **Confirmed** | 修 soft：MODE 0/1/2 alpha 源项 `a → a²`；同步测试/README；不动 gfx 契约 |
| EVID-J-01b | MODE 0/1/2 的 RGB 因子 | 同上 `0x13=5`(SRCALPHA)、`0x14=6/2`、`0xab=1/1/3`；`FUN_00414ba4` 行 391-462/463-509/510-560 的 TSS 序列 | 手册 `cg/cg.html`「通常 α 混合」；可见公式 `src·a+dst·(1−a)` | 首屏 `MODE=0` 1015 次 / `MODE=2` 1 次（`docs/render-parity.md` §2） | 同上 R1/R3/R5 | **Confirmed** | 保持 soft/wgpu 现有 RGB 公式 |

> EVID-J-01 的截图只证明 RGB；alpha 通道在 PNG 中因屏幕清屏不透明而不可见。
> alpha 源项的判定依据是 ①+②（状态语义），③/④证明同一帧的 RGB 混合因子与 A 输入。

---

## 2. Phase B · RECTPAINT 族（EVID-J-02..05）

### 2.1 语义与处理体

| ID | 语义点 | ① REA 地址 + 原文摘录 | ② 手册句 | ③ 脚本样本 | ④ 截图路径 | 结论 | 决策 |
|---|---|---|---|---|---|---|---|
| EVID-J-02 | `RECTPAINT` 填充图层纹理的 RGB，**并把 alpha 置为 0xFF**（`pixel = (SET & 0xFFFFFF) \| 0xFF000000`） | `FUN_0042f809`(0x42f809) 行 334-342：`if (in_AL == '\x18') { FUN_0043185c(); }` / `'\x19'` / `'\x1a'` 同函数；`FUN_0043185c`(0x43185c) 行 47-59：`if (in_stack_0000002c == '\x01') { ... *piVar4 = (in_stack_00000028 & 0xffffff) - 0x1000000; ... }` | `cg/cgact.html` 关键词：「RECTPAINT = 値(0/1)レイヤのRGBチャンネルを指定した色で塗りつぶす」；例 18：「レイヤ「BG」(のRGBチャンネル) を 0x2840ff の色で塗りつぶす。CGACT[ID=BG RECTPAINT=1 SET=0x2840ff]」 | `es_Sprite.yst:44-47`：`CG[ID="SPLDST" ... FILE=""]` / `CGACT[ID="SPLDST" RECTPAINT=1 SET=0x000000]`；`CG[ID="SPLDST2" ... FILE=""]` / `CGACT[ID="SPLDST2" RECTPAINT=1 SET=0xffffff]`。首屏 golden 记录 `SPLDST SET=0`、`SPLDST2 SET=16777215` | `target/tmp/fidelity-micro4-ref/frame_0004.png`：R1 `(70,70)=(255,0,0,255)`；R5 先 `RECTPAINT3 SET=0x00FF00FF`(A=0) 再 `RECTPAINT SET=0xFFFFFF` → `(550,70)=(255,255,255,255)`（证明 alpha 被强制 255） | **Confirmed** | 实现；`alpha=0xFF` 必须照抄 |
| EVID-J-03 | `RECTPAINT2` 只填充 alpha 通道（`pixel.alpha = SET & 0xFF`），RGB 不变；并写图层 `+0x86` 标志 | `FUN_0043185c` 行 60-83：`else if (in_stack_0000002c == '\x02') { ... *(char *)(iVar2 + 3 + param_2) = (char)in_stack_00000028; ... }`；`+0x86 = 0` 当 `SET&0xff==0xff` 且区域=整层，否则 `1` | `cg/cgact.html`：「RECTPAINT2 = 値(0/1)レイヤのαチャンネルを指定した色で塗りつぶす」；例 19：「αチャンネルを 0x80(=128) で塗りつぶす。(→レイヤが半透明になる)」 | `es_ERIS.yst:733-749`（`es.CGTSS`）：`CG[ID=$id ... A=0 FILE=""]` / `CGACT[COPY2=1 ID=$ID ID2=$id]` / `CGACT[ID=$id RECTPAINT2=1 SET=0xff]` / `CG[ID=$id A=256]`；首屏 `CGS100 SET=255`。另 `es_Scenario.yst:1182` `SET=255` | 微场景 R2：`RECTPAINT SET=0x00FF00` 后 `RECTPAINT2 SET=0x80` → `(190,70)=(16,160,48)`，与 RGB 不变、alpha 0.502 的 MODE 0 结果逐值吻合 | **Confirmed** | 实现；`+0x86` 语义未取证，只记录不建模（见 EVID-J-06） |
| EVID-J-04 | `RECTPAINT3` 用 32 位 `SET` 同时填充 alpha+RGB（`pixel = SET`）；`SET` 高字节为 alpha | `FUN_0043185c` 行 64-83：`else if (in_stack_0000002c == '\x03') { ... *puVar3 = in_stack_00000028; ... }`；`+0x86` 规则同 RECTPAINT2 | `cg/cgact.html`：「RECTPAINT3 = 値(0/1)レイヤのαおよびRGBチャンネルとを指定した色で塗りつぶす」；例 20：「αチャンネルを 0xff で塗りつぶし、RGBチャンネルを 0x2840ff の色で塗りつぶす。CGACT[ID=BG RECTPAINT3=1 SET=0xff2840ff]」 | `es_Font.yst:220-232`：`CG[ID="FONTL"+$... E=0 FILE=""]` / `CGACT[... RECTPAINT3=1 SET=@es.COL.CLEAR]`；`global.yst:246` `G_INT[@es.COL.CLEAR=0x00000000]` → 首屏 FONTL2..9 `SET=0` | 微场景 R3 `SET=0xFF0000FF` → `(310,70)=(0,0,255,255)`；R4 `SET=0x80FFFF00` → `(430,70)=(144,160,48)`（A=128，RGB 黄） | **Confirmed** | 实现 |
| EVID-J-05 | 区域 `X/Y/X2/Y2` 限制填充范围（纹理坐标，含端点） | `FUN_0042f809` 行 43-44/62-64：`local_2c = (in_stack_00000018 - in_stack_00000010) + 1; local_28 = (in_stack_0000001c - in_stack_00000014) + 1;`；`FUN_0043185c` 行 55-59/75-80：循环 `in_stack_00000014..1c` / `10..18` 写像素 | `cg/cgact.html`：「X = …レイヤの範囲指定をする際に使用」「X2 = レイヤのある範囲だけを加工するといった場合に使用」；例 27/28/29/30 | `es_Scene/BACKLOG/ES_Log.yst:361-370`：`CGACT[ID="LOG.VO1"... RECTPAINT2=1 SET=0 Y=0 Y2=-@y3]`；`ES_Log.yst:368-370` `Y=@y5-@y1 Y2=9999` | 微场景 R6：`CG[ID=":/W0/R6" FILE="cg/ani_01"]` + `RECTPAINT=1 SET=0x00FFFF X=50 Y=50 X2=149 Y2=149` → `(120,230)=(0,255,255,255)`，区域外 `(30,160)=(32,64,96,255)`（背景） | **Confirmed** | 实现；范围按纹理坐标、含端点、越界裁剪 |
| EVID-J-06 | `+0x86`（RECTPAINT2/3 写入的 alpha 标志）的渲染语义 | `FUN_0043185c` 行 45-52/67-73 写 `+0x86`；`FUN_00408b1d` 行 107/230 在绘制路径读 `+0x86`，但完整分支未还原 | 无手册页 | 首屏 CGS100/FONTL 都写；真实脚本未直接读 | 无独立像素证据 | **Uncertain** | 只记录，不建模；不因它改变填充像素结果。挂 DIV-GFX-041 |
| EVID-J-07 | `RECTPAINT`/`2`/`3` 的处理体是同一函数，模式 1/2/3 只由调用参数区分 | `FUN_0042f809` 行 334-342 三个分支同调 `FUN_0043185c`；`FUN_0043185c` 行 45-83 按 `in_stack_0000002c` 分支 | 手册三条命令名称/参数分别对应 | 微场景三条命令各自独立（R1/R2/R3） | 微场景 frame_0004 | **Confirmed**（函数级） | 实现三个动作，但在 gfx 保留动作枚举差异 |

### 2.2 首屏 11 处参数组合（真实语料 + golden）
来源：`crates/yu-gfx/tests/golden/eris_first_screen_display.json` 的 11 个 `Op::Image`；
脚本行见上表；所有 `X/Y/X2/Y2/SX/SY` 均为 `null`。

| # | 目标层 | 动作 | SET | SET2/SETSTR | 脚本依据 |
|---|---|---|---:|---|---|
| 1 | `:/W0/SPLDST` | rectpaint | `0x000000` | — | `es_Sprite.yst:46` |
| 2 | `:/W0/SPLDST2` | rectpaint | `0xFFFFFF` | — | `es_Sprite.yst:47` |
| 3 | `:/W0/CGS100` | rectpaint2 | `255` | — | `es_ERIS.yst:733-749`（`SET=0xff`） |
| 4 | `:/W0/FONTL2` | rectpaint3 | `0` | — | `es_Font.yst:220-232` + `global.yst:246` |
| 5 | `:/W0/FONTL3` | rectpaint3 | `0` | — | 同上 |
| 6 | `:/W0/FONTL4` | rectpaint3 | `0` | — | 同上 |
| 7 | `:/W0/FONTL5` | rectpaint3 | `0` | — | 同上 |
| 8 | `:/W0/FONTL6` | rectpaint3 | `0` | — | 同上 |
| 9 | `:/W0/FONTL7` | rectpaint3 | `0` | — | 同上 |
| 10 | `:/W0/FONTL8` | rectpaint3 | `0` | — | 同上 |
| 11 | `:/W0/FONTL9` | rectpaint3 | `0` | — | 同上 |

> `CGS100` 的 `RECTPAINT2` 目标是 `Snapshot(271)`（`COPY2` 结果），因此它的像素
> 执行依赖 Phase C 的 COPY；在 COPY 未取证/未实现前，这一条只能保持挂账。
> 其余 10 条（SPLDST/SPLDST2/FONTL2..9）目标是 1×1 黑图拉伸层，可直接执行。

### 2.3 组 RT 行为（EVID-J-08，Phase B 的边界条件）

| ID | 语义点 | ① REA 地址 + 原文摘录 | ② 手册句 | ③ 脚本样本 | ④ 截图路径 | 结论 | 决策 |
|---|---|---|---|---|---|---|---|
| EVID-J-08 | 主组 `W0` 的子层直接画到屏幕；子组（`ID` 末尾 `/`）先画进离屏，离屏 alpha 不参与父级合成，组自身 `A` 只在“组合成到父目标”时作用一次 | `FUN_00413dc8`(0x413dc8) 行 19-30：先递归 `if (*(char *)(iVar3 + 0x30) != '\0') FUN_00413dc8();`，再按 `+0xa4`（层种类）与 `+0x89`（E）遍历；行 57-83 的组 RT 分支（`+0x61c` SetRenderTarget、`+0x21c` GetSurfaceLevel、Clear）只在 `DAT_00801750` 调试分支可见；**生产路径的 RT 格式/alpha 写掩码未逐行定位** | `cg/cg.html`：「レイヤセット…不透明度を128…それ以下の全レイヤを“レイヤ合成したもの”が不透明度128」；`basic/layer.html`：ID 末尾 `/` = レイヤセット | 受控 `micro5`：`CG[ID=":/W0/K1" A=128 ...]`（根层）、`CG[ID=":/W0/G/" A=256]` + `G/K2 A=128`、`G2/ A=128` + `G2/K3 A=256`、`G3/ A=256` + `G3/K4 A=128` | `target/tmp/fidelity-micro5-ref/frame_0004.png`：K1 `(70,70)=(16,160,48)`；G/K2 `(210,70)=(0,128,0)`；G2/K3 `(350,70)=(16,160,48)`；G3/K4 `(490,70)=(0,128,0)` | **Confirmed（行为）**；RT 格式/生产路径 **Uncertain** | **不改契约**；挂 DIV-GFX-008/041/042，等主人确认后再设计主组直绘与 X8 组 RT |

**由 EVID-J-08 导出的验收边界**：

- 主组 `W0` 在官方是“直接画到屏幕”，内核当前把它当 `Offscreen(:/W0)` 再以组 alpha 合成。
  因此**半透明层**（R2/R4）在官方与内核之间必然不同；这部分不计入 Phase B 门禁，
  归入 DIV-GFX-008/041。
- 子组 RT 的 alpha 不参与合成（G/K2、G3/K4 的 `A=128` 子层最终为 `(0,128,0)`，
  而不是 `(16,160,48)`），说明组 RT 的 alpha 被当作不透明；内核的 RGBA8 组表面
  在这一点上与官方不同。Phase B 不实现该修正。

### 2.4 Phase B 验收阈值（先声明，后测量）

**区域定义**（以 `fidelity-micro4` 的 800×600 官方图为坐标）：

| 区域 | 屏幕矩形 | 用途 |
|---|---|---|
| R1 内部 | `x=21..118, y=21..118` | RECTPAINT 不透明 |
| R3 内部 | `x=261..358, y=21..118` | RECTPAINT3 alpha=255 |
| R5 内部 | `x=501..598, y=21..118` | RECTPAINT 强制 alpha=255 |
| R6 填充内部 | `x=70..169, y=201..299` | RECTPAINT 区域填充（纹理 50..149，去掉顶行） |
| R2/R4 | 不纳入 | 半透明层，受 EVID-J-08 组 RT 影响 |
| 1px 边界 | 不纳入 | D3D9 半像素/quad 相位未取证（DIV-GFX-042） |

**阈值**：每通道最大绝对差 `≤1`；差值 `>4` 的像素数必须为 `0`。
任一区域不达标 ⇒ Phase B 失败；不得用调整区域/阈值重测来掩盖。

### 2.5 Phase B 验收结果（正式测量）

内核 `first_frame` 以 `YURIS_SAMPLE_ERIS_ROOT=target/tmp/fidelity-micro4`、
`YURIS_FIRST_FRAME_TICK=5` 渲染出的 `target/tmp/first_frame.png`（已复制为
`target/tmp/fidelity-micro4-kernel-soft.png`，避免后续默认 ERIS 运行覆盖），与官方
`target/tmp/fidelity-micro4-ref/frame_0004.png` 按 §2.4 区域比较：

| 区域 | 逐通道最大差 | 差值>4 像素数 | 结论 |
|---|---:|---:|---|
| R1 内部 | 0 | 0 | PASS |
| R3 内部 | 0 | 0 | PASS |
| R5 内部 | 0 | 0 | PASS |
| R6 填充内部 | 0 | 0 | PASS |
| R2/R4（排除） | 48 | 10000/10000 | 归 EVID-J-08 |
| 1px 边界（排除） | 74 | 每矩形顶行 100 | 归 DIV-GFX-042 |

**首屏 11 个 image op**：`executable` 0 → 10（SPLDST/SPLDST2/FONTL2..9）；
`CGS100` 的 `RECTPAINT2` 因目标是 `Snapshot(271)`，保持
`gfx.imageop.copy_source.unsupported`，等 Phase C 的 COPY 取证后再解锁。

---

## 3. Phase C · COPY/COPY2

> **门禁**：C-1..C-5 四件套齐全且 `docs/render-parity.md` 的契约增节经主人确认后，
> 才允许改 gfx `Op::Copy`/`TextureRef::Snapshot` 与后端表面缓存。本会话只做到
> 证据与设计提案，**不实现 COPY**。

| ID | 语义点 | ① REA 地址 + 原文摘录 | ② 手册句 | ③ 脚本样本 | ④ 截图路径 | 结论 | 决策 |
|---|---|---|---|---|---|---|---|
| EVID-J-10 | `COPY` 复制图像+属性；`COPY2` 只复制图像 | `FUN_0042f809`(0x42f809) 行 365-447 的 `if (in_AL == '\b')` 属性块：`*(undefined2 *)((int)local_38 + 0x80) = *(undefined2 *)(iVar9 + 0x80);`（A）、`+0x89`（E）、`+0x87`（MODE）、`+0x82/+0x84`（T/TA）、`+0x68/+0x70`（X/Y）、`+0x78/+0x7c`（尺寸）、`+0x4c`（TID）、`+0x210`（MID）、`+0x1b0..+0x20c`（滤镜/变换）；`in_AL == '\t'` 行 214-248 为 SWAP 属性块 | `cg/cgact.html`：「COPY = 値(0/1)レイヤの画像と属性をコピーする」「COPY2 = 値(0/1)レイヤの画像のみをコピーする」 | `es_ERIS.yst:733-749`（`es.CGTSS`）：`CG[ID=$id ... A=0 FILE=""]` / `CGACT[COPY2=1 ID=$ID ID2=$id]` / `CGACT[ID=$id RECTPAINT2=1 SET=0xff]` / `CG[ID=$id A=256]`；`es_ERIS.yst:677`（`es.CGSSS`）同形 | 微场景 `fidelity-micro6h-ref/frame_0005.png`（COPY2 目标可见） | **Confirmed**（手册+REA 属性块+脚本） | 仍不实现；设计提案写入 `docs/render-parity.md`，等主人确认 |
| EVID-J-11 | 复制时序：旧线声称 `last_rendered_frame`；受控实验显示 **group 源在命令执行时取当前状态** | `FUN_0042f809` 的 COPY 分支不读取“上一帧”缓冲；`FUN_00413dc8` 的组遍历在复制前重绘/刷新组（行为证据，未逐行定位刷新点） | 手册只说“スクリーンショット機能”，未写时序 | `fidelity-micro6h`：BG + R1 红 + SRC 红；`WAIT[FRAME=1]` 后 `CGACT[SRC RECTPAINT green]` → `CGACT[COPY2=1 ID=":/W0/" ID2="DST"]` → `CGACT[SRC RECTPAINT blue]`；DST 全屏 A=256 | `target/tmp/fidelity-micro6h-ref/frame_0005.png`：SRC 区域 `(70,350)=(0,255,0)`（若复制上一帧应为红；若复制帧末状态应为蓝；若复制失败 DST 全屏黑） | **Refuted（旧线 `last_rendered_frame`，至少 group 源）**；layer 源未单独测 | 旧结论改为“命令执行时当前状态（group 源）”；不把 `last_rendered_frame` 写进新契约 |
| EVID-J-12 | 复制区域 `X/Y/X2/Y2` | `FUN_0042f809` 行 43-44/62-64 的矩形计算；COPY 分支是否使用该矩形尚未逐行确认 | `cg/cgact.html` 例 29/30：`COPY=1 X=30 Y=20 X2=100 Y2=120`（同层/ID2 两种） | 待微场景 | 待微场景 | **Uncertain** | 不实现；Phase C 下一会话补 |
| EVID-J-13 | 快照源/纹理的生命周期：层析构释放 `+0x90` 纹理、`+0x4c` TID、`+0x210` MID、`+0x218` | `FUN_0043d4bc`(0x43d4bc) 行 44-87：引用计数归零后 `if (*(int *)(iVar4 + 0x90) != 0) { FUN_00468e10(...); } *(undefined4 *)(iVar4 + 0x90) = 0;`，随后释放 `+0x4c/+0x210/+0x218`，最后 `+0xa4 = 0`（层死） | 无手册页 | 首屏 17 个 snapshot 层（golden）；`es.CGTSS` 后 `CGEND` 删除临时层 | 待 Phase C 微场景 | **Partial**（析构释放已取证；离屏 RT 的创建/清空未定位） | 不实现；`FUN_00462e40` 离屏族仍需逐函数定位 |
| EVID-J-14 | 首屏 17 个快照层（源层集/id/用途） | golden `crates/yu-gfx/tests/golden/eris_first_screen_display.json` 的 `Snapshot` 层；`GfxState::copies()` 审计日志 | `cg/cgact.html` COPY2 | 首屏按钮快照：`BT.OFFES.GAMEMAIN.*` 等 67+ 层；`CGS100` 为 `COPY2 ID=":/W0/"` | `target/tmp/fidelity-ref/frame_0006.png`（title） | **Confirmed（清单）** | 只登记，不实现 |

**17 个快照层逐一登记**（来自 golden；源层集 = `:/W0`，用途见下）：

| # | 目标层 | 快照 id | Z | 用途 |
|---|---:|---:|---:|---|
| 1 | `:/W0/BT.OFFES.GAMEMAIN.TIP.CLICK=0=1` | 272 | 99000000001 | 提示按钮快照（CLICK） |
| 2 | `:/W0/BT.OFFES.GAMEMAIN.TIP.CTRLSKIP=0=1` | 276 | 99000000001 | 提示按钮快照（CTRLSKIP） |
| 3 | `:/W0/BT.OFFES.GAMEMAIN.BTN.LOAD=0=1` | 281 | 99000000020 | 按钮栏快照（LOAD） |
| 4 | `:/W0/BT.OFFES.GAMEMAIN.BTN.QSAVE=0=1` | 285 | 99000000020 | 按钮栏快照（QSAVE） |
| 5 | `:/W0/BT.OFFES.GAMEMAIN.BTN.QLOAD=0=1` | 289 | 99000000020 | 按钮栏快照（QLOAD） |
| 6 | `:/W0/BT.OFFES.GAMEMAIN.BTN.PREVSEL=0=1` | 293 | 99000000020 | 按钮栏快照（PREVSEL） |
| 7 | `:/W0/BT.OFFES.GAMEMAIN.BTN.LOG=0=1` | 297 | 99000000020 | 按钮栏快照（LOG） |
| 8 | `:/W0/BT.OFFES.GAMEMAIN.BTN.VOICE=0=1` | 301 | 99000000020 | 按钮栏快照（VOICE） |
| 9 | `:/W0/BT.OFFES.GAMEMAIN.BTN.AUTO=0=1` | 306 | 99000000020 | 按钮栏快照（AUTO） |
| 10 | `:/W0/BT.OFFES.GAMEMAIN.BTN.SKIP=0=1` | 311 | 99000000020 | 按钮栏快照（SKIP） |
| 11 | `:/W0/BT.OFFES.GAMEMAIN.BTN.CONFIG=0=1` | 316 | 99000000020 | 按钮栏快照（CONFIG） |
| 12 | `:/W0/BT.OFFES.GAMEMAIN.BTN.SCREEN=0=1` | 320 | 99000000020 | 按钮栏快照（SCREEN） |
| 13 | `:/W0/BT.OFFES.GAMEMAIN.BTN.ERASE=0=1` | 324 | 99000000020 | 按钮栏快照（ERASE） |
| 14 | `:/W0/BT.OFFES.GAMEMAIN.BTN.TITLE=0=1` | 328 | 99000000020 | 按钮栏快照（TITLE） |
| 15 | `:/W0/BT.OFFES.GAMEMAIN.BTN.END=0=1` | 332 | 99000000020 | 按钮栏快照（END） |
| 16 | `:/W0/BT.OFFES.GAMEMAIN.BTN.MENU=0=1` | 336 | 99000000020 | 按钮栏快照（MENU） |
| 17 | `:/W0/CGS100` | 271 | 4551686018427387903 | title 截图（`es.CGTSS`，随后 `RECTPAINT2`） |

> 前 16 个按钮快照的源层在 `CGEND` 后已删除，渲染期无法重放；完整历史在
> `GfxState::copies()`。`CGS100` 的复制源在渲染期仍存在（`:/W0`），其
> `RECTPAINT2` 当前因快照源被跳过（EVID-J-10/J-13）。

### 3.1 Phase C 设计提案（未确认，暂不动契约）

```text
gfx:
  CopyOp 增加 `source_snapshot_id`(已有 CopyId) + `region`(已有) + `timing`
  将 `timing=last_rendered_frame` 改为 `timing=command_current`（EVID-J-11）；
  `Op::Copy` 的 `status` 仍由 gfx 决定,默认 Unimplemented,直到契约确认。

后端:
  帧内维护 `layer_id -> RGBA8 surface` 的跨帧缓存;
  `COPY/COPY2` 从源层/组的当前表面取矩形,按 region 写入目标层纹理;
  `COPY` 额外复制属性(A/E/MODE/T/TA/坐标/尺寸/滤镜/MID/TID);
  `COPY2` 只写像素。
  主组 W0 的直绘与组 RT X8 语义按 EVID-J-08 另行设计。
```

> 以上只是提案。`docs/render-parity.md` 的契约增节需主人确认后才动。

---

## 4. 受控微场景源码（官方编译）

`<repo>/target/tmp/fidelity-micro4/data/script/micro.yst`（CP932，YSCom 0.495 编译）：

```text
#MICRO
{
    CG[ID=":/W0/BG" Z=10 E=1 A=256 X=0 Y=0 SX=800 SY=600 FILE=""]
    CGACT[ID=":/W0/BG" RECTPAINT=1 SET=0x204060]

    CG[ID=":/W0/R1" Z=20 E=1 A=256 X=20 Y=20 SX=100 SY=100 FILE=""]
    CGACT[ID=":/W0/R1" RECTPAINT=1 SET=0xFF0000]

    CG[ID=":/W0/R2" Z=21 E=1 A=256 X=140 Y=20 SX=100 SY=100 FILE=""]
    CGACT[ID=":/W0/R2" RECTPAINT=1 SET=0x00FF00]
    CGACT[ID=":/W0/R2" RECTPAINT2=1 SET=0x80]

    CG[ID=":/W0/R3" Z=22 E=1 A=256 X=260 Y=20 SX=100 SY=100 FILE=""]
    CGACT[ID=":/W0/R3" RECTPAINT3=1 SET=0xFF0000FF]

    CG[ID=":/W0/R4" Z=23 E=1 A=256 X=380 Y=20 SX=100 SY=100 FILE=""]
    CGACT[ID=":/W0/R4" RECTPAINT3=1 SET=0x80FFFF00]

    CG[ID=":/W0/R5" Z=24 E=1 A=256 X=500 Y=20 SX=100 SY=100 FILE=""]
    CGACT[ID=":/W0/R5" RECTPAINT3=1 SET=0x00FF00FF]
    CGACT[ID=":/W0/R5" RECTPAINT=1 SET=0xFFFFFF]

    CG[ID=":/W0/R6" Z=25 E=1 A=256 X=20 Y=150 FILE="cg/ani_01"]
    CGACT[ID=":/W0/R6" RECTPAINT=1 SET=0x00FFFF X=50 Y=50 X2=149 Y2=149]

    LOOP[]
    {
        WAIT[FRAME=1]
    }
    LOOPEND[]
}
```

官方输出采样（frame_0004）：

```text
BG   (700,400) = (32, 64, 96, 255)
R1   ( 70, 70) = (255, 0, 0, 255)      # RECTPAINT, alpha forced 255
R2   (190, 70) = (16, 160, 48, 255)    # RECTPAINT2 A=128 over BG
R3   (310, 70) = (0, 0, 255, 255)      # RECTPAINT3 0xFF0000FF
R4   (430, 70) = (144, 160, 48, 255)   # RECTPAINT3 0x80FFFF00
R5   (550, 70) = (255, 255, 255, 255)  # RECTPAINT after A=0 -> opaque
R6   (120,230) = (0, 255, 255, 255)    # region fill
R6   ( 30,160) = (32, 64, 96, 255)     # outside region = BG
```

---

## 5. 结论统计（持续更新）

| 范围 | Confirmed | Refuted | Uncertain |
|---|---:|---:|---:|
| A（alpha） | 2 | 0 | 0 |
| B（RECTPAINT 族） | 6 | 0 | 1（J-06 `+0x86`） |
| C（COPY/COPY2） | 2（J-10/J-14） | 1（J-11 旧线时序） | 2（J-12/J-13 机制） |

**Phase A 门禁**：EVID-J-01 Confirmed；soft 修复后与 wgpu 同帧半透明像素逐通道差 ≤1，
差异像素数写入 `PROGRESS.md`。
**Phase B 门禁**：EVID-J-02..05 Confirmed；首屏可执行 image op 从 0 升至 10
（CGS100 因依赖 COPY 保持挂账）；微场景区域与官方逐通道最大差/差值>4 像素数
在测试前声明阈值，再测。
