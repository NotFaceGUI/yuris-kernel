# yu-cli · 原生引擎宿主 + 本地服务 + wasm 呈现端

任务书:`docs/tasks/task-k2-hosts.md`。架构按 2026-10-21 主人决定调整为 **B**:

```text
浏览器  www/index.html + app.js + yu-host-wasm(只呈现)
   │  WebSocket: RGBA 帧 / 输入 / 控制
   ▼
本地服务  yu-cli serve(或 yu-engine serve)
   │  同一份 native 读取路径:按官方挂载规则发现数据入口
   ▼
yu-rt(运行核) → yu-gfx(CompositorFrame) → yu-backend-soft / yu-backend-wgpu
```

要点:

- **读取与执行都在 Rust**,和 `yu-cli run` 完全同一条链;浏览器从不下载整包,
  因此多 GB 商业包不受 wasm 内存限制。
- **wasm 只是呈现端**:接收 RGBA、WebGPU 呈现(shim)优先,Canvas2D 回退;
  不含 `yu-rt`/`yu-gfx`/后端依赖(构建 < 1s 量级)。
- 浏览器不能启动本地进程 ⇒ 入口是 `yu-cli serve <project>`,它启动服务并
  默认打开 `http://127.0.0.1:<port>/`。用户看到的就是一个页面。
- 页面刷新/重连**不重启游戏**:状态在服务端,重连后继续收帧。
- `yu-cli dev` 是 HMR 监督进程:改 Rust → 重建 `yu-engine` 子进程 → 重启游戏;
  浏览器 WebSocket 自动重连。改 `www/` → SSE `reload` 让页面刷新。

## 命令

```text
yu-cli run  <project-root> [--backend soft|wgpu] [--entry LABEL] [--frames N]
                           [--until-visible] [--shot PATH] [--json PATH]
                           [--record-events on|off]
yu-cli serve <project-root> [--backend soft|wgpu] [--port 4173] [--no-open]
                           [--entry LABEL] [--fps 60] [--record-events on|off]
yu-cli dev  <project-root> [--port 4173] [--no-open] [--backend soft|wgpu]
yu-cli case <project-root> --all | --case NAME [--update]
yu-cli bench <project-root>
```

数据入口唯一约定(`AGENTS.md` §2):`<project-root>/data.ypf` 优先,
`<project-root>/data/` 为备;`<project-root>` 也可以是 `.ypf` 文件
(aux 根 = 其父目录)。

### `run`(桌面 headless)

- 同步 `GfxHost` 路径;`take_events()` 只做计数/drain。
- 默认跑 `--frames N`(默认 1)帧渲染最后一帧;`--until-visible` 在首个结构
  可绘制帧提前停(仍受帧数上限约束)。
- 宿主注入字段显式给值:`screen_color=32`、`dx_mode=0`、`os_sound=true`、
  `media_readonly=false`、`record_events` 默认 off。
  **`os_sound`/`media_readonly` 必须与参考环境一致**:`@_OS_SOUND=0` 会让 ERIS
  走不同启动分支(实测首帧只剩 2 层,不再到 title);`@_MEDIA_READONLY` 是
  "只读介质"(光盘)语义,不是"内核只读数据"(A2),官方硬盘安装为 0。
  宿主不播放声音 ≠ 引擎认为声音不可用。
- `--backend wgpu` 时同帧再跑软后端,计数必须逐项一致,否则 exit 2。
- 退出码:`0` 正常;`2` 运行/对账失败;`3` 缺样本或数据入口不存在。

### `serve`(产品入口)

```
GET /                 www/index.html(同源提供,无需额外静态服务器)
GET /app.js /styles.css /pkg/*  静态资源(no-cache)
GET /api/status       服务状态(JSON;被 HMR 测试轮询)
GET /healthz          ok
GET /events           SSE(HMR reload / 状态变更)
GET /ws               WebSocket:帧下发 + 输入/控制上行
```

服务端以 `--fps` 节拍推进引擎;每帧把 RGBA 直接推给所有 WS 客户端。
慢客户端不阻塞引擎:发送队列满时丢弃该客户端的最旧帧并计数。

`GET /api/status` 示例:

```json
{"ok":true,"marker":"k2-engine-0","backend":"soft","frame":123,
 "viewport":[800,600],"entry":"SYSTEM_START","fps":60,
 "dataset":{"kind":"packed","bytes":31501753},
 "clients":1,"dropped_frames":0,"tick_ms_avg":8.4,"render_ms_avg":6.1}
```

### `dev`(HMR 监督)

1. `cargo build -p yu-cli --bin yu-engine`(debug);
2. 启动子进程 `yu-engine serve <project> --port N`(stdin 作为控制通道);
3. 500ms 指纹轮询 `crates/**/*.rs` + `Cargo.toml`(排除 `target/`),300ms 去抖;
4. Rust 变更 → 结束子进程 → 重建 → 重启(浏览器 WS 自动重连,重新出首帧);
5. `www/**` 变更 → 向子进程 stdin 发 `reload` → SSE 广播 → 页面刷新;
6. 连续修改的耗时/内存记录在 `/api/status.hmr`。

监督进程本身不重建(Windows 上运行中的 exe 不可覆盖);改 `yu-cli` 自身的
监督逻辑需要手动重启一次,这一点在 `/api/status` 里显式写明。

### `case`

`cases/*.json`:

```json
{
  "name": "first-screen",
  "entry": "SYSTEM_START",
  "frames": 3,
  "until_visible": true,
  "inputs": [{ "frame": 2, "button": "CLICK", "x": 400, "y": 300 }],
  "expect": {
    "display_json_sha256": "…",
    "png_sha256": "…",
    "ops_total": 44,
    "skipped_ops_total": 29
  }
}
```

- `CLICK` = `MOUSE_L` 的别名;按下发生在 `frame`,在 `frame+1` 松开。
- `expect` 缺省字段不比较;`--update` 重生成并打印 diff,人工审后入库。
- 任一 fail ⇒ exit 2;缺样本 ⇒ exit 3。

## WebSocket 协议

服务端 → 客户端:二进制帧(小端)。头部 56 字节:

```text
+0   "YUF1"                    4 字节
+4   u32 width                 +8  u32 height
+12  u64 frame                 +20 u32 ops
+24  u32 skipped               +28 u32 executed
+32  u32 passes                +36 u32 layers
+40  u32 visible               +44 u32 ms
+48  u8  backend (0=soft,1=wgpu)
+49  u8  flags (bit0 drawable, bit1 rendered, bit2 paused)
+50  u16 reserved
+52  u32 rgba_len
+56  RGBA8 像素
```

连接建立时先发一条文本 hello(就是 `/api/status` 的 JSON),再补发最近一帧
(`--pause-at` 暂停或断线重连时页面不会空白等待)。

客户端 → 服务端(文本 JSON):

```json
{"t":"input","button":"MOUSE_L","pressed":true}
{"t":"cursor","x":400,"y":300}
{"t":"restart"}
{"t":"pause"} / {"t":"resume"}
```

## P0 性能声明(先声明,后测量)

> **2026-10-21 主人决定切到 B 架构后重新声明。** 下方「已作废声明」保留
> 审计痕迹,不再作为门禁。

| 指标 | 阈值(门禁) | 口径 |
|---|---:|---|
| 原生 boot(表 + 入口脚本)| **< 500 ms** | debug 构建 |
| 原生热重启:re-boot 后可 tick(第 1 个逻辑帧)| **< 1000 ms** | 第二次 boot + 首帧 tick(任务书原文口径) |
| 引擎稳态 800×600 tick + soft 合成 | **≥ 30 fps**(平均 ≤ 33.3 ms) | release;debug 仅记录 |
| 输入 → 下一帧下发(服务端) | **p95 ≤ 50 ms** | 本地 loopback |
| 浏览器连上 → 首帧呈现 | **≤ 1000 ms** | 页面加载后 WebSocket 首帧 |
| 帧传输持续 300 帧 | **丢帧 0**,平均 ≥ 30 fps | 800×600 RGBA / loopback |
| HMR:改 Rust → 子进程重启 → 页面拿到新 marker | **≤ 15000 ms** | `www` 的 WS 重连 |
| 连续 10 次 HMR | 无泄漏 | 子进程 RSS/handle 不单调增长 |

信息项(只记录,不设门禁):**到首个可绘制帧的累计耗时**(debug 下初始化帧
有 129 万命令,会显著超过 1 s)、`record_events` on/off 首帧耗时、wgpu device
初始化、wasm presenter debug/release 增量构建、`serve` 内存峰值。

> 口径修订(2026-10-21,正式 P0 测量前):原表把"boot + 首个可绘制帧 < 1 s"
> 当门禁;实测 debug 下初始化帧(129 万命令)单独就要 1.3–1.7 s,该口径与
> 任务书原文"re-boot 后可 tick < 1 s"不是同一件事。现按任务书原文设门禁,
> 首个可绘制帧的累计耗时降为信息项并如实记录;**不是测完再改数**。

复测:

```bash
export YURIS_SAMPLE_ERIS_ROOT=<project-eris>
cargo run -p yu-cli -- bench <project-eris>
```

### 已作废声明(2026-10-21 早,wasm 跑内核方案)

> 以下阈值在 B 决定前写下,保留审计;不再验收。
>
> - 原生热重启 < 1000 ms(仍在新表)
> - wasm 增量构建 + browser reload ≤ 15000 ms(语义由新 HMR 表接替)
> - 浏览器首帧含 31.5 MB fetch ≤ 5000 ms(B 下浏览器不再下载整包)
> - 连续 10 次 HMR 无泄漏(保留)

## 基准与实测(2026-10-21)

<!-- BASELINE:BEGIN -->
机器:Windows `win32 10.0.26300` / RTX 5070(NVIDIA 驱动,Vulkan)/ Rust 1.96.0。
语料:`<project-eris>/data.ypf`,31,501,753 字节。命令:

```bash
./target/release/yu-cli.exe bench <project-eris> --frames 300
```

| 项 | 实测(release) | 备注 |
|---|---:|---|
| boot(`record_events=off`) | **43.9–104.4 ms** | 160 ms(debug) |
| 第 1 个逻辑帧 tick | **2379–2440 ms** | 初始化帧 129 万命令 |
| 到首个可绘制帧(frame 3) | **2729–2842 ms** | 信息项 |
| `record_events=on` 首帧 | **4107–6047 ms** | 输出与 off 逐字节一致 |
| re-boot(丢 Vm+GfxState) | **75 ms** | 渲染器/纹理缓存保留 |
| re-boot 后可 tick(第 1 帧) | **2917–3301 ms** | 初始化帧占 2.4–3.2 s |
| 稳态 300 帧 | **41.5 fps**(avg 24.1 ms,p95 16.5 ms) | 最慢:frame 6 2816 ms、frame 3 310 ms、frame 227 153 ms |
| wgpu device 初始化 | **444 ms** | `NVIDIA GeForce RTX 5070` / Vulkan |
| wasm presenter 无改动构建 | **171 ms** | 增量 |
| wasm 改 `lib.rs` 后构建 | **575 ms** | 增量 |
| `wasm-bindgen --target web` | **96 ms** | |
| WS 连上→首帧 | **2589 ms** | 同初始化帧 |
| WS 稳态(过启动帧后 3 s) | **60.4 fps** | 丢帧 0 |
| 输入→下一帧(服务端) | **p95 32.3 ms** | loopback |
| HMR(10 次,debug 子进程) | **avg 3640 ms / max 4242 ms** | 构建 2.3–3.2 s + 重启 0.36 s |
| HMR 后监督进程 RSS/handle | **+0 / +0** | 第 10 次 vs 第 2 次 |

门禁对照:

```text
gate boot(off) < 500 ms: 104.43 <= 500.00 -> PASS
gate re-boot 后可 tick < 1000 ms: 3300.67 <= 1000.00 -> FAIL
gate 稳态 >= 30 fps: 41.51 >= 30.00 -> PASS
gate 输入→下一帧 p95 <= 50 ms: 32.30 <= 50.00 -> PASS
gate 浏览器首帧 <= 1000 ms: 2589 -> FAIL
gate HMR <= 15000 ms: 4242 -> PASS
```

**两条 FAIL 的根因相同**:ERIS 启动链的第一个大帧(129 万命令)在 release
下要 2.4–3.2 s,debug 下 14 s。boot 本身 44–104 ms;VM 跑完这个帧才能进入
title。优化它属于 `yu-rt` 派发/表达式热路径(本任务只读消费,未改)。
按纪律保留 FAIL,未调阈值、未换口径。

浏览器验收(2026-10-21,headless Chrome 149 + CDP):

```text
服务:yu-cli serve <project-eris> --no-open --port 4205 --pause-at 302
native:yu-cli run <project-eris> --until-frame 302
  frame: 302 ops: 27 total, 15 drawn, 12 skipped
  rgba sha256: 6d183527625c64f0eb12831a59f2ccd37a86cc39c20d62466d38bf133ed5df39

浏览器 ?present=2d:
  overlay: frame=302 ops=27 skipped=12 executed=15 present=2d
  canvas sha256: 6d183527...ed5df39   ← 与 native 逐字节一致
浏览器默认(WebGPU):
  overlay: frame=302 ops=27 skipped=12 present=webgpu
  canvas sha256: 6d183527...ed5df39   ← WebGPU 呈现同样逐字节一致
  Restart: 状态回到 frame 1 → 再次跑到 302 并停住,浮层数字不变
```

`target/tmp/browser-webgpu-paused.png` 是 title 画面的截图(frame 302,
Ku-ris B4 标题)。
<!-- BASELINE:END -->

## 已知边界

1. **浏览器不能启动本地进程**:`serve`/`dev` 是入口;页面单独打开(`file://`)
   时 wasm 模块受 CORS 限制,页面会显示“等待本地服务”。
2. **`yu-backend-wgpu` 不跑在浏览器**:它的设备初始化是同步 `pollster::block_on`,
   wasm32 无阻塞原语。浏览器侧 WebGPU 只做**呈现**,合成由 native 后端完成。
3. **内存**:服务端内存 = 引擎 + 纹理缓存;浏览器内存 = 单帧 RGBA + GPU 纹理。
   多 GB 包只在 Rust 侧按需读,不会进浏览器。
4. **帧格式**:当前是未压缩 RGBA(800×600 ≈ 1.9 MB/帧,loopback 足够);
   远程/弱机需要时再加 delta/压缩,协议版本号在头部 `YUF1` 上区分。
5. **Vite 未启用**:B 架构下页面由引擎服务同源提供、HMR 由 WebSocket 重连
   完成,不需要 Node 构建链。任务书里的 `--vite` 随之取消;需要前端框架时
   自行在 `www/` 里加,服务只做静态文件。
6. **多 GB 包**:服务端 `YpfSource::open` 惰性随机读,浏览器只收帧;
   没有整包进内存的路径。
7. **画面缩放**:引擎按工程 `yscfg` 的 800×600 渲染,页面把 canvas 平滑
   放大到窗口(视觉小说观感)。需要看原始像素时加 `?pixel=1`(最近邻)。
   CSS 缩放不参与像素校验:验收读的是 canvas 内部 800×600。
