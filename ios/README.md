# iOS 壳(骨架)

`ios/` 是 `yu-host-ios` 的 Swift 壳模板:数据入口选择、tick 循环、CPU 帧
显示、触摸、生命周期与 DIALOG。**不是**另写一套引擎,与 Android 壳同构。

> **现状**:`soft` 后端 + `CGImage` 上屏(CPU 帧拷贝)。Metal 的
> `CAMetalLayer` 入口已预留在 C ABI(`yu_ios_attach_layer`,当前 `-3`),
> 接线步骤见 [`crates/yu-platform/host-ios/README.md`](../crates/yu-platform/host-ios/README.md)。

## 前置

- macOS + Xcode(Windows 上只能跑 `cargo test -p yu-host-ios`)
- Rust targets:`rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios`
- [XcodeGen](https://github.com/yonaskolb/XcodeGen):`brew install xcodegen`

## 构建

```bash
# 只出静态库(device + simulator)
bash tools/ios/build.sh

# 生成 Xcode 工程并构建模拟器 app
YURIS_IOS_XCODE=1 bash tools/ios/build.sh

# 真机(先在 Xcode 里设 Team,或传 DEVELOPMENT_TEAM)
YURIS_IOS_XCODE=1 YURIS_IOS_DEVICE=1 DEVELOPMENT_TEAM=XXXXXXXXXX bash tools/ios/build.sh
```

产物:

- 静态库:`dist/ios/iphoneos/libyu_host_ios.a`、`dist/ios/iphonesimulator/libyu_host_ios.a`
- Xcode 工程:`ios/YuKernel.xcodeproj`(xcodegen 生成,gitignore)
- app:由 `xcodebuild` 输出到 DerivedData

也可以手动:

```bash
cd ios && xcodegen generate && open YuKernel.xcodeproj
```

## 数据导入

iOS 沙箱不允许浏览任意目录。本壳用系统「文件」集成:

1. `Info.plist` 已开 `UIFileSharingEnabled` + `LSSupportsOpeningDocumentsInPlace`;
2. 用「文件」App(iPhone 上「我的 iPhone → YuKernel」)或 Finder 文件共享,
   把整个游戏目录拷进 app 的 `Documents/`;
3. 回到 app 点「刷新」,壳对每个子目录调用 `yu_ios_probe_root`
   (`Project::discover` 同一判据),列出可用入口;
4. 选择入口后启动;存档写到 `Library/Application Support/YuKernel/Saves/`,
   日志写到 `Library/Logs/yu-kernel.log`。

多 GB 游戏目录会自动标记 `isExcludedFromBackup`,避免进 iCloud 备份。
首次导入大目录时「文件」App 的拷贝可能较慢,这是系统行为。

## 文件

| 文件 | 作用 |
|---|---|
| `AppDelegate.swift` | 窗口 + 导航 |
| `SetupViewController.swift` | 扫描 Documents、`probe_root`、选择入口 |
| `GameViewController.swift` | `CADisplayLink` tick、RGBA → `CGImage`、触摸、生命周期、DIALOG |
| `NativeHost.swift` | C ABI 的 Swift 包装(两次调用字符串、帧缓冲) |
| `YuKernel-Bridging-Header.h` | `#import "yu_ios.h"` |
| `project.yml` | XcodeGen 工程描述 |
| `Info.plist` | 文件共享、方向、全屏、启动屏 |

## 已知限制(按优先级)

1. **Metal 未接**:当前每帧 `yu_ios_copy_frame` 把 800×600 RGBA(1.9MB)
   拷到 Swift 再建 `CGImage`,只适合 PoC;上屏优化见下一节。
2. **音频会话**:`AppDelegate` 已设 `AVAudioSession` 分类 `.playback`
   (避免被静音键静音);电话/耳机中断后的恢复仍未处理,需要在
   `AVAudioSession.interruptionNotification` 里重建/恢复 cpal 流。
3. **线程**:骨架把 tick 放主线程;长帧会卡 UI。稳定版应改专用渲染线程,
   输入通过原子/队列转发(与 Android 壳同一模型)。
4. **数据导入**:仅支持「文件」App 拷贝;`UIDocumentPicker` 的安全作用域
   bookmarks、zip 导入、断点续拷都未做。
5. **分发**:GPL-3.0-or-later 与 App Store 条款的兼容性需单独评估;本骨架
   按自用/侧载定位,不包含上架配置。

## 下一步(Mac 上)

1. `host-android` 抽 `Presenter` 端口 → `CaPresenter`(`CAMetalLayer`)→
   `FrameView.layerClass = CAMetalLayer.self` + `attach_layer`;
2. 首 N 帧与 `soft` 逐字节 hash 对齐;
3. `AVAudioSession` 分类 + 中断恢复;
4. 专用渲染线程 + 输入队列;
5. 真机性能/内存(纹理缓存预算)与老旧机型基线。
