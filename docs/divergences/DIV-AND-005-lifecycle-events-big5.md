# DIV-AND-005 · 宿主退出信号缺失、挂起不丢时间、Big5 编码选项

- 状态:`fixed(unify)`(2026-11-07)
- 影响面:`yu-engine`、`yu-platform/host-android`、`android/`(Java 壳)、
  `yu-format`、`yu-vfs`、`yu-render/core`;桌面像素行为不变
- 关联:DIV-AND-004(宿主参数唯一取值表)、`docs/cross-platform.md`

## 问题(三条,均为真实缺陷)

### 1. 游戏结束后壳不知道(退出信号只在诊断串里)

脚本 `END` / 主脚本走完后,内核有 `main_finished`/`restart_requested` 两个
布尔,但 Android 壳只在 `nativeStatus()` 的**非 quiet** 状态串里能看到
`main_finished=1`。发布模式(`quiet=true`,上架默认)根本看不到 ⇒ 游戏
`END` 后画面停住、声音继续,用户只能按返回键杀进程。三个宿主各自读两个
布尔拼判断,也是同一逻辑写三遍。

### 2. 挂起不丢时间(Android `onPause` 回来会快进)

桌面有 `Engine::reset_realtime_clock` 但 Android 的 `onPause` 只停了渲染
线程,**没有重置引擎时钟**。后台放几分钟再回来,第一次 `tick_realtime`
把整段墙钟灌进逻辑时间 —— 淡出/补间按 `@_OS_TIME` 差值推进,全部瞬移。
同时音频输出在后台继续混音(设备持有)。

### 3. 文本编码只有 CP932/GBK,繁中(Big5)无法选择

港台旧作与部分繁中汉化补丁是 CP950/Big5;码表没有,cp950 与 cp936 前导
区间重叠也无法自动识别 —— 只能显式选择,但选项不存在。

## 处置

### 退出信号:`Lifecycle`(跨平台唯一)

`yu_engine::engine::Lifecycle { Running | Finished | Exited { restart } }`:

- `Engine::lifecycle()` 是宿主判断"游戏是否结束"的**唯一**入口
  (`main_finished()`/`restart_requested()` 降级为组成部分,不再单独用);
- `Lifecycle::host_code()`:`0` = 继续,`1` = 终态 —— Android `nativeTick`
  直接返回该值;`nativeLifecycle()` 供壳主动查询(`0/1/2/3/-1` 细分);
- **发布模式状态串也带生命周期**(`status=exited … restart=0`):它是退出
  信号,不是诊断,不随 `quiet` 省略;
- Java 壳:`nativeTick() == 1` ⇒ 停泵、`nativeShutdown()`、`finish()`
  回配置界面。任何新宿主照 `is_terminal()` 一个判据写。

### 挂起/恢复:`Engine::suspend` / `Engine::resume`

- `suspend()`:丢弃暂停期间的真实时间(恢复不快进)+ 静音音频输出
  (设备保持打开,复用 restart 的 boot 期静音路径);幂等。
- `resume()`:声音状态重新挂回音频端口;幂等。
- Android JNI:`nativeSuspend`/`nativeResume`;壳 `onPause`/`onResume`
  一一接线。**不做存档 flush** —— 存档在 `SAVE` 命令时刻已落盘。

### Big5(CP950)

- 码表:`gen_tables.py` 增加 code page 950 → `table_big5.rs`
  (19720 条双字节、10 条编码别名、lead 0x81..=0xFE / trail 0x40..=0xFE,
  尾字节 0x80..=0xA0 是空洞);
- `yu_format::Codec::Big5`(decode/encode/往返测试全绿)+ `from_name`
  (`big5|cp950|big-5`);
- `PatchKind::Big5`(`--patch big5` / `run-config.json` 的 `patch` 键):
  三域 `execution/text_default = Big5`,`name` 保持 CP932(资源命名从不随
  汉化改变);
- `yu_vfs::Codec::Big5`(路径扫描判据:lead 同 GBK,trail 含 0x40..=0x7E、
  空洞 0x7F..=0xA0);`TextEncoding::Big5` 进入 VFS 查找键;
- `yu_render/core` 的文件名编码回退从"CP932↔GBK 写死一对"改为遍历
  `Codec::ALL`(新增码页不必回来改调用点);
- **两层"自动识别"要分开看**:
  - `yu-format::codec::auto`(脚本/文本编码裁决)**不把 Big5 纳入裁决**:
    cp950 与 cp936 前导区间重叠,构成统计无法可靠区分;`auto` 仍在
    CP932/GBK 之间定,Big5 必须显式选。
  - `yu-vfs::encoding`(资源名编码扫描)**一直都把 Big5 算在候选里**
    (`CANDIDATES`/`scores` 早就有它),但 `as_codec()` 以前返回 `None`
    使结果无法落地。本轮让 `as_codec()` 返回 `Some(Big5)` —— 这是**行为
    变化**:若某批资源名被判为 Big5,现在会真的用 cp950 去查(以前会退化成
    CP932 查不到)。`yu-vfs` 单测 43 项全绿。

### 壳新参数(全部持久化到 SharedPreferences)

| Intent Extra | JSON 键 | 取值 |
|---|---|---|
| `EXTRA_PATCH` | `patch` | `auto / original / gbk / big5 / tunnel` |
| `EXTRA_SAVE_CODEC` | `save_codec` | `"" (自动) / zlib / snappy` |
| `EXTRA_FPS` | `fps` | `0 = 内核缺省 60` |
| `EXTRA_SCREEN_COLOR` | `screen_color` | `0 = 不覆盖` |

配置界面新增「文本编码」(自动/原版/简体/繁体)与「存档」(自动/zlib/snappy)
两组按钮卡片 + 说明文字;`nativeLifecycle`/`nativeSuspend`/`nativeResume`
与 `nativeTick` 返回值语义写进 `NativeHost.java` 文档。

### 顺带修复(工具链)

`tools/android/build.sh` 在 Windows 上把 sh 版 `*-clang` 喂给 Rust 链接器
(CreateProcess 无法执行,os error 193)。MINGW/MSYS 下自动改用
`*-clang.cmd` 包装器。

## 验证

```text
cargo test -p yu-format --lib            → 全绿(新增 big5 全表/单字节往返)
cargo test -p yu-vfs --lib               → 全绿(新增 big5 lead/trail 判据)
cargo test -p yu-host-android --lib      → 14 passed(新增 patch 词表对齐)
cargo test -p yu-cli --test lifecycle -- --ignored
  → starts_running_and_suspend_resumes_without_time_jump ok
    (挂起 300ms 后恢复首帧 clock_ms < 100 ⇒ 时间确实被丢弃)
Java 壳:javac(bootclasspath android-36 + 本机 Gradle 缓存 appcompat 1.6.1
  依赖树)编译通过;SetupActivity 变量冲突(saveCard 重名)当场发现并修
./target/debug/yu-cli run <project-eris> --frames 2 --patch big5
  → 启动正常;`--patch euc-kr` 显式报错并列出可用取值
三帧 headless --patch big5 的 rgba 与缺省一致(该语料是日文,boot 帧无文本
  差异;编码路线在 evidence 中标注 cli: big5)
cargo check -p yu-host-android --target aarch64-linux-android → 通过
tools/android/build.sh x86_64 → so + APK(需 NDK;构建脚本 Windows 链接器已修)
```

## 残留

- `EXTRA_FPS`/`EXTRA_SCREEN_COLOR` 的 UI 卡片未加(壳参数已通,配置界面
  下批补;fps 长期看应跟随游戏 yscfg 而非壳设置)。
- `nativeLifecycle` 的 `2/3` 细分(exited/exit+restart)桌面向未消费;
  桌面照旧走 `winit` 关窗事件,`is_terminal()` 判据一致。
- 真 wasm 宿主的 `local_now()` 仍恒 `None`(DIV-AND-004 已记)。
