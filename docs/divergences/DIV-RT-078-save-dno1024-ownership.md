# DIV-RT-078 · 带 `MEM` 表的 `DNO=1024` 占用与跨版本 LOAD

- 状态:**fixed(no-manual,2026-11-24;0.481/0.494/0.500/0.555 与 0.495 已取证)**
- 优先级:中(只影响脚本显式使用 DNO=1024 的边角;正常存/读档不触发)
- 影响面:`crates/yu-save/src/state.rs`(`check_reservable_dno`/`pack_mem`)、
  `crates/yu-rt/src/vm/handlers/file_io.rs`(SAVE/LOAD 1024)、
  `crates/yu-rt/src/semantics.rs`(`save_mem_pack` 派生)
- 关联:`DIV-RT-056`(500 槽内存包)、`DIV-RT-075`(LOAD 1024)、
  `DIV-RT-071`(SAVE[MEM=99999])
- 样本:
  - v555 `b02a4dc04ae`(2.1 MB 族,对应 `<v555-3>`;只记 text hash);
  - 0.481 = `<v481-x>`(LOAD `0x447406`/SAVE `0x44dc19`);
  - 0.495 = SDK `yu-ris.exe`(`0x495` 官方二进制,路径见 AGENTS §2)。

## 1. 结论(2026-11-24 取证)

1. **1024 不是保留号**。带 `MEM` 的 SAVE 处理体(0.481 `0x44dc19`、
   v555 `0x44b874:153-158`)只有通用重复检查,没有 `0x400` 特例;
   普通 `SAVE[DNO=1024 SET=@x]` 与其它 DNO 同路,照常预约/写普通块。
2. **`SAVE[MEM=99999]` 立即把包写进 DNO=1024 的块缓冲**:
   `FUN_0044d82c` 的目标 `DAT_0086a03c` 就是块指针数组(基址
   `0x86903c`)的 DNO=1024 项(`0x86a03c`),不是独立暂存区。下一次
   `SAVE[FILE]`/`SAVE[MEM=n]` 序列化块表时自然带上它,与命令里是否
   写 DNO 无关。
3. **同一写盘窗口内重复预约 DNO 直接报错**:

   ```text
   v555 0x44b874:153-158  if (DAT_0086a13c[DNO] != 0) { ...; return -1; }
   0.481 0x44dc19         cmp dword ptr [DNO*4 + 0x802ffc], 0; jne <error>
   ```

   暂存表在每次 `SAVE[FILE]`/`SAVE[MEM]` 成功后由 `FUN_00469ac0(-1)`
   清零。因此:
   - 普通 1024 先、包后 ⇒ 包的写入**覆盖**块缓冲,不查重复(包赢);
   - 包先、普通 1024 后 ⇒ 重复检查命中,普通 1024 错误返回 -1。
4. **0.481 也有 500 槽读取分支**:`LOAD 0x447406` 的
   `cmp esi,0x400; je 0x447a07` → `0x447cd8`(3×u32 头 + 500 条
   `raw_len|comp_len`),与 v555 `0x4447c8:346` 同型。
5. **0.495 SDK 没有该分支**:表里没有 `MEM`;`LOAD 0x44708c..0x44776c`
   全段反汇编只有 `YSSD`(`0x44535359`)与 `0x1e0` 校验、偏移表
   `dno*4+0xc`、缺块 `0x18ede`,没有 `0x400` 立即数,1024 按普通块。
6. **门控是命令表能力,不是版本号**:0.481/0.494/0.500/0.555 商业表的
   `SAVE/LOAD` 带 `MEM`;0.479 与 0.495 SDK 表没有。四个带 `MEM` 的构建
   都已确认 0x400 分支与重复 DNO 检查:
   - 0.481:LOAD `0x447406`、SAVE `0x44dc19`;
   - 0.494:LOAD `0x4472c0`、SAVE `0x44cda7`;
   - 0.500:LOAD `0x474feb`、SAVE `0x47b2fe`;
   - 0.555:LOAD `0x4447c8:346`、SAVE `0x44b874:153`。

## 2. 当前实现(2026-11-24 对齐)

- `SemanticProfile::from_boot`:按 `SAVE/LOAD` 是否带 `MEM` 参数派生
  `save_mem_pack`;`from_yscm_version` 只保留 v555 的保守默认。
- `SaveState::check_reservable_dno`:只校验 `0..=1024`;**不再拒绝 1024**。
- `set_duplicate_dno_error(bool)`:由引擎按 `save_mem_pack` 置位;
  同一写盘窗口重复预约 DNO(含 1024)返回错误诊断并拒绝该次预约
  (VM 侧命令失败),不覆盖前一块。
- `pack_mem()`:生成包后立即 `staged_dnos.insert(1024)`,对应官方
  直接写块缓冲;同窗口已有的普通 1024 预约被移除并留 verbose 诊断
  (官方静默覆盖),包后普通 1024 预约则报重复。
- `staged_dnos`:当前窗口已暂存 DNO 集合;`SAVE[FILE]`/`SAVE[MEM]` 成功后
  清零(对应官方 `FUN_00469ac0(-1)`),`SAVEEND` 连同未冲刷的包一起清。
- `SAVE[MEM=n]`:内存槽容器也带当前块表里的包(DNO=1024),序列化后
  清包标记;文件与内存槽两条路径都与官方“块缓冲共用”一致。
- `file_io.rs`:删除「普通 1024 一律拒绝」;包由 `pack_mem()` 直接暂存
  DNO=1024,无内容的 DNO 仍按“缺 SET/G_*”未知形态挂号(不再假设它是
  包冲刷)。

## 3. 回归

- `yu-save::state::dno_1024_is_ordinary_and_v555_rejects_duplicate_reservation`
- `yu-save::state::mem_pack_stages_dno_1024_and_rejects_later_reservation`
- `yu-save::state::mem_pack_after_ordinary_1024_overwrites_pack_block`
- `yu-save::state::mem_save_consumes_staged_pack`
- `fixture::file_save::save_file_dno_1024_is_ordinary_on_v555`
- `fixture::file_save::load_file_dno_1024_uses_the_mem_pack_host`(0.555)
- `fixture::file_save::load_file_dno_1024_uses_mem_pack_on_0481`(0.481)
- `fixture::file_save::load_file_dno_1024_stays_plain_block_on_legacy`(0.495)
- `cargo test -p yu-save --lib` → 43 passed / 3 ignored;
- `cargo test -p yu-rt --lib` → 159 passed;
- `cargo test -p yu-rt --test fixture` → 160 passed / 7 ignored;
- env-gated 官方样本:0.481 `<v481-natsu>` `save00151.sd` 的 DNO=1024
  包读回后重打包与官方载荷逐字节一致(`official_player_save_pack_loads`),
  `official_save_flush_preserves_all_blocks` 也通过。

## 4. 残余

- 无未闭环项。包内槽压缩器的 Snappy 分支没有官方写点证据,已按
  `aligned-stub` 固定 zlib,见 DIV-RT-056 §5。
- “DNO 在场但无 SET/组/ID”的官方块来源(`FUN_0044cbb9`)未定性;
  语料未出现该形态,内核按 `unsupported` 处理:显式挂“缺 SET/G_*”,
  不猜块内容。
- 0.495 的 `SAVE[MEM]` 不存在(表无参数),不受本门控影响。
