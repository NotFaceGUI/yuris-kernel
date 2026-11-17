# DIV-011 · V2 YSLB `field_b` 是 code 段字节偏移,装载期换成组下标

- 状态: **fixed**(2026-11-17)
- 优先级: 高(2xx 标签跳转/入口)
- 发现日期 / 发现方式: 2026-11-17,两套官方语料实测 + 0.292 引擎
  `FUN_0043f5d0`(YSLB 装载器)。

## 事实

- `ysl.ybn` 记录尾 `+0x04`(`field_b`)在 V5 是**指令下标**,在
  `200 < version < 300` 是**code 段字节偏移**。
- 0.292:1,189 个标签的 `field_b` 全部落在某条指令的起始偏移;
  0.255:1,026 个全部落在指令起点。
- 0.292 YSLB 装载器只原样保存 `field_b`,不转换;转换发生在跳转消费侧。
  本仓库选择在**装载期**转换(失败早、执行期只消费 pc)。
- 目标偏移不在指令起点 = `table.incomplete` Fatal;不能把偏移当组下标
  执行(会产生"看似能跑"的错位)。

## 实现

- `yu_format::ystb::v2_code_offsets(data, key, table, origin)`:
  只 XOR/走查 code 段,返回升序指令起始偏移(下标 = 组下标)。
- `TableSet::load_from`:标签表版本 < 300 时,按 `script_id` 预读
  `ysbin/yst%05d.ybn`(只走指令头,不完整解码),`binary_search` 换 pc,
  结果写回 `Label.pc`;找不到 = `table.incomplete`。
- `ScriptFile::pc_for_code_offset` / `code_offset_at` 暴露同一映射;
  控制流目标在 `Segments::decrypt_v2` 内部用同一张偏移表换 pc。

## 验证

- `crates/yu-format/tests/corpus_v2.rs`:1,189 / 1,026 个标签全部可换算;
- `crates/yu-rt/tests/corpus_v2.rs`:每个标签的 pc < 目标脚本组数,
  `pc_for_code_offset(code_offset_at(pc)) == pc` 全量成立。
