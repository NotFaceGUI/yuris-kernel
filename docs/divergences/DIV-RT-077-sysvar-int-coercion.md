# DIV-RT-077 · `_INT`/`_PINT`/`_DROPFILE_NUM` 写入的 RHS 类型转换

- 状态:**fixed(no-manual,2026-11-24;浮点/32 位已对齐,字符串 RHS 以诊断兜底)**
- 优先级:中(浮点/字符串写入边角;整数主路径不受影响)
- 影响面:`crates/yu-rt/src/vm/handlers/variables.rs`、
  `crates/yu-rt/src/value.rs`(`to_int_lossy`)
- 关联:`DIV-RT-074`(LET 系统变量写侧)
- 样本:`b02a4dc04ae`(2.1 MB 族,对应 `<v555-3>`;构建指纹只记 text hash)

## 1. 现象(修前)

这三个特殊写入口用 `rhs.as_int().unwrap_or(0)`:

- `Value::Int` 原样写入;
- `Value::Float` / `Value::Str` 返回 `None` → 静默写 **0**。

而普通 `LET @x = 3.7` 走变量表 `coerce_to(Int)`:浮点截断为 3、
字符串按十进制解析。同一句 LET,普通变量与特殊 sysvar 结果不一致。

## 2. 官方语义(2026-11-24 取证)

### 2.1 浮点 → int:向零截断

v555 LET 处理体 `0x443988` 按**目标类型**选择求值器,int 目标的浮点
RHS 走 `0x420cf4`:

```text
0x420cf4  sub esp,0x1c
0x420cf7  mov byte [0x7e1344],2        ; 结果类型 = float
0x420d10  call 0x420bb4                ; 求值表达式
0x420d15  fld qword [0x7ddb40]
0x420d1b  fnstcw word [esp+8]
0x420d24  or eax,0xc00                 ; FPU 舍入控制 RC = 11(向零)
0x420d2d  fldcw word [esp+0x10]
0x420d31  fistp qword [esp+0x14]       ; 64 位有符号截断
0x420d39  mov eax,[esp+0x14]
0x420d3d  mov edx,[esp+0x18]           ; 返回 64 位 int
```

⇒ 官方是**截断**(3.7 → 3,-3.7 → -3),不是四舍五入;与 Rust
`f as i64` 同口径。溢出时 `fistp qword` 写整数不定值
`0x8000000000000000`,Rust `as i64` 则饱和到边界;该极端值仍未对齐,
属残余(见 §5)。

### 2.2 `_DROPFILE_NUM` 是 32 位存储

`0x443988` int 分支 id `0x0f`:

```text
DAT_008724ac = uVar16;     ; 只写低 32 位
```

读侧把 `DAT_008724ac` 当“拖入文件数”参与循环/分配(如
`0x459670`/`0x45cf18`),是有符号 `int` 用法。内核按 `i32` 保存位型、
读侧 `i64::from(i32)` 保负,与 32 位存储一致。

### 2.3 字符串 RHS 仍未取证

int 结果求值表(`0x8727a4` 起)只安装了 int / float 两个入口
(`0x420cc4`/`0x420cf4`);没有独立的“字符串→int”入口。官方对
`LET @_INT(1000)="42"` 是解析、报错还是静默不写,现有证据无法判定。
内核保持十进制解析(与普通 LET 的 `coerce_to(Int)` 一致),但在三个
写入口各发一条 recoverable 诊断(`rt.sysvar.write`),避免静默分歧;
实机对拍后再按官方结论收紧。

## 3. 当前实现

- `Value::to_int_lossy()` 复用 `coerce_to(ElemType::Int, …)`;
  `_INT`/`_PINT`/`_DROPFILE_NUM` 三处统一使用。
- `_DROPFILE_NUM` 内部字段 `i32`;读侧 `i64::from(i32)`。
- `_PINT` 多下标/缺参显式报诊断(与 `_INT` 一致)。
- 字符串 RHS:三个写入口调用 `note_lossy_int_rhs`,发
  `rt.sysvar.write` recoverable(带脚本/pc),不改写值。

## 4. 回归

- `value::tests::to_int_lossy_truncates_float_toward_zero_and_parses_strings`
  (浮点两向截断、字符串解析/非法串 0);
- 既有:整数写入、`_INT` 标量/数组下标、`_PINT` 1..16 边界、
  `_DROPFILE_NUM` 符号;
- `cargo test -p yu-rt --lib` → 159 passed;
- `cargo test -p yu-rt --test fixture` → 160 passed / 7 ignored。

## 5. 残余

1. ~~浮点超出 i64 范围时的官方不定值~~ **已对齐(2026-11-24)**:
   `trunc_f64_to_i64` 按 x87 `FISTP qword` 对 NaN/Inf/越界写
   `i64::MIN`(整数不定值),不再用 Rust 饱和;回归
   `to_int_lossy_truncates_float_toward_zero_and_parses_strings`。
2. 字符串 RHS 的官方分支(解析/报错/静默写 0)在现有证据下无法判定:
   int 结果求值表只有 int/float 两项,没有独立字符串入口。内核**明确选择**
   十进制解析并每次发 `rt.sysvar.write` 诊断,作为“不静默”的兜底策略;
   这不是待办项——除非拿到 v555 编译器/断点级证据,否则不再改语义。
3. `_DROPFILE_NUM` 负值在官方读侧会当巨大计数使用;内核保位型,
   但未复现脚本写负值的实际路径。
