# DIV-RT-072 · `_`/`WORD` 文本输出的折空格与缓冲上限

- 状态: **fixed(no-manual,2026-11-23)**
- 影响面: `crates/yu-rt/src/vm/handlers/text.rs`、
  `crates/yu-rt/src/text.rs`、`crates/yu-rt/src/code.rs`
- 关联: `DIV-RT-032`(`_TEXT`/`_TEXT_FLAG` 写入)、`COV-M-03`
- 样本: 0.495 SDK、`<v555-1>`/`<v555-pc>`/2.1MB 族

## 1. 现象

`_` 的表达式值含控制字节时,内核把原字节直接写进 `_TEXT`;`_`/`WORD`
也没有官方缓冲上限。官方会把非打印单字节折成空格,并在超限时返回 -1
让出。

## 2. 证据

### 2.1 `_` 折空格

0.495 `0x4567c0`:

```text
0x4568c9  cmp eax,0x20
0x4568cc  jl  0x4568e1      ; <0x20 → 0x20
0x4568ce  cmp eax,0x7f
0x4568d1  jge 0x4568e1      ; >=0x7f → 0x20
0x4568ea  mov byte ptr [edi+ecx],0x20
```

v555 `0x455cd8` 同型(`0x455dd9..0x455dfc`)。前导字节先按构建的静态前导表
判断(v555 `0x59b0a0`;0.495 `0x5b40c0`),双字节整体保留,不折尾字节。

### 2.2 上限

| 命令 | 官方判据 | 行为 |
|---|---|---|
| `_` | `pos + len >= 0x1FFE`(v555 `0x455d9a`;0.495 `0x456882`) | 报 `0x18e70`,返回 -1 |
| `_` 源串 | `len > 0x400`(v555 `0x455da6`;0.495 `0x45688e`) | 报 `0x18e8e`,返回 -1 |
| `WORD` | `pos + len > 0x1FFE`(v555 `0x45854d..0x458557`) | 报 `0x18e70`,返回 -1 |

`pos` = `state+0x20` 当前文本写位;`WORD` 与 `_` 的阈值有一字节差
(`WORD` 允许正好写到 `0x1FFE`,`_` 只允许到 `0x1FFD`),按处理体原样复刻。

## 3. 修法

- `TextState::fold_text_out_bytes(codec, bytes)`:
  - 前导字节 + 尾字节原样;
  - 非前导 `<0x20`/`>0x7E` → `0x20`;
  - 其余原样。
- `op_text_out`(按官方顺序:先缓冲、后源串):
  - `self.text.bytes.len() + bytes.len() >= 0x1FFE` → `rt.text.out.limit` + `Yield`;
  - 源串 `>0x400` → 同码 + `Yield`;
  - 追加/事件都用折叠后的字节(折叠只替换字节值,长度不变)。
- `op_word`: `self.text.bytes.len() + len > 0x1FFE` → 同码 + `Yield`,不追加、不发 Text 事件。
- 两条命令在 0.495 与 v555 语义相同,**不按版本门控**;`WORD` 不折空格。

## 4. 回归

- `text_out_fold_keeps_lead_pairs_and_spaces_controls`(CP932 0x82B1
  保留、Big5 0xA1 0x41 尾字节不折);
- `text_out_folds_control_bytes_and_preserves_lead_pairs`(VM 级);
- `text_out_rejects_source_over_0x400_and_buffer_over_0x1ffd`;
- `word_rejects_buffer_over_0x1ffe`;
- `cargo test -p yu-rt --lib`(154 passed);`--test fixture`(见 PROGRESS)。

## 5. 残余

- 官方 `pos` = `state+0x20`;内核用 `text.bytes.len()` 作等价代理。
  正常 `_`/`WORD` 追加与 `FUN_00469f9c` 清缓冲下两者一致;脚本
  `LET $_TEXT = …`(`set_text`,DIV-RT-032)只替换字节、不移动官方游标,
  此后本上限判定可能提前/滞后。抽查 v555 文本脚本 s104/s105/s113 未见
  `_TEXT` 写入;要完全 1:1 需给 `TextState` 增加独立写游标。
- 源串最后一个字节是孤立前导字节时,官方从 NUL 终止缓冲再取一字节写入
  (前导+下一字节);内核只保留该前导。只影响非法编码串,语料未出现。
