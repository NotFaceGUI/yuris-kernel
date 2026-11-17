# DIV-009 · YSTB V2 版本带(200 < v < 300)与引擎"无版本分支"的关系

- 状态: **fixed**(2026-11-17;V2 走独立解码分支,按 ysc 名识别 RETURNCODE)
- 优先级: 高(2xx 旧作品入口)
- 发现日期 / 发现方式: 2026-10-19,任务 E 修复 F6;REA 复核 0.495 `yu-ris.exe`
  (sha256 `4f5eefa3…`)的 YSTB 装载器 `FUN_004518e9`。

## 事实

- 0.495 `FUN_004518e9` 读 `+0x04` 版本后**不做任何版本分支**:直接用
  `+0x0C..+0x18` 定位四段、以 `+0x08` count 迭代。引擎"接受"不等于
  V2 布局可读——把 V2 文件喂给 V5 路径会按 V5 段表解释,产出错误但
  "看似成功"的结果。
- 官方 0.292 `yu-ris.exe`(sha256 `72628dfa…`)的装载器
  `FUN_00437da9` 是**另一套**走查:读 `+0x08` code_size / `+0x0C`
  expr_size、code 从 `+0x20` 起、expr 在 `code + code_size`;每指令
  6 字节头 + `argc × 0xC`,并把 opcode `0x38` 当定长 10 字节
  (= 6 + 4,该版本 RETURNCODE)。
- 结论:版本不能作为唯一判据,但"200 < v < 300 走 V2 布局"在两套
  官方语料与两台官方引擎(0.292/0.495)上一致;V2 分支仍做完整结构
  闭合校验,不靠版本号免检。

## 实现

- `Segments::decrypt`(V5-only)对 V2 显式返回 `fmt.ybn.unsupported`,
  保留"V5 解码器拒绝 V2"的反向约束;
- `Segments::decrypt_with_table` 按版本带进入 V2 分支并**按工程
  `ysc.ybn` 的命令名**识别 RETURNCODE(0.292=0x38、0.255=0x36),
  禁止硬编码;
- 合成 V5 视图后仍跑 `self_check`(Σ argc×12、普通窗口 offset+size);
- `ScriptFile::parse`(V5-only)继续拒绝 V2,`parse_with_table` 才接受。

## 验证

- `crates/yu-format/tests/synthetic.rs::ystb_error_cases`:V2 头 + V5-only
  入口仍断言 `fmt.ybn.unsupported`;
- `crates/yu-rt/tests/corpus_v2.rs`:V2 语料走 `parse_with_table` 成功,
  V5-only 入口显式 `table.incomplete`;
- 两套语料 208 个 YSTB 全部闭合(见 DIV-007 / PROGRESS L-1)。
