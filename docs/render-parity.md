# 渲染语义 1:1 证据表与后端执行契约(M3-GFX)

> 状态:2026-10-20,会话 M3-GFX。范围 = `yu-gfx` 语义层 + 可序列化
> `CompositorFrame` + 证据表 + 回归。**不包含** wgpu/wasm/窗口实现。
>
> 证据等级:
> - **Confirmed** = 手册 + 真实脚本/产物 + 引擎反编译三者一致;
> - **Likely** = 三者中有两者 + 第三者可解释,但缺逐像素对拍;
> - **Unverified** = 状态/参数已取证,运算本体未取证;
> - **Unimplemented** = 只有名字/未定位本体,后端必须跳过。
>
> 引擎证据 = 官方 SDK 0.495 `yu-ris.exe`
> (sha256 `4f5eefa3544c7244a907477d89ccee97286f559df5e38b1b1fa674206981138f`)
> 的 Ghidra 12.1.4 反编译。引用格式 `FUN_004xxxxx` + 行号(本地反编译树
> `<side-repo>/workbench/analysis/decomp/<addr>_FUN_<addr>.c` 可复现)。
> 旧接手线 `<repo-old>` 的渲染文档只作线索;其与本文冲突处以本文的
> 二进制/手册证据为准,并在 `crates/yu-gfx/docs/divergences/` 留档。

---

## 0. 一帧数据流

```text
脚本任务(全部让出)
  → yu-rt 事件流(VmEvent::DomainCall,args 已按工程 ysc 表求值)
  → yu-gfx GfxState(图层树/组/过渡/文本模型)
  → CompositorFrame(稳定 JSON:passes + layers + diagnostics)
  → yu-backend-soft / yu-backend-wgpu(只执行,不改语义)
```

`yu-gfx` 不在帧里做像素、字形栅格化、GPU 状态或平台调用。

---

## 1. 证据总表

| # | 语义 | 手册页 + 原文 | 真实脚本/产物 | 引擎证据(地址/行) | 结论/等级 |
|---|---|---|---|---|---|
| 1 | 图层模型:ID/路径/Z/组 | `basic/layer.html`:「レイヤには優先度…値が大きいほど、手前に表示」「同じ優先度（Ｚ値）…不確定」「ＩＤの最後に「／」…レイヤセット」「ルートに「W0」…カレントパスもその場所」;`cg/cg.html`:「レイヤセット…不透明度を128…それ以下の全レイヤを“レイヤ合成したもの”が不透明度128」 | ERIS 源 660 处 `CG[`/307 `CGACT[`/103 `CGEND[`;首屏 `ID/IDNO/E/A/X/Y/Z/SX/SY/FILE/MODE` | `FUN_00413dc8` 行 34-49(先递归子层;`+0xa4/+0x89/+0x90` 判空跳过);`FUN_00414ba4` 行 115-156;`FUN_0042f57d` 行 61-112(SWAP 属性);`FUN_0042f809` 行 378-446(COPY 属性块) | Confirmed;同 Z 次序 = 内核取创建序(DIV-GFX-014) |
| 2 | `A` 0..256 | `cg/cg.html`:「A = 値(0～256)」 | ERIS 首屏 `A=256`/`A=0`,后续 `A=@_PINT(1)`;源 `CG[... A=@mA]` | `FUN_00414ba4` 行 116(`(short)+0x80`)、135-140(clamp 0..255)、149-154(TFACTOR=A 灰度)、910-913(`YS_AlphaVal=A/256`) | Confirmed;固定管线 `min(A,255)/255`,着色器 `A/256` |
| 3 | `MODE=0..18` | `cg/cg.html` 全表(0=通常…18=ピンライト);「4=除算…13=焼き込みリニア」 | ERIS 首屏 `MODE=0`(1015 次)+`MODE=2`(1 次);源 `MODE=@bl` | `FUN_00414ba4` 行 144-154(分派/TFACTOR)、391-462(M0)、463-509(M1)、510-560(M2)、561-633(M3)、634-701(M5)、702-752(M6)、753-820(M7)、824-874(M9)、881-971(HLSL) | 0 Likely;1/2 Confirmed;**3/5/6/7/9 状态表已实现(soft/wgpu 全部),官方像素待 YSCom 工具恢复**;4/8/10..18 Unimplemented |
| 4 | 纹理过滤 MAG/MIN/MIP | 无手册页;引擎设定工具写入 | ERIS `yscfg` 无过滤字段(全局设置) | `FUN_0043cd6c` 行 46-117:stage 0/1 各下发 `D3DSAMP_MAG/MIN/MIPFILTER`;值来自 `DAT_005b4804/4808/80131c`;0=POINT/1=LINEAR/2=ANISOTROPIC,MIP=-1=NONE | Confirmed 全局,非逐层;旧线“固定 Linear”❌ |
| 5 | 变换 X/Y/FX… | `cg/cg.html` 只文档化 X/Y;`basic/layer.html` 无 FX 说明 | ERIS `es_Sprite.yst` `FX=@fx*100 ... FRZ=@frz FCX=@fcx`;`FBX=10000`/`FCX=5000` | `FUN_00414ba4` 行 117-125(+0x1b0/1b8/1c0/1c8/1d0/1d8 `/100`,+0x1f8/1fc/200 f32 `/100`)、220-275(16.16 `ROUND(v*65536)`)、322-336(旋转矩阵,`π/180` 常量 0x58ebf8);半像素 `-0.5` 常量 0x58ebd0 | X/Y Confirmed;FX 家族**绑定已闭**(DIV-GFX-003/069),绘制合成公式仍 open(DIV-GFX-070) |
| 6 | 多部件层 / `FILE` 探测 / 1×1 黑 | `cg/cg.html`:「FILE="" で1x1サイズの黒画像生成」「拡張子…「.PNG」「.BMP」「.JPG」「.GIF」の順」 | ERIS 首屏 552 次 `FILE`(`FILE=""` 与 `cgsys/...` 资产);源 `CG[FILE=...]` | `FUN_0040c9b4`(`0x40c9b4`)按 `yscfg+0x1c` 码探测:`1→.png,2→.bmp,3→.jpg,4→.gif,5→.psb`;多部件 `+0xa6/+0xa7` 计数、`+0xb0/+0x130` 两表(`FUN_00414ba4` 155-156/200/219/985) | `FILE`/扩展名 Confirmed;多部件几何 Unimplemented(DIV-GFX-004) |
| 7 | 组/离屏/COPY | `cg/cg.html`:「レイヤセット…レイヤ合成したもの」「COPY キーワード…複製（スクリーンショット機能）」;`basic/layer.html`:`CG[ID=ABC/ ...]`、`CGACT[COPY2=1 ID=:/W0/ ID2=TMP]` | ERIS 首屏 `CG[ID=":/W3/" SX=..]`、`CGACT[COPY2=1 ID=:/W0/ ID2=CGS100]` | `FUN_00413dc8` 行 34-36(递归)、51-102(调试 RT/`+0x61c`);`FUN_00462e40`(离屏创建族);`FUN_0043cd6c` 行 118-169(帧末读回) | 机制 Confirmed;主组直绘已实现(DIV-GFX-008);COPY 时序/区域/冻结已实现(DIV-GFX-007);**首帧主组 COPY 无后缓冲源仍 open(DIV-GFX-051)** |
| 8 | 过渡 T/TA/TID | `cg/cg.html`:「T=トランジション進行度」「TA=精度」「TID=トランジションレイヤID」 | ERIS 源 `T/TID/TA` 少量(首屏 0 次);`TID=""` 用于结束过渡(`es_Button.yst:4526/4540`、`es_Sprite.yst:4238`) | `FUN_00414ba4` 行 166-188(TA<1 门控)、881-971(`YS_Texture1/2`、`YS_FadeVal` 公式 905-909、`YS_FadeVal2`);`FUN_00413dc8` `0x413e7b`:TID 空串直接跳过配对查询,非空才 `FUN_004141e4`+`FUN_00414559` | 公式 Confirmed;**TID 空串=无配对 Confirmed**;推进者仍未定位(DIV-GFX-006) |
| 9 | 文本 | `font/font.html`:NAME/NO/COLOR/COLOR2/SX/SY/BOLD/ANTIALIASING/DRAWMODE;`text/text.html`:ID/OUTPUTMODE/CLEAR/…;`font/fontinfo.html` | ERIS 首屏 `FONT` 3 次;`TEXT[OUTPUTMODE=0]`(`es_First.yst:22`)、`TEXT[CLEAR=1]`(`es_Text.yst:154`)、`FONT[NO/BNO]` | 文本为引擎内部绘制种类 98/100(旧线 03 §1,REA 复用绘制函数);`TextStyle` 现保存 NO/BNO,`TEXT[ID]` 进入后续 `VmEvent::Text.id`,`CLEAR` 清 VM 文本缓冲 | 属性模型 Confirmed;字形栅格化/定位 Unimplemented(DIV-GFX-012);非 UTF-8 TEXT ID 见 DIV-GFX-021 |
| 10 | 颜色空间 | 无手册页 | 官方截图未采集 | 反编译全树无 `D3DRS_SRGBWRITEENABLE`(state 194)调用;D3D9 默认在存储值空间混合 | Likely:8-bit sRGB 编码存储、gamma 空间混合(DIV-GFX-009) |
| 11 | `CGACT` 图像加工 | `cg/cgact.html`:SIZE/SIZE2/ROTATION/TRIM/RESIZE/SAIDO/MEIDO/AKARUSA/MONO/REVERSE/MOSAIC/BLUR/DIFFUSION/RASTER*/RECTPAINT* | ERIS 首屏 `RECTPAINT*` + `SET/SET2`;源 `CGACT[SIZE/REVERSE/HORIZONTAL]` | 动作分派 `FUN_0042f809`(`0x42f809`,按 AL=动作码;`0x42d5fb` 等 25 个调用点);RECTPAINT/2/3 处理体 `FUN_0043185c`(`0x43185c`)模式 1/2/3;RESIZE 几何 `FUN_0043c971`(`w+X+X2-1`,`h+Y+Y2-1`);其余像素算法未定位 | 参数/命令 Confirmed;`RECTPAINT/2/3`(EVID-J-02..05)、`REVERSE`/`HORIZONTAL`(EVID-T-13)、`SIZE`/`SIZE2`(EVID-J3-04)已实现并逐像素对拍;`ROTATION/TRIM/RESIZE/MONO/BLUR*` 仍 Unimplemented(DIV-GFX-013) |
| 12 | `WINDOW` 域 | `window/window.html`:NO=0 主窗口/CGID 显示图层组/E 显示隐藏/A(β4 仅显隐)/X/Y/SX/SY;`windowend.html`:不能删主窗口 | `es_First.yst:97/98/140`、`es_ERIS.yst:22`(子窗口 CGID) | `yu-rt` 发 `Domain::Window`;gfx 消费主窗口状态,Screen pass 用 `CGID` 与 `E/A`;子窗口状态输出给 host | Confirmed;主窗口 gfx 消费,叠放归 host(DIV-GFX-018) |
| 13 | 图像层显式尺寸 | 手册称 `SX/SY` 仅图层组有效,与真实脚本冲突 | `es_ERIS.yst:579/600`(全屏黑幕)、`es_First.yst:170`、`es_Font.yst:220/231` | `FUN_00465730` 层默认 `+0x78/+0x7c=1`;`FUN_0043772c` 按 SX/SY 重采样并写回;`FUN_00408724` 用层矩形 | Confirmed(引擎/脚本优先);`FILE=""` 黑图按尺寸绘制,资源图缩放挂 Unimplemented(DIV-GFX-022) |

---

## 2. ERIS 首屏 census(真实命令流)

命令表来自工程 `ysbin/ysc.ybn`(119 条;`CG` 45 参数、`CGACT` 70 参数)。
统计方式:`Vm::boot_with_aux` + `SYSTEM_START → es.ERIS`,`record_events=false`
(只保留 DomainCall),frame 0 与 frame 1。

| 帧 | 总命令 | `CG` | `CGACT` | `CGEND` | `CGINFO` | `FONT` | `FONTINFO` |
|---|---:|---:|---:|---:|---:|---:|---:|
| 0(初始化) | 1,290,326 | 30,668 | 280 | 1,183 | 606 | 3 | 1 |
| 1 | 274,466 | 31,906 | 353 | 1,216 | 736 | 3 | 1 |

`CG` 首屏实际参数(槽名→次数,frame 0):

```text
ID 30668 | LINT 29068 | E 1058 | FILE 552 | Z 553 | MODE 1016
X 1023 | Y 1023 | A 36 | SX 13 | SY 13
```

- 取值:`A` = 256×34 / 0×2;`E` = 0×1025 / 1×33;`MODE` = 0×1015 / 2×1。
- `Z` 出现 63 位大值(`4551686018427387903`)与 `99000000020` 等,必须用 i64。
- 首屏没有任何 `T/TA/TID/MID/FX/...`;这些在后续帧/商业语料才出现。
- `LINT` 出现 29,068 次;引擎把它存进图层 i64 字段 `+0x58`(按钮号,
  消费方是点击分派),本内核无该命中链 ⇒ 原样留档(DIV-GFX-011 已定案)。
- 首个有可见层的逻辑帧(历史 golden,`771cf79`):272 个图层、
  34 个 visible、15 个 pass / 44 个 op:12 个 `Texture` pass(11 个图像加工 +
  1 个复制;复制当前**全部** `Unimplemented`)、2 个组离屏(W0 30 ops、
  W1 1 op)+ screen 1 op(W0 组)。真实 `MODE=2` 图层是 `:/W0/SPLDST2`
  (A=0、E=0);`E=0` 图层共 238 个。
  **任务 J 复核**:当前 HEAD 的 boot 链在 tick 1 产出 8 visible / 18 ops
  (最新 `yu-rt` 的 GOSUB/YSVR 修复改变了启动路径);34/44 是历史 golden,
  `cargo test -p yu-gfx` 当前失败,未改 golden、未把历史值当现状。
  `FILE="" + SX/SY` 的黑幕保存显式尺寸(例 `:/W1/DBG` 260×600、
  `:/W0/FONTL2` 233×27)。
  另有 68 个按钮快照的复制源是已删除的临时层,无法在渲染期重放,
  对应图层标 `Unimplemented`;完整复制历史在 `GfxState::copies()`。
  **任务 J**:11 个 image op 中 10 个 `RECTPAINT*` 现在 `Executable`
  (SPLDST/SPLDST2/FONTL2..9,EVID-J-02..05);`CGS100` 的目标是
  `Snapshot(271)`,保持 `gfx.imageop.copy_source.unsupported`,等 COPY。
  **任务 J2**:title 稳定帧(tick 300)为 16 pass / 27 op / 17 drawn /
  10 skipped(按钮基名 `asset.missing`)。**任务 J3**:`yu-rt` 实现
  `FILEINFO[EXIST]→LET` 后换成 **27 op / 27 drawn / 0 skipped**,
  `asset.missing=0`;三端同帧 RGBA `6280eac4…b198`,600 帧逐帧哈希 0 差异
  (`docs/title-motion-ledger.md` §9)。
  `docs/tasks` 早先的“1 个可执行复制”在 J2 已更新为 COPY/COPY2 全部可执行
  (TEXCOPY/TEXSSCOPY 除外)。

---

## 3. 图层模型

### 3.1 ID / 路径

- ID 是**路径语义**:`/`、`.`、`:`(根)、`..`;默认根组 `W0`,当前路径初始在
  `W0`(手册 `basic/layer.html`)。
- 规范化规则(`crates/yu-gfx/src/path.rs`):
  - 相对 ID → `:/W0/<id>`;绝对 `:/...` → 从根开始;
  - `.` 忽略,`..` 弹一级,根处 `..` 夹紧;
  - 末尾 `/` = 图层组标记;
  - `IDNO` 按引擎约定拼成 `ID + "." + 十进制`。**旧引用 `0x43d334/0x43d4a4`
    在本地反编译树不存在**(会话 F 复核),已移除;`IDNO` 拼接方式保持
    **Uncertain**(无新的一手证据)。路径规范化/查表的实证函数是
    `FUN_004141e4`(基准+ID → 规范路径)与 `FUN_00414559`(哈希查层表);
    `FUN_00413dc8` 用它解析 `TID`/`MID`。
- 路径比较是**字节级、大小写敏感**(CP932 原始字节)。gfx 不做编码转换。

### 3.2 Z 与同 Z

- `Z` 大者在前;绘制顺序 = 从后到前 = `(z asc, seq asc, path)`。
- 同 Z:手册原文「重なり具合（優先度）は不確定」。内核取**创建序**
  (`seq`,单调递增),保证 A3 确定性;这是已记录的决策(DIV-GFX-014),
  不是“推断引擎按创建序”。
- `Z` 更新不改变 `seq`;因此同 Z 内顺序保持创建序。

### 3.3 组 / E 级联 / 主窗口

- ID 末尾 `/` = 图层组。组先渲染子层到离屏目标,再把组合成纹理作为一层
  绘制;组的 `A/MODE/E/Z` 作用于**合成结果**,不下推到子层(手册 + REA
  `FUN_00413dc8` 行 34-36)。
- **主窗口与子窗口**(手册 `window/window.html`):`NO=0` 是主窗口,
  `1..3` 是子窗口;`CGID` 指定该窗口显示的图层组。gfx 消费
  `Domain::Window`:`WINDOW[NO=0 CGID=...]` 更新主窗口图层组,
  `E`/`A`(β4 仅显隐)更新可见性,`X/Y/SX/SY/Z` 作为窗口几何记录。
  `CompositorFrame.windows` 输出全部窗口状态(按 `NO` 升序);
  `Screen` pass 只合成主窗口图层组(默认 `GfxConfig::main_layer_set`,
  初始 `:/W0`)。子窗口只产出 `Offscreen` pass,叠放/位置归 window/host
  (DIV-GFX-018)。`WINDOWEND NO=0` 被拒绝(手册:主窗口不可消去)。
- `E=0`:组不合成到父层,但子层仍渲染进组离屏(引擎遍历先递归子层、
  后检查本层 `+0xa4`);`CGACT[COPY2]` 仍可取该组内容。gfx 保留组离屏 pass,
  只在父层省略 `Op::Group`。
- 未给 `SX/SY` 的组:离屏尺寸/清屏未取证(DIV-GFX-008);gfx 输出
  `size=None`,后端暂用视口/透明黑(Likely)。

### 3.4 创建 / 更新 / 删除

- `CG[ID=... FILE=...]`:不存在则建层;存在则**只换图**,保留 A/Z/X/Y/MODE/
  T/TA/SX/SY 等属性(手册 `cg/cg.html` 例 1 原文 + `FUN_0042f809` 属性块)。
- `CG[ID=... SX=... SY=...]`:
  - 图层组 → 离屏尺寸(手册原文);
  - **图像层** → 引擎层矩形 `+0x78/+0x7c`。`FILE=""` 的 1×1 黑图按该尺寸
    铺满(真实 ERIS 黑幕,`es_ERIS.yst:579`);资源图的缩放/裁剪未取证,
    图层标 `Unimplemented`(DIV-GFX-022)。只给 SX/SY 之一保持未实现。
- `CG[ID=...]` 无 FILE:
  - 已存在 → 更新属性;
  - 不存在 → 引擎处理体是“取/建节点”(`<side-repo>` §3.24),建无纹理层,
    绘制遍历按 `+0x90==0` 跳过;手册例 4 称“图层不存在=错误”,两者张力
    记录为 DIV-GFX-016。gfx 采用引擎 get-or-create。
- `CGEND[ID=...]`:删层/删组;组递归删除子树。删除不存在的层 = Recoverable
  诊断 + 继续(不崩帧)。
- 单条命令失败(缺 ID/类型错/图层不存在)= Recoverable/Unimplemented 诊断 +
  跳过该命令;**整帧不失败**。后端绘制失败才是引擎的“本帧中止”语义
  (旧线 01 §5),不属于状态机。

---

## 4. Alpha `A`

| 路径 | 公式 | 证据 |
|---|---|---|
| 内部存储 | `(i16)层+0x80`,原值 0..256 | `FUN_00414ba4` 行 116;CGINFO `(s16)+0x80` |
| 固定管线 | `min(max(A,0),255)/255` | 行 135-140;`TFACTOR=(A<<24)|(A<<16)|(A<<8)|A` 行 149-154 |
| 着色器 | `A/256`(常量 0x58ec00=1/256) | 行 910-913 |
| 顶点色 | `A<<24` 打包(旧线 03 §4;`FUN_0041a308`) | 旧线反编译,未逐行复核 |

- `A=256` 固定管线 = 255(等值);着色器 = 1.0。**两条路径不得混用缩放**。
- gfx 的 `Layer.alpha` 存原值,不做 clamp;后端按 `BlendMode` 是否走
  固定管线选 [`yu_gfx::fixed_pipeline_alpha`] / [`yu_gfx::shader_alpha`]。
- 纹理 alpha × 顶点/层 alpha 的乘法链在固定管线阶段完成;gfx 不预乘。

---

## 5. 混合 `MODE=0..18`

### 5.1 分派与 TFACTOR

`FUN_00414ba4` 行 144-154:当 `MODE ∈ {0,1,2,3,5,6,7,9,100,98}` 时
`TFACTOR = A 灰度复制`;否则 `TFACTOR = 0xFF000000`。MODE 3/7 分支内又
显式覆盖 `RS(60, 0xFFFFFFFF)`。

### 5.2 固定管线状态表(最终状态,后写覆盖先写)

记法:`stage[i] = {color_op, color_arg1, color_arg2, alpha_op, alpha_arg1,
alpha_arg2}`;D3D9 原始值。`TF` = `D3DRS_TEXTUREFACTOR`。
`DIS` = `D3DTOP_DISABLE`;`SEL1` = `SELECTARG1`;`MOD` = `MODULATE`;
`BCA` = `BLENDCURRENTALPHA`(值 16,`Arg1 + Arg2*(1−C.a)`);`W/B/F` =
白纹理/黑纹理/TFACTOR。`D/T/C/F` = `DIFFUSE/TEXTURE/CURRENT/TFACTOR`。

| MODE | stage0 | stage1 | TF | SRCBLEND | DESTBLEND | BLENDOP | 等级 |
|---|---|---|---|---|---|---|---|
| 0 通常 | color SEL1(T); alpha MOD(T,F) | color SEL1(T); alpha DIS | A 灰度 | SRCALPHA | INVSRCALPHA | ADD | Likely |
| 1 加算 | color SEL1(T); alpha MOD(T,D) | DIS | A 灰度 | SRCALPHA | ONE | ADD | Confirmed |
| 2 減算 | color MOD(D,T); alpha MOD(D,T) | DIS | A 灰度 | SRCALPHA | ONE | REVSUBTRACT | Confirmed |
| 3 乗算 | color MOD(D,T); alpha MOD(D,T) | color BCA(C,W); alpha SEL1(C) | 0xFFFFFFFF | ZERO | SRCCOLOR | ADD | 可执行(soft+wgpu) |
| 5 スクリーン | color MOD(T,D); alpha MOD(T,D) | color BCA(C,B); alpha SEL1(C) | A 灰度 | INVDESTCOLOR | ONE | ADD | 可执行(soft+wgpu) |
| 6 比較(明) | color MOD(T,D); alpha MOD(T,D) | DIS | A 灰度 | ONE | ONE | MAX | Confirmed |
| 7 比較(暗) | color MOD(T,D); alpha MOD(T,D) | color BCA(C,F); alpha SEL1(C) | 0xFFFFFFFF | ONE | ONE | MIN | Confirmed |
| 9 除外 | color MOD(T,D); alpha MOD(T,D) | DIS | A 灰度 | INVDESTCOLOR | INVSRCCOLOR | ADD | 可执行(soft+wgpu) |

- `BCA` = `D3DTOP_BLENDCURRENTALPHA`(d3d9types.h 值 **16**):
  `Arg1 + Arg2*(1−C.a)`。旧表把它记成 `MAA`(MODULATEALPHA_ADDCOLOR,
  值 18)是错的;按 16 重读后,stage1 的作用是“向辅助色中性点淡化”。
- 引擎公共前缀:`SetTexture(2,NULL) → SetTexture(0,层纹理) →
  SetTexture(1,配对)`;MODE 3 把 stage1 绑白纹理(`DAT_00801d98`),
  MODE 5 绑黑纹理(`DAT_00801d9c`),MODE 7 的 `BCA` arg2 是白 TFACTOR。
- aux 纹理已定案:`DAT_00801d98` = **白**(失败串 `WHITETEXBUF ERROR.`),
  `DAT_00801d9c` = **黑**(`BLACKTEXBUF ERROR.`),见 `FUN_00462e40`
  行 282-310。于是源色归约为:
  - MODE 0/1:`T`
  - MODE 2/5/6/9:`T·A`(顶点 DIFFUSE 是 A 灰度,`FUN_00414ba4` asm
    `0x414d09..0x414d21`)
  - MODE 3/7:`T·A + (1−C.a)`
- 因此 MODE 3/5/6/7/9 的可见公式与手册名一致(乘/屏/max/min/除外),
  但 **MODE 2/3/5/6/7/9 的 RGB 会先乘 A**(旧实现漏了这一层,本次修正)。
- WebGPU 的 `BlendFactor::Src`/`OneMinusDst`/`OneMinusSrc` 是 D3D9
  `SRCCOLOR`/`INVDESTCOLOR`/`INVSRCCOLOR` 的逐通道等价物,所以 MODE
  3/5/9 用固定功能即可 1:1;MODE 6/7 用 MAX/MIN。两端均执行。

**手册可见公式(只作名称对应,不是实现依据)**:

| MODE | 手册名 | 可见公式 | 实现状态 |
|---|---|---|---|
| 0 | 通常 | `src·a + dst·(1−a)` | Likely;按此实现 |
| 1 | 加算 | `src·a + dst` | Confirmed |
| 2 | 減算 | `dst − src·a` | Confirmed |
| 3 | 乗算 | `dst · src` | 可执行(状态表归约,§5.2) |
| 4 | 除算 | 手册名;公式未定位 | Unimplemented |
| 5 | スクリーン | `src + dst − src·dst` | 可执行(状态表归约,§5.2) |
| 6 | 比較(明) | `max(src, dst)` | 可执行(MAX) |
| 7 | 比較(暗) | `min(src, dst)` | 可执行(MIN) |
| 8 | 差分 | 手册名;公式未定位 | Unimplemented |
| 9 | 除外 | `src + dst − 2·src·dst` | 可执行(状态表归约,§5.2) |
| 10..18 | Overlay..PinLight | 手册名;着色器本体未定位 | Unimplemented |

- **MODE 3/5/6/7/9 的实现依据是 §5.2 的 D3D9 状态表**(aux 白/黑 +
  `BLENDCURRENTALPHA` + DIFFUSE A 灰度),手册公式只作名称对照。
  官方微场景因 YSCom 编译器无法 attach 暂缺(见 `docs/shader-parity.md`
  §6);soft/wgpu 单元向量覆盖 stage 归约与最终 blend。
- **旧线 03 文档的 D3DTA 映射错位**:它把 `0=TFACTOR,1=CURRENT,2=DIFFUSE,
  3=TEXTURE`;D3D9 实际是 `0=DIFFUSE,1=CURRENT,2=TEXTURE,3=TFACTOR`。
  因此旧线对 MODE 0“顶点色直通”的结论作废。本文按 `d3d9types.h` 重新解读。
- MODE 0 的 stage1 在单纹理时绑定 `SetTexture(1,NULL)`;D3D9 对未绑定 stage
  的精确可见行为(跳过/白纹理)本地未逐像素取证,故 MODE 0 标 **Likely**。
  手册的可见语义是“通常 α 混合”,后端按此实现并对拍。

### 5.3 着色器路径

`MODE 4/8/10..18` 在固定管线分支中没有 case;这些模式的 Photoshop 公式本体
仍**未定位**(没有对应 blob)。引擎的 shader 集合已完整取证(任务 J4):
4 个 ps_2_0 blob 在同一个编译 effect(FXO,VA `0x58d420`,`0x1540` 字节)里,
pass 映射为:

| pass | blob | 参数 | 功能 | 触发 |
|---|---|---|---|---|
| 0 | `0x18d3d0` | `Samp1/2`,`YS_AlphaVal`,`YS_FadeVal` | 交叉淡化 `mix(tex1,tex2,FadeVal)` | HLSL 分支(存在配对) |
| 1 | `0x18d1cc` | `Samp1/2`,`YS_AlphaVal`,`YS_FadeVal`,`YS_FadeVal2` | mask 转场(R 通道) | HLSL 分支(`TA>=1`+多部件) |
| 2 | `0x18cf58` | `Samp3`,`YS_TexSize`,`SharpN` | 4-tap 锐利缩放 | 显示缩放档 `DAT_00801334=2` |
| 3 | `0x18c9a8` | `Samp3`,`YS_TexSize`,`YS_TexDiffU` | 16-tap 双三次缩放 | 显示缩放档 `DAT_00801334=3` |

- 创建链:`FUN_00462d28` 把 FXO 复制到栈上 → `D3DXCreateEffect`
  (`0x4cd434`,检查 magic `0xFEFF0901`)→ `SetTechnique("YS_ShaderTest1")`。
  `CreatePixelShader` 在静态链接的 D3DX9 内部调用,所以 exe 字符串里没有它。
- 4 个 pass 全部有独立 WGSL:`transition_psd/psc.wgsl`(pass0/1)、
  `scale_psb/psa.wgsl`(pass2/3)。缩放 pass 通过加法 API
  `WgpuRenderer::render_frame_scaled(frame, output, ScaleFilter)` 接入,
  `render_frame` 仍按 1:1 读回;UV/参数/采样器证据见
  `docs/shader-parity.md` §3.3。
- 本语料 `_DXMODE=0` 走固定管线;`_DXMODE≠0` 的脚本分支才会产生 HLSL
  转场。内核把 `dx_mode` 作为显式宿主契约(默认 0),非 0 时后端用
  psC/psD 独立 WGSL;详见 `docs/shader-parity.md`。
- **MODE 4/8/10..18 仍 `Unimplemented`**:这 4 个 pass 不是 Photoshop
  混合模式;不得拿它们冒充除算/差分/Overlay 等。

### 5.4 实现状态

| MODE | `fixed_state()` | 后端 |
|---|---|---|
| 0 | 有 | 可执行;先按手册 α 混合,像素对拍后再转正 |
| 1/2 | 有 | 可执行(MODE 2 的 RGB 已按 `T·A` 修正) |
| 3/5/6/7/9 | 有 | soft 与 wgpu 全部可执行(源色归约 + D3D9 因子;wgpu 用 `Src/OneMinusDst/OneMinusSrc` 对应颜色因子) |
| 4/8/10..18 | 无 | 后端必须跳过 |

`yu-gfx` 的 `BlendMode::confidence()` 与 `docs/render-parity.md` 本表一一对应;
`json::blend_table_json()` 输出完整状态表供后端初始化。

**任务 J(EVID-J-01)**:MODE 0/1/2 的 alpha 源项统一为 `a²`(D3D9 同因子);
soft 已修,soft/wgpu 半透明像素逐通道差 0(测试
`yu-backend-wgpu/tests/wgpu.rs::semi_transparent_pixels_match_soft_within_one`)。

---

## 6. 纹理过滤

- 全局设置,逐帧对 stage 0/1 下发(`FUN_0043cd6c` 行 46-117):
  MAG/MIN = `DAT_005b4804/4808`(0=POINT,1=LINEAR,2=ANISOTROPIC);
  MIP = `DAT_0080131c`(-1=NONE,0=POINT,1=LINEAR,2=ANISOTROPIC)。
- **值来源(2026-10-21 复核)**:`FUN_00460924` 读完 `yscfg.dat` 后
  **无条件**写 `MAG=1`、`MIN=1`、`MIP=0`、aniso=0
  (`DAT_005b4804/4808/80131c/801320`),不从设置文件读取 MIP/aniso;
  引擎不生成 mip 链。因此“引擎真值”= `LINEAR/LINEAR/POINT`、无各向异性。
- 不是逐层属性;`CG[MIPMAP=...]` 只把值减 1 留用(`0x42b064`),而引擎
  不生成 mip 链 ⇒ 对像素无影响,原样留档(DIV-GFX-011 已定案)。
- gfx 默认值仍是 POINT/POINT/NONE 占位(`filter.source=placeholder`),
  由宿主注入引擎真值(`GfxState::set_filter` → `filter.source=engine_settings`);
  后端在 `placeholder` 时不得声称过滤语义已对齐。
- 扩展名探测序来自 `yscfg +0x1c` 8 字节;`FUN_0040c9b4` 映射
  `1→.png, 2→.bmp, 3→.jpg, 4→.gif, 5→.psb`;样本工程 = `01 02 03 04 05`。

---

## 7. 变换与几何

- `X/Y` 存 i64 引擎像素;gfx 不做 16.16、不做半像素、不做缩放原点。
- 引擎绘制函数(`FUN_00414ba4` 行 117-125)把 `+0x1b0/1b8/1c0/1c8/1d0/1d8`
  除以 100.0,把 `+0x1f8/1fc/200` 的 f32 除以 100.0;行 220-275 用
  `ROUND(v * 65536)` 转 16.16 再拆分;行 322-336 构建旋转矩阵
  (`×π/180`,常量 0x58ebf8)。半像素:行 157/190/214 等有 `-0.5`
  (常量 0x58ebd0)。
- `FX/FY/FBX/FBY/FRX/FRY/FRZ/FCX/FCY/FEX/FEY/FEZ/FSD/FMD/FQU/F`
  在 ERIS 里以 ×100 整数出现(例 `FX=@fx*100`、`FBX=10000`、`FCX=5000`)。
  **槽位→字段绑定已闭**(DIV-GFX-069):`X/FX` 都写 `+0x1b0`(`X` 另写
  `+0x68`=`X`,`FX` 另写 `+0x68`=`FX/100`),`Y/FY`→`+0x1b8`,
  `FBX/FBY`→`+0x1c0/1c8`,`FRX/FRY/FRZ`→`+0x1f8/1fc/200`(f32),
  `FCX/FCY`→`+0x1d0/1d8`,`FEX/FEY/FEZ`→`+0x1e0/1e8/1f0`,
  `FSD/FMD/FQU/F`→`+0x204/208/20c/214`。gfx 落到 `Transform.affine`
  (引擎原值)并把 `FX/FY` 换算进 `transform.x/y`;绘制侧合成公式仍未对拍
  (DIV-GFX-070),非默认值时图层标 `gfx.affine.unverified`。
- UV 平铺/镜像(全局 `DAT_00801334` 1/2/3)未实现;`CGACT` 的
  `SIZE/ROTATION/...` 是图像加工,不是合成变换(DIV-GFX-013)。

---

## 8. 组 / 离屏 / COPY

- 组 = 离屏 pass + 一个 `Op::Group`。pass 顺序:所有 `Texture` 复制/图像加工
  (按命令交错序;J2 起 copy/image 共用一个 `seq`,RECTPAINT 族与 COPY 均可
  执行)→ 组离屏(后序:子组先于父组)→ 屏幕。
- 图像层的子层:引擎遍历对任意节点**先递归子层再画本层**
  (`FUN_00413dc8 0x413e1a`);gfx 把图像层的非组后代按同一后序扁平输出到
  父目标,不再静默丢层。
- 组离屏清屏 = 透明黑(`[0,0,0,0]`),组自身 `A/MODE` 作用于合成结果;
  未给 `SX/SY` 的组尺寸未取证(DIV-GFX-008)。
  **任务 J 微场景(EVID-J-08)**:官方主组 `W0` 的子层直接画到屏幕;
  子组离屏的 alpha 不参与父级合成(子组 `G/K2 A=128` 最终为 `(0,128,0)`,
  而根层 `K1 A=128` 为 `(16,160,48)`)。内核当前把 `W0` 当 `Offscreen`
  且用 RGBA8 直通 alpha,半透明结果因此不同;修正需主人确认契约
  (DIV-GFX-008/041)。
- `CGACT[COPY/COPY2]`:
  - 手册:COPY=图像+属性,COPY2=仅图像;
  - 引擎时序(任务 J 受控微场景 `fidelity-micro6h` + 任务 J2 `j2-micro-copy2`):
    group/layer 源都取**命令执行时的当前状态**;旧线 `last_rendered_frame`
    被 Refuted(EVID-J-11);
  - 区域 `X/Y/X2/Y2` = **源纹理坐标、含端点**;裁剪块写到目标 `(0,0)`,
    其余区域清空;无区域 = 整图(EVID-T-11,`j2-micro-copy`);
  - 命令交错序:COPY 与图像加工按脚本命令序立即执行;`frame()` 用
    `CopyOp::seq`/`ImageOp::seq` 交错输出纹理 pass(DIV-GFX-029 fixed);
  - 现状:`Op::Copy` 可执行;目标层纹理 = `TextureRef::Snapshot(CopyId)`;
    快照在后端按 `CopyId` **冻结一次**(目标层存活期间保持,EVID-T-12);
    `TEXCOPY/TEXSSCOPY` 仍 `Unimplemented`。
  - `CompositorFrame.passes` 只含**目标仍引用该快照且源图层仍存在**的复制;
    源已被 `CGEND` 删除的快照无法在渲染期重放,完整历史在
    `GfxState::copies()` 审计日志里。
- `SWAP/SWAP2`:`FUN_0042f57d` 行 61-112 交换纹理与 `+0x68/+0x70`(X/Y);
  模式 2 额外交换 `E/A/T/TA/MODE`。gfx 按此实现,SWAP/SWAP2 与模式 1/2 的
  精确对应为 Likely。

---

## 9. 过渡 `T/TA/TID`

- 门控(`FUN_00414ba4` asm `0x415558-0x415577`):配对层指针为空 **或**
  `TA < 1` ⇒ 单纹理普通层(无过渡)。`TID` 为空 = 不查配对
  (`0x413e7b`);非空但查不到层同样回退普通层(EVID-T-03/06)。
- uniform:
  - `YS_Texture1` = 本层纹理;`YS_Texture2` = 配对纹理(TID 对侧);
  - `YS_AlphaVal = A/256`;
  - `YS_FadeVal = T / (256/(256/TA + 128)) * (1/256)`(行 905-909);
  - `YS_FadeVal2` 来源未取证(HLSL 路径;本语料不走)。
- **像素语义(J2 修正)**:配对纹理的 **G 通道**是 mask(微场景
  `j2-micro-transA`:`FF0000/0000FF→0`、`00FF00→255`、`808000/008080→127`);
  不是 cross-fade。覆盖度:
  - `T >= 256` ⇒ 1(过渡完成);
  - 否则 `clamp((30+70*fade) * (mask - (1-2*fade)), 0, 1)`。
  证据:`j2-micro-trans6/8` 的 T×TA×mask 测量;TA=128 下残差 ≤8/255,
  TA≠128 的残差见 DIV-GFX-045。J3 起 wgpu shader 使用同一公式与 u8 量化链。
- `T` 由**脚本**推进(`es_Sprite.yst` `@TP`);`+0x1e8` 只在调试组 RT
  分支使用,不是过渡推进者(EVID-T-04)。
- `Transition::fade_value()` 实现公式,不 clamp;`TA<1` 返回 `None`。
- 本语料 `_DXMODE=0` 走固定管线双纹理路径;HLSL `ps_2_0` 的公式
  (`oC0.rgb=tex1.rgb`;`oC0.a=tex1.a*sat((FadeVal-0.5*tex2.r)*FadeVal2)*AlphaVal`)
  在台账 EVID-T-09 备档,不实现。
- 后端:`Op::Transition` 可执行;soft 用整数 mask 采样,wgpu 用独立
  transition pipeline;两端同帧像素一致(见 §14)。

---

## 10. 文本

- 只定义模型:`TextStyle`(NAME/NO/BNO/COLOR/COLOR2/SX/SY/BOLD/
  ANTIALIASING/DRAWMODE,另留 `line_spacing`/`stroke` 占位)+ `TextRun`
  (ID/CP932 字节/样式/原点/字形图集引用/帧号)。
- `FONT` 属性来自手册 `font/font.html`;`NO` 保存字体列表序号;
  `BNO` 无手册页(真实脚本只用于调试槽),保存原值并挂
  `gfx.text.unsupported`。
- `TEXT[ID]` 由 `yu-rt` 保存并写入后续 `WORD`/`_` 的
  `VmEvent::Text.id`;`TEXT[CLEAR=1]` 清空 VM 文本缓冲。WAIT/RETURN/
  PAGE/CLICK/AUTO*/BUTTON 属显示节奏/宿主输入,不伪造。
- 事件 `id` 是 `String`;非 UTF-8 的 CP932 ID 不做有损替换,清空并显式
  报告(DIV-GFX-021)。
- `FONTINFO`/`CGINFO`/`WINDOWINFO` 的 `LET`:已实现
  `yu_rt::event::GraphicsHost` + `yu_gfx::GfxHost` 同步链路。`Vm` 在产生
  图形/窗口/文本/帧事件时**同步**送入 `GfxState`,`INFO` 命令在脚本执行期
  查询并写 `LET`。
  - `CGINFO`:EXIST/E/A/X/Y/Z/MODE/T/TA/TID;组/显式尺寸/1×1 黑图的 SX/SY。
  - `FONTINFO`:当前样式的 COLOR/COLOR2/SX/SY/BOLD/ANTIALIASING/DRAWMODE/
    NAME(无 `NO` 索引)。
  - `WINDOWINFO`:EXIST/E/X/Y/SX/SY(主窗口默认 yscfg 尺寸)。
  - **未取证**:资源图纹理尺寸(需资产元数据,DIV-GFX-024)、多字段
    写回顺序(DIV-GFX-025)、COLOR/ONMOUSE/CAPTION 等需要像素/鼠标/窗口
    host 的字段(DIV-GFX-026);返回 `Unimplemented`,VM **不写** LET。
  - 安装宿主后,调用方不得再把同一批事件 drain 后喂给同一 `GfxState`。
- 字形栅格化、行距、描边、窗口定位、文本层与 CG 图层的坐标关系
  留给字体/后端会话(DIV-GFX-012)。

---

## 11. 颜色空间

- `ColorSpace::GammaEncodedSrgb8`:纹理/离屏/最终呈现都按 8-bit sRGB 编码值
  存储;混合在这些存储值上直接做算术(D3D9 默认,未启用 sRGB write)。
- 证据:反编译全树无 `D3DRS_SRGBWRITEENABLE`(state 194)调用;引擎固定管线
  的 blend 因子/op 都是 D3D9 默认语义。
- 旧线选择 `Rgba8UnormSrgb`(硬件自动线性化)与引擎行为不同;本契约**不继承**。
  待官方截图逐像素对拍后转正(DIV-GFX-009)。

---

## 12. 后端执行契约

### 12.1 Pass 顺序

1. `PassTarget::Texture(id)`:按**命令交错序**执行 `Op::Copy` / `Op::Image`
   (J2:copy/image 共用一个 `seq`,不再按类型分组;EVID-T-15)。
   未实现的操作必须跳过(看 `status.executable=false`),不得近似。
   可执行的 `Op::Image`:`RECTPAINT/RECTPAINT2/RECTPAINT3`(EVID-J-02..05)
   与 `REVERSE`/`HORIZONTAL`(EVID-T-13),目标基图可为 Asset / 1×1 黑 /
   可执行 COPY 的快照。`Op::Copy` 可执行;后端按 `CopyId` 冻结快照并
   在图层绘制阶段用 `TextureRef::Snapshot` 取样。
2. `PassTarget::Offscreen(id)`:组的离屏合成。顺序 = 子组先于父组。
   清屏 = 该 pass 的 `clear`(透明黑)。组内 ops 按 `(z, seq, id)`;
   图像层子层按后序(子先父后)扁平输出。
3. `PassTarget::Screen`:最后执行;`clear` = `CompositorFrame.clear`;
   ops = **主窗口图层组**(`WINDOW[NO=0].CGID`,未指定时
   `GfxConfig::main_layer_set`,初始 `:/W0`)的合成 op;主窗口
   `E=0`/`A=0` 时 ops 为空。子窗口/其他根组只以 `Offscreen` pass 出现,
   由 window/host 会话负责窗口叠放与位置(手册 `window/window.html`;
   DIV-GFX-018)。

后端可以合并/缓存 pass,但不得改变顺序与可见结果。

### 12.2 Alpha / 颜色

- `Layer.alpha` 原值 0..256。
- 固定管线模式(0/1/2/3/5/6/7/9):用 `fixed_pipeline_alpha(A)` =
  `min(max(A,0),255)/255`。
- 着色器模式(4/8/10..18):用 `shader_alpha(A)` = `A/256`;但本版这些模式
  本身就是 Unimplemented。
- 颜色空间 = `frame.color_space`;当前固定 `gamma_encoded_srgb8`,**不要**
  在线性空间混合。
- 不预乘 alpha;乘法链由纹理采样 × 层 alpha 在混合方程里完成。

### 12.3 Blend

- 用 `BlendMode` + `json::blend_table_json()` 的 `fixed` 状态;数值是 D3D9
  原始枚举,后端映射到自己的 pipeline。
- 0/1/2/3/5/6/7/9 可执行(状态表 + D3D9 因子);4/8/10..18 无状态,必须跳过。
- TFACTOR 规则:`AlphaGray` = `(a<<24)|(a<<16)|(a<<8)|a`,`a=min(max(A,0),255)`;
  `OpaqueWhite` = `0xFFFFFFFF`。

### 12.4 Filter

- `frame.filter.mag/min/mip` → 两个采样器(stage 0/1)的 MAG/MIN/MIP。
- `frame.filter.source=placeholder` 表示宿主未注入引擎设置;后端不得声称
  过滤语义已对齐。`engine_settings` 才是已取证值。
- **引擎真值(2026-10-21 复核)**:`FUN_00460924` 在读完 `yscfg.dat` 后
  **无条件**写入 `MAG=1(LINEAR)`、`MIN=1(LINEAR)`、`MIP=0(POINT)`、
  aniso=0(`DAT_005b4804/4808/80131c/801320`),不从设置文件读取 MIP/aniso;
  引擎也不生成 mip 链。因此后端固定 LOD0 + anisotropy=1 + `mipmap_filter`
  最近点与引擎等价(不是“静默忽略”:引擎没有可注入的 MIP/aniso 值)。
  `CG[MIPMAP]` 只改图层字段、引擎不生成 mip 链 ⇒ 对采样无影响(DIV-GFX-011)。
- 不做逐层过滤;`frame.filter` 是全局设置。

### 12.5 Group / Transition / Copy

- Group:先执行 `Offscreen` pass,再在父 pass 中把 `Op::Group.offscreen`
  当纹理,按 `layer.alpha/blend` 合成。
- Transition:`from`/`to` 两个纹理 + 已算好的 `fade`;`to=None` 不会到达
  (gfx 已回退普通层)。`dx_mode=0`:像素 RGB 取 `from`;alpha = 纹理 α ×
  覆盖度 × 层 A;覆盖度用 `to` 的 **G 通道**与 `fade`/`T` 计算
  (`yu_gfx::transition_coverage`,EVID-T-05)。`dx_mode≠0`:psC(mask = 配对
  纹理 **R**,`saturate((FadeVal-0.5*mask.r)*TA)*A/256`)或 psD(交叉淡化)。
  **注意**:`dx_mode` 是内核 A/B 契约,不是引擎开关;引擎 effect 始终创建,
  psC/psD 的触发条件是“有配对 + TA”。见 `docs/architecture.md` §5.1。
  `to` 纹理与 `from` 同尺寸(引擎要求,尺寸不符会报调试错误,EVID-T-07)。
- 组上的 `TID`:gfx 输出 `Op::Transition { from: Offscreen(组), to: 配对 }`,
  不再当普通组画掉;配对可以是图像层或组。引擎按层表统一解析 TID,
  官方微场景因 YSCom 编译器 attach 失败暂缺(标 Likely)。
- Copy:`status` 为 Executable 时执行;快照按 `CopyId` **只捕获一次**
  (目标层存活期间冻结);区域是源坐标含端点,裁剪块写目标 `(0,0)`;
  目标表面尺寸 = 目标层显式尺寸,否则源尺寸。后端可以对组源按需重绘
  (`group` 的当前子层状态)以取得复制源。**主组例外**:主组已直绘到屏幕、
  没有 Offscreen pass;首帧无后缓冲时的源仍未裁决(DIV-GFX-051)。
  `frame.passes` 里的复制是
  “仍可达”的复制;`GfxState::copies()` 是完整审计日志(含源已删除的
  历史项),不是帧执行清单。
- Mask:`Op::Mask.status` 为 Unimplemented;不要用 α 或亮度猜。
- Image size:`LayerKind::Image{size}` 是绘制矩形。`FILE=""` 的
  `SolidBlack1x1` 必须铺满 `size`;资源图 + `size` 的图层是
  Unimplemented,后端跳过。
- Window:`frame.windows` 是窗口状态(含子窗口);Screen 只执行主窗口
  选择,不要自行改变窗口叠放。
- Diagnostics:`frame.diagnostics` 是**本帧**诊断,同键合并并带 `count`;
  不是累计日志。
- 日志上限:`GfxState` 的复制/图像日志各上限 16,384 条,超限丢弃最旧
  一半并计数(`dropped_ops()`),同时发 `gfx.log.truncated` 诊断。

### 12.6 Unimplemented 清单(后端必须跳过)

- MODE 4/8/10..18(公式本体未定位)。MODE 3/5/6/7/9 已由状态表实现:
  soft 与 wgpu 全部执行(`Src/OneMinusDst/OneMinusSrc` 对应颜色因子)。
- `MID` 掩码。
- `FX/FR/FC/FEX/...` 滤镜/仿射。
- 多部件层(非空 `parts`)。
- `CGACT[COPY/COPY2/TEXCOPY/TEXSSCOPY]`:COPY/COPY2 已可执行(DIV-GFX-007);
  **组目标复制也已实现**(soft `compose_group` / wgpu `plan_group_compose`);
  仅 `TEXCOPY/TEXSSCOPY` 仍 `Unimplemented`(`gfx.copy.texcopy.unsupported`)。
  **例外**:`SIZE`/`SIZE2` 已实现(`scale_region`,两端共用;
  无区域时按 StretchRect 透传,EVID-J3-04);`ROTATION`/`RESIZE`/`TRIM`/
  `MONO`/`BLUR*` 等仍只有参数、无像素算法。
  **另一例外**:`RECTPAINT/RECTPAINT2/RECTPAINT3` 与 `REVERSE`/`HORIZONTAL`
  在整层、Asset/1×1 黑/快照基图时标 `Executable`,后端必须执行
  (EVID-J-02..05、EVID-T-13)。
- TID 非空但配对层缺失的过渡:不再 Unimplemented,gfx 回退单纹理普通层
  (EVID-T-03)。
- 资源图 + `SX/SY` 的显式缩放/裁剪(仅 `FILE=""` 黑图可执行;DIV-GFX-022)。
- `CGINFO/FONTINFO/WINDOWINFO` 的未取证字段:资源图 `SX/SY`(DIV-GFX-024)、
  多字段顺序(DIV-GFX-025)、`COLOR/ONMOUSE*/CAPTION/...`(DIV-GFX-026)。
  已取证字段由 `GfxHost` 同步回答。
- `FONT[BNO]` 的字体槽语义(值已保存)。
- `TEXT` 的 WAIT/RETURN/PAGE/CLICK/AUTO*/BUTTON 显示节奏。
- 文本字形(需字体/后端会话)。

---

## 13. 与旧线文档的勘误

| 旧线结论 | 本文更正 | 证据 |
|---|---|---|
| `03-blend-modes.md` D3DTA 映射 `0=TFACTOR,1=CURRENT,2=DIFFUSE,3=TEXTURE` | D3D9 实际 `0=DIFFUSE,1=CURRENT,2=TEXTURE,3=TFACTOR` | `d3d9types.h`;按正确映射重读 `FUN_00414ba4` 状态序列 |
| 旧线把 `TSS(0,6,0)`/`TSS(0,2,0)` 读成 TFACTOR | 实为 DIFFUSE:MODE 1 alpha=`MOD(T,D)`、MODE 3=`MOD(D,T)`;MODE 7 stage1 的 `TSS(1,3,3)` 才是 TFACTOR | 行 484/576/779 |
| MODE 0 “顶点色直通、贴图 RGB 不进颜色” | 作废:stage0 = `SEL1(TEXTURE)`;MODE 0 的 stage1 单纹理行为待像素对拍 | 行 397-441 |
| “纹理过滤固定 Linear” | 错:全局 MAG/MIN/MIP 来自引擎设置 | `FUN_0043cd6c` 行 46-117 |
| “A 只有 `/256` 一条路径” | 固定管线 `min(A,255)/255`,着色器 `A/256` | 行 135-154 / 910-913 |
| “同 Z 按创建序” | 手册称不確定;内核取创建序是**确定性决策**,不是引擎承诺 | `basic/layer.html` |
| `A=256` 固定管线等于 255 | 正确,但只在固定管线;着色器 A=256=1.0 | 同上 |
| 审查 B4:“TFACTOR 只在 MODE 0/3/7” | **错**:集合是 `{0,1,2,3,5,6,7,9,100,98}`;3/7 只是分支内覆盖为白 | `FUN_00414ba4` 行 144-154/567/754 |
| 审查 B7:“`in_stack_00000010==0 → A=0` 未建模” | **错**:图层遍历调用点恒传 1(`0x413f29`),清零分支不属于本路径 | `FUN_00413dc8` 汇编 |
| 审查 B2:“E 可能写 +0xa4” | **错**:E=`+0x89`,层种类=`+0xa4`(1 图像/2 组/0 死) | `0x413e34/0x413e47`、`0x42c400`、`0x40a38c`、`0x40e02c`、`0x43d4bc` |
| 审查 B12:旧地址 `0x42c4a4/0x43d334/0x43d4a4` | 本地反编译树不存在;改用 `0x413dc8/0x4141e4/0x414559/0x43d4bc` | 会话 F 台账 B12 |
| 审查 B11/§2“1 个可执行复制” | **J2 已更新**:`Op::Copy`(COPY/COPY2)现在全部可执行;`TEXCOPY/TEXSSCOPY` 仍 Unimplemented | `state.rs::cgact_copy`;EVID-T-11/12 |
| 手册“SX/SY 仅图层组有效” | **与引擎/真实脚本冲突**:图像层同样保存层矩形;`FILE=""+SX/SY` 是黑幕标准写法 | `FUN_00465730/0043772c/00408724`;`es_ERIS.yst:579` |
| 旧线“COPY 复制上一帧已渲染目标” | **Refuted(至少 group 源)**:受控微场景显示 `COPY2 ID=":/W0/"` 取命令执行时的当前状态;命令后再改图层不影响快照 | `target/tmp/fidelity-micro6h-ref/frame_0005.png`;EVID-J-11 |
| “主组 W0 按普通组离屏合成” | **错**:官方主组 W0 的子层直接画到屏幕;子组离屏 alpha 不参与父级合成 | `target/tmp/fidelity-micro5-ref/frame_0004.png`;EVID-J-08 |

---

## 14. 复现

```bash
export TEMP="<side-repo>/workbench/tmp"; TMP="$TEMP"
export YURIS_SAMPLE_ERIS_ROOT="<side-repo>/workbench/project-eris"   # 数据入口:data.ypf 优先,data/ 为备
export YURIS_REQUIRE_SAMPLES=1

cargo run -p yu-rt --example gfx_census -- 2 8     # 命令/参数 census
cargo run -p yu-gfx --example eris_dump -- 2       # GfxState 帧摘要
cargo test -p yu-gfx                               # 单元 + 首屏 golden
UPDATE_GFX_GOLDEN=1 cargo test -p yu-gfx --test eris_first_screen
cargo clippy -p yu-gfx --all-targets
grep -rn "winit\|wgpu\|web-sys\|wasm-bindgen" crates/yu-gfx/src   # 必须 0
grep -rn "println!\|eprintln!\|dbg!" crates/yu-gfx/src            # 必须 0
```

> 注:命令表解析在 `yu-rt::tables::YscmTable`(源 = 工程 `ysc.ybn`);
> gfx 不内置任何命令序号/参数名常量。首屏 census 的 `LINT` 等未分类参数
> 全部原样保留在 `Layer.extra`,没有静默丢弃。
