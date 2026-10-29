# DIV-GFX-067 · 资产解码并行(非官方机制)

> 状态:`open`(实现层分歧;可观测像素/时序语义未变)。
> 范围:`yu-render-core::TextureStore::preload_parallel` + 引擎 tick 末尾预热。
> 相关:`PROGRESS.md`「成果 PERF-ASSET-PRELOAD」、`docs/perf-spike-report-2026-10-27.md` §14。

## 1. 官方线程普查(REA,0.495 sha256 `4f5eefa3…`)

| 事实 | 锚点 |
|---|---|
| 只导入 `KERNEL32.DLL::CreateThread`;无 `QueueUserWorkItem` / `CreateThreadpool` / `_beginthread` | `search_procedures` |
| `CreateThread` 仅两处调用,均为 CRT 风格线程包装(分配 0x74 上下文、挂起创建、`ResumeThread`) | `0x46bb30`、`0x46bdd1` |
| 包装的唯一实际调用方 = 按 CPU 数建线程池,每个线程 `SetThreadIdealProcessor`,入口 `0x4254d4` | `0x463f50`(调用方 `0x46164c`,WinMain 初始化) |
| 池任务表全是**软件合成**:逐行 `memcpy` + 每像素 alpha/色键混合(`pavgb`/`paddusw`)、缩放、通道混合 | `0x4265cc`、`0x425e08`、`0x425a08`、`0x425680`、`0x4255ec` 等 |
| 单独一条线程入口先 `CoInitializeEx`,再调 `0x464924` 等影片/COM 路径 | `0x45f3f4` → `0x422268` |
| 另一条线程由开关 `DAT_008015fc` 控制 | `0x4640c4` → `0x42543c` |
| `CG[FILE]` 在**命令处理体内同步解码**(不投递线程池) | `0x42a498`(v494)/`0x42a514`(0.495)→ `0x42c2a8` → `0x40c7c9` → `0x40c9b4` |

结论:官方没有并行资产解码;并行只用于软件合成与影片播放。

## 2. 内核行为

- `GfxState::apply_cg` 在 `CG[FILE]` 命令时刻登记资产 key(官方同帧语义),
  引擎在**同一 tick 末尾**调用 `Renderer::preload_assets`。
- `TextureStore::preload_parallel`:
  1. 主线程顺序读 VFS(读取端不进工作线程);
  2. 按资产切块,`std::thread::scope` 并行 `image::load_from_memory`;
  3. 结果**按输入顺序**写回缓存;每项独立 `catch_unwind`,线程 panic 记
     `[asset] preload worker panic`,该批退化为后续懒解码;
  4. 线程数 = `available_parallelism - 1`,钳制 `1..=4`(留一核给主线程/音频)。
- 上传仍在主线程(`ensure_asset`),顺序与命令登记序一致。
- 预热**只由 `FILE` 触发**:`CASH` 仍未分类,单独出现不会登记资产;
  图层组/类型冲突分支提前返回后也不登记(`yu-gfx --test preload` 覆盖)。
- **时机差异**:官方在 `CG[FILE]` 处理体内同步解码;本内核在**同一 tick
  末尾**批量解码/上传。像素可用性不变(`CGINFO[SX/SY]` 走资产头探测;
  `SAVE[ID]` 走 `DIV-GFX-068` 的命令时刻抓帧),差异只在解码墙钟归属
  ——仍计入同一帧的 `tick/preload`,不会漂到下一帧。要严格对齐需给
  gfx 注入同步装载端口(跨层 I/O),暂无行为收益,保持现状并留档。

## 3. 为什么保留

同一批次 4 张 1332×3351(17.85MB/张)PNG:

- 串行解码 57.5ms;并行(4 线程)21–24ms;VFS 读包仅 ~0.6ms。
- 解码发生在 `CG[FILE]` 所在帧(与官方一致),不会拖到角色真正出场帧;
  并行只缩短该帧墙钟。
- 像素结果与串行逐字节一致(解码纯函数、写回有序),已有回归
  `parallel_preload_matches_sequential_and_caches`。

## 4. 风险与收敛

| 风险 | 处置 |
|---|---|
| 抢占主线程/音频 CPU | 线程数留一核,上限 4;解码只在有 `CG[FILE]` 的 tick 发生 |
| 瞬时内存(压缩字节 + 解码位图) | 与串行同量级;位图最终都要常驻纹理缓存 |
| 结果不确定 | 写回按输入序;解码不触碰 VM/gfx 状态 |
| 工作线程 panic | `catch_unwind` + 错误日志;失败项留给懒解码路径并报 `asset.*` |
| 与官方机制不一致 | 本档案;若后续要求严格对齐,把线程数固定为 1 即退回串行,无接口变更 |

## 5. 回归

- `cargo test -p yu-render-core`(11 项,含并行/串行像素一致、重复 key 去重)
- `cargo test -p yu-gfx --test preload`(仅有效图像层登记、取走不重发)
- `cargo test -p yu-engine --test capture_snapshot`(晚一帧 SAVE 端到端)
- `<game-b>` 1200 帧:`decode` 最大 0ms、`render p95` 2.06–3.39ms;
  e1014 `work 167.8→51.7ms`、`capture 76.1→0`。
