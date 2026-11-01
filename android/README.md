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

## 生命周期

`onPause` 停渲染线程(停时钟停泵),`onResume` 重建;`onDestroy` 释放内核会话。
存档在 `SAVE` 刷盘的**命令时刻**落盘,因此挂起不需要额外 flush。
