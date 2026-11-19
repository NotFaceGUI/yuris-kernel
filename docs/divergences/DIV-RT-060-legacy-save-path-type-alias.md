# DIV-RT-060 · 0.466 `__SAVE_PATH_TYPE` legacy 槽

- 状态:**open(evidence)**
- 影响面:`yu-rt::sysvar`(仅 0.466 表)
- 关联:`DIV-RT-007`(手册未取证系统变量)、`DIV-RT-009`

## 1. 事实

| 版本 | ycd 槽名 | id | 二进制含 `yssfs.dat` |
|---|---|---:|---|
| 0.466 | `__SAVE_PATH_TYPE`(双下划线) | 77 | **否** |
| 0.479 | `_SAVE_PATH_TYPE` | 90 | 是 |
| 0.495 | `_SAVE_PATH_TYPE` | 112 | 是 |

- 0.495 的 `_SAVE_PATH_TYPE` 语义已收口(`fixed(no-manual)`):getter
  `FUN_00427350` case 112 读 `[0x801324]`;写侧 `FUN_00460c80` 从
  `<app>\yssfs.dat` 偏移 0x54 读 u32(只接受 0..=3),见
  `crates/yu-rt/docs/divergences/README.md` DIV-RT-007 的 2026-10-30 条。
- 0.466 的 ycd 把名字写成 `__SAVE_PATH_TYPE`(多一个下划线),且
  **0.466 `yu-ris.exe` 全字节搜索没有 `yssfs.dat`**;0.479/0.495 都有。
- 0.466/0.495 两版手册都没有该名字的页(`basic/svariable.html` 0 命中)。

## 2. 为什么没有直接别名

把 `__SAVE_PATH_TYPE` 直接映射到 0.495 的 `vm.save_path_type()` 会引入
两个未取证假设:

1. 0.466 的值源是否也是 `yssfs.dat` 偏移 0x54(0.466 没有该文件名字符串,
   可能走别的配置源/恒 0);
2. 0.466 的 `__` 前缀是否是编译器的保留命名,而不是同名变量。

因此内核当前让它落入 `sysvar.rs::undoc()` → `rt.sysvar.unimplemented`,
**不猜**。它不是命令参数残项;CSV/SAVE/LOAD 的修复见
`DIV-RT-057` / `DIV-RT-061`。

## 3. 解除条件

任一条即可定案:

1. REA 0.466 getter 分发表中 id 77 的处理体,确认全局/常量来源;
2. 0.466 商业样本或官方 SDK 工程中 `__SAVE_PATH_TYPE` 的实际引用 +
   可观测值;
3. 0.466 的配置读取链(`yssfs.dat` 等价物)定位。

定案后按 `fixed(no-manual)` 或 `aligned-stub` 归档,并在
`crates/yu-rt/docs/coverage-matrix.md` §3 补行。
