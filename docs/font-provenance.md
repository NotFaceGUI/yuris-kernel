# font-provenance · 字体/栅格化/FONT 参数取证

> 会话 G-FONT。证据优先级按 `AGENTS.md` §0：官方引擎可观测行为 > SDK 0.495
> 二进制逆向事实 > 原版数据自证 > 旧线 golden。
> 本文件是进入实现的硬门禁；表中每条都给出可复查的地址/手册页/脚本组号。
> 被跟踪文件中的仓库路径一律使用占位符（见 `AGENTS.md` §1）。

## 0. 结论摘要

| 问题 | 结论 | 等级 |
|---|---|---|
| 官方是否自带字体文件 | **否**。参考工程 `data.ypf` 1,407 条中 0 个 `.ttf/.otf/.ttc/.fnt/.fon`；唯一 `.psb` 是 E-mote 模型，不是字体 | Confirmed |
| 字体来源 | **系统字体**：引擎启动时用 `EnumFontFamiliesExA`（`lfCharSet=SHIFTJIS_CHARSET`）枚举，再按固定回退表选第一个已安装 family | Confirmed |
| 默认字体 family | 回退表顺序：`ＭＳ ゴシック` → `標準ゴシック` → `ＭＳ Ｐゴシック` → `ＭＳ 明朝` → `標準明朝`。参考机命中第 1 项（`msgothic.ttc`，fontdb 同时给出 `MS Gothic` / `ＭＳ ゴシック`） | Confirmed |
| 栅格化路径 | GDI：`CreateFontIndirectA` + `TextOutA` 画到 8-bit DIB，再读 coverage | Confirmed |
| 是否 ClearType / 自绘 outline | **否**。无 `GetGlyphOutline` / `ExtTextOut` / `DrawText` / `AddFontResource` / `ID3DXFont` 导入；`lfQuality` 未设置（默认 0） | Confirmed |
| BOLD | `lfWeight = 700`（普通 400），同一 face name；无 bold face 时由 GDI 合成 | Confirmed |
| ANTIALIASING | 布尔值选择 AA / 非 AA 合成器；默认 1 | Confirmed |
| SX/SY | 每轴 0..=256 px，默认 20；参考游戏对话 12×12、系统文本 14×15 | Confirmed |
| COLOR/COLOR2 | `0xRRGGBB`，默认 `0xffffff`；COLOR2 是渐变第二色 | Confirmed |
| GRADATION | 参数存在（0..=8，默认 1），但 0.495 手册无页、语料无使用；方向映射未见证据 | **Likely/Unknown** |
| ROTATION | 参数存在（0..=8，默认 0），但 0.495 手册无页、语料无使用、渲染器读取未观察到 | **Unverified** |
| BNO | 仅 ysc 表与真实脚本出现；手册无页；未解析 | **Unknown** |
| 官方参考图 | 标题画面 1200×900（用户提供），菜单 `スタート` 墨迹框 54×16 px；最佳匹配 15px bold MS Gothic，IoU 0.537 | Confirmed（图）/ Likely（字号） |

## 1. 字体文件扫描（不是字节搜索）

工具：`crates/yu-font/examples/scan_evidence.rs`，走 `yu-vfs::YpfSource::entries()`。

```text
## data.ypf
entries=1407 index_start=32 name_key=0xFF name_hash_ok=1407
types=[(0,146),(2,1245),(3,7),(6,8),(9,1)] compressed=152
FONT-LIKE type=9 compressed=false usize=8832512 csize=8832512 name=cg\stand\emote_test.psb
font_like_entries=1
extension histogram: .png 1241 / .ybn 143 / .ogg 8 / .jpg 7 / .ymv 3 / .psb 1
```

- `.psb` = E-mote 模型（type 9），不是字体/字库。
- 工程 `system/`、`save/data/`、`unpack/` 下没有 `.ttf/.otf/.ttc/.fnt/.fon`。
- 结论：**没有自带字体**；引擎只能使用系统字体。

## 2. 引擎字体枚举与默认 family

| 证据 | 内容 |
|---|---|
| REA `FUN_00403afc` @ `0x403afc` | `GetDC(NULL)` → `EnumFontFamiliesExA(hdc, &LOGFONTA{lfCharSet=SHIFTJIS_CHARSET(1), lfFaceName="", lfPitchAndFamily=0}, &LAB_004038fc, 0, 0)`，先计数再枚举；为 8 个字体槽逐一解析 family |
| REA `FUN_00403a70` @ `0x403a70` | 在枚举结果中按名字查找并设置槽的 font index |
| 字符串 @ `0x807008` | CP932 `ＭＳ ゴシック`（`82 6c 82 72 20 83 53...`） |
| 字符串 @ `0x807018` | CP932 `標準ゴシック` |
| 字符串 @ `0x807028` | CP932 `ＭＳ Ｐゴシック` |
| 字符串 @ `0x807038` | CP932 `ＭＳ 明朝` |
| 字符串 @ `0x807044` | CP932 `標準明朝` |
| 汇编 @ `0x403b8e..0x403bdc` | 依次 `MOV EAX, 0x807008/0x807018/0x807028/0x807038/0x807044; CALL 0x403a70`，命中即停止 |
| 参考机 | `msgothic.ttc` 已安装；fontdb 枚举出 `MS Gothic` + `ＭＳ ゴシック`，weight 400，face index 0 |

**不是** MS Gothic 的臆断：回退表来自引擎字节；参考机命中只是该表在当前环境的
运行结果。若主机没有上述 family，参考渲染器显式失败（`font.missing`），不退回
Noto/sans。

## 3. 官方栅格化路径（REA，0.495 `yu-ris.exe`）

目标：`<side-repo>/target/sdk/0.495/yu-ris_0495_032/システム/yu-ris.exe`
SHA-256：`4f5eefa3544c7244a907477d89ccee97286f559df5e38b1b1fa674206981138f`

### 3.1 导入表

| 导入 | 地址 | xref |
|---|---|---|
| `GDI32.DLL::TextOutA` | `EXTERNAL:0xcf` | `0x4451bc`（`FUN_004450c0`）、`0x58c028`（导入跳板） |
| `GDI32.DLL::CreateFontIndirectA` | `EXTERNAL:0xd1` | `0x445708`（`FUN_00445421`）、`0x58c030`（导入跳板） |
| `CreateFontA/W` | 不存在 | — |
| `AddFontResourceA/W` | 不存在 | — |
| `GetGlyphOutlineA/W` | 不存在 | — |
| `ExtTextOutA/W` | 不存在 | — |
| `DrawTextA/W` | 不存在 | — |
| `ID3DXFont` | 不存在 | — |

### 3.2 `FUN_00445421` @ `0x445421`（创建 per-slot GDI 字形栅格器）

关键汇编/伪代码事实：

```c
// LOGFONTA 分配 0x3c 字节，初值来自 FUN_00464fac
*(u32 *)(logfont + 0x10) = (bold ? 700 : 400); // lfWeight
*(u32 *)(logfont + 0x00) = height;             // lfHeight（栈参 +0x10）
*(u32 *)(logfont + 0x04) = 0;                  // lfWidth
copy(logfont + 0x1c, font_name);               // lfFaceName[32]
font = CreateFontIndirectA(logfont);
SetBkMode(hdc, 1);                             // TRANSPARENT
SetTextColor(hdc, 0);                          // 黑色字，读 mask
```

- `CreateDIBSection(hdc, &bmi, 1, &bits, NULL, 0)` + `CreateCompatibleDC`：把字符画进
  8-bit DIB。
- 代码未设置 `lfItalic`、`lfQuality`、`lfCharSet`（初始化时 `+0x17=1` =
  `SHIFTJIS_CHARSET`）；因此不是 ClearType，也没有斜体/质量切换。
- `lfHeight` 来自调用参数，`lfWidth=0`；SX/SY 的非等比由外层测量/缩放处理。

### 3.3 `FUN_004450c0` @ `0x4450c0`（单字符绘制 + 墨迹框）

```c
size = size_table[in_EDX];                    // DAT_007b1f40
if (FUN_00445421(...) == -1) return 0;
TextOutA(hdc, 0, 0, ch_ptr, length_table[*ch_ptr] + 1); // 每字符一次
FUN_004669fc(&bits);                          // 读 DIB mask
// 扫描 DIB 求 bbox；AA 标志由 FUN_00403a2c 读取
```

- `length_table[*ch] + 1` 是 CP932 单/双字节长度。
- 逐字符调用 `TextOutA`，每字符独立栅格化 → 引擎**没有字距/kerning**。
- AA 布尔值来自当前字体槽（见 §4），选择后续 AA/非 AA 合成器。

### 3.4 绘制合成

| REA | 事实 |
|---|---|
| `FUN_004056b8` @ `0x4056b8` | 调 `FUN_004450c0` 取得位图，再按 ANTIALIASING 调 `FUN_00406184`（AA）或 `FUN_00407cf0`（非 AA） |
| `FUN_00406184` @ `0x406184` | AA 合成器，逐像素 coverage/颜色混合 |
| `FUN_00407cf0` @ `0x407cf0` | 非 AA 合成器 |
| `0x404a1b..0x404ac7` | 按字符取 AA 标志（`[text+0x34]`）、DRAWMODE（`[text+0x33]`）等，调用字形函数 |

**结论**：GDI outline rasterizer + 8-bit mask + 引擎自己的颜色合成；不是
`GetGlyphOutline` 自绘，也不是 ClearType。

## 4. FONT 参数语义（REA + 手册 + 编译产物）

### 4.1 ysc 表（参考工程 `ysc.ybn`，opcode 27）

```text
FONT opcode=27 argc=12
 0:BNO            1:NO             2:NAME          3:ANTIALIASING
 4:COLOR          5:COLOR2         6:GRADATION     7:ROTATION
 8:DRAWMODE       9:SX            10:SY           11:BOLD
FONTINFO opcode=28 argc=19
 0:NO 1:NAME 2:ANTIALIASING 3:COLOR 4:COLOR2 5:GRADATION 6:ROTATION
 7:DRAWMODE 8:SX 9:SY 10:BOLD 11:NUM 12:PITCH 13:LANG 14:X 15:Y
 16:SET 17:SETSTR 18:LET
```

### 4.2 存储与 clamp（REA `0x444428..0x444b1c`）

FONT 处理器按 `0x684160..0x68416b` 的“本次命令是否设置该参数”顺序处理：

| 参数 | 写入 text 对象偏移 | per-slot 全局 | clamp | 默认值（`FUN_00464fac` @ 0x464fac） |
|---|---|---|---|---|
| BNO | 特殊分支 @ `0x444ce7`（未解析） | — | — | — |
| NO | font index（`0x4039e0`） | `0x5d1664` | `< 字体总数` | 0 |
| NAME | font index（`0x403a6c`） | `0x5d1664` | 名字查找 | 槽默认 family |
| ANTIALIASING | `+0x34` | `0x5d1738` | 0..=1 | 1 |
| COLOR | `+0x28` | `0x5d16e8` | 0..=0xffffff | 0xffffff |
| COLOR2 | `+0x2c` | `0x5d1708` | 0..=0xffffff | 0xffffff |
| GRADATION | `+0x30` | `0x5d1728` | 0..=8 | 1 |
| ROTATION | `+0x31` | `0x5d1730` | 0..=8 | 0 |
| DRAWMODE | `+0x33` | `0x5d1740` | 0..=1 | 0 |
| SX | `+0x20` | `0x5d1688` | 0..=0x100 | 20 |
| SY | `+0x24` | `0x5d16a8` | 0..=0x100 | 20 |
| BOLD | `+0x32` | `0x5d16c8` | 0..=1 | 0 |

汇编锚点（每条都是 `MOV ... , AL/EAX` 到上表全局）：

```text
0x4444af  MOV byte ptr [EDX+0x5d1738], AL   ; ANTIALIASING
0x44457f  MOV dword ptr [EDX*4+0x5d16e8], EAX ; COLOR
0x44464a  MOV dword ptr [EDX*4+0x5d1708], EAX ; COLOR2
0x444713  MOV byte ptr [EDX+0x5d1728], AL   ; 注意：此处是 GRADATION（0..8）
0x4447db  MOV byte ptr [EDX+0x5d1730], AL   ; ROTATION（0..8）
0x4448a3  MOV byte ptr [EDX+0x5d1740], AL   ; DRAWMODE（0..1）
0x44496d  MOV dword ptr [EDX*4+0x5d1688], EAX ; SX
0x444a38  MOV dword ptr [EDX*4+0x5d16a8], EAX ; SY
0x444afe  MOV dword ptr [EDX*4+0x5d16c8], EAX ; BOLD
```

### 4.3 手册（0.495 `font/font.html` / `fontinfo.html` / `text/text.html`）

- `NAME = フォント名`；“フォントリスト番号” 的 `NO`。
- `COLOR = 0..0xffffff`（示例 `0xffff00` 黄、`0x0000ff` 蓝 → RGB 顺序）。
- `COLOR2`：`文字色を指定(グラデーション用)`；示例 `COLOR=0xffff00 COLOR2=0x0000ff`。
- `SX`/`SY`：横/竖大小。
- `BOLD = 1/0`。
- `ANTIALIASING = 0/1`。
- `DRAWMODE = 0/1`：0 = 含透明部分一起覆盖（默认），1 = 合成覆盖。
- **0.495 手册没有 GRADATION / ROTATION / BNO 页**；它们只存在于 ysc 表与
  真实脚本。记录为“手册缺口”，不作默认语义推断。

### 4.4 参考游戏编译产物中的 FONT

扫描 136 个可解码 `yst%05d.ybn`，共 **42 个 FONT 组**；所有值均为字面量或
字面量表达式（无宏残留）。关键配置：

| 来源 | group | 参数（槽号→值） |
|---|---|---|
| `data\script\eris\text\es_text.yst` | 1517 | SX=12, SY=12, COLOR=0x999999, COLOR2=0x999999, BOLD=0, ANTIALIASING=0 |
| 同上 | 1698 | BOLD=1, ANTIALIASING=1 |
| 同上 | 1606 | COLOR=0xffffff, COLOR2=0xffff00 |
| `data\script\eris\es_system.yst` | 132 | DRAWMODE=1, BOLD=1, SX=14, SY=15 |
| `data\script\eris\es_font.yst` | 188 | NAME=`ＭＳ ゴシック` |
| `data\script\eris\sprite\es_sprite.yst` | 278 | BNO=1, COLOR=0xeeffff(15658751), COLOR2=same, BOLD=0, ANTIALIASING=0, SX/SY 非字面量 |
| `es_system.yst` | 133/135 | COLOR/COLOR2 = 0 / 0xffffff |
| `es_text.yst` | 1553/1556/1559 | 12×12；COLOR/COLOR2 = 0xffff00 / 0xfff7c4 / 0x999999 |

完整 42 组由 `render_reference` 运行时打印，并写入
`target/tmp/font-ref/render.log`；`docs/font-contract.md` 只固定上面这组。

### 4.5 GRADATION / ROTATION / BNO 的未知部分

- GRADATION：clamp 0..8、默认 1；`COLOR2` 是渐变第二色。**方向/坐标/色标未取证**。
  `docs/font-contract.md` 采用可替换的 `Gradation::Direction{octant}`，默认把
  1 解释为“竖直、上→下”，并明确标记 `Likely`；`StylePolicy::Strict` 只允许
  `COLOR==COLOR2`（视觉上等于纯色）。
- ROTATION：clamp 0..8、默认 0；引擎存储但没有观察到 0.495 文本渲染读取/使用。
  实现为“octant × 45° 顺时针、绕 pen 原点”，标记 `Unverified`，仅
  `StylePolicy::Reference` 可用。
- BNO：只在 `es_sprite.yst` 等真实脚本出现（FONT[BNO=1 …]），手册无页；
  `FONT` 处理器有独立分支 `0x444ce7`，本会话未解析其字体列表语义。`yu-font`
  只保存 `BNO` 值，不做解析（`font.unverified`）。

## 5. 官方参考图

| 项 | 内容 |
|---|---|
| 文件 | `target/tmp/font-ref/official_title_user.png`（用户提供，不入库） |
| 尺寸/格式 | 1200×900 RGB PNG |
| 画面 | 官方 0.495 运行的 ERIS 标题画面 |
| 文本样本 | 第 1 个按钮 `スタート`；墨迹框（相对 crop `150,402`）`x=136..190, y=13..29` = 54×16 px |
| 观测颜色 | 字心约 `#444583`；按钮底约 `#e5f4ff`；字缘有浅色 rim（约 `#eef7fe`） |
| 方法 | 用户直接提供原图；不再启动引擎自动截图（用户指示“用我发你直接继续”） |

### 5.1 与参考渲染的 diff

`examples/render_reference.rs` 对 `スタート` 搜索 size 12..24 × bold/normal，
在官方 crop 上按 bbox 对齐并搜索 ±2 px 偏移；用深色墨迹 mask 计算：

```text
official ink = 227 px
best = size 15, bold=true, outline=0, shadow=None
intersection = 153, false_positive = 58, false_negative = 74
IoU = 0.536842
max channel diff = 178, mean abs channel diff = 73.88
```

归因：

1. GDI outline rasterizer 与 swash/skrifa 的 hinting/AA 不同（主因）；
2. 官方菜单层有浅色 rim（约 1 px），diff 的 mask 只取深色字心，未把 rim 计入；
3. 字号取 15 px 是搜索值，官方没有暴露字号；16 px 墨迹高与 15 px 候选最接近；
4. 像素取整/基线位置：官方 GDI 用整数墨迹框，本实现用 f32 placement 后取整。

## 6. 证据表

| 项 | 证据（手册页/脚本行/REA 地址/截图） | 结论 | 等级 |
|---|---|---|---|
| 无自带字体 | `YpfSource` 1,407 条，0 个字体扩展；唯一 `.psb` 为 E-mote | 系统字体 | Confirmed |
| 字体枚举 | REA `0x403afc` `EnumFontFamiliesExA`，`lfCharSet=1` | 枚举 SHIFTJIS family | Confirmed |
| 默认 family 回退表 | 字符串 `0x807008/18/28/38/44`；调用 `0x403b8e..0x403bdc` | 5 项顺序 | Confirmed |
| 参考机字体 | `msgothic.ttc` 已安装；fontdb `MS Gothic`/`ＭＳ ゴシック` | 命中第 1 项 | Confirmed |
| 栅格器 | REA `FUN_00445421` @ `0x445421` | `CreateFontIndirectA` + DIB + `TextOutA` | Confirmed |
| 单字符绘制 | REA `FUN_004450c0` @ `0x4450c0`；`TextOutA` xref `0x4451bc` | 逐字符 8-bit mask | Confirmed |
| 无 ClearType | 导入表无相关 API；`lfQuality` 未设置 | GDI 默认质量 | Confirmed |
| BOLD | `0x4456xx` 写 `lfWeight=700/400` | 700/400 | Confirmed |
| AA 开关 | FONT handler `0x444713` 写 `0x5d1728`；默认 1；`FUN_004056b8` 分支 AA/非 AA | 布尔 | Confirmed |
| SX/SY 范围 | clamp `0..=0x100`；默认 20；ERIS 12/12、14/15 | px | Confirmed |
| COLOR/COLOR2 | clamp `0..=0xffffff`；默认 `0xffffff`；ERIS 值 | RGB | Confirmed |
| DRAWMODE | clamp `0..=1`；默认 0；手册 0/1 语义 | 覆盖/合成 | Confirmed |
| GRADATION | ysc/FONTINFO 有；clamp 0..=8；默认 1；手册无页；语料无使用 | 存储语义确认，方向未知 | Likely/Unknown |
| ROTATION | ysc/FONTINFO 有；clamp 0..=8；默认 0；手册无页；语料无使用 | 存储语义确认，渲染未知 | Unverified |
| BNO | ysc 有；`es_sprite.yst` 使用；手册无页；分支未解析 | 仅保存 | Unknown |
| 参考图 | `official_title_user.png` 1200×900；`スタート` 54×16 | 标题菜单样本 | Confirmed |
| 官方 diff | IoU 0.537、fp 58、fn 74、max 178 | 见 §5.1 | Confirmed（数值） |
| 字体文件格式 | 参考工程无位图字体；未发现 `.fnt` | 不适用位图 | Confirmed |

## 7. 复查命令

```bash
export TEMP="<side-repo>/workbench/tmp"; TMP="$TEMP"
export YURIS_SAMPLE_ERIS_ROOT="<side-repo>/workbench/project-eris"   # 数据入口:data.ypf 优先,data/ 为备
export YURIS_SAMPLE_ERIS_YPF="<side-repo>/workbench/project-eris/data.ypf"
cargo run -p yu-font --example scan_evidence
cargo run -p yu-font --example render_reference
```

`scan_evidence` 只读；`render_reference` 输出到 `target/tmp/font-ref/`（不入库）。
