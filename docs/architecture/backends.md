# 双后端架构:`soft` = CPU 模式,`wgpu` = GPU 模式(两条独立实现)

- 状态:2026-10-27 立项(此前多轮性能收口均因违反本原则而返工)
- 适用:`crates/yu-render/soft`、`crates/yu-render/wgpu`、`crates/yu-engine`、`crates/yu-cli`

## 0. 一句话原则

> **`yu-backend-soft` 与 `yu-backend-wgpu` 是两条独立的渲染实现。
> soft 是 CPU 模式(同时充当语义参考/对拍基线);wgpu 是 GPU 模式,
> 它的像素工作必须由 GPU 完成 —— 运行期不得把像素算法委托给 soft。**
>
> 用户报告里反复出现的「越玩越卡」,历史根因之一就是 GPU 后端悄悄走了
> soft 的逐像素循环(先例:MERGE 每帧 40 条 CPU 合成 = 10.9ms;
> 2026-10-27 用合成链记忆化压到 1.5–2.0ms,但**那只是缓存,不是 GPU**)。

## 1. 为什么必须分开

1. **交叉验证**是双后端的全部价值:两条独立实现逐像素对拍,才能发现
   「官方到底怎么算的」。若 GPU 后端调用 CPU 实现,对拍退化成自己和自己比。
2. **性能上限**:GPU 后端被 CPU 逐像素循环拖住时,用户看到的就是
   「GPU 模式还是卡」。这类瓶颈此前已修过多轮(线条/文字/CG/合成)。
3. **职责清晰**:出问题时必须能立刻回答「这是 GPU 画错了,还是 CPU 兜住了」。
   混在一起时,`SKIP`/计数/像素三张证据会互相掩盖(立绘归位那帧的
   `snapshot_missing` 就是被 CPU 侧的空实现掩盖了一整个规划阶段的原因)。

## 2. 允许共享的东西(只有契约与平台设施)

| 类别 | 内容 | 归属 |
|---|---|---|
| 输出/报告契约 | `Surface`、`blit`、`RenderReport`、`SkippedOp` | `yu-render-core`(2026-10-27 已迁) |
| 资产解码 | `TextureStore`(vfs 探测、PNG/BMP/JPG/GIF/PSB 解码)、`Texture`、`AssetIssue` | `yu-render-core`(已迁) |
| 字形栅格化/排版 | `TextComposer`、`TextPaintStats`、`text_base_key`、`ComposedLayer` | `yu-render-core`(已迁;**字体栅格天然是 CPU**,不是后端职责) |
| 图像缓存键 | `ImageCacheKey`/`ImageOpSig`/`ImageSource`/`SizeKey`/`SizeSource` | `yu-render-core`(已迁) |
| 栅格原语 | `scale_region` | `yu-render-core`(已迁;GPU 只允许在 CPU 读回路径用) |
| PNG | `png_bytes` | `yu-render-core`(已迁) |
| 语义常量 | `fixed_pipeline_alpha`、`shader_alpha`、`transition_coverage` | `yu-gfx` |

`yu-render-core` 里**不放任何后端行为**:软后端的 `apply_*` 与 wgpu 的
管线/目标管理各自留在自己的 crate。`yu-backend-soft` 为兼容保留了同名
再导出(`pub use yu_render_core::…`),新代码应直接用 `yu_render_core::`。

## 3. 判定规则(机械可查)

在 `crates/yu-render/wgpu/**` 里:

- **违规**:`yu_backend_soft::apply_*`(rect_paint/merge/bitwise/resize/trim/
  meido/reverse/horizontal/vertical/rotation/saido/akarusa/mono/mosaic/blur*/
  diffusion/whirlpool/raster)与 `yu_backend_soft::blend_transition`
  —— 这些是**CPU 后端的像素实现**。
- **允许**:`yu_render_core::*`(契约与平台设施,见 §2)。其中
  `scale_region` 只允许出现在「CPU 消费者确实需要像素」的读回路径,并需在
  调用处标注原因;`blit`/`text_base_key` 等为共享原语。

审查命令(应只剩账本 §4 里在册的 `apply_*`):

```bash
grep -rn "yu_backend_soft::" crates/yu-render/wgpu/src
```

合规结果应只有 `apply_*` / `blend_transition` 这一类**在册待清**的算法调用;
出现 `Surface`/`TextureStore`/`TextComposer`/`ImageCacheKey` 等指向
`yu_backend_soft` 即回退(它们应来自 `yu_render_core`)。

## 4. 现状账本(2026-10-27 实测)

| 动作 | wgpu 现状 | 合规 | 目标实现 |
|---|---|---|---|
| `SIZE`/`SIZE2` | UV 子矩形 + 浮点目标尺寸(采样器) | ✅ GPU | 保持 |
| `HORIZONTAL`/`VERTICAL`(层) | UV 负步长 | ✅ GPU | 保持 |
| 层绘制混合/过渡 | 固定管线 + 过渡 pipeline | ✅ GPU | 保持 |
| `COPY`/快照 | `copy_texture_to_texture` + 快照 RT | ✅ GPU | 保持 |
| `RECTPAINT` 族(目标是快照) | GPU fill(`PaintKind::Solid`) | ✅ GPU | 保持 |
| `MERGE`/`MERGE2`(目标是快照) | GPU quad(`PaintKind::Texture`) | ✅ GPU | 保持 |
| **`MERGE`/`MERGE2`(普通层)** | `yu_backend_soft::apply_merge`(CPU) | ❌ | 离屏覆盖目标(`Override::Pass`)+ 同一 merge pipeline |
| **`RECTPAINT` 族(普通层)** | `apply_rect_paint`(CPU) | ❌ | 同上,GPU fill |
| **`XOR`/`AND`** | `apply_bitwise`(CPU) | ❌ | GPU blend 模式 |
| **`TRANSITION`(CGACT 图像加工)** | `blend_transition`(CPU) | ❌ | 复用层绘制的双纹理过渡 pipeline |
| **`REVERSE`** | `apply_reverse`(CPU) | ❌ | 通道取反 shader |
| **`HORIZONTAL`/`VERTICAL`(图像加工)** | `apply_horizontal/vertical`(CPU) | ❌ | 与层路径统一为 UV 负步长 |
| **`RESIZE`/`TRIM`/`MEIDO`** | `apply_resize/trim/meido`(CPU) | ❌ | 几何变换走顶点/UV,必要时 GPU pass |
| **`ROTATION`/`SAIDO`/`AKARUSA`/`MONO`/`MOSAIC`/`BLUR*`/`DIFFUSION`/`WHIRLPOOL`/`RASTER*`** | `apply_*`(CPU) | ❌ | 各自 GPU pass/shader |
| 快照像素重建(`snapshot_pixels`) | `scale_region`(CPU) | ⚠️ 条件合规 | 只在「CPU 消费者确实需要像素」时保留(文本合成基面、需要读回的后续加工),并标注为**读回路径**,不作为常规渲染路径 |
| 文本合成 | `TextComposer`(`yu-render-core`) | ✅ 共享设施 | 已迁(2026-10-27) |
| 资产解码 | `TextureStore`(`yu-render-core`) | ✅ 共享设施 | 已迁(2026-10-27) |

行号锚点(`crates/yu-render/wgpu/src/passes.rs`,2026-10-27):
`blend_transition` 2954、`apply_merge` 2977–2978、`apply_bitwise` 2979–2980、
`apply_rotation` 3042/3067、`apply_saido..raster` 3050–3065、
`apply_rect_paint` 2998、`apply_reverse` 3004、`apply_horizontal` 3010。

## 5. 迁移顺序与验收

按「运行期出现频率 × 面积」排序(现场实测数据来自 β4 演出1 / 对话窗):

1. **普通层 `RECTPAINT` 族 + `MERGE`/`MERGE2`**(已确认每帧 41 条、10.9ms):
   给普通层加离屏覆盖目标 `Override::Pass` —— 首次 op 建 RT(尺寸取基图
   或层声明),`RECTPAINT` 用 `LoadOp::Clear`,后续 `MERGE` 用现有
   `merge_pipeline` 画 quad,最终绘制/`COPY` 直接采样该 RT。
   **合成链记忆化(2026-10-27 已上线)只是过渡**:命中时跳过 CPU 链,
   miss 时仍走 CPU;`Override::Pass` 落地后 CPU 链整体退役。
2. `TRANSITION` / `REVERSE` / `XOR` / `AND`(shader 或 blend 模式)。
3. `HORIZONTAL`/`VERTICAL` 图像加工统一为 UV 负步长(与层路径同源)。
4. `ROTATION`/`SAIDO`/`AKARUSA`/`MONO`/`MOSAIC`/`BLUR*`/`DIFFUSION`/
   `WHIRLPOOL`/`RASTER*`:按语料实际出现顺序逐个 shader 化;未实现前
   保持 CPU 并**在报告里显式标注 `gpu.cpu_fallback.<action>`**,不允许静默。

**每一项的验收口径(缺一不可)**

1. `soft` 与 `wgpu` 对同一场景逐帧像素对拍(现有 `scenario_probe
   --hash-csv` 两遍对比,允许的舍入差按 `render-parity.md` 口径);
2. `grep` 规则(§3)通过:该动作不再出现在违规列表里;
3. `cargo test -p yu-backend-wgpu --all-targets` 全绿,且新增用例覆盖
   「该动作在 GPU 上完成」(报告里不出现 CPU 回退标注);
4. 在关闭 soft(或让 soft 对应函数 `unreachable!()`)的构建下仍能出画面
   —— 这是「没有偷偷走 CPU」的硬证明。

## 6. 后端选择(两个独立项目,同一入口)

`Backend` 是运行期开关(`yu_engine::engine::Backend::{Soft, Wgpu}`),
CLI 已暴露:

```bash
yu-cli run <project> --mode desktop --backend wgpu   # GPU 模式(产品路径)
yu-cli run <project> --mode headless --backend soft  # CPU 模式(无 GPU/确定性)
yu-cli serve <project> --backend soft                # 服务端 CPU 渲染
yu-cli dev <project> --backend wgpu
```

缺省:`headless` = `soft`(确定性、无 GPU 依赖),`desktop` = `wgpu`。
两个后端消费**同一份** `CompositorFrame`(`yu-gfx` 是唯一语义来源),
互不引用实现;**soft 可以理解为「CPU 模式」**,不是 wgpu 的兜底。

## 7. 耦合审查(2026-10-27)

### 7.1 依赖方向(合规,无需动作)

| 检查 | 结果 |
|---|---|
| `yu-gfx` 是否依赖任一后端 | 否(只依赖 `yu-log`/`yu-rt`) |
| `yu-backend-soft` 是否依赖 `yu-backend-wgpu` | 否 |
| `yu-backend-wgpu` 是否依赖 `yu-backend-soft` | **是,但只剩 §4 在册的 `apply_*`**(契约/设施已迁 `yu-render-core`) |
| `yu-engine` 构建渲染器的位置 | `engine.rs:461-485`,按 `opts.backend` 分支,失败即报错 |

### 7.2 已修:GPU 启动失败会自动降级到 CPU

`crates/yu-cli/src/desktop.rs` 原先在 `wgpu` 初始化失败时**静默回退 soft**
(只打一条 `host` 级 Warn)。后果:用户以为在看 GPU 结果,实际是 CPU 结果,
性能/画面问题的定位全部失真 —— 这与「不能有兜底」的纪律直接冲突。

已改为**显式失败**:

```text
wgpu 后端启动失败:<原因>
本内核不会自动回退到 CPU 模式;确需 CPU 渲染请显式加 --backend soft。
```

`yu-engine` 侧本来就是硬失败(`Failure::runtime("wgpu 初始化失败…")`),
这次只是把宿主层多出来的兜底删掉。

### 7.2.1 已修:`--backend` 有两套解析

`run.rs`(headless)与 `desktop.rs`(desktop)各写了一份取值校验,缺省值与
错误文案都可能漂移。已统一为 `Backend::parse`,两个入口只保留**缺省策略**
(headless=soft、desktop=wgpu)—— 缺省属于模式策略,不重复实现校验。

### 7.3 已清:契约类型不再寄居 soft(2026-10-27)

新建 `crates/yu-render/core`(`yu-render-core`),承接:两后端的表面/报告
(`Surface`/`blit`/`RenderReport`/`SkippedOp`)、资产解码(`TextureStore`/
`Texture`/`AssetIssue`)、字形栅格(`TextComposer`/`TextPaintStats`/
`text_base_key`)、图像缓存键(`ImageCacheKey`/`ImageOpSig`/`ImageSource`)、
共享栅格原语(`scale_region`)、PNG。

- `yu-backend-soft` 保留同名再导出(既有路径不破),实现留在本地;
- `yu-backend-wgpu`/`yu-engine`/`yu-cli`/`host-desktop` 改为直接引用
  `yu_render_core::`;
- **行为中性**:6200 帧逐帧屏幕哈希与重构前完全一致(`differing 0`);
  38 个测试套件全绿。

剩下的唯一 `wgpu → soft` 依赖就是 §4 账本里的 `apply_*` 像素算法。

### 7.4 唯一允许两后端共存的地方:对拍

`yu-engine` 在 `opts.reconcile == true` 时,于 `Renderer::Wgpu` 变体里额外持一个
`SoftRenderer` 做逐帧比对(`check` 字段)。**这是对拍装置,不是回退路径**:

- 缺省关闭(`EngineOptions::reconcile = false`);
- 它读的是同一份 `CompositorFrame`,不参与 wgpu 的任何像素产生;
- 比对结果只进报告/诊断,不影响上屏。

新增「两后端共存」代码前先问一句:是不是对拍?不是就不允许。

### 7.5 宿主层的边界

`crates/yu-platform/host-desktop` 通过 `Engine::wgpu_renderer()` 拿 device 做
窗口呈现与 egui 渲染,并要求 `--backend wgpu`(soft 下显式报错要求切后端)。
这是**平台/呈现层**的职责,不是渲染语义;后端内部的 `Targets`/管线缓存不对外暴露。
