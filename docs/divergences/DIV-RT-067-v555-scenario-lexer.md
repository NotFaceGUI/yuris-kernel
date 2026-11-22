# DIV-RT-067 · v555 场景词法器类型码与 `#=` 合并

- 状态:**fixed(2026-11-22)**
- 影响面:`crates/yu-rt/src/other/scenario.rs`(v555 `other` 运行面;
  旧版任何 `version != 555` 不调用)
- 关联:`docs/formats/v555-truth-2026-11-21.md` §11.6.7(本次修正其类型码表)、
  `DIV-RT-065`(GOSUB 三组参数)
- 回归:`crates/yu-rt/src/other/scenario.rs` 11 项单测;`yu-rt` fixture 144、
  `yu-engine` lib 68

## 1. 现象

`<v555-4>`(<main-exe>)启动 f3 弹官方错误框:

```text
ラベル : = が重複しています。
```

点确定后场景链没有建起来,主任务走完、无子任务 → 窗口退出(看似崩溃)。
`DIALOG` 发出点:`s193 ystdlib.yst pc363`;调用方 `s58 es_scenario_label.yst
pc108 GOSUB es._mes`,实参已经带上了空标签名(`:` 与 `=` 之间为空)。

## 2. 根因

`scenario/saku02_01_14h_num007.txt` L62:

```text
#=saku02_01_14h_num007_rp0 //...
```

同文件 L54 的引用是 `\GO.IF.RPMODE0(saku02_01_14h_num007_rp0)`(无 `=`)。

旧词法器把 `#=` 切成 `#` + `=` 两个 token。官方 s58 标签注册(编译版)
在 `#` token 后取**一个** token 作为标签名,于是注册名变成 `=`;全量
scenario 合并数组里**每个**文件都有一行 `#=<file>_rp0`,第二个文件起
全部撞成同一个 `=` → 弹「ラベルが重複」。

## 3. 官方词法器事实(REA b3c02)

样本:`<main-exe>`(v555-2),SHA-256
`a06878de36e9bf50b0d2e25aa9433c830d61a43a7ac0f8987e3008046936897e`。

`FUN_0045a874` @ `0x45a874`(4470 B,词法器):

| 输入 | 类型码 | 文本 | 关键地址 |
|---|---|---:|---|
| `#` | 0x39 | `"#"` | `0x45b1e0` |
| `#=` | **0x39** | **`"#"`** | `0x45b260: CMP ECX,0x3d` → `0x45b2e1: ADD EBX,2` |
| `\` | 0x33 | `"\"` | `0x45b56b` |
| `(` `)` `,` | 0x34/0x35/0x36 | 原字符 | `0x45b4eb/0x45b46b/0x45b3eb` |
| `@` `$` | 0x37/0x38 | 原字符 | `0x45b36b/0x45b2eb` |
| `++` `+=` `-=` `*=` `/=` | 0x0b/0x0c/0x0d/0x0e/0x0f | 运算符两字节 | `0x45b14c/…/0x45ae1e`;字符串表 `0x879ca0` |
| `"…"` | **0x1f** | 引号内文本 | `0x45b883` 分支(遇 `"` 或 `\r` 结束) |
| 标识符/数字 | 0x20 | 词 | 状态机 `0x20` |
| 单个未归类标点 | **0x10** | 该字节 | `0x45ab9f: MOV [EDX+ECX*8],0x10` |
| `;` | — | 跳过(无 token) | `0x45ab2c` |
| `\r` | 0 | `"\r"` | `0x45ad63` |

> truth §11.6.7 旧表写的「其它标点/字符串 = 自身 ASCII」不完整:字符串
> 是 0x1f、未归类标点是 0x10、运算符有 0x0b–0x0f 专用码。本次以 RE 为准。

## 4. 修复

`crates/yu-rt/src/other/scenario.rs::tokenize_stateful`:

1. `#=` 合成一个 token:类型 0x39、文本 `"#"`、前进 2。
2. 字符串:类型 0x1f;遇 `"` 或 `\r` 结束。
3. 运算符对:`++/+=/-=/*=//=` → 0x0b–0x0f。
4. 未归类单字节:类型 0x10、文本 = 字节。
5. `;` 跳过。
6. `/* ... */` 块注释:跨行状态 `LexState::block_comment` 由
   `other::State` 保存,`PRODUCT[COM=22]` 行间传回;块注释内 CR 仍写
   EOL token(0x45acc0),`*/` 后回到正常词法(0x45adb5)。
7. 单独 `-`:按官方 0x45aa34 进词状态 0x5b,`-`/`-5`/`-abc` 是词(0x20);
   词内 `-` 结束词(`a-b` → `a` + `-b`)。
8. 词字符集按官方状态机:字母/数字/`_`/`.`/`/`/lead;`/` 在词内保留
   (`scenario/foo` 是一个词)。
9. 删除词法器顶层 1023 静默截断;`PRODUCT[COM=22]` 在 token 数超过
   STR/INT 数组维度时报 `rt.var.shape`(官方写手越界 0x18b14)。
10. 模块文档更新为 RE 类型码表;`EOL_CODE` 保持 0(§11.6.7)。

## 5. 验证

- `<v555-4>` `probe_dialog` 240 帧:DIALOG=0、ERR=0;TEXT[FILE] 逐文件
  正常加载(含 `sc_tw_scenario/scenario_start.txt` 122 行)。
- 单测:`label_alias_hash_equals_is_one_token`、`operator_pairs_use_dedicated_codes`、
  `unclassified_punctuation_is_code_0x10`、`semicolon_is_skipped`、
  `string_literal_keeps_cp932_bytes_without_quotes` 等 11 项。
- 旧版:0.495/v555-2 未调用 `other::scenario`;fixture 144 + engine 68 全绿。

## 6. 证据边界(非挂账)

1. **块注释 token 类型码/计数**:官方 0x45adb5 在 `*/` 处 memcpy 整段
   注释文本并调用文本写手,但反编译里**看不到 INT 类型码写入与 token
   计数递增**;类型码可能来自未定义路径。当前实现按“跳过、不产 token”
   (行内 CR 仍产 EOL),与“注释不是语法 token”的消费方行为一致。
   若后续 trace 证明官方确实输出注释 token,再补类型码。
2. **越界后的官方流程**:0x18b14 弹错后是否中止整行/任务未取证;当前
   报 `rt.var.shape` 并按数组维度截断写入,不静默。
3. 词法器 token 上限:删除 1023 静默截断;实际上限由输出数组维度
   (`STR[1024]`/`INT[1024]`)决定并报诊断。
