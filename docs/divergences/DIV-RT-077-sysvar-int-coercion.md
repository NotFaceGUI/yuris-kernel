# DIV-RT-077 · `_INT`/`_PINT`/`_DROPFILE_NUM` 写入的 RHS 类型转换(待取证)

- 状态:**挂账(待取证,2026-11-24)**
- 优先级:中(浮点/字符串写入边角;整数主路径不受影响)
- 影响面:`crates/yu-rt/src/vm/handlers/variables.rs`、
  `crates/yu-rt/src/value.rs`(`to_int_lossy`)
- 关联:`DIV-RT-074`(LET 系统变量写侧)
- 样本:v555 语料;具体脚本待补

## 1. 现象

修前这三个特殊写入口用 `rhs.as_int().unwrap_or(0)`:

- `Value::Int` 原样写入;
- `Value::Float` / `Value::Str` 返回 `None` → 静默写 **0**。

而普通 `LET @x = 3.7` 走变量表 `coerce_to(Int)`:浮点截断为 3、
字符串按十进制解析。同一句 LET,普通变量与特殊 sysvar 结果不一致。

## 2. 当前实现(挂账)

- 新增 `Value::to_int_lossy()`,直接复用 `coerce_to(ElemType::Int, …)`;
  `_INT`/`_PINT`/`_DROPFILE_NUM` 三处统一使用。
- `_DROPFILE_NUM` 内部字段改 `i32`,保负数;读侧 `i64::from(i32)`。
- `_PINT` 多下标显式报诊断(与 `_INT` 一致)。

## 3. 风险 / 未取证点

1. 官方 v555 处理体对这三类的 RHS 是否也做 `Float→Int` 截断、
   `Str→Int` 解析或报错,未逐条取证;当前是“与普通 LET 内部一致”,
   **不等于官方语义**。
2. `DAT_008724ac`(`_DROPFILE_NUM`)的存储宽度/符号未逐位确认;
   当前按 i32 保负,若官方是 u32 则负值行为不同。
3. `_PINT` 的多余下标/缺参官方行为仍按“不猜、报诊断”处理
   (见 DIV-RT-074 §2.4)。

## 4. 待取证

- v555 LET 处理体 `0x443988` 的 int 分支(id 0x0f/0x32/0x35):
  RHS 是浮点时用 `cvttsd2si` 还是别的?字符串是否解析/报错?
- `_DROPFILE_NUM` 写点(`DAT_008724ac`)的读写宽度与符号扩展;
- 构造脚本:`_INT(1000)=3.7`、`_INT(1000)="42"`、`_PINT(1)=-2`、
  `_DROPFILE_NUM=-1`,在官方 v555 与内核各跑一次,对比变量表/行为;
- 用 trace 或 REA 断点确认官方是否发诊断/任务终止。

## 5. 回归

- 现有:整数写入、`_INT` 标量/数组下标、`_PINT` 1..16 边界、
  `_DROPFILE_NUM` 符号(代码注释);
- 待补:浮点/字符串 RHS 的单元用例(锁定 `to_int_lossy` 语义)。
