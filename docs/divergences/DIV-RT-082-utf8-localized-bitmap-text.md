# DIV-RT-082 · UTF-8 本地化构建的位图字体正文（挂账，未支持）

- 状态: **deferred(master)**（通用 UTF-8 编解码已支持；该构建的正文管线未实现）
- 优先级: 低（单一发行商的定制构建）
- 发现日期 / 发现方式: 2026-11-25 / `<utf8-localized-build>` 启动与正文取证

## 复现

- 游戏: `<utf8-localized-build>`（YU-RIS 0.481 定制 exe；根目录带
  `Settings_English/SimplifiedChinese/TraditionalChinese.txt`，
  资源是 `cgsys_zhcn/*`）
- 命令: `yu-cli run <project-root> --mode desktop --patch utf8`
- 现象: 启动链通过（不再弹 ruby 错误），Logo / CAUTION / 标题 / 背景 /
  立绘 / UI 正常；**正文消息窗空白**。

## 已确认事实

- 脚本文本是 UTF-8：把导出乱码按 GBK 回编码得到
  `e7 b5 82 e4 ba 86 e7 a2 ba e8 aa 8d` = `終了確認` 的 UTF-8 字节。
- 正文排版在 `data\script\eris\全角文字表示処理.txt`（游戏脚本），
  字形来自预烘焙图集 `cgsys/font/font_u22/fnt_s22_n1..145`
  （540×540，16×16 字格子）。
- 该 exe 的 `ysc.ybn` 多出 `VARINFO[UTF8CODE]` 参数名（官方 0.495
  处理体不读该槽，手册无页）。
- 运行期逐字取到的字节是 `E3 80 20`、`E7 91 20`……：每个 3 字节
  UTF-8 字符的**第 3 字节被替换成 0x20**。因此 `UTF8CODE` 只返回首字节，
  字形页号回落 `n1`，`TextImage.cell` 为空，正文不可见。
- 对照：我们的 UTF-8 取子串在**路径字符串**上验证正确
  （`全角文字表示処理.txt` 逐字切片无误），损坏只发生在正文文本行
  进入逐字循环之前。

## 根因（待 REA 证实）

定制 exe 的 TEXT / 字符串处理体把 UTF-8 文本转换成 `2 字节 + 0x20`
的私有码流，再交给逐字排版；该转换发生在游戏脚本接收文本之前。
官方 0.495 没有这套处理体，手册无页——**不猜**，按挂账处理。

## 影响

该构建的正文文本不支持；其余画面与对话框正常。可用官方 exe 游玩。

## 若日后支持，需要的证据

1. REA `<utf8-localized-build>` 主 exe：
   - `VARINFO[UTF8CODE]` 处理体（返回值口径）；
   - `VARACT[COPY]` 的 POS/LENGTH 在 UTF-8/私有码流下的步进；
   - `TEXT` / 字符串缓冲的 UTF-8 → 私有码流转换；
   - `CGACT[COPY2]/MEIDO/MERGE` 的字体页单元链。
2. 用上述证据实现独立 opt-in 路径，默认不影响 CP932/GBK/Big5 游戏。

## 回归 / 边界

- 通用 UTF-8 codec 单测：`cargo test -p yu-format --lib codec::` → 8 passed。
- 本文档只挂账，不改默认行为。
