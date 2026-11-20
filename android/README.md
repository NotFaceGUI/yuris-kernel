# Android 宿主(`yu-host-android`)—— 构建与验证

跨平台目标的一部分(架构 `docs/architecture.md` §2.5 / §2.5.8 H7):
**同一份 `yu-engine` 装配 + Android 薄壳**,不是另写一套引擎。

```text
crates/yu-platform/host-android/     ← Rust:JNI 入口 + 会话管理(cdylib)
android/                             ← 轻量 UI 壳(纯 Java,无 AndroidX)
tools/android/build.sh               ← so 交叉编译 + APK 打包
```

## 渲染路径:GPU 为主

壳固定以 `wgpu` 启动:Java 侧把 `TextureView` 的 Surface 交给 so,wgpu 在
同一 device 上直接采样上屏(零拷贝,省掉 GPU→CPU 读回与重上传)。单次启动
不会静默降级:GPU 初始化失败即显式报错并回配置界面;壳随后把重试参数切到
`soft`,由用户手动重试(避免自动降级让「GPU 模式画错」永远对不上号)。

`soft` 仍是 so 的缺省后端(不传 `backend` 时)与开发机会话测试的参考路径:
确定性、零 GPU 依赖,帧以 RGBA 交 Java 画 Bitmap,与桌面
`yu-cli run --backend soft` 同构。壳内的 CPU 按钮已停用,仅占位。

## 构建

```bash
# 前置:rustup target add aarch64-linux-android x86_64-linux-android
export ANDROID_HOME=/path/to/sdk
export ANDROID_NDK_HOME=/path/to/ndk        # 或不设,自动取 $ANDROID_HOME/ndk 下最高版本
export JAVA_HOME=/path/to/jdk17+            # AGP 8.13 要求

tools/android/build.sh                      # arm64-v8a + x86_64
tools/android/build.sh arm64-v8a            # 只打真机
```

产物:debug → `android/app/build/outputs/apk/debug/app-debug.apk`;
release → `android/app/build/outputs/apk/release/app-release.apk`。

### 本机一键构建(2026-11-09 实测,直接照抄,不要再全盘找 gradle)

本机 **不要用**逆向工具目录里的 JDK 21 跑 Gradle(会报
`Failed to load native library 'native-platform.dll'`);用 Android Studio
自带的 JBR。Gradle 用 `~/.gradle/wrapper/dists` 里已有的 8.14.3,不要再下载。
本机的具体绝对路径(用户名、SDK、Gradle 缓存 hash)记在 `AGENTS.md`
的「Android 构建」一节,不写进被跟踪文件。

```bash
export PATH="$HOME/.gradle/wrapper/dists/gradle-8.14.3-bin/<hash>/gradle-8.14.3/bin:$PATH"
export JAVA_HOME="<Android Studio>/jbr"
export ANDROID_HOME="<Android SDK>"
export GRADLE_FLAGS="--offline --no-daemon"

BUILD_TYPE=release tools/android/build.sh arm64-v8a
# 产物: android/app/build/outputs/apk/release/app-release.apk
```

NDK 不用显式设:`build.sh` 会取 `$ANDROID_HOME/ndk` 下最高版本。
`--offline` 复用默认 `~/.gradle` 的依赖缓存;**不要**再把旧
`transforms-4` 拷进新的 `GRADLE_USER_HOME`(会报
`Could not move temporary workspace`)。`aapt2.exe` 由 `build.sh` 自动探测。

脚本的环境变量(CI 与本地共用同一份):

| 变量 | 作用 |
|---|---|
| `BUILD_TYPE=debug\|release` | 打哪个构建类型,缺省 debug |
| `YURIS_SO_ONLY=1` | 只交叉编译 so 并落到 `jniLibs`,不跑 gradle |
| `GRADLE_FLAGS=...` | 覆盖 gradle 参数,缺省 `--offline`;CI 传 `--no-daemon` |
| `GRADLE_PROJECT_ARGS=...` | 追加 `-P`(`-PreleaseVersionName=… -PreleaseVersionCode=…`) |
| `YURIS_KEYSTORE_FILE` / `YURIS_KEYSTORE_PASSWORD` / `YURIS_KEY_ALIAS` / `YURIS_KEY_PASSWORD` | 发布签名四件套 |

`versionCode` / `versionName` 只有 `-PreleaseVersionName` /
`-PreleaseVersionCode` 一个来源(CI 由 tag 与 `GITHUB_RUN_NUMBER` 注入),
缺省 `0.1.0` / `1`。

发布签名:四个变量齐备时用 `signingConfigs.release`;缺任一项则回落 debug 签名
(自用安装,不可分发)。**在 CI 上(`GITHUB_ACTIONS=true`)缺签名会直接构建失败**,
不会静默产出 debug key 签名的"发布版"。签名配置见 `docs/release.md`。

首次用 Gradle 需生成 `android/local.properties`(`sdk.dir=...`,**用正斜杠**;
反斜杠会被 Java properties 当转义吃掉)。若要完全离线构建,`build.sh` 会自动
探测 `$ANDROID_HOME/build-tools/*/aapt2` 并传 `-Pandroid.aapt2FromMavenOverride`;
手工调 gradle 时照抄同一个 `-P` 即可。

**若 `gradle` 启动即报 `Could not initialize native services /
Failed to load native library 'native-platform.dll'`**:那是 `~/.gradle/native`
下的产物在本机加载失败(常见于被 IDE/沙箱改过权限),与项目无关。处置是给它一个
独立的 `GRADLE_USER_HOME`,只把依赖缓存拷/链接过去,不动原有 `~/.gradle`:

```bash
export GRADLE_USER_HOME=/some/writable/ghome
mkdir -p "$GRADLE_USER_HOME/caches"
cp -r ~/.gradle/caches/modules-2 "$GRADLE_USER_HOME/caches/"
cp -r ~/.gradle/caches/jars-9    "$GRADLE_USER_HOME/caches/" 2>/dev/null
gradle --offline --no-daemon assembleRelease
```

本机(2026-11-07 实测)可用组合:`JAVA_HOME = Android Studio 的 JBR`
(`<Android Studio>/jbr`;全 JDK 反而报
`Failed to load native library 'native-platform.dll'`)+ 上面的独立
`GRADLE_USER_HOME`(依赖缓存已有 178M,离线 `assembleDebug` 40s)。
**不要**把 `transforms-4` 一起拷过去(见下)。

2026-11-09 复测:**JBR + 默认 `~/.gradle` + `GRADLE_FLAGS="--offline --no-daemon"`
直接 `assembleRelease` 29s 成功**。不要再把旧 `transforms-4` 拷进新的
`GRADLE_USER_HOME`:Gradle 8.14.3 会因旧 transform 工作区被占用而报
`Could not move temporary workspace`;只复用 `caches/modules-2` 即可。
Windows 下 NDK 的 sh 版 `*-clang` 无法被 Rust exec,`build.sh` 会自动改用
`.cmd` 包装器(也兼容 r28 起 sysroot 挪进 toolchains 的新布局)。

同理,若出现 `modules-2.lock ... 拒绝访问`,说明有 IDE 正在使用同一份缓存;
独立 home 可绕开。

同理,若出现 `modules-2.lock ... 拒绝访问`,说明有 IDE 正在使用同一份缓存;
独立 home 可绕开。

## 壳侧开关(发布 / 诊断)

`SetupActivity` 的两个开关都会持久化并传给游戏页:

| 开关 | 默认 | 作用 |
|---|---|---|
| 状态浮层 | 关 | 开:左上角显示 `fps=`;再开「调试日志」才铺全量计数 |
| 版本名角标 | **开** | 左下角显示 `YU-RIS <版本名>`;内核判定支持=绿,不支持=红(如 v555) |
| 帧率上限 | **60** | 30 / 60 / 120;壳侧帧节拍与内核 `fps` 参数同值 |
| 调试日志 | 关 | 开:每帧状态行写 logcat + `<游戏目录>/debug/yu-kernel.jsonl`,并启用混音抓取 |
| 资产预取 | 关 | 等价桌面 `--prefetch`:`CG[FILE]` 声明即交后台解码,首用命中池/同步兜底 |
| 快照延迟 | 关 | 等价桌面 `--snapshot-defer`:无读者的 COPY2 只登记,后续被引用再物化 |
| 缺字补全 | 关 | 等价桌面 `--font-fallback`:主字体缺字时按「宿主注入字体→系统字体」补字形;默认关保持官方缺字不画 |
| SIZE 双线性 | **开** | 官方 `SIZE` 是最近邻;手机高 DPI 下 CG/差分缩放锯齿,开启后按双线性采样(像素与官方有差异,见 `DIV-GFX-083`) |
| PNG 解码 | **开** | 开:Android 走 libpng(NEON 反滤波;画面逐字节一致);关:回 `image` crate。角色首帧解码尖峰可在此 A/B |

两者都关时,壳**完全不调** `nativeStatus()`,内核也处于发布模式
(`RunConfig::quiet`):logcat 只放 `warn` 以上、不抓音频、状态串最小化。

## 手机化输入

- **启动方式由配置界面决定**:「操作方式」卡片选「触摸」或「虚拟光标」,
  随 Intent Extra `mouse_mode` 作为启动值;游戏内切换不持久化。
- **触摸**:点哪里点哪里;抬手后把脚本光标移出视口,避免"下一屏同位置
  按钮一直 hover"。
- **双指上下滑**:滚轮(`_MOUSE_U`/`_MOUSE_D`),用于回看文本。
- **虚拟光标**:光标先落在画面中心;拖动移动光标、抬手在光标处点击。
- **游戏内切换**:右下角半透明小圆钮只在**本次会话**内临时切换,不落盘,
  也不覆盖配置界面的选择 —— 下次启动仍按壳的设置来。

## 对话框(DIALOG)

脚本的 `DIALOG`(改名输入、确认、通知等)由**壳**呈现:内核只把待答请求
以 JSON 透出,壳回答。参考壳用平台 `AlertDialog`:

- 确认/通知:OK / OK-Cancel / Yes-No / Yes-No-Cancel;返回键只对带
  Cancel 的组合生效;
- 输入(改名):单行输入框,DEFSTR 预填,INPUTLIMIT 限制,IME 完成键 = OK;
- 对话框存续期间壳不向游戏投输入。

其他 Android 壳可以整体替换 `DialogPresenter`,只要消费
`nativePendingDialogs()` / `nativeAnswerDialog()` 的同一份 JSON(字段见
`NATIVE_API.md` 与 `docs/cross-platform.md`)。

## 选择游戏目录

配置界面用系统**目录选择器**(`ACTION_OPEN_DOCUMENT_TREE`)选游戏文件夹,
不再要求先在目录里点一个文件。选中项解析成真实路径交给内核(内核只认路径,
不认 `content://` URI):

- 选到游戏根本身、或它的子目录(向上找 4 层)都可以;
- 选到游戏的上一级且只有一个子目录像游戏根时,自动进入该子目录;
- 多个候选不猜,提示重新选;
- 个别文件管理器/受限目录(如 Android 11+ 的 Download 根)选不了,或
  provider 解不出真实路径时,用「手动输入路径…」输入 `/sdcard/...` 绝对路径。
- 存档目录的「另外指定…」同样走系统目录选择器;「默认」仍为
  `<游戏目录>/save`。

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

## Tyranor-Next 第三方运行时模块(额外 APK)

同一个壳可以再打一份 `applicationId = com.tyranor.runtime.yuris.kernel` 的
APK,作为 Tyranor-Next 的第三方运行时模块。宿主按包名前缀
`com.tyranor.runtime.*` 发现模块,再用显式 Intent 派发:

- action `com.tyranor.runtime.action.LAUNCH`;
- extra `com.tyranor.runtime.extra.FOLDER_PATH`(游戏根目录绝对路径);
- 可选 `GAME_TITLE` / `PROTOCOL_VERSION`(当前 1)/ `SETTINGS`。

宿主不传 SAF URI,也不代授存储权限;本壳本来就按真实路径读写,首次启动
仍需用户自己授「所有文件访问」。模块入口在 `SetupActivity`;收到
`FOLDER_PATH` 后落到与手动选目录相同的状态,权限与字体就绪即自动进游戏,
正常退出游戏会关掉配置页回到 Tyranor。

构建(不影响普通 `org.yuris.kernel` 包):

```bash
# 模拟器(x86_64)
YURIS_APK_CHANNEL=tyranor BUILD_TYPE=release YURIS_APK_VARIANTS=x86_64 \
  tools/android/build.sh x86_64

# 真机(arm64-v8a)
YURIS_APK_CHANNEL=tyranor BUILD_TYPE=release YURIS_APK_VARIANTS=arm64-v8a \
  tools/android/build.sh arm64-v8a
```

通用包(arm64-v8a + x86_64)用 `YURIS_APK_VARIANTS=universal` 且不传 ABI
参数:

```bash
YURIS_APK_CHANNEL=tyranor BUILD_TYPE=release YURIS_APK_VARIANTS=universal \
  tools/android/build.sh
```

产物在 `target/android-apks/app-tyranor-release-<abi>.apk`。安装后在
Tyranor-Next 的「工具 → 引擎模块管理」里**手动启用**(默认禁用),再重扫
游戏库;扫描/设置/导入声明都在
`app/src/main/res/raw/tyranor_runtime.json`:

```json
{
  "protocol": 1,
  "label": "YU-RIS Kernel",
  "scan": {
    "match": "any",
    "extensions": [".ypf"],
    "markers": ["data.ypf", "data/ysbin", "ysbin"]
  },
  "settings": {
    "version": 1,
    "items": [
      { "key": "fps", "type": "enum", "title": "帧率", "default": "60",
        "options": [{ "value": "60", "title": "60 fps" }] },
      { "key": "font", "type": "text", "title": "字体", "default": "", "maxLength": 1024 }
    ]
  },
  "fileImports": {
    "version": 1,
    "items": [
      { "key": "fonts", "title": "导入字体", "directory": "fonts", "multiple": true }
    ]
  }
}
```

当前暴露的单游戏设置(完整声明以 JSON 为准):`fps`、`mouse_mode`、
`bilinear_scale`、`font_fallback`、`patch`、`gpu_backend`、`movie_backend`、
`font`、`debug_log`、`show_hud`。Tyranor 在游戏抽屉的「引擎设置」里按声明
渲染;模块只能控制 title/description/type/default,卡片间距与样式由宿主决定。

字体链路:

- 「导入字体」把文件复制到 `<游戏目录>/fonts/`;
- 内核启动时加载 `<游戏目录>/fonts/*.ttf|otf|ttc|otc`(按文件名排序);
- `font` 留空 = 自动:首个已加载字体的第一 family 作为默认
  (实测日志 `font_bytes=1 host_fonts=1 font=SimSun`);
- `font` 也可填字体族名或 `.ttf/.otf/.ttc` 绝对路径。

说明:

- 普通 APK 的清单里也有同一个过滤器,但宿主的包名前缀检查会把它排除,
  因此不会被误识别;两包可并存安装。
- 模块卡片引擎名显示为 `Plugin`;`label` 只出现在模块管理里。
- 暂未声明 `saves` 托管:本壳允许自定义存档目录,声明后会和宿主托管冲突。
- 改包名等于换模块身份,已有绑定不会迁移。

模拟宿主派发(设备上已有游戏目录时):

```bash
adb install -r target/android-apks/app-tyranor-release-x86_64.apk
adb shell appops set --uid com.tyranor.runtime.yuris.kernel \
  MANAGE_EXTERNAL_STORAGE allow
adb shell am start -a com.tyranor.runtime.action.LAUNCH \
  -n com.tyranor.runtime.yuris.kernel/org.yuris.kernel.SetupActivity \
  --ei com.tyranor.runtime.extra.PROTOCOL_VERSION 1 \
  --es com.tyranor.runtime.extra.FOLDER_PATH /sdcard/Download/yugame
```

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
`MOUSE_L` 的按下/抬起(经 `yu-platform-input` 的按钮 ABI);双指上下滑映射
滚轮,虚拟光标模式由壳把拖动/抬手翻译成移动 + 左键。虚拟按键(菜单/存档
快捷键)尚未取证(架构 §2.5.9 开放问题 9)。

## 生命周期(见 DIV-AND-005)

`onPause` → `nativeSuspend()`:内核丢弃暂停期间的真实时间(恢复**不快进**
—— 否则后台几分钟回来,第一帧把整段墙钟灌进 `@_OS_TIME`,淡出/补间瞬移)
并静音音频;壳停渲染线程。**音频闸门在调用线程立即生效**(设备回调补静音、
mixer 停止取数并清空环形缓冲),不依赖渲染线程的下一帧 tick —— 壳关泵后
挂起请求可能要等回前台才被消费,只靠引擎 `suspend()` 会「切后台还有声音」。
`onResume` → `nativeResume()`:声音挂回、重建渲染循环。`onDestroy` 释放内核
会话。存档在 `SAVE` 刷盘的**命令时刻**落盘,因此挂起不需要额外 flush。

退出信号是跨平台统一的:脚本 `END`/主脚本走完后 `nativeTick()` 返回 `1`
(内核 `Lifecycle::host_code`),壳据此收尾并回配置界面 —— 发布模式
(quiet)下同样可见,不再依赖诊断状态串。

## 启动参数(Intent Extra → run-config.json 键)

| Extra | 键 | 取值 | 缺省 |
|---|---|---|---|
| `game_dir` / `save_dir` | 同名 | 游戏根 / 存档根 | `files/game` / `<game>/save` |
| `backend` / `gpu_backend` | 同名 | `soft\|wgpu` / `auto\|gles\|vulkan` | 壳固定 `wgpu`；so 缺省 `soft` / auto |
| `entry` | 同名 | 入口标签 | SYSTEM_START |
| `font_path` / `font_family` | `font_file` / `font` | 字体文件 / family | 空 |
| `patch` | 同名 | `auto\|original\|gbk\|big5\|tunnel` | auto |
| `save_codec` | 同名 | `zlib\|snappy` | 自动判定 |
| `movie_backend` | 同名(+`cache_dir`) | `auto\|builtin\|external`;外置=设备 MediaCodec,自动失败回退内置 | auto |
| `mouse_mode` | —(壳侧) | 布尔:`true`=虚拟光标,`false`=触摸 | false |
| `fps` | 同名 | 正整数 | 60 |
| `screen_color` | 同名 | `@_SCREEN_COLOR` 代号 | 32 |
| `audio` / `tone` | `audio` / `audio_tone` | 布尔(诊断) | true / false |
| `prefetch` | `prefetch` | 布尔(资产异步预取) | false |
| `snapshot_defer` | `snapshot_defer` | 布尔(COPY2 延迟物化) | false |
| `font_fallback` | `font_fallback` | 布尔(逐字形缺字回退) | false |
| `bilinear_scale` | `bilinear_scale` | 布尔(`SIZE` 双线性;手机画质) | 壳默认 true / 内核默认 false |
| `png_fast` | `png_backend` | 布尔(开=`libpng`,关=`image`) | 壳默认 true |
| `debug_log` / `show_hud` | —(壳侧) | 布尔 | false |

取值解析全部只有一份(见 `docs/cross-platform.md`「宿主参数唯一取值表」);
配置界面的「文本编码」「存档」卡片与选择持久化在 `SetupActivity`。
