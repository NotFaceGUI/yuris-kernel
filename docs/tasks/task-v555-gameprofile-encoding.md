# 任务书 · v555 GameProfile / 编码对齐 / 正文文本修复

> **启动提示词（新窗口先读这一段，再动手）**
>
> 你接手 `<repo>`，目标是 v555 独立运行面的 1:1 对齐。**
> 开工前必须读：`AGENTS.md`、本任务书、`docs/formats/v555-truth-2026-11-21.md`。
>
> 铁律，任何一条不满足就停手挂账，不许“先写再说”：
>
> 1. **旧版本行为必须与 `c1549eb` 一致**：0.479 / 0.481 / 0.486 / 0.488 / 0.494 / 0.495
>    以及任何 `ysc_version != 555` 的工程，不得改变现有行为。
> 2. **新语义只允许精确 `ysc_version == 555`**，经 `SemanticProfile` 门控；
>    禁止 `>=555`；556/600 不得继承 555。
> 3. **禁止游戏名/品牌/文件名特判**；差异只能落在 `other` 运行面或数据 Profile。
> 4. **无证据 → `Unimplemented` / 挂账，禁止猜**；实现处必须写证据锚点
>    （RE 地址 / trace 编号 / 官方产物字节）。
> 5. **不复制 `<repo-old>` 代码**；事实可引用并注明出处。
> 6. **不做 git 写操作**；不改游戏数据、`msic/`、`yu-emote`。
> 7. **不许“顺手重构”无关模块**；每个提交/改动必须能单独编译、单独回归。
>
> 本任务书之外的自由发挥一律视为越界。

---

## 0. 现场摘要

### 0.1 已落地（本轮之前）

- `SemanticProfile`：`TextMode` / `TextInfoMode` / `RefIntParamMode` / `partial_index`
  均由精确 555 派生；旧版本保持原值。
- `crates/yu-rt/src/other/gosub.rs`：v555 GOSUB 对 ERIS 旧式数组
  （`#` / `` ` ``）传**槽号**；`@` 普通数值数组仍按值。
- `crates/yu-rt/src/value.rs`：555 允许**前缀下标**（`@1177[110,19]` 这类二维访问）。
- `crates/yu-engine/src/patch.rs`：v555 `Auto` 扫描 exe 的 256B 静态 lead 表，
  自动判 Big5 / GBK / CP932；只在 555 + Auto 生效。
- `crates/yu-platform/movie-ffmpeg`：桌面外置 FFmpeg MovieHost（宿主能力，非本任务重点）。
- 回归：`yu-engine` lib 65 项、`yu-rt` fixture 144 项通过；
  0.495 工程仍是 `kind=original source=default`。

### 0.2 关键未决

1. **`<v555-sample>`（v555 汉化/日文双 exe）**
   - 自动 `original` 在 `userdefine/*.txt pc3` 报“字符串字面量未闭合”；
   - `--patch gbk` / `--patch big5` 都推进到 `pc13`，报：
     `rt.expr.malformed: 操作数越界: op=0x4d 偏移 0 声明 12 字节,窗口共 11`
   - 这是 v555 userdefine 的**窗口/字符串字面量边界**问题，不是单纯编码选择。
2. **v555 多语言显示**
   - exe 静态表只是“执行编码”；正文显示应按语言切 JP CP932 / CN GBK / TW Big5。
   - 现在 `PatchProfile.text_default` 是单值；`GfxState` 已加 `text_codec` 字段和
     `set_text_codec()`，但**引擎尚未接线**。
3. **正文文本“一个一个出现又消失”**
   - 需抓 `es_text.yst` / `es_texttask.yst`、`TEXT CLEAR`、`CGACT[TEXT]` 的时序。
4. **`<v500-sample>`（暂缓）**
   - `YSCM version=500, count=121`，命令名与 v555 的 121 槽一致
     （含 `MOVIE2` / `SYSTEMMODE` / `COMPILEMODE` / `RELEASEMODE` /
     `PROJECTFOLDER` / `PRODUCT`），但 handler 语义未对齐；
     0.500 必须独立 `SemanticProfile` 族，不能直接套 555。
   - `pac/*.ypf` 是 YPF v500，`ysc.ybn` 载荷是 zlib；当前 `YpfSource`
     卡在校验/多段，未实现。
   - 汉化是 <patch-group> 外挂重定向 + 字体 Hook，不是改原版 YPF；后续再做。

### 0.3 临时改动 / 待清理

- `YU_MACRO_TRACE` / `YU_PC_TRACE` 以及 `read1177` / `write1177` /
  `declare1177` / `[let]` / `[sysvar]` / `[pc65]` 打印（env 门控，收尾删除）。
- 测试期间移动的存档要还原（路径见 `AGENTS.md` 占位符）：
  - `<games-555>/<v555-2>`：`save.bak-yk-20261122`、`save.clean-lang1`、`save.scripted-en`
  - `<games-555>/<v555-3>`：`save.bak-yk-20261122`、`save.test-run`
  - `<games-555>/<v555-1>`：`save.bak-yk-20261122`
  - 检查 `<v555-sample>` 是否因测试新建了 `save/`，不要误删原档。
- `crates/yu-gfx/src/state.rs` 的 `text_codec` 字段已加但未接线；
  要么在 T1 完成，要么回退到 `config.display.default`。

---

## 1. 目标

1. 落地 **GameProfile**：执行编码、资源名编码、语言显示编码三者分离。
2. 用实际 v555 资源跑通：
   - `<v555-sample>` 正常启动；
   - `<v555-2>` / `<v555-3>` / `<v555-4>` 的 CN / TW / JP 显示正确；
   - 正常游戏不再需要手工 `--patch`（manifest 显式覆盖除外）。
3. 修正文文本显示：逐字出现后正常累积，不逐字消失。
4. 旧版本回归全绿，行为不变。

---

## 2. 任务分解

### T1 · GameProfile v1

- 读：`crates/yu-engine/src/patch.rs`、`project.rs`、
  `crates/yu-rt/src/semantics.rs`、`crates/yu-gfx/src/state.rs`。
- 数据模型：
  - `execution_codec`：构建静态表（exe 的 256B lead 表）；
  - `name_codec`：资源路径（通常 CP932）；
  - `languages: { jp: {text_codec, prefix}, cn: {...}, tw: {...} }`。
- 优先级（不猜）：
  1. `game-profile.json` / 现有 `codecs.json`（汉化工具可直接产出）；
  2. exe 静态表；
  3. 目录布局（`scenario/`、`sc_cn_scenario/`、`sc_tw_scenario/`、
     `cgsys_cn/`、`cgsys_tw/`）；
  4. 默认值；有歧义显式记录。
- 只在精确 555 启用；旧版继续 `PatchKind`，`Auto` 行为不变。
- 运行时：
  - 启动只用 `execution` / `name`；
  - 每 tick 观察 `@1000/@1001/@1002`，切当前语言 `text_codec`
    （`GfxState::set_text_codec` 已存在，接线即可）；
  - 资源前缀由脚本自己选，内核不替它选。
- 测试：合成 v555 多语言工程；0.495/0.479 的 `Auto` 输出保持
  `kind=original source=default`。

### T2 · v555 userdefine / 窗口解析（`<v555-sample>`）

- 用 `--patch gbk` / `--patch big5` 各跑 1 帧，抓 `pc13` 窗口原始字节：
  - 打印 `op=0x4d` 的 payload 长度、窗口剩余、实际字符串字节；
  - 确认是 lead 字节吞了结尾、长度前缀差 1，还是文件编码不对。
- 对照 `es_eris.yst` / `es_scenario_load` 同类 string 窗口，
  区分“通用 555 解析”与“该构建特殊”。
- 修复只进 555 路径（`other` 或 `SemanticProfile` 门控），共享旧版不动。
- 加夹具：该窗口字节形态 + 期望解析。

### T3 · 正文文本显示

- 抓一条对白：
  - `YU_PC_TRACE` 限定 `es_text.yst` / `es_texttask.yst`；
  - 记录 `TEXT CLEAR`、`CGACT[TEXT]`、`CGEND`、层 ID、帧号；
  - 对照官方 trace（若有）确认“出现→消失”是清空对象错还是追加/重绘错。
- 重点怀疑：
  - v555 `TEXT CLEAR` 是否被我们当成“清行数组”而非“清显示缓冲”；
  - 文本层是否每字重建；
  - `WAIT` / 节奏返回值是否恒 0。
- 修复留在 v555；加回归：连续两行文本，第一行保留到换页，第二行追加。

### T4 · 回归、清理、文档

- 跑：
  - `cargo test -p yu-engine --lib`
  - `cargo test -p yu-rt --test fixture --no-default-features`
  - `cargo test -p yu-gfx`
- 旧版冒烟：0.495 工程、0.479 样本；确认 `patch: kind=original source=default`。
- 删除临时 trace；还原存档；更新 `PROGRESS.md`、
  `docs/formats/v555-truth-2026-11-21.md`、必要的 DIV 档案；不执行 git 写。

### T5 · 暂缓（格式层已部分完成）

- **已完成**：YPF v500 的 `payloadHash=xxh32(存储字节,0)` 与
  YSCF `V500` 布局（title @0x58）；证据/结果见
  `docs/formats/v500-abi-census-2026-11-22.md`。
- **已完成普查**：0.500 与 555 的 `ysc.ybn` 均为 121 条、11102B，
  仅版本字段与 `GRADATION` 参数 kind 不同；系统变量 ABI `0..999`
  1000/1000 一致。
- **已完成(2026-11-24)**:0.500 归入 555 商业族
  (`SemanticProfile::is_v555_family`)。处理体对拍:`TEXT`/`CGINFO`/
  `VARINFO`/`RETURNCODE`/`GOSUB`/`RETURN`/`LET`/`VARACT`/`SAVE`/`LOAD`
  与 v555 同构;0.500 的 `MOVIE2*`/`REGINFO` 是 29B 空桩,分发表覆盖为
  `NoopEval`;`PRODUCT[COM=1/2/3]` 按 `0x488b20` 落地。
  证据/差异见 `docs/divergences/DIV-RT-079-v500-commercial-family.md`。
- **仍未做**:`SYSTEM[SX/SY]` 的宿主屏幕尺寸/后缓冲重建、
  `PRODUCT[COM=24001/24002/24009]`(YSWEBM)与 `COM=56/57`、
  `$4780[-1]` 负下标语义。
- **仍未做**：<patch-group> 归档/重定向 + 字体 Hook。
- 视频 / FFmpeg 外置保持现状，不扩。

---

## 3. 验收

- `<v555-sample>` 无 Fatal，进入游戏并能看到正文。
- v555 多语言样本系列：CN 选简中显示 GBK，TW 选繁中 Big5，JP CP932；
  警告页 / 标题正常；不需要 `--patch`。
- 正文逐字出现后正常累积，不逐字消失。
- 旧版本行为不变；所有既有测试通过。

---

## 4. 关键命令

```bash
export TEMP="<side-repo>/workbench/tmp"
export TMP="$TEMP"

# 版本 / 补丁判定
cargo run -q -p yu-cli --no-default-features --bin yu-cli -- \
  run "<games-555>/<v555-sample>" --frames 1 --backend soft --log-level info

# 桌面验收（避免 mimalloc 构建问题）
cargo build -p yu-cli --bin yu-cli --no-default-features --features desktop
```

---

## 5. 风险 / 禁区

- v555 静态表是“执行编码”，不是“显示编码”；不要再用一个 `--patch` 合并两者。
- 0.500 命令表与 555 同名同槽，但 handler 可能不同；没有证据前不要复用 555 处理体。
- `<v555-sample>` 的失败是窗口解析，不是版本识别；不要靠换 `--patch` 掩盖。
- 旧版 `Auto` 一律不改；所有新分支必须过 555 门控。
- 遇到无证据点：停手、写清 `Unimplemented` / 挂账，不要猜。
