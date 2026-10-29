# yu-backend-soft 独立审查报告

> **⚠ 历史快照(HEAD `b2a5737`,早于 J2/J3/J4)。2026-10-22 复核:**
> 本报告的 P1/P2 绝大多数已关闭或改写,阅读时**不要**把下面的结论当现状。
>
> | 本报告条目 | 2026-10-22 状态 |
> |---|---|
> | P1-1 alpha `As` → `As²` | **已修**(EVID-J-01,`blend_pixel` 单测钉死 `[128,128,128,191]`) |
> | P1-2 MODE 2 DIFFUSE RGB | **已修**(J4:顶点色 = A 灰度,`T·A` 归约) |
> | P2-2 零尺寸 Offscreen 不计 `ops_total` | **已修**(`group.size.zero` 路径先 `ops_total += 1`) |
> | P2-3 Group transition 静默忽略 | **已修**(J2:`group_transition_op`) |
> | P2-4 Transition 一律跳过 | **已修**(J2:两端执行) |
> | P2-5 `first_frame` HostPaths / report | **部分**:示例仍用 `BootConfig::default()`(见 `audit-2026-10-22.md` §C3) |
> | P2-8 未消费 `fixed_state()` | **部分**:状态表与后端公式仍各写一份,但有 `blend_table` 逐字段测试 |
> | P2-9 `image_probe_order` 只在构造时生效 | **已修**(每帧从 `frame.image_probe_order` 取) |
> | “只是烟雾测试”的整体判断 | **已过时**:title 稳定帧 600 帧 soft/wgpu 0 差异、浏览器同哈希 |
>
> 现状入口:`docs/audit-2026-10-22.md`、`docs/todo-inventory.md`。

- 审查对象:`crates/yu-backend-soft`(HEAD `b2a5737`;`src/lib.rs`、`src/render.rs`、`src/texture.rs`、`src/png.rs`、`tests/render.rs`、`examples/first_frame.rs`)
- 消费契约:`yu-gfx` 的 `CompositorFrame` / `Op` / `Layer` / `Pass`,以及 `yu-vfs` 的 `DataSource` / `Mount` / `LookupKey`
- 性质:**只读审查**。未修改被审源码,未执行 git 写操作,未启动/注入官方引擎,未改动 `<repo-old>`。本报告是本次审查唯一新增的仓库文本文件;运行示例会重写 `target/tmp` 下的生成物。
- 环境:`YURIS_REQUIRE_SAMPLES=1`;`YURIS_SAMPLE_ERIS_YSBIN` 指向 `<side-repo>/workbench/<project-eris>/save/data/ysbin`(143 个文件);`YURIS_SAMPLE_ERIS_YPF` 指向 `<side-repo>/workbench/<project-eris>/data.ypf`。
- 证据等级沿用仓库定义:**Confirmed** = 手册/真实产物/引擎反编译一致;**Likely** = 已取证但缺逐像素对拍;**Unverified/Uncertain** = 关键事实未取证;**Unimplemented** = 未实现,必须跳过。

---

## 0. 结论摘要(TLDR)

**逻辑框架是连贯的,测试与确定性也成立;但它目前只是一个“可审计的确定性烟雾测试”,不是可做 1:1 像素对拍的首帧实现。**

具体结论:

1. **用户关于“现在对拍还不到时候”的判断正确。**
   - `first_frame` 实际选中的是 **逻辑帧 3 的 display 状态**:2 个 tick 后,首个“结构上有可绘制屏幕 op”的帧;不是官方首帧,也不是 boot 完成锚点。
   - 官方 **事件级 L1 未达成**(分派探针 0 命中);官方**屏幕级**采集的是启动完成后的标题菜单,与内核 `frame 3` 不是同一逻辑锚点,比较已按用户裁决撤回(`docs/divergences/DIV-H-001`,status=withdrawn)。
   - 后端自己报告 `filter.source=placeholder`;清屏、颜色空间、组 RT、MODE 0 都是 Likely/Unverified。此时做像素对比,差异无法归因。
   - 实跑 `first_frame` 得到的是黑底 + 蓝色系统文本框(README 也写“系统文本框”),不是标题菜单。

2. **两个 P1 级语义问题(建议在像素工作前先修):**
   - **Alpha 通道混合公式与 D3D9 同因子语义不符(确定错误)。** MODE 0/1 的 RGB 源/混合因子与各自 stage 状态一致(MODE 0 仍因 stage1 NULL 纹理只能算 Likely);MODE 2 的 RGB 还额外依赖“diffuse=白”(见下一条)。但 alpha 通道把源项写成了 `As`,而 `D3DBLEND_SRCALPHA` 的因子是 `(As,As,As,As)`,在未启用 `SEPARATEALPHABLENDENABLE` 时 alpha 也使用同一组因子。正确源项是 `As²`,不是 `As`。现有单测把错误值钉死了。
   - **MODE 2 被执行,但它的 stage0 颜色是 `MOD(DIFFUSE, TEXTURE)`;DIFFUSE RGB 未取证。** 后端按 `diffuse=白` 直接用纹理 RGB,违反“无证据 → Unimplemented”的铁律。账本 B5 与 DIV-GFX-019 都明确写着 DIFFUSE RGB 未建模,且账本还自相矛盾地说“这些模式已 Unimplemented”(MODE 2 实际是可执行)。

3. **一组 P2 级契约/证据缺口:**
   - 组离屏 RT 的 **alpha 存储/格式**未取证;后端按 RGBA8 直通 alpha 合成。若引擎 RT 是 X8R8G8B8,组的 alpha 根本不存;若是 A8R8G8B8,则 P1 alpha 公式生效。两种情况后端都没有证据。
   - **零尺寸 Offscreen pass** 的跳过路径没有累加 `ops_total`,会破坏 `ops_total == drawn + skipped` 的 debug 断言/报告不变量。
   - **Group 的 `transition` 被静默忽略**:`Op::Group` 不经过 `op_for_layer` 的过渡分支,后端 Group 分支也不看 `layer.transition`,于是带 TID 的组会被当作普通组整块绘制,不跳过、不计数。
   - **所有 `Op::Transition` 一律跳过**,即使 gfx 给出 `to=Some` 且状态为 Executable;与 `render-parity.md` §12.5“`to=None` 时才跳过”的措辞不一致。
   - `first_frame` 示例**丢弃了 `tick_at` 的 report**(`main_finished`/errors),且 BootConfig **没有注入 HostPaths**;`es_First.yst:28` 要读 `$_SAVE_PATH`,内核会返回 Unimplemented。boot 环境不等价。
   - `first_frame` 的 **YPF 默认路径**在 `save/data/ysbin` 布局下指向 `save/data.ypf`,真实包在 `<project-eris>/data.ypf`;README 却把 YPF 标为“可选”,省略时纹理会静默缺失。
   - 过滤只实现 Point/Linear(bilinear),**MIP 完全忽略,Anisotropic 静默等同双线性**,混合缩放的 mag/min 选择是启发式,均未取证。
   - 后端**没有消费** `BlendMode::fixed_state()` / `json::blend_table_json()`,而是手写三个公式;与 §12.3“用 fixed 状态表”的契约不符,也让状态表变化无法自动反映。

4. **正确/已验证的部分:**
   - `cargo test -p yu-backend-soft`:10 lib + 5 integration + 1 doc = **16 passed,0 failed**。
   - `cargo clippy -p yu-backend-soft --all-targets`:本 crate **0 告警**(输出的 5 条 warning 均来自依赖 `yu-rt`)。
   - 实跑 `first_frame`:frame 3、800×600、272 层、15 pass、44 op(15 drawn / 29 skipped)、13 张纹理解码、0 缺失资产、`filter.source=placeholder`、`color_space=gamma_encoded_srgb8`、`deterministic=true`。
   - pass 顺序、Screen 用 `frame.clear`、组 `A/MODE` 只在合成时作用一次、`SolidBlack1x1 + SX/SY` 铺满、COPY/图像加工跳过并计数、缺失资产诊断、文本只计数——这些与 §12 的当前实现一致。
   - PNG 参数固定(`Fast` + `NoFilter`),同帧两次编码逐字节一致。

---

## 1. 审查方法与证据来源

| 类别 | 本次使用的内容 |
|---|---|
| 契约 | `docs/render-parity.md` §12、`crates/yu-gfx/src/{frame,layer,op,pass,state,texture...}.rs`、`first_frame.display.json` |
| 引擎语义 | 本地 Ghidra 反编译树 `FUN_00414ba4`(逐层绘制/混合状态)、`FUN_00462e40`(设备状态初始化)、`FUN_00413dc8`(遍历/组 RT)、`FUN_0040c9b4`(纹理探测);真实 ERIS 脚本 `es_First.yst` |
| D3D9 语义 | Microsoft `D3DRENDERSTATETYPE`、`D3DBLEND` 文档:状态编号、`SEPARATEALPHABLENDENABLE` 默认行为、`SRCALPHA` 因子 |
| 内部行为 | `cargo test`、`cargo clippy`、`cargo run --example first_frame`、`cargo run -p yu-rt --example run_eris` |
| 对拍状态 | `tools/official_trace/README.md`、`docs/divergences/DIV-H-001-official-first-screen.md`、`PROGRESS.md` 的 M3-SOFT |

**没有做的事**:没有运行官方引擎,没有注入断点,没有把官方标题画面拿来和内核 `frame 3` 对拍,没有修改任何源码/测试/文档(除本报告)。

**工作树说明**:审查期间工作树存在并发改动(`Cargo.lock` 被修改、未跟踪的 `crates/yu-backend-wgpu/`)。它们不属于本报告范围;本报告只审 HEAD 中的 `yu-backend-soft` 源码。

---

## 2. 实际复现结果

### 2.1 测试与 clippy

```text
$ cargo test -p yu-backend-soft
running 10 tests ... test result: ok. 10 passed
running 5 tests ...  test result: ok. 5 passed
Doc-tests ...        test result: ok. 1 passed
```

```text
$ cargo clippy -p yu-backend-soft --all-targets
warning: `yu-rt` (lib) generated 5 warnings
Finished `dev` profile ...
```

`yu-backend-soft` 自身 0 告警;`src` 内无 `println!/eprintln!/dbg!`,`lib.rs` 有 `#![forbid(unsafe_code)]`。

### 2.2 `first_frame` 实跑(真实 ERIS 样本)

```text
event path: GfxHost sync (Vm::take_events drained for stats only)
entry: SYSTEM_START; record_events=false
frames ticked: 2 (selected tick 1)
stop rule: first frame with a drawable screen op (limit 120);
           frame_has_drawable_output is structural, no pixel work
frame: 3 viewport: 800x600
layers: 272 (visible 34), passes: 15, offscreen passes: 2
ops: 44 total, 15 drawn, 29 skipped
  skip copy       x1    gfx.copy.unsupported
  skip image      x11   gfx.imageop.unsupported
  skip layer      x16   gfx.copy.unsupported
  skip layer      x1    gfx.imageop.unsupported
missing assets: 0
skipped text: 0 runs (no glyph rasterizer)
filter: mag=point min=point mip=none source=placeholder
color space: gamma_encoded_srgb8
assets: probe=["png","bmp","jpg","gif","psb"] loose_files=1535 ypf=true
textures: decoded=13 cache_entries=13
events: drained=578426 dropped=0
png: target/tmp/first_frame.png (785127 bytes) deterministic=true
```

产物:

```text
sha256(first_frame.png)         = 59f6f5b7ba72a9485a28641369429d9f8a815c17685fbf8cf2fc3a9732e4bfac
sha256(first_frame.display.json)= 522b6bfb646abd2c68be796612e9fbba43a44faae60485ea60c2f8521e828212
```

PNG 为 800×600 RGBA;像素统计:487 种颜色,绝大多数为黑,蓝色文本框区域约 79k 像素;alpha 全为 255(原因见 §3.1:屏幕清屏不透明 + 后端 alpha 公式)。

### 2.3 `run_eris` 的 boot 事实

```text
$ cargo run -q -p yu-rt --example run_eris -- <side-repo>/workbench/<project-eris>/save 3 0
frame 0: cmds=1290326 errors=0 main_finished=false
frame 1: cmds=274466  errors=0 main_finished=false
frame 2: cmds=3315    errors=0 main_finished=false
```

`first_frame` 在 tick 0/1 后选中逻辑帧 3;`run_eris` 的 tick 0/1/2 对应逻辑帧 1/2/3。也就是说:

- 内核确实在跑启动链(无 VM error);
- 但 `main_finished=false`,帧 3 的命令量刚从 27 万降到 3 千,**不能据此声称 boot 已完成或已到达官方首屏**;
- `first_frame` 示例完全不看 `_report.main_finished` / `_report.errors`(见 §5.6)。

---

## 3. P1:确定的语义错误与铁律冲突

### 3.1 P1-1 · Alpha 通道混合公式与 D3D9 同因子语义不符

**结论:这是本次审查中唯一可以仅凭“权威 D3D9 文档 + 引擎反编译 + 代码”就判定为确定错误的地方。**

引擎侧证据:

- `FUN_00414ba4`(0.495 `yu-ris.exe`)逐层设置混合状态:
  - MODE 0:`SetRenderState(0x13, 5)` = `D3DRS_SRCBLEND=SRCALPHA`,`(0x14, 6)` = `DESTBLEND=INVSRCALPHA`,`(0xab, 1)` = `BLENDOP=ADD`(反编译行 445/450/455);
  - MODE 1:`(0x13,5)/(0x14,2)/(0xab,1)`(行 492/497/502);
  - MODE 2:`(0x13,5)/(0x14,2)/(0xab,3)` = `REVSUBTRACT`(行 543/548/553)。
- `FUN_00462e40` 在设备状态初始化里设置 `SetRenderState(0x1b, 1)` = `D3DRS_ALPHABLENDENABLE`,错误串为 `"D3DRS_ALPHABLENDENABLE Error."`(反编译行 160-166)。
- 本地反编译树中 **没有找到** `D3DRS_SEPARATEALPHABLENDENABLE`(0xCE)的写入;Microsoft 文档说明该状态默认 FALSE,且“when set to FALSE, the render-target blending factors and operations applied to alpha are forced to be the same as those defined for color”。

D3D9 定义(`D3DBLEND`):

- `D3DBLEND_SRCALPHA`:blend factor = `(As, As, As, As)`。
- `D3DBLEND_INVSRCALPHA`:blend factor = `(1-As, 1-As, 1-As, 1-As)`。

因此,未启用 separate alpha 时,alpha 通道的正确结果是:

| MODE | RGB(与后端一致的部分) | 正确的 alpha | 后端实际 alpha |
|---|---|---|---|
| 0 | `Cs·As + Cd·(1-As)` | `As·As + Ad·(1-As)` | `As + Ad·(1-As)` |
| 1 | `Cs·As + Cd` | `As·As + Ad` | `As + Ad` |
| 2 | `Cd - Cs·As` | `Ad - As·As` | `Ad - As` |

8-bit 例子(`As=128/255≈0.502`):

| 路径 | dst alpha | 后端输出 | D3D9 同因子输出 |
|---|---:|---:|---:|
| MODE 0 | 255 | 255 | 191 |
| MODE 0 | 0 | 128 | 64 |
| MODE 2 | 255 | 127 | 191 |

代码位置:

- `crates/yu-backend-soft/src/render.rs:735-752`:
  - `Normal` 的 alpha = `(a + (dst[3]*inv + 127)/255).min(255)`(第 739 行);
  - `Add` 的 alpha = `(a + dst[3]).min(255)`(第 745 行);
  - `Subtract` 的 alpha = `dst[3].saturating_sub(src_alpha)`(第 751 行)。
- 注释与 README 也写错了状态编号:`render.rs:729` 与 `README.md:52` 把 `0xAB` 说成 `D3DRS_ALPHABLENDENABLE`;实际 `0xAB=171` 是 `D3DRS_BLENDOP`,`0x1B=27` 才是 `ALPHABLENDENABLE`。`README.md:53-54` 由“同一组因子”推出 `MODE 2 alpha = dst_a − a`,恰好与正确公式相反。
- 单测把错误值钉死:`render.rs:762-782` 的 `Normal` 断言 alpha=255(应为 191),`Subtract` 断言 127(应为 191)。

影响:

- **屏幕直绘**:RGB 仍按 `As` 混合,alpha 通道错误;若只比较 RGB,直绘层可能看不出。
- **组离屏**:组表面以透明黑起手,alpha 会累积。若组内有半透明纹理,组表面的 alpha 错误;随后组以该 alpha 合成到屏幕,RGB 也随之错误。
  - 例:源 alpha 0.5 画进透明组,再合成到不透明黑屏。后端屏幕 RGB 系数 ≈ `As²=0.25`;D3D9(A8 RT)是 `As³=0.125`;若 RT 是 X8(见 §4.1),则是另一种结果。
- **PNG alpha**:即使屏幕清屏不透明,半透明内容区域的 alpha 在 D3D9 下会小于 255,后端恒为 255。
- **MODE 2 的 alpha** 尤其明显:后端 `255-128=127`,D3D9 `255-64=191`。

修复建议:

1. 把 alpha 源项改成 `mul255(src_alpha, src_alpha)`(即 `As²`),三个模式分别:
   - `Normal`: `mul255(a,a) + Ad*(1-As)`;
   - `Add`: `mul255(a,a) + Ad`;
   - `Subtract`: `Ad - mul255(a,a)`。
2. 单测改为:
   - MODE 0、dst alpha 255、src alpha 128 → 191;
   - MODE 0、dst alpha 0、src alpha 128 → 64;
   - MODE 2、dst alpha 255、src alpha 128 → 191。
3. 加一个组离屏半透明集成测试,断言组表面与最终屏幕的 RGB/alpha。
4. 修正 README/注释里的 `0xAB` 状态身份。

> 注意:如果后续取证发现引擎的组 RT 是 X8R8G8B8(alpha 不存),那么组合成需要走另一套语义,而不是简单套用上面的 A8 公式。见 §4.1。

---

### 3.2 P1-2 · MODE 2 执行时依赖未取证的 DIFFUSE RGB

**结论:MODE 2 的 RGB 公式建立在“顶点 diffuse = 白”的未取证假设上,违反仓库“无证据 → Unimplemented”的铁律。**

引擎侧证据:

- `FUN_00414ba4` MODE 2 分支(反编译行 510-538):
  - `COLOROP = MODULATE`(值 4);
  - `COLORARG1 = DIFFUSE`(值 0);
  - `COLORARG2 = TEXTURE`(值 2);
  - `ALPHAOP = MODULATE`、`ALPHAARG1 = DIFFUSE`、`ALPHAARG2 = TEXTURE`。
- `crates/yu-gfx/tests/blend_table.rs:33-47` 也钉死了 `MODE 2 stage0 = MOD(DIFFUSE, TEXTURE)`。

契约/账本自己承认没取证:

- `docs/gfx-verification-ledger.md:71` B5:**顶点 DIFFUSE RGB 未建模**,`FUN_0041a308` 的 diffuse 来源在反编译中未能可靠还原;结论 **Uncertain**。
- `docs/gfx-unimplemented-todo.md:32` DIV-GFX-019:**顶点 DIFFUSE RGB**,状态 `open`,当前行为 `未建模`。
- 同一行 B5 还写“模式 2/3/5/6/7/9 才在 color stage 使用 DIFFUSE,而这些模式已 Unimplemented”——但 MODE 2 在 `render-parity.md:41` 是 **Confirmed**,在后端里是 **可执行**。这是账本与实现的直接矛盾。
- `crates/yu-backend-soft/README.md:57-58` 明确写“MODE 2 的 diffuse:契约可见公式按 `dst − src·a`(diffuse=白);引擎 stage0 是 `MOD(D,T)`……这里按白处理。”

后端代码:

- `render.rs:747-752` 的 `Subtract` 直接使用 `src`(纹理 RGB),没有乘 diffuse RGB。
- 只有当 diffuse RGB = `(255,255,255)` 时,`MOD(D,T)` 才退化为 `T`;这个值没有证据。

影响:

- 只要 MODE 2 图层可执行且 diffuse RGB 不是白色,RGB 就错。
- 首帧唯一的 MODE 2 图层是 `:/W0/SPLDST2`,其 `A=0`、`E=0`(`render-parity.md` §2),所以本帧不可见;但后续帧/商业语料会用到。
- 这不仅是“像素可能差一点”,而是**代码明确执行了一个依赖 Unimplemented 输入的模式**,与项目“禁止猜”的纪律冲突。

修复建议(二选一,不能两者都不做):

1. **先取证**:还原 `FUN_0041a308` 调用点的 diffuse 构造(顶点色是 `0xFFFFFFFF`、`A<<24|0xFFFFFF`,还是别的);如果确认 RGB=白,再保留 MODE 2 可执行,并把证据写进 DIV-GFX-019/账本。
2. **未取证前降级**:把 `BlendMode::Subtract` 的 `confidence()`/`LayerStatus` 改为 Unimplemented(或至少在 `apply_layer_params` 里标 `gfx.blend.unverified`),后端明确跳过 MODE 2,而不是按白 diffuse 执行。

同时修正账本 B5 的“这些模式已 Unimplemented”措辞,避免与 `render-parity` 的 Confirmed 表冲突。

---

## 4. P2:契约/证据缺口与实现缺陷

### 4.1 P2-1 · 组离屏 RT 的 alpha 存储/格式未取证

后端行为:

- `render.rs:162-168`:组表面 = `Surface::new(width, height, clear.rgba)`,即 **RGBA8 直通 alpha**;
- `render.rs:226-243`:组以 `layer.alpha/blend` 合成到父目标。

契约/证据:

- `docs/render-parity.md:372-374` 只说“清屏 = 该 pass 的 `clear`(透明黑)”,没有说 RT 的 `D3DFMT`。
- `docs/gfx-unimplemented-todo.md:23` DIV-GFX-008 只覆盖“组离屏尺寸/清屏”,**没有覆盖 RT 格式/alpha 存储**。
- 本地反编译可见 `+0x61c` 是组 RT、`+0x21c` 是纹理对象(`FUN_00413dc8` 的调试组分支),但 `FUN_00462e40`/`FUN_00413dc8` 里没有定位到 RT 创建时的 `D3DFMT`/`COLORWRITEENABLE` 参数。
- 若 RT 是 `A8R8G8B8`,则 §3.1 的 alpha 公式生效;若 RT 是 `X8R8G8B8`,alpha 根本不存,采样时通常视为 1.0,组的半透明行为完全不同。
- 若引擎对组 RT 关闭 alpha 写(`D3DRS_COLORWRITEENABLE` 去掉 alpha 位),又是第三种行为。

影响:组的 A/MODE 合成是首帧和所有 UI/CG 组都走的主路径。在 RT 格式未取证前,**不能声称组合成 1:1**。

建议:

1. 新增 DIV(建议 DIV-GFX-041 或并入 008):“组离屏 RT 的 D3DFMT/alpha 写掩码/采样 alpha 未取证”。
2. 在反编译中定位 `+0x61c` 的 `CreateRenderTarget`/`CreateTexture` 调用与 `D3DFMT` 实参;或在官方引擎上做受控实验(半透明图层进组,读回组纹理 alpha)。
3. 在证据关闭前,后端应把“组合成”标为 Likely,并在 `RenderReport` 里显式输出该假设(目前只有 `filter_source`/`color_space` 有 source 字段)。

---

### 4.2 P2-2 · 零尺寸 Offscreen pass 破坏 `ops_total` 不变量

代码:

```rust
// render.rs:154-160
PassTarget::Offscreen(id) => {
    let (width, height) = group_surface_size(frame, id);
    if width == 0 || height == 0 {
        for op in &pass.ops {
            st.skip_op(op, "group.size.zero");   // 只加 skipped_ops_total
        }
        continue;                                 // 没有 ops_total += 1
    }
    ...
```

而 `render.rs:184-188`:

```rust
debug_assert_eq!(
    st.report.ops_total,
    st.report.ops_drawn + st.report.skipped_ops_total as usize,
    "每个 op 必须恰好被绘制或跳过"
);
```

`skip_op`(`render.rs:413-417`)只增加 `skipped_ops_total`,不增加 `ops_total`。因此:

- debug 构建:组尺寸为 0 时,该断言会 panic;
- release 构建:报告里 `ops_total != drawn + skipped`,破坏的不变量正是 `render.rs:91-95` 文档承诺的“每个 op 恰好被绘制或跳过”。

触发方式:脚本给组 `SX=0` 或 `SY=0`(`apply_layer_params` 允许 u32 0),或 viewport 为 0。首帧没有触发,但代码显式写了这个分支,说明作者预期它可能发生。

建议:在 `skip_op` 之前 `st.report.ops_total += 1`,或统一让 `skip_op` 负责 `ops_total`;加一个零尺寸组的测试。

---

### 4.3 P2-3 · Group 的 transition 被静默忽略

链路:

- gfx 的组 op 创建:`state.rs:1762-1768` 直接 `Op::Group { layer: node.layer.clone(), offscreen }`。
- 过渡分支只在 `op_for_layer` 里(`state.rs:1831-1860`),而 `op_for_layer` 只被图像层使用(`emit_image_subtree`);组节点不会走它。
- `apply_layer_params` 对 `T/TA/TID` 只写 `layer.transition`(`state.rs:2245-2273`),**不**调用 `mark_param_unimplemented`;因此组层 `status` 仍是 Executable。
- 后端 `render.rs:213-243` 的 `Op::Group` 分支只检查 `visible`、`status`、`parts`、`blend`、`offscreen`,**从不看 `group.layer.transition`**,于是直接绘制整组。

后果:带 `CG[ID="G/" T=… TA=… TID=…]` 的组会被当作普通组整块绘制,不跳过、不计数、不报告。这是**静默忽略**,比“跳过并计数”更危险,也违背 `README.md:66` 的“过渡不近似”。

建议:

- 后端 Group 分支检查 `group.layer.transition`:若 `fade_value()`/`tid` 有效,要么实现双纹理过渡,要么 `skip_op(op, "transition.unsupported")`;
- `frame_has_drawable_output` 的 Group 预测同步处理;
- 加一个组过渡测试(即使当前预期为 skip,也必须出现在报告里)。

---

### 4.4 P2-4 · 所有 `Op::Transition` 一律跳过,与 §12.5 的措辞不一致

`render.rs:244-247`:

```rust
Op::Transition(_) => {
    let reason = status_reason(op).unwrap_or("transition.unsupported");
    st.skip_op(op, reason);
}
```

- `docs/render-parity.md:415` 写的是“Transition:`from`/`to` 两个纹理,`fade` 已按公式算好;`to=None` 时跳过。”这暗示 `to=Some` 且状态 Executable 时应执行。
- gfx 在 `to=Some` 且 `TA>=1` 时确实会给出 `Op::Transition`(`state.rs:1839-1859`),`Transition::fade_value()` 的公式也已按 `FUN_00414ba4` 905-909 实现。
- 后端无论 `to` 是否存在都跳过,并把 reason 设为 `transition.unsupported`。

这不是“像素错”,而是**能力缺口与契约不一致**:一个已标记 Executable 的 op 被后端跳过。报告是透明的(有 skip 计数),但验收上应把“过渡未实现”写成显式 DIV,而不是让 §12.5 看起来已经支持。

建议:

- 要么实现 `from/to/fade` 的双纹理过渡(需要补 `YS_FadeVal` 的推进者证据,DIV-GFX-006);
- 要么把 `Op::Transition` 加入后端已知未实现清单,并让 gfx/账本明确“本版全部过渡由后端跳过”;不要用“to=None 时才跳过”的表述。

---

### 4.5 P2-5 · `first_frame` 不是官方首帧,示例也不验证 boot 完成

这是用户判断的核心,仓库证据完全支持。

代码事实:

- `examples/first_frame.rs:49-54` 的 `BootConfig` 只设置 `entry_label/fps/record_events`,**没有注入 `paths`**(默认 `HostPaths::default()`,全部 None)。
- `first_frame.rs:88` `let _report = vm.tick_at(...)`:**丢弃** tick report(`main_finished`、`errors`、`commands_pumped`)。
- `first_frame.rs:93-99` 一旦 `frame_has_drawable_output` 为真就 break;该函数是**结构预测**(`render.rs:436-459`),不看资产是否存在、是否 boot 完成。
- 实跑:tick 1(逻辑帧 3)选中;`main_finished=false`(`main_finished` 对游戏主循环来说通常一直是 false,它本身不等于 boot 失败;这里的问题是示例**没有别的 boot 完成判据**,不能把 frame 3 当作 boot 完成);`es_First.yst:28` 读 `$_SAVE_PATH`,而 `sysvar.rs:221-244` 对未注入路径返回 `Unimplemented`;`first_frame` 没有注入路径,VM 的该诊断也被忽略。
- `es_First.yst:167-170` 有 `IF[@_DXMODE]` 分支;`first_frame` 用默认 `dx_mode=0`,官方 Windows 引擎的真实 `@_DXMODE` 未注入/未验证。

官方锚点状态:

- `tools/official_trace/README.md:11-13`:事件级 L1 **未达成**,分派探针 25s 0 命中;屏幕级只能证明“画面摘要”。
- `tools/official_trace/README.md:101-102`:**“不要用官方标题画面去对内核 `first_frame`”**,该比较已撤回。
- `docs/divergences/DIV-H-001...:10-11`:官方采集到的是启动完成后的标题菜单;内核 `first_frame.png` 是 `frame=3`、黑底+文本框。
- 实跑 PNG 内容:黑底 + 蓝色系统文本框,与 DIV-H-001 的描述一致。

结论:

- 当前 `first_frame` 是**启动链中第一个结构上可绘制的 display 状态**,不是官方首帧,也不是 boot 完成状态。
- 命名和验收文档中的“首帧”应加限定;至少在 README/PROGRESS 里写“启动期 display 状态(frame 3),非官方首帧”。
- 在事件级 L1 打通前,任何像素差异都只能归因到“锚点不同/环境不同”,不能归因到后端。

建议:

1. 示例改名或增加输出字段:`first_drawable_display` / `boot_display_snapshot`,并打印 `main_finished`、`errors.len()`、`events_dropped`。
2. 注入与 `run_eris` 相同的 HostPaths(`es_First.yst:28` 需要 `$_SAVE_PATH`),并从工程配置注入 `@_DXMODE`/`@_SCREEN_COLOR`(而不是吃默认值)。
3. 在报告/README 中明确:该 PNG 是 smoke test,不是 oracle;像素对拍等事件级 L1。

---

### 4.6 P2-6 · YPF 默认路径在 `save/data` 布局下错误

代码:

- `first_frame.rs:243-258` 的 `normalize_root`:若 `<root>/save/data/ysbin/ysc.ybn` 存在,返回 `<root>/save`。
- `first_frame.rs:260-266` 的 `ypf_path`:默认 `root.join("data.ypf")`。
- 因此 `YURIS_SAMPLE_ERIS_YSBIN=<project-eris>/save/data/ysbin` 时,root=`<project-eris>/save`,`ypf_path` 找的是 `<project-eris>/save/data.ypf`;真实包在 `<project-eris>/data.ypf`。
- README `crates/yu-backend-soft/README.md:14-16` 把 `YURIS_SAMPLE_ERIS_YPF` 标为“可选”。

后果:如果用户按 README 省略 YPF,`ypf_path` 返回 None,不报错;Mount 只有散盘 `save/data`,YPF 中的资产不会被挂载,`missing assets` 可能增加,PNG 可能残缺。实跑之所以 `ypf=true`,是因为任务环境显式设置了 `YURIS_SAMPLE_ERIS_YPF`。

建议:

- 依次尝试 `root/data.ypf`、`root.parent()/data.ypf`(或从 ysbin 路径向上找 `data.ypf`);
- 若没有 YPF 且散盘也不覆盖,显式打印/诊断“no YPF”;
- README 不要笼统写“可选”,应写“在 save/data 布局下必须设置 `YURIS_SAMPLE_ERIS_YPF`,或确认散盘包含全部资产”。

---

### 4.7 P2-7 · 过滤:MIP 未实现、Anisotropic 等同双线性、混合缩放选择无证据

代码:

- `render.rs:650-702` 的 `sample`:`Point` 最近邻;`Linear | Anisotropic` 都走同一段双线性;没有 mip 链。
- `render.rs:524-536` 的 `pick_filter`:只要目标任一维大于源,就选 `mag`,否则选 `min`。
- `RenderReport` 输出 `filter_mag/min/mip`(`render.rs:404-407`),但不对“mip 被忽略/Anisotropic 被降级”给出任何提示。

契约:

- `docs/render-parity.md:404-409`:MAG/MIN/MIP 映射到两个采样器;`source=placeholder` 时不能声称对齐;`engine_settings` 是已取证值。
- `docs/render-parity.md:42`:MIP=-1=NONE,0=POINT,1=LINEAR,2=ANISOTROPIC。

影响:

- 当前首帧 `filter.source=placeholder` 且大部分是直接 blit/组合成,过滤路径几乎不生效;但一旦宿主注入 `engine_settings` 且出现缩放/MIP,输出会与引擎不同。
- 混合缩放(如 2×1 → 4×2)时,D3D9 的 MAG/MIN 选择不是简单的“任一维放大就 mag”;后端的启发式未取证。

建议:

- 若暂不实现 mip/anisotropic,在 `RenderReport` 里增加 `filter_approx`/`filter_note`,或在 `mip != None`/`Anisotropic` 时显式标记 `Unimplemented`;
- 用受控缩放测试确认 MAG/MIN 选择规则,或对非均匀缩放显式跳过;
- 先保证 `engine_settings` 注入路径与官方设置读取存在(当前示例没有注入)。

---

### 4.8 P2-8 · 后端未消费 `fixed_state()` / `blend_table_json()`

契约 `docs/render-parity.md:397-400` 明确要求“用 `BlendMode` + `json::blend_table_json()` 的 `fixed` 状态;数值是 D3D9 原始枚举”。`crates/yu-gfx/src/layer.rs:330-505` 提供了 `fixed_state()`,`json.rs:757` 提供 `blend_table_json()`。

但 `yu-backend-soft` 完全没有引用它们:

```text
$ grep -n "fixed_state\|blend_table\|FixedPipelineState" crates/yu-backend-soft/src/*.rs
(no output)
```

后端在 `render.rs:731-755` 手写了 MODE 0/1/2 的公式。后果:

- 状态表/`render-parity` 与后端实现之间没有单一事实源;MODE 2 的 diffuse 缺失、alpha 公式错误都无法通过“读状态表”自动发现。
- 后续要实现 MODE 3/5/6/7/9 或修正 MODE 0 stage1 时,必须再手写一套,而不是复用状态表。

建议:

- 至少用 `fixed_state()` 做断言/交叉检查(例如在 debug 构建里验证手写公式与状态表的 SRCBLEND/DESTBLEND/BLENDOP 一致);
- 更理想的是实现一个 D3D9 固定管线求值器,从 `FixedPipelineState` + 纹理采样 + diffuse/TFACTOR 推导像素,而不是三个特例公式。

---

### 4.9 P2-9 · `image_probe_order` 只在 `TextureStore` 构造时生效

- `lib.rs:27` 说“按 `frame.image_probe_order` 探测扩展名”。
- `render.rs:136-137` 的 `render_frame` 只调用 `self.textures.begin_frame()`,**不读取** `frame.image_probe_order`。
- `texture.rs:108-123` 在 `TextureStore::new` 时保存 `probe_order`,之后不再更新。
- 示例在 `first_frame.rs:124-128` 用选中帧的 order 构造 store;首帧的 yscfg order 稳定,所以实跑正确。

后果:如果调用方复用一个 `SoftRenderer` 渲染不同 `image_probe_order` 的帧(例如宿主切换工程/设置),后端会静默使用旧 order。当前 ERIS 场景不会触发,但 API 契约含糊。

建议:

- 在 `render_frame` 里比较 `frame.image_probe_order` 与 store 的 order;不一致时更新或返回诊断;
- 或者把契约改成“probe order 是 `TextureStore` 构造参数,帧字段仅供装配层使用”,并在 README/lib 文档里写清楚。

---

## 5. P3:诊断、覆盖与残留风险

### 5.1 P3-1 · 读取/归一化失败被归为 `asset.missing`

`texture.rs:235-248`:

```rust
let bytes = match self.source.open(&lookup) {
    Ok(bytes) => bytes,
    Err(diag) => {
        return Cached {
            resolved: Some(candidate.clone()),
            result: Err(Diag::recoverable(code::MISSING, Boundary::Data)
                .at_data(candidate.clone(), 0)
                .with_detail(format!("命中 {} 但读取失败: {}", ..., diag.detail))),
        };
    }
};
```

`texture.rs:203-215` 对 `LookupKey::new` 失败也返回 `code::MISSING`。但 `texture.rs:23-31` 的码表写的是:

- `asset.missing` = 所有探测候选都不存在;
- `asset.decode` = 找到文件但解码失败;
- `asset.unsupported` = 格式已识别但没有解码器。

“候选存在但 `open` 返回 I/O/corrupt/NOT_DIR”既不是“都不存在”,也不是“解码失败”。`AssetIssue` 只保留 `diag.detail`,丢掉了原始 `vfs.io`/`vfs.path.collision` 等 code 与 severity。对排障来说,这会把“包坏了”误导成“文件没有”。

建议:

- 增加 `asset.io` / `asset.path` 码,或把原始 code 拼进 `AssetIssue.code`/detail;
- 保留 `Diag` severity,不要把 fatal VFS 错误降级成 recoverable asset missing。

### 5.2 P3-2 · Image op 的 skip target 用 source 而不是 ID2 target

`render.rs:549-558`:

```rust
Op::Image(image) => image.source.display(),
```

但 `ImageOp` 有 `target: Option<LayerId>`(`op.rs:177-198`),`ID2` 存在时加工结果写入 target。跳过报告按 `(kind, reason, target)` 聚合,用 source 会把“写入 ID2 的加工”归因到 ID1。

建议:`image.target.as_ref().unwrap_or(&image.source).display()`。

### 5.3 P3-3 · “raw key 优先”未取证

`texture.rs:282-299` 的 `candidates`:若 key 自带扩展名,先试原 key;`README.md:61-63` 也这么写。

但 `FUN_0040c9b4` 的反编译显示的是:规范化路径后,按 `yscfg+0x1c` 的 code 逐个 **追加** `.png/.bmp/.jpg/.gif`(行 60-160 一带),没有看到“先原样试 key”的分支。真实脚本的 `FILE` 通常不带扩展名,所以首帧不受影响。但如果某个脚本传了 `foo.png`,后端会先找 `foo.png`,引擎可能找 `foo.png.png`;行为可能不同。

建议:用一个受控脚本确认;确认前把 README 的“先试原 key”标为 Likely/Uncertain,不要写成已取证事实。

### 5.4 P3-4 · 半像素/16.16 变换未建模

`docs/render-parity.md:43` 记录引擎绘制函数 `FUN_00414ba4` 行 220-275 使用 `ROUND(v*65536)` 转 16.16,行 157/190/214 有 `-0.5` 半像素常量;`render-parity.md` §7 又说 gfx 不做 16.16/半像素。

后端 `draw_image`(`render.rs:596-648`)直接用整数矩形和 texel-center 采样;没有证据证明它与引擎的 D3D9 quad/UV 半像素约定等价。对无缩放、无旋转的直绘,可能只是边缘/采样相位差异;对缩放/亚像素定位,可能可见。

建议:等官方像素锚点打通后,用 1px 偏移/缩放样本做对拍;在那之前把“几何/采样相位”列为 Unverified,不要用当前 PNG 做边缘判断。

补充:MODE 0 在单纹理时 `SetTexture(1,NULL)`,而 stage1 的 color op 仍是 `SELECTARG1(TEXTURE)`;D3D9 对未绑定 stage 的可见行为本地没有逐像素取证(`docs/render-parity.md:224-225` 已把 MODE 0 标为 **Likely**)。后端完全忽略 stage1,直接按纹理 RGB 绘制。这也是 MODE 0 只能算 Likely、不能算 Confirmed 的原因之一。

### 5.5 P3-5 · 逐格式解码/颜色键未建模

后端把 PNG/BMP/JPG/GIF 全部交给 `image` crate(`texture.rs:250-256`)。引擎的 `FUN_0040c9b4` 对 BMP 有独立的通道顺序/加载分支(`DAT_008015b4`、`+8/+9` 字段),并可能涉及颜色键/透明色;这些语义没有进入后端。

建议:用真实 BMP/GIF 资产做受控对拍;确认引擎是否做颜色键、BMP BGR/RGB 顺序、GIF 首帧/透明色;未确认前把“逐格式解码 = 1:1”标为 Uncertain。

### 5.6 P3-6 · `clip`、未分类 `extra`、UV wrap

- `Layer.clip`(`layer.rs:956`)当前在 gfx 里恒为 None(所有构造点),但 `json.rs` 会序列化它;后端完全不看。若未来填充,后端会静默忽略。属潜在缺口。
- `apply_layer_params` 对 `LINT/TSX/TSY/...` 放进 `extra`,不改变 `LayerStatus`;首帧 `LINT` 出现 29,068 次(`render-parity.md` §2)。**2026-10-29 更新**:`DIV-GFX-011` 已定案——`LINT/LINT2` 是引擎 i64 字段 `+0x58/+0x60`(按钮号,消费方是点击分派)、`TSX/TSY/DXDRAW/CASH` 在属性块无读取点、`MIPMAP` 不改采样、`DXBUF` 是全局开关 ⇒ 这些参数**确认不影响像素**,后端绘制这些层是正确的;`RID`(资源图)另记 `gfx.image.rid.unsupported`。
- `render-parity.md:276-277` 记录全局 UV 平铺/镜像(`DAT_00801334` 1/2/3)未实现;`CompositorFrame` 没有对应字段,后端无法表达。

建议:把 `extra` 的“未分类”与“非视觉”区分开;若不能证明非视觉,至少在后端/报告里加一个 `unclassified_params` 风险标记;UV wrap 新增 DIV。

### 5.7 P3-7 · 测试覆盖缺口

现有 16 个测试全部通过,但覆盖的是:

- 2×2 不透明 PNG 直绘;
- 1×1 黑图拉伸;
- 缺失资产;
- 文本计数;
- COPY 跳过;
- 纹理探测/PSB/raw key 等单元行为。

**没有覆盖**:

- 半透明纹理的 alpha(尤其组合成);
- Linear 采样(只有 Point 有测试);
- MODE 2 的 diffuse;
- 零尺寸组;
- 组 transition;
- `image_probe_order` 变化;
- YPF 缺失/默认路径;
- BootConfig 的 paths/`_DXMODE`;
- 资产读取失败与错误分类;
- 混合缩放的 MAG/MIN;
- mip/anisotropic。

而且 `render.rs:762-782` 的两个 alpha 断言**把错误值钉死**了;修 alpha 公式时必须同步改测试。

---

## 6. 已经验证成立/实现正确的部分

以下不是“看起来合理”,而是代码路径 + 实跑/测试对得上的:

| 项 | 证据 |
|---|---|
| 严格按 `frame.passes` 顺序执行,不重排 | `render.rs:141-182`;实跑 15 pass/44 op |
| `Texture` pass 全部跳过并计数 | `render.rs:144-153`;实跑 12 条 texture skip |
| `Offscreen` 用 pass.clear,`Screen` 用 `frame.clear` | `render.rs:162-179`;`render-parity.md:372-376` |
| 组 `A/MODE` 只在 `Op::Group` 合成时作用一次 | `render.rs:213-243` |
| `A` 固定管线 clamp 到 0..255 | `render.rs:520-522`;单测 `alpha_clamps_to_255` |
| `SolidBlack1x1 + SX/SY` 铺满 | `render.rs:324-348`;单测 `solid_black_1x1_is_stretched_to_sx_sy`;实跑 W1/DBG 260×600 |
| 资源图 + SX/SY 跳过 | gfx `LayerStatus::Unimplemented("gfx.image.size.unsupported")`;后端 `render.rs:292-299` 双保险 |
| COPY/图像加工跳过并计数 | `render.rs:144-153`、`252-259`;实跑 1 copy + 11 image + 16 snapshot + 1 image-layer skip |
| 缺失资产产生 `asset.missing` 并进清单 | `texture.rs:200-279`;单测;实跑 0 缺失 |
| 文本只计数不栅格化 | `RenderState::new` 的 `skipped_text: frame.text.len()`;实跑 0 |
| 颜色空间按存储值混合 | 没有线性化;`render.rs:731-755`;`color_space=gamma_encoded_srgb8` |
| PNG 确定性 | `png.rs:16-30` 固定 Fast/NoFilter;单测 + 示例 `deterministic=true` |
| 无 unsafe / 无调试打印 | `#![forbid(unsafe_code)]`;grep 0 |
| 测试/clippy | 16 passed;本 crate 0 warning |

---

## 7. 对提示词/契约“已知事实”的逐条复核

| # | 主张 | 复核结论 | 依据 |
|---|---|---|---|
| 1 | 0.495 `FUN_00414ba4` 对 MODE 0/1/2 设 `SRCBLEND=5`、`DESTBLEND=6/2`、`BLENDOP=1/3` | **Confirmed** | 反编译行 445/450/455、492/497/502、543/548/553 |
| 2 | “只设 `D3DRS_ALPHABLENDENABLE`(0xAB)” | **Refuted** | `0xAB=BLENDOP`;`ALPHABLENDENABLE=0x1B`,由 `FUN_00462e40` 行 160-166 设置 |
| 3 | “未设 `SEPARATEALPHABLENDENABLE`,alpha 与 RGB 用同一组因子” | **Confirmed(有保留)** | 本地反编译树 grep `0xce,` 无命中;Microsoft 文档默认 FALSE;但这是“未找到写入”,不是绝对证明 |
| 4 | 后端实现了上述“同因子”alpha | **Refuted** | `render.rs:739/745/751` 用 `As + …`,不是 `As² + …` |
| 5 | MODE 2 按 `dst − src·a` 可执行 | **Unverified** | 引擎 stage0 是 `MOD(DIFFUSE,TEXTURE)`;DIFFUSE RGB 是 DIV-GFX-019 open;后端按白 diffuse |
| 6 | 组离屏尺寸/清屏 = 视口/透明黑 | **Likely(未取证)** | DIV-GFX-008 open;RT 格式/alpha 还没进 DIV |
| 7 | 清屏 = 不透明黑、颜色空间 = gamma | **Likely(未取证)** | DIV-GFX-009/010 open;首帧用了这两个默认 |
| 8 | `first_frame` = 官方首帧 | **Refuted** | 实跑 frame 3;DIV-H-001 withdrawn;官方 L1 0 命中 |
| 9 | boot 已完成/已对齐 | **Not established** | 示例没有 boot 完成判据(`main_finished` 对主循环通常是常态,不能当判据);paths 未注入;`es_First.yst:28` 需要 `$_SAVE_PATH` |
| 10 | 消费了 `yu-font` 公开契约 | **Refuted** | `Cargo.toml` 无 `yu-font`;文本只计数 |
| 11 | 16 个测试全绿、本 crate clippy 0 告警 | **Confirmed** | 本次实跑 |
| 12 | “同帧 PNG 逐字节一致” | **Confirmed(自洽)** | 示例 + 单测;但只证明确定性,不证明正确性 |
| 13 | AGENTS.md §2 的 `<project-eris>/data/ysbin` | **Refuted(过期)** | 实际只有 `<project-eris>/save/data/ysbin`(143 文件);任务书/README 用的是 save 路径 |

---

## 8. 为什么现在不能做像素对拍

把当前状态用一句话说清楚:

> **当前 PNG 是“内核启动链在逻辑帧 3 的、按一组 Likely/Unverified 假设渲染出来的 smoke test 图”。**

具体阻塞:

1. **锚点不成立**:官方事件级 L1 未达成;官方屏幕级是标题菜单,内核是 frame 3 系统文本框;DIV-H-001 已撤回比较。
2. **boot 环境不等价**:`first_frame` 没注入 HostPaths,`$_SAVE_PATH` 走 Unimplemented;`@_DXMODE`/`@_SCREEN_COLOR` 用默认值;没有 boot 完成判定。
3. **渲染输入不完整/不精确**:
   - 12 个 Texture pass(1 copy + 11 image)跳过;
   - 17 个 snapshot 按钮层跳过;
   - 1 个图像加工层跳过;
   - 文本 0 栅格化;
   - `filter.source=placeholder`;
   - 清屏/颜色空间/组 RT/MODE 0 都是 Likely;
   - MODE 2 的 diffuse 未取证。
4. **像素语义仍有确定错误**:alpha 通道公式;这会直接影响组合成。
5. **没有跨后端 oracle**:README 里的 `yu-backend-wgpu` 同帧逐像素对比尚未完成(审查时该 crate 还是未跟踪的新增目录,不在 HEAD)。

因此,现在的正确动作不是“把 PNG 和官方截图比一下”,而是:

1. 先修 P1(alpha、MODE 2)与 P2(组 RT、零尺寸、组过渡);
2. 让 `first_frame` 注入与 `run_eris` 相同的 HostPaths,输出 `main_finished/errors/events_dropped`,并改名/注明“非官方首帧”;
3. 打通事件级 L1(同一 `(script, pc, frame)` 锚点);
4. 再由官方 producer 在同一锚点采集屏幕/帧描述;
5. 最后才做 soft vs wgpu、soft vs official 的像素对比。

---

## 9. 建议的修复/验证顺序

| 顺序 | 动作 | 关联发现 |
|---|---|---|
| 1 | 修 alpha 公式 + 改单测 + 加组半透明集成测试 | §3.1 |
| 2 | 取证 diffuse RGB,或把 MODE 2 降级为 Unimplemented | §3.2 |
| 3 | 新增组 RT 格式/alpha DIV;定位 `+0x61c` 的 D3DFMT/写掩码 | §4.1 |
| 4 | 修零尺寸 pass 的 `ops_total` + 测试 | §4.2 |
| 5 | Group transition 显式 skip/计数或实现;统一 Transition 策略 | §4.3/§4.4 |
| 6 | `first_frame`:注入 HostPaths,检查 `tick_at` report,改名/注明非 oracle | §4.5 |
| 7 | 修 YPF 默认探测;README 去掉“可选”误导 | §4.6 |
| 8 | 过滤:mip/anisotropic 显式标记;注入 engine settings | §4.7 |
| 9 | 用 `fixed_state()` 交叉检查/驱动 blend | §4.8 |
| 10 | 诊断码/目标/覆盖率补强 | §5 |
| 11 | 事件级 L1 → 官方帧锚点 → 像素对拍 | §8 |

---

## 10. 附录:复现命令与原始结果

```bash
export TEMP="<side-repo>/workbench/tmp"; export TMP="$TEMP"
export YURIS_REQUIRE_SAMPLES=1
export YURIS_SAMPLE_ERIS_YSBIN="<side-repo>/workbench/<project-eris>/save/data/ysbin"
export YURIS_SAMPLE_ERIS_YPF="<side-repo>/workbench/<project-eris>/data.ypf"

cargo test -p yu-backend-soft
# 10 lib + 5 render + 1 doc = 16 passed; 0 failed

cargo clippy -p yu-backend-soft --all-targets
# yu-backend-soft 0 warning; 5 warnings from dependency yu-rt

cargo run -p yu-backend-soft --example first_frame
# frame 3 viewport 800x600; layers 272 (visible 34); passes 15; offscreen 2
# ops 44 total / 15 drawn / 29 skipped; missing assets 0; skipped text 0
# filter mag=point min=point mip=none source=placeholder
# color gamma_encoded_srgb8; decoded=13 cache_entries=13
# deterministic=true; png 785127 bytes

cargo run -q -p yu-rt --example run_eris -- <side-repo>/workbench/<project-eris>/save 3 0
# frame 0: cmds=1290326 errors=0 main_finished=false
# frame 1: cmds=274466  errors=0 main_finished=false
# frame 2: cmds=3315    errors=0 main_finished=false
```

产物摘要:

```text
target/tmp/first_frame.png        800x600 RGBA, 785127 bytes
sha256 59f6f5b7ba72a9485a28641369429d9f8a815c17685fbf8cf2fc3a9732e4bfac
target/tmp/first_frame.display.json 19822 bytes
sha256 522b6bfb646abd2c68be796612e9fbba43a44faae60485ea60c2f8521e828212
```

**审查边界声明**:本报告没有把上述产物当作 correctness 证据;它们只用于证明“当前实现确实这样运行”。所有涉及像素是否与官方一致的主张,均以 `Uncertain`/`Unverified` 标注,并给出还缺哪项证据。
