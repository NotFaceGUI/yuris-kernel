# 官方 trace 采集器(任务 H)

本目录只放**仓库外运行**的采集工具;截图/语料/trace 一律写到仓库外或 `target/`,
不入库。采集器不修改官方二进制、不修改游戏文件。

## 1. 当前等级

| 等级 | 状态 | 证据 |
|---|---|---|
| **屏幕级(screen)** | 已达成 | `capture_screen.py` 启动官方 0.495 `yu-ris.exe` + ERIS 数据,截取窗口客户区 800×600,写 schema v3 `frame/frame_description`(`level=screen`) |
| **事件级(L1)** | **未达成** | `probe_dispatch.py` 在 0.495 命令分派点 `0x4044db` 下只读断点,25 s 内 0 命中(标题画面);未注入/未改二进制 |

> 屏幕级只能证明"画面摘要",不能替代事件级 L1。事件级命令流、参数、变量写点尚未采集;
> 报告与 PROGRESS 必须保持这个标注。

## 2. 屏幕级采集

```bash
python tools/official_trace/capture_screen.py \
  --exe <官方 yu-ris.exe> --cwd <工程根> \
  --out <仓库外输出目录> --frames 6 --interval 0.4 --warmup 8
```

输出:

```text
<out>/frame_0001.png ...       客户区截图(不入库)
<out>/official_screen.jsonl    schema v3 trace(producer=official, level=screen)
<out>/input_timeline.json      帧号 -> 输入(首屏为空)
<out>/manifest.json            引擎/样本 hash、窗口尺寸、等级声明
```

屏幕摘要定义(官方与内核必须一致):

```text
pixels_sha256 = SHA-256(b"RGBA8" + width:u32 LE + height:u32 LE + RGBA bytes)
display JSON  = {"width":W,"height":H,"pixels_sha256":...}
display_sha256 = SHA-256(display JSON 规范文本,无空白,键序 width/height/pixels_sha256)
```

屏幕级记录的 `pass_count/op_count/executable_ops/skipped_ops`、`boot` 配置在引擎外部
不可观测,按 0/默认值记录;**不是实测 0**。`level=screen` 已表达这一点,manifest 里也
重复声明。

## 3. 运行环境与安全

- 64 位 Python(本机 3.12)、Pillow;Windows 桌面会话。
- 采集器是只读观测:启动进程、`PrintWindow` 截图、可选 `PostMessage` 输入;
  结束时 `terminate` 引擎。不写游戏目录、不写二进制。
- 截图与 trace 不提交;`manifest.json` 可能含本机绝对路径,只用于复核。
- 若窗口被遮挡,`PrintWindow` 仍能取到客户区;失败时才回退到前台截图。

## 4. 事件级路线(已定位、未打通)

0.495 `yu-ris.exe`(sha256 `4f5eefa3…`)的静态事实(REA/capstone 复核):

| 项 | 值 |
|---|---|
| 命令处理器表 | `0x7ace20`,步长 4,共 119 项(与 `yscm-commands-495.json` 的 119 个名字对齐) |
| 表初始化器 | `0x45d1c2`–`0x45d550` 一带的 `mov dword ptr [0x7ace..], imm32`;未显式写入的槽 = 默认 stub `0x4634d8` |
| 任务泵 | `0x404398`;`[0x80171c]` = 当前任务记录;pc 在 `task+0x20`(取指后自增) |
| 命令分派 | `0x4044db: call dword ptr [ecx + edi*4]`;`EDI` = 本命令 pc,`ECX` = 本脚本的正常/跳过处理器表 |
| 脚本 ID | `task+0x3c`(u16,与旧线 v555 配置的 `script_id_off` 同偏移) |
| 任务泵调用者 | `0x40416a`、`0x40468b` |

`probe_dispatch.py` 已实现:WOW64 只读调试、清 PEB 反调试、在 `0x4044db` 下 int3、
命中时读 `(script, pc, handler)`。**当前在标题画面 25 s 内 0 命中**,说明要么任务泵
尚未运行(标题画面由另一条路径驱动),要么需要先点击「スタート」/发送输入。下一步:

1. 在标题画面点击「スタート」后重跑探针;
2. 或在任务泵入口 `0x404398` 先下断点确认泵是否运行;
3. 命中后把 handler 地址经 `0x7ace20` 反查命令 ID;共享 handler 用脚本内
   `+0x20 + pc*4` 的命令 ID 字节消歧(待运行时验证);
4. 绝不把"未命中的探针"包装成事件级 trace。

## 5. 与内核 trace 对齐

内核侧:

```bash
cargo run -q -p yu-trace --example emit_frame_trace -- \
  --display target/tmp/first_frame.display.json \
  --out target/test-tmp/official/kernel_event_frame.jsonl \
  --frame 3 --level event --pass-count 15 --op-count 44 \
  --executable-ops 15 --skipped-ops 29 --producer kernel
```

屏幕级对齐(本例):

```bash
cargo run -q -p yu-trace --example diff_traces -- \
  --expected target/test-tmp/official/eris_screen/official_screen.jsonl \
  --actual target/test-tmp/official/kernel_screen.jsonl \
  --mode cross --header codec-sample --subs frame --kinds frame_description \
  --frame-first 3 --frame-last 3 --context 2
```

对齐键是 `(sub, kind, frame, ordinal)`,不是物理行号 `seq`;header 用
`--header codec-sample` 只比较 `codec` 与样本身份。

**不要用官方标题画面去对内核 `first_frame`**:两者不是同一逻辑锚点。该比较已按用户
裁决撤回(`docs/divergences/DIV-H-001`,status=withdrawn);在事件级 L1 打通前,
首屏一致性不作结论。
