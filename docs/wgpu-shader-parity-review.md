# wgpu 着色器 / 通用性 / 1:1 审查报告(任务 J 线)

> 审查对象:`crates/yu-gfx`、`crates/yu-backend-soft`、`crates/yu-backend-wgpu`
> 的着色器与混合路径,以及 J1/J2/J3 的交付。
> 证据:源码、`docs/title-motion-ledger.md`、`docs/fidelity-ledger-j.md`、
> `docs/render-parity.md`、`PROGRESS.md` 成果 J1/J2/J3、
> 官方 exe 提取的 PS_2_0 字节码(`target/tmp/j2-shaders/`,不入库)。
> 本报告只读审查;未改代码。

## 0. 结论摘要

1. **ERIS title 在 `_DXMODE=0` 固定管线路径上已经跑通**(J3:600 帧
   soft/wgpu 0 差异、浏览器 canvas 与 native 同哈希、按钮列补齐、
   gfx golden 重生成后全绿)。
2. **但着色器远未 1:1**:对整个 `yu-ris.exe` 做字节码扫描,至少检出
   **13 个真实 shader 候选**,跨 `ps_1_1 / ps_1_4 / ps_2_0 / ps_3_0 / vs_1_1`
   多种模型;当前只提取了其中 **8 个 ps_2_0**,wgpu 只有 **3 个内联 WGSL**
   (quad / transition / paint);引擎的 HLSL 路径(`ps4` 转场等)**没有移植**,
   `MODE 4/8/10..18` 共 **11 个着色器模式**仍未实现。
3. **引擎的双路径选择没有建模**:引擎按 `_DXMODE` 在固定管线与
   `SetPixelShader` 之间切换;`dx_mode` 目前只在 `BootConfig` 里,
   `CompositorFrame` / gfx / 后端都拿不到,后端永远走固定管线近似。
4. **通用性**:核心代码没有 ERIS 专用分支(只有注释里的证据引用和示例);
   但存在两个通用性瑕疵——转场覆盖度是拟合模型(DIV-GFX-045)、
   `image_probe_order` 在 yscfg 缺失时静默用默认值(A5 风险)。
5. **文档有自相矛盾**:`title-motion-ledger.md` §8.5 说转场 shader 已改为
   新公式,后文(?436)又说“仍是旧公式”;代码实际是新公式。需要一次勘误。

## 1. 当前着色器清单

### 1.1 wgpu 侧(3 个,全部内联字符串)

| 位置 | 作用 | 备注 |
|---|---|---|
| `src/pipelines.rs:92` `SHADER` | 普通纹理 quad(顶点色 alpha + 采样 + blend 状态) | 所有固定管线 MODE 0/1/2 复用 |
| `src/pipelines.rs:129` `TRANSITION_SHADER` | 双纹理转场(mask G + fade 覆盖度 + u8 量化链) | 复刻**固定管线**路径,不是引擎 HLSL |
| `src/pipelines.rs:188` `PAINT_SHADER` | 快照上的 RECTPAINT 族(整像素 / 只写 alpha) | COPY 目标纹理原地绘制 |

没有独立的 `.wgsl` 文件、没有 shader registry、没有“引擎 shader → WGSL”映射表。
过渡公式在 Rust(`yu_gfx::transition_coverage`, `layer.rs:893`)与 WGSL 里
各写了一份,必须逐位同步(维护风险)。

### 1.2 引擎侧(已提取 8 个 PS_2_0 字节码)

`target/tmp/j2-shaders/`(证据产物,不入库):

| blob | 地址 | 参数(反汇编可见) | 推测作用 | 反汇编状态 |
|---|---|---|---|---|
| ps0 | `0x18c9a8` | `Samp3`, `YS_TexDiffU`, `YS_TexSize` | 纹理采样/位移类(非转场) | 完整 |
| ps1 | `0x18c9bc` | — | 未知 | 反汇编错乱 |
| ps2 | `0x18cf58` | `Samp3`, `SharpN`, `YS_TexSize` | 锐化/采样 | 完整 |
| ps3 | `0x18cf6c` | — | 未知 | 反汇编错乱 |
| ps4 | `0x18d1cc` | `Samp1`, `Samp2`, `YS_AlphaVal`, `YS_FadeVal`, `YS_FadeVal2` | **转场(HLSL 路径)** | 完整 |
| ps5 | `0x18d1e0` | — | 未知 | 反汇编错乱 |
| ps6 | `0x18d3d0` | — | 未知 | 空 |
| ps7 | `0x18d3e4` | — | 未知 | 空 |

已确认的 ps4 公式(`EVID-T-09`):

```text
oC0.rgb = tex1.rgb
oC0.a   = tex1.a * saturate((YS_FadeVal - 0.5*tex2.r) * YS_FadeVal2) * YS_AlphaVal
```

注意它用 **tex2.r**(红通道),而当前固定管线模型用 **mask.g**(绿通道);
两者不是同一个引擎路径,不能互相替代。

### 1.3 映射缺口

- 引擎 19 个 blend 模式中:固定管线可执行 3(0/1/2)、状态已取证但未对拍 5
  (3/5/6/7/9)、**着色器模式 11**(4/8/10..18)未实现;
- 8 个 PS blob 与 11 个模式不是一一对应(可能复用/参数化),
  `CreatePixelShader`/`SetPixelShader` 的调用点尚未逐个映射到 MODE;
- 因此“引擎有多少 shader、每个什么时候用”目前没有完整答案。

### 1.4 全量 exe 扫描(2026-10-21 深审补充)

对 `yu-ris.exe` 按 D3D9 version token 做全量扫描,过滤明显的误报
(token 出现在数据区、无法走到 END)后,**至少 13 个真实候选**:

| 模型 | 文件偏移 | 大小 | 说明 |
|---|---|---:|---|
| ps_1_1 | `0xe7bc8` | 6,412 | 未提取;疑似图像处理/旧模型 |
| ps_3_0 | `0xe7c4c` | 6,280 | 未提取 |
| vs_1_1 | `0x15598c` | 924 | **顶点着色器**,未提取;可能用于 E-mote/特殊绘制 |
| ps_2_0 ×8 | `0x18c9a8..0x18d3e4` | 472..1,432 | J2 已提取的 8 个(ps0..ps7) |
| ps_1_4 ×3 | `0x1cbea0/0x1cc078/0x1cc32c` | 4,684..5,848 | 未提取;疑似 CGACT 图像处理(MONO/BLUR 等) |

其他事实:
- `ID3DXEffect` 字符串出现 **56 次**,`SetPixelShader` 2 次;
  未见 `CreatePixelShader`/`D3DXCreateEffect` 字符串(可能走导入/虚表),
  调用点需用 REA 追;
- 之前 §1.2 的“8 个”只是 **ps_2_0 子集**,不能代表引擎 shader 全集;
- ps1/3/5/6/7 的反汇编失败/为空,可能是解析器问题,不代表 blob 无效。

## 2. 通用性审查

### 2.1 通过

- `grep -rniE "eris|cgsys|spl101|spltr|btn_" crates/*/src` 在核心代码中
  **没有可执行分支**,只有注释证据和示例;转场/COPY/按钮逻辑都由
  `CompositorFrame` 与工程参数驱动。
- 按钮 `_off/_on/_over/_na` 由脚本 `FILEINFO[EXIST]` 探测,
  内核只补通用 `FILEINFO`(J3);没有为 ERIS 写死后缀。

### 2.2 不通过/需修

| 问题 | 位置 | 影响 |
|---|---|---|
| 转场覆盖度是拟合式 `(30+70*fade)*(mask-(1-2*fade))`,TA=128 校准 | `yu-gfx/src/layer.rs:893`、`pipelines.rs:159` | TA≠128 时最大偏差 ~0.12(DIV-GFX-045),不是精确引擎公式 |
| 固定管线与 HLSL 双路径没有选择 | `dx_mode` 只在 `BootConfig`;`CompositorFrame`/gfx/后端无该字段 | 所有工程被迫走固定管线近似;`_DXMODE≠0` 的工程转场会错 |
| `image_probe_order` 缺省兜底 | `yu-gfx/src/state.rs:110-115` 保留 `GfxConfig` 默认 | yscfg 缺表时不拒绝启动,违反 A5 |
| `SYSTEM_START` 默认值 | `yu-rt/src/vm.rs:103` | 调用方可覆盖,但仍是内置约定;跨作品为 Likely |
| 公式双写(Rust/WGSL) | `layer.rs` + `pipelines.rs` | 一处改动漏同步就 soft/wgpu 分叉 |
| 示例 ERIS 耦合 | `examples/title_probe.rs` 等 | 开发探针可接受,但需标注“非核心” |

## 3. 1:1 审查(按层)

| 层 | 状态 | 说明 |
|---|---|---|
| 固定管线 MODE 0/1/2 | 接近 | J3 在 ERIS `_DXMODE=0` 下 600 帧 soft/wgpu 0 差异;MODE 0 仍标 Likely(stage1 NULL 行为未逐像素取证) |
| MODE 3/5/6/7/9 | **未 1:1** | gfx 有 `fixed_state()` 状态表,Backend 只支持 0/1/2;这些模式被跳过(状态未对拍) |
| MODE 4/8/10..18 | **未实现(2026-10-30 定案为官方无实现)** | 本行原判断"依赖引擎 shader"已被推翻:`FUN_00414ba4` 全函数体无对应 `CMP`,官方不设任何 D3D 状态 ⇒ `unsupported-by-official`,内核归约到通常路径(`BlendMode::engine_effective`),见 `docs/shader-parity.md` §3.2 |
| 转场(固定管线) | 近似 1:1 | ERIS 路径对拍通过;TA≠128 残差 DIV-045 |
| 转场(HLSL) | **未实现** | 引擎 ps4 公式已取证,未移植;`_DXMODE≠0` 时错误 |
| 顶点 DIFFUSE RGB | **未建模** | MODE 2/3/5/6/7/9 的 stage0 依赖 diffuse;当前按白/纹理处理(DIV-GFX-019) |
| 颜色空间 | Likely | 未用官方截图逐像素验证(DIV-GFX-009) |
| 过滤 | 部分 | MIP 忽略、Anisotropic 等同双线性(DIV-GFX-009/审查 P2-7) |
| 半像素/quad 相位 | 未 1:1 | 1px 边缘差(DIV-GFX-042) |
| 组 RT alpha/主组直绘 | 未裁决 | title 平坦区不依赖;通用半透明仍偏差(DIV-GFX-008/041) |
| 文本/MID/多部件 | 未实现 | 与 shader 无关,但同属渲染 1:1 缺口 |

**结论**:对 ERIS title(`_DXMODE=0`、固定管线、TA=128)已经达到
“可验收的一致”;对引擎整体(尤其 shader 路径与 11 个模式)还**不是 1:1**,
当前状态应标注为“ERIS title 路径 1:1 候选,shader 模式未覆盖”。

## 4. 问题清单(按优先级)

| ID | 级别 | 问题 | 证据/位置 |
|---|---|---|---|
| SH-01 | P0 | 引擎 8 个 PS blob 未全部反汇编/映射到 MODE | `target/tmp/j2-shaders/ps1/3/5/6/7`;`render-parity.md` §5.3 |
| SH-02 | P0 | `_DXMODE` 双路径未建模,后端永远走固定管线 | `grep dx_mode` 仅 `BootConfig`;`transition.rs` 注释 |
| SH-03 | P1 | 11 个 shader 模式未实现 | `layer.rs:530 needs_shader`;`render-parity.md` §5.3 |
| SH-04 | P1 | 引擎 HLSL 转场(ps4,tex2.r)未移植 | EVID-T-09;当前 WGSL 用 mask.g |
| SH-05 | P1 | 转场固定管线模型为拟合式,TA≠128 残差 | `layer.rs:893`;DIV-GFX-045 |
| SH-06 | P1 | 5 个固定管线模式(3/5/6/7/9)状态未对拍且后端不支持 | `layer.rs:508` confidence;后端 `supported_blend` |
| SH-07 | P2 | Rust/WGSL 公式双写,无同步守卫 | `layer.rs` + `pipelines.rs` |
| SH-08 | P2 | `image_probe_order` 缺省兜底(A5) | `state.rs:110-115` |
| SH-09 | P2 | 文档自相矛盾(转场旧公式段落) | `title-motion-ledger.md` §8.5 vs 后文 |
| SH-10 | P3 | shader 以内联字符串存在,无独立文件/注册表 | `pipelines.rs` |
| SH-11 | P1 | 两端 `supported_blend` **只接受 0/1/2**;gfx 已有 3/5/6/7/9 状态但后端拒绝 | `soft/render.rs:1043`、`wgpu/passes.rs:1341` |
| SH-12 | P1 | `Vertex` 只有 position/uv/alpha,**没有 DIFFUSE RGB**;而 MODE 2/3/5/6/7/9 的 `fixed_state` 使用 Diffuse | `wgpu/textures.rs:387`;`layer.rs` 各 state |
| SH-13 | P1 | 采样器固定 LOD0 / anisotropy=1:**MIP 与各向异性被静默忽略** | `pipelines.rs:713-727` |
| SH-14 | P2 | transition/paint 的 bind group 每条 draw/paint 新建,未缓存 | `passes.rs:586/937` |
| SH-15 | P2 | 样本 gate 不一致:README 用 `ROOT/YPF`,但 `filter.rs` 只认 `YSBIN`;只设 ROOT 时 `REQUIRE_SAMPLES=1` 会失败 | `tests/filter.rs:73`;实测失败 |
| SH-16 | P2 | 文档过期:`render-parity.md` §5.3 仍说“shader 源/字节码落点未定位”;ledger 有旧公式矛盾段落 | `render-parity.md:241`、`title-motion-ledger.md` |

## 4.1 本次深审补出的自身遗漏(第一版报告没查到)

- 第一版只扫了 `target/tmp/j2-shaders/` 的 8 个文件,没有扫整个 exe;
  实际至少 13 个候选、跨 5 种 shader 模型;
- 没有读 `supported_blend`,误以为 gfx 有状态表后端就能用;
- 没有检查 `Vertex` 结构,漏掉 DIFFUSE 缺失;
- 没有检查采样器参数,漏掉 MIP/anisotropy 静默忽略;
- 没有实际跑测试,漏掉 `probe_order_matches_real_eris_yscfg` 在
  “只设 ROOT”环境下的失败(gate 不一致,不是渲染 bug);
- 没有核对 J3 的“全绿”声明与当前工作区状态(重跑后 1 个样本 gate 失败)。

## 5. 建议的修复路线

1. **P0 证据**:把 8 个 PS blob 全部正确反汇编(修正 ps1/3/5/6/7 的
   dumpbin/字节码解析),逐个记录参数、常量寄存器、采样器;用 REA 找
   `CreatePixelShader`/`SetPixelShader` 调用点,建立
   “MODE/功能 → shader blob → 调用条件”映射表。
2. **P0 契约**:把 `dx_mode`(或“shader 路径开关”)从宿主注入到
   `CompositorFrame`/gfx,后端按它选择固定管线或 shader 路径;
   不假设所有工程都是 `_DXMODE=0`。
3. **P1 实现**:把引擎 shader 逐个移植为**独立 WGSL 文件**
   (`src/shaders/<name>.wgsl`)+ registry(引擎 blob ↔ WGSL ↔ 模式),
   先做 ps4 转场(HLSL 路径),再做 11 个模式;
   每个 shader 附官方微场景对拍。
4. **P1 固定管线**:对 MODE 3/5/6/7/9 用微场景逐一对拍状态表;
   不支持的模式显式跳过并计数,不再“状态有、后端无”。
5. **P2 通用性**:去掉 `image_probe_order` 缺省兜底(A5:缺表拒绝或记录);
   把转场覆盖度公式集中到一处并生成 WGSL 常量(或共享规范),
   避免 Rust/WGSL 双写漂移。
6. **P2 文档**:修正 `title-motion-ledger.md` 的自相矛盾;新增
   `docs/shader-parity.md` 作为引擎 shader ↔ WGSL 的映射与证据台账。
7. **P3 性能**:shader 变体按需创建、缓存;保持 J3 的 0 逐帧重传基线。

## 6. 验收标准(修复任务)

- 8 个 PS blob 全部有反汇编/参数/调用条件记录,映射表完整;
- `dx_mode` 可从宿主注入并在 gfx/后端生效,两种路径都有微场景对拍;
- 11 个 shader 模式中可取证的全部实现;无证据的显式 `Unimplemented`
  并计入报告;
- 每个 shader 有独立 `.wgsl` 文件 + 官方微场景像素对照(阈值先声明);
- soft/wgpu/browser 三端同帧一致;MODE 3/5/6/7/9 状态对拍通过;
- `image_probe_order` 缺表行为符合 A5;
- 文档无矛盾;PROGRESS 追加成果条目。
