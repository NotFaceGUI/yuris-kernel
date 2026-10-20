# DIV-009 · YSTB V2 版本带(200 < v < 300)与引擎"无版本分支"的关系

- 状态: **open**(实现保持 `Unimplemented`,但错误码与告警显式化)
- 优先级: 低(0.49x/0.55x 语料均为 V5;V2 属旧作品)
- 发现日期 / 发现方式: 2026-10-19,任务 E 修复 F6;REA 复核 0.495 `yu-ris.exe`
  (sha256 `4f5eefa3…`)的 YSTB 装载器 `FUN_004518e9`。

## 事实

- `FUN_004518e9` 读 `+0x04` 版本后**不做任何版本分支**:直接用 `+0x0C..+0x18`
  定位四段、以 `+0x08` count 迭代。也就是说 0.495 引擎对版本字段是容忍的。
- 但引擎"接受"不等于 V2 布局可读:V2 的头是 0x2C 字节、指令内联 6 字节头
  (RxYuris / YU-RIS-Script-Editor 两家一致)。把 V2 文件喂给 V5 路径会按
  V5 段表解释,产出错误但"看似成功"的结果。
- 本仓库无 V2 真实语料。按铁律(无证据不实现),V2 仍返回 `Unimplemented`。

## 本次改动

- 错误码从 `fmt.version` 改为专用 `fmt.ybn.unsupported`(已进 `ALL_CODES`),
  严重级仍为 `Unimplemented`;诊断文案明确写出"0.495 装载器无版本分支,
  但不证明 V2 可读"。
- 回归:`ystb_error_cases` 断言 `code::YBN_UNSUPPORTED` + `Unimplemented`。

## 后续

引入真实 V2 样本后:先按 V2 布局独立解码器实现,再改本档案为 fixed,
并补"V5 解码器拒绝 V2 文件"的负例。
