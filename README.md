<div align="center">

<img src="docs/assets/logo.png" alt="yuris-kernel" width="112">

<h1>yuris-kernel</h1>

<p><b>YU-RIS 兼容运行时</b><br>
以官方引擎的可观测行为为唯一真值</p>

<p>
<a href="https://github.com/NotFaceGUI/yuris-kernel/actions/workflows/ci.yml"><img src="https://github.com/NotFaceGUI/yuris-kernel/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
<a href="LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0--or--later-blue" alt="License: GPL-3.0-or-later"></a>
<a href="https://github.com/NotFaceGUI/yuris-kernel/releases"><img src="https://img.shields.io/github/v/release/NotFaceGUI/yuris-kernel?label=release&color=blue" alt="Release"></a>
<img src="https://img.shields.io/badge/YU--RIS%20BETA%204-0.450--0.495-2ea44f" alt="YU-RIS BETA 4">
<img src="https://img.shields.io/badge/YU--RIS%20BETA%203-0.276--0.292-dfb317" alt="YU-RIS BETA 3">
<img src="https://img.shields.io/badge/YU--RIS%20BETA%202-0.255-dfb317" alt="YU-RIS BETA 2">
<img src="https://img.shields.io/badge/YU--RIS%205xx-official--commercial-lightgrey" alt="YU-RIS 5xx（官方商业）">
<img src="https://img.shields.io/badge/platform-Windows%20%7C%20Android%20%7C%20headless-8250df" alt="Platforms">
<img src="https://img.shields.io/badge/rust-2021%20edition-orange?logo=rust" alt="Rust">
</p>

<p><b>简体中文</b> · <a href="README.en.md">English</a></p>

<p>
<a href="#简介">简介</a> ·
<a href="#支持范围">支持范围</a> ·
<a href="#演示">演示</a> ·
<a href="#下载">下载</a> ·
<a href="#快速开始">快速开始</a> ·
<a href="#android">Android</a> ·
<a href="#架构">架构</a> ·
<a href="#文档">文档</a> ·
<a href="#许可">许可</a>
</p>

</div>

## 简介

`yuris-kernel` 是 YU-RIS 引擎的兼容运行时，仓库不包含官方引擎代码、游戏数据或存档。

- **多版本覆盖** —— 官方系列 BETA 2 / BETA 3 / BETA 4：
  BETA 4 主线（0.450–0.495），BETA 3（0.276–0.292）/ BETA 2（0.255）适配中；
  官方商业 5xx 分支（v500 封包 / v555 七表）适配中；27 张 0.255–0.495
  系统变量 ABI 表已入册，另有 v555 1000 槽实测表（`docs/formats/v555-*`）。
- **双渲染后端** —— `soft`（CPU，确定性参考）与 `wgpu`（GPU，桌面 / Android 主路径）。
- **多宿主** —— Windows 桌面、headless / 本地服务、浏览器呈现（调试）、Android。
- **汉化与存档** —— `original` / `gbk` / `big5` / `tunnel` 四条文本路线；
  `translate.json` 运行时替换角色/选项/UI/正文（CP932 自动扩隧道）；
  `.sd` 存档与桌面字节一致。
- **可观测** —— `yu-trace` 录制 / 回放 / diff，用于和官方引擎逐帧对拍。

## 支持范围

### 引擎版本

官方版本分为 BETA 2 / BETA 3 / BETA 4 三个主线系列；5xx 是 YU-RIS 官方商业分支（商业发行版），不属于这三条主线。个别商业样本的发行包可能带重打包/汉化处理，但那是样本层面的处理，不是引擎本身。

| 系列 | 状态         | 说明                                                                                                                                                            |
| ---- | ------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| BETA 2 | 适配中     | 0.255（0.55）：539 系统变量表已入册；YSTB V2 缺真实样本                                                                                                          |
| BETA 3 | 适配中     | 0.276 样本；0.292（0.92）：91 命令 / 482 系统变量                                                                                                                |
| BETA 4 | 可用（主线） | 0.450–0.495：0.466 / 0.479 / 0.495：100 / 104 / 119 命令，513 / 526 / 548 系统变量；0.450 / 0.460 / 0.470 / 0.480 / 0.490 抽样实机通过，0.481 / 0.486 / 0.494 / 0.495 实机验收 |
| 5xx    | 适配中（v555） | 官方商业分支：v500 封包可读；v555 七表/剧本链已实测；已落地 `TEXT[FILE]`、`PRODUCT`、`SAVE/LOAD[MEM]`、`_DROPFILE*`、`VARINFO`；标题画面（注意页 / build 动画 / 完整菜单 / 按钮状态机）已打通 |

0.450–0.493 各版本的 ABI 表已全部入册（27 张，覆盖 0.255–0.495）。

### 文件格式与封包

| 类别 | 形态 / 扩展名 | 状态 |
| --- | --- | --- |
| 封包 | `.ypf`（0.255–0.495：LenEncNot 索引、逐字节取反名、zlib 条目；0.495 名字键 0xFF + MurmurHash2 索引） | 读 |
| | `.ypf`（0.500 商业族：Meta22 布局、明文 zlib、xxh32 载荷校验） | 读 |
| | 旧 NulXor 方言（NUL 结尾名 + XOR 名）：无真实样本，遇到即显式拒绝 | 未实现（显式拒绝） |
| | 散盘 `data/` 目录（工程形态） | 读 |
| | `pac/*.ypf` 按名挂载 + `update1..9.ypf` 依次叠加；伪装成 `.ypf` 的 MPEG-PS 按魔数跳过 | 读 |
| 表 / 脚本 | `ysbin/ysc.ybn`（YSCM 命令表，0.255–0.555） | 读 / 执行 |
| | `ysbin/ysl.ybn`（YSLB 全局标签）、`ysbin/ysv.ybn`（YSVR 变量表）、`ysbin/yst_list.ybn`（YSTL 脚本清单） | 读 |
| | `ysbin/yse.ybn`（YSER 错误名表）、`ysbin/yst.ybn`（YSTD 变量槽）、`ysbin/yscfg.ybn`（YSCF 分辨率/扩展名优先序/标题，四种头部布局） | 读 |
| | `ysbin/yst%05d.ybn`（YSTB 编译脚本；V5 执行，V2 待真实样本） | 执行 |
| | `scenario/*.txt`（5xx 剧本 DSL：`#标签`、`\命令`、宏；`sc_cn_/sc_tw_scenario/` 原生多语言变体） | 词法 / 执行 |
| 图像 | PNG / BMP / JPG / GIF / WebP / JXL / QOI / AVIF | 解码 |
| | YCG 容器（`.ycg`，或 4.7x/4.8x 包内伪装成 `.png` 的 `YCG\0`）：method 0 裸 BGRA / method 1 两段 zlib，输出 BGRA→RGBA | 解码 |
| | `.ydg`（0.500 容器；QOI/WebP 行瓦片，拼回整图） | 解码 |
| | `.psb`（0.495 图像码 5） | 识别，无解码器（显式 `asset.unsupported`） |
| 音频 | WAV PCM / ACM WAVE（非 PCM tag 走系统 ACM） | 解码 |
| | OGG Vorbis / OGG Opus（0.500） | 解码 |
| 影片 | MPEG-1 program stream（`.mpg` / 伪装成 `.ypf` 的 MPEG-PS）、YMV（YSMV 双平面帧序列） | 播放 |
| 存档 / 配置 | `.sd`（YSSD；0x138/0x1E0/0x1E1 布局、DNO=1024、500 槽内存包） | 读写 |
| | `yssfs.dat`（存档路径设置，128 字节定长记录，`_SAVE_PATH_TYPE` @ +0x54） | 读 |

### 文本与字体

| 项 | 实现 |
| --- | --- |
| 字体来源 | 0.495 及更早走系统字体：启动时按官方回退表 `ＭＳ ゴシック` → `標準ゴシック` → `ＭＳ Ｐゴシック` → `ＭＳ 明朝` → `標準明朝` 枚举（参考工程 `data.ypf` 1,407 条中没有字体文件）；v555 官方中文等版本改用预制字形页，见下一行 |
| TTF / OTF / TTC 栅格 | sfnt 解析（含 TTC face 索引）→ swash 轮廓栅格为 8-bit coverage，对齐官方 GDI `CreateFontIndirectA` + `TextOutA` 路径；`SX/SY` 0–256 px、灰度/二值抗锯齿、合成粗体、颜色/渐变；旋转/描边/阴影仅在 `Reference` 策略下启用 |
| 位图字形（v555 官方中文/多语言） | 正文用预制字形页：`cgsys_cn/font/font_c20/fnt_s20_nNN` → `CGACT[COPY2]` 按 30 px 字格抠格（X/X2、Y/Y2 含端点，纹理 31×31）→ `MEIDO` 着色 → `MERGE2` 盖到正文/人名层；内核逐轮重新抠格并降级为持久文本图像，避免跨帧外字链丢失（8b58feb，`<v555-4>` 取证） |
| 传统位图字体 `.fnt/.fon` | 参考工程与官方产物都没有这类独立位图字库文件；本内核不实现该文件格式，遇到也不静默回退（显式不支持） |
| 补丁字体 | `hook.ini` + ttf 的 CP932 载体字重映射补丁由 `yu-engine` 启动时检测并强制注入全部 `FONT` 请求 |
| 缺字回退 | 翻译模式自动开启系统字体缺字回退；`--font` 指定默认字体，`--font-lock` 强制锁定 |


### 5xx / v555 现状

- **官方商业分支**：v500 封包、v555 七表（YSCM/YSLB/YSVR/YSTL/YSER/YSCF/YSTD）均已实测；
  剧本密钥 = CRC32(作品 ID)，启动链、剧本 DSL 词法/宏、标签表、品牌影片、
  GAMEMain UI 图层已打通，headless 已到标题选择循环。
- **原生多语言**：v555 剧情文本外置为 `scenario/`（基准）、`sc_cn_scenario/`（简中）、
  `sc_tw_scenario/`（繁中）三份变体，由 `config.sd` 里的语言号选择；
  实测编码分别为 CP932 / GBK / Big5。当前内核对多语言仍依赖全局 `--patch gbk`，
  正在按语言变体收口（优化计划 O5）。
- **标题与演出（2026-11-24 补）**：`注意` 警示页 → 标题 build 动画 → 完整
  菜单链路已打通；`FILEINFO[CGFILE]` 按 0.500 扩展名表探测
  `_off/_over/_on/_onover/_na`，按钮 hover/点击生效；`.ydg`/QOI 背景与
  按钮图、`.opus` BGM/语音均可解码。
- **仍缺**：`type2/` 皮肤目录选择规则、`PRODUCT` 其它 COM、正文逐字对拍、
  内存包槽的 Snappy 分支。
- **已落地（2026-11-23 补）**：`SAVE[MEM=99999]` 的 0..499 内存槽打包、
  `LOAD[DNO=1024]` 读取、`SAVE[FILE]` 写回 DNO=1024、`TEXT[FILE]` 行内翻译、
  `WORD`/选项/按钮 `translate.json` 运行时替换。
- **文档**：v555 普查/差距/ABI 与多语言调研在
  `docs/formats/v555-commercial.md`、`v555-gap-2026-11-19.md` 与其他
  `docs/formats/v555-*`。

### 宿主平台

| 平台                | 状态   |
| ------------------- | ------ |
| Windows 桌面        | 可用   |
| headless / 本地服务 | 可用   |
| 浏览器呈现端        | 仅调试 |
| Android             | 可用   |
| Linux / macOS 桌面  | 未验证 |

Linux / macOS 桌面与真 wasm（内存数据源 + 浏览器存档）的缺口见[待办](#待办)。

## 演示

| 0.481 · 标题画面 | 0.481 · 对话 | 0.494 · 对话 |
| :---: | :---: | :---: |
| [![0.481 标题画面](docs/assets/case/1_t.png)](docs/assets/case/1_t.png) | [![0.481 对话](docs/assets/case/1_d.png)](docs/assets/case/1_d.png) | [![0.494 对话](docs/assets/case/2_d.png)](docs/assets/case/2_d.png) |
| **0.555 · 对话** | **0.555 · 官方中文** | **0.474 · 标题画面** |
| [![0.555 对话](docs/assets/case/3_d.png)](docs/assets/case/3_d.png) | [![0.555 官方中文](docs/assets/case/4_d.png)](docs/assets/case/4_d.png) | [![0.474 标题画面](docs/assets/case/5_t.png)](docs/assets/case/5_t.png) |
| **0.466 · 对话** | **0.500 · 标题画面** | **0.500 · 对话** |
| [![0.466 对话](docs/assets/case/6_d.png)](docs/assets/case/6_d.png) | [![0.500 标题画面](docs/assets/case/7_t.png)](docs/assets/case/7_t.png) | [![0.500 对话](docs/assets/case/8_d.png)](docs/assets/case/8_d.png) |


## 下载

预构建产物见 [Releases](https://github.com/NotFaceGUI/yuris-kernel/releases)：

| 平台         | 产物                                             | 内容                              |
| ------------ | ------------------------------------------------ | --------------------------------- |
| Windows x64  | `yuris-kernel-vX.Y.Z-windows-x64.zip`            | `yu-cli.exe` + `yu-serve.exe`     |
| Android      | `yuris-kernel-vX.Y.Z-android-universal.apk`      | 通用包（arm64-v8a + x86_64）      |
| Android      | `...-android-arm64-v8a.apk` / `-x86_64.apk`      | 按 ABI 分包                       |

桌面版解压即用；Android 安装 APK 后，在配置界面选择游戏目录即可。
需要自己构建或调试内核时，继续看下一节。

## 快速开始

### 环境要求

- **桌面**：Rust stable（2021 edition）；Windows x64 已验证，Linux / macOS 未验证。
- **Android**：JDK 17+、Android SDK（`minSdk 24`，即 Android 7.0+）；
  NDK 由 `tools/android/build.sh` 自动取 `$ANDROID_HOME/ndk` 下最高版本。
- **运行数据**：自备合法获取的游戏数据；仓库不附带。

### 构建

```bash
cargo build --release -p yu-cli
```

Windows x64 桌面已验证；Linux / macOS 桌面未验证。Android 构建见下方命令
与 [Android](#android) 小节。

```bash
# Android：so 交叉编译 + APK 打包
export ANDROID_HOME=/path/to/sdk ANDROID_NDK_HOME=/path/to/ndk JAVA_HOME=/path/to/jdk17
tools/android/build.sh                  # debug
BUILD_TYPE=release tools/android/build.sh
YURIS_SO_ONLY=1 tools/android/build.sh  # 只出 so
```

### 运行

数据入口按官方挂载规则：工程根下有 `data/` 目录即开发工程布局
（数据根 = `data/`），否则为发行版布局（数据根 = 工程根）。
数据根及其 `pac/` 下的全部 `*.ypf` 按文件名挂载，后挂覆写先挂；
`update1..9.ypf` 依次叠加为增量层；散盘内容目录叠在包之上。

```bash
# 最小运行：游戏目录含 data.ypf 或 data/
target/release/yu-cli run "<游戏目录>" --mode desktop

# Windows 桌面窗口
target/release/yu-cli run "<project>" --mode desktop --backend wgpu --debug-ui

# 本地服务 + 浏览器呈现；默认打开浏览器，--no-open 只起服务
target/release/yu-cli serve "<project>" --backend wgpu --port 4173

# HMR
target/release/yu-cli dev "<project>" --backend wgpu --port 4173

# 用例
target/release/yu-cli case "<project>" --all

# 导出文本 / 角色表(翻译与运行时文本 hook 的输入;UTF-8 CSV,不跑引擎)
target/release/yu-cli export "<project>" [out-dir] --text-codec auto
# 翻译模板:只出 translate.json(角色 + 选项 + UI 字面量 + 正文,带空译文槽)
target/release/yu-cli export "<project>" [out-dir] --format translate-json

# 汉化
target/release/yu-cli run "<project>" --mode desktop --patch gbk
target/release/yu-cli run "<project>" --mode desktop --patch big5
target/release/yu-cli run "<project>" --mode desktop --patch tunnel

# 换钥作品:显式指定脚本密钥(8 位十六进制;缺省自动探测);存档写盘压缩
target/release/yu-cli run "<project>" --mode desktop --backend wgpu \
  --script-key 0x1A2B3C4D --save-codec snappy
```

## 自有翻译流程

```mermaid
flowchart LR
    A[YU-RIS 工程] -->|yu-cli export| B[translate.json]
    B -->|characters / choices / literals / text| C[AI 或人工翻译]
    C -->|validate_translate.py| D{0 errors?}
    D -- 否 --> C
    D -- 是 --> E[yu-cli run --translate]
    E --> F[运行时替换]
```

### 简单教程

1. **导出模板**

   ```bash
   yu-cli export "<project>" "<out>" --format translate-json
   ```

   只生成一个 `<out>/translate.json`，含角色、选项、UI 字面量和正文；文件外文本不会修改。

2. **先翻角色，再翻选项/UI，最后翻正文**

   - `characters`：角色名，保证全篇统一；
   - `choices`：选项文字；
   - `literals`：按钮/菜单/对话框/提示；只填显示类，`context` 为 `CG`、`CGACT`、`es.SP.*`、`es.BT.KEY.SET`、`ES.SEL.GO` 等内部 ID 留空；
   - `text`：正文台词与外部剧本。

3. **校验**

   ```bash
   python .agents/skills/yuris-translation/scripts/validate_translate.py "<out>/translate.json"
   ```

   有 ERROR 先修；WARN 逐条判断。UI 全角标签不要混半角 `E-mote`/数字/空格，否则按钮会偏离中心。

4. **运行时启动**

   ```bash
   # headless / 桌面
   yu-cli run "<project>" --translate "<out>/translate.json"
   yu-cli run "<project>" --mode desktop --translate "<out>/translate.json"
   ```

   翻译模式自动打开字体缺字回退；中文显示不全时再显式指定 CJK 字体，例如
   `--font path/to/simsun.ttc --font-lock`。

5. **预渲染图片按钮**

   底部系统栏（`セーブ/ロード/...`）等按钮的文字烘焙在图片里，文本替换改不了，
   需要替换 UI 素材或改按钮绘制；这是当前文本 hook 的边界。

翻译规则（角色统一、说话人前缀、控制符、全角宽度、codec）见
[`docs/translation-workflow.md`](docs/translation-workflow.md) 与
[`SKILL.md`](.agents/skills/yuris-translation/SKILL.md)。

## 文本编码

`--patch` 选汉化路线；桌面、serve、Android 配置界面用同一份取值。

| 汉化类型  | 取值（含别名）                        | 说明                                                                          |
| --------- | ------------------------------------- | ----------------------------------------------------------------------------- |
| 原版日文  | `original`（`off` / `none` / `cp932` / `sjis` / `shift_jis`） | 未汉化的原版                                                                  |
| 简体汉化  | `gbk`（`cp936` / `gb2312`）           | 汉化补丁；译文放在散盘 `ysbin/`，靠「散盘先于封包」的读链生效                 |
| 繁体汉化  | `big5`（`cp950` / `big-5`）           | 同上，港台旧作与繁中补丁                                                      |
| SJIS 隧道 | `tunnel`（`sjis_ext`）                | 不动原编码、追加码点表的补丁；需游戏根有 `sjis_ext.bin`                       |
| 自动      | `auto`（缺省）                        | 按 `codecs.json` → `sjis_ext.bin` → 工具签名识别；不认补丁名                  |
| 运行时翻译 | `--translate` + `translate.json`（CP932 自动扩隧道） | 只替换文件内条目，不写回数据；目标 codec 跟随当前 `--patch` |
| 补丁字体  | `hook.ini` + ttf（自动检测）          | CP932 载体字重映射；启动时强制注入全部 `FONT` 请求                            |
| v555 原生多语言 | `config.sd` 语言号 → `scenario/` / `sc_cn_scenario/` / `sc_tw_scenario/` | CP932 / GBK / Big5；不是 `--patch` 汉化路线 |
| 不支持    | —                                     | exe / DLL 注入类补丁（如 `setdll` 挂 `MyDll.dll` 换字体）：不改程序，故不适用 |

> v555 原生多语言不是 `--patch` 汉化路线：`scenario/`、`sc_cn_scenario/`、
> `sc_tw_scenario/` 三份变体由 `config.sd` 的语言号选择，codec 分别为
> CP932 / GBK / Big5；`--patch gbk` 只用于汉化补丁/旧工程。

<table>
<tr>
<td align="center"><img src="docs/assets/patch/gbk.png" alt="GBK 汉化"><br><sub><b>GBK 汉化</b></sub></td>
<td align="center"><img src="docs/assets/patch/tunnel.png" alt="SJIS 隧道"><br><sub><b>SJIS 隧道</b></sub></td>
</tr>
</table>

## Android

so + 轻量 Java 壳

- 构建：`tools/android/build.sh`，产物 `app-debug.apk` / `app-release.apk`（arm64-v8a + x86_64）。
- 安装后选游戏目录即可运行；存档写 `<游戏目录>/save/`，`.sd` 与桌面字节一致。
- 输入：触摸 = 鼠标左键，双指上下滑 = 滚轮，右下角切换虚拟光标。
- 退出：脚本 `END` 后回到配置界面。

<p align="center"><img src="docs/assets/android/title.jpg" alt="Android 宿主实机运行" width="720"></p>

## 架构

`yu-engine` 负责 boot 装配与每帧 `Vm::tick_at` / render 调用。下图是数据流，
不是 crate 依赖图；`yu-trace` 是旁挂。

```mermaid
flowchart TD
    PROJ["工程根<br/>发行版:*.ypf + pac/*.ypf<br/>开发工程:data/"]
    UPD["update1..9.ypf<br/>叠加挂载"]
    PATCH["文本编码<br/>original / gbk / big5 / tunnel"]
    TRJ["translate.json<br/>characters · choices · literals · text"]
    TRANS["yu-engine::translation<br/>JSON → 目标编码 / SJIS 隧道扩展"]
    VFS["yu-vfs<br/>DirSource / YpfSource / Mount"]
    FMT["yu-format<br/>ysc · ysl · ysv · yst_list · yst%05d · yscfg<br/>scenario · literal 重写器"]
    RT["yu-rt<br/>TableSet → Vm::boot_with_aux<br/>任务 / 命令 / 表达式 / 变量域"]
    GFX["yu-gfx<br/>GfxHost → GfxState"]
    SND["yu-sound<br/>SoundHost"]
    SAV["yu-save<br/>SaveHost"]
    CF["CompositorFrame<br/>passes / layers / text"]
    CORE["yu-render-core<br/>TextureStore / TextComposer"]
    FONT["yu-font<br/>字体栅格"]
    SOFT["yu-backend-soft<br/>CPU 模式 / 参考"]
    WGPU["yu-backend-wgpu<br/>GPU 模式"]
    SURF["RGBA8 Surface"]
    HOST["宿主<br/>headless / 桌面 / Android / 浏览器呈现"]
    ENG["yu-engine<br/>boot / tick / render"]
    TRACE["yu-trace<br/>旁挂录制"]

    PROJ --> VFS
    UPD --> VFS
    PATCH -.-> FMT
    PATCH -.-> TRANS
    TRJ --> TRANS
    TRANS -->|"TranslationTable"| RT
    TRANS -.->|"DisplayConfig.tunnel"| CORE
    VFS --> FMT --> RT
    RT -->|"命令时刻同步"| GFX
    RT -->|"命令时刻同步"| SND
    RT -->|"命令时刻同步"| SAV
    GFX --> CF
    CF --> SOFT
    CF --> WGPU
    FONT -.-> CORE
    CORE -.-> SOFT
    CORE -.-> WGPU
    SOFT --> SURF
    WGPU --> SURF
    SURF --> HOST
    ENG -.->|"boot 装配"| RT
    ENG -.->|"boot 前装载"| TRANS
    ENG -.->|"tick / render"| CF
    TRACE -.-> VFS
    TRACE -.-> RT
    TRACE -.-> CF
```

启动顺序：`Project` 发现数据入口 → 若给 `--translate`，先用
`yu-engine::translation` 编码并扩展隧道 → `Vm::boot_with_aux` 装载表与入口 →
`GfxState::from_tables` → 把 `GfxHost` / `SoundHost` / `SaveHost` 装进 VM。
翻译命中发生在 VM 入口：`op_word` 按 `(script_id, pc)`、v555 `TEXT[FILE]`
写 LET 前按 `(file, line, seq)`、`GOSUB` 按 `(script_id, pc, arg)`、命令缓存
按 `(script_id, pc, window, segment)` 重写 `M` 常量；未给条目保持原文。
每帧 `Vm::tick_at` 执行任务，域宿主在命令时刻同步更新；`GfxState::frame()`
产出 `CompositorFrame`，后端合成 RGBA8。

## 项目结构

| 路径                                              | 内容                               |
| ------------------------------------------------- | ---------------------------------- |
| `crates/yu-log`                                   | 诊断与日志；所有 crate 的错误出口  |
| `crates/yu-vfs`                                   | 数据源、YPF、挂载                  |
| `crates/yu-format`                                | ybn、YSSD、YMV、CP932/GBK/Big5、剧本/字面量重写 |
| `crates/yu-rt`                                    | VM、任务、命令分派、变量域、ABI 表 |
| `crates/yu-gfx`                                   | 渲染语义与 `CompositorFrame`       |
| `crates/yu-sound` / `crates/yu-save`              | 声音 / 存档宿主                    |
| `crates/yu-font`                                  | 字体发现与字形栅格                 |
| `crates/yu-render/core`                           | 两后端共享契约                     |
| `crates/yu-render/soft` / `crates/yu-render/wgpu` | CPU 参考 / GPU 主后端              |
| `crates/yu-engine`                                | boot / tick / render 装配、translate.json 装载 |
| `crates/yu-cli`                                   | CLI、服务、HMR、用例               |
| `crates/yu-platform/*`                            | 桌面、Android、浏览器呈现、输入    |
| `crates/yu-trace`                                 | 录制 / 回放 / diff                 |
| `android/`                                        | Android 壳（Java）                 |
| `tools/android/build.sh`                          | so 交叉编译 + APK 打包             |

## 测试

```bash
cargo test --workspace
```

仓库不附带游戏数据。缺样本时相关用例显式 `SKIP` 并计数；
`YURIS_REQUIRE_SAMPLES=1` 改为失败。CI 只跑不依赖样本的 lib 测试
（`.github/workflows/ci.yml`）；完整 `--workspace` 还会覆盖语料与 GPU 用例，
需要本地样本与可用显卡。

## 待办

- 渲染：多部件层几何语义；`CGINFO[SX/SY]` 已覆盖已注入资产与 `COPY/COPY2`
  切片，未注入资产仍缺；`COLOR` 等字段、缺失目标的非 EXIST 字段未收口；
  半像素 / quad 相位与过渡覆盖度待官方截图定论。
- 文本/翻译：`WORD`、v555 `TEXT[FILE]`、角色/选项、命令缓存 `M` 常量已接
  `translate.json`；残余：UI 图片按钮（底部系统栏）需素材替换、0.500 外置
  `TEXT[FILE]` 语义未启用、literals 的显示/ID 分类仍是 `context` 规则、
  多语言变体只有一套目标 codec（运行时切换未接）。正文排版残余：GDI
  字迹/基线（DIV-GFX-056）、文字与图像加工命令序（DIV-GFX-076）。
- 字体：游戏自带「設定→フォント」已接全量目录（宿主默认字体第一 + 本机
  系统字体），游戏内切换即时生效；`--font` 改为默认字体语义，`--font-lock`
  保留旧强制行为；翻译模式自动打开缺字回退。残余：默认模式仍不逐字形回退、
  游戏硬编码默认标签（DIV-FONT-012）；位图字体（`.fnt/.fon`）参考工程无
  样本，保持显式不支持。
- 格式：YSTB V2 缺样本；YPF 旧 NUL-XOR 方言无样本，显式拒绝。
- 运行时：`CSVINFO` 未取证；`VAR`、`SAVEINFO` 已按官方 no-op 对齐；
  `_STR`、v555 按钮纹理/SPL/背景 CG 仍为 P0。
- 存档：`SAVE[MEM=99999]`、`LOAD[DNO=1024]` 与 `SAVE[FILE]` 的 1024
  写回已落地（DIV-RT-056/075/078）；带 `MEM` 的表（0.481/0.494/0.500/
  0.555）按 500 槽内存包读取，1024 不是保留号，同窗口重复预约 DNO
  报诊断；0.495 SDK 无 `MEM`，1024 仍是普通块。`FILEACT` 图层导出
  未闭环、`FILEACT[EXEC]` 拒绝执行（宿主安全策略）。
- 宿主：`REGINFO` 桌面已接（Windows 注册表）；Android 已接
  `translate.json`/`nativeStatus` 统计；Linux / macOS 桌面与真 wasm
  （内存数据源 + 浏览器存档）未验证。
- 影片：MPEG-1 program stream 可播（视频 + MP2 音频）；MPEG-2 / AVI / FLASH
  等需新的媒体 / Flash 宿主；`MOVIE[PLAY=0]` 与音视频时钟漂移未对齐。
- E-mote：子模块 `yu-emote/`，主线暂缓。

## 文档

- `PROGRESS.md` — 进度、勘误、验证结果
- `docs/architecture.md` — 架构
- `docs/cross-platform.md` — 跨平台能力矩阵
- `docs/architecture/backends.md` — soft / wgpu 边界
- `docs/render-parity.md` — 渲染语义与证据
- `docs/todo-inventory.md` — 历史缺口快照（2026-10-22；当前状态以
  `PROGRESS.md` 与各台账为准）
- `docs/references/open-source-tools.md` — 工具参考
- `docs/release.md` — tag 发布与产物
- `android/README.md` — Android 壳构建与实机验证
- 各 crate 的 `README.md` — 职责边界、端口与验证命令；`crates/yu-platform`、
  `crates/yu-render` 是目录索引

## 致谢

- [YU-RIS_Guide](https://github.com/TheVNConnoisseur/YU-RIS_Guide) —
  官方工具与 SDK 归档（仓库未声明许可）；27 张系统变量 ABI 表由其中
  `YSCom.ycd` 提取，仓库不附带 SDK / ycd 原文件；每张表头记录来源 sha256 与
  覆盖版本区间（`crates/yu-rt/data/sysvar-abi-*.tsv`）。
- [Yuris-Tyn](https://github.com/Weiss-UltimateSavior/Yuris-Tyn) —
  启动链、任务 / 驱动形态、格式事实与 golden 回归参照；只读参考。
- [RxYuris](https://github.com/ZQF-ReVN/RxYuris) — YSTB / YSCM / YSTL 解析流程、
  YSCOM Loader。
- [YU-RIS-Script-Editor](https://github.com/Aionfatedio/YU-RIS-Script-Editor) —
  YSTB V5 段序、V2 版本带判定、编码自检测；未声明许可，仅事实参考，未复制代码。
- [yuris_decompiler](https://github.com/shimamura-sakura/yuris_decompiler) —
  V2 表达式类型限定符映射（`$@` / `&$` / `&@`）与 V2/V5 结构事实；MIT，
  算法级采用并注明出处（未逐字复制源码）。
- [YURIS_TOOLS](https://github.com/jyxjyx1234/YURIS_TOOLS) — 汉化文本流程与
  隧道码点分配算法。

## 相关仓库

- [yu-emote](https://github.com/NotFaceGUI/yu-emote) — E-mote（PSB/EMT）数据层与
  运行时；以子模块挂在 `yu-emote/`，暂缓开发。

## 免责声明

- 本项目是独立实现的 YU-RIS 兼容运行时，与 YU-RIS 引擎的开发商、发行商无隶属、
  授权或背书关系；相关产品名与商标归其权利人所有。
- 仓库不包含官方引擎代码、游戏数据、存档或游戏素材文件；文档中的运行截图仅用于
  展示兼容性，相关权利归原游戏权利人。使用者需自行合法取得游戏本体及相关素材。
- 本项目面向个人学习、研究与兼容性测试。请遵守所在地法律与游戏许可协议，
  不要用于盗版、未经授权的数据分发或其他侵权用途。
- 软件按“现状”提供，不对兼容性、稳定性或存档/数据完整性作任何担保；
  使用风险由使用者自行承担。

## 许可

<a href="LICENSE"><img src="https://www.gnu.org/graphics/gplv3-127x51.png" alt="GPLv3 Logo" width="127" height="51"></a>

本项目参考的开源项目见 [致谢](#致谢)；第三方 crate 依赖均为 MIT / Apache-2.0 / BSD / Zlib 等与 GPL-3.0 兼容的宽松许可。

除另有说明外，全部源代码以 **[GPL-3.0-or-later](LICENSE)** 发布。完整许可声明见
[NOTICE](NOTICE)；第三方组件、依赖清单与许可文本见
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md) 与
[`third_party/licenses/`](third_party/licenses/)。

如需分发修改版并保留新增代码不按 GPL-3.0 发布，请通过 [@NotFaceGUI](https://github.com/NotFaceGUI) 联系作者获取单独许可。
