# 任务书 · 图层表面常驻(渲染契约对齐官方,一次做完)

> 主人令(2026-10-28):**不分阶段,一次做完**;性能 / 证据 / 回归三者并重。
> soft 后端只保留为**对拍 oracle**(用户不跑 soft);用户真实体验 = wgpu。
> 净室纪律不变:证据优先、不猜、`Unimplemented` 不得作为终态。

## 0. 一句话目标

把 wgpu 后端从「每帧重放 op 流 + CPU 合成 2048² 缓冲 + 整张上传 + 整面快照拷贝」
改成官方模型:**每个图层一张常驻 GPU 表面,`CGACT` 就地改它,每帧只把可见层
blit 到后缓冲**。soft 保持现状,仅作为逐像素参考。

## 1. 证据

### 1.1 官方模型(REA,均在仓库内)

| 事实 | 锚点 |
|---|---|
| `CGACT` 在命令处理器内**就地**改目标表面 | `crates/yu-gfx/src/state.rs:3440`(`FUN_0042f809`:`*(iVar4+0x90) = local_68`) |
| `COPY` 直接改纹理,不读"上一帧"缓冲 | `docs/divergences/DIV-GFX-051-main-group-copy-first-frame.md:39` |
| 图层有常驻像素表面;存档直接读它 | `crates/yu-gfx/src/host.rs:37`(读取端 `0x4477c0`,首 u32 = `0x640` 位图头) |
| `CGACT[TEXT]` 即时写层表面(文本不逐帧重烘) | `docs/divergences/DIV-GFX-055-cgact-text-layer-compose.md`;`crates/yu-render/wgpu/src/passes.rs:499` 附近注记 |
| `MERGE`/`MERGE2` alpha 公式未 REA 级验证 | `docs/divergences/DIV-GFX-063-no-manual-image-filters.md` §2.4 |

### 1.2 现实现模型

- `GfxState::frame()`(`crates/yu-gfx/src/state.rs:529`)每帧从图层表 + 命令记录
  **重建** `CompositorFrame`(`crates/yu-gfx/src/frame.rs:264`):`layers` 全量、
  `passes` = 本帧需要执行的 op(`PassTarget::{Screen,Offscreen,Texture}`,
  `crates/yu-gfx/src/pass.rs:16`)。
- wgpu 后端 `passes::plan`(`crates/yu-render/wgpu/src/passes.rs:237`)在**规划期**
  把每个图层的 op 链用 CPU 合成成 `Override::Cpu`(`passes.rs:2350-2470` 三态:
  复用 / `TextureCache::rewrite` 原地写 / 新上传),再逐帧上传。
- 快照(`CopyId`)在 `TargetSet`(`crates/yu-render/wgpu/src/textures.rs`)里是独立
  纹理;`COPY` = `copy_texture_to_texture` 整面(实测 2112²、读写 35MB/次)。
- 文本每帧由 `TextComposer`(`crates/yu-render/core/src/text.rs:66`)烘焙进覆盖纹理。
- 存档抓帧走 CPU 镜像表(`crates/yu-engine/src/capture.rs`)+ 每帧 GPU 读回
  (`SNAPSHOT_READBACK_CAP`,`crates/yu-engine/src/engine.rs:30`)。

### 1.3 实测账单(必须在验收时复现/复核)

<game-b>(v494 商业包,1200 帧,自动推进到本篇,`--no-sound`,wgpu):

```text
每帧口径            数值
image(CPU 合成)    p95 10–35ms
plan(含合成)       p95 16–58ms
上传               平均 8.8MB/帧,重帧 54–115MB
快照 GPU 拷贝       重帧 107–186MB(2 次 × 35MB,每帧都有)
draw                31 个 draw、124 个实际 op(帧本身很轻)
```

即:帧的**重**完全来自"每帧重算 + 搬运图层像素",与 GPU 算力无关。

## 2. 目标契约

### 2.1 图层表面(LayerSurface)

- 每个活图层一张 GPU 纹理:`{ texture, view, width, height, content_rev, chain_sig, last_frame }`。
- 生命周期:`CG` 建层/改尺寸时确保,`CGEND` 后按存活集合剪枝,视口变化重建。
- 尺寸 = 图层声明尺寸(与官方一致;不做 bbox 缩表——那是另一条分歧)。
- 预算:沿用 `TargetSet` 的字节上限/容量上限与剪枝口径,超限报诊断(不静默)。

### 2.2 命令应用语义

- **按命令序(`Op.seq`)就地应用**,不是"帧末重建":`COPY`/`RECTPAINT*`/`MERGE`/
  `MERGE2`/`TRANSITION`/`SIZE`/`SIZE2`/`REVERSE`/`TRIM`/`ROTATE` 全部写成对该
  表面纹理的 GPU pass(已有:lerp/merge/paint/quad+UV;缺的补 pass)。
- 读写同纹理禁止:同层链式操作用**双槽 ping-pong**(现 `TargetSet.composites`
  已具备),链末落回主表面。
- 层基础纹理(资产/快照)变化时,首条 op 以它为新基面;无 op 的层直接沿用表面。
- `E=0`/alpha=0/完全在视口外 ⇒ 不产生 pass,但表面内容保持(与官方一致:
  不可见不影响表面)。

### 2.3 快照三态(按证据选择,不许猜)

1. **别名**:源表面本帧之后不再被 `CGACT` 改写、且快照本身不再被 `MERGE`
   当作目标 ⇒ 快照 = 源纹理句柄(零拷贝)。
2. **真拷贝**:上述条件不成立 ⇒ `copy_texture_to_texture` 只拷 `Op::Copy.extent`
   (已是区域级);目标纹理按 `CopyId` 跨帧复用。
3. **读回**:仅当 SAVE 抓帧真的要用这张快照且 CPU 侧拿不到源像素时才读回
   (按需、上限 1 次/存档,不逐帧)。

### 2.4 文本

- `CGACT[TEXT]` 的绘制项进**目标层表面**(一次 pass),层内文本随表面常驻;
  文本内容/排版变化才重画。
- `TextRun`(对话框文本)保持现有烘焙路径,但结果贴进图层表面而不是每帧整面重烘。
- soft 的文本烘焙保持不变(对拍基准)。

### 2.5 帧 = blit

- 屏幕 pass = 按 `(z, seq)` 逐可见层一个 quad(带 alpha/blend/mask/transition),
  不再有 "图层自合成" 与 "覆盖纹理上传"。
- 屏幕读回保持现非阻塞实现(桌面呈现走 `screen_texture`)。

### 2.6 soft 的定位

- soft 不改语义、不优化:继续按 `CompositorFrame` 逐 op 重放,作为 wgpu 的逐像素 oracle。
- 两个后端共享的只有 `CompositorFrame`/`Op` 数据结构与 `TextureStore`。

## 3. 工作项(一次做完;内部顺序仅为减少返工)

| # | 内容 | 主要文件 | 判据 |
|---|---|---|---|
| W1 | `LayerSurfaces` 模块:表面表、生命周期、剪枝、预算、诊断 | `yu-render/wgpu/src/surfaces.rs`(新) | 800 层场景表面数与字节数稳定、退出摘要可见 |
| W2 | op → pass:把链式 op 的写入目标从 `Override` 改为层表面 ping-pong;补齐缺的 GPU pass(`MERGE2`/`TRIM`/`ROTATE`/`REVERSE`) | `passes.rs`、`pipelines.rs`、`shaders/*` | 链式 op 不再产生 `Override::Cpu`/上传 |
| W3 | 快照三态(别名/区域拷贝/按需读回),替换整面拷贝 | `passes.rs`、`surfaces.rs` | `copy_bytes` 重帧 ≤ 8MB;别名与真拷贝像素一致 |
| W4 | 文本进层表面 | `passes.rs`、`yu-render/core/src/text.rs`(只读改写) | 文本随表面常驻;`text_bake` 不再逐帧整面 |
| W5 | 屏幕 blit 收口:只画可见层,不再全链重算 | `passes.rs`、`lib.rs` | 帧内 `ops_total`≈可见层数;`plan` ≤ 2ms(稳态) |
| W6 | 存档/抓帧改读表面:删 `GfxCapture.mirrors`、`SNAPSHOT_READBACK_CAP`、每帧读回 | `yu-engine/src/{capture,engine}.rs`、`yu-render/wgpu/src/lib.rs` | SAVE 缩略图与旧实现逐像素一致;帧内无按需读回 |
| W7 | 诊断:新增 `RenderPhases{ surface_passes, blits, alias_hits, copy_bytes, reused_layers }`;spike/CSV 接线 | `yu-render/core/src/report.rs`、`host-desktop` | 每帧可复现账单,不掉任何既有字段 |
| W8 | 旧路径清理:wgpu 删 `Override::Cpu` 的合成/上传分支(soft 不动),保留一版 `YURIS_LEGACY_FRAME_REPLAY=1` 回退开关至验收后删除 | `passes.rs` | 无死代码告警;回退开关可用 |
| W9 | 测试:新增多帧场景对拍(换差分/眨眼/过渡/MERGE/文本/存档),别名等价性、表面复用、确定性 | `yu-render/wgpu/tests/wgpu.rs`、新增 `tests/surfaces.rs` | 新增 ≥8 条测试全绿 |
| W10 | 文档:`DIV-GFX-0xx-frame-contract.md`(帧契约不再逐 op 重放)+ PROGRESS 成果 | `docs/divergences/`、`PROGRESS.md` | 台账齐 |

## 4. 性能目标(验收即门槛)

`<game-b>` 1200 帧自动推进(`--no-sound`),改后必须同时满足:

```text
render p95 ≤ 6ms      (现 16.9ms)
work   p95 ≤ 12ms     (现 30.7ms)
fps        ≥ 55       (现 45.2)
上传       平均 ≤ 2MB/帧,单帧 ≤ 8MB   (现 8.8MB / 115MB)
copy_bytes 单帧 ≤ 8MB                 (现重帧 186MB)
plan       p95 ≤ 4ms                  (现 16.4ms)
```

`<project-eris>` 3000 帧不得回归(`tick p50 ≤ 3.7ms`、启动帧 ≤ 260ms)。

### 测量口径(必须原样使用)

```bash
export TEMP=TMP=D:/Art/ai/projects/YurisDroid/workbench/tmp
export YURIS_SAMPLE_ERIS_ROOT=<project-eris>
export YURIS_SAMPLE_ERIS_YSBIN=<project-eris>/data/ysbin
export YURIS_SAMPLE_SDK0479=<sdk-0479>; export YURIS_SAMPLE_SDK0495=<sdk-0495>

# 逐帧分位数 + 命令成本 + 长帧构成
YURIS_CASE_BENCH=1 ./target/release/yu-cli.exe case <game-b> --case <case> --cases target/tmp

# 桌面逐帧 CSV(关键列:plan_ms/image_ms/render_ms/upload_bytes/copy_bytes/readback_ms/memo_*)
./target/release/yu-cli.exe run <game-b> --mode desktop --frames 1200 --no-sound \
  --script target/tmp/<case>.json --frame-stats target/tmp/a.csv \
  --spike-log target/tmp/a-spike.log --spike-ms 20
```

## 5. 回归门禁(缺一不可)

1. `cargo build --workspace --all-targets`
2. `cargo test -p yu-backend-wgpu`(25 个逐像素对拍 + 单测)
3. `cargo test -p yu-backend-soft` / `-p yu-render-core` / `-p yu-rt` / `-p yu-engine` / `-p yu-sound`
4. `cargo test -p yu-cli --test logging_guards`(直写终端宏 / 静默兜底审计)
5. `yu-cli case <project-eris> --all`(golden:PNG + display JSON + 事件计数)
6. **新增**:换差分/眨眼/过渡/MERGE/文本/存档 的 60 帧序列,wgpu 与 soft 逐帧
   像素一致(允许差 ≤1 的既有采样差异,须逐例说明)
7. 长稳:桌面 2500 帧 exit 0、无 wgpu 验证错误、`gpu targets` 字节稳定

## 6. 纪律

- 不做任何 git 写操作(提交/推送),改动留工作区由主人审阅。
- 被跟踪文件用占位符(`<game-b>`/`<project-eris>`/`<repo>` 等),真名只进 AGENTS.md。
- 禁止 `println!/eprintln!/dbg!`;诊断走统一控制台或 `output::error`。
- 禁止静默回退:任何 skip/降级必须计数 + 诊断(带原因),不得只 warn 一次了事。
- `RenderReport` 参与 soft↔wgpu 对账,新字段放 `RenderPhases`,不改 `RenderReport` 语义。
- 每个改动点必须能指出证据:官方 REA 地址 / 手册页 / 语料自证 / 既有 DIV 档。

## 7. 交付物

- 代码:W1–W8 落地,工作区可编译、全测试绿。
- 测试:W9 的新增用例 + 一份多帧对拍报告(逐帧像素差 0 或逐例说明)。
- 数据:`<game-b>` 1200 帧 / `<project-eris>` 3000 帧两组前后对比表(第 4 节门槛)。
- 文档:W10 的分歧档案 + PROGRESS 成果(含验证命令与原始输出)。

## 8. 风险与回滚

| 风险 | 处置 |
|---|---|
| 命令序与帧末重建不一致 ⇒ 画面差异 | 严格按 `Op.seq` 应用;多帧对拍门禁;差异逐例登记 |
| 图层表面显存增长 | 沿用剪枝 + 字节上限;退出摘要核对;超限诊断 |
| 快照别名破坏"冻结"语义 | 只在不改写的证明成立时别名;否则真拷贝;测试覆盖两种 |
| 文本常驻导致重画时机错 | 修订号(`text_draws` revision)驱动;对拍 |
| 大规模改动引入回退 | 保留 `YURIS_LEGACY_FRAME_REPLAY=1` 至验收通过,此后删除 |
