# 跨平台能力矩阵(2026-11-13 更新)

> 上位文档:`docs/architecture.md` §2.5(v2 跨平台目标架构)。
> 本文件记录**实际落地**的可移植性状态,不是规划。

## 分层落位

```text
L0 内核(平台中立)      yu-rt / yu-gfx / yu-sound / yu-save / yu-format /
                        yu-vfs / yu-backend-soft / yu-backend-wgpu / yu-font
L1 能力端口             SaveFs(存档文件系统) · HostPaths(路径注入) ·
                        FontBytes(字体字节) · AudioOut(混音出口) · DataSource(数据树)
L2 唯一装配             yu-engine(Engine / EngineOptions / Project / boot / tick)
L3 宿主                 yu-cli(--mode desktop / headless) · host-android
                        · host-wasm(浏览器呈现端,调试链路)
```

## 能力矩阵

| 能力 | 桌面 | Android | 浏览器 wasm |
|---|---|---|---|
| 数据读取(ypf / 散盘) | 真实 fs | 真实 fs(app 私有 / 外置卡) | **待做**:需 `YpfSource::from_bytes` |
| 存档读 | 真实 fs | 真实 fs(app 私有目录) | **待做**:需 `SaveFs` 注入实现 |
| 存档写 | 真实 fs,字节级一致 | 真实 fs,字节级一致 | **待做**(同上) |
| 系统字体枚举 | `fontdb` 系统枚举 | `fontdb` 空操作 ⇒ 补 `/system/fonts` + `/product/fonts` | 无系统字体 |
| 宿主字体注入 | 可用 | 可用(`<data>/fonts/`) | **必需**(唯一来源) |
| 路径系统变量 | `USERPROFILE`/`HOME` | 宿主显式给(app 私有目录) | 宿主显式给 |
| 声音设备 | cpal | cpal(oboe 后端;真机 59.9fps 出声,欠载 0) | 暂不出声(架构决定) |
| 渲染 | soft / wgpu | 壳默认 wgpu(零拷贝 `ANativeWindow`;失败重试可切 soft) | 待做 |
| 输入 | winit → `yu-platform-input` | 触摸/双指滑/虚拟光标 → `MOUSE_L` / 滚轮 | DOM 事件 → 同层 |
| 影片 | MPEG-1 PS + MP2 音频(pl_mpeg 后端) | 同左 | 暂不支持 |
| 退出信号 | `Engine::lifecycle()` → 关窗 | 同一判据:`nativeTick()==1` ⇒ `finish()` | 同一判据(待接) |
| 挂起/恢复 | `suspend`/`resume`(窗口最小化) | `onPause`/`onResume` → `nativeSuspend/Resume` | 页面隐藏(待接) |
| 文本编码 | `--patch`(auto/original/gbk/**big5**/tunnel) | 配置界面选 + `run-config.json` 同键 | 同一份 `PatchKind` |
| `@_TIME` | 启动时注入系统本地时间 | 同(`host_clock::local_now`) | 恒确定性时钟(无本地时区 API) |

## 音频交付:为什么安卓要单独一层

桌面 cpal 回调是普通线程、缓冲大,阻塞几毫秒也不出事;安卓 oboe/AAudio 回调
跑在实时优先级、预算几毫秒,**等锁/分配/解码就是爆音**。因此安卓宿主的
交付层(`host-android/src/sound.rs`)与桌面不同构,但语义等价:

```text
mixer 线程(THREAD_PRIORITY_AUDIO)    oboe 回调(实时线程)
  lock(SoundState) → render_and_advance      pop(无锁 SPSC 环) → 设备
  → push(无锁 SPSC 环)                       不足补静音,绝不阻塞
```

三条硬约束,全部有实测依据(见 `DIV-AND-003`):

1. **回调零锁、零分配、零解码**(参考 `siglus_rs` 用的 kira 0.9:它的
   cpal 后端同样在回调里跑无锁 mixer,解码在独立 resource 线程)。
2. **必须有流监督者**:AAudio 在音频路由变化(插拔耳机/蓝牙)时**会断开并
   关闭流**,不重建就永久静音 —— 这是实测到的真 bug,kira 的
   `StreamManager` 正是干这个的。
3. **三类欠载分开计数**,否则无法归因:

   | 计数 | 含义 | 归属 |
   |---|---|---|
   | `clipped` | 混音叠加过量,硬削波 | 混音(音量结构) |
   | `rtsil` / `rtmiss` | 源解码窗口未就绪 | 预取(LEAD_MS/解码耗时) |
   | `short` | 环形缓冲被抽干 | 交付(线程调度/水位/宿主) |

   三者全 0 而仍有杂音 ⇒ 故障在 oboe 之后(设备驱动/模拟器转发)。

**模拟器音频不可作为听感判据**:同一份 APK/数据,模拟器有电流声与卡顿、真机
完全正常,而两边 `short`/`rtsil` 都是 0(读回耗时 2232~5588µs vs 556~865µs)。
听感判定只在真机做。

诊断开关(无需重编壳,写 `<游戏目录>/debug/run-config.json` 即可):

```json
{ "audio_tone": true }     // 旁路引擎,直接灌 1kHz 正弦(纯音也炸 ⇒ 设备侧)
{ "render": false }        // 跳过合成/上屏,只跑 VM+声音(归因渲染抢 CPU)
{ "audio_rate": 48000 }    // 强制输出采样率(默认 44100,与 OGG 源一致)
```

## 宿主参数:唯一取值表(见 DIV-AND-004)

平台差异只允许出现在 **端口实现**;「同一个参数的取值解析」全平台只有一份。
宿主只负责把值传进来,不再各自 match:

| 参数 | 唯一实现 | 覆盖入口 |
|---|---|---|
| `log_level` | `yu_log::LogLevel` | CLI `--log-level`、`YU_LOG`、内核 `MinLevel`、依赖门面桥、Android `run-config.json` |
| `backend` | `yu_engine::engine::Backend::parse` | CLI `--backend`、Android `backend` 键、host-desktop 示例探针 |
| `gpu_backend` | `yu_backend_wgpu::GpuBackend` | CLI `--wgpu-backend`、Android `gpu_backend` 键、`EngineOptions.gpu_backend` |
| `save_codec` | `yu_format::yssd::BlockCodec::parse` | CLI `--save-codec`、Android `save_codec` 键 |
| `script_key` | `yu_engine::keys::ScriptKey::parse_hex` | CLI `--script-key`、Android `script_key` 键 |
| `patch` | `yu_engine::patch::PatchKind::parse` | CLI `--patch`、Android `patch` 键、serve `--patch` |
| `on|off` 旗标 | `yu_cli::args::parse_on_off_flag` | headless / desktop / serve |
| `@_TIME` 本地时间 | `yu_engine::host_clock::local_now` | run / serve / desktop / Android;wasm 恒 `None` |

### 运行开关:CLI 旗标是正式入口,环境变量只是兼容别名

渲染/诊断开关(**预取、缓存预算、文本 GPU、PNG 后端、timing/profiling**)
以前只有 `YURIS_*` 环境变量一条路:桌面要 `set VAR=1`,Android 则在宿主里
`set_var` 转一手。现在统一成 `yu_render_core::RenderTuning` —— 与
`backend`/`gpu_backend` 一样是**引擎参数**:

```text
CLI 旗标 ─┐
          ├─→ RenderTuning ─→ EngineOptions.tuning ─→ 渲染/字体/gfx 只读字段
Android ──┘
JSON 键
```

- 解析只在宿主边界发生;渲染层(`yu-render-core` / `yu-backend-*` /
  `yu-gfx` / `yu-font`)不再读这些环境变量。
- CLI 布尔旗标:`--vm-profile` = on,`--vm-profile=off` = off;
  **不消费**下一个 token(位置参数不会被误吃)。
- 显式旗标 > 旧环境变量 > 默认值。环境变量只为不破坏既有 CI/脚本保留,
  文档与帮助以旗标为准。
- Android 的 JSON 键与 CLI 旗标同名,`RunConfig.tuning` 与
  `EngineOptions.tuning` 是同一个结构;宿主不再 `setenv`。

| JSON 键 / CLI 旗标 | 默认 | 旧环境变量 |
|---|---|---|
| `prefetch` / `--prefetch[=on\|off]` | off | `YURIS_PREFETCH` |
| `prefetch_mb` / `--prefetch-mb N` | 128 | `YURIS_PREFETCH_MB` |
| `prefetch_window` / `--prefetch-window N` | 2048 | `YURIS_PREFETCH_WINDOW` |
| `asset_cache_mb` / `--asset-cache-mb N` | 512(0 = 不限) | `YURIS_ASSET_CACHE_MB` |
| `asset_name_fallback` / `--asset-name-fallback[=on\|off]` | on | `YURIS_ASSET_NAME_FALLBACK` |
| `bilinear_scale` / `--bilinear-scale[=on\|off]` | off | —(Android 专属,桌面可开) |
| `snapshot_defer` / `--snapshot-defer[=on\|off]` | off | `YURIS_SNAPSHOT_DEFER` |
| `text_base_cache` / `--text-base-cache[=on\|off]` | on | `YURIS_TEXT_BASE_CACHE` |
| `text_gpu` / `--text-gpu[=on\|off]` | on | `YURIS_TEXT_GPU` |
| `pipeline_prewarm` / `--pipeline-prewarm[=on\|off]` | on | `YURIS_PIPELINE_PREWARM` |
| `png_backend` / `--png-backend image\|libpng` | image | `YURIS_PNG_BACKEND` |
| `png_cmp` / `--png-cmp[=on\|off]` | off | `YURIS_PNG_CMP` |
| `vm_profile` / `--vm-profile[=on\|off]` | off | `YURIS_VM_PROFILE` |
| `asset_timing` / `--asset-timing[=on\|off]` | off | `YURIS_ASSET_TIMING` |
| `tick_split` / `--tick-split[=on\|off]` | off | `YURIS_TICK_SPLIT` |
| `cg_timing` / `--cg-timing[=on\|off]` | off | `YURIS_CG_TIMING` |
| `readback_debug` / `--readback-debug[=on\|off]` | off | `YURIS_READBACK_WHY` |
| `trans_debug` / `--trans-debug[=on\|off]` | off | `YURIS_TRANS_DEBUG` |
| `text_gpu_debug` / `--text-gpu-debug[=on\|off]` | off | `YURIS_TEXT_GPU_DEBUG` |
| `render_timing` / `--render-timing[=on\|off]` | off | `YU_RENDER_TIMING` |
| `render_trace_geometry` / `--render-trace-geometry[=on\|off]` | off | `YU_RENDER_TRACE_GEOMETRY` |
| `p8_timing` / `--p8-timing[=on\|off]` | off | `YU_P8_TIMING` |
| `font_trace` / `--font-trace[=on\|off]` | off | `YU_FONT_TRACE` |
| `wgpu_backend` / `--wgpu-backend NAME` | auto | `YURIS_WGPU_BACKEND` |
| `wgpu_adapter` / `--wgpu-adapter NAME` | 空 | `YURIS_WGPU_ADAPTER` |

四条约束:

1. **新增宿主不改取值表**:加一个平台只实现端口,参数解析直接复用上表。
2. **新增取值只改一处**:词表与错误文案一起走,不会出现「CLI 收而 Android 拒」。
3. **枚举化边界**:Android `RunConfig` 的 `backend`/`gpu_backend`/`save_codec`/
   `log_level`/`tuning.png_backend` 是枚举而非 `String`,解析发生在 JSON 边界,
   下游无字符串判断。
4. **环境变量不是一等入口**:新增运行开关只允许加 CLI 旗标 + Android JSON 键
   (+ `RenderTuning` 字段);旧 `YURIS_*` 读取必须收敛到
   `yu_cli::args::HostTuning` 这一处兼容层,宿主与渲染层不得再 `setenv`。

## 事件与生命周期:宿主唯一接线面(见 DIV-AND-005)

内核只暴露**一个**退出信号面,任何宿主照抄三条规则:

```text
1. 判断"游戏是否结束"   → Engine::lifecycle()(Lifecycle::is_terminal)
                          Android 等价物:nativeTick() 返回 1 / nativeLifecycle() != 0
2. 挂起(页面隐藏/后台) → Engine::suspend()   丢墙钟 + 静音;幂等
3. 恢复                  → Engine::resume()    声音挂回;幂等
```

- 生命周期**必须在发布模式可见**:它是退出信号,不是诊断
  (Android `nativeStatus` 在 quiet 下也带 `status=exited … restart=N`)。
- `Lifecycle::Exited { restart }` 的 `restart` = 官方 `RESTART` 标志
  (官方进程收尾 `CreateProcessA` 重启自身);内核只报告,宿主决定是否
  重建会话(`Engine::restart`)。
- 存档**不需要**在挂起时 flush:SAVE 命令时刻已落盘,再写一次只会
  制造第二份不一致现场。
- `suspend` 的关键语义是**丢弃暂停期间的真实时间**:否则后台几分钟回来,
  第一帧把整段墙钟灌进 `@_OS_TIME`,淡出/补间全部瞬移(实测守护:
  `cargo test -p yu-cli --test lifecycle -- --ignored`)。

## 已收口的硬编码(见 DIV-AND-002)

| 原实现 | 现实现 |
|---|---|
| `SaveState` 内 8 处 `std::fs` 直连 | `SaveFs` 端口(`NativeSaveFs` 默认 = 旧行为) |
| `ensure_save_root` 的 `cfg(target_arch = "wasm32")` 分支 | `ensure_save_root_with(&dyn SaveFs)` |
| `host_paths` 硬编码 `USERPROFILE` + `\Documents` | `PlatformHome` + `HostPathOverrides` |
| `acm::build_pcm` 未按平台门控(非 Windows 编译中断) | `cfg(windows)` |
| 各宿主各写一份参数解析(含 Android `@_TIME` 漏接 ⇒ 恒 1970) | 上表「唯一取值表」 |

## 尚未端口化(有意保留)

| 位置 | 原因 |
|---|---|
| `yu-vfs::DirSource` / `YpfSource` 走 `std::fs` | 数据源**定义**就是文件系统/封包;浏览器目标由内存构造(`from_bytes` / `MemSource`),不是把 fs 抽象化 |
| `yu-font::FontSource::from_file` | 桌面/Android 专用;wasm 用 `from_bytes` |
| `yu-engine::read_save_path_type` / `detect_block_codec` | 读游戏根的 `yssfs.dat` / `YSSnp.dll` 信号;无 fs 的目标按缺失等价处理(值 = 0 / zlib),与官方"文件不存在"同义 |

## wasm 占位说明(本轮不实现)

浏览器目标需要三件事,均未做:

1. `yu-vfs::YpfSource::from_bytes(Vec<u8>, Codec)` —— 把 fetch 到的封包装成内存数据源;
2. `SaveFs` 的浏览器实现(OPFS / localStorage 同步桥),`SaveState::with_fs` 已留好注入口;
3. 字体字节注入路径(`EngineOptions::font_bytes` 已就绪,宿主侧需读文件选择器/内置字体)。

本轮**只预留**上述接缝(端口 + 注入口),不写浏览器实现。已有 wasm 分支的
实验改动独立存放,不在本分支。

## 验证入口

```bash
# 桌面全量
cargo test --workspace

# Android 宿主会话(真实语料,开发机可跑)
YURIS_SAMPLE_ERIS_ROOT=<project-eris> cargo test -p yu-host-android --test session -- --nocapture

# Android so 交叉编译
tools/android/build.sh arm64-v8a
```
