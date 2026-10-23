# 性能报告 · 交互卡顿与启动停顿(2026-10-23)

> 范围:桌面宿主固定节拍、egui 管线、整帧拷贝、`serve` 诊断门控、VM 热路径。
> 语料:`<project-eris>`(data.ypf,官方 0.495 编译器全量编译)。
> 真值层级与占位符见 `AGENTS.md`。证据产物全部在 `target/tmp/perf/`(不入库)。
>
> 结论先行:600 帧稳态从 **38.6–40.9fps / 24.5ms 帧时间** 提升到
> **60.0fps / avg 16.665ms(排除 2 次场景事件尖峰;含尖峰 58.9–59.1fps)**;首帧脚本执行从
> **1.97s 降到 0.37–0.55s**,設定按钮点击的实际“输入→画面更新”从
> **~0.5s 级降到 ~0.27–0.28s(仍有一次 ~0.26–0.27s 的场景装载突发)**。

## 0. 环境与口径

- Windows 10.0.26300 x64,NVIDIA RTX 5070,release 构建。
- 桌面宿主:`yu-desktop <project-eris> --frames 600`;`--frame-stats` 逐帧 CSV。
- 每次运行清空 `--log-file`,首次运行前无其它 GPU/CPU 压测。
- “稳态”= 跳过前 5 帧(含首帧场景装载)的 start-to-start 间隔。
- 输入延迟 = 宿主收到输入事件的时间戳 → 含该输入的帧完成 `present`。
- 样本路径与编译环境见 `docs/symptom-ledger.md` §5.0;缺样本的用例显式报错。

## 1. 基线(修复前,600 帧)

```text
$ ./target/release/yu-desktop.exe <project-eris> --frames 600
yu-desktop: frames=600 elapsed=14.68s avg_fps=40.9 backend=wgpu viewport=800x600
frame 1: host_tick=1969.2ms engine_tick=1950.2ms engine_render=10.5ms draw=1.1ms
frame 2: host_tick=211.5ms  engine_tick=204.7ms  engine_render=4.9ms  draw=0.5ms
frame 4: host_tick=1858.7ms engine_tick=1848.1ms engine_render=1.2ms  draw=0.5ms
frame 600: host_tick=4.8ms engine_tick=3.3ms engine_render=1.0ms draw=0.3ms
```

- 稳态工作量只有 ~5ms,但帧间隔 ~24.5ms:不是 VM/渲染,是**等待粒度**。
- `--fps 1000` 仍只有 42.6fps ⇒ 排除 present/vsync 主导,锁定
  `ControlFlow::WaitUntil` 被 Windows 默认 15.6ms 定时器粒度放大。
- 首帧 1.95s、第 4 帧 1.85s:同一份启动脚本在内核里执行了 ~150 万条命令。

## 2. 已实施修复(按影响排序)

### 2.1 固定节拍 + 1ms 定时器(P0-1)

- `host-desktop/src/lib.rs::schedule_next_frame`:落后时对齐到下一个未来槽位,
  不再 `now + frame`(那会把工作与睡眠串行)。
- `timer_resolution`:Windows `timeBeginPeriod(1)` / `timeEndPeriod(1)`(RAII)。
- `wait_target()`:提前 0.8ms 唤醒,抵消 `WaitUntil` 的 ~0.3ms 晚到。

效果(600 帧,`--frame-stats target/tmp/perf/ego-off.csv`):

```text
frame pacing (steady, skip first 5): n=595 avg=16.92–16.97ms p50=16.65ms
                                     p95=17.04–17.09ms max=150.06–150.50ms
                                     => 58.9–59.1fps
frame pacing (first 5 intervals): 194.3–264.8, 382.0–561.5, 83.1–116.9,
                                  16.4–16.6, 400.4–550.1 ms
```

最终 release 复核(run 与 run 之间有 ±0.3ms 抖动;p50 均 16.65ms,
排除场景尖峰后 avg=16.665ms = 60.0fps):

```text
$ yu-desktop <project-eris> --frames 600 --shot target/tmp/perf/final2.png
frame 1: engine_tick=366.1ms  frame 2: 75.7ms  frame 4: 381.6ms
frame 600: host_tick=4.3ms engine_tick=3.3ms engine_render=0.8ms draw=0.3ms
frame pacing: avg=16.92ms p50=16.65ms p95=17.09ms => 59.1fps
# 排除 2 次 >30ms 的场景事件尖峰(frame 12=33.5ms / frame 217=150.3ms):
# avg=16.665ms => 60.0fps(节拍本身已达 60Hz)
shot sha256 a6db617b…(与 P1 台账 §5.4 一致)
```

### 2.2 egui 惰性化(P0-2)

- 面板隐藏、无冻结、无卡住横幅时只做游戏帧 blit;跳过
  `run_pass / tessellate / update_texture / update_buffers / egui pass`。
- 隐藏期间仍排空 `egui_winit` 事件缓冲(否则隐藏数小时会无界增长,
  重新显示时回放陈旧点击)。

对照(`--debug-ui` 前后各 600 帧):

```text
egui off: frame 600 draw=0.3ms
egui on : frame 600 draw=0.7ms
shot sha256(off) = a6db617b896dfb8a2f478238c3ef190fe4b0e5e4e825bb19771c8a940d05fc72
shot sha256(on)  = a6db617b896dfb8a2f478238c3ef190fe4b0e5e4e825bb19771c8a940d05fc72
```

⇒ 每帧省 ~0.4ms,游戏像素 0 差异。稳态 58.7fps(egui 开)vs 59.1fps(关),
差异被 60Hz vblank 量化吸收。

> 说明:F1 打开后同样进入 `show_debug=true` 路径;本轮用 `--debug-ui`
> 常开做稳态对照(脚本 runner 目前没有“按宿主 F1”动作)。
> 另修了一个惰性化边界:面板隐藏后不再让 `egui_ctx` 的旧“指针在面板上”
> 状态吞掉游戏输入,只有面板/冻结真正可见时才接受 egui 的输入消费。

### 2.3 消除整帧拷贝(P0-3)

- `RenderOutput.surface` / `WgpuRenderOutput.surface` / `RenderedFrame.surface` /
  `Engine::last_surface` / 宿主 `last_surface` 改为 `Arc<Surface>` 共享。
- 软后端 `last_screen`、wgpu 后端 `backbuffer` 与返回值共享同一份像素。
- 每帧省掉 2 次 800×600×4 ≈ 1.9MB 的 `Vec` 拷贝(engine 一次、renderer 一次)。
- 观测:`engine_render` 1.0–1.6ms → 0.6–0.9ms(其中大部分是 GPU 读回;
  拷贝已不再是独立项)。`case --all` 8/8 全绿。

### 2.4 `serve` 诊断门控(P0-4)

- 无 `/diag` 订阅者:每帧只低成本排空诊断,不构造 `DiagReport`/JSON。
- 有订阅者:最多每 500ms 构造一次并推送(与 www 消费端节拍一致)。
- 证据:无客户端时 `/diag` 快照保持初始 `{"ok":false}`(40s 未变);
  接上客户端后第三帧收到真实快照(frame=1618, waiting_tasks…)。

### 2.5 VM 启动/场景切换热路径(P0-6 取证后修复)

`--profile-frames` 采样显示首帧 **81.8% 的时间在 `VARACT`**
(28,916 次,平均 ~48µs/次),不是 I/O、不是文本链路。

- 根因:`VARACT[PUSH]` 的 `shift_1d` 把整个数组读成 `Vec<Value>`、
  `clone()` 一份、再逐元素 `store.write`。启动脚本对 169 个按钮逐条
  `PUSH` 到 8192 槽数组,复杂度 O(n²) 且字符串元素每次复制。
- 修复:`VariableStore::shift_1d` 用切片 `rotate_right/left` 原地搬运,
  长度/零值填充语义不变。

同一首帧的逐命令耗时(采样窗口,`--profile-frames 1,4`):

```text
修复前 frame 1: total 1775.6ms   VARACT 1453.2ms(81.8%, 50.3µs/op)
修复后 frame 1: total  329.8ms   LET 160.3ms(48.6%, 0.29µs/op)
                                 CG 55.1ms / IF 40.4ms / VARACT 11.0ms(3.3%)
```

其余热路径优化(同样语义不变):

- `step_command` 用 `remove_entry` 取回原键,去掉每命令一次 `TaskId` 的
  `String` 分配;执行位表 `BTreeMap` → `FxHashMap`。
- 命令缓存从 `FxHashMap<(script,pc)>` 改为“脚本号 → pc” 的稠密 `Vec` 下标
  (50 万级命令/帧时省一次哈希);
- 表达式窗口在 `fill_cmd` 时预解码;单条字面量窗口做常量折叠
  (`CmdWindow.const_value`);LET/IF 不再每次哈希查解码缓存;
- 设定打开 tick:410–480 → **264–270ms**(-35~45%);LET 0.26–0.29 → 0.211µs,
  IF 0.27–0.30 → 0.227µs。
- 诊断定位不再逐命令拷贝命令名:保留取指缓存的 `Rc<Cmd>`;
  任务名只在任务切换(按 `seq`)时重写。
- `VARINFO SEARCH` 直接扫槽内数据,不再逐元素 `read`(槽查找 + `Value`
  clone);20,085 次查询从 223.8ms 降到从 top-10 消失。
- `VARACT[INIT]` 的 ysv 投影在启动期做一次;同一掩码且 `VariableStore`
  无写入时跳过重复恢复(写入用 generation 计数检测)。

### 2.6 首帧/启动时间线

```text
$ YU_LOG=info yu-desktop <project-eris> --frames 2
startup: vm boot (tables/entry/script) = 45.9ms
startup: boot rest (gfx/sound/save)     = 50.5ms
startup: boot=98.9ms renderer=570.3ms text_warm=143.9ms total=669.3ms
         (窗口创建前;renderer 含 text_warm)
```

- 窗口出现后首帧脚本:1.97s → **0.37–0.55s**(取决于文件缓存/上下文);
  第二/第三帧 76–106ms / 5ms;第 4 帧场景切换 1.85s → **0.38–0.60s**。
- 首个可播放画面 ≈ **0.67s(启动,含字体预热)+ 0.37–0.55s(首帧)≈ 1.04–1.22s**;
  修复前 ≈ 2.5s。预热用 +144ms 启动换掉第一次文本页签的 130ms 卡顿。

## 3. 交互延迟(实测)

```text
# 多轮 450 帧 + cases/host/symptom-1-title-settings-back.json(真实 OS 光标也计入;
# 光标/鼠标与 script 样本来自不同轮次)
input latency [cursor]: n=81 avg=11.69ms p95=16.52ms max=17.26ms
input latency [mouse] : n=2  avg=15.58ms p95=15.70ms max=15.70ms(早前一轮)
input latency [script]: n=4  avg=92.45ms p95=283.86ms max=283.86ms
  | frames avg=1.00 p95=1 max=1
```

- 普通输入(光标/鼠标)平均 ~11.7–15.6ms、p95 ≤17ms:满足“≤1 帧 + 渲染”的目标。
- **逻辑帧差恒为 1**:宿主在事件后第一次 tick 消费输入(`frames avg=1.00,
  p95=1, max=1`);wall 时间的差异完全来自该帧的脚本/渲染工作量
  (設定场景那帧 410ms)。这比“平均毫秒”更能区分“调度慢”与“这一帧活多”。
- `script` 的 p95 = 点击「設定」后那次**场景装载**:
  `es_button` 在新场景里重跑约 100 万条命令,单个 tick 264–270ms(未采样)。
  这是当前剩余的最大交互卡顿。

### 3.1 設定画面:打开与页签切换(逐帧采样窗口)

> 采样口径已修正:每次打印前清零,读数是**单帧**,不是“自上次打印以来”。
> 之前把累计窗口误读成單帧,导致“設定点击 1.8s”的高估;以下是修正值。

打开設定(host frame 300;`es_button` 菜单构造):

```text
sampled total 220.4ms(实测 tick 264–270ms)
LET      106.3ms (48.2%) hits=503,912 avg=0.211us/op
IF        37.5ms (17.0%) hits=165,052 avg=0.227us/op
CG        33.0ms (15.0%) hits= 23,837 avg=1.382us/op
LOOPEND   15.9ms ( 7.2%) hits=321,676 avg=0.049us/op
INT        6.2ms ( 2.8%) hits= 25,810 avg=0.240us/op
IFEND      4.6ms ( 2.1%) hits=163,106 avg=0.028us/op
GOSUB      3.8ms ( 1.7%) hits=  8,544 avg=0.448us/op
VARACT     3.4ms ( 1.6%) hits= 14,412 avg=0.237us/op
```

- 最热调用点:`es_button.yst` pc1126 / pc1129(源码 line 1694/1695),
  各 **64,896 hits**(= 169 按钮 × 48 槽 × 8 参数)。
- 关闭設定后再次打开(host frame 600):**计数完全相同**
  (503,912 LET / 165,052 IF / 321,676 LOOPEND,采样 275.7ms)。
  ⇒ 不是“缺预载/缓存被清空”,而是脚本每次都重跑菜单构造;
  差距在**每条命令的解释器开销**(见 §4),不是资源加载。

页签切换(同一会话,逐帧):

```text
システム2 / サウンド / 音声 / E-mote:
  sampled 9.4–10.4ms;tick 15.5–18.6ms;work 20–30ms(约丢 1 帧)
テキスト 第一次:
  render 169ms ⇒ 其中 fontdb 首次枚举 130ms;启动预热后 render 33.4ms
  work 50ms(仍有一次字体字节/字形缓存的一次性成本)
テキスト 再次进入:
  render 10.9–11.1ms;work 27–30ms
```

- `fontdb::Database::load_system_fonts()` 本机 ~130–144ms;桌面宿主/serve
  现在在 `Engine::new` 里预热(`EngineOptions.warm_text`),把这次成本从
  第一次「テキスト」点击移到启动阶段。
- 代价:启动总时长 479→669ms(+144ms),交换游戏中第一次文本页签的
  165→33ms;headless 用例默认不预热。

## 4. 剩余瓶颈与下一步(不在本会话修复)

1. **场景装载仍是同步突发**:打开設定一帧约 50.4 万 `LET` + 16.5 万 `IF`
   + 32.2 万 `LOOPEND`（`es_button` 清 169×48×8 参数）,tick 264–270ms。
   单命令固定开销(LET/IF ~0.21–0.23µs)仍是主项;native 解释器约快一个数量级。
   下一步候选(按风险/收益):
   - `exec` 执行位从“每命令 remove/insert”改为泵内缓存
     (同一任务连续执行时只取/还一次);
   - `IF/LET` 的窗口解码缓存命中路径去掉重复 `window_hash`;
   - `LET` 常量右值在 `fill_cmd` 时预求值(`CmdWindow.const_value`)。
2. **`SOUND` 同步解码**:531 次调用 91ms(172µs/次)。需要按时长/文件缓存
   已解码 PCM,或把解码移到声音线程(必须先取证官方加载时机)。
3. **`VARINFO`/`CG` 查询**:`CG` 111k 次 179ms(1.61µs/次),多为缺失资产
   挂号路径;需要在 P2 的 gfx 取证里确认是否可缓存“缺失”结果。
4. **首个可播放 1.0s**:启动前 0.67s 是 wgpu/adapter + 软后端对账 + 字体预热;
   可把窗口创建提前/异步,但 Presenter 需要 engine 的 GPU 上下文(结构改动)。

### 4.1 为什么不用“限流/超时”兜底脚本突发

曾尝试把调度器的 `pump_cap` 暴露为 `--pump-cap N`(每任务每帧最多 N 条命令),
用 100k/500k 两档实测:

```text
--pump-cap 100000 → frame 23: Fatal rt.label @ es_button.yst@2377 GOSUB
--pump-cap 500000 → frame  9: Fatal rt.label @ 同一位置
```

同一位置失败,说明脚本依赖**帧内任务交错/初始化顺序**,任意边界抢占会破坏
它。该开关保留为实验性诊断(默认关,日志打 EXPERIMENTAL),见
`docs/divergences/DIV-HOST-002-pump-cap.md`。
**通用修复仍应是解释器吞吐**,不是抢占。

## 5. 像素与回归门禁

```text
# title 600 帧,soft/wgpu 各一份逐帧 SHA
$ scenario_probe <project-eris> target/tmp/perf/title-600.json \
    --backend soft --hash-csv target/tmp/perf/sha-soft8.csv
$ scenario_probe <project-eris> target/tmp/perf/title-600.json \
    --backend wgpu --hash-csv target/tmp/perf/sha-wgpu8.csv
$ wc -l sha-soft8.csv sha-wgpu8.csv   → 601 / 601
$ diff -q sha-soft8.csv sha-wgpu8.csv → 无输出(0 差异;与上一轮逐帧一致)
```

- 600 帧宿主 `--shot` 的标题像素 SHA = `a6db617b…`,与 P1 台账
  (`docs/symptom-ledger.md` §5.4,本会话前采集)一致。
- `yu-cli case <project-eris> --all`:**8 passed / 0 failed**(基线 8/8 不变)。
- `cargo test -p yu-rt -p yu-host-desktop -p yu-cli -p yu-trace`:
  **27 个 test target 全 ok / 0 failed**(见 `target/tmp/perf/test-final7.out`)。

## 6. 复现命令

```bash
export TEMP="<side-repo>/workbench/tmp" TMP="$TEMP"
export YURIS_SAMPLE_ERIS_YSBIN="<project-eris>/save/data/ysbin"
# 基线/修复后 600 帧 + 逐帧 CSV + 前两帧命令耗时
yu-desktop <project-eris> --frames 600 \
  --log-file target/tmp/perf/run.jsonl \
  --frame-stats target/tmp/perf/run.csv \
  --profile-frames 1,4
# 設定点击(场景装载)profile
yu-desktop <project-eris> --frames 450 \
  --script cases/host/symptom-1-title-settings-back.json \
  --frame-stats target/tmp/perf/settings.csv --profile-frames 301
# 逐帧像素 SHA(soft/wgpu)
scenario_probe <project-eris> target/tmp/perf/title-600.json \
  --backend soft --hash-csv target/tmp/perf/sha-soft8.csv
```
