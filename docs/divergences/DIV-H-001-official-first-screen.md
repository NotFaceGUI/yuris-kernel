# DIV-H-001 · 官方首屏与内核首帧的屏幕级比较(锚点不成立,已撤回)

- 状态: **withdrawn**(不是引擎分歧,不进入修复队列)
- 优先级: —
- 发现日期 / 发现方式: 2026-10-20 / 官方 0.495 屏幕级采集 vs 内核 `first_frame.png`
- 撤回日期 / 原因: 2026-10-20 / 用户裁决 + 复核:两边的"首帧"不是同一逻辑锚点

## 为什么撤回

官方采集到的是**启动完成后的标题菜单**(`スタート/エピローグ/ロード/…`),内核
`first_frame.png` 是启动链后的第一个 display JSON(`frame = 3`,黑底 + 文本框)。
屏幕级采集没有事件级帧锚定,无法证明二者对应同一 `(script, pc, frame)`。

因此:

- 99.9% 像素差异是**锚点不同**的表现,不能作为"内核渲染错"的证据;
- 不能由此推导 L1/L2/L3/L4 任何一层结论;
- 不用容差/归一化/重新选帧把它包装成"一致"或"不一致"。

## 仍然成立的事实(仅作工具记录)

- 官方屏幕级 producer 可用:0.495 `yu-ris.exe` + ERIS `data.ypf`,PrintWindow
  截取 800×600 客户区,schema v3 `frame/frame_description`(`level=screen`)。
- 内核首帧 display JSON 可用:`target/tmp/first_frame.display.json`(frame 3,
  15 passes / 44 ops / 15 executable / 29 skipped),但它是事件级帧.
- 首分歧报告 `path=/payload/display_sha256` 只是两个不同锚点的屏幕摘要差异,
  不作为对拍结论。

## 后续(暂停,待用户恢复)

1. 先打通官方事件级 L1(0.495 分派探针 `tools/official_trace/probe_dispatch.py`,
   当前 0 命中且 `ReadProcessMemory` error 299);用它确定同一 `(script, pc)` 锚点;
2. 再由后端在同一锚点输出 display JSON/PNG,重新采集官方帧;
3. 在此之前,首屏一致性**不作结论**。

## 回归

无。DIV 撤回,不新增 golden。
