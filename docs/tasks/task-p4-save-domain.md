# 任务书 P4 · 存档与文件域宿主(yu-save + SAVE/LOAD/FILEINFO/FILEACT 接线)

> 仓库:`<repo>`。先读 `AGENTS.md`、本文件、`crates/yu-format/src/ystd.rs`
> (存档容器解码已就绪)、`docs/domain-event-contracts.md`、
> `crates/yu-rt/docs/coverage-matrix.md`(SAVE/LOAD/FILEINFO/FILEACT 行)、
> `DIV-RT-012/010`、`PROGRESS.md` 成果 N(声音域宿主的接线范式,照抄)。
>
> 背景:本篇存/读档按钮当前完全无响应(域事件无宿主)。官方 save 产物
> 可自证(真值层级 3):`save/data/*.sd` 由官方 exe 生成,
> roundtrip 逐字节一致即对齐。
>
> 文件占用:
> - 独占 `crates/yu-save`(新)、`crates/yu-cli`(engine 接线:
>   `engine.rs`/`engine/`/`server.rs`/`case.rs`);
> - `crates/yu-format` 只加法(ystd 缺字段/写路径);
> - 用例新文件一律 `cases/save-*.json`;
> - `yu-rt`/`yu-gfx`/宿主 crate 不碰;
> - 零 git 写操作;PROGRESS 只追加"成果 P4";证据产物只放 `target/tmp`。

## 0. 基线(动手前必做)

```bash
export TEMP="D:/Art/ai/projects/YurisDroid/workbench/tmp"; export TMP="$TEMP"
export YURIS_SAMPLE_ERIS_ROOT="D:/Art/ai/projects/YurisDroid/workbench/project-eris"
cargo test --workspace
```

> 注意:P1 同时在改 `host-desktop`(只读 engine);本任务改 `yu-cli`。
> engine 侧共享文件只有本任务动,P1 的需求以交接件进来(见 §4)。

## 1. 阶段

### M-A · 官方存档自证(P0,先做)

1. 定位语料侧官方产物:`<project-eris>/save/`(或官方 exe 现场生成一组
   `save/data/*.sd` + 索引;现场生成时不得改工程数据,产物只放
   `target/tmp`);
2. `yu-format` ystd 解码覆盖官方样本;缺字段加法补齐并注明出处;
3. **写路径**:内核写出的 `.sd` 与官方同槽位产物逐字节一致
   (roundtrip 用例;不一致字段逐个取证,不猜)。

### M-B · `yu-save` 域宿主(P0)

1. `SaveHost`(范式照抄 `yu-sound` 的 SoundHost):层状态 =
   槽位表(存在性/时间戳/缩略图依据)/当前 SAVE/LOAD 操作;
2. 消费 `SAVE`/`LOAD`/`SAVEEND` 域事件(参数名经
   `VmEvent::domain_args` 取,`COV-M-04`);`SAVEINFO` 查询写回 LET;
3. 槽位存在性/`FILEINFO` 盘上事实(FILE/FOLDER/SIZE/RELATION 按手册
   `file/*.html` + REA `0x44286c`)——注意 `FILEINFO[EXIST]` 内核已实现
   (J3),不要双写;
4. `FILEACT`(20 处,REA `0x441370`)按手册逐操作实现
   (COPY/DELETE/RENAME/…以手册页为准),缺页操作挂账;
5. 路径根:`$_SAVE_PATH`(宿主注入已有,engine.rs:984)为唯一写盘根;
   禁止写到工程数据目录。

### M-C · Engine 接线(P0)

1. `Engine` 安装 `SaveHost`(可选,默认 None 时保持域事件直通);
2. 存档异步性:官方 SAVE 是否阻塞脚本(REA SAVE 处理体 `0x44c45c` /
   LOAD `0x44708c` 取证)——决定任务是让出还是同步完成;
3. `cases/save-*.json`:标题 → 本篇 → 存档 → 读档 → 继续,全程
   `case` 可回放;`case --all` 全绿。

### M-E · log 门面消费(P0,排障刚需)

> 现状:全工作区没有任何 logger 初始化(无 `log` 依赖、无 `SetLogger`)。
> wgpu/winit 等依赖经 `log` 门面发的 error/warning 全部被静默吞掉,
> 宿主层崩溃排查瞎一半。本项把它接进 yu-log,不引入 env_logger/tracing。

1. `yu-log` 新增 `log::Log` 桥(建议 `facade.rs`,`log` 为普通依赖):
   - `log::Record` → yu-log 结构化 sink(JSONL,`sub:"log"` 或等价标注,
     携带 target/level/文件行号)+ 控制台一行(经 CLI 既有 output 层,
     不破坏"`src/` 无 stdout 宏"纪律);
   - 级别映射 error→Error … trace→Trace;`yu-log` 自身诊断码不受影响;
   - 环境变量控制级别(建议 `YU_LOG`,接受 `error|warn|info|debug|trace`,
     默认 `warn`;解析 `RUST_LOG` 不做,避免隐式全局语义);
2. `yu-cli` 所有子命令(`run/serve/dev/case/bench`)main 入口处初始化,
   早于引擎 boot;wasm 宿主不初始化;
3. 验证:构造一条 wgpu/winit 层 warning(如强制请求不存在的 adapter)
   能在 CLI 输出与 JSONL 同时出现;`--log-level`/`YU_LOG` 过滤生效。

### M-D · P1 交接件落地(P1,收尾时做)

- H-P1-01:音频锁中毒降级(`engine.rs:488` 不 panic);
- H-P1-02:诊断 drain API(等待任务列表/挂号表)供 egui 面板;
- H-P1-03:server `--diag-ws` 通道(www 消费);
- P3 的 `DomainCall.script/pc` 字段落地后在 server/case 侧透传。
- 每项都以 P1 台账编号回写状态,不实现语义之外的东西。

## 2. 验证

- 官方 `.sd` roundtrip 逐字节一致(真值层级 3,写进 PROGRESS 原始输出);
- ERIS 实机:存档 → 重启进程 → 读档 → 画面/变量恢复(可人工 + case 双轨);
- `cargo test -p yu-save -p yu-format -p yu-cli` 全绿;clippy/fmt clean;
- census 计数不回退。

## 3. 交接件(出给他人)

- H-P4-01:DIALOG engine 语义(P3 契约到位后):任务阻塞 + LET 写回
  归本任务,P1 呈现(egui/原生框)归集成轮——若 P3 未完成则本项挂起;
- H-P4-02:存档缩略图(CGETSCREEN 类)如涉及 gfx ⇒ 交接 P2/集成轮。

## 4. 停止/上报条件

- 官方 `.sd` 有无法解释的字段/校验和 ⇒ 挂账 + REA 取证路径,
  禁止"大概是对称写回";
- 存档写盘路径涉及系统权限/多用户目录 ⇒ 上报主人确认根路径策略;
- SAVE/LOAD 阻塞性证据不足 ⇒ 显式选择同步完成并挂 Unverified,
  不得在未知处让出。
