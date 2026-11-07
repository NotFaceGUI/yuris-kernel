# DIV-GFX-079 · `CGACT[TEXT]` 空串被误报成「缺 SETSTR」

- 状态: fixed
- 优先级: 中(日志语义错误 + 演出期刷屏;像素无差异)
- 发现日期 / 发现方式: 2026-11-07 / 商业包桌面实跑日志
  (`WARN gfx.arg.missing @ s11 pc331 … CGACT[TEXT] 缺 SETSTR` 每次演出
  出现 5 条,聚合显示为 `x5`)

## 复现

- 游戏位置: 0.486 商业包(`es_Font.yst` s11 pc331..344)、0.494 商业包
  (s11 pc416 同型)。标题菜单/对话逐字路径都会命中。
- 锚点: `CGACT[ID=$ID TEXT=1 SET=0 X=@FONT.X Y=@FONT.Y SETSTR=$TX]`
  (0.495 SDK 散盘 `Text/es_Text.yst:1031` 的 `\BT.FONT.DRAW(...)` 下游;
  外字分支同文件 648..672)。

## 官方行为

- 手册 `cg/cgact.html`:`SETSTR = 文字列 TEXTキーワードで文字を表示する際に指定`。
  未给时取参数表缺省 = 空字符串。
- 引擎文本合成 `FUN_004056b8`(CGACT 处理体 `0x404398 → 0x4046fc →
  0x404914` 调用点 `0x42e22f`)对空串直接返回:**不写层表面、不报错**。
- `es_Text` 的 `es.TX.OUTPUT.ONE` 每条目 `\_strmid($TX,@TX.POS,1)` 取
  1 字 → 逐字 `\BT.FONT.DRAW(...,$txone)`;名字窗口
  (`TIP.NAMEW.TX`)在无名字时也发同命令。空字/无名字是**正常数据**。

## 内核行为

旧实现 `cgact_text`:

```rust
let Some(bytes) = set_str.filter(|b| !b.is_empty()) else {
    // 报 Recoverable gfx.arg.missing
    // detail: “CGACT[TEXT] 缺 SETSTR:引擎不绘制空字符串,但参数缺失不是同义”
};
```

`Option::filter` 把「参数窗口缺失」与「参数在但值为空」合并成同一分支,
按 Recoverable 每帧报;商业包一次 `FONT.DRAW`(影 3 + 描边 1 + 正文 1)
聚合成 `x5`,演出期持续刷屏。

反编译核实(光翼 s11 pc331 参数窗口):

```text
w0 ID     ... 48 03 00 24 b7 07      → $ID
w1 TEXT   42 01 00 01                → 1
w2 SET    42 01 00 00                → 0
w3 X      ...
w4 Y      ...
w5 SETSTR 48 03 00 24 b8 07          → $TX   (窗口存在)
```

实跑探针(临时 `YURIS_PROBE_CGACT_TEXT`,已删除)输出证明参数有值:

```text
[probe] CGACT TEXT target=:/W0/BT.OFFES.GAMEMAIN.TIP.MESW.TX=0=1
        set_str=Some(2) params=[TEXT=Int(1) SET=Int(0) X=Int(8) Y=Int(7)
        SETSTR=Str([215, 247])]
```

即 `SETSTR` 从未缺失;告警帧只是 `$TX` 求值为空串。

## 二分记录

- L1 译码一致: 是(`dump_script` 参数窗口/表达式解出 `$TX`)。
- L2 执行/事件流一致: 是(同一 `FONT.DRAW` 链路,非空帧像素正常)。
- L3 帧层状态一致: 是(空串不产生 TextDraw,旧实现同样提前返回)。
- L4 帧间编排一致: 不适用。
- 定位结论: 诊断分类错误(缺参 vs 空串),渲染语义本来就对。

## 根因

诊断层把 `SETSTR` 的「参数槽缺省(空串,官方语义)」和「参数窗口不存在」
混为一个 `None/empty` 分支,并按 Recoverable 上报。官方两者都是空串短路,
不存在错误。

## 修法

`crates/yu-gfx/src/state.rs::cgact_text`:

- `Some(非空)` → 正常登记绘制项;
- `Some(空串)` / `None` → `error::verbose(code::TEXT_EMPTY)` 留痕后返回。

新增稳定码 `gfx.text.empty`(`crates/yu-gfx/src/error.rs`),仅
`--log-level verbose` 可见。两类 detail 分开写(空串/未给),便于排查
「文本为什么没出现」。

## 回归

- 新增测试 `crates/yu-gfx/tests/text_pipeline.rs::cgact_text_empty_setstr_is_a_verbose_noop`:
  空串与缺省都不产出 TextDraw、不得出现 Recoverable、留 2 条 Verbose。
- `cargo test -p yu-gfx --test text_pipeline` → 15 passed。
- 商业包 headless 1100 帧(0.486 / 0.494 / 0.481 各一次):
  `gfx.arg.missing`、`缺 SETSTR` 计数 0;标题菜单文本仍正常绘制
  (0.494 首屏 PNG sha 与修复前一致 `1d684027…dcd75d`)。
- 残留: `gfx.text.empty` 属 Verbose,默认日志不可见;需要时
  `--log-level verbose` 可见,不改变 display JSON 之外的任何产物。
