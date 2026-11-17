# 静默状态缺口任务组（2026-11-17）

来源：`docs/tasks/silent-state-audit-2026-11-17.md`。
本组任务按风险从高到低排列，**一次只做一个**；每个任务先取证、再改代码，
最后更新台账与 PROGRESS。

## 顺序

0. **开工前验收**（10 分钟，不单独建任务书）
   用 0.486 样本跑同一剧情过渡，确认 `RESIZE`/`CG[FILE]` 修复后：
   - 画面不再“跳一下”；
   - 立绘按官方过渡淡出，不再瞬间消失；
   - 日志无 `gfx.info.unsupported`。
   未通过则先回退/定位，不进入 01。

1. `task-01-copy-rect.md`（P1）
   `COPY/COPY2` 目标矩形与 `LayerKind::Image.size`/`ONMOUSE` 命中尺寸分离。

2. `task-02-trim-extent.md`（P2）
   `TRIM` 裁后尺寸回写；先定位官方执行体，再决定同步像素/后端反馈方案。

3. `task-03-evidence-gaps.md`（P3）
   目录形 `CG[FILE]` 失败矩形、`TRIM` 全透明、`SIZE` 单轴、资产换图同帧尺寸
   四个小证据点，逐项 fixed / unsupported-by-official / 继续 open。

4. `task-04-layer-epoch-refactor.md`（触发式，默认不启动）
   **仅当同类“命令时刻 vs 回放”bug 再次出现**，或修同类边界需要再加
   provenance 表/`target_live` 特判时，才考虑 LayerEpoch 第一阶段重构。
   当前 `DIV-GFX-090` 的语义修复已覆盖已验证路径，不要提前重构。

## 启动方式

打开**新的 agent 会话**，把 `startup-prompts.md` 里对应任务的一段原样粘贴。
每个任务书都是自包含契约；提示词只负责把 agent 带到任务书。

## 全组共同约束

- 先读 `AGENTS.md`；真值层级：官方可观测行为 > 0.495 REA > 原版数据 >
  旧线 golden。
- 不改 `yu-emote`；不改 `<repo-old>` 工作区；iOS 不在范围内。
- 未经主人明确指令，不执行任何 git 写操作（含本仓库）。
- 临时探针/日志用完删除，不入库；`Unimplemented` 只能是过渡态。
- 每个结论必须带证据锚点（REA 地址 / trace / 官方数据字节）；无证据不猜。
- 每完成一项，更新对应 `docs/divergences/` 台账与 `PROGRESS.md`。
