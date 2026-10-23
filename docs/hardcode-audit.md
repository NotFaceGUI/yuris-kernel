# 硬编码常量审计(2026-10-23)

> 范围:本会话直接修改/审阅的热路径 crate(`yu-platform/host-desktop`、
> `yu-cli`、`yu-rt` VM、`yu-gfx` 文本、两个渲染后端)。
> 规则:**能给出处就补注释;给不出处不改数值**,改为命名常量并挂账。
> 真值层级见 `AGENTS.md` §0;值本身一律不改,除非有证据。
>
> 说明:这是**定向审计**,不是全仓字面量普查。`src/` 里的 0/1/2 等
> 语义显然值、数组下标、位宽不列入。下一轮可按本表口径扩展到全仓。

## 1. 有出处(已补注释)

| 位置 | 值 | 证据 | 处置 |
|---|---|---|---|
| `yu-gfx/src/text.rs` `DEFAULT_FONT_SIZE` | 20 | 引擎默认样式 `FUN_00464fac`;`docs/font-contract.md` §3.3“SX=SY=20” | 命名常量 + 注释 |
| `yu-render/soft/src/text.rs` `font_style` | 0→20 | 同上(渲染侧第二次投影) | 注释 + 回退保留 |
| `yu-render/soft/src/text.rs` `color2` | `unwrap_or(color)` | 引擎字段默认 `0xffffff`(COLOR2 槽 `+0x2c`,`docs/font-provenance.md`,REA `0x5d1708`);“脚本未给”分支无单独手册例 | 注释 + 挂账(见 §2) |
| `yu-gfx/src/text.rs` `clamp` | `0x100` / `8` / `1` / `0x00FF_FFFF` | FONT 命令 clamp,REA `0x444428..0x444b1c` | 已有注释 |
| `yu-gfx/src/state.rs` SWAP | 模式 1/2 | `FUN_0042f57d`;精确对应仍 `Likely`(DIV-GFX-032) | 原有 TODO 保留 |
| `yu-rt/src/value.rs` `MAX_ELEMS` | `1<<26` | **内核自保**,不是引擎语义(真实声明最大 `.TX.MAX=4096`) | 已有注释 |
| `yu-cli/src/engine.rs` `DEFAULT_MAX_STEP_MS` | 500 | `serve` 的既有口径;桌面宿主按 2 帧收紧(H-P1-05) | 命名常量 + 注释 |
| `yu-cli/src/server.rs` `DIAG_PUSH_MS` | 500 | H-P1-03 `/diag` 推送节拍 | 命名常量 |
| `yu-platform/host-desktop` `TIMER_PERIOD_MS` | 1 | 实测默认 15.6ms → ~41fps;Win32 `timeBeginPeriod` 文档语义 | 命名常量 + 实测记录(`docs/perf-report.md`) |
| `yu-platform/host-desktop` `WAIT_LEAD_US` | 800 | 实测 `WaitUntil` 晚 ~0.3ms;提前量使 p50 落在 16.66ms | 命名常量 + 实测记录 |
| `yu-rt/src/vm.rs` `GLOBAL_RESTORE_KEYS` | `1<<12` | 掩码编码:3 族 × 4 位(`G_*1..4`) | 命名常量 + 推导注释 |

## 2. 无出处(命名常量 / 挂账,不改行为)

| 位置 | 值 | 性质 | 处置 |
|---|---|---|---|
| `yu-platform/host-desktop` `STUCK_PIXEL_FRAMES` | 120 | 诊断启发式(“2 秒不变”),非引擎语义 | 已有命名常量;挂账 H-HOST-01 |
| `yu-platform/host-desktop` `STUCK_LOGIC_FRAMES` | 60 | 同上 | 已有命名常量;挂账 H-HOST-01 |
| `yu-platform/host-desktop` `attention_hint` 点击窗口 | 300 宿主帧 | 横幅抑制启发式 | 挂账 H-HOST-01;不改 |
| `yu-platform/host-desktop` `FRAME_STATS_CAP` | 200_000 | 证据工具内存上限 | 命名常量;工程值 |
| `yu-platform/host-desktop` `PENDING_INPUT_CAP` | 4096 | 异常保护 | 命名常量;工程值 |
| `yu-platform/host-desktop` `INPUT_LATENCY_CAP` | 8192 | 证据工具内存上限 | 命名常量;工程值 |
| `yu-cli/src/engine.rs` `SOUND_DIAG_CAP` | 8192 | 诊断暂存上限 | 命名常量;工程值 |
| `yu-cli/src/server.rs` `IDLE_SLEEP_MS` | 15 | 无客户端空转节拍 | 命名常量;工程值 |
| `yu-cli/src/server.rs` 落后重置 | `budget * 4` | 长帧后不补帧 | 挂账 H-SRV-01;不改 |
| `yu-rt/src/vm.rs` `DECODE_CACHE_CAP` | 200_000 | 解码缓存护栏 | 命名常量;工程值 |
| `yu-rt/src/vm.rs` `pump_cap` | `None` | 与引擎同构的“无限泵”;任务书记录引擎无每帧上限 | 保留;`yu-desktop --pump-cap` 为实验性(默认关,见 DIV-HOST-002;实测会触发 rt.label Fatal) |
| `yu-rt/src/vm.rs` `last_global_restore` | 4096 槽 | 由掩码位数推导 | 已命名(`GLOBAL_RESTORE_KEYS`) |
| `yu-render/soft` `CacheLimits::default()` | 库默认 | 字符缓存尺寸无官方数据 | 挂账 DIV-GFX-056 相关;不改 |
| `yu-font` `FontConfig` 默认 | SX/SY=20、color2 白 | `FUN_00464fac` | 已注释 |
| `yu-gfx/src/text.rs` `line_height` | `SY=0 → 20` | 行距未单独取证(DIV-GFX-056) | 保留;文档已标 `Unverified` |

## 3. 明确不动的“魔法数”

- VM 命令窗口 `slot`/`kind`/操作码:来自工程 `ysc.ybn`,不是硬编码常量。
- `@48 = @_LC`、`@1118`、脚本行号:数据事实/取证坐标,不是可调参数。
- `0x00FF_FFFF` 颜色掩码、`0x100` 字号上限:有 REA/手册锚点(见 §1)。
- 音频 `256`(PAN/PITCH 中点)、`0x105/0x107...`:来自声音对象布局取证
  (`yu-sound` 注释),不在本轮范围。

## 4. 挂账(交接)

- `H-HOST-01`:卡住横幅的三个启发式阈值(120/60/300)无官方语义,
  只是“给人看”的诊断;若将来做官方诊断面板需重新定口径。
- `H-SRV-01`:`serve` 落后 `budget*4` 丢弃补帧的策略未与官方对齐
  (官方无 `serve` 形态);保持现状,改动需主人裁决。

## 5. 复核方式

```bash
# 本表 §1/§2 的常量都可在源码里直接 grep 到名字:
rg -n "DEFAULT_FONT_SIZE|DEFAULT_MAX_STEP_MS|DIAG_PUSH_MS|TIMER_PERIOD_MS|WAIT_LEAD_US|GLOBAL_RESTORE_KEYS|DECODE_CACHE_CAP" crates/
# 行为回归
cargo test -p yu-rt -p yu-host-desktop -p yu-cli -p yu-trace
yu-cli case <project-eris> --all
```
