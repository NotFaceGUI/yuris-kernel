# DIV-RT-080 · `LOOP` 计数按浮点求值导致 FLT fatal;`@("")` 被报未实现

- 状态: fixed
- 优先级: 高(启动必经路径)
- 发现日期 / 发现方式: 2026-11-25 / `<game-c>` 桌面启动 fatal + REA 反汇编

## 复现

- 游戏位置: `<game-c>` 启动后首次进入 `data\script\eris\es_select.yst`
- 锚点: `es_select.yst pc544`;`es_scenario_macro.yst pc43`

## 官方行为

`LOOP` 的 `SET` 在 ysc 里是参数类型 3(`ParamKind::Other(3)`)。0.495
`yu-ris.exe`:

- `0x462095` 起把求值表 `0x8018b4` 填成:
  `[1]=0x426870`(INT)、`[2]=0x4268a0`(FLT)、`[3]=0x426870`(INT)…
- `0x426870` 置 `0x7b99c4=1` 后调表达式机 `0x426760`;
- 整数模式下压 f64 常量(`0x4287d0`)走
  `or 0xC00; fldcw; fistp qword` —— **向零截断**后只压 i64 槽;
- `/` 的整数分支(`0x4294ec`)只读 i64 槽,除零写 0。

`@()`(`0x4289d4`)先扫 `.`/`-`/数字;扫描指针等于串尾才算合法。
空串扫 0 字节即到尾部,走十进制解析,结果为 **0**,不报错。

## 内核行为

- `op_loop` 用默认求值器(按运行期值类型分派),pc544 的
  `A*(B*2/16.6667)` 得到 `Value::Float`,随后 `rt.var.type` fatal:
  `LOOP 计数必须是整数,得到 FLT`。
- `Value::cast_to_int` 对空串返回 `rt.undetermined`
  `字符串转数值失败:""`。

## 根因

求值器没有“官方整数路径”这一维;`LOOP` 是整数参数,却按浮点求值。
空串是 `@()` 的合法输入(无剩余字符),旧实现把它当解析失败。

## 修法

- `Evaluator.int_flavor` + `with_int_flavor()`:
  f64 常量向零截断、FLT 变量/下标截断、`binary_int` 走 i64 运算;
- `Vm::eval_window_int` 用于 `LOOP` 计数(常量/单变量/简单表达式快路径
  同样经 `Value::to_int_param` 收口);
- `cast_to_int` 空串(trim 后)返回 0。

## 回归

- `crates/yu-rt/src/expr.rs::tests::int_flavor_truncates_float_literals_and_uses_i64_division`
- `crates/yu-rt/src/value.rs::tests::cast_empty_string_to_int_is_zero`
- `crates/yu-rt/tests/fixture/control.rs::loop_count_uses_official_integer_flavor`
- 命令: `cargo test -p yu-rt --lib -- int_flavor cast_empty` → 2 passed;
  `cargo test -p yu-rt --test fixture loop_count_uses_official_integer_flavor`
  → 1 passed。

## 残余

- `fistp` 对 NaN/超出 i64 范围的官方结果是 `0x8000000000000000`;当前
  `as i64` 是饱和转换。语料未出现该输入,留档待补。
