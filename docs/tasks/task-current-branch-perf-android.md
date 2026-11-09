# 当前分支任务书：渲染尖峰优化 + 安卓测量

> 状态：**初版任务书，未动代码**。主人审查后按此顺序推进。
> 分支：当前 `master`（本地提交，不推送）。
> 基线提交：`3d35ebc`（绑定组剪枝修复）。
> 关联计划：[单表面渲染模型重构计划](task-surface-single-model-plan.md)
> ——本任务书**不做**单表面重构；安卓测完仍不达标时再转入那份计划。

---

## 0. 工作区现状（开工前必须收口）

未提交文件：

```text
M crates/yu-cli/examples/input_probe.rs        # YURIS_PROBE_FPS / YURIS_RES_EVERY
M crates/yu-render/wgpu/src/lib.rs             # [res] bg=/rw=
M crates/yu-render/wgpu/src/plan.rs            # TRANSITION/RECTPAINT 原地复用
M crates/yu-render/wgpu/src/surfaces.rs        # take_for_rewrite / 池 / owned
M crates/yu-render/wgpu/tests/surfaces.rs      # rw/池相关回归
?? docs/tasks/task-surface-single-model-plan.md
```

真机结论：`rw=0`（两把桌面，61+36 次采样），原地复用从未命中；
有效的是**表面池跨帧轮换**（`reused` 1–5/帧，`pooled` 峰值 35–62MB）。

---

## 1. 目标与非目标

目标：

1. 收口工作区：删除无效的原地复用，保留并验证表面池；
2. 降低三个已取证的尖峰：首次入场（preload/解码）、菜单（COPY2 快照）、文本首烘；
3. 在安卓真机上量化残余卡顿，决定是否需要单表面重构。

非目标：

- 不做单表面/就地写的架构改造（另见计划书 P0–P5）；
- 不改 VM、脚本语义、存档格式、gfx 命令语义；
- 不猜官方行为；缺证据的路径保持现状并记录。

---

## 2. 任务总览

| # | 任务 | 主要证据 | 预期收益 | 风险 |
|---|---|---|---|---|
| T0 | 收口：删原地复用、留表面池 | `rw=0`；池已生效 | 代码减负，保留 17.8MB 表面轮换 | 低 |
| T1 | 资产工作集 + 预热移出游戏帧 | frame 78 tick 107ms / preload 13.1ms/33 文件；frame 859 preload 23.8ms | 首次入场尖峰降 30%+ | 中（内存/时序） |
| T2 | COPY2 快照批量/延迟物化 | frame 974 21MB/262 次、render 15.4ms；frame 1149 8MB/61 次；菜单 tick 62–102ms | 菜单尖峰降 30–50% | 中高（命令时刻语义） |
| T3 | 字形跨层复用 | 菜单 170 字形首烘 10.4ms | 首开菜单降 ~10ms | 中（图集内存） |
| T4 | 安卓测量与决策门 | 上述全部 | 判断是否转单表面 | 低（测量） |

建议顺序：T0 → T1 → T2 → T3 → T4。T1/T2 是尖峰大头，T3 独立且低耦合。

---

## 3. T0 收口：删除原地复用，保留表面池

### 3.1 做什么

- 删除 `surfaces.rs::take_for_rewrite`、`owned_textures` 中仅为原地复用的判定
  （池的资产守卫保留）、`stats.rewrite_reused` / `[res] rw=`；
- 删除 `plan.rs` 里 TRANSITION/整层 RECTPAINT3 的 `reused = take_for_rewrite`
  分支，以及 `first_overwrites` 的 `keep_content` 特判（回到“写新表面/池取”）；
- 保留：
  - `acquire` 池弹、`release` 入池（单张 ≤24MB、池 ≤64MB）；
  - `set_sampled` 释放被替换的旧表面；
  - 资产 `owned` 守卫（资产永不入池）；
  - `[res] created/reused/released/pooled/live/vram` 诊断；
  - `input_probe` 的 `YURIS_PROBE_FPS` / `YURIS_RES_EVERY`。
- 测试：删除只验证 `rw` 的用例；保留/补齐“池轮换 + 资产不池化 + 像素一致”用例。

### 3.2 验收

- `cargo build --workspace --all-targets` 通过；
- `cargo test -p yu-backend-wgpu`（现 39 项）与 `--test surfaces`（现 23 项，删改后数量变化）通过；
- koto 桌面跑 2000 帧：无 OOM、`created` 稳态低、`pooled` 有值；
- `PROGRESS.md` 记录本任务；`docs/divergences/` 若有口径变化（rw 删除）补一条说明。

### 3.3 回滚

T0 单独一个本地提交；出问题 `git revert` 该提交即可。

---

## 4. T1 资产工作集固定 + 预热移出游戏帧

### 4.1 证据

- frame 78：`work=109.5ms tick=107.2ms preload=13.1ms/33`，`ops=8113`；
- frame 859：`work=56.5ms preload=23.8ms/4` + 声音解码 6.7ms；
- frame 1700/1812/1864：`preload` 17–18ms；
- 现有 `TextureStore::preload_parallel`（`core/src/texture.rs:1058`）与
  `engine.rs:1497` 的帧内预载，解码占耗时 >95%（`texture.rs:1320` 注释）。

### 4.2 设计方向（待实现时确认）

1. **预热提前**：在 `WAIT`/场景切换/加载画面期间预取下一场景资产，
   不等到角色入场那一帧；优先 `CG[FILE]` 声明链。
2. **固定工作集**：给资产缓存设上限 + 淘汰策略（当前 `evicted=0`，
   assets 265MB 单调涨）；按场景/最近使用保留，避免无界增长。
3. **预算分摊**：单帧 preload 设时间上限（如 4–6ms），超出部分排队到
   后续帧；入场帧不允许一次灌 33 个文件。
4. **线程**：沿用 `preload_parallel` 的 worker；确认与渲染线程的同步点
   不引入帧内等待。

### 4.3 涉及文件

- `crates/yu-render/core/src/texture.rs`（预算/淘汰/预热）
- `crates/yu-render/wgpu/src/lib.rs:254`、`soft` 对应实现
- `crates/yu-engine/src/engine.rs:572/1493`（预载入口与统计）
- 可能：`yu-gfx` 的资产声明/场景切换信号

### 4.4 验收

- koto 首次入场：`preload_ms` 单帧 ≤6ms，tick 尖峰从 107ms 降到 ≤70ms；
- 无 `asset.missing` 增加；资产缓存不超预算；画面像素不变；
- 安卓同场景帧时间/卡顿计数改善（与 T4 一起量）。

### 4.5 风险

- 预取错资产浪费内存/IO；需要场景提示或按声明链预测；
- 淘汰策略错误会导致重复解码，反而更慢；先只做“上限 + 不淘汰在场资产”；
- 预热线程与 GPU 上传的同步要避免帧内 `poll`。

---

## 5. T2 COPY2 快照批量 / 延迟物化

### 5.1 证据

- frame 974：`copy_bytes=21,059,840 / copy_count=262`，`render=15.4ms`（image 6.7ms）；
- frame 1149：`copy_bytes=8,052,000 / copy_count=61`，`work=114.6ms`（tick 98.4）；
- 菜单打开 tick 62–102ms；菜单按钮链 260+ 张快照（既有分析）。

### 5.2 设计方向

1. **延迟物化**：`COPY2` 先登记“快照请求”（源层 + 源版本 + 区域 + 目标），
   只有被屏幕绘制、存档读回或显式读取时才真正拷贝；未被读的快照不产生 GPU pass。
2. **命令时刻语义**：延迟必须配合**源版本固定**——登记时记源版本；
   若源在本帧后续被就地写（未来单表面）或复用，先执行拷贝再允许写。
   当前模型不就地写，但仍要保证 `COPY2→SAVE` 同 tick 读到登记时刻像素。
3. **批处理**：同一帧多个同源/同尺寸快照合并为一个 pass 或一次 buffer 拷贝；
   相同 `(源版本, 区域)` 去重，复用快照表面。
4. **表面复用**：快照目标仍走显式分配/池；与 T0 的池共用预算。

### 5.3 涉及文件

- `crates/yu-gfx/src/state.rs`（CopyOp 登记/生命周期）
- `crates/yu-render/wgpu/src/plan.rs`（COPY/COPY2 分支）
- `crates/yu-render/wgpu/src/textures.rs`（快照表面/缓存 key）
- `crates/yu-engine/src/capture.rs`（存档读回必须拿到登记时刻像素）

### 5.4 验收

- 菜单打开帧：`copy_count`/`copy_bytes` 明显下降（目标 ≥50%），tick p95 降 30%；
- `COPY2→SAVE` 同 tick 缩略图像素与官方样本一致；存档界面停留不出现 UI 截图；
- soft/wgpu 像素对拍通过；快照冻结语义回归通过。

### 5.5 风险

- 延迟物化最怕“源被改写后才拷”：必须版本固定或写前强制物化；
- 快照被存档/调试导出读取的时机不固定，需要统一的“读取时物化”入口；
- 批处理跨 pass 合并可能引入屏障/顺序问题，先做同帧同源合并。

---

## 6. T3 字形跨层复用

### 6.1 证据

- 菜单 170 字形首次烘焙 10.4ms（既有分析）；当前文本层按层缓存，
  跨层重复字形没有共享。

### 6.2 设计方向

- 全局字形图集：key = `(font_id, px_size, style, glyph, subpixel)`，
  跨层共享纹理与 UV；
- 每层只保留 draw list（layer → glyph instances），不再各自烘一张图；
- 图集分页 + 上限；淘汰只清“无层引用”的页；
- 文本基面 `raw` 语义不变（T0 后仍冻结）。

### 6.3 涉及文件

- `crates/yu-render/core/src/text.rs`（TextComposer/字形缓存）
- `crates/yu-render/wgpu/src/text_gpu.rs`（图集上传/绘制）
- `crates/yu-render/wgpu/src/plan.rs`（TextUploads 生命周期）

### 6.4 验收

- 菜单首开文本烘焙 ≤5ms（或降 ≥50%）；第二次打开接近 0；
- 文本像素与 soft 全等；图集显存有上限；层销毁后图集可回收。

### 6.5 风险

- 字体/字号/风格组合多，图集碎片化；需要分页；
- 与 T0 的池/资产守卫交互：图集是只读资产，不参与当前表面写。

---

## 7. T4 安卓测量与决策门

### 7.1 测量口径

- 构建：`cargo build -p yu-cli --release`（桌面）与 `host-android` release；
- 场景：标题循环 10 轮、首次入场、立绘差分、菜单打开、存档/读档；
- 指标：
  - 帧时间 p50/p95/p99/max、>33ms 卡顿帧占比；
  - `[res] created/reused/released/pooled/live/vram`；
  - `spike-log` 中 preload/copy/text_render 阶段耗时；
  - 设备侧 GPU 时间（Perfetto/AGI，若可用）；
  - 温度/降频（长跑 3 分钟）。
- 基线：T0 收口后先测一轮，作为对照；每完成 T1/T2/T3 各测一轮。

### 7.2 决策门

满足任一条件 → 转单表面计划书（P0/P3 spike）：

- 菜单/过渡 p95 仍 >33ms，或 >33ms 帧占比 >10%；
- 安卓最差帧仍由 `created/released` 分配抖动主导（GPU 计时证实）；
- `vram` 峰值随游玩时长持续上涨，池无法压住。

否则关闭表面话题，只保留池，不再做单表面重构。

### 7.3 交付物

- `docs/perf-android-<日期>.md`：设备、场景、原始 CSV/日志路径、前后对比；
- `PROGRESS.md` 追加一条，附验证命令与结果；
- 决策结论（继续/转单表面）写进该报告。

---

## 8. 提交与测试纪律

- 每个任务一个本地提交，提交信息按 `type(scope): 描述`（中文，不加句号）；
- 每个提交在 HEAD 上必须 `cargo build --workspace --all-targets` 通过；
- 每完成一项在 `PROGRESS.md` 追加“成果 N”（含命令与原始结果）；
- 不推送；不把 `docs/tasks/task-surface-single-model-plan.md` 与实现混在一个提交；
- 测试缺样本必须显式跳过并计数，禁止空跑通过。

---

## 9. 未决问题

1. T1 预取触发点：等场景切换信号，还是按 `CG[FILE]` 声明提前一帧？
2. T2 延迟物化的“读取时物化”入口是否覆盖所有存档/调试路径？
3. T3 图集分页大小与淘汰策略；
4. T4 安卓设备型号与 GPU 计时工具是否具备；
5. T0 删除 `rw` 后，`[res]` 诊断字段是否保留兼容别名（脚本/文档引用）。

---

## 10. 一句话

**先把已取证的三个尖峰打下去、把表面池收口，再上安卓量；量完仍不达标才动单表面。**
