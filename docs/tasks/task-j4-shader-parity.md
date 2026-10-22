# 任务书 J4 · 引擎着色器 1:1:提取、映射、移植与双路径

> 仓库:`<repo>`。先读 `AGENTS.md`、本文件、`docs/wgpu-shader-parity-review.md`
> (审查报告)、`docs/render-parity.md` §5、`docs/title-motion-ledger.md`
> EVID-T-08/09。
>
> 目标:把引擎的 **PS_2_0 着色器集合**完整提取、映射到 `MODE`/功能,
> 在 wgpu 中用**独立 WGSL 文件**逐一等价实现;并让引擎的
> `_DXMODE` 固定管线/着色器双路径可被宿主选择。
>
> 文件占用:
> - 独占 `crates/yu-gfx`、`crates/yu-backend-wgpu`、`crates/yu-backend-soft`;
> - `crates/yu-rt` 只允许**加法**(把 `BootConfig.dx_mode` 透传到
>   `CompositorFrame`/gfx 所需的最小改动);不改其它 rt 语义;
> - 不改 `yu-cli`/`yu-host-wasm`/`www`;宿主开关以交接方式提出;
> - 零 git 写操作;PROGRESS 只追加“成果 J4”。

## 0. 现状(来自审查报告 `docs/wgpu-shader-parity-review.md`)

- wgpu 只有 3 个内联 WGSL:quad / transition / paint;没有独立文件与注册表。
- 引擎已提取 **8 个 PS_2_0 blob**(`target/tmp/j2-shaders/ps0..ps7`);
  但对整个 exe 做 D3D9 version-token 扫描,至少还有 **13 个真实候选**
  (含 `ps_1_1 / ps_1_4 / ps_3_0 / vs_1_1`),8 个只是 ps_2_0 子集;
  仅 ps0/ps2/ps4 反汇编完整,ps1/3/5 错乱,ps6/7 为空;
  对应 11 个 shader 模式(4/8/10..18)与 CGACT 图像处理(MONO/BLUR 等)的
  映射未建立。
- 引擎双路径由 `_DXMODE` 选择;`dx_mode` 只在 `BootConfig`,
  `CompositorFrame`/gfx/后端没有该字段,后端永远走固定管线近似。
- 转场:固定管线模型是对拍通过的近似(TA≠128 有残差,DIV-GFX-045);
  引擎 HLSL ps4 公式(tex2.r)未移植。
- 固定管线 MODE 3/5/6/7/9:状态已取证、后端未支持;5 个模式被跳过。

## 0.1 审查未覆盖:必须补查/确认(来自审查报告 §4.1)

上一轮只做了目录级审阅,下列内容**必须在本任务里逐条查证**,不得直接沿用:

| # | 必查项 | 要产出什么 |
|---|---|---|
| 1 | 整个 `yu-ris.exe` 的 shader 候选(至少 13 个,跨 ps_1_1/ps_1_4/ps_2_0/ps_3_0/vs_1_1) | 每个候选的偏移/大小/模型;真假判定(必须走到 END token);哪些是已提取的 8 个 |
| 2 | ps1/3/5/6/7 反汇编失败/为空 | 确认 blob 是否有效;修正解析器或给出“无法反汇编”的证据 |
| 3 | `CreatePixelShader`/`SetPixelShader`/`ID3DXEffect` 调用点 | REA 地址 + 调用条件;为什么 `CreatePixelShader` 字符串不存在(导入/虚表) |
| 4 | `ps_1_4` blobs 与 `CGACT` 的 MONO/BLUR2..6 等图像动作 | 对应关系表;决定是否纳入本轮实现 |
| 5 | `vs_1_1` 顶点着色器用途 | 是否用于 E-mote/变形/特殊绘制;是否影响 2D title 路径 |
| 6 | 后端 `supported_blend` 只接受 0/1/2 | 确认 3/5/6/7/9 是否要支持;`fixed_state()` 与后端能力差异清单 |
| 7 | `Vertex` 只有 position/uv/alpha | 确认 MODE 2/3/5/6/7/9 是否需要非白 DIFFUSE;需要就补顶点属性 |
| 8 | 采样器 LOD0/anisotropy=1 | 引擎设置是否有 MIP/aniso;无证据则显式跳过并记录 |
| 9 | bind group 每条 draw/paint 新建 | 复测性能;能缓存则缓存,不改语义 |
| 10 | J3 的 600 帧/浏览器/性能数字 | 独立复跑 `frame_walk`/`wgpu_frame_walk`/浏览器锚点,确认或推翻 |
| 11 | `filter.rs` 只认 `YSBIN`,README 用 `ROOT/YPF` | 统一样本 gate;只设 ROOT 时 `REQUIRE_SAMPLES=1` 不得失败 |
| 12 | 文档过期/矛盾(`render-parity` §5.3、ledger 旧公式) | 修正并留勘误 |
| 13 | `image_probe_order` 缺省兜底 | 按 A5 决定:拒绝启动 or 显式记录默认值 |
| 14 | `_DXMODE≠0` 路径是否在目标范围 | 向主人确认;在范围内才实现 shader 路径 |
| 15 | 组(layer set)上的 TID 转场 | 微场景确认引擎是否支持;支持就实现,不支持就显式跳过+计数 |

**注意**:第 10、14、15 项涉及“是否在范围内”的判断,拿不准时写进报告
等主人确认,不要自行缩小范围。

## 0.2 硬性纪律

1. 每个 shader/模式必须先有台账行:引擎 blob 地址 + 反汇编关键指令 +
   参数/采样器 + 调用条件(REA `CreatePixelShader`/`SetPixelShader`)+
   官方微场景/截图;缺证据不实现,但**必须去取证**,不得以“未取证”停机。
2. 证据产物(`.cso`/`.asm`/截图)只放 `target/tmp` 或仓库外,不入库;
   入库的只有**语义描述与 WGSL**。
3. 核心 `src/` 禁止出现 ERIS/语料标识(CI grep 守卫);实现只能依赖
   `CompositorFrame`、工程数据与引擎语义。
4. 不得以“看起来一样”验收:每个 shader 要有官方微场景的逐像素数字。
5. 被跟踪文件一律占位符。

## 2. 阶段

### P0 · 完整 shader 清单与映射(先做)

- **先扫整个 `yu-ris.exe`**,不要只信已提取的 8 个:按 D3D9 version token
  (`ps_1_1/ps_1_4/ps_2_0/ps_3_0/vs_1_1`…)枚举候选,过滤误报(必须能走到
  END token);记录每个 blob 的偏移/大小/模型;
- 修正 ps1/3/5/6/7 的反汇编(检查偏移/字节码解析;必要时用
  `fxc /dumpbin`、D3D9 反汇编器或手动解码);
- 对每个 blob 记录:地址、长度、参数(`YS_*` uniform)、采样器、常量寄存器、
  关键指令;推测作用必须有依据(参数名/使用点),不允许凭名字猜;
- 用 REA 找所有 `CreatePixelShader`/`SetPixelShader` 调用点,建立
  “MODE/功能 → blob → 调用条件(如 `_DXMODE`、`TA`、按钮状态)”映射;
  同时判断 `vs_1_1` 是否用于特殊绘制(E-mote/变形),是否影响 2D 路径;
- **图像处理 shader 映射**:`CGACT` 的 MONO/BLUR2..6 等动作是否对应
  `ps_1_4`/`ps0`/`ps2`;若是,把对应关系写入台账,供 DIV-GFX-013 收尾;
- 产出 `docs/shader-parity.md`(唯一台账):
  表列 `blob | 地址 | 模型 | 参数/采样器 | 反汇编摘要 | 对应 MODE/功能 | 调用条件 | 状态`;
- 明确 blob 与 11 个 shader 模式、图像处理动作的关系(复用/参数化/缺 blob)。

**门禁 G0**:全部候选 blob 有结论(含“无法反汇编”的证据),
映射表可读;不允许“只做 8 个”。

### P1 · `dx_mode` 双路径

- 把宿主注入的 `dx_mode` 送入 `CompositorFrame`(新增字段)或 gfx 配置;
- 后端按 `dx_mode` 选择:
  - `0` = 固定管线(现状,保留);
  - 非 0 = 引擎 shader 路径(ps4 等);
- 对 title/boot 路径做 A/B:`_DXMODE=0` 与 `≠0` 各一套微场景,
  记录两条路径的像素差;
- 契约改动先写设计小节等确认。

**门禁 G1**:两条路径都能被宿主选择,报告里能看到当前走哪条。

### P2 · shader 移植(独立 WGSL 文件 + registry)

- 新建 `crates/yu-backend-wgpu/src/shaders/`:
  - 每个引擎 blob 一个 `.wgsl`(如 `transition_hlsl.wgsl`);
  - `mod.rs` 提供 registry:`(MODE/功能, dx_mode) → pipeline`。
- 先移植 **ps4 转场(HLSL 路径)**:按反汇编逐指令等价
  (`tex1.a * saturate((FadeVal - 0.5*tex2.r)*FadeVal2)*AlphaVal`);
  与固定管线路径分开,不互相替代;
- 再按 P0 映射移植其余 shader,覆盖 MODE 4/8/10..18 中可取证的部分;
- 无证据/无法反汇编的模式保持 `Unimplemented` + 报告计数;
- 每个 shader 附官方微场景与 soft/wgpu 对拍(阈值先声明)。

**门禁 G2**:每个已实现 shader 的微场景像素差在声明阈值内;
soft/wgpu/browser 同帧一致。

### P3 · 固定管线 3/5/6/7/9 与后端能力对拍

- 两端 `supported_blend` 目前只接受 0/1/2;先让 3/5/6/7/9 真正可执行
  (状态表已存在于 `fixed_state()`),再用微场景逐一验证;
- **DIFFUSE 顶点色**:`Vertex` 只有 position/uv/alpha;MODE 2/3/5/6/7/9 的
  固定状态使用 `Diffuse`。要么补顶点色属性,要么先证明这些模式在目标语料
  不使用非白 DIFFUSE;缺证据保持跳过;
- **采样器**:当前 LOD0 / anisotropy=1;MIP 与各向异性被静默忽略。
  按引擎设置取证后显式支持或显式跳过(DIV-GFX-009/审查 SH-13);
- **bind group 缓存**:transition/paint 每条 draw 新建 bind group;
  在保持正确性的前提下缓存(性能,非语义);
- 无法对拍的模式保持跳过并计数,不近似;更新 `BlendConfidence` 需有像素证据。

### P4 · 收尾

- 通用性修复:
  - `image_probe_order` 缺省兜底改为“拒绝/显式记录”(A5);
  - 转场覆盖度公式单一来源(Rust 与 WGSL 不再各写一份);
  - 修正 `title-motion-ledger.md` 的旧公式矛盾段落;
- 更新 `docs/render-parity.md` §5、`docs/gfx-unimplemented-todo.md`、
  `docs/todo-inventory.md`;
- PROGRESS“成果 J4”:映射表、双路径 A/B、shader 微场景数字、
  仍 Unimplemented 的 blob/模式与证据状态;
- `cargo test -p yu-gfx -p yu-backend-soft -p yu-backend-wgpu` 全绿;
  clippy 0 新增;`src/` 无 `println!/eprintln!/dbg!`。

## 3. 验收

- `docs/shader-parity.md` 完整:全部候选 blob(不止 8 个) × 参数 × 映射 ×
  调用条件;含 ps_1_4/ps_1_1/ps_3_0/vs_1_1 的结论;
- `dx_mode` 可选,gfx/后端两路径可运行,报告标注路径;
- 已取证 shader 全部有独立 WGSL + 官方微场景对拍;
- MODE 3/5/6/7/9 的状态与像素对拍结论明确(支持/跳过 + 证据);
- DIFFUSE、MIP/anisotropy 的缺口有明确处置(实现或显式跳过 + DIV);
- 样本 gate 统一:`ROOT/YPF` 与 `YSBIN` 两种入口不再互相矛盾
  (`filter.rs` 等只认 YSBIN 的测试要么支持 ROOT,要么文档写明);
- 无 ERIS 专用分支;证据产物不入库;
- PROGRESS 成果 J4 含原始命令输出。

## 4. 环境

```bash
export TEMP="<side-repo>/workbench/tmp"; export TMP="$TEMP"
export YURIS_REQUIRE_SAMPLES=1
export YURIS_SAMPLE_ERIS_ROOT="<side-repo>/workbench/project-eris"
export YURIS_SAMPLE_ERIS_YPF="<side-repo>/workbench/project-eris/data.ypf"
```

引擎 shader 证据:`target/tmp/j2-shaders/`(已有 8 blob);
微场景编译/采集流程见 `docs/eris-corpus.md` 与
`docs/title-motion-ledger.md` §0。

## 5. 停止/上报条件

- blob 无法反汇编且无法用其它工具恢复 ⇒ 记录证据,标记 blocked;
- 需要改宿主(`yu-cli`/`yu-host-wasm`)暴露 `dx_mode` ⇒ 写交接,不抢改;
- 契约改动(CompositorFrame 新字段)⇒ 先设计小节等确认;
- 工具/样本硬阻塞 ⇒ 挂账 + 已尝试路径。
