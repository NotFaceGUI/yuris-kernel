# 任务书 P3 · yu-rt 命令与系统变量取证收口(含默认处理体 / DIALOG)

> 仓库:`<repo>`。先读 `AGENTS.md`、本文件、
> `crates/yu-rt/docs/coverage-matrix.md`(全表 + §2/§3/§5)、
> `crates/yu-rt/docs/divergences/README.md`(DIV-RT-001..042)、
> `docs/command-provenance.md`、`PROGRESS.md` 成果 M-1/M-2/AU4。
>
> 背景:进本篇后大量脚本分支静默不成立("点了没反应"的内核侧根源)。
> 本任务按**静态频度**逐条取证并实现/收口,全部走三层证据
> (手册 / 真实脚本 / REA),无证据 ⇒ `Unimplemented` 挂号,禁止猜。
>
> 文件占用:
> - 独占 `crates/yu-rt`(src + docs);`docs/divergences/DIV-RT-*`
>   可增改;`docs/domain-event-contracts.md` 可增补;
> - `yu-cli`/`yu-gfx` 不碰(接线列交接件);
> - 零 git 写操作;PROGRESS 只追加"成果 P3";证据产物只放 `target/tmp`。

## 0. 基线(动手前必做)

```bash
export TEMP="D:/Art/ai/projects/YurisDroid/workbench/tmp"; export TMP="$TEMP"
export YURIS_SAMPLE_ERIS_ROOT="D:/Art/ai/projects/YurisDroid/workbench/project-eris"
cargo test -p yu-rt
cargo run -p yu-rt --example coverage_census -- 600 8   # 记录改动前计数
```

## 1. 取证队列(按游玩影响排序,不是静态频度序)

### P0-A · 默认处理体 `0x459e90`(一次取证,解一组挂账)

`F_INT`(337)/`F_FLT`(72)/`F_STR`(64)/`);`/`COMPILEMODE`/`G_*`/`S_*` 等
全部落默认槽。必须先反编译 `0x459e90`,回答:**官方对默认槽命令是
no-op、报错还是别的**?若官方 no-op ⇒ 内核应改为"声明类/no-op"
(与 `Unimplemented` 挂号是**可观测差异**,直接影响脚本流程),
逐条建 DIV-RT 修正;若非 no-op ⇒ 按 REA 语义实现。
注意:`F_*` 在静态语料 473 处,很可能是 YSCom 编译产物形态,
结论必须同时给"静态扫描口径"与"600 帧运行口径"。

### P0-B · `DIALOG`(17 处,手册 `other/dialog.html`,REA `0x43fc74`)

窗口关闭确认/游戏内弹窗依赖它。产出:
- 完整参数契约(参数名/类型/默认值/按钮组合/超时)进
  `domain-event-contracts.md`;
- VM 侧语义:任务阻塞形态(对照 TASK 让出模型)、返回值 LET 写回、
  与 `_WINDOW_EXEC_CLOSE` 的交互(AU4 已有宿主兜底,须对齐);
- 实现内核侧状态机,宿主呈现面列交接件(归 P4/P1)。

### P1 · 其余手册有页未实现

`TRAY`(2,`0x453a4c`)、`CLIPACT`(1,`0x43e2d0`)、`CLIPINFO`
(`0x43e410`)、`LABEL`(`0x4634d8`)、`MACRO`、`REGINFO`(`0x44bd30`)。
每个:REA 处理体 + 手册页 + 语料用例;实现或显式挂账。

### P2 · 手册无页但运行期出现

`YMV`/`YMVINFO`/`YMVEND`(`0x458ee0`/`0x459b98`/`0x459a64`,疑似動画
变量族)、`MATH3D`(`0x448900`,1 次——注意 J3 台账 EVID-J3-03/04 已有
MATH3D 投影相关结论,先对表避免双写)、`DEBUGLIST`(5)、`ERROR`(1)、
`CSV`/`CSVINFO`/`PLUG`/`PREP` 等零频项(最低优先,可只做 REA 普查)。

### P3 · 系统变量与表达式收口

- `_TEXT_FLAG` **写**路径(DIV-RT-032,REA 写入点复核);
- `_TEXT_NUM`(`0x4276e3`)、`_WINDOW_INIT_POS`(`0x4276f3`);
- `_WINDOW_EXEC_CAPTION`(DIV-RT-007):维持"宿主 setter"结论,产出
  setter 契约提案(实现归宿主,交接件);
- `HANTOZEN`/`ZENTOHAN` 三张转换表(DIV-RT-013):从 0.495 二进制取
  表字节,移植 + 单测;
- `MATH[RND]` 种子注入(DIV-RT-014):BootConfig 注入口 + 确定性默认;
- 表达式细节(DIV-RT-002/003/004/005)只做语料驱动的增量对齐,
  不开大工程。

### P4 · 加法契约(为 P1 排障面板与 trace 供电)

- `VmEvent::DomainCall` 增加**可选** `script`/`pc` 字段(默认 None,
  不破坏现有构造与模式匹配;coverage-matrix §5 曾声明"本轮不改",
  本任务就是那个"下一轮"):实现 + 全仓迁移说明(不改 yu-cli 调用点,
  列交接件);
- `ArgValue` 变量引用身份(DIV-RT-042)只写提案,不实现。

## 2. 硬性纪律

1. 每条命令:先台账行(REA 处理体反汇编关键事实 + 手册页 + 语料
   触发点),后实现;三者缺一 ⇒ 挂账并给出取证路径,不停机;
2. 实现后 `coverage_census 600 8` 复跑:已有计数**不得回退**,
   新实现命令的"未实现"计数应归零并在矩阵更新状态;
3. 新增 fixture 单测逐命令覆盖;`cargo test -p yu-rt` 全绿;
   clippy/fmt clean;
4. 矩阵 `§1/§2/§3` 与 `DIV-RT` README 同步更新(含状态行格式统一,
   参考 todo-inventory §6.1)。

## 3. 停止/上报条件

- `0x459e90` 反汇编结论与静态频度矛盾(如 F_* 实为编译残留)⇒ 上报
  主人裁决口径,不自行改静态扫描器;
- DIALOG 宿主呈现涉及原生 UI 选型 ⇒ 交接件,不在内核猜;
- 任一命令三层证据无法闭合 ⇒ DIV-RT 挂 open,继续下一条,不阻塞整队。
