# DIV-RT-056 · 玩家存档 `DNO=1024` 是 `SAVE[MEM=99999]` 的 500 槽内存包

- 状态: **fixed(no-manual,2026-11-23;原“嵌套容器”定性已勘误)**
- 优先级: 高(玩家存档互通;官方 `LOAD[FILE DNO=1024]` 直接消费)
- 发现日期 / 发现方式: 2026-10-30 / 逐块解压普查;2026-11-23 以
  555 控制存档 + `FUN_0044d7a8`/`FUN_004453f4` 反编译闭合
- 关联: `DIV-RT-071`(`SAVE[MEM=99999]`)、`DIV-RT-075`
  (`LOAD[FILE DNO=1024]` 读取)

## 1. 结论

`DNO=1024` **不是**嵌套 YSSD 容器,也不是“存档历史环”。它是
`SAVE[MEM=99999]` 把 0..499 号内存槽打包后的产物:

```text
块体(外层 YSSD 的 DNO=1024 载荷区) =
  12 字节块头: btype=0 | strict=1 | var=0 | u32 packed_total |
                u32 unpacked_total
  + 500 条:    u32 raw_len | u32 comp_len | comp_len 字节 zlib(level 1)
```

- `raw_len == 0` 是空槽,官方同时写 `comp_len=0` 且不写数据;空槽仍占
  8 字节。
- 因此全空包 = `500*8 = 4000` 个零字节,块头 `clen=ulen=4000`。
- `packed_total = Σ(8 + comp_len)`,`unpacked_total = Σ(8 + raw_len)`,
  两者都包含空槽的 8 字节。
- 外层块头 `clen = packed_total`,`ulen = unpacked_total`。
- 槽数据压缩层固定 zlib level 1(`78 01`),与容器块本身的 zlib/Snappy
  层无关。

## 2. 证据

### 2.1 写侧 `FUN_0044d7a8`(555 `0x44d7a8`)

- 分配 `Σ(8+comp) + 12` 字节;
- 先写 `00 01 00 00`,再写 `u32 0`,再写 `u32 Σ(8+raw)`;
- 循环 500 次:写 `raw_len`、`comp_len`,仅在 `raw_len > 0` 时写压缩数据;
- 最后把 `clen` 回填到偏移 4;
- 调用点是 SAVE 处理体 `0x44ba9c..0x44baa2`:`MEM == 0x1869f(99999)`
  时 `0x44c296 → FUN_0044d7a8` 后立即 `return 0`。

### 2.2 读侧 `FUN_004453f4`(555 `0x445374`)

- 读 12 字节块头(丢弃),再循环 500 次读 `raw_len|comp_len|data`;
- `raw_len==0` 清槽,否则 `DAT_00872164 == 0 ? zlib : YSSnp` 解压;
- 调用点:LOAD 处理体 `0x444aa3` 在 `DNO == 0x400` 时跳到
  `0x4450a4 → FUN_004453f4`。

### 2.3 官方存档控制样本

| 样本 | 容器 | DNO=1024 块头 clen/ulen | 非空槽 | 首槽 |
|---|---|---:|---:|---|
| `<v555-kuro>/save00001.sd` | `0x1E0` | 47044 / 522453 | 9 | `57341 / 4455 / 78 01…` |
| `<v555-kuro>/save00004.sd` | `0x1E0` | 351730 / 3313467 | 56 | 同上 |
| `<v481-natsu>/save00151.sd` | `0x1E1` | 1187162 / 5727527 | 100 | `57369 / 11951 / 78 01…` |
| `<v500-relirium>/save50001.sd` | `0x1E0` | 4000 / 4000 | 0 | 全零 |
| `pieces/schs70579.sd` | `0x1E0` | 4000 / 4000 | 0 | 全零 |

旧文档把首槽后的 `comp_len` 误当成“第二段长度 `len_b`”,并因从槽数据
中间继续按 zlib 解析而失败;实际是 500 条固定表项,不是嵌套容器。

对 `save00001`(9 槽)、`<v500-relirium>/save50001`(全空)与
`<v481-natsu>/save00151`(100 槽)按上述规则重打包,载荷(不含外层
12 字节块头)与官方原文件**逐字节一致**。内核读侧
`load_mem_pack` → `pack_mem` 重打包也在 `save00001` 与
`<v481-natsu>/save00151` 上逐字节复现官方 DNO=1024
(`official_player_save_pack_loads`)。

## 3. 内核实现

- `SaveState::pack_mem()` 生成完整块体(`12 字节块头 + 500 条`)。
- `pack_mem()` 设置 `mem_pack_pending`;下一次 `SaveState::flush()` 把
  该块体原样 `set_block(1024, body)`,写盘成功后消费标记。
- `SaveState::load_mem_pack()` 读取 DNO=1024 的**块载荷**为 500 条表项
  (不再跳 12 字节),校验 `clen/ulen` 与空槽规则,解压到 `mem_slots`。
- 系统存档(gsd/config/global/seld/…)不调用 `MEM=99999`,因此不会带
  DNO=1024;0.495 及更早没有 `MEM` 参数,写盘字节不受影响。

## 4. 回归

- `state::mem_99999_pack_matches_official_layout`
- `state::mem_99999_pack_is_written_to_next_file_save_only`
- `state::empty_mem_pack_is_4000_zero_bytes`
- `state::load_mem_pack_reads_official_500_slot_layout`
- `fixture::file_save::save_mem_99999_packs_memory_slots_instead_of_slot_99999`
- `state::official_save_flush_preserves_all_blocks`(env-gated):对
  `<v555-kuro>`、0.495 SDK `save00001.sd`、`<v481-natsu>` `save00151.sd`
  三份官方存档,`flush` 后所有旧块逐字节保留。
- `cargo test -p yu-save` / `-p yu-rt --lib` / `--test fixture`

## 5. 残余

- `DAT_00872164 != 0` 的 Snappy 包分支未实现;当前 555/481 样本的包槽
  全部是 zlib level 1,静态全局在 555 构建里为 BSS 0。
- 官方 `SAVE[FILE]` 是否在 `MEM=99999` 后无条件消费包、还是按文件类型
  消费,当前用“待写标记 + 写盘成功后清除”建模;系统存档实测均无该块。
