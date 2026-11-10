# `libyu_host_android.so` · 宿主 API

> **so 是本体,APK 只是壳。** 任何模拟器/启动器的 Android 壳都能直接 dlopen
> 这个 so,按下面的 JSON 配置驱动它。新增能力只加 JSON 键,**JNI 签名不变**。

## 加载

```java
static { System.loadLibrary("yu_host_android"); }
```

so 的 ABI:`arm64-v8a`(真机)、`x86_64`(模拟器)。目前固定 **soft 渲染后端**
(CPU 出帧,零 GPU 依赖,与桌面 `--backend soft` 同构)。

## 入口一览

| 方法 | 作用 |
|---|---|
| `nativeInitJson(String json)` | **推荐**。按 JSON 配置启动会话 |
| `nativeTick()` | 推进一帧;`0` 成功 / `-1` 未初始化 / `-2` 失败 |
| `nativeFrameSize()` | `[w, h]`;未出帧为 `[0, 0]` |
| `nativeCopyFrame(int[] out)` | 写 ARGB_8888 像素;返回像素数,负值失败 |
| `nativeSetInput(String button, boolean pressed)` | 注入按钮(`MOUSE_L` / `KEY_ENTER` …) |
| `nativeSetCursor(int x, int y)` | 注入光标(游戏视口像素坐标) |
| `nativeStatus()` | 诊断摘要(状态、帧号、fps 之外的对账数字、最近错误) |
| `nativeListFonts()` | 本机字体族名单(`\n` 分隔) |
| `nativeVersion()` | 构建目标标记 |
| `nativeShutdown()` | 释放会话;`0` = 确实释放了 |

**线程**:所有方法必须在**同一线程**调用(内核单线程,会话状态在
thread-local)。推荐用一个专用渲染线程,主线程只发输入标志。

## 启动配置(JSON)

```jsonc
{
  // ---- 必填 ----
  "data_root": "/sdcard/Download/yugame",        // 含 data.ypf 或 data/
  // ---- 存档 ----
  "save_root": "/sdcard/Download/yugame/save",   // 省略 = data_root/save
  // ---- 脚本 ----
  "entry": "SYSTEM_START",                        // 缺省 SYSTEM_START
  "debug_mode": false,                            // @_DEBUGMODE=1
  "script_key": "0x11223344",                     // 省略 = 自动解析
  // ---- 字体 ----
  "font": "Noto Sans CJK JP",                     // 强制 family(等价 --font)
  "font_file": "/sdcard/fonts/msgothic.ttc",      // 导入的字体文件
  // ---- 声音 ----
  "os_sound": true,   // @_OS_SOUND 注入值(对拍口径恒 true)
  "audio": true,      // 是否真的输出音频;false 只静音,不影响 os_sound
  // ---- 视频 ----
  "backend": "soft",  // soft(CPU) | wgpu(GPU);GPU 不可用即显式失败,不降级
  "fps": 60,
  "screen_color": 32, // @_SCREEN_COLOR
  "dx_mode": 0,       // @_DX_MODE
  // ---- 存档/媒体 ----
  "save_codec": "zlib",        // zlib | snappy;省略 = 自动判定
  "media_readonly": false,     // @_MEDIA_READONLY
  // ---- 诊断 ----
  "record_events": false,      // 记录完整事件流(占内存,对拍用)
  "log_file": "debug/yu-kernel.jsonl",  // 相对 data_root;同 CLI --log-file
  "log_level": "verbose",      // quiet|error|warn|info|verbose
  // ---- 运行开关(与桌面 CLI 同名;同一份 RenderTuning)----
  "prefetch": false,           // 资产异步预取(等价 --prefetch)
  "prefetch_mb": 128,          // 预取池预算
  "asset_cache_mb": 512,       // 资产缓存预算(0 = 不限)
  "asset_name_fallback": true, // 编码不一致时的唯一同名回退
  "bilinear_scale": false,     // SIZE 也按双线性采样(手机画质)
  "snapshot_defer": false,     // COPY2 快照延迟物化
  "text_base_cache": true,     // 文本基面 CPU 缓存
  "text_gpu": true,            // GPU 文本 pass
  "pipeline_prewarm": true,    // 启动期管线预热
  "png_backend": "image",      // image | libpng
  "vm_profile": false,         // VM opcode 逐条耗时归因
  "asset_timing": false,       // 资产装载/预取计时
  "tick_split": false,         // tick 内部分段计时
  "cg_timing": false,          // CG 差分计时
  "readback_debug": false,     // GPU 读回原因
  "trans_debug": false,        // 过渡合成取证
  "text_gpu_debug": false,     // GPU 文本失败/回退原因
  "render_timing": false,      // 渲染阶段 timing
  "render_trace_geometry": false,
  "p8_timing": false,
  "font_trace": false,
  "png_cmp": false
}
```

### 与 CLI 的对应关系

| CLI 旗标 | JSON 键 |
|---|---|
| `<project-root>` | `data_root` |
| `--save-root DIR` | `save_root` |
| `--entry LABEL` | `entry` |
| `--debug-mode on` | `debug_mode` |
| `--script-key 0x…` | `script_key` |
| `--font FAMILY` | `font` |
| `--save-codec zlib\|snappy` | `save_codec` |
| `--record-events on` | `record_events` |
| `--no-sound` | `audio: false`(不是 `os_sound`) |
| `--backend soft` / `--backend wgpu` | `backend` |
| `--wgpu-backend NAME` | `gpu_backend` |
| `--log-file PATH` | `log_file`(相对路径落 `data_root`) |
| `--log-level LVL` | `log_level` |
| `--prefetch` / `--prefetch=off` | `prefetch` |
| `--asset-cache-mb N` | `asset_cache_mb` |
| `--png-backend image\|libpng` | `png_backend` |
| `--vm-profile` / `--vm-profile=off` | `vm_profile` |
| `--asset-timing` / `--tick-split` / `--cg-timing` | 同名键 |

完整运行开关表见 `docs/cross-platform.md`「宿主参数:唯一取值表」。
Android 与桌面共用 `yu_render_core::RenderTuning`;`debug/run-config.json`
覆盖文件也用同一批键(不再有宿主 `setenv` 的第二套开关)。

## 最小用法

```java
String json = "{\"data_root\":\"/sdcard/Download/yugame\"}";
String init = NativeHost.nativeInitJson(json);   // "OK ..." 或 "ERR: ..."
if (init.startsWith("ERR")) { /* 显示 init */ return; }

// 渲染线程
while (running) {
    if (NativeHost.nativeTick() != 0) break;
    int[] size = NativeHost.nativeFrameSize();
    int[] pixels = new int[size[0] * size[1]];
    NativeHost.nativeCopyFrame(pixels);
    // Bitmap.createBitmap(pixels, size[0], size[1], ARGB_8888)
}
NativeHost.nativeShutdown();
```

## 日志

`log_file` 非空即写 JSONL,**格式与 `yu-cli --log-file` 完全一致**,可直接用
同一套工具分析:

```bash
adb pull /sdcard/Download/yugame/debug/yu-kernel.jsonl
```

JSONL 走 `BufWriter`,内核在启动收尾、`nativeStatus()` 与 `nativeShutdown()`
时显式 flush —— 边跑边 `adb pull` 也能拿到内容。

## 渲染后端

| `backend` | 说明 |
|---|---|
| `soft`(默认) | CPU 出帧。零 GPU 依赖,与桌面 `--backend soft` 同构 |
| `wgpu` | GPU 出帧。设备不支持 Vulkan/GLES 时**显式失败**,不自动降级 |

不自动降级是有意的:自动降级会让"GPU 模式画错"永远对不上号(用户看到的是
CPU 结果,却以为在看 GPU)。

## 性能:帧时间构成与已知瓶颈

`nativeStatus()` 现在带逐阶段耗时(单位 µs),排查掉帧先看这里:

```
render_ms=18.51 phases[plan=6995 encode=2062 readback=8145 tail=69]us
```

| 阶段 | 典型耗时 | 性质 |
|---|---|---|
| `tick_ms` | 1.8-2.0ms | 脚本逻辑 |
| `plan` | 4-9ms | CPU 遍历图层、生成顶点 |
| `encode` | 1.8-5ms | 命令编码(每 clear 目标一个 render pass) |
| **`readback`** | **6.8-13ms** | **GPU→CPU 整帧读回(1.9MB/帧)** |

**`readback` 是最大单项**,原因是当前壳用 `nativeCopyFrame(int[])` 取像素再画
Bitmap —— 每帧都要把整帧从 GPU 拷回 CPU。桌面宿主不这么做:它把 GPU 纹理
直接上屏。

**要跑满 60fps,需要零拷贝上屏**:把 wgpu 渲染直接输出到 SurfaceView 的
原生窗口(ANativeWindow + wgpu::Surface),跳过 CPU 读回。这是后续优化项,
当前未实现。

另外注意:模拟器是**软件模拟的 x86_64**,单核跑满(实测 CPU 96%)时
`plan`/`encode` 波动很大。真机上这两项会明显下降,同样的场景帧率更高。
评估性能请在真机上看 `phases` 数据,不要以模拟器为准。

## 输入坐标约定

`nativeSetCursor(x, y)` 收的是**游戏视口像素坐标**(`nativeFrameSize` 那个
尺寸),不是屏幕像素。壳侧要先把触摸点从屏幕坐标换算:

```text
relX = touchX - presentLeft;      // presentLeft/Top = 画面在黑边内的偏移
vx   = relX * viewportW / presentWidth;
```

**只按 View 全尺寸做比例换算是错的** —— 黑边区域被算进去,越靠边缘偏移越大
(实机复现:点菜单项落到别的项)。渲染与触摸必须共用同一个绘制矩形。

另外:按下/抬起要**按序投递**。只采样"当前是否按下"会丢掉整个发生在两帧之间
的快速点击(表现为"点了没反应")。

## 数据与存档

Android 上 app 只能直接读写自己的目录或(授予「所有文件访问」后)任意共享
目录。内核按**真实文件路径**读写,因此:

- 要么让用户通过 SAF 选目录,壳把文件落到 app 可读的位置;
- 要么申请 `MANAGE_EXTERNAL_STORAGE` 后直接读 `/sdcard/Download/<game>`
  (参考壳的做法)。

存档默认写 `<data_root>/save/`;`.sd` 字节与桌面完全同构,可互换。

## 字体

Android 没有 Windows 日文字体(ＭＳ ゴシック 等),两条来源:

1. **系统字体**:内核扫 `/system/fonts`、`/product/fonts`
   (`nativeListFonts()` 列出 family);
2. **导入字体**:`font_file` 指向 ttf/otf/ttc,或把字体放进
   `<data_root>/fonts/`(自动加载)。

`font` 键会**强制**所有 `FONT` 请求解析到该 family;查不到即硬失败,
不静默回落(否则用户会以为换上了其实没换)。
