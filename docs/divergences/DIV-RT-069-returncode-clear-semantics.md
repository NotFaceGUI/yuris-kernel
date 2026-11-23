# DIV-RT-069 · 旧代 stdlib 文本第二句越界(行末清缓冲请求未生效)

- 状态: **fixed(no-manual,2026-11-23)**
- 影响面: `crates/yu-rt/src/semantics.rs`、
  `crates/yu-rt/src/vm/handlers/text.rs`
- 关联: `DIV-RT-032`(`_TEXT`/`_TEXT_FLAG` 写入路径)、
  `DIV-RT-015`(`TASK[RESTART]`)
- 样本: `<v555-1>`(0.555,旧代 ERIS stdlib)

## 1. 现象

旧代 stdlib 游戏正文:

- 第一句正常显示;
- 点出第二句时脚本弹
  「テキスト内部エラーです。テキストサイズ情報を正しく格納できませんでした。」
  随后 `END`,主任务终止(宿主表现为退出/黑屏)。

运行期探针(`YU_SLOT_TRACE`)实测:

- `s105 es_textbufset.yst` 的 `BUFSET.SIZE` 循环
  `@5709 = len(_TEXT) - @5606 + 1`;
- 正常行 `@5709` 能从 N 递减到 0 并 `LOOPBREAK`;
- 出错行入口 `@5709 = 0` 乃至负值,`IF @5709 == 0` 永假,
  `@5710` 空转到 4096 → 错误弹窗 + `END`。

`s104 es_text.yst` 的 `es.TX.OUTPUT.Z` 每行把
`@5605/@5606/@5607` 复位为 1;该分支只在标志流读到 `'Z'` 时进入,
而 `'Z'` 由脚本 `es.TX.OUTPUT.LINEEND` 插入到自己的 `$TX.TASK` 副本。
逐行复位没有发生 ⇒ `@5606` 带着上一行残值。

## 2. 根因:引擎行末不请求清缓冲,标志流跨行累积

三个 0.555 构建的 `RETURNCODE` 处理体完全同型(78 字节):

| build | 处理体 | 行为 |
|---|---|---|
| `b1baf88a871`(`<v555-1>`) | `0x44b920` | `state+0x15 = 1`;追加 `0x45`('E');`task+9 = 1` |
| `b1e3d85507b`(`<v555-pc>`) | `0x44b9e4` | 同上 |
| `b02a4dc04ae`(2.1MB 族) | `0x44b824` | 同上 |

即:**只追加 `'E'`,不追加 C/R/P,并在追加前无条件置清缓冲请求**。
释放路径:

```text
0x404454 任务泵:  task+9 置位 → 0x4047c4
0x4047c4:         task+9 = 0; FUN_0046b32c()
0x46b32c:         if (state+0x15) { state+0x15=0;
                    state+0x20/0x24/0x28/0x2c = 0;
                    memset(state+0x18, 0, 0x2000);   // _TEXT
                    memset(state+0x1c, 0, 0x2000); } // _TEXT_FLAG
```

内核旧路径沿用 0.495 形态:自动模式追加 `'C'/'R'/'P'` + `'E'`,
清缓冲请求只在手动模式置位。旧代 stdlib 每行都由脚本自己插入
C/R/P/Z,引擎再追加 C/R/P 会把标志流拉长;更关键的是**清缓冲请求恒不置位**,
`_TEXT` 被脚本换行替换而 `_TEXT_FLAG` 保留上一行尾部,逐行累积。
BUFSET.SIZE 的 `@5709 = len - @5606 + 1` 于是与标志流长度错位,
在第二/第三行进入负值空转。

## 3. 修法

`SemanticProfile` 新增 `ReturnCodeMode`:

- `Legacy`:0.495 及更早,保持原行为;
- `EndOnly`(精确 `ysc_version == 555`):只写 `'E'`,并无条件置
  `clear_pending`(对应 `state+0x15`);
- 释放路径(`consume_text_clear_request`)已有,按请求清空
  `_TEXT`/`_TEXT_FLAG` 与游标。

逐字控制(C/R/P/Z)仍由旧代 stdlib 的 `es.TX.OUTPUT.LINEEND` 负责,
引擎不重复写。

## 4. 回归

- 交互复现:`<v555-1>` 进入正文连续推进多句,无
  「テキスト内部エラー」,主任务不再 `END`(主人实测通过);
- `cargo test -p yu-rt --lib`(含 `semantics` 门控断言);
- 0.495 及更早:`ReturnCodeMode::Legacy` 断言覆盖
  450/466/474/479/480/481/486/494/495 与 556/600。
