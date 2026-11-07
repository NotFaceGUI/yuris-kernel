# 任务书 P0 · 读取链与 codec 对齐官方引擎

- 状态: **部分落地(2026-11-06)**;见 §实施进度
- 依据: 2026-11-04 REA/Ghidra 12.1.4 取证(v494 `<game-b>` 主程序,
  sha256 `1ff3135d57b5bf941bc52682411dfb46ba6437125e0ca33a9393e570c508cea8`)
- 目标: 让 `<repo>` 的**文件读取顺序**、**散盘/封包优先级**、**文本 codec**
  与官方引擎一致; 解决「官方 `_chs.exe` 显示中文、我们的内核显示日文」。
- 纪律: A5(零硬编码)、A3(确定性)、A1(数据源不可见); 不猜未取证字段;
  每个结论必须带 REA 地址或真实数据锚点。

> **2026-11-07 范围修订（工具取证 + 实机验收后）**
> 市面翻译产物分两条路线：**全局编码替换**（与官方 `_chs.exe` 换首字节表
> 同构，整包一个编码）与 **SJIS 隧道**（`sjis_ext.bin`）。
> P0 的 codec 为**三域** `script` / `text` / `name`，并支持隧道表；
> **逐文件/逐载荷配置已删除**——实测其语义不成立（绘制发生在系统脚本里，
> 只按当前脚本取 codec 会永远落到默认），且与官方 CHS 不同构。
> 工具取证见 `docs/divergences/DIV-RT-017-sysvar-abi-tables.md` 与
> `crates/yu-engine/src/patch.rs` 模块文档。

### 实施进度(2026-11-06)

| 步 | 状态 | 落地内容 | 未完成 |
|---|---|---|---|
| S1 `SourceOrder` | ✅ | `project.rs` 枚举 + `push_sources()`;默认 `LooseFirst`;单测 9 条 | — |
| S2 统一 `open_source`/`mount` | ✅ | 两条路径共用 `push_sources`;散盘覆盖包 | `DirSource` 仍构造期全量索引(性能风险) |
| S3 `Yscf` 原始开关 | ◐ | `file_priority_raw: [u32; 7]` 已解析 | trace/dataset 输出未接;消费者未定位 |
| S4 三域 codec | ◐ | `CodecConfig`/`StringDomain`;CLI `--script/text/name-codec`;`BootConfig.codecs` | `name_codec` 未接 VFS;`per_file` 未用于运行时切换 |
| S5 text codec 自动采样 | ✅ | 新 `yu-format::translation` 自动识别(文本池/脚本池/逐站点/逐文件) | manifest 优先级未接(P1-C) |
| S6 `TextBoundary` | ◐ | `TextState`/`yu-gfx`/`yu-render` 文本域走 codec;`yu-font/cp932.rs` 删除 | `STRLEN`/`_strmid` 等通用字符串操作仍 CP932;`Value::Str` 未带域标签 |

自动识别入口:`Project::detect_translation()` / `detect_translation_with_key()`;
真实样本回归:`crates/yu-engine/tests/translation_patch.rs`(`<game-b>` 判
`mixed/text=gbk`,ERIS 判 `none/cp932`)。

---

## 0. 一句话结论

官方引擎不是“只读封包”，也不是“chs 特殊通道”。它对 `ysbin\yst%05d.ybn`
有一条**四源读取链**，默认顺序是 **额外路径 → 本机散盘 → YPF 封包**。
`_chs.exe` 只改了 SJIS→GBK 首字节表和字体，没有改读取链。

我们的 `Project::open_source()` 目前只挂 `data.ypf`，等于把这条链截成了
“只读包”；同时 `CODEC = Cp932` 写死。两处叠加，导致：

```text
官方: ysbin/yst00127.ybn (GBK 中文) 先命中 → 显示中文
我们: data.ypf/yst00127.ybn (CP932 日文) 唯一命中 → 显示日文
```

---

## 1. 取证事实(REA, v494)

### 1.1 脚本读取链: `FUN_0045177d`

`FUN_0045177d`（`ysbin\yst%05d.ybn` 的装载入口）内有一个 4 项优先级表：

```c
if (DAT_0080288c == 1)  order = [0, 1, 2, 3];
else                    order = [0, 1, 3, 2];
```

四个源函数：

| order 值 | 函数 | 实际来源 |
|---|---|---|
| 0 | `FUN_0040db54` | 额外搜索路径表 A（`DAT_005eb360`，4 组 × 0x201） |
| 1 | `FUN_0040dafc` | 额外搜索路径表 B（`DAT_00802b30`） |
| 2 | `FUN_0040d820` | **YPF 封包**：MurmurHash2 查表 + 读取/解压 |
| 3 | `FUN_0040d7fc` | **本机文件系统**：`GetFileAttributesA` |

命中后按 source type 调读取器：

```text
type 1 → FUN_0043fa84   普通文件读（malloc + fread 形态，已核对）
type 2 → FUN_0043f194   封包读 + zlib 解压（已核对）
type 3 → FUN_0043f11c   源 3 读（具体形态待核对）
type 4 → FUN_0043f09c   源 4 读（已见 DAT_005e9260 表；来源待核对）
```

### 1.2 散盘与封包的判据

- **散盘**：`FUN_0040d7fc` 只做 `GetFileAttributesA(path)`；存在且不是目录
  ⇒ 返回 1，source type = 1。
- **封包**：`FUN_0040d820` 对请求名算 MurmurHash2，与 `DAT_00802af0` 的条目
  哈希比较；命中后取 `DAT_00802ae4`/`DAT_00802af4` 的条目，走
  `FUN_0045ea08` / `FUN_0045ea84` / `FUN_0045ea98` 读压缩或原样数据。
  source type = 2。

### 1.3 封包发现: `FUN_0045eb34`

- `FindFirstFileA("*.*")` 递归扫描；
- 只递归白名单目录（`PTR_PTR_005b4730`，即 `pac`）；
- 文件后缀 `.ypf`（`DAT_00808ce0`）：
  - `update1.ypf` … `update9.ypf` 作为增量层登记；
  - 其余作为基础包，调 `FUN_0045e820` 校验并加入索引。
- `FUN_0045e820` 校验魔数 `0x465059`（`"YPF"`）与版本（494 或 ≥234）。

### 1.4 `yscfg.ybn` 走同一条读取链

`FUN_0045d950` 读 `ysbin\yscfg.ybn` 时同样使用上述四源优先级。读到后：

- `0x10/0x14` 宽高、`0x18` 开关、`0x1C/0x24` 扩展名顺序、`0x28` flag；
- `0x2C..0x44` 的 7 个 u32 被复制到全局：

```text
0x2C → DAT_00802888
0x30 → DAT_00802898
0x34 → DAT_008028bc
0x38 → DAT_008028c0
0x3C → DAT_008028c4
0x40 → DAT_008028c8
0x44 → DAT_008028cc
```

这些是 `filePriority*` 一类的文件源开关；**具体字段名与消费者尚未逐一取证**，
P0 只把它们作为原始开关读入 trace，不猜测语义(见 §4.3)。

样本 `yscfg.ybn` 的原始值（用于回归对拍）：

```text
0x2C=0, 0x30=1, 0x34=1, 0x38=0, 0x3C=0, 0x40=0, 0x44=0
```

### 1.5 模式标志 `DAT_0080288c`

- PE 静态初值：**0x00**；
- 已抽查 6 处引用均为 `CMP byte ptr [0x80288c], 1` 读取；其余引用未逐一确认；
- 运行期写入点尚未定位（推测来自启动模式/`SYSTEMMODE`/其他初始化，
  不是 `yscfg.ybn`，因为它在 `yscfg.ybn` 之前就被读取）；
- 该样本实际能读到 loose 中文，说明运行时值为 **0**：

```text
0 → order = [0,1,3,2] → 散盘先于包
1 → order = [0,1,2,3] → 包先于散盘
```

### 1.6 `_chs.exe` 没有文件通道

`<game-b>_chs.exe` 与原件只差 416 字节：

- v494 首字节判定表从 SJIS 范围改为 GBK 范围；
- PE 导入表新增 `MyDll.dll`（`setdll`）。

`MyDll.dll` 导入表只有 `CreateFontIndirectA`（GDI32）与内存/线程 API，
没有 `CreateFileA` / `ReadFile` / 文件查找 hook。所以中文来自引擎原生散盘源，
不是 chs 的“特殊读取通道”。

---

## 2. 现状与差距

| # | 官方行为 | `<repo>` 现状 | 影响 |
|---|---|---|---|
| 1 | 默认散盘先于包（`DAT_0080288c=0`） | `Project::open_source()` 只挂包 | 译文永远读不到 |
| 2 | `yscfg.ybn` 的 `0x2C..0x44` 控制文件源 | `Yscf` 保留为 `switches`/`unknown_34`，不解析 | 无法按游戏配置切换 |
| 3 | 包先/散盘先由模式决定 | 无 `SourceOrder` 状态 | 无法支持 mode 1 |
| 4 | 全局 codec 由引擎表补丁决定（GBK） | `CODEC = Cp932` 写死；无 `--codec` | loose 挂上也解错 |
| 5 | `ysc/ysl/ysv/yscfg` 也走同一读取链 | `open_source()` 只读包 | loose 表被忽略 |
| 6 | 源 3/4 额外搜索路径 | 未实现 | 非零售场景缺一块 |
| 7 | 路径/名字按 CP932 字节匹配 | `DirSource` 用全局 codec 折回 Unicode 名 | codec 切 GBK 后日文名会错 |
| 8 | 读取顺序/codec 可观察 | trace 无 source_order 记录 | 无法二分 |

---

## 3. 范围

### 3.1 P0 必做

- **P0-A · 源顺序**
  `Project` 增加 `SourceOrder { LooseFirst, PackFirst }`；
  默认 `LooseFirst`（证据：`DAT_0080288c` PE 初值 0）；
  `open_source()` 与 `mount()` 共用同一套叠层顺序。

- **P0-B · yscfg 原始开关**
  `Yscf` 增加 `0x2C..0x44` 的原始 7×u32 字段与 trace 输出；
  `SourceOrder` 的来源优先级：
  `CLI 显式 > 已验证的 yscfg 开关 > 默认 LooseFirst`。
  未验证前，yscfg 开关只记录、不改变顺序。

- **P0-C · 运行时 codec（三域）**
  `ProjectConfig` 增加 `script_codec` / `text_codec` / `name_codec`：
  - `text_codec`：`WORD`/`SPEAKER`/选项/人名定义/文本绘制命令；
  - `script_codec`：普通 VM 字符串（默认 CP932；全局 GBK 工程才设 GBK）；
  - `name_codec`：资源路径/包内名字（默认 CP932/ASCII）。
  CLI：`--text-codec auto|cp932|gbk`、`--script-codec cp932|gbk`、
  `--name-codec cp932|gbk`。`auto` 只用于 `text_codec`，用
  `yu-format::codec::auto::detect` 采样生效 `WORD` 文本。
  `Project::CODEC` 常量删除。

- **P0-D · 文本边界按域 codec 化 + 字符串域标签**
  `Value::Str` 增加 `StringDomain { Script, Text, Path }`（默认 `Script`；
  `_TEXT`、`WORD`、`_`、选项、人名定义、文本绘制命令标 `Text`；
  资源路径标 `Path`）。`TextBoundary` 按域选 codec；
  VM 的 `_TEXT`/`STRLEN`/`SUBSTR`、`_TEXT_FLAG`、gfx 排版、显示解码、
  存档写回全部按域走。跨域拼接先记录 `rt.text_domain_mixed` 并转
  `Text`（P0 可接受的一次性 Unicode 转换），P1 再定完整策略。

### 3.2 非目标（P1/P2，另开任务书）

- `yscfg.dat`（128 字节）完整解析；
- 源 3/4 额外搜索路径的填充规则；
- **SJIS 隧道**（`sjis_ext.bin` / 字符表映射）；
- **补丁识别**:`PatchKind`(auto/original/gbk/tunnel) + `codecs.json` 全局三域；
- 翻译构建工具（全量转码、符号归一化、人名定义、选项）；
- 字体回退/覆盖报告；
- `DAT_0080288c` 运行期写入点的最终定位。

> 注意：P0 完成后，完整 GBK 工程与该样本主线会显示中文；未翻译的包内脚本
> （125/126 及 136–159 的部分）仍是 CP932 `Text` 载荷，在全局 GBK
> `text_codec` 下会乱码。这与官方 `_chs.exe` 同型，**不是 P0 回归**；
> 补丁识别与编码路线见 `crates/yu-engine/src/patch.rs`。

---

## 4. 设计

### 4.1 `SourceOrder`

```rust
pub enum SourceOrder {
    /// 官方 DAT_0080288c=0：额外路径 → 散盘 → 包
    LooseFirst,
    /// 官方 DAT_0080288c=1：额外路径 → 包 → 散盘
    PackFirst,
}

pub struct Project {
    // ...
    source_order: SourceOrder,
}
```

`Project::discover` 默认 `LooseFirst`；`--source-order auto|loose-first|pack-first`
可覆盖。`auto` 在 P0 阶段等于 `LooseFirst`，并在 trace 中标记
`source_order_origin="default"`（附 `default-pe-initial` 说明）；等 yscfg
映射取证完成再改为 `yscfg` 驱动。

### 4.2 统一 `open_source()` / `mount()`

`Mount` 是**后挂覆盖**（`yu-vfs/src/mount.rs`）。顺序必须按 `SourceOrder`：

```text
LooseFirst:
  base packs → update1..9 → loose dir → overlay
PackFirst:
  loose dir → base packs → update1..9 → overlay
```

`open_source()` 现在只在无包时挂 loose；改为与 `mount()` 共用
`push_sources(&mut mount)`：

```rust
fn push_sources(&self, mount: &mut Mount) -> Result<(), Failure> {
    match self.source_order {
        SourceOrder::LooseFirst => {
            push_packs(mount)?;
            push_loose(mount)?;
        }
        SourceOrder::PackFirst => {
            push_loose(mount)?;
            push_packs(mount)?;
        }
    }
    mount.push(Box::new(self.overlay.clone()));
    Ok(())
}
```

`mount()` 直接调用同一函数，避免脚本和贴图走两套顺序。

### 4.3 `Yscf` 原始开关

`crates/yu-format/src/yscf.rs` 当前已解析 `0x2C..0x34` 为 `switches`、
`0x34..0x48` 为 `unknown_34`。P0 增加：

```rust
pub struct Yscf {
    // ...
    pub file_priority_raw: [u32; 7], // 0x2C,0x30,0x34,0x38,0x3C,0x40,0x44
}
```

trace header/dataset info 输出这 7 个值。**不据此改变 `SourceOrder`**，
直到 REA 把 `DAT_00802898`/`DAT_008028bc` 等消费者逐一定位；届时另开
DIV/任务书记录字段名与映射。

### 4.4 运行时 codec（三域）

```rust
pub enum CodecSource { User, Auto, Default }

pub struct CodecConfig {
    /// 普通 VM 字符串：默认 CP932；全局 GBK 工程才设 GBK。
    pub script_codec: Codec,
    /// 文本域：WORD/SPEAKER/选项/人名定义/文本绘制。
    pub text_codec: Codec,
    /// 资源路径/包内名字：默认 CP932/ASCII。
    pub name_codec: Codec,
    pub text_codec_source: CodecSource,
}
```

- CLI：`--text-codec auto|cp932|gbk`、`--script-codec cp932|gbk`、
  `--name-codec cp932|gbk`。
- 缺省：`script_codec=cp932`、`name_codec=cp932`、`text_codec=auto`。
- `auto` 只作用于 `text_codec`，采样顺序：
  1. 先列 loose `ysbin/yst*.ybn`（脚本号排序），再列生效包内
     `ysbin/yst*.ybn`（脚本号排序）；
  2. 逐个 `Segments::decrypt`，只取 `WORD`/选项的文本载荷；
  3. 累计到固定上限（建议 64 KiB）即停；
  4. 调 `yu_format::codec::auto::detect(&samples)`；
  5. 结果与两项分数写入 trace；平局回退 `script_codec`（默认 CP932）。
- 采样失败/无样本：显式 `codec.auto.no_samples` 诊断，回退 `script_codec`，
  不得静默“空跑通过”。
- 全量 GBK 工程：`--script-codec gbk --text-codec gbk --name-codec cp932`；
  只翻 WORD 的补丁：`--script-codec cp932 --text-codec gbk`。
- 所有 `CODEC` 常量调用点按域替换：
  - `project.rs`/`engine.rs`/`keys.rs` 的路径键用 `name_codec`；
  - VM 普通字符串用 `script_codec`；
  - 文本缓冲/显示用 `text_codec`。

### 4.5 文本边界按域 codec 化 + 字符串域标签

```rust
pub enum StringDomain { Script, Text, Path }

pub struct TextValue {
    pub domain: StringDomain,
    pub bytes: Vec<u8>,
}
```

- 默认 `Script`；`_TEXT`、`WORD`、`_`、选项、人名定义、文本绘制命令
  标 `Text`；资源路径标 `Path`。
- `TextBoundary` 按域选 codec：

```rust
pub trait TextBoundary {
    fn is_lead(&self, b: u8) -> bool;
    fn char_len_at(&self, bytes: &[u8], i: usize) -> usize;
    fn spans(&self, bytes: &[u8]) -> Vec<(usize, usize)>;
    fn char_count(&self, bytes: &[u8]) -> usize;
    fn slice_chars(&self, bytes: &[u8], start: usize, end: usize) -> Vec<u8>;
}
```

- `_TEXT`/`STRLEN`/`SUBSTR`/`_TEXT_FLAG` 用 `text_codec`；
  普通字符串操作/比较/存档用 `script_codec`；路径查找用 `name_codec`。
- **控制码不参与解码**：文本缓冲先按 `_TEXT_FLAG` 拆成
  `TextUnit::Char` / `TextUnit::Control`（`EF F0/F2/F3` 等），
  `TextBoundary` 只处理 `Char`；GBK 下 `EF F0` 不得被当正文解码。
- 跨域拼接：P0 记录 `rt.text_domain_mixed`，按 `Text` 处理：
  两域各自解码后用 `text_codec` 重新编码；不可编码记
  `rt.text_domain_unencodable` 并保留原 `Script` 字节；
  完整策略（不丢失字节身份）归 P1。
- 替换点：
  - `crates/yu-rt/src/cp932_text.rs` → `TextBoundary` + 域 codec；
  - `crates/yu-rt/src/text.rs:96` `is_lead` → `text_codec`；
  - `crates/yu-gfx/src/text.rs:451` `is_cp932_lead` → `text_codec`；
  - `crates/yu-render/core/src/text.rs:295/569/737/809/931` 的
    `cp932::decode` → `text_codec.decode`（统一 `yu-format` 码表，
    不再用 `encoding_rs::SHIFT_JIS` 第二真源）；
  - `crates/yu-font/src/cp932.rs` 删除/收编；`yu-font` 只吃 `char`；
  - `crates/yu-sound/src/state.rs:1686` 文件名字符步进 → `name_codec`；
  - `crates/yu-save` 按字符串域选 codec 写回。

### 4.6 trace

`HeaderPayload` 已有 `codec` 字段；P0 增加：

```text
source_order: "loose-first" | "pack-first"
source_order_origin: "default" | "cli" | "yscfg"
script_codec: "cp932" | "gbk"
text_codec: "cp932" | "gbk"
name_codec: "cp932" | "gbk"
text_codec_source: "auto" | "cli" | "default"
text_codec_scores: { cp932: ..., gbk: ... }
file_priority_raw: [ ..7 u32.. ]
loose_scripts: N          // loose ysbin 命中数(审计)
text_domain_mixed: N      // 跨域拼接计数(P0 只记录)
```

`DatasetInfo`/`Project::describe` 同步输出，便于 `--trace` 二分。

---

## 5. 实施步骤

| 步 | 内容 | 文件 | 验收 |
|---|---|---|---|
| S1 | `SourceOrder` + `push_sources` | `yu-engine/src/project.rs` | 单测：loose 覆盖 pack；pack-first 反向 |
| S2 | `open_source()` 与 `mount()` 统一 | 同上 | `<game-b>` `yst00127` 能打开 |
| S3 | `Yscf.file_priority_raw` + trace | `yu-format/src/yscf.rs`、trace | 7 值与包内 `yscfg` 一致 |
| S4 | 三域 codec + CLI | `yu-cli/src/run.rs`、`yu-engine/src/project.rs`、`engine.rs`、`keys.rs` | `--text-codec gbk --script-codec cp932` 生效 |
| S5 | `text_codec` auto 采样 + trace | `yu-format/src/codec/auto.rs` 调用侧 | `<game-b>` auto=GBK；ERIS auto=CP932 |
| S6 | `StringDomain` + `TextBoundary` 收口 | `yu-rt`、`yu-gfx`、`yu-render`、`yu-font`、`yu-sound`、`yu-save` | 全仓无 `is_cp932_lead`/`encoding_rs::SHIFT_JIS` 旁路 |
| S7 | 回归/文档 | 测试、`PROGRESS.md`、DIV | 见 §6/§7 |

---

## 6. 测试

### 6.1 单元/集成

- `source_order_loose_first_overrides_pack`：临时目录放同名
  `ysbin/yst00001.ybn`，断言读到 loose 字节。
- `source_order_pack_first_overrides_loose`：`--source-order pack-first`
  断言读到 pack 字节。
- `loose_ysbin_only_when_present`：loose 无该文件时回包，不报 missing。
- `yscfg_file_priority_raw`：用样本 `yscfg.ybn` 断言 7 个原始值。
- `text_codec_auto_game`：固定采样，断言 `<game-b>` 选 GBK（记录分数）。
- `text_codec_auto_eris`：断言 ERIS 选 CP932。
- `word_only_gbk_synthetic`：WORD/选项 GBK、其他字符串 CP932，
  断言 `text_codec=gbk, script_codec=cp932` 下两边都正确。
- `text_boundary_gbk`：`你`(C4 E3) 在 GBK 下 char_count=1、
  `spans` 正确；CP932 下不误判。
- `string_domain_tag`：`WORD`/`_TEXT`/选项/人名定义标 `Text`；
  普通字面量标 `Script`；路径标 `Path`；跨域拼接计数 `text_domain_mixed`。

### 6.2 真实样本

| 样本 | 期望 |
|---|---|
| `<game-b>` loose `ysbin/yst00127.ybn` | 用 GBK 解出 `作品中登场的地名…`；内核读 loose 而非 pack |
| `<game-b>` `data.ypf` 的 `yst00127.ybn` | 仍为 CP932 日文（作为对照，不应被优先选中） |
| ERIS 主语料 | 无 loose 时仍走包；auto=CP932；逐帧 golden 不回归 |

### 6.3 审计

- 启动日志/trace 必须出现：
  `source_order=loose-first`、`script_codec=cp932`、`text_codec=gbk`、
  `text_codec_source=auto`、`loose_scripts=N`、`file_priority_raw=...`。
- 任何 `fmt.codec.invalid`、`text.decode_invalid`、`text_domain_mixed`
  必须计数并可见；不允许静默替换。

---

## 7. 验收

1. `cargo build --workspace --all-targets` 通过。
2. `<game-b>` 运行到免责声明，显示中文（截图或文本采集入 case）。
3. ERIS 主语料 `--mode headless` 帧序列与改前一致（A3 回归）。
4. `--text-codec auto` 在 `<game-b>` 选 GBK、在 ERIS 选 CP932，
   trace 有分数；`--script-codec`/`--name-codec` 显式覆盖生效。
5. `--source-order pack-first` 可强制读包（日文），证明顺序可观测。
6. 全仓 grep：运行核/域状态机无新增游戏常量；`is_cp932_lead` 与
   `encoding_rs::SHIFT_JIS` 旁路归零。
7. `PROGRESS.md` 追加成果；相关 DIV 有勘误/新档。

---

## 8. 风险与未决

| 项 | 现状 | 处置 |
|---|---|---|
| `DAT_0080288c` 运行期写入点 | 未定位；PE 初值 0 | P0 用默认 LooseFirst；写入点另开 REA 任务 |
| `0x2C..0x44` 字段名/消费者 | 未逐一取证 | P0 只记录原始值；不据此切顺序 |
| 源 3/4 额外搜索路径 | 未实现 | P2；先确认零售包不依赖 |
| 部分翻译补丁 | 只翻译一部分脚本 | 全局编码下未翻译脚本必然乱码（与官方 CHS 一致），需整包翻译 |
| SJIS 隧道 | 未实现 | P1；`text_codec=cp932` + `sjis_ext.bin` 映射 |
| `Value::Str` 域标签 | 目前无 tag | P0 加 `StringDomain`；跨域拼接只记录并转 Text，P1 完整策略 |
| 文本边界层放哪 | `yu-format` 或新 `yu-text` | 开工前定：优先 `yu-format` 复用码表；若 `yu-font` 需要，再抽 crate |

---

## 9. 台账/文档更新

- `PROGRESS.md`：成果条目 + 验证命令/原始输出；
- `docs/architecture.md` §7：把“单一 codec”改成“script/text/name 三域”
  说明，并记录 P0 的 `source_order` 与 trace 字段；
- `docs/divergences/`：若发现官方顺序/字段与旧文档不一致，建 DIV；
- `docs/references/open-source-tools.md`：已补 2026-11-06 工具取证与
  §2.5 四形态对照；
- `crates/yu-engine/src/patch.rs`：补丁路线识别与 `PatchProfile` 定义。

---

## 10. 交付物

- 代码：S1–S6；
- 测试：§6 全部；
- 文档：本任务书 + `PROGRESS.md` + `architecture.md` 更新；
- 证据：REA 地址清单（§1）与 `<game-b>` 免责声明解码样本。
