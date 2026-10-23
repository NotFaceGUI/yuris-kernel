# DIV-RT-050 · 场景截图栈(CGS)未清:弹窗/存档背景透出上一屏

- 状态: open
- 优先级: 中(可见性错误,非崩溃)
- 发现日期 / 方式: 2026-10-23 / 主人桌面实测 + headless 回放取证(P8 期间)

## 复现

- 游戏位置: ERIS 演示流程 → 选项菜单 → 点底栏最右「メニュー」(或打开存档界面)
- 操作序列(headless,与 `cases/symptom-ingame-save` 同一入口):
  `click 300,217,281`(标题 スタート) → `click 470,750,583`(メニュー;headless 帧号按 tick)
  或 `click 440,45,585`(セーブ)
- 锚点: 打开菜单后任意稳定帧(实测逻辑帧 530 / 存档帧 562)

## 官方行为

- 官方桌面截图(主人提供,2026-10-23):「メニュー」弹窗背景为 `cgsys/menu/back`(800×600,
  `alpha=102`)叠在**黑底**上;弹窗后面的上一屏内容不可见。
- 存档界面: 板外背景为纯净深色,无上一屏文字。
- 机制(脚本侧,官方产物): `es_ERIS.yst:662 #=es.CGSSS` 每次场景切换
  `@es.CGSNO+=1` → `CG[ID=CGSn FILE=""]` → `CGACT[COPY2 ID=:/W0/ ID2=CGSn]`
  → `CG[ID=CGSn A=256]`(截上一呈现帧入栈);`es_ERIS.yst:942 #=es.CGSDELALL`
  按 `@es.CGSNO` 递减逐层 `CGEND`。
- 待补: 官方 0.495 该路径的**事件级 trace**(`tools/official_trace/probe_dispatch.py`),
  确认进菜单/开存档时官方对 `CGS1..n` 发出的 `CGEND/CG/alpha` 序列与帧号。

## 内核行为(headless `input_probe` 复现)

- 打开「メニュー」(frame 530)时的 screen pass(14 层,自底向上):
  1. `:/W0/CGS1` **snapshot 快照,A=256,visible=true**(内容是上一屏的选项菜单,含文字);
  2. `BT.OFFES.MENU.TIP.BG=0=1`(asset `cgsys/menu/back`,800×600,alpha=102);
  3. `BT.OFFES.MENU.TIP.DIALOG=0=1` + 12 个弹窗按钮。
- 因此 40% 暗纱下透出上一屏(文字清晰可见),与官方「黑底」不同。
- 存档界面(frame 562)同理:`:/W0/CGS1` 为最底层(z=95e9,全屏),其内容 = 点セーブ后
  第 1 帧执行 `CGACT[COPY2 ID=:/W0/ ID2=CGS1]` 时“上一呈现帧”的选项菜单(带文字)。
- 同帧层状态:`CGS1` 之外不存在 `CGS100`,也没有 `SYSBLACK`;`@es.CGSNO`/`es.CGSDELALL`
  的清栈结果在显示列表里看不到(即该清栈**未发生**或清的不是这一层)。

## 二分记录

- [ ] L1 译码一致?(未做:需要官方事件级 trace)
- [ ] L2 执行/事件流一致?(未做)
- [ ] L3 帧层状态一致?**已证不一致**:内核保活 `CGS1` 且 A=256;官方该帧背景无上一屏内容
- [ ] L4 帧间编排一致?(未做)
- 定位结论: 差异在**场景 CGS 截图栈的生命周期**(`es.CGSSS` 入栈 / `es.CGSDELALL` 清栈 /
  截图时刻),不在弹窗按钮,也不在 gfx 文本管线(P8 已单独修复文字)。

## 根因(候选,待 trace 定论)

1. 进菜单/存档的路径上 `es.CGSDELALL`(或等价的 `CGSn` 隐藏/alpha 归零)**未执行**
   ——按钮事件链中某条未实现命令(该路径已观测到 `FILEACT`/`SYSTEM`(es_gsd)/
   `WINDOWINFO[FULLSCREEN]`(es_Button IDBtnTask)/`FONTINFO[NUM]` 均 Unimplemented,
   LET 不写回会使分支走偏);
2. 或截图时机不同:官方在该处截到的是黑帧,内核截到的是上一亮帧。
   官方按“上一已呈现帧”取像素(EVID-J3-06),故需要 trace 判定官方在截图前是否已有一帧
   隐藏后的呈现。

## 修法(待定,不猜)

- 若确认清栈缺失:在场景切换/结束路径补齐 CGS 栈清理(`CGEND CGSn` 直到 `@es.CGSNO`),
  或对齐 `es.CGSDELALL` 的触发点;属 `yu-rt`/场景层,跨 crate 契约需主人裁决。
- 若确认截图时机:对齐“隐藏 → 呈现一帧 → 截图”的调度边界。

## 回归

- 对拍用例: `cases/symptom-ingame-save`(背景应为净色)、菜单弹窗微场景(待建)
- 验证命令与原始结果:
  - `./target/release/examples/input_probe.exe <project> --frames 560 \
      --click 300,217,281 --click 470,750,583 --shot 530=target/tmp/p8/menu-popup2.png`
    → 弹窗叠加在选项菜单上(CGS1 透出);
  - 相关证据: `target/tmp/p8/menu-popup2.png`、`target/tmp/p8/save-f562-full.json`
    (`screen` pass 中 `:/W0/CGS1` 位于最底、A=256)
