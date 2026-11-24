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
| `<v555-pieces>/schs70579.sd` | `0x1E0` | 4000 / 4000 | 0 | 全零 |

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
- `pack_mem()` 设置 `mem_pack_pending` 并立即把 DNO=1024 标记为暂存
  (对应官方直接写块指针数组(基址 `0x86903c`)的 DNO=1024 项
  (`0x86a03c`));下一次 `SaveState::flush()`
  或 `SaveState::save_mem(n)` 把块体原样 `set_block(1024, body)`,
  序列化成功后清缓冲(`FUN_00469ac0(-1)` 语义)。
- `SaveState::load_mem_pack()` 读取 DNO=1024 的**块载荷**为 500 条表项
  (不再跳 12 字节),校验 `clen/ulen` 与空槽规则,解压到 `mem_slots`。
- 系统存档(gsd/config/global/seld/…)通常不调用 `MEM=99999`;若包尚未
  消费,任何 `SAVE[FILE]` 都会带出 DNO=1024(官方块缓冲语义)。
- 0.479/0.495 SDK 的表没有 `MEM` 参数;0.481/0.494/0.500/0.555 有,
  由 `SemanticProfile::from_boot` 按表派生(DIV-RT-078)。

## 4. 回归

- `state::mem_99999_pack_matches_official_layout`
- `state::mem_99999_pack_is_written_to_next_file_save_only`
- `state::mem_pack_stages_dno_1024_and_rejects_later_reservation`
- `state::mem_pack_after_ordinary_1024_overwrites_pack_block`
- `state::mem_save_consumes_staged_pack`
- `state::save_end_discards_unflushed_pack`
- `state::empty_mem_pack_is_4000_zero_bytes`
- `state::load_mem_pack_reads_official_500_slot_layout`
- `fixture::file_save::save_mem_99999_packs_memory_slots_instead_of_slot_99999`
- `state::official_save_flush_preserves_all_blocks`(env-gated):对
  `<v555-kuro>`、0.495 SDK `save00001.sd`、`<v481-natsu>` `save00151.sd`
  三份官方存档,`flush` 后所有旧块逐字节保留。
- `cargo test -p yu-save` / `-p yu-rt --lib` / `--test fixture`

## 5. 残余

- **Snappy 包分支按 `aligned-stub` 收口(2026-11-24)**:`DAT_00872164`
  控制包内槽压缩器(0=zlib,非 0=`YSSnp_Compress`)。555 构建全量反编译
  只有读点、没有写点(BSS 初值 0);0.481/0.500 官方存档的包槽首字节
  仍是 `78 01`(zlib level 1)。没有证据表明官方运行期会进入 Snappy
  分支,因此内核固定 zlib 与官方产物同形,不再记为“未实现”。
- **消费时机已取证(2026-11-24)**:`FUN_0044d82c` 的 `DAT_0086a03c`
  就是块指针数组的 1024 项,包在 `SAVE[MEM=99999]` 时立即进入块缓冲;
  下一次 `SAVE[FILE]`/`SAVE[MEM=n]` 序列化块表时自然写出,与命令里
  是否写 DNO 无关。内核按此建模:包即 DNO=1024 暂存块,
  `SAVE[FILE]`/`SAVE[MEM]` 都消费,`SAVEEND` 丢弃(DIV-RT-078 §1.2)。
- 包后普通 `SAVE[DNO=1024]` 按官方重复检查报错;普通 1024 先、包后时
  包覆盖缓冲(官方静默,内核留 verbose 痕迹)。
