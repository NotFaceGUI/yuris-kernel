# 引擎着色器 1:1 台账(任务 J4)

> 唯一台账:引擎 shader 字节码 ↔ 参数/采样器 ↔ 调用点 ↔ 模式/功能 ↔
> 本仓库实现状态。证据产物(`.cso`/`.asm`/`.fxo`)只放 `target/tmp/j4-shaders/`,
> 不入库;本文件只保留语义描述。
>
> 真值层级:官方 exe 可观测行为/字节码 > SDK 0.495 逆向事实 > 真实语料 >
> 旧接手线。引用地址 = PE 文件偏移(同时给 VA);反汇编工具 =
> `fxc /dumpbin`(Windows SDK 10.0.22621 fxc,可正确解 ps_2_0)。
>
> **范围**:本台账只覆盖引擎主体 `yu-ris.exe` 的 FXO。E-mote 是可选第三方
> 插件运行时(`YSEmote.dll` → `emotedriver.dll`),自带 58 个 shader,
> 单独记在 `docs/emote-parity.md`;不计入“引擎内置 shader”的 4 个。

## 0. 证据源

| 项 | 值 |
|---|---|
| 官方引擎 | SDK 0.495 `yu-ris.exe`(sha256 `4f5eefa3544c7244a907477d89ccee97286f559df5e38b1b1fa674206981138f`) |
| PE | image base `0x400000`;`.rdata` 文件 `0x18ae00` ↔ RVA `0x18c000` |
| 扫描器 | `target/tmp/j4-shaders/scan_shaders.py`(全 exe 4 字节对齐枚举 D3D9 version token,按 token 流走到 `END=0x0000FFFF`;注释 token `0xFFFE` 长度在高 16 位) |
| 反汇编 | `target/tmp/j4-shaders/{psA,psB,psC,psD}.cso/.asm` |
| FXO 容器 | `target/tmp/j4-shaders/effect.fxo`(从 VA `0x58d420` 起 `0x1540` 字节) |
| FXO 解析依据 | Wine `d3dx9_36/effect.c` 的 `d3dx9_effect_init_from_binary` / `d3dx_parse_effect` / `d3dx_parse_effect_pass` / `d3dx_parse_resource`(公开实现,用于字段顺序;在本文件 `target/tmp/j4-shaders/wine_effect.c` 留档) |

## 1. 全量候选扫描(审查报告 §1.4 的 13 项复核)

对全 exe 扫描共 76 处 version-token 值。**能走到 END 且前面有精确长度前缀的
只有 4 个,全部 ps_2_0**;其余是代码立即数、CTAB 内嵌字段或镜像数据表:

| 文件偏移 | 值 | 判定 | 证据 |
|---|---|---|---|
| `0x18c9a8` | `0xFFFF0200` ps_2_0 | **真 blob #1**(1432 B) | 走到 `END`;长度前缀 `0x598` 在 `0x18c9a4` |
| `0x18cf58` | `0xFFFF0200` ps_2_0 | **真 blob #2**(604 B) | 前缀 `0x25c` |
| `0x18d1cc` | `0xFFFF0200` ps_2_0 | **真 blob #3**(492 B) | 前缀 `0x1ec` |
| `0x18d3d0` | `0xFFFF0200` ps_2_0 | **真 blob #4**(792 B) | 前缀 `0x318`;含 preshader |
| `0x18c9bc/0x18cf6c/0x18d1e0/0x18d3e4` | `0xFFFF0200` | 假:四个 blob 的 CTAB 内 `Version` 字段 | 流在 1 条指令内断(操作数被当 opcode) |
| `0xe7bac/0xe7bc8/0xe7c2c/0xe7c4c`;`0xf8c48/0xf8c8c/0xf8d04`;`0x15598c` | `0xFFFE01xx`/`0xFFFF01xx`/`0xFFFF03xx` | 假:`cmp reg, imm32` 的版本常量 | 反汇编 `3D 01 01 FE FF` = `cmp eax, 0xFFFE0101` 等;立即数后接 `JZ` |
| `0x1957a0..0x19598c` | 成串 `ps_/vs_` 版本 | 假:D3DX9 静态库的版本字符串/表 | 同地址段落里是 0 长度操作数,流立即断 |
| `0x1cb66c..0x1cc490` | `ps_1_3/1_6/1_4/1_7` | 假:ps_1_x opcode 元数据表(0xFFFF 间隔) | 相邻字是 `0xFFFF`;流断 |
| `0x12a0c..0x12ab0` | `vs_3_0/ps_3_0/ps_1_4...` | 假:代码里的比较立即数 | 同上 |

**结论(推翻审查报告 §1.4 的“13 个真实候选”)**:

1. 引擎主体只有 **4 个 PS_2_0 着色器**,全部在同一个编译后 effect 容器里;
   (E-mote 插件的 58 个 shader 属于独立运行时,见 `docs/emote-parity.md`。)
2. **没有 `ps_1_1`/`ps_1_4`/`ps_3_0` 字节码**;对应命中均为版本常量或元数据表;
3. **没有 `vs_1_1` 顶点着色器**;`0x15598c` 是 `cmp edx, 0xFFFE0101`,位于
   D3DX9 静态库的版本校验代码;引擎顶点处理是固定管线 FVF
   (`XYZRHW|DIFFUSE|TEX1/4/8`,见 §4);
4. 因此“ps1/3/5/6/7 反汇编错乱/为空”不是解析器缺陷,而是 J2 把
   **CTAB 内嵌 Version 字段**、以及**截断的 ps6** 当成独立 shader 提取:
   - `ps1/ps3/ps5/ps7` = 四个 blob 的 CTAB 起始(不是代码);
   - `ps6` 只截了 656 B(preshader 部分),真实长度 792 B,所以 `fxc`
     输出为空;按 792 B 重提后完整反汇编(§2 #4)。

## 2. 四个真实 blob

四个 blob 都在 VA `0x58d420` 起、长度 `0x1540` 的 **D3DX9 编译 effect(FXO)**
容器内(`0x18c220` 文件偏移;magic `0xFEFF0901`)。`0x18c9a8` 等是容器内偏移
`0x788/0xd38/0xfac/0x11b0`。

> J2 编号对照(旧文件 `target/tmp/j2-shaders/ps0..ps7`):
> `ps0`=psA、`ps2`=psB、`ps4`=psC、`ps6`(截断)=psD 的前半;
> `ps1/ps3/ps5/ps7` 是 CTAB 内嵌 Version 字段,不是 shader。

| # | 名称 | 文件偏移 | VA | 大小 | pass | 模型 | 参数/采样器 | 反汇编摘要 | 对应功能 |
|---|---|---|---:|---:|---|---|---|---|---|
| 1 | psA | `0x18c9a8` | `0x58dba8` | 1432 | Pass3 | ps_2_0 | `Samp3`(s0),`YS_TexSize`(c0),`YS_TexDiffU`(c1) | `t0..t7`;`frc(t*TexSize)` 生成 4×4 三次权重(`2/-2/1/-1` 常量),16 次 `texld`(t0..t7 及 `+TexDiffU` 偏移),加权累加;`oC0=Σ` | **16-tap 双三次缩放滤镜**(引擎放大过滤“最高”档) |
| 2 | psB | `0x18cf58` | `0x58e158` | 604 | Pass2 | ps_2_0 | `Samp3`(s0),`YS_TexSize`(c0),`SharpN`(c1) | `t0..t3`;`frc(t*TexSize)±0.5`,`mad_sat(...,SharpN,0.5)` 生成权重,4 次 `texld`,`lrp_sat`;`oC0` | **4-tap 锐利双线性缩放滤镜**(“高”档) |
| 3 | psC | `0x18d1cc` | `0x58e3cc` | 492 | Pass1 | ps_2_0 | `Samp1`(s0),`Samp2`(s1),`YS_AlphaVal`(c0),`YS_FadeVal`(c1),`YS_FadeVal2`(c2) | `tex1=s0`,`tex2=s1`;`a=tex1.a*saturate((FadeVal-0.5*tex2.r)*FadeVal2)*AlphaVal`;RGB=`tex1.rgb` | **TID/mask 转场(HLSL 路径)**;mask 取配对纹理 **R** 通道 |
| 4 | psD | `0x18d3d0` | `0x58e5d0` | 792 | Pass0 | ps_2_0 + preshader | `Samp1`(s0),`Samp2`(s1),`YS_AlphaVal`(c0),`YS_FadeVal`(c1) | preshader:`c2.x=1-FadeVal`;像素:`oC0=tex1*(1-FadeVal)+tex2*FadeVal`;`oC0.a*=AlphaVal` | **交叉淡化转场**(RGB+α 线性插值) |

反汇编原文见 `target/tmp/j4-shaders/psA..psD.asm`;FXO 内 `CTAB` 参数表列出
`YS_Texture1/2/3`、`YS_AlphaVal`、`YS_FadeVal`、`YS_FadeVal2`、`YS_TexSize`、
`YS_TexDiffU`、`SharpN`、`Samp1/2/3`,与四个 shader 的参数集一致。

## 3. 调用链(REA + 反编译)

### 3.1 effect 的创建

| 地址 | 事实 |
|---|---|
| `0x462d28` `FUN_00462d28` | `memcpy(stack, 0x58d420, 0x1540)` 后用 `D3DXCreateEffect`(`0x4cd434`)创建 effect;`0x4cd434` 检查首 DWORD `0xFEFF0901`(编译 effect magic) |
| `0x462d7f..0x462da2` | `GetParameterByName("YS_ShaderTest1")` 后 `SetTechnique`;参数串在 `0x80b51c` |
| `0x413b20` | 销毁 effect(`DAT_00801e7c=0`) |
| 导入表 | **只导入 `Direct3DCreate9`**;设备方法全部走 `IDirect3DDevice9` 虚表。`CreatePixelShader` 由静态链接的 D3DX9 在 `D3DXCreateEffect` 内部调用,所以 **exe 字符串里没有 `CreatePixelShader`** |

FXO 结构(按 Wine 解析器解码):

```text
magic=0xFEFF0901
offset=0x5FC → 参数表:12 个参数(9 个 uniform + 3 个 sampler)
technique=1  "YS_ShaderTest1" → 4 个 pass
pass0 → object id 7 → 791 B  → psD(交叉淡化)
pass1 → object id 8 → 492 B  → psC(mask 转场)
pass2 → object id 9 → 604 B  → psB(4-tap)
pass3 → object id 10 → 1432 B → psA(16-tap)
资源表另有 3 个 sampler 状态(引用 Samp1/2/3)
```

### 3.2 绘制点(`FUN_00414ba4`,逐层绘制)

| 条件 | 选择 | REA/反编译 |
|---|---|---|
| 公共前缀 | `device->SetPixelShader(0)` 复位 | vtable `+0x1ac`,地址 `0x414dfa` |
| 无多部件或 `TA<1` 且无过渡配对 | 固定管线(`iStack_68=0`) | `0x414e..` |
| 无多部件或 `TA<1`,有过渡配对 | `BeginPass(0)` → **psD 交叉淡化** | vtable `+0x108`,地址 `0x416b80` |
| 多部件且 `TA>=1` | `BeginPass(1)` → **psC mask 转场** | `0x416b..` |
| MODE ∈ {0,1,2,3,5,6,7,9,100,98} | 固定管线状态表(不走 effect) | 同函数 0x415..0x417.. |
| MODE 4/8/10..18 | **无分支**:既不走固定管线状态表,也不因 MODE 进入 effect 块 | 全函数体(`0x414ba4..0x4171f0`,9804 B)字节扫描:只有对 `1/2/3/5/6/7/9` 的 `CMP`,没有任何对 `4/8/10..18` 的比较;`0x416ad1` 是 `MOV EDX,[0x801e7c]`(effect 指针),按转场条件(TID/TA)进入,与 MODE 无关(2026-10-30 更正) |

HLSL 参数设置(REA 地址,均在 `0x416a..0x416e`):
`YS_Texture1`、`YS_Texture2`、`YS_AlphaVal=A/256`、`YS_FadeVal`、
`YS_FadeVal2`;`CommitChanges` 后 `DrawPrimitiveUP`(`0x416f..`)。

### 3.3 显示缩放滤镜(`FUN_004195ec`)

| 全局/条件 | 选择 |
|---|---|
| `DAT_00801334 == 2` | `BeginPass(2)` → psB;FVF `0x444`(XYZRHW+DIFFUSE+TEX4,4 组 UV) |
| 否则 | `BeginPass(3)` → psA;FVF `0x844`(TEX8,8 组 UV) |
| `DAT_00801334 ∈ {0,1}` 或 `DAT_008012e4/e8` 为 0 | 不走 effect(`0x462e40` 的 FVF 分支不进入) |

`DAT_00801334` 在 `0x460924` 由配置字段 `piVar2[8]` 映射:
`0→3, 1→2, 2→1, 3→0`;`FUN_0041a308` 按 2/3 生成 4/8 组 UV。

**采样器(2026-10-22 复核)**:
- **全局帧采样**走 `FUN_0043cd6c`:配置枚举 `0/1/2` 分别映射到 D3D9
  `D3DTEXF_POINT/LINEAR/ANISOTROPIC`(值 1/2/3)。`DAT_005b4804/4808/80131c`
  = 1/1/0,所以全局是 **LINEAR/LINEAR/POINT**(D3DTEXF 2/2/1),无 mip 链。
  旧文写的 “MAG=1(LINEAR)” 用的是**引擎配置枚举**,结论正确。
- **显示缩放 pass** 在 `FUN_004195ec` 的 `0x419832..0x41988d` 直接
  `SetSamplerState(0, 5/6/7, 1)`;D3D9 `D3DTEXTUREFILTERTYPE` 是
  `NONE=0 / POINT=1 / LINEAR=2 / ANISOTROPIC=3`,所以这里是
  **POINT/POINT/POINT**,与全局设置无关。
- psA/psB 的 16/4 个 tap 因此是**逐 texel POINT 取值**,不是线性采样;
  本仓库用显式 `floor(u*W)` + `textureLoad` 复刻 D3D9 的 floor 规则,避免
  WebGPU nearest 在整 texel 边界上的 tie-break 差异(1:1 必须逐像素一致)。

**UV 与参数(`FUN_0041a308`,`0x41a308`,已逐行复核)**:

```text
W = DAT_00801304,H = DAT_00801308(源纹理尺寸;同时作为 YS_TexSize)
u,v = 输出像素对应的源 texel-center 坐标(q)归一化值

DAT_00801334 == 2(psB,4 组 UV):
  t0=(u,     v    )  t1=(u+1/W, v    )
  t2=(u,     v+1/H)  t3=(u+1/W, v+1/H)

DAT_00801334 == 3(psA,8 组 UV):
  t0=(u-1/W, v-1/H)  t1=(u-1/W, v    )  t2=(u-1/W, v+1/H)  t3=(u-1/W, v+2/H)
  t4=(u,     v-1/H)  t5=(u,     v    )  t6=(u,     v+1/H)  t7=(u,     v+2/H)

0x419f72..0x419faa:`YS_TexDiffU = (2/W, 0)`(仅 psA)
0x419dbe..0x419e13:`YS_Texture3` + `YS_TexSize = (W, H)`
SharpN:反汇编只在 FXO 参数初值出现(`0x3FA00000 = 1.25`),引擎从不 SetValue。
```

psA 的像素 shader 把 `YS_TexDiffU` 加到 t0..t7,等价于把 4 组行 UV 扩展成
`origin=(u-1/W, v-1/H)` 起、步长 `(1/W,1/H)` 的 4×4 网格;psB 是 2×2 网格。
顶点在 CPU 侧按 `FUN_0041a308` 生成后用 `DrawPrimitiveUP(stride=0x54)`
绘制,无顶点着色器。

## 4. 与模式/图像加工的映射结论

| 审查假设 | 结论 | 证据 |
|---|---|---|
| `ps_1_4` 三个 blob ↔ `CGACT MONO/BLUR2..6` | **Refuted**:不存在 ps_1_4 blob;`CGACT` 的图像加工是 CPU 像素路径(`FUN_0042f809` 动作分派),不引用 `DAT_00801e7c` | 全 exe 扫描 §1;`0x801e7c` 的 34 个 xref 无 `0x42f809` |
| 图像处理 shader 在 effect 里 | **部分成立但用途不同**:psA/psB 是**显示缩放滤镜**(pass2/3),不是 `CGACT` BLUR/SHARP | §3.3;`FUN_004195ec` 的 pass 选择 |
| `vs_1_1` 用于 E-mote/变形/2D title | **Refuted**:无 vertex shader;顶点由 CPU 生成后 `DrawPrimitiveUP`,固定 FVF | §1;`0x41a308` 顶点写入 + `0x462e40` `SetFVF` |
| 11 个“着色器模式”(MODE 4/8/10..18) 各有一个 blob | **未成立**:effect 只有 4 个 pass(交叉淡化 / mask 转场 + 两种缩放滤镜),没有一个是混合模式;而且这 11 个 MODE 值在 `FUN_00414ba4` 里**根本没有分支**(全函数体扫描无对应 `CMP`),既不走固定管线也不进 effect 块 ⇒ 官方未实现,不是"公式未定位" | FXO pass 表;`FUN_00414ba4` 全函数体扫描(2026-10-30 更正) |
| 本语料 `_DXMODE=0` | 与引擎无关的**脚本分支**:`@_DXMODE` 决定脚本走 `CGACT[TRANSITION]`/`TID`(0)还是 `DXBUF` 分支(≠0);引擎 effect 始终创建 | `es_first/es_Sprite` 脚本;EVID-T-08 |

## 5. 实现状态(本任务)

| 项 | 状态 |
|---|---|
| 引擎 blob 提取/反汇编 | **完成**(4/4,含 preshader) |
| 调用点与 pass 映射 | **完成**(§3) |
| 固定管线 MODE 0/1/2 | 已实现;MODE 2 的 RGB 在本任务修正为 `T·A`(此前漏乘 A) |
| 固定管线 MODE 3/5/6/7/9 | **已实现**:soft 与 wgpu 全部执行;wgpu 用 `Src/OneMinusDst/OneMinusSrc` 对应 D3D9 颜色因子,MAX/MIN 对应 6/7 |
| psC(TID HLSL) | **独立 WGSL** `shaders/transition_psc.wgsl`;`dx_mode≠0 && TA>=1` 启用 |
| psD(交叉淡化) | **独立 WGSL** `shaders/transition_psd.wgsl`;`dx_mode≠0 && TA<1` 启用 |
| psA/psB(缩放滤镜) | **独立 WGSL**:`shaders/scale_psa.wgsl`(Pass3)/`shaders/scale_psb.wgsl`(Pass2),逐指令等价(含 `SharpN=1.25`、`YS_TexDiffU=2/W`、显式 floor tap)。接入点为加法 API `WgpuRenderer::render_frame_scaled(frame, output, ScaleFilter)`;`render_frame` 仍 1:1 不变;缩放是呈现步骤,不进 `CompositorFrame` |
| psA/psB 对拍 | 单测 `display_scale_psa/psb_matches_reference_{upscale_and_downscale}`:GPU 与同式 CPU 参考在 2×/0.625× 逐通道 ≤1;`display_scale_1to1_reproduces_composited_screen`:1:1 与 `render_frame` ≤1 |
| 顶点 DIFFUSE | **已定案**:`FUN_00414ba4` asm `0x414d09..0x414d21` 写 `A` 灰度;shader 模式写 `0xFF000000`。以 `T·A`/`T·A+(1−C.a)` 建模,不新增顶点 RGB 属性 |
| MIP/aniso | **引擎硬编码**配置值 1/1/0 → D3DTEXF `LINEAR/LINEAR/POINT`(2/2/1)、aniso=0、无 mip 链(`FUN_00460924` 尾部);后端 LOD0/aniso=1 等价,不是静默忽略 |
| 采样器过滤来源 | `GfxConfig.filter` 仍为宿主注入契约;引擎配置真值 = 1/1/0(LINEAR/LINEAR/NONE) |
| bind group | paint 按 `(alpha_only, color)` 跨帧缓存;transition 仍按 draw 新建(fade/目标纹理逐帧变化,不能无成本复用) |

## 6. 待门禁/需要主人确认

1. **`dx_mode` 契约(已按加法实现)**:引擎侧没有渲染全局 `_DXMODE`;
   `@_DXMODE` 只改脚本。本任务把宿主注入值送入
   `GfxConfig.dx_mode` → `CompositorFrame.dx_mode`(默认 0),非 0 时后端
   选择 psC/psD。宿主交接:CLI/wasm 需要把 `BootConfig.dx_mode` 同步传给
   `GfxConfig`(一行);当前示例用 `YURIS_DX_MODE` 环境变量做 A/B。
   **示例验收**:`YURIS_DX_MODE=0/1` 各跑 600 帧,soft/wgpu 稳态同哈希;
   dx1 有 1 帧(max 3/255,量化)soft/wgpu 差异,阈值声明见 §7。
2. **MODE 4/8/10..18:官方无实现(2026-10-30 定案,`unsupported-by-official`)**。
   `FUN_00414ba4` 全函数体只有 `1/2/3/5/6/7/9` 的 `CMP`,这 11 个值不设任何
   D3D 状态、也不进 effect 块 ⇒ 不存在"待定位的 Photoshop 公式",不要再按
   `Unimplemented` 挂账。内核按官方行为对齐:归约到通常路径绘制
   (`BlendMode::engine_effective`),置信度 `Unsupported`。
3. **组(layer set)上的 TID**:gfx 已输出 `Op::Transition { from: Offscreen }`,
   配对可为图像/组;引擎按层表统一解析。标 `Likely`,待官方微场景。
4. psA/psB 已按 `DAT_00801334` 的 pass2/pass3 接线(`render_frame_scaled`);
   非 effect 档(引擎值 0/1)的固定管线缩放路径仍无像素证据,调用方传
   `ScaleFilter` 之外不伪造。宿主若要把窗口缩放交给后端,只需在
   `render_frame`/`render_frame_scaled` 之间选择,并传窗口尺寸。
5. **官方微场景工具阻塞**:`yscom_harvest` 启动的 YSCom 在
   `state0 != 1` 处不 attach(120s 超时;LE 与非 LE 均复现,
   `target/tmp/j4-micro-modes2/harvest.log`)。MODE 3/5/6/7/9 的官方像素
   对拍因此待工具恢复;当前以 D3D9 状态表 + soft/wgpu 端到端逐像素一致为证据。

## 7. 声明阈值

| 项 | 阈值 | 实测 |
|---|---|---|
| 固定管线 MODE 0/1/2(600 帧) | soft/wgpu 逐字节 0 差异 | **0**(本任务复跑) |
| dx_mode=0 → 1 的 title 稳态帧 | 0 差异(过渡结束后) | **0**(`6280eac4…`) |
| dx_mode=1 过渡帧 soft/wgpu | 每通道 ≤4/255;声明为量化残差 | 仅 tick 35 一帧,max 3,0 像素 >4 |
| MODE 3/5/6/7/9 单元向量 | 与状态表逐位一致 | soft 单元测试覆盖 stage 归约+blend;wgpu 颜色因子状态测试 + `fixed_pipeline_modes_3_5_6_7_9_match_soft` 两端逐像素一致 |
| dx_mode=1 vs 0 | 不要求相同(两条引擎路径) | 47 帧不同(过渡区间),稳态相同 |

