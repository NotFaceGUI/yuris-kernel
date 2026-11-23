# DIV-RT-071 · SAVE[MEM=99999] 的 0..499 内存槽打包

- 状态: **fixed(no-manual,2026-11-23)**
- 影响面: `crates/yu-save/src/state.rs`、`crates/yu-save/src/host.rs`、
  `crates/yu-rt/src/event.rs`、`crates/yu-rt/src/vm/handlers/file_io.rs`
- 关联: `DIV-RT-058`(存档容器版本)、`DIV-RT-062`(图像块版本布局)
- 样本: `<v555-1>`、`<v555-pc>`、2.1MB 族

## 1. 现象

`SAVE[MEM=99999]` 原先落 `Unimplemented` 诊断;语料每个 v555 作品**各 1 组**
真实调用(<v555-1> s18 pc307、<v555-pc> s31 pc402、2.1MB 样本 s33 pc329),
位置都在 `es.Save` 的 `SAVE FILE` 之前,是必经路径。

## 2. 官方语义(REA)

### 2.1 处理体分支

`b02a4dc04ae SAVE 0x44b874`(反编译 `0044b874_V555H_44b874.c:142-150`):

```text
MEM 参数(kind 1)的值 == 99999
  → FUN_0044d82c();
  → return 0;          // 不再走 FILE / 普通内存槽 / 预约块路径
```

0.495 无 `MEM` 参数,不存在该分支。

### 2.2 `FUN_0044d82c`(`0x44d82c`,829 B)

遍历 0..499 号内存槽(`DAT_008670c0[i]` / 已用长 `DAT_00868080[i]`),
把每个槽的**整个内存存档容器**再压缩,写入全局 `DAT_0086a03c`:

```text
12 字节头:
  +0  u8  0
  +1  u8  1
  +2  u16 0
  +4  u32 packed_total    = Σ(8 + comp_len)
  +8  u32 unpacked_total  = Σ(8 + raw_len)

之后 500 条:
  u32 raw_len
  u32 comp_len
  [comp_len] 压缩数据      // 仅 raw_len > 0 时写
```

- 压缩分支:`DAT_00872164 == 0` → `compress2(level=1)`(zlib);
  非 0 → `YSSnp_Compress`。
- `DAT_00872164` 在 555 构建里位于 BSS(文件初值 0),全量反编译只有
  4 个读点、没有写点;`DAT_0059b6f8`(存档容器压缩层,555 构建静态初值 1)
  是**另一条路径**,不参与本打包。
- 空槽:`raw_len = 0`、`comp_len = 0`,不写数据。
- 官方 exe 内除 `FUN_0044d82c` 自身外**没有** `DAT_0086a03c` 的直接引用;
  `LOAD[FILE DNO=1024]` 的 `FUN_004453f4` 读的是**同型文件**,不是这个全局。

## 3. 修法

- `SaveState::pack_mem()`:
  - 遍历 0..499,取 `mem_slots` 的 YSSD 容器字节;
  - 空槽写 `0/0`;非空槽 `zlib_compress_level1`;
  - 按 §2.2 写 12 字节头 + 500 条,存入 `packed_mem`;
  - `packed_mem()` getter 供取证/后续导出。
- `SaveHost::pack_mem()`:默认 `Unimplemented`;`yu_save::SaveHost` 转发到
  `SaveState::pack_mem`。
- `SAVE` 处理体 `MEM=99999` 分支改为调用宿主 `pack_mem()`;
  不再把 99999 当普通槽,也不产生 Unimplemented。
- **不实现** `LOAD[MEM=99999]`:官方 LOAD 处理体没有 99999 特殊分支
  (只有 `LOAD[FILE DNO=1024]` 读同型文件);现有 `LOAD` 的 99999 检查
  保持显式未支持,不猜。

## 4. 回归

- `crates/yu-save/src/state.rs::mem_99999_pack_matches_official_layout`:
  - 头部 `0,1,0u16`、`packed_total = len-12`、`unpacked_total = Σ(8+raw)`;
  - 500 条逐项长度、空槽 `0/0`;
  - 每个非空槽 `zlib` 解包后等于 `SAVE[MEM]` 容器。
- `crates/yu-rt/tests/fixture/file_save.rs::
  save_mem_99999_packs_memory_slots_instead_of_slot_99999`:
  - `MEM=99999` → 宿主 `pack_mem` 一次,普通槽不出现 99999;
  - 后续 `SAVE[MEM=7]` 仍走 `save_mem`;
  - 不刷文件、无错误。
- `cargo test -p yu-save`(32 passed,含样本环境);
- `cargo test -p yu-rt --test fixture`(146 passed / 7 ignored)。

## 5. 限制

1. `DAT_00872164` 的运行时写点未定位;当前按静态默认值 0 使用 zlib。
   若未来发现命令行/配置会置 1,需要在宿主侧增加 pack codec 选择,
   并补 `YSSnp_Compress` 分支。
2. `packed_mem` 在官方 exe 内无直接消费者;本实现保留字节,
   不把它接入任何脚本可见状态。
3. `LOAD[MEM=99999]` 不是官方形态,仍按未支持处理。
4. 复合形态 `SAVE[MEM=99999 FILE=...]`:官方在判 99999 之前先做 FILE
   名称校验(`0x44b874:83-141`);当前实现先判 99999。语料里该分支
   均为 `MEM=99999` 单参,无 `FILE` 同组;此边缘顺序未复刻,
   不据此猜 FILE 校验语义。
