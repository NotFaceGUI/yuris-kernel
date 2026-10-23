# 任务书 P8 · 按钮快照携带文本(DIV-GFX-058:存读档界面行文本空白)

> 仓库:`<repo>`。先读 `AGENTS.md`、本文件、
> `crates/yu-gfx/docs/divergences/README.md` 的 **DIV-GFX-058**(本任务的
> 档案)、`docs/handoffs/H-P2-05-dialog-save-ui.md`(§3 H-P1-08)、
> `docs/symptom-ledger.md` §S4、`PROGRESS.md` 成果 P2(文本管线)/
> **成果 P7 续**(MATH 下标 + VARACT 存在性双修复,S2/S3/S4 的 Fatal 已除)。
>
> 背景:P2 落地文本管线后,进本篇对话正文可见;但 S4 存读档界面的
> **8 个选择行按钮只有底图、标签文本全部空白**(底栏 セーブ/ロード 等
> 有字 —— 那些是烤好文字的整图素材,不走文本管线,勿混淆)。
>
> 文件占用:
> - 独占 `crates/yu-gfx`、`crates/yu-render/soft`、`crates/yu-render/wgpu`
>   (`yu-font` 按需);
> - **`yu-rt` / `yu-cli` / `yu-save` 不碰**(文本的 VM 事实层已齐:
>   `CGACT[TEXT]` 解析、`_TEXT_FLAG` 排版流、reveal 模型);
> - 零 git 写操作;PROGRESS 只追加「成果 P8」;证据产物只放 `target/tmp`。

## 0. 基线(动手前必做)

```bash
export TEMP="D:/Art/ai/projects/YurisDroid/workbench/tmp"; export TMP="$TEMP"
export YURIS_SAMPLE_ERIS_ROOT="D:/Art/ai/projects/YurisDroid/workbench/project-eris"
export YURIS_SAMPLE_ERIS_YSBIN="$YURIS_SAMPLE_ERIS_ROOT/save/data/ysbin"
cargo test -p yu-rt -p yu-gfx -p yu-font -p yu-backend-soft -p yu-backend-wgpu
```

复现入口(主人桌面已实证):`cases/symptom-ingame-save.json`(headless)、
`cases/host/symptom-4-save-load.json`(桌面)。现象 = 进入存读档界面后,
8 个行按钮的 `CGACT[TEXT]` 标签不显示。

## 1. 现状(证据链已到位,不要重新发明)

- **机制链(已取证,2026-10-23)**:ERIS 按钮标签 =
  `\BT.*` 宏(`save/data/script/ERIS/Button/es_Button.yst`,**明文**)→
  `CGACT[TEXT]` 画在临时按钮层(`:/W0/BT.OFF.TMP` 等)→ 按钮系统
  `COPY` 快照到按钮精灵(S4 探针:copy 68→76 条;DIV-GFX-053:
  `CopyOp.source_texture` 在命令时刻固定源纹理)。
- **内核分歧点**:`GfxState` 的文本是**渲染期覆盖层**
  (`state.rs`:`text_draws: BTreeMap<LayerId, Vec<TextDraw>>`,不进图层
  基图 op);`copy_attributes`(state.rs)拷 alpha/parts/mask/…
  **不含文本** ⇒ COPY 后文本留在源层,快照丢失。
- **官方语义**:引擎 `CGACT[TEXT]` **即时把字符写进图层表面**
  (手册 `cg/cgact.html` 例 25/26;`es_Font.yst` 同位 `SET=1` 轮廓 →
  `SET=0` 填色的两次表面写;REA `FUN_004056b8` SET→1/9/3 次合成)
  ⇒ 官方的 COPY **天然携带**已画文本。分歧在我方的覆盖层模型,不在 COPY。
- **性能约束(P2 契约,硬门禁)**:对话正文是逐字 `CGACT[TEXT]`
  (設定界面单帧命令量百万级),P2 为此选择渲染期合成、无文本帧零惰性。
  title 600 帧 soft/wgpu 与基线**逐帧 SHA-256 相同**是回归红线。

## 2. 阶段

### M-A · 取证与方案决策(P0,先于一切编码)

1. **明文源码走读**(样本在手,不需要猜):读 `Button/es_Button.yst`、
   `Scene/BACKLOG/ES_LogSet.yst`、`es_SaveLoad.yst` 中按钮标签的绘制链:
   画文本的命令序列、目标层(`BT.OFF.TMP` 族)、COPY/SNAPSHOT 的先后与
   频度;输出带源码行号的链路笔记(放本任务书附录或 `target/tmp`)。
2. **REA 复核**(优先做,0.495 `yu-ris.exe`):确认 `CGACT[TEXT]` 处理体
   (`0x4056b8` 附近)确实写层表面(而非挂显示列表);确认 `COPY` 语义
   含表面内容。已有的 SET→1/9/3 合成次数证据可直接引用。
3. **方案决策**(结论写进 DIV-GFX-058,再动手):
   - **方案 A(优先)**:COPY 携带文本 —— `CopyOp` 在命令时刻把源层当前
     `text_draws` 一并固定,后端解析快照面时先合成「基图 + 文本」再拷。
     理由:COPY 低频(按钮快照 ~76 条/场景),对话逐字路径零改动,
     P2 性能模型保持;
   - 方案 B:CGACT 时刻烘焙进层表面 —— 内存模型与官方最一致,但逐字
     高频重合成有性能风险,**除非 A 被证据否定否则不选**;
   - 无论哪种:无文本帧必须保持完全惰性;`TextRun` 的 flags/reveal
     语义不得破坏。

### M-B · gfx 模型(P0)

按 M-A 决策改 `state.rs`:CopyOp 携带源层文本(含下标/裁剪区域与
`CopyRegion` 的坐标关系)或等价机制;`copy_attributes`/`CGEND` 同帧删除
的既有语义(DIV-GFX-053)不得回退;`frame()`/`json.rs` 契约变更保持
「空时不输出」的 golden 稳定性。

### M-C · soft 参考实现(P0)

快照面 = 基图 + 源层已画文本(按 `_TEXT_FLAG` 逐字宽、`SET` 合成次数
既有规则);新增微场景 golden:**先 CGACT[TEXT] 后 COPY**(按钮标签形态)
与**先 COPY 后 CGACT**(文本不得出现在快照里 —— DIV-GFX-053 命令时刻
固定语义)两条都要。

### M-D · wgpu 对齐(P0)

与 soft 同帧逐像素一致(atlas 复用既有 `TextComposer`);沿用
`wgpu_text_probe` / `frame_walk` 对拍工具链。

### M-E · 门禁与验收(P0)

1. `cargo test -p yu-gfx -p yu-font -p yu-backend-soft -p yu-backend-wgpu`
   全绿;fmt/clippy clean;
2. title 600 帧 soft/wgpu 与基线 **0 差异**(P2 红线);
3. S4:`cases/symptom-ingame-save.json` 更新 expect(新 display/PNG 先给
   主人过目再 `--update`);桌面 `yu-desktop` 截图人工验收 ——
   8 个行标签可见;
4. soft/wgpu 在 S4 流程同帧逐像素一致。

## 3. 台账 / 交接

- DIV-GFX-058 → **fixed**(带 M-A 证据链 + 微场景名);
- H-P2-05 的 gfx 子项随之关闭;`sv.write.refused`(DIV-RT-042 门禁)与
  SAVEINFO 等**不在本任务范围**;
- 若取证发现官方 CGACT[TEXT] 并非写表面 ⇒ 停在 M-A,上报证据,改立档案。

## 4. 停止 / 上报条件

- 明文源码与 REA 结论冲突 ⇒ 停,以官方二进制为准并勘误;
- wgpu 与 soft 不可解释差异 ⇒ 停在最小复现,不抹平;
- 需要动 `yu-rt`/`yu-cli` 契约 ⇒ 交接件,等主人裁决。
