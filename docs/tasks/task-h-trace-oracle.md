# 任务书 H · 官方 trace 对拍(`yu-trace` 从自证到真值)

> 目标: 让 `yu-trace` 能拿**官方引擎的真实可观测行为**做对拍,而不是只证明
> “内核和自己一致”。本会话只改 `crates/yu-trace`(必要时加 `tools/` 采集脚本)。
>
> ⚠️ 官方采集方法必须先验证;旧线的 `engine_trace.py`/笔记只是方法论线索,
> 不能直接当工具/真值。不要修改官方引擎、不要修改旧线工作区。

## 0. 现状与缺口

已有: vfs/vm 边界录制、严格 JSONL、replay、diff、事件容量已关闭丢弃、`hex4` 已修。
缺:

1. 没有官方侧 producer;所有 golden 都是内核自生成。
2. header 含内核版本/prose,跨 producer 无法直接 diff。
3. 没有按 `sub/kind/frame` 的投影/对齐;`seq` 绑物理行号。
4. `tick_at` 的 now_ms 序列未记录;`header.codec` 未参与启动;aux 帧句柄未接线。
5. 输入摘要覆盖了 VM 未消费的逻辑帧。

## 1. 范围(按当前实现修正)

当前 `yu-trace` 的真实状态:

- **已实现**:①vfs 读、②vm 事件、JSONL roundtrip、diff、replay、事件容量已关丢弃、
  `hex4` 已修;schema 版本 = **2**。
- **未实现**:③`sub:"frame"`/`kind:"frame_description"` 与 ④`sub:"platform"`/
  `kind:"platform_call"` 只是占位;`record.rs` 在写入/解析时**显式拒绝**这两类
  (`record.rs:579`/`1121`)。官方侧 producer 也不存在。

因此本会话的范围是:

1. **不要重写 ①②**;只补 ③帧描述的记录/回放/diff。
2. ④平台命令保持显式拒绝,不实现;等平台后端会话。
3. 官方 producer、producer-neutral schema、投影/对齐、回放闭合同样属于本会话。
4. 不改 `yu-rt`/`yu-gfx`;只消费公开 API。若 gfx 仍在改,使用“调用方传入
   规范化 frame JSON/摘要”的输入结构,避免硬绑字段。

## 2. Schema v3(producer-neutral)

- 记录封套保持 `{schema,seq,frame,sub,kind,payload}`;新增 `producer` 元数据:
  `kernel` / `official` / 采集器版本 / 样本 hash。
- header 里的内核版本、人类 prose、表来源等**不得参与跨 producer diff**;
  diff 时必须能按字段投影/跳过,而不是整行比较。
- `seq` 改为逻辑序号(同一 producer 内单调);跨 producer 对齐用 `(sub,kind,frame,ordinal)`。
- 明确哪些字段是 `volatile`(时间戳/路径/版本),默认不参与 diff;不得用容差掩盖语义差异。

## 3. 官方采集(硬骨头)

先读旧线 `<repo-old>/scripts/engine_trace.py`、`docs/windows-handoff.md` 的**方法**,
重写,不搬代码。可选路径:

- 64 位 Python/ctypes 驱动 32 位官方引擎(WOW64),在命令分派/文本输出/变量写点
  做只读观测;
- 或用调试器/注入工具在官方引擎的命令分派表上打点(不改二进制、不写游戏文件);
- 或退一步:官方引擎帧级截图 + 输入时间线,作为屏幕级 oracle;
  但这只能证明“画面像”,不能替代事件级 L1,必须在报告里明确。

硬性要求:

- 采集必须帧锚定,记录输入时间线、帧号、可观测命令/文本/变量/存档;
- 不允许把采集器逻辑塞进内核;采集器在 `tools/` 或仓库外;
- 采集语料/截图不入库;
- 同一输入时间线下,官方 trace 与内核 trace 能逐子系统对齐。

## 4. 回放闭合

- 在 header 记录每次 `tick_at` 的 `now_ms` 序列(或明确只支持 `tick()`);
- 回放时应用 `header.codec` 并与数据源编码一致性校验;
- aux 源的 `FrameHandle` 与主源一致;
- 输入摘要只覆盖 VM 实际消费的逻辑帧。

## 5. 验收

- schema v3 + 投影/对齐 diff 的单元测试;
- 官方 ERIS 首屏至少一份真实 trace(事件级或屏幕级,注明等级);
- 内核同输入 trace;diff 报告:首分歧位置/字段/期望值/实际值;
- 每个分歧建 DIV;无法解释的不得用容差掩盖;
- `cargo test -p yu-trace` 全绿;`YURIS_REQUIRE_SAMPLES=1` 真样本执行;
- 无 git 写操作;不改旧线;不改 rt/gfx。

## 6. 环境

```bash
export TEMP="D:/Art/ai/projects/YurisDroid/workbench/tmp"; TMP="$TEMP"
export YURIS_TEST_TMP="D:/Art/ai/projects/yuris-kernel/target/test-tmp"
export YURIS_REQUIRE_SAMPLES=1
export YURIS_SAMPLE_ERIS_ROOT="<side-repo>/workbench/project-eris"   # 数据入口:官方挂载规则
export YURIS_SAMPLE_ERIS_YPF="D:/Art/ai/projects/YurisDroid/workbench/project-eris/data.ypf"
export YURIS_SAMPLE_SDK0495="D:/Art/ai/projects/YurisDroid/target/sdk/0.495/yu-ris_0495_032"
```
