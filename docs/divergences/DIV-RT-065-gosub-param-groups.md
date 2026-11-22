# DIV-RT-065 · GOSUB 参数三组独立(修复单一 `args[16]` 覆盖)

- 状态:**fixed(2026-11-20)**
- 影响面:`yu-rt::vm`(`GosubFrame`)、`yu-rt::sysvar`(`_PINT/_PFLT/_PSTR`、
  `_RINT/_RFLT/_RSTR`)、v555 剧本链
- 关联:`DIV-RT-007`(系统变量)、`docs/formats/v555-commercial.md` §13/§17
- 回归:`crates/yu-rt/tests/fixture/control.rs::gosub_keeps_pint_and_pstr_groups_apart`

## 1. 事实

官方手册 `control/gosub.html`:

> `GOSUB[#="いろは" PFLT=12.3 PSTR2="ゲーム" PINT3=10]`
> → `@_PFLT(1)` に 12.3、`$_PSTR(2)` に `"ゲーム"`、`@_PINT(3)` に 10。

即 `PINT1` / `PFLT1` / `PSTR1` **编号相同但互不覆盖**:它们分别落进
`@_PINT(1)` / `@_PFLT(1)` / `$_PSTR(1)` 三个不同的系统变量。引擎任务记录
同样是三组数组(旁线 `format-vm-opcodes.md` §3.16:GOSUB 处理体 `0x4459c4`,
参数组 A `+0x10` 整数 / `+0x98` 实数 / `+0x120` 字符串)。

内核旧实现只有一个 `GosubFrame::args[16]`,按「参数编号」落位:同一条
`GOSUB[PSTR="CSEL" PINT=1 …]` 里 `PINT` 写 `args[0]` 后把 `PSTR` 的
`"CSEL"` 覆盖成 `1`。这直接破坏 v555 剧本链:
`es_scenario_parser.yst` 用 `PSTR` 传命令名、`PINT` 传参数序号,
`es.ScenarioMacroExec` 读到的是整数,`es.ST.MACRO.SEARCH` 永远找不到宏,
于是 `es_scenario_lexer/parser` 全程空转。

## 2. 修复

`GosubFrame` 拆成三组(进/出各三组):

```text
args_int[16] / args_flt[16] / args_str[16]   @_PINT/_PFLT/_PSTR
ret_int[16]  / ret_flt[16]  / ret_str[16]    @_RINT/_RFLT/_RSTR
arg_flags[16] / ret_flags[16]                @_PFLG/_RFLG(任一组置位)
```

`GOSUB`/`RETURN` 按 ysc 参数名(经 `param_plan`,名字前缀 `PINT`/`PFLT`/
`PSTR`/`RINT`/`RFLT`/`RSTR`)选择对应组;sysvar 读取也各取本组。
这是**全版本正确性修复**,不是 v555 特例:0.495/0.479 手册与引擎语义相同。

## 3. 验证

- `cargo test -p yu-rt --test fixture`:
  `fixture::control::gosub_keeps_pint_and_pstr_groups_apart` 通过 ——
  同一条 `GOSUB[PINT=21 PSTR="CMD"]` 后,`@_PINT(1)=21` 且
  `$_PSTR(1)="CMD"`。
- v555 样本 headless(120 帧):修复后 `es.ScenarioMacro.CSEL`
  (s70 pc917)实际执行,主任务停在 `es_select2.yst` 的标题选择循环;
  修复前剧本链在宏搜索处空转后执行 `\END` 退出。证据记录见
  `docs/formats/v555-commercial.md` §17 与 `PROGRESS.md`。

## 4. 影响与风险

- 旧版语料未在同一 GOSUB 里混用同编号 PINT/PSTR,行为不变;
  0.495 ERIS 全部用例通过。
- `@_PFLG(n)` 在手册里只说「该编号是否传过」,未区分类型;内核按
  「任一组置位」实现,与旧行为一致。
