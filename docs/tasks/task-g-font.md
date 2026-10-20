# 任务书 G · `yu-font`:一次性完整文本样式(参考游戏配置直出)

> ## 目标
>
> **不做“第一版/最小版”。一次性把字体 + 文本样式子系统做完整**,并用
> **参考游戏(ERIS)的真实 FONT 配置**直接渲染出可对比的文本样例(PNG),
> 供主人肉眼/像素对比;后续由主人桥接到 `yu-gfx`/后端。
>
> ## ⚠️ 唯一硬门禁:先取证
>
> 字体来源、栅格化路径、FONT 参数语义、描边/阴影/旋转算法**必须先取证**。
> 未取证的样式不许用“看起来合理”的公式;要么按官方证据实现,要么显式
> `Unimplemented` 并在报告中列出。**取证是门禁,不是版本阶段。**

## 0. 边界

- 本会话独占新建 `crates/yu-font`;可写 `docs/font-*.md`、`examples/`、`tests/`。
- **不修改** `yu-gfx` / `yu-rt` / 后端 / 根 `Cargo.toml`(workspace 自动纳入新 crate)。
- `yu-font` 只负责“字体 + 字形外观 + 样式合成 + 参考渲染器”;
  排版(换行/对齐/ruby/逐字节奏)只定义接口,不实现引擎排版。
- 参考渲染器用于验收:输入字符串 + 完整样式,输出 RGBA/PNG;
  它不进入生产路径,主人在后续桥接时决定复用哪些层。

## 1. 必须交付

1. `crates/yu-font/` 完整实现(见 §3/§4)。
2. `examples/render_reference.rs`:读取参考游戏配置 + 样本文本 → PNG contact sheet。
3. `docs/font-provenance.md`:字体来源/引擎栅格路径/FONT 参数/参考截图证据表。
4. `docs/font-contract.md`:给 `yu-gfx/text` 与后端的接口、样式语义、Unimplemented 清单。
5. 测试与 golden(见 §6)。
6. `PROGRESS.md` 追加“成果 G1”。

## 2. Phase 0 · 取证(硬门禁,先完成)

### 2.1 官方字体来源

- 用 `yu-vfs` 的 `YpfSource` 列出 `data.ypf` 全部条目,按
  `.ttf/.otf/.ttc/.fnt/.psb` 过滤;**不要只做字节搜索**(条目可能压缩/名字取反)。
- 同时检查工程目录、`system/`、`data/` 下的字体/字库文件。
- 如果没有字体文件:记录“疑似系统字体”,从 `FONT[NO/BNO/NAME]`、`yscfg`、
  编译产物/引擎导入反查字体名;不要直接假定 MS Gothic/Noto。
- 如果发现位图字体/字库:优先按位图实现(最容易 1:1),记录格式。

### 2.2 官方栅格化路径(REA)

- 官方 `yu-ris.exe`(sha256 `4f5eefa3…`)Ghidra/REA:
  - 导入表/xref:`CreateFontA/W`、`CreateFontIndirectA/W`、`AddFontResourceA/W`、
    `GetGlyphOutlineA/W`、`TextOutA/W`、`ExtTextOutA/W`、`DrawTextA/W`、
    `GetTextExtentPoint32`、`ID3DXFont` 等;
  - 找到字体创建、字形缓存、文本绘制函数;确认 GDI / 自绘 / ClearType。
  - 给函数入口地址 + 关键伪代码/汇编行;找不到就写 Unknown。
- 只读,不 annotate;结束时 close_binary。REA 同一时间只允许一个会话使用;
  若其他会话占用,先读本地反编译树或等待。

### 2.3 参考游戏 FONT 配置

- 从 ERIS **编译产物**扫描 `FONT` 命令(宏展开后才会出现;源 grep 可能为 0):
  - 用 `yu-format` 解码 YSTB,按 ysc 表找到 `FONT` 的 opcode,
    打印每个窗口的槽号→参数名与值;至少覆盖 ERIS 首屏/标题/对话/菜单。
  - 同样扫描 `TEXT`、`FONTINFO`、`WINDOW` 里与文本样式相关的参数。
- 从官方手册抄 `FONT` 参数语义:
  `BNO/NO/NAME/ANTIALIASING/COLOR/COLOR2/GRADATION/ROTATION/DRAWMODE/SX/SY/BOLD`
  的默认值、单位、范围、省略行为。
- 如果参考游戏没有显式 FONT 配置,使用**引擎默认值**(必须由 REA/手册证明),
  不能拿库默认值顶替。

### 2.4 官方参考图

- 官方 0.495 跑 ERIS/最小脚本,截取原生分辨率文本样例:
  单字、单行、带描边、带阴影、不同字号、粗体、彩色/渐变、旋转;
  保存到仓库外 `target/tmp/font-ref/`。
- 记录截图方法/分辨率/字体名/字号。

### 2.5 证据表

`docs/font-provenance.md` 必须逐项给出:

| 项 | 证据(手册页/脚本行/REA 地址/截图) | 结论 | 等级 |
|---|---|---|---|

没有这张表,不进入 §3。

## 3. 完整字体管线(一次实现)

### 3.1 字体解析与加载

- `FontSource`:
  - 自带字体文件(优先);
  - 系统字体(fontdb/平台 API);缺失显式失败;
  - 位图字体/字库(若证据表明使用)。
- `FaceId` 解析:`NAME`、`NO`、`BNO`、默认字体;记录来源。
- 不写死系统路径;不静默 fallback。

### 3.2 字形栅格化

- 主实现:`swash`(解析 + outline + coverage;纯 Rust/wasm);
- 可选:`fontdue`(简单 coverage);`GdiRasterizer`(Windows 参考,feature gate);
- 输出 `GlyphBitmap { width, height, bearing_x, bearing_y, advance, coverage: Vec<u8> }`;
- AA 模式:灰度/无/ClearType(仅证据支持);
- 确定性:同一 key 两次输出逐字节一致。

### 3.3 完整样式合成

一次性实现下列全部能力(每项按 §2 证据决定算法/参数,未取证则显式 Unimplemented):

| 能力 | 实现要求 |
|---|---|
| 字号 SX/SY | 在最终显示尺寸栅格化;非等比缩放按其语义 |
| 粗体 BOLD | 优先 Bold face;合成加粗时固定算法并记录 |
| 斜体 | 优先 Italic face;否则 shear,记录角度/原点 |
| ANTIALIASING | 灰度/关闭/ClearType 按证据 |
| COLOR / COLOR2 | 颜色乘法/双色;颜色空间按证据 |
| GRADATION | 线性/径向/逐字或整体;坐标/色标按证据 |
| 描边 | 轮廓 stroke(tiny-skia)或 coverage 膨胀;线宽/圆角/斜接/AA 固定 |
| 阴影 | 偏移/颜色;模糊仅在证据支持时实现 |
| ROTATION | 旋转 quad 或按角度重栅格化,按证据 |
| DRAWMODE | 按手册页语义 |
| 字距/kerning | 标量/字体 metrics;引擎覆盖规则按证据 |
| 行距/对齐/ruby | **只定义接口**;排版归 gfx,本 crate 不实现 |

样式合成顺序必须固定并写进 `docs/font-contract.md`:
base coverage → bold/italic → outline → shadow → color/gradient → rotation/scale → composite.
如果官方证据显示不同顺序,以证据为准并更新契约。

### 3.4 参考渲染器

- `examples/render_reference.rs`:
  1. 读取参考游戏 FONT 配置(扫描编译产物;无则用已证引擎默认);
  2. 生成样例:ASCII/假名/汉字/标点、单字/整行、彩色/渐变/描边/阴影/旋转/粗体;
  3. 用完整样式管线渲染到 RGBA,写 PNG contact sheet;
  4. 每格标注使用的字体/字号/样式参数;
  5. 同时输出一份 JSON,记录每个样例的配置和输出 hash。
- 输出目录:`target/tmp/font-ref/`(不入库)。
- 与官方截图并排;报告差异统计(完全不同像素、最大通道差、差异区域),
  并归因到字体/AA/描边/取整。

## 4. 缓存与性能

- 字体只解析一次;glyph cache key = `(FaceId, char, px, style, AA, outline, shadow)`。
- cache 有界(条目 + 近似像素双上限),淘汰确定性;不得每帧重栅格化。
- atlas 由后续后端/上层管理;本 crate 输出位图与 metrics。
- 基准:1000 常用汉字 × 2 字号 × 描边,记录首次/缓存命中耗时与内存。

## 5. 跨平台

- `swash`/`fontdue`/`fontdb`/`rustybuzz` 必须能在
  `wasm32-unknown-unknown` 与 `aarch64-linux-android` 编译。
- `std::fs` 仅出现在桌面/Android FontSource;wasm 用 fetch/内存字节。
- Windows 的 GDI 参考实现 feature gate,不进入默认依赖。
- 系统字体缺失时:
  - 报告要求字体名/文件;
  - 参考渲染器直接失败,不生成“近似字体”的假结果。

## 6. 测试与验收

1. `cargo test -p yu-font` 全绿;缺字体样本显式失败/可见 skip。
2. 确定性:同一 `(font, char, style)` 两次 coverage 逐字节一致。
3. 字形 golden:ASCII/假名/汉字/标点,记录 coverage hash + metrics。
4. 样式 golden:
   - 粗体(face vs 合成)、斜体;
   - 描边不同线宽/圆角;
   - 阴影偏移/模糊(若实现);
   - 颜色/渐变/旋转;
   - contact sheet 与官方截图并排,给出 diff 统计。
5. 缺失:字体不存在、字形不存在、坏字体 → `font.*` Diag,不 panic。
6. 跨目标:`cargo check -p yu-font --target wasm32-unknown-unknown`、
   `aarch64-linux-android` 通过(或记录依赖阻塞)。
7. `cargo clippy -p yu-font --all-targets` 无新增 warning;
   `grep -rn "println!|eprintln!|dbg!" crates/yu-font/src` = 0。
8. 参考渲染器必须真的读取参考游戏配置;报告贴出读取到的 FONT 配置清单。

## 7. 并行与安全

- 只新建 `crates/yu-font` 与 `docs/font-*`;不改其它 crate。
- 开工先 `find crates -type f -newermt "-10 minutes"`;有活跃写者的 crate 只读。
- REA 单会话;若 F/其他会话占用,先读本地反编译树或等待。
- 禁止 git 写操作;禁止修改旧线工作区;字体/游戏数据不入库;被跟踪文件用占位符。

## 8. 环境

```bash
export TEMP="D:/Art/ai/projects/YurisDroid/workbench/tmp"; TMP="$TEMP"
export YURIS_TEST_TMP="D:/Art/ai/projects/yuris-kernel/target/test-tmp"
export YURIS_REQUIRE_SAMPLES=1
export YURIS_SAMPLE_ERIS_YSBIN="D:/Art/ai/projects/YurisDroid/workbench/project-eris/save/data/ysbin"
export YURIS_SAMPLE_ERIS_YPF="D:/Art/ai/projects/YurisDroid/workbench/project-eris/data.ypf"
export YURIS_SAMPLE_SDK0495="D:/Art/ai/projects/YurisDroid/target/sdk/0.495/yu-ris_0495_032"
```

## 9. 报告

- `docs/font-provenance.md` 证据表(字体来源/栅格路径/FONT 配置/参考图)。
- 完整样式能力矩阵:每项 Confirmed/Likely/Unimplemented + 实现位置 + 测试。
- 参考渲染器输出的 contact sheet 路径 + 与官方截图的 diff 统计。
- 读取到的参考游戏 FONT 配置原文(参数名/值/来源)。
- 性能基准与缓存数据。
- 未解决项/需要 REA 复核项。
