# DIV-AND-001 · Android 无系统字体枚举(fontdb 上游缺口)

- 状态:`fixed(host-capability)`(2026-11-01)
- 影响面:`yu-font`、Android 宿主;桌面/web 不受影响
- 关联:架构 §2.5(H7)、`docs/font-contract.md`

## 现象

`fontdb::Database::load_system_fonts()` 在 `target_os = "android"` 上是**空
操作**——上游 `fontdb 0.24` 的该函数只有 windows / apple / redox / 一般
unix 分支,`android` 被显式排除在 unix 分支之外
(`cfg(all(unix, not(any(macos, ios, android))))`)。

后果:Android 上 `FontSource::load_system_fonts()` 得到零 face ⇒
`default_face()` 报 `font.MISSING` ⇒ 整屏文本不绘制。游戏脚本点名的是
Windows 日文字体(ＭＳ ゴシック),即便枚举成功也不会命中,但**零 face
与「枚举成功但没有目标 family」是两种不同状态**,前者会让文本层完全失效。

## 处置

两层,都在"字体由宿主提供"这条既有能力面内,不引入内置字体:

1. **系统目录扫描**(`crates/yu-font/src/source.rs`):Android 目标额外扫
   `/system/fonts` 与 `/product/fonts`(AOSP 与主流 ROM 的实际落盘位置),
   走 `fontdb::load_fonts_dir`。这是"枚举系统已装字体",与桌面同能力。
2. **宿主字节注入**(`FontSource::load_with_extra_bytes` +
   `TextComposer::add_font_bytes` + `EngineOptions::font_bytes`):
   Android 宿主读 `<dataRoot>/fonts/*.ttf|otf|ttc` 注入。用于补齐游戏点名的
   Windows 日文字体,与桌面「系统枚举 ∪ 宿主附加」同一合并顺序。

## 为什么不内置字体

A5(通用运行时零硬编码)禁止把游戏/版本特定资源写进代码;字体是**宿主
能力**,不是引擎语义。桌面从系统取,Android 从系统 + app 数据目录取,web
从宿主字节取 —— 同一入口,不同来源。

## 验证

- `cargo check -p yu-font --target aarch64-linux-android` 通过。
- `cargo test -p yu-host-android --test session` 用真实语料跑通 12 帧
  (登录界面文本经字体链路绘制,`drawable=true`)。
- 桌面行为不变:`load_with_extra_bytes(&[])` 与旧 `load_system_fonts()`
  的枚举结果一致(同一 `fontdb` 调用序)。

## 残留

- 真机上的具体命中 family 未逐一取证(不同 ROM 字体集合不同)。诊断行
  `[font] system fonts: N faces ... engine default family "..." -> ...` 会
  在 logcat 里给出实际结果。
- `fontdb` 上游若补上 android 分支,本处扫描可删(届时只留宿主字节注入)。
