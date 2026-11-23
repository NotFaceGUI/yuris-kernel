# DIV-RT-078 · v555 `DNO=1024` 的普通 SAVE 占用与跨版本 LOAD(待取证)

- 状态:**挂账(待取证,2026-11-24)**
- 优先级:中(只影响脚本显式使用 DNO=1024 的边角;正常存/读档不触发)
- 影响面:`crates/yu-save/src/state.rs`(`check_reservable_dno`)、
  `crates/yu-rt/src/vm/handlers/file_io.rs`(SAVE/LOAD 1024)
- 关联:`DIV-RT-056`(500 槽内存包)、`DIV-RT-075`(LOAD 1024)、
  `DIV-RT-071`(SAVE[MEM=99999])
- 样本:v555 语料;具体脚本待补

## 1. 已取证部分

- v555 `LOAD[FILE DNO=1024]` 有专用分支(`0x4447c8` → `FUN_004453f4`),
  按 500 槽内存包读取,不写 `LET`(DIV-RT-075)。
- v555 `SAVE[MEM=99999]` 的 500 槽包写到下一次 `SAVE[FILE]` 的 DNO=1024
  (DIV-RT-056/071)。

## 2. 未取证部分(本轮挂账)

1. **v555 普通 `SAVE[DNO=1024 SET=@x]`**:官方是拒绝、写入普通块,
   还是后续内存包覆盖?目前没有任何反编译/语料证据。
   当前实现:显式 recoverable 诊断并拒绝预约,避免与内存包静默同 DNO。
2. **0.495/0.481 的 `LOAD[FILE DNO=1024]`**:旧版处理体是否存在 0x400
   专用分支未取证。当前实现:非 v555 一律按普通块(DIV-RT-075 §3 门控)。
3. **冲突语义**:同一容器内既预约普通 DNO=1024、又有待写内存包时,
   官方以谁为准未取证。当前实现:`build_container` 直接报冲突错误。

## 3. 当前实现(防御性,非官方结论)

- `SaveState::check_reservable_dno`:v555 下 1024 保留给内存包;
  0..=1024 范围由宿主接受,FILE 的 1 基由 VM 卡。
- `file_io.rs`:v555 普通 `SAVE[DNO=1024]` 先报 recoverable 并 Continue;
  `LOAD` 1024 仅在 `SemanticProfile::save_mem_pack` 为真时走包读取。
- 回归:`mem_pack_reserves_dno_1024_only_on_v555`、
  `load_file_dno_1024_uses_the_mem_pack_host`(v555)、
  `load_file_dno_1024_stays_plain_block_on_legacy`(0.495)。

## 4. 待取证

- v555 SAVE 处理体(`0x44c296` 内存包写点附近)对 DNO 的特殊判断:
  是否在预约阶段就把 1024 排除,还是写入时覆盖;
- 构造脚本:`SAVE[DNO=1024 SET=@x]`(带/不带 `MEM=99999`),
  用官方 v555 存档后逐块解析,确认 DNO=1024 内容/谁胜出;
- 0.495/0.481 官方 `LOAD[DNO=1024]`:是否报 `0x18ede`/按普通块;
- 若官方允许普通块占用 1024,则需要把“拒绝”改成“允许但内存包优先/报冲突”
  并同步本档案与回归。

## 5. 风险

- 在取证前,显式使用 DNO=1024 的第三方脚本会被我们拒绝(可恢复诊断,
  不终止任务);正常玩家存档不受影响。
