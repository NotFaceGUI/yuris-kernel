# DIV-RT-081 · 宿主路径按 UTF-8 注入,脚本/存档按 CP932 解出乱码

- 状态: fixed(含非 CP932 字符;比较走投影、I/O 回真实根)
- 优先级: 高(含日文目录名的工程存档全部失效)
- 发现日期 / 发现方式: 2026-11-25 / `<game-c>` 日志 `sv.path.escape`

## 复现

- 游戏位置: `<game-c>`(工程根目录含日文)
- 锚点: 启动后 `sv@frame1854 pc0`:
  `绝对路径不在 save 根内: D:/.../迪ｫ蠢阪∴…/save/save30002.sd`

## 官方行为

官方是 win32 ANSI 程序:路径系统变量(`$_SAVE_PATH` 等)以**系统代码页**
字节交给脚本;`FILEACT/SAVE` 用同一代码页还原文件名。日文 Windows 上
即 CP932。脚本字符串本身也是 CP932 原始字节。

## 内核行为

`host_paths::script_dir` 旧实现 `String::into_bytes()`(UTF-8)直接注入。
含日文的存档根(例:`D:/games/<game-c>/save/`)的 UTF-8 字节被脚本/
存档层按 CP932 解码,得到 `迪ｫ蠢阪∴縺上☆縺ｯ繝ｼ縺ｨ…` 一类乱码,与真实
save 根不相等,
`sv.path.escape` 拒绝写入。

## 根因

宿主注入口与脚本字符串编码不一致:内核全链路用 CP932,宿主给了 UTF-8。

## 修法

`script_text_bytes()`:先 `Codec::Cp932.encode`;失败时逐字符编码,
不可表示字符按 Windows ANSI 口径写 `?`。绝不把 UTF-8 字节伪装成 CP932。

## 回归

- `crates/yu-engine/src/host_paths.rs::tests::non_ascii_save_root_is_injected_as_cp932`
- 命令: `cargo test -p yu-engine --lib -- host_paths` → 4 passed。
- `<game-c>` 桌面启动:不再出现 `sv.path.escape`,save 路径按日文原名显示。

## 残余

`?` 兜底意味着**简体中文/其它 CP932 不可表示字符**在脚本可见字节里仍是
`?`(`<v555-2>` 等中文目录名实测 5 条 `sv.path.escape`)。正确口径应跟随
系统 ANSI(本机 CP936)或让宿主/存档层共用同一编码参数,单列跟进。

## 2026-11-25 续(E2 收口:投影比较 + 真实根 I/O)

- 新增 `Codec::encode_lossy`(`yu-format`),与 `host_paths::script_text_bytes`
  同一口径;`yu-save` 的 `confined_path` 先用 `script_visible_root()` 把
  真实 save 根投影成脚本可见形式(`?` 保留),按该前缀匹配脚本传来的绝对
  路径,再把相对部分 `self.root.join(rel)` 接回真实 Unicode 根做实际 I/O。
- 安全边界不变:仍拒绝 `..` 与投影根的祖先;`?` 只参与比较,不会成为
  绕过路径校验的后门。
- 回归:`crates/yu-save/src/state.rs::tests::confined_path_accepts_cp932_projected_root`
  (`cargo test -p yu-save --lib`)。
- `<v555-2>` 桌面启动 200 帧:启动期 `sv.path.escape` 由 4 条降为 **0**。
- 残余(语义层):脚本如果**显示/比较** `$_SAVE_PATH` 字面值,看到的仍是
  `?` 投影而非官方在中文 Windows 上的 CP936 字节;当前商业脚本只拿它拼
  save 路径,未观察到可见分歧。若要 1:1 复刻 ANSI 行为,需要把「宿主路径
  注入编码」与「脚本文本编码」解耦,单列后续。
