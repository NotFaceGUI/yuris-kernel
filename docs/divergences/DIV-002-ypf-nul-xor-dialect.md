# DIV-002 · ypf 两种索引方言并存:NUL-XOR 分支本机无真样本

- 状态: **fixed(回退已删除;方言本身仍无真样本)**
- 优先级: 高
- 发现日期 / 发现方式: 2026-10-19,任务书 A 要求实现 `.ypf` 时,用真实 v494 包核对
  + 参考实现/笔记对 v500 样本的断言。

## 复现

- 所需数据: 任务书指定的样本 `bn.ypf`(v500 / 309 条)在本机不存在
  (`YURIS_SAMPLE_PAC` 指向的目录下无 `bn.ypf`);本机实有 v494 包为
  `LenEncNot` 方言。
- 锚点: 索引区首字节。

## 官方行为

(二进制事实,来自旁线笔记 + 参考实现对 v500 样本的断言,未在本机复现。)

```text
NulXor(key) 索引条目:
  name (逐字节 XOR key,含 1 字节虚拟根 `$`/`%`/`9` [+ 尾部类型码]);
  u8 NUL; u8 flag(0/1); u32 usize; u32 csize; u32 offset; u32 reserved;
  u8 tail[8]
```

- 资源型条目(`.png/.ogg/.wav`)的类型码是**存储原值** `2/5/6` 紧贴名字尾,
  不参与 XOR;这些条目无独立 flag 字节,数据按原样存储。
- 引擎自己的查找形态有两种(带虚拟根 / 不带),参考实现对 v500 样本同时登记。

## 内核行为

`LenEncNot` 由真实 v494 包实测(见 DIV-001)。`NulXor` 仅由合成包测试覆盖:

- `crates/yu-vfs/tests/nul_xor.rs::nul_xor_dialect_is_rejected_with_unimplemented`
- `crates/yu-vfs/tests/nul_xor.rs::corrupted_nul_xor_index_is_rejected`

关键判别顺序(实现选择,非官方文档): 若 `LenEncNot` 能结构完整走完整块,
直接选它;否则对 256 个 key 做预筛 + 全量打分,以“已知扩展名命中率 +
路径/长度自洽”择优,并列时取最小 key(确定性,并记 `vfs.ypf.ambiguous_key`)。

## 二分记录

- [x] `LenEncNot` 真包全量一致
- [ ] `NulXor` 真包全量一致 —— **缺 v500 `bn.ypf` 样本,未做**
- [ ] 虚拟根别名是否应进入 `list()` 输出 —— 当前只进 `open/exists` 查找表,
      不进目录树;官方 `list` 等价行为未观测。
- [x] >4 GiB 封包的偏移表达:`dataOffset` 实为 **u64**
      (REA `FUN_0043fb9c` 读 8 字节;旧笔记把高 4 字节记作 `unknown0`)。
      V3 已按 u64 读取,不再拒绝 >4 GiB 文件。

## 本次(2026-10-19 会话 A 重启)结论

官方 0.495 与旧 v494 商业包**都**是 `HashedLen` 方言(起点 0x20),旧实现"v494 是
LenEncNot、v500 是 NUL-XOR"的二分判断不成立。

## 修复(2026-10-19 任务 E · V1)

**NUL-XOR 的 `Ok` 回退已删除**(REA 复核 `FUN_0045e3b8`:起点恒 `0x20`、
步长 `nameLen + 0x1B`、`T[0xFF-lenEnc]`、名字取反,循环内无版本分支)。
现在:0x20 布局不成立 → `vfs.ypf.unknown_dialect`(Unimplemented);
名字哈希零命中 → 同样 Unimplemented;绝不产出解码后的垃圾名字。
`YpfDialect::NulXor` 变体仅为 ABI/文档保留,本版本不再产出。

回归:`nul_xor_dialect_is_rejected_with_unimplemented`(合成旧方言包必须 Err)。

## 根因

样本缺失,且两种方言的判别在“条目数很少 + 根级文件名”时天然退化
(错误密钥可能拼出短路径)。当前以显式结构自洽率 + 扩展名命中率双重约束收敛。

## 修法

已实现:`src/ypf.rs` 的 `detect_dialect` / `parse_lenenc_block` /
`parse_nulxor_block`。所有未知字段原样保留。

## 回归

```text
cargo test -p yu-vfs --test nul_xor
test result: ok. 2 passed; 0 failed
```

待办: 拿到 v500 `bn.ypf` 后,把 `real_sample.rs` 的样本断言从合成包切换到真包,
并在本档案追加原始输出。
