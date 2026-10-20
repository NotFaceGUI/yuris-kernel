# DIV-007 · YSTB V2(200 < 版本 < 300)无语料,不实现

- 状态: **open**(等真实 V2 语料到手再实现;拿到前显式 `Unimplemented`)
- 优先级: 低(0.49x/0.55x 语料均为 V5;V2 属旧作品)
- 发现日期 / 发现方式: 2026-10-19,任务书 B 开工勘察;开源两家(RxYuris
  `YSTB.cpp`、YU-RIS-Script-Editor `core/ystb.py`)都有 V2 支路,但本仓库
  语料(ERIS 官方产物,版本 495)无 V2 形态。

## 事实

- V2 布局(RxYuris / Script-Editor 二手一致):头 0x2C 字节
  (魔数/版本/codeSegSize/argsSegSize/…),code 段内联 6 字节指令头
  (`op, argc, 4 字节`)与 `argc × 12` 参数项,参数载荷在独立 args 段;
  分段异或与 V5 同族(各段从密钥下标 0 重启)。
- 按仓库铁律(无证据不实现、禁止臆造),V2 解码在本 crate 显式返回
  `Diag{ code: "fmt.version", severity: Unimplemented }`。

## 回归

- `cargo test -p yu-format --test synthetic`(子集 `ystb_error_cases`):
  合成 V2 头 → 断言 `Unimplemented`。
- 后续引入 V2 时:先取真实样本(任务书 B §0"先找语料再实现"),
  参考上述两实现重走本档案,再实现并改本档案为 fixed。
