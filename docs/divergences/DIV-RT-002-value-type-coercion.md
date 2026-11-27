# DIV-RT-002 · 表达式里的类型混用(字符串与数值)

- 状态: **fixed(no-manual, 2026-11-27)** —— 仅排序比较 `<` `>` `<=` `>=`;
  `==`/`!=` 混用与字符串算术仍 open
- 影响面: `crates/yu-rt/src/value.rs`(`Value::cmp`/`ordering_number`)
- 关联: `DIV-RT-072`(`_`/`WORD` 文本折叠);错误码 `rt.var.type`
- 证据: 官方 0.495 SDK `yu-ris.exe`(sha256 `4f5eefa3…81138f`)

## 1. 现象

剧本对字符串变量做排序比较(实测 `compare STR 8192`),内核报
`rt.var.type … compare 两侧类型不支持`,该 `IF` 整条被跳过。0.481 样本启动
即命中 287 次,`es_charname.yst pc53` 每次角色名解析都走同一条。

## 2. 官方语义(REA)

字符串压进表达式栈时,官方把**数值槽清零**、只把字符串池索引写进标签:

```text
FUN_0042894c (0x42894c):
  0x42898d  MOV dword ptr [EBP*0x8 + 0x7b41c0], 0   ; 数值槽 lo = 0
  0x428994  MOV dword ptr [EBP*0x8 + 0x7b41c4], 0   ; 数值槽 hi = 0
  0x4289a2  MOV word  ptr [EBP*0x2 + 0x7b91c2], CX  ; 标签 = 字符串池索引
```

读取变量值(`FUN_00427d56` `0x427d56`)对 `kind==3`(STR)走
`0x44e018`(取串)+ `0x42894c`(压栈),不存在「字符串转数字」这一步。

排序处理体 `<` `0x429244`、`>` `0x4291a8`、`<=` `0x429374`、`>=` `0x4292dc`
只读数值槽(整数/浮点两条分支,旁线 `format-vm-opcodes` §3.9.2),没有字符串
分支。因此字符串参与排序比较时的数值就是 **0**:

```text
IF $name < 8192   ⇒   0 < 8192   ⇒ 真
```

## 3. 内核实现

`Value::ordering_number()`:INT/FLT 取自身,STR 取 `0.0`;`lt/le/gt/ge` 全部
走它。不再发 `rt.var.type`。

回归:`value::tests::string_ordering_uses_official_numeric_zero` 同时覆盖
`STR < INT`、`INT > STR`、`STR` 对 `STR`(两串数值槽都是 0,`<=`/`>=` 成立、
严格序不成立)。实机 0.481 样本 400 帧内 `rt.var.type` 归零。

## 4. 仍未取证

- `==`/`!=` 的混用:官方 `0x429038` 同时有 `strcmp` 与数值分支,标签不同的
  两条路径(STR vs INT)未逐指令核对 ⇒ 内核仍报 `VAR_TYPE`,不猜。
- 字符串参与算术(`+` `-` 等):未取证,保持报错。
- 诊断文案已带上操作数(类型、长度、前 32 字节 ASCII 预览),后续取证不必
  回脚本重跑。
