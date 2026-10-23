# 任务书 P5 · yu-trace 排障闭环(帧描述 / 故障快照 / 官方 L1 续)

> 仓库:`<repo>`。先读 `AGENTS.md`、本文件、`docs/tasks/task-d-trace.md`
> 与 `task-h-trace-oracle.md`(历史任务书)、`docs/oracle.md`、
> `crates/yu-trace/src/`(schema v3 现状)、`DIV-L-002`。
>
> 背景:三症状(崩溃/无响应/黑屏)排障需要"现场"。本任务让
> `yu-trace` 从"自证一致"升级为"排障工具":补齐帧描述轨道、
> 故障快照(fault dump)、官方 L1 的任务相位。
>
> 文件占用:
> - 独占 `crates/yu-trace`、`tools/`(采集脚本,若加);
>   `crates/yu-cli/examples/` 可加新示例;
> - engine/server 的挂钩点列交接件(归 P4,一行式);
> - `yu-rt`/`yu-gfx` 不碰;零 git 写操作;PROGRESS 只追加"成果 P5"。

## 0. 基线

```bash
export TEMP="D:/Art/ai/projects/YurisDroid/workbench/tmp"; export TMP="$TEMP"
cargo test -p yu-trace
```

## 1. 现状(schema v3)

- 已实现:vfs/vm 边界录制、严格 JSONL、replay、diff、producer-neutral
  头、volatile 投影;官方 L1 已打通(命令顺序级,args 不可观测保持);
- `sub:"frame"`/`kind:"frame_description"` 与 `sub:"platform"` 仍是
  占位:`record.rs:579/1121` 显式拒绝;
- open:DIV-L-002(script 60 返回后的任务相位/调度)。

## 2. 交付

### M-A · 帧描述轨道(P0)

1. 解除 `frame_description` 拒绝:记录/回放/diff 闭环;内容 = 调用方
   传入的规范化 frame 摘要(层计数/op 计数/skipped/text runs/clear 色
   等稳定字段),**不硬绑 `CompositorFrame` 内部类型**(gfx 在 P2 手里,
   用输入结构解耦——沿用任务书 H 的既定方针);
2. `sub:"platform"` 维持显式拒绝,状态写进文档;
3. diff:`(sub,kind,frame,ordinal)` 对齐已有,补 frame_description 的
   投影规则(哪些字段 volatile)。

### M-B · 故障快照 fault dump(P0,排障核心)

1. `TraceRecorder` 增加 ring buffer 模式:最近 N tick 的
   {命令流摘要, Diag(含 Unimplemented 挂号), 输入事件, frame_description};
2. `dump_on_fault(path)`:进程异常(panic hook / catch_unwind 捕获 /
   Fatal Diag)时由宿主调用,原子写 JSONL + 头(样本 hash/帧号/时间线);
3. 回放侧:`replay` 能吃 fault dump,离线重演到故障帧
   (输入时间线已随 dump 保存);
4. 示例:`yu-cli/examples/` 加 fault-dump 演示(合成 panic 场景);
   桌面宿主接线 = 交接件 H-P5-01(归 P4/P1 集成轮)。

### M-C · 官方 L1 续(P1)

1. DIV-L-002:script 60 返回后的任务相位/调度对齐——官方探针扩展,
   采集 ERIS 600 帧,与内核 tick 相位 diff;
2. 官方 args 级不可观测保持结论,不硬造;
3. 采集脚本放 `tools/`,方法论重写自 `<repo-old>/docs/windows-handoff.md`
   (只读参考,不搬代码);产物只放 `target/tmp`。

## 3. 验证

- `cargo test -p yu-trace` 全绿(新轨道 roundtrip/diff 单测);
- 真语料 600 帧:record → replay → diff 闭环,报告 0 未预期差异;
- fault dump:合成 panic 场景产出 dump,离线 replay 复现到故障帧;
- clippy/fmt clean。

## 4. 交接件

- H-P5-01:桌面宿主/serve 的 panic hook → `dump_on_fault` 接线
  (P1 防线捕获处调用;engine 挂钩归 P4);
- H-P5-02:`case --trace` 开关(case.rs 归 P4)。

## 5. 停止/上报条件

- 官方探针在某分派点拿不到稳定观测 ⇒ 降级到已有命令顺序级,
  挂账说明,不造数据;
- ring buffer 内存口径(建议 ≤ 8MB/tick 摘要)超标 ⇒ 上报调参,
  不静默丢帧(丢弃必须有计数与标注)。
