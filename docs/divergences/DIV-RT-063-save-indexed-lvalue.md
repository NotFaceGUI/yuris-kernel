# DIV-RT-063 · SAVE/LOAD 带下标的左值被当成整组变量:qsave 的 DNO2 变 Int[256,128],官方 QLOAD 维数不符

- 状态: fixed(2026-11-20)
- 优先级: 高(存档内容与官方不兼容;本内核读回也会丢块)
- 发现日期 / 发现方式: 2026-11-20 / 对照两个 0.479 商业游戏
  (`黒獣2`、`聖奴隷学園2`)的官方 `save00001.sd` 与本内核 `save10001.sd`
  逐 DNO 解出类型/维数时发现。

## 复现

- 游戏位置 / 操作序列:`黒獣2` 存档 → 官方 `save00001.sd` 与本内核
  `QSAVE` 的 `save10001.sd` 逐块解析;
- 差异:DNO=2 官方是 **Int 标量 14**(dims=`[]`),本内核是
  **Int[256,128]**(262144B,全 0)。其余 DNO 类型/维数一致。

## 官方行为

`es_saveload.yst` 的 `SAVE` 序列里:

```text
SAVE[DNO=2 SET=@0x45d[0x8d][1]]
```

`@0x45d` 在 `ysv`/`gsd.sd` 里是 Int[256,128];带**两个完整下标**的 SET
保存的是该**元素**(标量块)。官方 0.479 LOAD 对块类型 0 做严格
类型/维数比对(`FUN_00440490` 0x440c22..:块 type/dims 与 LET 目标
当前变量逐项比较,不符即报错退出),因此标量块只能写回 `LET` 的
**下标元素**。

官方 `save00001.sd` 的 DNO=2 = `00 00 00 00 | 0e 00 00 00 ...`
(type=1,dim=0,data=14)是直接证据。

## 内核行为

两处对称缺陷(2026-11-20 前):

1. `op_save` 的 `SET` 分支已经按 `lv.indices` 读出了元素值,但生成预约快照
   时调用 `var_snapshot(var)` **忽略下标**,把整个 `@0x45d`(Int[256,128])
   写进块;
2. `op_load` 对块类型 0 的写回调用 `restore_snapshot(var, snap)`,同样
   **忽略 LET 下标**;官方标量块对 Int[256,128] 目标做整组恢复,元素数
   不符 → 本内核报 `rt.var.shape` 并丢弃该块。

后果:

- 官方 QLOAD 读到本内核 qsave 的 DNO2:块 dims=[256,128],目标
  `@0x45d[141][1]` 是标量 → 维数不符,读档失败(与缩略图布局问题
  `DIV-RT-062` 叠加);
- 本内核 QLOAD 官方 qsave/自身 qsave 时 DNO2 不恢复,游戏状态缺变量。

## 修法

1. 新增 `var_snapshot_at(var, indices, value)`:
   - `indices` 为空 → 整组快照(原语义,`SET=@x`/`@x()`);
   - `indices` 数 == 变量维数 → **标量块**(`dims=[]`,单元素 = 该下标值);
   - 部分下标(读子数组)官方语义未取证 → 不生成块(显式拒绝,不猜)。
   `op_save` 的 SET 分支改用它;系统变量仍按引用值成标量块。
2. `restore_snapshot(var, indices, snap)`:
   - `indices` 为空 → 原整组恢复(块类型 2 按 var_id 恢复同样走这里);
   - 完整下标 → 要求块是标量同型,`store.write(var, indices, element)`
     只写该元素;
   - 部分下标 → 显式 `rt.var.shape`(未取证)。
   `op_load` 块类型 0 使用 `LET` 的 `lv.indices`,块类型 2 仍按
   `var_id` 恢复整组(不带 LET 下标)。
3. 0.479 写盘时块类型 0 的 `var_id` 写 0、0.495 系写实际 id
   (`DIV-RT-062` 同批)。

## 回归

- `yu-rt` fixture:
  - `save_set_on_array_element_reserves_scalar_snapshot`
  - `load_into_array_element_writes_only_that_element`
  - 既有 `save_load_loop_records_reserve_flush_and_writes_back`(整组语义
    不变)与 `save_set_on_system_variable_reserves_scalar_snapshot` 通过。
- 真实存档对照:DNO=2 新写盘为 Int 标量(=14 量级),与官方
  `save00001.sd` 同形。

## 残余

- 部分下标(`SAVE[SET=@x[i]]` 且变量 2 维)没有官方样本;当前不生成/不
  恢复,留给有样本时补。
