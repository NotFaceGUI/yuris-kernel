# DIV-007 · YSTB V2(200 < 版本 < 300)已实现

- 状态: **fixed**(2026-11-17;官方 0.292 / 0.255 两套语料全量闭合)
- 优先级: 高(2xx 旧作品入口)
- 发现日期 / 发现方式: 2026-10-19,任务书 B 开工勘察;2026-11-17 取得两套官方
  V2 语料并完成实现。

## 事实(官方产物 > 开源实现)

V2 布局由两套官方语料逐字节实证,并与官方 0.292 `yu-ris.exe`
(sha256 `72628dfa…`)装载器 `FUN_00437da9` 的走查循环一致:

```text
+0x00 char[4] "YSTB"
+0x04 u32     版本(255 / 292)
+0x08 u32     code_size
+0x0C u32     expr_size
+0x10 u32     expr_offset == 0x20 + code_size
+0x14 u32[3]  padding(观测 0)
+0x20 code[code_size]   ← XOR,密钥从下标 0 重新开始
+...  expr[expr_size]   ← XOR,密钥从下标 0 重新开始
```

- 指令头 6 字节(`op, argc, u32 line`);普通参数 `argc × 12`
  (`u16 id, u8 typ, u8 aop, u32 expr_size, u32 expr_offset`)。
- `RETURNCODE` 的 opcode **随 ysc 命令表变化**(0.292=0x38、0.255=0x36),
  必须按工程 `ysc.ybn` 的名字识别,禁止硬编码;0.292 装载器内部写死 0x38
  只是该版本的编译器常量。
- `RETURNCODE` 载荷 4 字节(0.290 为 8 字节);两套语料实测是**本脚本内
  文本行计数**(1 起逐行 +1)。
- `IF/ELSE/LOOP` 的第 2/3 参数 `expr_size` 是 **code 段字节偏移**;
  0.292 的 2,590 个、0.255 的 2,279 个目标全部落在指令起始地址。
- `WORD` 的 expr 数据是裸 CP932 字符串;`_` 仍是表达式。

## 实现

- `crates/yu-format/src/ystb.rs`
  - `Segments::decrypt_with_table` 新增 V2 分支:两段解密 → 走查 →
    合成与 V5 同形的 `inst/args_index/args_data/line_numbers` 视图;
  - 控制流目标换算成组下标;目标不在指令起点 = `fmt.ybn.layout` Fatal;
  - 保留原始 `code/expr/padding`(`V2Raw`),`encrypt` 按原始头版本
    逐字节回写(2xx 文本改写暂不承诺);
  - `v2_code_offsets` 只走 code 段建立偏移表(标签转换用)。
- `crates/yu-format/src/code.rs` 新增 `fmt.ybn.layout`。
- `crates/yu-rt/src/tables.rs`
  - `ScriptFile::parse_with_table` 消费 V2;V5-only 的 `parse` 仍拒绝 V2;
  - `GroupRec.param = 0`(V2 无 gosub_npar;GOSUB/RETURN 只依赖窗口序列);
  - `ScriptFile::pc_for_code_offset` / `code_offset_at`。
- `crates/yu-engine/src/keys.rs` 新增 V2 结构自举(见 §密钥)。

## 密钥

- 0.292:`D3 6F AC 96`(默认 `CRC32("Yu-ris")` 大端字节)。
- 0.255:旧键 `07 B4 02 4A`。
- 自动探测候选:全零、默认、旧键、样本 `+0x2C` 的 u32、首条指令行号
  启发式;每个候选过**完整 V2 走查**,多候选同时通过 = 不唯一 = 失败。

## 验证(原始结果见 PROGRESS 成果 L-1)

- `cargo test -p yu-format --test corpus_v2` → 2 passed:
  - 0.292:112 个 YSTB / 20,117 条指令 / 2,590 个控制目标 / 1,189 个标签,
    逐字节回写一致;
  - 0.255:96 个 YSTB / 16,351 条指令 / 2,279 个控制目标 / 1,026 个标签,
    逐字节回写一致。
- `cargo test -p yu-rt --test corpus_v2` → 3 passed(表装载、标签换 pc、
  全脚本投影、错键拒绝)。
- `yu-cli run <corpus-0292>` headless 600 帧 → 标题画面,0 Unimplemented。
- `yu-cli run <sample-0255>` headless 600 帧 → 标题画面(样本包只含
  ysbin;补同版官方标题资产后 7/7 op 绘制),0 Unimplemented。

## 勘误

- 既有计划记 0.255 为 97 个 YSTB;实测目录/包内是 **96 个 YSTB**
  (103 个 `.ybn` − 7 张元数据表;`yst_list` 100 条,缺 4 个脚本)。
  以文件系统/封包内容为准。
- 既有计划称 YPF `<265` "无哈希";官方 0.255 样本 103/103 条名字哈希
  都是 Adler32(负载也是 Adler32),见 `DIV-016`。未加入无哈希族
  (无官方 oracle,避免放宽校验)。
