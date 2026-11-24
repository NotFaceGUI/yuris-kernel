# DIV-RT-075 · `LOAD[FILE DNO=1024]` 的 500 槽内存打包读取

- 状态: **fixed(no-manual,2026-11-23)**
- 影响面: `crates/yu-rt/src/vm/handlers/file_io.rs`、
  `crates/yu-rt/src/event.rs`、`crates/yu-save/src/{state.rs,host.rs}`
- 关联: `DIV-RT-071`(`SAVE[MEM=99999]` 打包)、`DIV-RT-056`
  (玩家存档 `DNO=1024` 嵌套容器)
- 样本: v555 2.1MB 族、`<v555-1>`/`<v555-pc>`、`<v555-kuro>`

## 1. 现象

`LOAD[FILE=$file DNO=1024]` 原先落到通用单块读取:把 1024 当普通 YSSD
块解析。官方处理体在文件分支对 `DNO == 0x400` 有独立分支,读的是
12 字节头 + 500 槽打包格式,不写 `LET`。

语料实证:`es_saveload.yst`(不同工程 sid 不同;`<v555-kuro>` s33)
pc412:

```text
LOAD FILE=$2216 DNO=1024 LET=@2218
```

`@2213 < 0` 的 save7xxxx 路径必经。

## 2. 证据

### 2.1 处理体分支

`b02a4dc04ae LOAD 0x4447c8`(反编译 `004447c8_V555H_4447c8.c`):

```text
:338  FUN_0046fff2(file, dno*4 + 0xc, 0);  // 偏移表项
:339  FUN_004700ab(&off,1,4,file);
:340  if (off == 0) { error 0x18ede; return 0; }
:345  FUN_0046fff2(file, off + 4, 0);      // 跳过块头 dno
:346  if (iVar5 == 0x400) {
:347      FUN_004453f4();
:348      close(file);
:349      return 0;
:350  }
```

### 2.2 `FUN_004453f4`

```text
read 3×u32              ; 12 字节头(值不参与判据)
loop 500:
  read u32 raw_len
  read u32 comp_len
  if raw_len == 0: 槽长清零
  else:
    read comp_len bytes
    DAT_00872164 == 0 ? zlib_uncompress : YSSnp_Uncompress
    slot[raw_len] 写入 DAT_008670c0[i]
```

- 打包**块体** = 12 字节块头 + 500 条 `raw_len|comp_len|data`,不经过
  块级压缩;块**载荷**(去掉 12 字节头)从槽 0 的 `raw_len|comp_len`
  开始,没有第二份头。`FUN_004453f4` 在跳过块头后直接读记录。
- `DAT_00872164` 在 555 构建里是 BSS 0,当前只实现 zlib 分支。
- 该分支不读 `LET`;找不到 DNO=1024 时官方报 `0x18ede` 后 `return 0`,
  内核按 recoverable 诊断继续。

## 3. 修法

- `SaveHost::load_mem_pack(file)`:默认未实现;`yu_save::SaveHost` 转发
  到 `SaveState::load_mem_pack`。
- `SaveState::load_mem_pack`:
  - 解析 YSSD,取 DNO=1024 块载荷;
  - 校验 12 字节头、500 条边界、`raw_len/comp_len`;
  - zlib 解压并闭合 `raw_len`,写入 `mem_slots`(空槽移除);
- `Vm::op_load` 在 `SemanticProfile::save_mem_pack` 且
  `mem.is_none() && dno == 1024` 时直接调用宿主 `load_mem_pack`,不落
  通用 `load_block`,不写 `LET`。该门控由命令表是否带 `MEM` 派生:
  0.481/0.494/0.500/0.555 商业表启用;0.495 SDK 表没有 `MEM`,
  其 LOAD 处理体也无 0x400 分支,1024 仍是普通块(DIV-RT-078)。

## 4. 回归

- `yu_save::state::load_mem_pack_reads_official_500_slot_layout`:
  合成 DNO=1024 原始打包块,槽 0/499 读回、空槽不存在;
- `fixture::file_save::load_file_dno_1024_uses_the_mem_pack_host`:
  VM 必须走 `load_mem_pack`,不得走 `load_block`/`load_block_mem`;
- `cargo test -p yu-save` 37 passed / 3 ignored(2026-11-24);
- `cargo test -p yu-rt --lib` 158 passed;
- `cargo test -p yu-rt --test fixture` 156 passed / 7 ignored(2026-11-24)。

## 5. 存档互通(2026-11-23 闭合)

- 官方普通存档 `save50001.sd` 的 DNO=1024 **就是**本分支的 500 槽
  打包块,不是嵌套 YSSD 容器;旧 DIV-RT-056 的“嵌套/第二段”定性已
  勘误为 500 条固定槽表。
- 块载荷不含第二份 12 字节头:12 字节头由外层 YSSD 块头承载,载荷
  从槽 0 的 `raw_len|comp_len` 开始。本轮已修正读侧偏移。
- 写侧:`SAVE[MEM=99999]` 生成完整块体并置待写标记;下一次
  `SAVE[FILE]` 原样写 DNO=1024,成功后清除。系统存档不调用
  `MEM=99999`,不会误带该块。
- `DAT_00872164 != 0`(Snappy)分支未实现;当前 555 静态为 0。
