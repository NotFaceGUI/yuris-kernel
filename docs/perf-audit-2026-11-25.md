# 性能审计 · v555 接入后掉帧（2026-11-25）

> 范围：5 路子代理静态普查 + 本机 release 实测（帧统计 / 逐命令采样 / 长帧取证）。
> 真值层级与占位符见 `AGENTS.md`。证据产物在 `target/tmp/perf/`（不入库）。
>
> **状态：A 级（A1–A5）已实施并复测，见 `PROGRESS.md` 成果
> `PERF-V555-A`。** 实测：v555 正文 tick p50 1.435→0.968ms、
> 长帧 39→23 条、avg fps 56.0→57.4；0.495 主语料无回归。
> 其中 `YU_SP_TRACE` 取证块属工作树并行改动，**未随优化提交**；
> 推送代码不含该块。下文保留审计时的原始证据与行号（工作树随后有
> 并行改动，落地前需重 grep）。

---

## 0. 结论先行

1. **掉帧不是渲染后端退化**。稳态 p50 仍是 16.6ms（60Hz 满帧），
   问题集中在 **v555 按钮/菜单脚本的每帧 VM 工作量** 与
   **菜单开关帧的 `CGINFO`/`CGEND` 高频命令**。
2. 实测最大的长帧来源：`es_scenario.yst` 的主循环每帧调用
   `es.BT.BTLOOP` + `es.BT.DRAW`，一个 tick 执行 **25–43 万条 VM 命令**，
   耗时 **50–93ms**；`WAIT FRAME=1` 本身正常让出，工作量来自按钮宏。
3. v555 引入后新增了两类直接成本：
   - 工作树里新加的 **逐 `GOSUB` 环境变量查询 `YU_SP_TRACE`**（调试代码进了热路径）；
   - v555/0.500 文本的 **图像字形页整页深拷贝** 与 **逐字全量日志扫描**。
4. 先做「零语义风险」的一批（环境查询惰性化、INFO 事件门控、CGEND 快照集合缓存、
   空快照短路、逐字诊断门控），再评估文本 `Rc` 共享与 GOSUB/RETURN 微优化。

---

## 1. 实测基线（release，本机）

构建：`cargo build --release -p yu-cli`（审计时工作树，约 2026-11-24 22:2x）。

### 1.1 帧率口径

`yu-cli run <project> --mode desktop --frames N --frame-stats <csv>`；
“稳态”= 跳过前 5 帧的 start-to-start 间隔。

| 场景 | 帧数 | 稳态 p50 | p95 | max | avg fps |
|---|---:|---:|---:|---:|---:|
| `<project-eris>`（0.495 主语料） | 600 | 16.68ms | 17.05ms | 33.4ms | 59.9 |
| `<v555-1>` 标题流程（无输入） | 600 | 16.66ms | 17.08ms | 83.3ms | 59.5 |
| `<v555-1>` 正文（定时点击推进） | 1500 | 16.61ms | 17.13ms | 100.4ms | 56.7→54.5 |

结论：**稳态没有整体掉到 30/40fps**；用户感知的“帧数下降”更像
每秒 1–2 次的 50–100ms 卡顿（长帧把平均 fps 拉到 54–57）。

### 1.2 长帧取证（`--spike-log --spike-ms 20`）

39 条长帧中 **38 条**停在 `data\script\eris\es_scenario.yst@568`（`WAIT`），
实际执行的是上一组的 `es.BT.BTLOOP` / `es.BT.DRAW`（另 1 条为启动期路径）：

```text
host=933 work=89.9ms tick=86.9ms ops=428780 layers=164 passes=682
  cmds= LET=96980 IF=73515 IFEND=72564 LOOPEND=50659 INT=35350
        GOSUB=27005 RETURN=27005 VARINFO=18123
```

- 命令量 25–43 万/tick，tick 50–93ms；渲染 `render_ms` 只有 1.5–5.9ms。
- `WAIT FRAME=1` 的让出是正确的（主循环每帧只跑一轮）；成本在按钮宏内部。

### 1.3 逐命令耗时（`--profile-commands`，长 tick 采样）

| 命令 | 单价（实测） | 典型单帧次数 | 单帧成本 |
|---|---:|---:|---:|
| `LET` | 0.07–0.12µs | 9.6 万 | ~8–12ms |
| `IF` | 0.08–0.19µs | 7.4 万 | ~6–14ms |
| `GOSUB` | 0.10–0.61µs | 2.4–2.7 万 | ~3–5ms |
| `RETURN` | 0.10–0.32µs | 2.4 万 | ~2–8ms |
| `LOOPEND` | 0.03µs | 5.0 万 | ~1.5ms |
| **`CGINFO`** | **6.10µs** | **1360** | **8.3ms** |
| **`CGEND`** | **3.07µs** | **1401** | **4.3ms** |
| `FILEINFO` | 20.35µs | 50 | 1.0ms |

`GOSUB` 单价约为 `LET` 的 4–8 倍，`CGINFO`/`CGEND` 是普通命令的 25–80 倍。

### 1.4 渲染侧常规成本（非主因，但需知）

| 指标 | 720p 实测 | 说明 |
|---|---:|---|
| `readback_bytes` | 3.69MB/帧，每帧 | 1080p 约 8.3MB/帧 |
| `copy_bytes`（切场景帧） | 最高 26MB、71 次 | 整帧表面拷贝 |
| `uploads`（进场景帧） | 8–19MB | 资产上传，一次性 |
| `render_ms` | p50 0.70ms，max 5.9ms | 不是长帧主因 |
| `plan_ms` | p50 0.18ms，max 5.4ms | 同上 |

---

## 2. 已确认的浪费（按收益/风险排序）

### A 级 · 零语义风险，建议先做

#### A1. 逐 `GOSUB` / 逐 `CG` / 逐 op 的环境变量查询

| 位置（审计快照） | 现象 | 频率 |
|---|---|---|
| `crates/yu-rt/src/other/gosub.rs:87` | `if std::env::var_os("YU_SP_TRACE").is_some()`（**工作树新加的调试代码**） | 每次 GOSUB |
| `crates/yu-rt/src/vm/handlers/control.rs:43` | 同上（工作树新加） | 每次 GOSUB |
| `crates/yu-gfx/src/state.rs:3700/3753` | `YU_CG_FILE_TRACE` | 每条 `CG[FILE]` |
| `crates/yu-render/wgpu/src/passes.rs:224/284` | `YU_OP_TRACE` | **每个 op** |
| `crates/yu-render/wgpu/src/plan.rs:483/816/2565` | `YU_PASS_TRACE` | 每个纹理 pass / 屏幕 copy |
| `crates/yu-platform/host-desktop/src/lib.rs:3107/3317` | `YU_TEXT_TRACE` / `YU_FRAME_LAYERS` | 每帧 2 次 |

- Windows 上 `std::env::var_os` 走 `GetEnvironmentVariableW`（进程环境块锁 +
  `OsString` 分配），量级 0.3–2µs/次。
- 这可以直接解释 `GOSUB` 0.4–0.6µs 的单价：一次环境查询几乎就是全部成本。
  按单帧 2.4 万次 GOSUB 估算，仅此项约 **5–15ms**（视机器）。
- 修法：每个开关改成 `static OnceLock<bool>` / 模块级 `fn xxx_trace() -> bool`；
  `YU_SP_TRACE` 的调试代码保留功能但只查一次。
  > 注：`YU_SP_TRACE` 取证块属工作树并行改动，未随本次优化提交；
  > 推送代码不含该块，因此也没有它的逐 GOSUB 查询开销。
- 风险：零（开关语义不变；`=1` 时行为一致）。

#### A2. `op_info` 在未录制事件时仍构造 owned 事件

`crates/yu-rt/src/vm/handlers/media_info.rs:561-610`（审计快照）：

```rust
let mut targets: Vec<LValue> = Vec::new();
let mut event_args: Vec<ArgValue> = Vec::new();
let mut query_args: Vec<ArgValue> = Vec::new();
...
event_args.push(ArgValue::with_ref(...));      // LET 目标还要 read_var
event_args.push(ArgValue::new(slot, v.clone())); // 值克隆
...
self.emit_domain(domain, name.to_string(), event_args); // 无条件
```

- 桌面宿主 `record_events: false`（`crates/yu-cli/src/desktop.rs:215`），
  `Vm::emit` 对 Graphics/Window 域事件在同步宿主消费后**直接丢弃**；
  但 `op_info` 在此之前已经做了：3 个 `Vec` 分配、`name.to_string()`、
  每个参数一次 `Value` 克隆、`LET` 目标一次 `read_var`、一次事件入队/出队。
- 实测 `CGINFO` 6.1µs/次 × 1360 次/菜单帧 = **8.3ms**；其中很大一部分是上述事件构造。
- 修法：仅当 `self.record_events` 为真时构造 `event_args` 并 `emit_domain`；
  查询路径与 `write_target` 不变。
- 风险：低。录制开启（golden/用例）时逐字不变；关闭时事件本来就丢弃。

#### A3. `GfxState::frame()` 空快照也建全局索引

`crates/yu-gfx/src/state.rs:1574-1587`（审计快照）：

```rust
let emitted: FxHashSet<CopyId> = texture_ops.iter().filter_map(...).collect();
let by_id: HashMap<CopyId, &CopyOp> =
    self.copy_log.iter().map(|copy| (copy.id, copy)).collect(); // 无条件
for (id, snapshot) in &self.screen_snapshots { ... }
```

- `screen_snapshots` 为空时 `by_id` 完全没用，却每帧把 `copy_log` 全量收进
  `HashMap`（一轮菜单 2218 条，上限 16384）。
- 修法：把 `by_id` 移进 `if !self.screen_snapshots.is_empty()` 分支。
- 风险：零。

#### A4. `CGEND` 每次重建快照层集合

`crates/yu-gfx/src/state.rs:6289-6367`（审计快照）：

- 每个 `CGEND` 调 `drop_ops_for` → 若 `screen_snapshots` 非空，就把
  **全部快照的全部 `layer_ids`** 重新 collect 成 `HashSet`（注释自证历史
  “快照数十个 × 每份 1400+ 个 LayerId”）。
- 菜单解构帧实测 `CGEND` 1401 次 × 3.07µs = **4.3ms**；集合重建是其中一部分。
- 修法：给 `screen_snapshots` 加一个代次号（insert/retain 时 `+1`），
  缓存 `(代次, Rc<HashSet<LayerId>>)`；`drop_ops_for` 命中代次时只 clone `Rc`。
- 风险：低。集合内容只在快照表变更时变化；缓存键覆盖所有变更点即可。

#### A5. 逐字降级诊断未门控

`crates/yu-gfx/src/state.rs:5325-5375`（审计快照）：

```rust
self.push_text_draw(t, draw.clone());   // 整条 TextDraw 再深拷贝
if let Some(image) = &draw.image {
    // 注释写 `YU_GLYPH_TRACE=1` 才打印，但代码里没有 env 判断
    self.push_diag("CGACT", error::verbose(..., format!(
        "外字链降级: {} -> {} asset={} cell={:?} {}x{} tint={:?} at ({},{})", ...)));
}
```

- 每个图像字形（v555/0.500 逐字 MERGE 降级）一次 `format!` + 诊断构造；
  中文补丁作品的正文每帧几十字时是实打实的每字成本。
- 修法：`static YU_GLYPH_TRACE: OnceLock<bool>`，关闭时整段跳过；
  同时去掉 `draw.clone()`（先发诊断再 move，或诊断只用借用）。
- 风险：低（只是把临时取证恢复为“开关才开”）。

---

### B 级 · 低风险，但改动面更大

#### B1. 文本图像字形：每次文本变化深拷贝整张字形页

`crates/yu-render/wgpu/src/plan.rs:3342-3358`、`crates/yu-render/soft/src/render.rs:1625`：

```rust
if let Ok(texture) = textures.resolve(&image.asset) {
    images.insert(image.asset.clone(), texture.clone()); // 整页 RGBA 深拷贝
}
```

- `Texture` = `{ width, height, rgba: Vec<u8> }`；1024×1024 字形页 = **4MB/次**。
- wgpu 在 **compose 缓存判定之前**就构建 `images`；soft 更是无条件构建，
  缓存命中也照付。v555/0.500 的图像字形正文逐字变化 ⇒ 每字一次。
- 修法：改用 `TextureStore::resolve_shared`（已存在，返回 `Rc<Texture>`），
  把 core `TextComposer` 的 `images: &BTreeMap<Vec<u8>, Texture>` 统一改成
  `&BTreeMap<Vec<u8>, Rc<Texture>>`；soft 把 `images` 构建移到缓存命中判断之后。
- 风险：低（只读像素；类型机械替换）。需同步改 core 单测里的 `assets` map。

#### B2. `text_draws_now()` 每次追加字形都全量深拷贝

`crates/yu-gfx/src/state.rs:1852-1867`、`5148-5176`（审计快照）：

- `push_text_draw` 每追加一条就 `bump_text_draws()`；`text_draws_now()`
  缓存失效后把**所有文本层所有字形** `cloned()` 成新 `Vec`。
- 逐字输出时长页退化 O(N²)（第 N 个字要克隆 N 份）。
- 修法：`TextDraw` 改 `Arc<TextDraw>`，或按层存 `Arc<[TextDraw]>` 增量更新。
- 风险：中（`TextDraw` 消费点较多，需对照 golden）。

#### B3. `image_log` 只增不删 + `frame()` 多遍全量扫描

`crates/yu-gfx/src/state.rs:1437-1562`、`1763-1770`、`1995-2041`（审计快照）：

- `frame()` 每帧对 `copy_log`/`image_log` 做 6–7 遍扫描，再加一次
  `screen_contract_key` 全量哈希；日志上限 16384。
- MERGE/区域填充等不可折叠 op 只增不删，长会话 `frame()` 成本线性上涨
  （历史注释已记录过“passes 128→139、render 30.8→40.8ms”同型问题）。
- 修法（保守顺序）：
  1. `by_id` 按需（A3）；
  2. `screen_contract_key` 的日志部分按“日志代次”缓存，或把日志哈希做成增量；
  3. 帧末压缩“已烘焙且无后续读者”的日志项（需逐案核对语义）。
- 风险：中（发射语义敏感，建议先加计数观测）。

#### B4. `StoreSampler` 的 `TextureStore` 无帧边界/无预算

`crates/yu-engine/src/engine.rs:3167-3184`、`3389-3394`（审计快照）：

- `CGINFO[ONMOUSE/COLOR]` 采样面持有第三份 `TextureStore`，从不
  `begin_frame()`/`set_budget_bytes()`：采样过的资产像素永久驻留，
  失败条目没有上限，且首次命中会与主渲染器各解码一份。
- 对照：抓帧器 `capture.rs` 显式做了 `begin_frame()`，此处是遗漏。
- 修法：给 `TextureSampler` trait 加默认 no-op 的 `begin_frame(frame)`，
  `GfxHost` 在 `VmEvent::FrameStart` 调到；或由引擎每 tick 直接调采样面。
- 风险：低（预算/失败上限是既有机制；需确认默认预算数值）。

#### B5. 音频：预解码与实时回调共用一把锁；锁毒化策略不一致

- `crates/yu-sound/src/host.rs:54/99/112/125/171/180` 全部 `.lock().unwrap()`：
  音频线程 panic 后锁毒化 → 下一次 tick 的 `advance` 直接 panic，
  与 `engine.rs:1691-1697` 声称的“毒化降级”矛盾。
- `cursor: SoundState::advance` 持锁做 OGG/Opus 预解码（`LEAD_MS=1500`），
  桌面 cpal 回调每块要同一把锁 ⇒ 回调可能被解码阻塞数十 ms（爆音/断音）。
- 实测 `sound_decode_ms` 最高 **7.85ms**、`sound_stream_ms` 7.23ms，
  且 78% 的帧非零（小幅）。
- 修法：① 6 处 `unwrap` 改 `unwrap_or_else(PoisonError::into_inner)`；
  ② 中期把解码结果与播放位置拆锁（SPSC/无锁窗口），或把预解码拆成每帧小步。
- 风险：①零；②中（涉及音频时序）。

#### B6. 输入时间线无界

`crates/yu-rt/src/input.rs:329-341`、`engine.rs:2290-2298`（审计快照）：

- `InputTimeline.frames: BTreeMap<u64, Rc<InputFrame>>` 每帧一条、只增不删，
  桌面每次 `CursorMoved` 都会写。
- 修法：只保留最近 N 帧（如 120）滑动窗口；`effective_ref` 只需最近一条。
- 风险：低。

#### B7. 影片帧整帧 `to_vec`

`crates/yu-engine/src/engine.rs:2421-2426`（审计快照）：

- 播放期每帧 `frame.rgba.to_vec()`（1080p = 8.3MB）+ 新建 `Rc<Texture>`。
- 修法：`MovieFrame` 直接移交 `Vec`/`Arc<[u8]>`，`insert_rgba` 接管所有权。
- 风险：低。播片时收益明显。

---

### C 级 · 结构性优化（需先测量/设计）

| 项 | 位置 | 说明 |
|---|---|---|
| 每帧整屏 `screen → screen_surface` 拷贝 | `wgpu/src/lib.rs:869-873` | 3.7MB/帧（1080p 8.3MB）；可考虑双缓冲渲染目标，避免 copy |
| 读回兜底整屏 `Surface::clone()` | `wgpu/src/readback.rs:304-308`、`lib.rs:962-966` | 改 `Arc<Surface>` 共享可省一次整帧 memcpy |
| `CGEND` 批量化 | `yu-gfx/src/state.rs:6289-6367` | 把同一 tick 的 `removed` 累积后统一 `drop_ops_for_batch`，从 O(CGEND×image_log) 降到 O(1) 次 |
| `live_snapshots` 图遍历 | `wgpu/src/passes.rs:1393-1449` | 每帧 `|快照| × |op|`；先建 `CopyId→CopyOp` 索引再遍历 |
| `transition_mask_transform` 全扫日志 | `yu-gfx/src/state.rs:6949-6963` | 相邻 `mask_base` 有帧内缓存，这一项漏了；加同型缓存即可 |
| 声音预解码拆锁 | `yu-sound/src/state.rs:831-850` | 与 B5 同源，需设计 |
| boot 期 5 份 YPF/VFS | `yu-engine/src/engine.rs:936/3214/3383/3391/3410` | 索引解析 5 次、资产最多 4 份独立解码；共享 `Arc<YpfIndex>` 可省 boot 与首次解码 |
| `CGINFO[SX/SY]` 压缩条目整条解压 | `yu-vfs/src/ypf.rs:997` | 只为读 4KB 头却整条 inflate；可做前缀 inflate |
| 存档 `refresh_slots` 重读全部 `.sd` | `yu-save/src/state.rs:641` | 每次保存重读 100 个槽；可只重解析刚写的文件 |
| `--pump-cap` 限制每泵命令 | 已有诊断开关 | **不建议默认开启**：会破坏“帧原子”脚本语义（DIV-HOST-002） |

---

## 3. 误报 / 不要动的部分（子代理已核实）

- `SemanticProfile::is_v555_family()` 只是 `u32` 比较，启动派生一次；每条命令一次也仅 ~1ns。
- 图像扩展名探测（YDG/WebP/JXL/QOI/AVIF）：失败结果有缓存（`MAX_FAILED_CACHE_ENTRIES=4096`），
  每资产最多一次 10 个哈希查表，不是每帧。
- `PRODUCT[COM=34]` 磁盘剩余空间：boot 一次注入，查询只读字段，不是每帧 syscall。
- 语音/纹理上传：`TextureStore` + `TextureCache` 双层缓存，同尺寸换内容用
  `write_texture` 就地覆写；不是每帧重解码/重上传。
- 字体/glyph：`GlyphCache` 4096 条/8MP LRU；文本合成按修订号 + 前缀增量烘焙。
- egui：面板隐藏时只做游戏帧 blit（历史已优化）。
- 主循环节拍：`WaitUntil` + 1ms 定时器 + 0.8ms 提前量，p50 16.65ms 已达 60Hz。
- `yu-emote/` 当前不在 workspace members、引擎零引用，E-mote 为 `deferred(master)`，
  对当前帧率无影响。

---

## 4. 建议的落地顺序（供审阅）

### 第一批（零风险，先做先测）

1. A1 环境变量查询惰性化（含工作树 `YU_SP_TRACE`）。
2. A2 `op_info` 事件门控。
3. A3 `frame()` 的 `by_id` 按需。
4. A4 `CGEND` 快照层集合按代次缓存。
5. A5 逐字降级诊断门控。

预期：菜单开关帧的 `CGINFO`/`CGEND` 成本明显下降；
按钮宏长帧中的 `GOSUB` 成本下降。语义不变，可用现有 golden 全量回归。

### 第二批（低风险，改动面中等）

6. B1 文本字形页 `Rc` 共享（wgpu + soft + core 签名统一）。
7. B4 `StoreSampler` 加帧边界/预算。
8. B5 声音锁毒化恢复（只改 `unwrap`，先不动拆锁）。
9. B6 输入时间线窗口淘汰。
10. B7 影片帧所有权移交（可选）。

### 第三批（先加观测再动）

11. B3 `image_log` 压缩 / `screen_contract_key` 增量哈希。
12. C 级：`CGEND` 批量化、`live_snapshots` 索引、整屏拷贝/读回架构。
13. GOSUB/RETURN 微优化（在 A1 去掉环境查询后重新测单价，再决定是否值得做）。

---

## 5. 复测与验收口径

### 5.1 复测命令

```bash
export TEMP="D:/Art/ai/projects/YurisDroid/workbench/tmp" TMP="$TEMP"
cargo build --release -p yu-cli

# 0.495 主语料（回归）
target/release/yu-cli.exe run <project-eris> --mode desktop --frames 600 \
  --frame-stats target/tmp/perf/after-eris.csv

# v555 标题 + 定时点击推进（对比 56.7fps/最大 100ms）
target/release/yu-cli.exe run <games>/<v555-1> --mode desktop \
  --script target/tmp/perf/v555-advance.json \
  --frame-stats target/tmp/perf/after-v555.csv \
  --spike-log target/tmp/perf/after-spike.log --spike-ms 20 \
  --profile-commands
```

### 5.2 验收指标

- 稳态 p50 不超过 16.7ms（不应为优化牺牲节拍）。
- v555 正文 avg fps ≥ 58（当前 54.5–56.7），长帧 >20ms 数量减半。
- 菜单开关帧 `CGINFO` 单价 < 2µs、`CGEND` 单价 < 2µs。
- `GOSUB` 单价在去掉环境查询后应接近 `RETURN` 量级。
- 像素门禁：`yu-cli case --all` 全绿；wgpu/soft 哈希对拍不变。

### 5.3 建议先加的观测（不改行为）

- `frame()`：`copy_log.len()` / `image_log.len()` / `screen_contract_key` 耗时。
- `prune_copy_log`：快速返回命中 / 完整 prune 次数。
- 事件队列：`record_events=false` 时 `op_info` 事件条数（验证 A2 收益）。
- 长会话资源：`--res-probe` 的 `snap=` / `created/reused` 曲线。

---

## 6. 附：子代理分工与主要证据锚点

| 路 | 范围 | 最强证据 |
|---|---|---|
| 1 | 帧循环热路径 | `state.rs:1763` 每帧 contract key；`state.rs:1437-1562` 多遍日志扫描；`plan.rs:3342` 整页深拷贝；`passes.rs:284` 每 op env |
| 2 | 锁/泄漏/毒化 | `sound/host.rs:125` 毒化 unwrap；`engine.rs:3167/3391` StoreSampler 无预算；`input.rs:329` 无界时间线；`texture.rs:682` O(n²) 预算回收 |
| 3 | v555 新增路径 | `state.rs:5325-5375` 逐字深拷贝+无门控诊断；`state.rs:1852-1867` text_draws_now O(N²)；`product.rs:274` 每行 format! |
| 4 | I/O 与缓存 | `ypf.rs:997` 压缩前缀整条解压；`engine.rs` 5 份 VFS；音频每次整读；`texture.rs:804` 失败 churn |
| 5 | 渲染/文本/音频 | `plan.rs:3342` 深拷贝；`passes.rs` env；`readback.rs:304` 整屏 clone；`sound/state.rs:831` 解码锁 |

> 本报告为静态审计 + 本机实测的合并结论；A 级项均有明确代码锚点，
> B/C 级项的收益量级来自仓库自证规模与静态推算，落地后以 `--frame-stats`
> 复测为准。
