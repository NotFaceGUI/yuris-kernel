# DIV-RT-057 · CSV/CSVINFO 跨版本处理器与 `LINE`/`DATA` 语义

- 状态:**fixed**(2026-11-19)
- 取证对象:官方 0.466 / 0.479 / 0.495 `yu-ris.exe` 与各自 `YSCom.ycd`;
  本地 v474 商业样本(样本名/哈希不入库)
- 关联:`DIV-RT-011`(手册未取证命令)、`DIV-RT-054`(诊断噪声)

## 1. 四代命令表形状

| 表 | `CSV` | `CSVINFO` | 参数 |
|---|---|---|---|
| 0.466 | 序号 **7** | 序号 **8** | CSV: FILE, **LINE**, LET;CSVINFO: FILE, **LINE**, SIZE, LET |
| 0.479 | 序号 7 | 序号 8 | CSV: FILE, LET;CSVINFO: FILE, SIZE, **DATA**, LET |
| 0.495 | 序号 7 | 序号 8 | 同 0.479 |
| v474 商业样本 | 序号 **8** | 序号 **9** | CSV: FILE, LET;CSVINFO: FILE, SIZE, LET |

关键:**不能只按序号选处理器**——0.466 的 `CSV` 也是序号 7,但它是真实
解析器;0.479/0.495 的序号 7 才是空桩。区分依据是**参数表结构**:
表里有没有 `LINE`。

## 2. 处理器地址(REA)

| 版本 | `CSV` | `CSVINFO` | 备注 |
|---|---|---|---|
| 0.466 | **`0x431970`** | **`0x432050`** | 单元格写入 `0x431f58` |
| 0.479 | `0x43869c`(短桩,候选入口) | `0x4386bc` | 与 0.495 同形 |
| 0.495 | `0x43e640`(空桩) | `0x43e660` | DATA 分支 `0x43e7e3`;SIZE 分支 `0x43e71f`;单元格写入 `0x43f0d8` |
| v474 | `0x43577c` | `0x435cfc` | SIZE 计数,不消费 DATA |

0.495 `CSV` 空桩原始字节与旧记录一致:

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

0.466 `CSV`/`CSVINFO` 的识别:两处都是
`push edi; push esi; push ebp; push ebx; sub esp, …; mov eax, 7|8;
call 0x41d56d`(参数求值器),与 0.495 同构但地址不同。

## 3. 0.466 `CSV[LINE]`(读取器)

- `LINE` 是 **1 基行号**;`<= 0`/缺省 = 全部行。
- 文件字节后追加 CRLF;行号从 1 起,每遇 CR/LF(CRLF 一对) +1。
- 单元格归属终止它的行;写入条件 `0x431c03`/`0x431d9c`:
  `LINE <= 0 || LINE == 当前行`。
- 输出下标 `[esp+0x150]` **只在写入时**递增 ⇒ 过滤后数组从 0 重新计。
- 行尾空单元格规则与 v474 解析器相同(本行出现过逗号 / 整行为空)。

## 4. 0.466 `CSVINFO[LINE/SIZE]`

`0x432050` 的三条路径:

1. `SIZE=1`(在场且非 0):计数,按 `LINE` 过滤;
   - 每个引号外逗号 +1;
   - 每个行终止且本行有内容(`edi==0`)+1;空行不计;
   - **EOF 时最后一行有内容则再 +1**(`0x43225a`)——与 v474/0.495
     的“末行无终止符不计”不同。
2. `SIZE=0` 且 `LINE` 窗口在场:写**行数** = CR/LF 序列数 + 1
   (`0x4322f2` 把当前行号 `ebp` 写回)。
3. 两者都不在场:返回 0。

## 5. 0.479/0.495 `CSVINFO[DATA]`(读取器)

0.495 `0x43e660`:

- `DATA` 非 0 → `0x43e7e3` 分支:打开 FILE、追加 CRLF、按与 `CSV`
  相同的分隔规则解析;
- 每格经 `0x43f0d8(var_id, index, cell)` 写入 `LET` **字符串数组**:
  - 检查变量类型字节 `+1 == 3`(字符串),否则报错;
  - 数组不足时官方 realloc 扩容;
  - 写入后 `index++`,跨行连续;
- `SIZE` 非 0 → `0x43e71f` 分支:只计数(不追加 CRLF,末行无终止符
  不计);`DATA` 与 `SIZE` 同时在场时**先写单元格再写计数**
  (`0x43ee35` 跳回 `0x43e710`)。

## 6. 内核选择(按表结构,不按版本号)

```rust
"CSV" => {
    let has_line = yscm.command(i).is_some_and(|c|
        c.params.iter().any(|p| p.name == "LINE"));
    if has_line || i != 7 { Csv } else { NoopEval }
}
```

- 0.466(有 `LINE`)→ 解析器,即使序号是 7;
- 0.479/0.495(无 `LINE`,序号 7)→ 空桩;
- v474(序号 8)→ 解析器;
- 其它形状按解析器一系处理(`Likely`,代码注释留档)。

`CSVINFO` 由 `op_csvinfo` 按参数表是否含 `LINE` 选计数口径:
含 `LINE` = 0.466 口径,否则 = v474/0.495 口径。

## 7. 解析器语义(跨版本共用)

- 文件字节后追加 CRLF(`len+2`);
- **只有 `,` 受引号保护**:CR/LF 分支在引号判断之前,引号内的换行
  同样断行;
- `"` / `'` 只切换保护状态,引号本身保留在单元格里;
- **单引号状态跨行不复位**(0.466 `[esp+0x14c]` / v474 `[esp+0x148]`
  的行终止路径没有清零),双引号状态会复位;
- 行首空格/Tab 跳过,行尾空白保留;
- 行终止只在“本行出现过逗号”或“整行为空”时产生末尾单元格。

## 8. 残留

- 0.495 `DATA` 的官方数组扩容在本内核里受**固定变量维长**限制:超过
  `LET` 容量时停在最后一个可写下标并挂 `rt.csv` 可恢复诊断(与 `CSV`
  既有口径一致,不静默越界)。
- 0.466 的 `__SAVE_PATH_TYPE`(双下划线 legacy 槽)不属于 CSV 族;
  0.466 二进制里**没有** `yssfs.dat` 字符串,不能按 0.495 的
  `_SAVE_PATH_TYPE` 语义直接别名,另见 `DIV-RT-060`(open)。

## 9. 验证

```text
cargo test -p yu-rt --lib csv_parse_tests
  → 7 passed(引号/换行/单引号跨行/行号/0.466 计数/行数)
cargo test -p yu-rt --test fixture csv
  → 7 passed(0.495 空桩 / v474 解析 / 0.466 LINE 过滤 /
            CSVINFO[DATA] 写数组 / 0.466 LINE 计数与行数)
cargo test -p yu-rt
  → 113 lib + 129 fixture + consistency 全绿
project-0466 (0.466) game_diag_probe 300 帧 → 0 unimplemented / 0 script_errors
project-eris (0.495) game_diag_probe 300 帧 → 仅 EMOTE:12(deferred)
```
