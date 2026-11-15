# DIV-RT-057 · CSV/CSVINFO 按命令表序号区分两代处理器

- 状态:**fixed**(2026-11-15)
- 取证对象:官方 0.495 `yu-ris.exe`(`4f5eefa3…`)与本地 v474 商业样本;
  两代的 `ysc.ybn` 命令表

## 1. 两代命令表不同位

| 表 | `CSV` | `CSVINFO` | 参数 |
|---|---|---|---|
| 0.495(ERIS 语料 `ysc.ybn`,0.495 SDK) | 序号 **7** | 序号 **8** | CSV: FILE, LET;CSVINFO: FILE, SIZE, DATA, LET |
| v474 商业样本 | 序号 **8** | 序号 **9** | CSV: FILE, LET;CSVINFO: FILE, SIZE, LET |

v474 的表在 CSV 前多一个槽位,序号整体后移一位。

## 2. 处理体不同

- **0.495 `CSV`(序号 7)→ 空桩**。`0x43e640` 的原始字节:

```text
56              push esi
B8 07 00 00 00  mov eax, 7
E8 86 B9 FE FF  call 0x41d461      ; 参数求值器
0F BE C0        movsx eax, al
85 C0           test eax, eax
74 07           je  +7
B8 FF FF FF FF  mov eax, -1
59 C3           pop ecx; ret
33 C0 59 C3     xor eax,eax; pop ecx; ret
```

即:只求值参数,失败 -1,否则 0;不读文件、不写 LET。

- **v474 `CSV`(序号 8)→ 真实解析器 `0x43577c`**:打开 FILE、追加 CRLF、
  按 `,`/CR/LF 切分、逐格写 LET。`CSVINFO`(序号 9)→ `0x435cfc`,只计数。

## 3. 内核选择(不按版本号)

`build_handler_table` 用**工程 ysc 表里的序号**决定:

```rust
"CSV" => if i == 7 { HandlerType::NoopEval } else { HandlerType::Csv }
```

- 序号 7(0.495 形状)→ 空桩,与官方一致;
- 序号 8(v474 形状)→ 解析器;
- 其它序号无样本,按解析器一系处理(`Likely`,代码注释留档)。

## 4. 解析器语义(REA `0x43583d..0x435bb5` / `0x435df7..0x435f42`)

- 文件字节后追加 CRLF(`len+2`);
- **只有 `,` 受引号保护**:`0x43591b` 的 CR/LF 分支在引号判断之前,
  引号内的换行同样断行;
- `"` / `'` 只切换保护状态,引号本身保留在单元格里;
- **单引号状态跨行不复位**(官方 `[esp+0x148]` 在行终止路径没有清零),
  双引号状态会复位;这是官方既有行为,内核照转;
- 行首空格/Tab 跳过,行尾空白保留;
- 行终止只在“本行出现过逗号”或“整行为空”时产生末尾单元格;
- CSVINFO 不追加 CRLF,末行无终止符时最后一格不计入。

## 5. 残留

0.495 的 `CSVINFO` 多一个 `DATA` 槽,其语义未取证;内核只写 `LET` 计数,
不消费 `DATA`(不猜)。若后续拿到 0.495 商业样本调用 CSVINFO 的 trace,
再补 `DATA` 语义。

## 6. 验证

```text
cargo test --release -p yu-rt csv
  → 4 个单元解析测试 + 4 个夹具测试全绿
  (0.495 空桩不写 LET / v474 形状解析并计数 / 缺文件让出)
v474 样本 headless --frames 5 → boot 通过
```
