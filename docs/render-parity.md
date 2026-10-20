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
| 3 | `MODE=0..18` | `cg/cg.html` 全表(0=通常…18=ピンライト);「4=除算…13=焼き込みリニア」 | ERIS 首屏 `MODE=0`(1015 次)+`MODE=2`(1 次);源 `MODE=@bl` | `FUN_00414ba4` 行 144-154(分派/TFACTOR)、391-462(M0)、463-509(M1)、510-560(M2)、561-633(M3)、634-701(M5)、702-752(M6)、753-820(M7)、824-874(M9)、881-971(HLSL) | 0 Likely;1/2 Confirmed;3/5/6/7/9 Unverified(状态表已取证);4/8/10..18 Unimplemented |
| 4 | 纹理过滤 MAG/MIN/MIP | 无手册页;引擎设定工具写入 | ERIS `yscfg` 无过滤字段(全局设置) | `FUN_0043cd6c` 行 46-117:stage 0/1 各下发 `D3DSAMP_MAG/MIN/MIPFILTER`;值来自 `DAT_005b4804/4808/80131c`;0=POINT/1=LINEAR/2=ANISOTROPIC,MIP=-1=NONE | Confirmed 全局,非逐层;旧线“固定 Linear”❌ |
| 5 | 变换 X/Y/FX… | `cg/cg.html` 只文档化 X/Y;`basic/layer.html` 无 FX 说明 | ERIS `es_Sprite.yst` `FX=@fx*100 ... FRZ=@frz FCX=@fcx`;`FBX=10000`/`FCX=5000` | `FUN_00414ba4` 行 117-125(+0x1b0/1b8/1c0/1c8/1d0/1d8 `/100`,+0x1f8/1fc/200 f32 `/100`)、220-275(16.16 `ROUND(v*65536)`)、322-336(旋转矩阵,`π/180` 常量 0x58ebf8);半像素 `-0.5` 常量 0x58ebd0 | X/Y Confirmed;FX 家族槽位→偏移 Unimplemented(DIV-GFX-003) |
| 6 | 多部件层 / `FILE` 探测 / 1×1 黑 | `cg/cg.html`:「FILE="" で1x1サイズの黒画像生成」「拡張子…「.PNG」「.BMP」「.JPG」「.GIF」の順」 | ERIS 首屏 552 次 `FILE`(`FILE=""` 与 `cgsys/...` 资产);源 `CG[FILE=...]` | `FUN_0040c9b4`(`0x40c9b4`)按 `yscfg+0x1c` 码探测:`1→.png,2→.bmp,3→.jpg,4→.gif`;多部件 `+0xa6/+0xa7` 计数、`+0xb0/+0x130` 两表(`FUN_00414ba4` 155-156/200/219/985) | `FILE`/扩展名 Confirmed;多部件几何 Unimplemented(DIV-GFX-004) |
| 7 | 组/离屏/COPY | `cg/cg.html`:「レイヤセット…レイヤ合成したもの」「COPY キーワード…複製（スクリーンショット機能）」;`basic/layer.html`:`CG[ID=ABC/ ...]`、`CGACT[COPY2=1 ID=:/W0/ ID2=TMP]` | ERIS 首屏 `CG[ID=":/W3/" SX=..]`、`CGACT[COPY2=1 ID=:/W0/ ID2=CGS100]` | `FUN_00413dc8` 行 34-36(递归)、51-102(调试 RT/`+0x61c`);`FUN_00462e40`(离屏创建族);`FUN_0043cd6c` 行 118-169(帧末读回) | 机制 Confirmed;离屏尺寸/清屏 Likely;COPY 时序 Unimplemented(DIV-GFX-007/008) |
| 8 | 过渡 T/TA/TID | `cg/cg.html`:「T=トランジション進行度」「TA=精度」「TID=トランジションレイヤID」 | ERIS 源 `T/TID/TA` 少量(首屏 0 次);`es_Sprite` 过渡用法 | `FUN_00414ba4` 行 166-188(TA<1 门控)、881-971(`YS_Texture1/2`、`YS_FadeVal` 公式 905-909、`YS_FadeVal2`) | 公式 Confirmed;TID 配对/推进者 Likely(DIV-GFX-006) |
| 9 | 文本 | `font/font.html`:NAME/NO/COLOR/COLOR2/SX/SY/BOLD/ANTIALIASING/DRAWMODE;`text/*` | ERIS 首屏 `FONT` 3 次;`TEXT[OUTPUTMODE=0]` 在 `es_First.yst` | 文本为引擎内部绘制种类 98/100(旧线 03 §1,REA 复用绘制函数) | 属性模型 Confirmed;字形栅格化/定位 Unimplemented(DIV-GFX-012) |
| 10 | 颜色空间 | 无手册页 | 官方截图未采集 | 反编译全树无 `D3DRS_SRGBWRITEENABLE`(state 194)调用;D3D9 默认在存储值空间混合 | Likely:8-bit sRGB 编码存储、gamma 空间混合(DIV-GFX-009) |
| 11 | `CGACT` 图像加工 | `cg/cgact.html`:SIZE/SIZE2/ROTATION/SAIDO/MEIDO/AKARUSA/MONO/REVERSE/MOSAIC/BLUR/DIFFUSION/RASTER*/RECTPAINT* | ERIS 首屏 `RECTPAINT*` + `SET/SET2`;源 `CGACT[SIZE/ROTATION/...]` | 命令处理体 `0x42c4a4`(70 参数;本会话只读旁线事实,未逐项 REA);像素算法未定位 | 参数/命令 Confirmed;像素 Unimplemented(DIV-GFX-013) |

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
- `LINT` 出现 29,068 次,视觉语义未分类(DIV-GFX-011),原样留档。
- 首个有可见层的逻辑帧(测试取 frame 0+1 之后):272 个图层、
  34 个 visible、15 个 pass(1 个可执行复制 + 11 个图像加工 +
  2 个组离屏 + screen;其中 W0 离屏 30 ops、W1 离屏 1 op、screen 1 op(W0 组))。
  另有 68 个按钮快照的复制源是已删除的临时层,无法在渲染期重放,
  对应图层标 `Unimplemented`;完整复制历史在 `GfxState::copies()`。

---

## 3. 图层模型

### 3.1 ID / 路径

- ID 是**路径语义**:`/`、`.`、`:`(根)、`..`;默认根组 `W0`,当前路径初始在
  `W0`(手册 `basic/layer.html`)。
- 规范化规则(`crates/yu-gfx/src/path.rs`):
  - 相对 ID → `:/W0/<id>`;绝对 `:/...` → 从根开始;
  - `.` 忽略,`..` 弹一级,根处 `..` 夹紧;
  - 末尾 `/` = 图层组标记;
  - `IDNO` 按引擎约定拼成 `ID + "." + 十进制`(`CGEND` 处理体 0x43d334,
    `CGINFO` 0x43d4a4;`CG` 共用同一 `(ID, IDNO)` 定位,见
    `<side-repo>/docs/status.md` §3.23)。
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
  `1..3` 是子窗口;`CGID` 指定该窗口显示的图层组。`CompositorFrame` 的
  `Screen` pass 只合成主窗口图层组(默认 `:/W0`,可用
  `GfxConfig::main_layer_set` 改)。其余根组(`:/W1`…`)仍产出
  `Offscreen` pass,交给后续 window/host 会话组合;gfx 不做窗口叠放/位置
  (DIV-GFX-018)。
- `E=0`:组不合成到父层,但子层仍渲染进组离屏(引擎遍历先递归子层、
  后检查本层 `+0xa4`);`CGACT[COPY2]` 仍可取该组内容。gfx 保留组离屏 pass,
  只在父层省略 `Op::Group`。
- 未给 `SX/SY` 的组:离屏尺寸/清屏未取证(DIV-GFX-008);gfx 输出
  `size=None`,后端暂用视口/透明黑(Likely)。

### 3.4 创建 / 更新 / 删除

- `CG[ID=... FILE=...]`:不存在则建层;存在则**只换图**,保留 A/Z/X/Y/MODE/
  T/TA 等属性(手册 `cg/cg.html` 例 1 原文 + `FUN_0042f809` 属性块)。
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
`MAA` = `MODULATEALPHA_ADDCOLOR`。`D/T/C/F` = `DIFFUSE/TEXTURE/CURRENT/TFACTOR`。

| MODE | stage0 | stage1 | TF | SRCBLEND | DESTBLEND | BLENDOP | 等级 |
|---|---|---|---|---|---|---|---|
| 0 通常 | color SEL1(T); alpha MOD(T,F) | color SEL1(T); alpha DIS | A 灰度 | SRCALPHA | INVSRCALPHA | ADD | Likely |
| 1 加算 | color SEL1(T); alpha MOD(T,D) | DIS | A 灰度 | SRCALPHA | ONE | ADD | Confirmed |
| 2 減算 | color MOD(D,T); alpha MOD(D,T) | DIS | A 灰度 | SRCALPHA | ONE | REVSUBTRACT | Confirmed |
| 3 乗算 | color MOD(D,T); alpha MOD(D,T) | color MAA(C,T); alpha SEL1(C) | 0xFFFFFFFF | ZERO | SRCCOLOR | ADD | Unverified |
| 5 スクリーン | color MOD(T,D); alpha MOD(T,D) | color MAA(C,T); alpha SEL1(C) | A 灰度 | INVDESTCOLOR | ONE | ADD | Unverified |
| 6 比較(明) | color MOD(T,D); alpha MOD(T,D) | DIS | A 灰度 | ONE | ONE | MAX | Unverified |
| 7 比較(暗) | color MOD(T,D); alpha MOD(T,D) | color MAA(C,F); alpha SEL1(C) | 0xFFFFFFFF | ONE | ONE | MIN | Unverified |
| 9 除外 | color MOD(T,D); alpha MOD(T,D) | DIS | A 灰度 | INVDESTCOLOR | INVSRCCOLOR | ADD | Unverified |

- 引擎在公共前缀 `SetTexture(2,NULL) → SetTexture(0,层纹理) → SetTexture(1,配对)`,
  以及 `SetPixelShader` 复位(行 162-165)。MODE 3/5 的 stage1 辅助纹理是
  `DAT_00801d98/d9c`(内容未知,DIV-GFX-001)。

**手册可见公式(只作名称对应,不是实现依据)**:

| MODE | 手册名 | 可见公式 | 实现状态 |
|---|---|---|---|
| 0 | 通常 | `src·a + dst·(1−a)` | Likely;按此实现 |
| 1 | 加算 | `src·a + dst` | Confirmed |
| 2 | 減算 | `dst − src·a` | Confirmed |
| 3 | 乗算 | `dst · src` | 状态已取证,公式未对拍 |
| 4 | 除算 | 手册名;公式未定位 | Unimplemented |
| 5 | スクリーン | `src + dst − src·dst` | 状态已取证,公式未对拍 |
| 6 | 比較(明) | `max(src, dst)` | 状态已取证,公式未对拍 |
| 7 | 比較(暗) | `min(src, dst)` | 状态已取证,公式未对拍 |
| 8 | 差分 | 手册名;公式未定位 | Unimplemented |
| 9 | 除外 | `src + dst − 2·src·dst` | 状态已取证,公式未对拍 |
| 10..18 | Overlay..PinLight | 手册名;着色器本体未定位 | Unimplemented |

- **MODE 3/5/6/7/9 的公式列只来自手册/Photoshop 家族命名,不作为实现依据**;
  后端只能下发状态表并等待像素对拍,或跳过。
- **旧线 03 文档的 D3DTA 映射错位**:它把 `0=TFACTOR,1=CURRENT,2=DIFFUSE,
  3=TEXTURE`;D3D9 实际是 `0=DIFFUSE,1=CURRENT,2=TEXTURE,3=TFACTOR`。
  因此旧线对 MODE 0“顶点色直通”的结论作废。本文按 `d3d9types.h` 重新解读。
- MODE 0 的 stage1 在单纹理时绑定 `SetTexture(1,NULL)`;D3D9 对未绑定 stage
  的精确可见行为(跳过/白纹理)本地未逐像素取证,故 MODE 0 标 **Likely**。
  手册的可见语义是“通常 α 混合”,后端按此实现并对拍。

### 5.3 着色器路径

`MODE 4/8/10..18` 在固定管线分支中没有 case;引擎持有 `ID3DXEffect`
(`DAT_00801e7c`)与 `SetPixelShader` 封装。着色器源/字节码落点未定位
(旧线 G7/G11),因此这些模式**一律 Unimplemented**;gfx 只保留 `MODE` 原值
与图层状态,不猜 Photoshop 公式。

### 5.4 实现状态

| MODE | `fixed_state()` | 后端 |
|---|---|---|
| 0 | 有 | 可执行;先按手册 α 混合,像素对拍后再转正 |
| 1/2 | 有 | 可执行 |
| 3/5/6/7/9 | 有 | 状态已取证;辅助纹理/运算本体未取证 ⇒ 后端应跳过 |
| 4/8/10..18 | 无 | 后端必须跳过 |

`yu-gfx` 的 `BlendMode::confidence()` 与 `docs/render-parity.md` 本表一一对应;
`json::blend_table_json()` 输出完整状态表供后端初始化。

---

## 6. 纹理过滤

- 全局设置,逐帧对 stage 0/1 下发(`FUN_0043cd6c` 行 46-117):
  MAG/MIN = `DAT_005b4804/4808`(0=POINT,1=LINEAR,2=ANISOTROPIC);
  MIP = `DAT_0080131c`(-1=NONE,0=POINT,1=LINEAR,2=ANISOTROPIC)。
- 不是逐层属性;`CG[MIPMAP=...]` 是另一条未取证路径,原样留档(DIV-GFX-011)。
- 旧线“固定 Linear”是已知 ❌;gfx 默认值明确为 POINT/POINT/NONE 占位
  (`filter.source=placeholder`),由宿主从引擎设置注入真实值
  (`GfxState::set_filter` → `filter.source=engine_settings`)。后端在
  `placeholder` 时不得声称过滤语义已对齐。
- 扩展名探测序来自 `yscfg +0x1c` 8 字节;`FUN_0040c9b4` 映射
  `1→.png, 2→.bmp, 3→.jpg, 4→.gif`;ERIS = `01 02 03 04 05`。

---

## 7. 变换与几何

- `X/Y` 存 i64 引擎像素;gfx 不做 16.16、不做半像素、不做缩放原点。
- 引擎绘制函数(`FUN_00414ba4` 行 117-125)把 `+0x1b0/1b8/1c0/1c8/1d0/1d8`
  除以 100.0,把 `+0x1f8/1fc/200` 的 f32 除以 100.0;行 220-275 用
  `ROUND(v * 65536)` 转 16.16 再拆分;行 322-336 构建旋转矩阵
  (`×π/180`,常量 0x58ebf8)。半像素:行 157/190/214 等有 `-0.5`
  (常量 0x58ebd0)。
- `FX/FY/FBX/FBY/FRX/FRY/FRZ/FCX/FCY/FEX/FEY/FEZ/FSD/FMD/FQU/FID/F`
  在 ERIS 里以 ×100 整数出现(例 `FX=@fx*100`、`FBX=10000`、`FCX=5000`)。
  绘制侧读取的字段已定位,但**槽位→字段的逐项绑定未取证**(旧线 G1)。
  gfx 将原值放进 `Transform.filter` 并标 `Unimplemented`;后端不得猜。
- UV 平铺/镜像(全局 `DAT_00801334` 1/2/3)未实现;`CGACT` 的
  `SIZE/ROTATION/...` 是图像加工,不是合成变换(DIV-GFX-013)。

---

## 8. 组 / 离屏 / COPY

- 组 = 离屏 pass + 一个 `Op::Group`。pass 顺序:所有 `Texture` 复制/图像加工 →
  组离屏(后序:子组先于父组)→ 屏幕。
- 组离屏清屏 = 透明黑(`[0,0,0,0]`),组自身 `A/MODE` 作用于合成结果;
  未给 `SX/SY` 的组尺寸未取证(DIV-GFX-008)。
- `CGACT[COPY/COPY2]`:
  - 手册:COPY=图像+属性,COPY2=仅图像;
  - 引擎时序(旧线实测):复制的是**上一帧已渲染**的目标,不是当前脚本状态;
  - 区域 `X/Y/X2/Y2` 原样保留;
  - 现状:`Op::Copy` + `CopyTiming::LastRenderedFrame` + `Unimplemented`
    (DIV-GFX-007)。目标层纹理 = `TextureRef::Snapshot(CopyId)`,后端在
    语义关闭前跳过。
  - `CompositorFrame.passes` 只含**可执行**复制(目标仍引用该快照且源图层
    仍存在);源已被 `CGEND` 删除的快照无法在渲染期重放,完整历史留在
    `GfxState::copies()` 审计日志里,对应图层标 `Unimplemented`。
- `SWAP/SWAP2`:`FUN_0042f57d` 行 61-112 交换纹理与 `+0x68/+0x70`(X/Y);
  模式 2 额外交换 `E/A/T/TA/MODE`。gfx 按此实现,SWAP/SWAP2 与模式 1/2 的
  精确对应为 Likely。

---

## 9. 过渡 `T/TA/TID`

- 门控(`FUN_00414ba4` 行 166-188):部件计数 0 或 `TA < 1` ⇒ 无过渡 pass
  (单纹理);否则 `BeginPass(1)` 双纹理。
- uniform:
  - `YS_Texture1` = 本层纹理;`YS_Texture2` = 配对纹理(TID 对侧);
  - `YS_AlphaVal = A/256`;
  - `YS_FadeVal = T / (256/(256/TA + 128)) * (1/256)`(行 905-909);
  - `YS_FadeVal2` 来源未取证。
- `Transition::fade_value()` 实现该公式;**不 clamp**;`TA<1` 返回 `None`。
  例:`T=256,TA=256 → 129/256`;`T=256,TA=1 → 1.5`。
- `TID` 配对层缺失/推进者(`+0x82` vs `+0x1e8`)未取证(DIV-GFX-006);
  gfx 在 `to=None` 时把层标 Unimplemented。

---

## 10. 文本

- 只定义模型:`TextStyle`(NAME/COLOR/COLOR2/SX/SY/BOLD/ANTIALIASING/
  DRAWMODE,另留 `line_spacing`/`stroke` 占位)+ `TextRun`(ID/CP932 字节/
  样式/原点/字形图集引用/帧号)。
- `FONT` 属性来自手册 `font/font.html`;`FONTINFO` 是只读查询,归 VM,
  gfx 不实现(诊断挂号)。
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

1. `PassTarget::Texture(id)`:按帧内顺序执行 `Op::Copy` / `Op::Image`。
   未实现的操作必须跳过(看 `status.executable=false`),不得近似。
2. `PassTarget::Offscreen(id)`:组的离屏合成。顺序 = 子组先于父组。
   清屏 = 该 pass 的 `clear`(透明黑)。组内 ops 按 `(z, seq, id)`。
3. `PassTarget::Screen`:最后执行;`clear` = `CompositorFrame.clear`;
   ops = **主窗口图层组**(默认 `:/W0`)的合成 op。子窗口/其他根组只以
   `Offscreen` pass 出现,由 window/host 会话负责窗口叠放与位置
   (手册 `window/window.html`;DIV-GFX-018)。

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
- 0/1/2 可执行;3/5/6/7/9 状态可下发但可见结果未对拍,默认应跳过;
  4/8/10..18 无状态,必须跳过。
- TFACTOR 规则:`AlphaGray` = `(a<<24)|(a<<16)|(a<<8)|a`,`a=min(max(A,0),255)`;
  `OpaqueWhite` = `0xFFFFFFFF`。

### 12.4 Filter

- `frame.filter.mag/min/mip` → 两个采样器(stage 0/1)的 MAG/MIN/MIP。
- `frame.filter.source=placeholder` 表示宿主未注入引擎设置;后端不得声称
  过滤语义已对齐。`engine_settings` 才是已取证值。
- 不做逐层过滤;`CG[MIPMAP]` 原样留档,不影响全局设置。

### 12.5 Group / Transition / Copy

- Group:先执行 `Offscreen` pass,再在父 pass 中把 `Op::Group.offscreen`
  当纹理,按 `layer.alpha/blend` 合成。
- Transition:`from`/`to` 两个纹理,`fade` 已按公式算好;`to=None` 时跳过。
- Copy:`status` 为 Unimplemented 时**不要执行**;`Snapshot` 纹理在
  语义关闭前无效。`frame.passes` 里的复制是可执行的;`GfxState::copies()`
  是完整审计日志(含源已删除的历史项),**不是**帧执行清单。
- Mask:`Op::Mask.status` 为 Unimplemented;不要用 α 或亮度猜。

### 12.6 Unimplemented 清单(后端必须跳过)

- MODE 4/8/10..18;MODE 3/5/6/7/9(状态可下发,结果未对证)。
- `MID` 掩码。
- `FX/FR/FC/FEX/...` 滤镜/仿射。
- 多部件层(非空 `parts`)。
- `CGACT[COPY/COPY2/TEXCOPY/TEXSSCOPY]` 与 `RECTPAINT*`/`SIZE*`/`ROTATION`/
  `MONO`/`BLUR*`/... 图像加工。
- `TID` 缺失的过渡。
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

---

## 14. 复现

```bash
export TEMP="<side-repo>/workbench/tmp"; TMP="$TEMP"
export YURIS_SAMPLE_ERIS_YSBIN="<side-repo>/workbench/project-eris/save/data/ysbin"
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
