# 任务书 P2 · 文本链路落地(字形栅格化 / 排版 / 跨帧持久)

> 仓库:`<repo>`。先读 `AGENTS.md`、本文件、`docs/gfx-unimplemented-todo.md`
> (DIV-GFX-012/021/033/034/035/036/040)、`docs/font-contract.md`、
> `docs/font-provenance.md`、`crates/yu-rt/docs/coverage-matrix.md` §7
> (P0 文本路径验证方法)、`PROGRESS.md` 成果 M2/COV-M-01..03。
>
> 背景:进本篇后对话文本完全不可见("黑屏/无字"主因)。VM 侧 TEXT
> 状态机(M2)与 `yu-font`(发现/加载/栅格/样式)已就绪,缺的是
> **gfx 文本模型收口 + 两后端字形执行**。
>
> 文件占用:
> - 独占 `crates/yu-gfx`、`crates/yu-font`、`crates/yu-render/soft`、
>   `crates/yu-render/wgpu`;
> - `yu-rt` / `yu-cli` 不碰;需要跨 crate 契约变更 ⇒ 交接件等主人裁决;
> - 零 git 写操作;PROGRESS 只追加"成果 P2";证据产物只放 `target/tmp`。

## 0. 基线(动手前必做)

```bash
export TEMP="D:/Art/ai/projects/YurisDroid/workbench/tmp"; export TMP="$TEMP"
export YURIS_SAMPLE_ERIS_ROOT="D:/Art/ai/projects/YurisDroid/workbench/project-eris"
cargo test --workspace
```

## 1. 现状

- gfx 只输出 `TextRun`/`TextStyle`(语义模型),**每帧清空**
  (DIV-GFX-034:文本跨帧持久 + `TEXT[CLEAR]` 传播缺失);
- 后端**没有字形路径**:soft/wgpu 都不栅格化(DIV-GFX-012);
- FONT 命令字段(NO/BNO/GRADATION/ROTATION/PITCH/LANG、默认字体)
  只保存原值(DIV-GFX-035);FONTINFO 不写 LET(DIV-GFX-036);
- TEXTINFO 已在 `yu-rt` 实现(M-2),gfx 侧未消费(DIV-GFX-040);
- 逐字符宽度证据已存在:VM 的 `WORD`/`_` 每字符写 `'1'/'2'` 到
  `_TEXT_FLAG`(COV-M-03,REA 0x458d44/0x4567c0)——**排版宽度的一手
  证据,不要自己另造布局**;
- 文本推进按钮已读 `TEXT[CLICKBUTTON1..4]/SKIPBUTTON1..4]`(AU4)。

## 2. 阶段

### M-A · gfx 文本模型收口(P0)

1. 文本跨帧持久:`GfxState` 持有跨帧文本缓冲,`TEXT[CLEAR]`/窗口删除
   才清(DIV-GFX-034);`frame()` 每帧输出当前可见 TextRun 集;
2. `TextStyle` 与 FONT 字段映射:FONT NO/BNO → 字体身份(工程字体表 +
   `yu-font` 发现链);GRADATION/ROTATION/PITCH/LANG 留值 + 标注
   Unverified(取证不足不实现渲染效果,但**不得丢值**);
3. 默认字体取证:手册 `font/*.html` + REA `0x444428`(FONT)/
   `0x444cfc`(FONTINFO)+ ERIS 工程 yscfg 字体表;缺证据 ⇒ 显式
   Unimplemented + DIV,不猜;
4. FONTINFO 已取证字段经 `GfxHost` 写回 LET(DIV-GFX-036 中可解部分)。

### M-B · soft 参考后端字形执行(P0)

1. `yu-font` 栅格 → 字形位图;soft 按 `_TEXT_FLAG` 逐字符宽度排布
   (等价于引擎逐字标志流),颜色 = TEXT 颜色状态;半透明按 alpha;
2. 文本绘制层序:取证(手册 text 显示层级 + REA)后落 soft;
   证据不足先用"窗口 CG 层之上、按窗口 Z"并挂 Unverified;
3. 输出 PNG 与 `display.json` 对账(glyph 数、advance 累计)。

### M-C · wgpu 字形执行(P0)

1. 字形 atlas 缓存(按 字体+字号+码点 键);TextRun → quads;
2. 与 soft 同帧逐像素一致(沿用既有对拍工具链,`frame_walk` 扩展
   文本帧);600 帧回归不引入差异(title 帧必须逐字节不变)。

### M-D · 显示节奏对接(P1)

1. VM 让出与 reveal 计数:TEXT OUTPUTMODE/WAIT/CLICK 的已显字符数
   若需 VM 暴露 ⇒ 交接件(不改 yu-rt);soft/wgpu 消费"当前应显示的
   字符数"渲染截断;
2. 自动模式行尾 C/R/P/E(COV-M-02)在显示侧的表现按手册取证后实现。

## 3. 验证(方法照抄 coverage-matrix §7 P0)

```bash
# 绕过标题直接 boot 到文本标签,固定时钟 + 定时点击
# SCENARIO_EPILOGUE(s129): 30 tick, 1 行「エピローグ。」
# RP_101(s133): 每 10 tick 点击, 10 行文本逐行推进
```

- 10 行文本逐行**可见**、CLICK 推进与 VM 让出一致;
- soft/wgpu 文本帧 PNG 同帧一致;title 600 帧回归 0 差异;
- `cargo test -p yu-gfx -p yu-font -p yu-backend-soft -p yu-backend-wgpu`
  全绿;clippy/fmt clean。

## 4. 交接件

- H-P2-01:`VmEvent::Text.id` 为 UTF-8 `String`,非 UTF-8 CP932 ID 被清空
  (DIV-GFX-021,coverage-matrix §5):需改事件字段类型(字节),跨 crate
  契约变更 ⇒ 等主人裁决,本任务只保证模型不丢字节(留 raw 备用);
- H-P2-02:VM 侧 reveal/已显字符数查询 API(若 M-D 需要);
- H-P2-03:官方文本窗口截图对拍(oracle 挂账,YSCom attach 恢复后做)。

## 5. 停止/上报条件

- 字体文件缺失/加密 ⇒ 建档挂 blocked,给出取证路径,不猜字体;
- 层序/半像素等证据不足 ⇒ Unverified 标注 + DIV,不阻塞主路径;
- wgpu 与 soft 出现不可解释差异 ⇒ 停在最小复现,上报,不抹平差异。
