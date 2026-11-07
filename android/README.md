# Android 宿主(`yu-host-android`)—— 构建与验证

跨平台目标的一部分(架构 `docs/architecture.md` §2.5 / §2.5.8 H7):
**同一份 `yu-engine` 装配 + Android 薄壳**,不是另写一套引擎。

```text
crates/yu-platform/host-android/     ← Rust:JNI 入口 + 会话管理(cdylib)
android/                             ← 轻量 UI 壳(纯 Java,无 AndroidX)
tools/android/build.sh               ← so 交叉编译 + APK 打包
```

## 为什么第一期用 soft 后端

要验证的是**跨平台能力等价**(数据读取、存档写入、字体枚举、脚本执行),
不是 GPU 像素性能。`--backend soft` 是确定性参考实现、零 GPU 依赖,帧以
RGBA 交给 Java 侧 Bitmap 上屏,与桌面 `yu-cli run --backend soft` 出**同构**
的帧。wgpu(`Surface`/Vulkan)是后续增强路径,不阻塞本阶段验证。

## 构建

```bash
# 前置:rustup target add aarch64-linux-android x86_64-linux-android
export ANDROID_HOME=/path/to/sdk
export ANDROID_NDK_HOME=/path/to/ndk        # 或不设,自动取 $ANDROID_HOME/ndk 下最高版本
export JAVA_HOME=/path/to/jdk17+            # AGP 8.13 要求

tools/android/build.sh                      # arm64-v8a + x86_64
tools/android/build.sh arm64-v8a            # 只打真机
```

产物:`android/app/build/outputs/apk/debug/app-debug.apk`。

首次用 Gradle 需生成 `android/local.properties`(`sdk.dir=...`,**用正斜杠**;
反斜杠会被 Java properties 当转义吃掉)。若要完全离线构建,`gradle.properties`
里的 `android.aapt2FromMavenOverride` 指向 SDK 里已有的 aapt2。

**若 `gradle` 启动即报 `Could not initialize native services /
Failed to load native library 'native-platform.dll'`**:那是 `~/.gradle/native`
下的产物在本机加载失败(常见于被 IDE/沙箱改过权限),与项目无关。处置是给它一个
独立的 `GRADLE_USER_HOME`,只把依赖缓存拷/链接过去,不动原有 `~/.gradle`:

```bash
export GRADLE_USER_HOME=/some/writable/ghome
mkdir -p "$GRADLE_USER_HOME/caches"
cp -r ~/.gradle/caches/modules-2 "$GRADLE_USER_HOME/caches/"
cp -r ~/.gradle/caches/jars-9    "$GRADLE_USER_HOME/caches/" 2>/dev/null
cp -r ~/.gradle/caches/transforms-4 "$GRADLE_USER_HOME/caches/" 2>/dev/null
gradle --offline --no-daemon assembleRelease
```

本机(2026-11-07 实测)可用组合:`JAVA_HOME = Android Studio 的 JBR`
(`C:\Program Files\Android\Android Studio1\jbr`,全 JDK 反而报
`Failed to load native library 'native-platform.dll'`)+ 上面的独立
`GRADLE_USER_HOME`(依赖缓存已有 178M,离线 `assembleDebug` 40s)。
Windows 下 NDK 的 sh 版 `*-clang` 无法被 Rust exec,`build.sh` 会自动改用
`.cmd` 包装器(也兼容 r28 起 sysroot 挪进 toolchains 的新布局)。
```

同理,若出现 `modules-2.lock ... 拒绝访问`,说明有 IDE 正在使用同一份缓存;
独立 home 可绕开。

## 壳侧开关(发布 / 诊断)

`SetupActivity` 的两个开关都会持久化并传给游戏页:

| 开关 | 默认 | 作用 |
|---|---|---|
| 状态浮层 | 关 | 开:左上角显示 `fps=`;再开「调试日志」才铺全量计数 |
| 调试日志 | 关 | 开:每帧状态行写 logcat + `<游戏目录>/debug/yu-kernel.jsonl`,并启用混音抓取 |

两者都关时,壳**完全不调** `nativeStatus()`,内核也处于发布模式
(`RunConfig::quiet`):logcat 只放 `warn` 以上、不抓音频、状态串最小化。

## 手机化输入

- **触摸(默认)**:点哪里点哪里;抬手后把脚本光标移出视口,避免"下一屏同位置
  按钮一直 hover"。
- **双指上下滑**:滚轮(`_MOUSE_U`/`_MOUSE_D`),用于回看文本。
- **虚拟鼠标**:右下角半透明小圆钮切换(状态记忆)。鼠标态下拖动移动光标、
  抬手在光标处点击。

## 安装与验证

```bash
adb install -r android/app/build/outputs/apk/debug/app-debug.apk

# 数据放到 app 外置私有目录(adb push 即可,无需任何存储权限)
GAME=/sdcard/Android/data/org.yuris.kernel/files/game
adb shell mkdir -p $GAME
adb push <project>/data.ypf      $GAME/
adb push <project>/yscfg.dat     $GAME/
adb push <project>/yu-ris.exe    $GAME/     # 可选:脚本密钥扫描来源
adb push <fonto>/msgothic.ttc    $GAME/fonts/   # 可选:游戏点名的日文字体

adb shell am start -n org.yuris.kernel/.MainActivity
adb logcat -s yu-android                    # 每帧一行 status(含 fps)
```

存档写 `<files>/save/`(即
`/sdcard/Android/data/org.yuris.kernel/files/save/`),与桌面 `save/` 语义一致,
`.sd` 字节级同构。

## 数据布局

```text
files/game/
  data.ypf | data/      ← 数据入口(封包优先,散盘次之;与桌面同一规则)
  yscfg.dat             ← 视口/配置
  yu-ris.exe            ← 可选,脚本密钥自举
  fonts/*.ttf|otf|ttc   ← 可选,宿主字体注入
files/save/             ← 存档(可写)
```

## 触摸 → 输入

触摸坐标按 surface 尺寸映射回游戏视口像素,`ACTION_DOWN/UP` 映射
`MOUSE_L` 的按下/抬起(经 `yu-platform-input` 的按钮 ABI)。虚拟按键/手势
映射尚未取证(架构 §2.5.9 开放问题 9),当前只做单点 = 鼠标左键。

## 生命周期(见 DIV-AND-005)

`onPause` → `nativeSuspend()`:内核丢弃暂停期间的真实时间(恢复**不快进**
—— 否则后台几分钟回来,第一帧把整段墙钟灌进 `@_OS_TIME`,淡出/补间瞬移)
并静音音频;壳停渲染线程。`onResume` → `nativeResume()`:声音挂回、重建
渲染循环。`onDestroy` 释放内核会话。存档在 `SAVE` 刷盘的**命令时刻**落盘,
因此挂起不需要额外 flush。

退出信号是跨平台统一的:脚本 `END`/主脚本走完后 `nativeTick()` 返回 `1`
(内核 `Lifecycle::host_code`),壳据此收尾并回配置界面 —— 发布模式
(quiet)下同样可见,不再依赖诊断状态串。

## 启动参数(Intent Extra → run-config.json 键)

| Extra | 键 | 取值 | 缺省 |
|---|---|---|---|
| `game_dir` / `save_dir` | 同名 | 游戏根 / 存档根 | `files/game` / `<game>/save` |
| `backend` / `gpu_backend` | 同名 | `soft\|wgpu` / `auto\|gles\|vulkan` | soft / auto |
| `entry` | 同名 | 入口标签 | SYSTEM_START |
| `font_path` / `font_family` | `font_file` / `font` | 字体文件 / family | 空 |
| `patch` | 同名 | `auto\|original\|gbk\|big5\|tunnel` | auto |
| `save_codec` | 同名 | `zlib\|snappy` | 自动判定 |
| `fps` | 同名 | 正整数 | 60 |
| `screen_color` | 同名 | `@_SCREEN_COLOR` 代号 | 32 |
| `audio` / `tone` | `audio` / `audio_tone` | 布尔(诊断) | true / false |
| `debug_log` / `show_hud` | —(壳侧) | 布尔 | false |

取值解析全部只有一份(见 `docs/cross-platform.md`「宿主参数唯一取值表」);
配置界面的「文本编码」「存档」卡片与选择持久化在 `SetupActivity`。
