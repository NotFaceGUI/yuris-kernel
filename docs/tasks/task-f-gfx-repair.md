# 任务书 F · yu-gfx 修复与复核(完美渲染对齐)

> ## ⚠️ 先读这一段
>
> **本文件列出的所有“问题”都来自一次审查,审查本身可能误读手册、误读反编译、误读代码。
> 默认假设:每一条都“不一定正确”,都必须重新求证。禁止直接照单修改。**
>
> 你首先要产出的不是补丁,而是一份**验证台账**:每条审查主张 → 重新取证 → 结论
> (Confirmed / Refuted / Uncertain)→ 决策(修复 / 挂账 Unimplemented / 撤销并勘误)。
>
> **核心目标只有四个字:完美渲染。**
> 判定顺序:语义正确(图层/顺序/可见性/Alpha/混合/变换/窗口/文本)
> → 显示列表正确(CompositorFrame 可逐字段核对)
> → 像素正确(软参考逐像素、官方截图有据容差)。
> 本会话只做前两层;像素层留给后端会话。没有语义正确,像素对齐没有意义。

## 0. 真值层级(高于本文件、高于旧线文档)

1. **官方引擎可观测行为**(0.495/0.479 实跑、截图、受控脚本输出)
2. **官方手册页原文**(逐页读,不能只 grep;HTML 含非文本字节)
3. **真实脚本与官方编译产物**(ERIS `.yst` / `.ybn`;命令 pc/参数可定位)
4. **反编译/REA 事实**(官方 exe,给函数地址+行号;地址必须能复核)
5. 旧线文档/旧 golden —— 只是线索,不是真值
6. **本文件的审查主张** —— 最低等级,必须复核后才能升级

> 审查主张与手册冲突时,先重读手册;审查主张与真实脚本冲突时,先看脚本上下文;
> 审查主张与 REA 冲突时,重新反编译;三者都不能证明时,保持 `Unimplemented`。

## 1. 复核方法(每条都要走)

- **手册**:`<side-repo>/target/sdk/0.495/yu-ris_0495_032/マニュアル/YU-RIS/html/`
  用 CP932 解码读取相关页;记录页面路径+原文句子。
- **真实脚本**:`<side-repo>/workbench/project-eris`(数据入口 = `data.ypf` 优先,
  散盘 `data/` 为备;脚本在包内 `script/ERIS/`,编译产物在 `ysbin/`)。
  记录文件:行号、命令、参数、上下文注释。
- **REA**:官方 `yu-ris.exe`(sha256 `4f5eefa3…`)。
  - 可调用 `mcp__rea__open_binary` → `procedure_pseudo_code`/`analyze_function` → `close_binary`。
  - **不要相信旧笔记的地址**:`0x4506f4/0x4521dc/0x4567c0/0x42c4a4/0x43d334/0x43d4a4`
    已出现无法复核/落错函数的情况;必须从 xref、字符串、调用链重新锚定。
  - 先在本地反编译树找:`<side-repo>/workbench/analysis/decomp/`;找不到再直接 REA。
  - 每条 REA 证据写:函数入口地址、关键伪代码/汇编行、observed。
- **官方观测**:需要判断“可见结果”时,用官方引擎跑最小脚本/ERIS 并截图或记录状态;
  截图必须原生分辨率、无窗口边框、无缩放插值。
- **代码**:`crates/yu-gfx/src/...:行号`;不要凭记忆。

## 2. 验证台账(第一个交付物)

新建 `docs/gfx-verification-ledger.md`,每条一行:

| ID | 审查主张(原文缩写) | 复核证据(手册页/脚本行/REA 地址/代码行) | 结论 | 决策 | 回归 |
|---|---|---|---|---|---|

结论只能是 `Confirmed` / `Refuted` / `Uncertain`。
决策只能是 `修复` / `保持 Unimplemented` / `撤销审查主张并勘误`。
`Uncertain` 不得进入修复;只能挂 DIV 并保持现状。

## 3. 待复核清单(审查主张,不是事实)

### A. 手册/真实脚本冲突类(优先)

| ID | 审查主张 | 先看哪里 |
|---|---|---|
| A1 | `TID=""` 被归一化为 `:/W0` 并保留旧 `TA`,产生整屏过渡 | 代码 `state.rs:1531`、`path.rs:57`;脚本 `es_Button.yst:4526/4540`、`es_Sprite.yst:4238`;手册 `cg/cg.html` |
| A2 | 图像层 `SX/SY` 被丢弃,`FILE="" SX/SY` 黑幕变 1×1 | 代码 `state.rs:1594`;手册 `cg/cg.html`(“SX/SY 仅图层组有效”);脚本 `es_SaveLoad.yst:643`、`es_ERIS.yst:579/600` |
| A3 | `WINDOW` 域整体被忽略,主窗口 CGID/E/Z/X/Y 无效 | 代码 `state.rs:197-206`、`:75`;手册 `window/window.html`;脚本 38 处 `WINDOW[` |
| A4 | `TEXT[ID/OUTPUTMODE/CLEAR/…]` 被丢弃,`TextRun.id` 恒空 | 代码 `yu-rt/handlers.rs:1976-1992`、`:2011/2037`;手册 `text/text.html`;脚本 `es_First.yst`、`es_Text.yst` |
| A5 | `CGINFO/FONTINFO/WINDOWINFO` 查询不写 `LET` | 代码 `state.rs:319-332`;`yu-rt/handlers.rs:2426-2451`;手册 `cg/cginfo.html`、`font/fontinfo.html`、`window/windowinfo.html`;脚本多处 |
| A6 | `CGACT SIZE/SIZE2` 的 SX/SY 丢失;`RESIZE/TRIM` 未识别 | 代码 `state.rs:604-610/819-832/1368-1397`;手册 `cg/cgact.html` 例 3/21/22;脚本 SIZE 46 次、RESIZE 14 处 |
| A7 | `FONT[NO/BNO]` 不生效 | 代码 `state.rs:925-931`;手册 `font/font.html`;脚本 16 处 `FONT[BNO=1` |
| A8 | `CG` 无 `FILE` 创建鬼层,与手册“层不存在=错误”冲突 | 代码 `state.rs:432-446`;手册 `cg/cg.html` 例 4;需引擎实际行为 |
| A9 | MODE 3/5/6/7/9 与 4/8/10..18 被整层跳过 | 代码 `state.rs:1473-1502`、`layer.rs`;手册 `cg/cg.html` MODE 全表;先确认哪些模式在真实首屏/商业样本出现 |

### B. REA/引擎对齐类(必须重新取证)

| ID | 审查主张 | 先看哪里 |
|---|---|---|
| B1 | 代码每帧按 `(z,seq,id)` 重排;引擎遍历器只走链表,Z 排序在链表维护侧;改 Z 是否重排未证明 | `state.rs:1112-1131`;`FUN_00413dc8`;旧线 `02-layer-model.md` G3 |
| B2 | `E` 可能写 `+0x89` 而非 `+0xa4`;代码只有一个 `visible` | `state.rs:1430-1436`;`0042c400`、`0042f57d`、`0042f809`;需写侧 REA |
| B3 | 组离屏证据: `+0x61c`/`+0x90` 可能不是 RT/合成纹理 | `group.rs:7-8`;`00413dc8:57`、`0043cd6c:154`、`0042f809:365` |
| B4 | TFACTOR 只在 MODE 0/3/7 出现,代码给 1/2/5/6/9 也填 AlphaGray | `layer.rs:321-496`;`00414ba4:392/567/754` |
| B5 | 顶点 DIFFUSE RGB 未建模,可能整体色偏 | `layer.rs:388-493`;`0041a308` 调用链 |
| B6 | MODE 3/5 的 stage1 白/黑辅助纹理缺失 | `layer.rs:689-696`;`00414ba4:562/635`、`00462e40:278-318` |
| B7 | Alpha 的 `in_stack_00000010==0 → A=0` 未建模 | `layer.rs:850-861`;`00414ba4:141` |
| B8 | X/Y 单位可能不是像素原值(/100、16.16、-0.5) | `layer.rs:808-817`;`00414ba4:117-125/157/190/214` |
| B9 | 屏幕/组清屏为不透明黑/透明黑,引擎帧入口无 Clear 证据 | `frame.rs:37-53`、`state.rs:1182`;`0043cd6c` |
| B10 | 颜色空间 `GammaEncodedSrgb8` 是 Likely,不是逐像素确认 | `frame.rs:63-80`;`DIV-GFX-009` |
| B11 | COPY 全部 Unimplemented,但文档写“可执行复制”;pass 顺序是 copy→image→组,不是命令顺序 | `state.rs:220-254`、`:736`;golden;`docs/render-parity.md` §2/§12.5 |
| B12 | `0x42c4a4/0x43d334/0x43d4a4` 在本地反编译树不存在,引用失效 | `docs/render-parity.md:44/49/93-94`;decomp 目录 |
| B13 | 过渡 `T/TA/TID` 写侧/推进者仍未取证;`TID` 当层路径可能是假设 | `state.rs:1215-1235/1509-1540`;`00414ba4:895-913` |

### C. 硬编码/A5 类

| ID | 审查主张 | 先看哪里 |
|---|---|---|
| C1 | 视口尺寸未从 yscfg 装载,`from_tables` 忽略 `boot.config.width/height` | `state.rs:104-112`;`yu-format/yscf.rs` |
| C2 | 主窗口固定 `:/W0`,`WINDOW[CGID]` 不消费 | `state.rs:75/197-206` |
| C3 | `IMAGE_ACTIONS` 仅 29 项,缺 `ROTATION2/TRANSITION2/XOR/AND/TRIM/RESIZE/BLUR2..6` | `state.rs:1368-1397`;真实 ysc 表 |
| C4 | 创建默认值 A=256/E=1/MODE=0/T=TA=0/FONT 白/antialias/Transition 0/0 无证据 | `state.rs:978-1053`;`text.rs:47-61`;`layer.rs:820-843` |
| C5 | 扩展名 code 5 未映射(仓库内已有 5=psb 证据) | `layer.rs:141-159`;`yu-format/yscf.rs` |
| C6 | 域分类用 `starts_with("CG")/("FONT")`,不是 ysc 数据 | `yu-rt/handlers.rs:2426-2451` |

### D. 缺失/未考虑类

WINDOW 域;CGINFO/FONTINFO/WINDOWINFO;TEXT 管线;图像层 SX/SY;SIZE/RESIZE/TRIM;
多部件层;MID 掩码;组过渡;子窗口叠放;COPY 时序;RECTPAINT 等图像加工;
文本栅格化;LINT 语义;psb 扩展名;主组缺失 fallback;图像层下挂子层。

### E. 健壮性/性能类

| ID | 审查主张 | 先看哪里 |
|---|---|---|
| E1 | 诊断永久累积、每帧 clone+sort、重复诊断被吞 | `state.rs:177-185/1247-1257/294-297` |
| E2 | `image_log` 只增不减、每帧重放、retain 全量扫描 | `state.rs:859-871/242-254/1104-1110` |
| E3 | `Unimplemented` 粘滞,后续合法更新不恢复 | `state.rs:1348-1352` |
| E4 | 每命令多次小分配,首屏 35–40 万次 | `state.rs:949-959/357/364/1564` |
| E5 | `frame()` 无缓存、比较器内 BTreeMap 查找/id clone | `state.rs:287-292/1125-1129` |
| E6 | 三处无界递归,深路径/深组可栈溢出 | `state.rs:1057-1068/1133-1149/1152-1185` |
| E7 | 图像层可挂子层,子层静默不绘制 | `state.rs:1057-1085/1162-1171` |
| E8 | 缺必需命令表不 fail-fast | `table.rs:66-79` |
| E9 | `fmt_f64` 对整数值浮点用 `{:.1e}`(128→130),NaN/Inf 非法 JSON | `json.rs:49-65` |
| E10 | CGACT 动作检测 O(29·P²) | `state.rs:1399-1416` |

### F. 测试真实性类

- golden 由实现自生成,`UPDATE_GFX_GOLDEN=1` 写后不执行断言。
- ERIS 首屏只断言 `>100/>10/>=2`,没有断言 272/34/15、W0 30 ops、screen 内容。
- 真实 MODE 2 / A=0 / E=0 图层没有被断言;`all(Normal|Subtract)` 在 golden 里全是 Normal。
- `group.rs` 的“COPY2 不复制属性”没有执行 COPY2;`swap2` 测试没断言 X/Y。
- `filter.rs` 的“ERIS yscfg”输入是硬编码数组;`Other(5)` 被当期望。
- `blend_table` JSON 未与 `fixed_state()` 逐字段交叉验证;MODE 0 完整 stage 未测。
- 没有像素/官方截图对拍;缺样本默认 SKIP 仍算 passed。

## 4. 决策规则

1. `Confirmed` 且影响可见结果 ⇒ 修;必须新增回归,证明“修复前失败/修复后通过”。
2. `Refuted` ⇒ 不改代码;更新 `docs/render-parity.md`/DIV,留下“审查主张被驳回”的勘误。
3. `Uncertain` ⇒ 保持现状或 `Unimplemented`;写 DIV;不得用近似实现。
4. **手册与真实引擎/脚本冲突** ⇒ 以引擎为准,同时把手册勘误写清楚(旧结论/更正/发现方式)。
5. 修复不得引入新的硬编码;参数继续按 ysc slot→name;屏幕尺寸从 yscfg;主窗口从 WINDOW。
6. 性能修复不得改变可见语义;先保证正确,再做缓存/去分配。

## 5. 修复顺序

1. **台账完成**(A/B/C/D/E/F 全部有结论;允许 `Uncertain`)。
2. **P0 语义**:A1–A8 中 Confirmed 的项;B1–B13 中 Confirmed 的项;缺失块中影响首屏/真实游戏的(WINDOW/TEXT/CGINFO/图像尺寸)。
3. **P0 健壮性**:E9(数值/JSON)、E3(粘滞)、E1/E2(无界)、E7(静默丢层)、E6(递归)。
4. **P1 性能**:E4/E5/E10。
5. **测试**:F 类逐项补;golden 加样本哈希/生成命令;首屏精确 census;真实 MODE 2/A=0/E=0。
6. **文档**:`docs/render-parity.md` 证据状态更新;DIV 新增/关闭;旧线勘误留痕。

## 6. 验收

- `docs/gfx-verification-ledger.md` 覆盖全部审查主张,无“未复核就修复”。
- 每个修复有回归;报告贴“修复前失败/修复后通过”的原始输出。
- `cargo test -p yu-gfx` 全绿;`YURIS_REQUIRE_SAMPLES=1` 下真样本必须执行。
- `cargo test --workspace --no-fail-fast` 不回归。
- `cargo clippy -p yu-gfx --all-targets` 无新增 warning。
- `grep -rn "winit|wgpu|web-sys|wasm-bindgen" crates/yu-gfx/src` = 0;
  `println!/eprintln!/dbg!` = 0;`HashMap` = 0。
- 不确定项没有被“修”成猜测;DIV 与实际状态一致。
- 首屏 golden 至少能回答:哪些层可见、为什么可见、哪些 op 被跳过、为什么。

## 7. 禁止

- 禁止把本文件的审查主张当事实直接改代码。
- 禁止为了通过测试而放宽断言或 `UPDATE_GFX_GOLDEN` 掩盖差异。
- 禁止引入平台类型到 `yu-gfx`;禁止硬编码槽号/屏幕尺寸/主窗口。
- 禁止修改旧线工作区;禁止任何 git 写操作;被跟踪文件用占位符。
- 禁止在证据不足时“实现一个看起来合理的公式”;保持 Unimplemented。

## 8. 环境

```bash
export TEMP="D:/Art/ai/projects/YurisDroid/workbench/tmp"; TMP="$TEMP"
export YURIS_TEST_TMP="D:/Art/ai/projects/yuris-kernel/target/test-tmp"
export YURIS_REQUIRE_SAMPLES=1
export YURIS_SAMPLE_ERIS_ROOT="<side-repo>/workbench/project-eris"   # 数据入口:data.ypf 优先,data/ 为备
export YURIS_SAMPLE_ERIS_YPF="D:/Art/ai/projects/YurisDroid/workbench/project-eris/data.ypf"
export YURIS_SAMPLE_SDK0495="D:/Art/ai/projects/YurisDroid/target/sdk/0.495/yu-ris_0495_032"
```

开工前先跑 `find crates -type f -newermt "-10 minutes"`;有活跃写者的 crate 只读不写。
