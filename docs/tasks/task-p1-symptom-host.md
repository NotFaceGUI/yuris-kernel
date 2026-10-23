# 任务书 P1 · 游玩症状工单与宿主稳定层(崩溃 / 无响应 / 黑屏)

> 仓库:`<repo>`。先读 `AGENTS.md`、本文件、`README.md`、"当前状态"节、
> `PROGRESS.md` 成果 AU1–AU4(尤其 AU4 输入收口)、`docs/architecture.md`。
>
> 背景:主人要进游戏本篇。当前实机(桌面宿主 + `serve`)仍会出现
> **点击按钮崩溃、点了没反应、黑屏** 三类症状。本任务不猜语义,
> 负责**复现、归因、宿主层止血**,把不属于本层的修复以交接件移交。
>
> 文件占用:
> - 独占 `crates/yu-platform/host-desktop`、`crates/yu-platform/input`、
>   `www/`、`cases/`(新文件一律 `cases/symptom-*.json`)、
>   新建 `docs/symptom-ledger.md`;
> - `crates/yu-cli` **只读**(engine 侧改动列交接件,归属 P4);
>   `yu-gfx`/`yu-rt`/`yu-trace` 不碰;
> - 零 git 写操作;PROGRESS 只追加"成果 P1";证据产物只放 `target/tmp`。

## 0. 基线(动手前必做)

```bash
export TEMP="D:/Art/ai/projects/YurisDroid/workbench/tmp"; export TMP="$TEMP"
export YURIS_SAMPLE_ERIS_ROOT="D:/Art/ai/projects/YurisDroid/workbench/project-eris"
cargo test --workspace        # 工作区含未提交 AU1–AU4 改动,必须先全绿
```

## 1. 现状(症状的已知机理,来自代码与台账)

- **无响应**:`vm.rs:1346-1360` 对 `Unimplemented/Recoverable` 是
  "挂号留痕后继续下一条"。脚本分支(如 `es.BT.MOUSECHECK`、DIALOG 确认、
  SAVE/LOAD、MENU)在宿主能力缺失时**静默不成立**,画面停在原地,
  用户看到的就是"点了没反应"。当前没有任何用户可见的挂号面。
- **崩溃**:宿主/引擎热路径存在 panic 面——`engine.rs:488`
  `sound.lock().unwrap()`(音频线程一旦 panic,锁中毒,后续每帧 tick 连锁
  panic);`yu-gfx/src/state.rs` 多处 `expect("checked")`;wgpu surface
  在窗口 resize/最小化后的丢失路径未验证。桌面宿主无 `catch_unwind`,
  任何一处 panic = 进程直接死,无现场。
- **黑屏**:文本链路未落地(`DIV-GFX-012/033/034/035`,归属 P2——
  对话不可见即"黑屏/无字");另有 skipped ops、无配对转场、
  首帧清屏锚点(`DIV-GFX-010/051`)等次要来源。

## 2. 交付一:症状台账 `docs/symptom-ledger.md`

每条症状一行档案,字段:症状 / 稳定复现步骤 / 根因归类
(宿主 bug|引擎语义缺失|取证阻塞|归属任务)/ 证据(diag 码、日志、
截图哈希)/ 处置(本任务修复 or 交接件编号)/ 状态。

必测剧本(ERIS 真语料,`yu-desktop --release` 与 `serve` 各一遍):

1. 标题 → 設定 → 戻る(AU4 已有用例,作冒烟);
2. 标题 → **はじめてから / つづきから**(进本篇的主要按钮);
3. 本篇内右键菜单、オート/スキップ/既読/環境設定按钮;
4. セーブ/ロード画面打开与点击(预期:P4 前为无响应,须建档);
5. 窗口关闭一次/两次(DIALOG 确认未实现前的 AU4 兜底行为);
6. resize / 最小化还原 / 失焦恢复 / 快速连点 / 滚轮狂转;
7. 长时间挂机(≥10 分钟,观察音频回调与实时时钟补帧 `max_step_ms`)。

## 3. 交付二:宿主稳定层

1. **panic 防线**:`host-desktop` 主循环对 `engine.tick_realtime()` 与
   render 调用包 `catch_unwind`;panic 不再杀进程,而是:
   记录到日志(panic message + backtrace 摘要)、egui 面板显示红色横幅、
   引擎进入"冻结"状态(停止推进,保留最后一帧),可按键退出。
   音频回调线程同样不得 panic(回调里只做非抛出操作)。
2. **egui 诊断面板扩展**(F1 现有面板):
   - 最近 50 条 Diag(挂号表:码、次数、最近帧号);
   - 每帧 skipped ops / Unimplemented 计数 top-N;
   - 输入计数器快照(按下沿计数、`_KEYLAST/_MOUSELAST`);
   - "为什么卡"提示:当帧无 drawable 输出、主线让出原因
     (消费 `FrameReport` 与 engine 已有公开 API;缺的接口列交接件,
     例如"当前等待任务 label""DomainCall 的 script/pc"——后者 P3 在做)。
3. **Web 呈现对等**:`www/` 只消费现有 WS 帧;新增诊断通道列为交接件
   (server.rs 归 P4)。
4. **极短点击/滚轮回归**:AU4 的 `deferred_release`/`pulse` 已有,
   补宿主侧单测;`cases/symptom-*.json` 至少 2 条(进本篇点击 +
   关闭两次),随 `case --all` 通过。
5. **log 门面初始化**:桌面宿主 main 最前调用 P4 落地的 `yu-log` 桥
   初始化(见任务书 P4 M-E);panic 防线捕获的 backtrace 与
   wgpu/winit 的 `log::error!` 一并进日志文件(宿主本地日志路径
   自定,如 `%TEMP%` 或工作目录,写明即可)。桥本身不由本任务实现。

## 4. 交接件(编号 H-P1-*,写进台账,不动别人的文件)

- H-P1-01:`engine.rs` 音频锁中毒连锁(`lock().unwrap()` → 中毒后降级
  静音 + Diag,不 panic)——归 P4;
- H-P1-02:诊断面板需要的新 engine 公开 API(等待任务列表、
  挂号表 drain)——归 P4,P3 的 DomainCall script/pc 字段到位后升级;
- H-P1-03:server `--diag-ws` 通道(给 www)——归 P4;
- H-P1-04:DIALOG 确认呈现(P3 出契约、P4 出语义后的 egui/原生框)——
  集成轮做。

## 5. 门禁与验收

- 上述 7 个剧本每个都有台账行:要么本任务修复,要么有交接件编号;
- 桌面宿主在全部剧本中**进程不死**(panic 被防线接住并可见);
- `cargo test -p yu-host-desktop -p yu-platform-input`、
  `yu-cli case <eris> --all` 全绿;clippy/fmt clean;
- PROGRESS 追加"成果 P1"(含台账摘要与原始输出)。

## 6. 停止/上报条件

- 复现出语义缺口但三层证据不足以定性 ⇒ 只建台账行,不实现;
- 需要 gfx/rt 侧修复 ⇒ 交接件,不在本层绕过;
- 症状无法复现 ⇒ 记录环境与尝试,交主人确认是否语料/版本差异。
