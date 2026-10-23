# 症状台账 · P1(崩溃 / 无响应 / 黑屏)

> 任务书:`docs/tasks/task-p1-symptom-host.md`。真值层级见 `AGENTS.md` §0。
> 本文只记录**实测**事实;推断处标“假设”,未取证处标“取证阻塞”。
>
> 状态口径:`fixed`(本任务改代码) · `mitigated`(本任务加了可见面/防线,根因在外层) ·
> `handed-off`(移交编号) · `blocked`(证据不足,不实现) · `open`。
>
> 会话:2026-10-23。语料 `<project-eris>`(data.ypf,官方 0.495 编译器全量编译)。
> 桌面宿主 = winit 0.30 + wgpu 30 + egui 0.37;`serve` = `yu-cli serve --backend wgpu`。
>
> **树状态注记**:本会话期间工作区有 P2/P3/P4 并发改动,主树在部分时段不能编译
> (`yu-gfx` 文本管线、`yu-backend-soft` 文本 pass 未闭合)。宿主层代码先用 `git HEAD`
> 隔离快照验证(§0.4),主树恢复后补跑全部门禁(§5.8、§6)。

## 0. 方法与证据工具

### 0.1 七个剧本的驱动方式

| 剧本 | 桌面宿主 | `serve` | 输入来源 |
|---|---|---|---|
| S1 标题→設定→戻る | `--script cases/host/symptom-1-*.json` | WS `pointer` + `cursor` | 机械坐标(AU4 用例同源) |
| S2 标题→スタート(进本篇) | 同上 | 同上 | 同上 |
| S3 本篇按钮/右键 | `cases/host/symptom-3-ingame-buttons.json` | 同上 | 同上 |
| S4 セーブ/ロード | `cases/host/symptom-4-save-load.json` | 同上 | 同上 |
| S5 关闭两次 | `--script` 的 `close` 动作(与 `CloseRequested` 同处理) | 无(服务端无关闭通道) | 宿主路径注入 |
| S6 resize/最小化/还原/失焦/连点/滚轮 | `--script` 的 `resize/minimize/restore/focus_lost` + `inputs/pulses` | WS `pointer/wheel` | 真 winit 调用;失焦为宿主路径注入 |
| S7 挂机 ≥10 分钟 | `--frames 3600`(60s 快速验证);10 分钟口径见 §5.7 | 10 分 20 秒连续客户端 | 无输入 |

“宿主路径注入”= 直接调用与 winit 事件处理**同一个函数**(`release_all` /
`request_window_close`),不伪造 OS 事件;台账里逐条注明。

### 0.2 工具(均在 `target/tmp`,不入库)

| 工具 | 位置 | 作用 |
|---|---|---|
| `scenario_probe` | `crates/yu-platform/host-desktop/examples/scenario_probe.rs` | headless 逐帧:drawable/ops/skipped、画面 SHA、Diag 码/首末帧/位置、指定帧 PNG + display JSON |
| `serve_probe` | 同目录 `examples/serve_probe.rs` | 连 `serve` 的 WS,按逻辑帧注入指针/滚轮,记录帧号/指纹/最长停顿 |
| `ws_probe.py` | `target/tmp/p1/ws_probe.py` | 同上(Python 备用实现;10 分钟口径用它跑) |
| 宿主日志 | `--log-file <jsonl>` / 默认 `%TEMP%/yu-desktop-host.jsonl` | `yu-log::facade` 桥:panic + wgpu/winit `log::*` + 宿主事件 + 退出诊断摘要 |

### 0.3 构建口径

- 桌面宿主必须 `--release` 才有可玩帧率(AU4 §4):debug 首场景 tick ~6.8s、
  稳态 ~32ms/帧;release 稳态 ~5ms tick + 1.7ms render。
- **宿主行为**(panic 防线 / 最小化还原 / 关闭两次 / 面板)在 debug 下验证:
  与优化级别无关,且 debug 更容易暴露时序问题。
- **引擎语义**结论用 release(`yu-cli run` / `case`)或 10 分钟 `serve`。

### 0.4 隔离快照验证(主树不可编译期间)

`git archive HEAD` → `target/tmp/shadow-head/`,只覆盖本任务的文件
(`crates/yu-platform/host-desktop/**`);对 HEAD 缺失的并发 API
(`yu_log::facade`、`Engine::take_save_diags`、`EngineOptions.save_root`)在快照里
用等价 no-op/stub 顶替。验证结果:

```text
cargo test -p yu-host-desktop -p yu-platform-input   → 24 passed, 0 failed
                                                       (21 lib + 3 input;1 ignored)
```

主树恢复后,同一套代码在主树重跑(§5.8)。

## 1. 台账(每条一行)

| # | 症状 | 稳定复现步骤 | 根因归类 | 证据 | 处置 | 状态 |
|---|---|---|---|---|---|---|
| S1 | 标题 → 設定 → 戻る:正常 | 桌面 `--script symptom-1`,点 (217,368) → (750,565) | 无(冒烟通过) | 340 帧=設定画面 `6bc7a605…`;440 帧=标题 `a6db617b…`(= `cases/title-config-back.json` 的 title 哈希);`fatal=None` | 保留为宿主冒烟 | fixed(无缺陷) |
| S2 | 标题 → スタート:进本篇后**背景全黑、文本框空白** | headless `scenario_probe` 点 (217,281)@300,到 470 | 引擎语义缺失:背景/文本的产出任务在 `es_select.yst:153` 被 Fatal 终止 | 404 帧 `rt.expr.div_zero`(`es.ScenarioScript@153 LET`);同帧 `CGINFO[TEX] SPLBUF1 不存在` ×100;422 帧 `ops=158 / text_runs=0 / skipped_text=0 / missing=[]`,342 帧 PNG 纯黑 | H-P1-07(上游);宿主横幅/面板可见化 | mitigated |
| S3a | 本篇 設定 按钮:正常 | スタート@300 → 設定 (494,583)@440 | 无 | ops 158→450;截图完整设定菜单(6 个页签、开关、底栏);此后 165 帧静态(等输入) | 用例 `symptom-ingame-config` | fixed(无缺陷) |
| S3b | 本篇 タイトル / 終了 按钮:正常(脚本自绘确认框) | (654,583)@440 → 「タイトル画面へ戻りますか?」;(690,583)@440 → 「ゲームを終しますか?」→ はい (352,302)@490 | 无 | 点击后 ops 158→143、画面切换;はい 后 `main_finished=true`,ops→11,末帧 `a7aff0f8` | 无需修 | fixed(无缺陷) |
| S3c | 本篇 **右键**:完全没有反应 | スタート@300 → MOUSE_R (400,520)@440 | 引擎语义缺失/取证阻塞(右键菜单分支未生效;无 Diag) | 430→560 帧 SHA 恒为 `f0c2706d`,`ops=158` 不变;无 Fatal/新增 Diag | H-P1-07 附注(右键路径需 P3 取证);宿主横幅 | mitigated |
| S3d | 本篇 **故事推进点击**:没有反应(文本不前进) | スタート@300 → 点文本框 (400,520)@440 与 @500 | 同 S2:推进文本的任务已被 Fatal 终止 | 未点击时 158 ops;两次点击后仍 `f0c2706d`/158;`text_runs=0` | H-P1-07;宿主“为什么卡”直接显示 Fatal | mitigated |
| S4 | 本篇 **セーブ**:界面不可见,并且**主任务随后终止**(桌面宿主直接关窗) | スタート@300 → セーブ (45,585)@440 | ①合成不可见(P2/P4);②`es_number.yst:SYSTEM@56` 再次整数除 0(Fatal)→ 主任务 END | headless:ops 158→196 但像素恒 `f0c2706d`;桌面:`FATAL rt.expr.div_zero @ es_number.yst:SYSTEM@56 LET engine_frame=442`、`main_finished=true`、host frames=440 即退出(exit 0);`sv.arg.missing`×5 / `sv.write.refused`×3(P4 SaveHost) | H-P1-08(P2+P4);H-P1-07 扩展(es_number:56);用例 `symptom-ingame-save` | handed-off |
| S5 | 标题点“終了する”:画面**变暗后没有对话框**,再点无反应;关窗两次才退出 | 桌面 `--script symptom-5`(close@400, close@430);或标题点 (217,557) | 宿主能力缺失:标题走**原生 `DIALOG`**(VM 已挂起,无呈现端);与 S3b 的脚本自绘确认框不同 | 变暗后像素恒定 121+ 帧;无任何 Diag(静默挂起);close#1=`@_WINDOW_EXEC_CLOSE=1`,close#2=`exit`,exit 0 | H-P1-02(API)+ H-P1-04(egui 模态);宿主保证第二次关闭可退 | mitigated |
| S6 | 最小化 → 还原:**draw 阶段 wgpu 校验错误**;其余 resize/失焦/连点/滚轮不崩 | 桌面 `--script symptom-6`(1024x768→640x480→minimize→restore) | **宿主 bug**(surface 配置 1x1 与 egui scissor 不一致) | `PANIC 捕获 phase=draw frame=430 … Scissor Rect {x:0,y:0,w:960,h:50} is not contained in the render target (1,1,1)`;修复后同剧本 720 帧 exit 0、无 panic | **本任务修复**(§3.6) | fixed |
| S7 | 挂机数秒后:标题进入演示 → **黑屏并永久停住** | 桌面无输入 `--frames 500` | 引擎语义缺失(演示任务命中同一 Fatal);依赖**实时时钟路径** | 末帧 PNG 纯黑;Fatal@269;无输入 260 帧无 Fatal(未到触发点);4 次运行分别 223/281/无/无(**非确定**;3600 帧那次演示已启动但未命中 Fatal) | H-P1-07(含实时/固定步取证);宿主面板/横幅 | mitigated |
| P1-A | 引擎 panic(任意原因) | `--panic-selftest 30 --frames 60` | 宿主缺陷(原:无防线,进程直接死) | 日志含 panic message/location/backtrace 摘要;`exit frozen: phase=tick …`;进程 **exit 0**;冻结期继续出帧(2.5–3.3ms/draw) | **本任务修复**(§3.1) | fixed |
| P1-B | `serve` 连接后首帧 ~1.9s 无输出 | `serve` 建立 WS 后首帧 | 宿主调度(一次性场景装载;`serve` 未设步长上限) | 10 分钟会话 `max_gap_ms=1858`(首帧);之后 36,528 帧 0 回退 | H-P1-05 | handed-off |

## 2. 逐条档案

### S1 标题 → 設定 → 戻る

- 复现(桌面,debug):`--script cases/host/symptom-1-title-settings-back.json`。
- 300 帧点 設定 → 340 帧 PNG `6bc7a605…`(設定画面);390 帧点 戻る →
  440 帧 PNG `a6db617b…`(标题,与 `cases/title-config-back.json` 的期望一致)。
- 日志 `fatal=None`。结论:AU4 输入闭环在宿主路径成立;作为后续剧本的“输入有效”基准。

### S2 标题 → スタート:黑屏 + 文本框空白

- 复现(headless,可逐帧取证):
  `scenario_probe <project-eris> target/tmp/p1/probe/title-start.json`
  (点 `スタート (217,281)` @ 逻辑帧 300,跑到 470)。
- 画面:290 帧标题(147 ops);302–332 帧转场;342 帧**纯黑**;422 帧出现本篇 UI
  (底栏 15 按钮 + 空文本框),背景全黑。
- 关键读数(422 帧):`ops=158 / skipped=0 / text_runs=0 / skipped_text=0 / missing_assets=[]`。
  ⇒ 本帧没有任何 TEXT 产出,**也不是**“文本栅格化被跳过”。
  任务书 §1 假设的“文本链路 = 黑屏主因”在本语料未证实;黑屏是“背景/文本任务
  在产出文本之前就死了”。
- 根因证据(帧 404):
  ```json
  {"code":"rt.expr.div_zero","severity":"Fatal","boundary":"Script",
   "where":{"kind":"script","file":"data\\script\\eris\\es_select.yst",
            "label":"es.ScenarioScript","pc":153,"cmd":"LET"},
   "detail":"整数除以 0:引擎走 x87/CRT 硬件异常路径;内核按 Fatal 处理(DIV-RT-003)"}
  ```
  同帧 `gfx.info.missing` ×100:`CGINFO[TEX] 目标 :/W0/SPLBUF1 不存在`。
  Fatal 之后 UI 任务仍活(见 S3a/S3b),故画面能停在“空文本框 + 可点底栏”。
- 假设(未取证,不实现):`CGINFO[TEX]` 对不存在的 `SPLBUF*` 返回缺失值,
  脚本把它当尺寸/数量做 `LET` 除法 ⇒ 0 除数。需要 P3 把 `LET` 的左右操作数
  打进 Diag 才能定论。
- 归类:引擎语义缺失/取证阻塞(官方 x87 整数除 0 行为未采集,DIV-RT-003)。
  **不在宿主层绕过。**

### S3 本篇按钮(矩阵)

进本篇后(スタート@300)逐项点击,headless wgpu,同一光标路径:

| 目标 | 坐标@帧 | 结果 | 读数 |
|---|---|---|---|
| 設定 | (494,583)@440 | **正常**:完整设定菜单 | ops 158→450;165 帧静态等输入 |
| タイトル | (654,583)@440 | **正常**:确认框「タイトル画面へ戻りますか?」 | ops 158→143,画面 `8471188e` |
| 終了/メニュー | (690,583)@440 | **正常**:确认框「ゲームを終しますか?」→ はい (352,302)@490 | `main_finished=true`,ops→11,末帧 `a7aff0f8` |
| セーブ | (45,585)@440 | **异常**:ops 增加、画面不变 | 详见 S4 |
| 右键 | (400,520)@440 | **无反应** | 430→560 SHA 恒 `f0c2706d`,158 ops |
| 故事推进(点文本框) | (400,520)@440 与 @500 | **无反应** | SHA 恒 `f0c2706d`,158 ops |

- 结论:本篇底栏按钮的**脚本自绘 UI 路径可用**(設定/タイトル/終了);
  “点了没反应”集中在:右键菜单、故事推进(任务已被 Fatal 终止)、セーブ(合成不可见)。
- 归类:S3c/S3d = S2 上游 + 右键路径取证阻塞;S4 见下。

### S4 セーブ:界面不可见 + 主任务终止

两个缺陷叠加,分两步取证:

1. **合成不可见**(headless `scenario_probe`,スタート@300 → セーブ (45,585)@440):
   - ops 158→196,新增 op:
     ```text
     + image :/W0/CGS100  rectpaint2 ×1
     + image :/W0/SLTEMP  size2      ×1
     + image :/W0/SLTEMP  rectpaint2 ×1
     + copy  {group :/W0}            ×3
     ~ copy  {layer :/W0/BT.OFF.TMP} 68 → 76
     ~ layer (无 target)             22 → 46
     ```
   - 像素 SHA 在 430/450/480/540/620 全部等于 `f0c2706d`(点击前)。
     ⇒ `SLTEMP`/`CGS100` 的 rectpaint2 之后没有可见结果(缺合成/COPY,或
     MODE 2 的可见性未实现)。
2. **主任务终止**(桌面宿主,同一输入):
   ```text
   FATAL rt.expr.div_zero at data\script\eris\es_number.yst:SYSTEM@56 LET
         engine_frame=442
   exit diag: … main_finished=true …
   yu-desktop: frames=440 elapsed=15.12s  (exit 0)
   ```
   `es_number.yst:56` 第二次整数除 0 ⇒ **主任务 END** ⇒ 宿主按 `END` 语义退出
   (窗口关闭)。同时 P4 的 SaveHost 报 `sv.arg.missing`×5、`sv.write.refused`×3
   (存档域已接线,脚本参数/落盘门禁拒绝)。
- 归类:①gfx 合成(P2)+ 存档域(P4);②整数除 0(P3/P4,H-P1-07 扩展)。
  P1 不绕过,只锁行为:`cases/symptom-ingame-save.json`(headless 只钉像素/ops)。

### S5 标题“終了する” / 关闭两次

- 复现(桌面):`--script cases/host/symptom-5-close-twice.json`。
- 原始日志:
  ```text
  yu-desktop: script frame=400 close #1 → @_WINDOW_EXEC_CLOSE=1
  yu-desktop: script frame=430 close #2 → exit
  yu-desktop: frames=429 elapsed=40.13s avg_fps=10.7 backend=wgpu viewport=800x600
  ```
- 首次关闭后标题变暗(模态遮罩已画),**没有对话框**;像素恒定
  (exit 摘要 `pixel_stall=294`),无任何 Diag ⇒ 静默挂起。
- 对比证据:S3b 的 `タイトル`/`終了` 是**脚本自绘确认框**,点击正常;
  标题的“終了する”走**原生 `DIALOG`**(`yu-rt::vm::pending_dialogs`),
  `Engine` 未透出 ⇒ 宿主无法呈现/回答。
- 归类:宿主能力缺失 → H-P1-02(API)+ H-P1-04(egui 模态)。
  宿主兜底已验证:第二次关闭强制退出(exit 0)。

### S6 resize / 最小化还原 / 失焦 / 连点 / 滚轮

- 复现(桌面):`--script cases/host/symptom-6-stress.json`
  (5 次连点 @300–308;20 次滚轮 @320–339;resize 1024x768@360;640x480@380;
  minimize@400;restore@430;focus_lost@450;resize 800x600@470)。
- **修复前**:restore 当帧 draw panic:
  ```text
  wgpu error: Validation Error
  In a set_scissor_rect command
    Scissor Rect { x: 0, y: 0, w: 960, h: 50 } is not contained in the
    render target (1, 1, 1)
  ```
  根因:最小化时 `Resized(0,0)` 把 surface 配成 1x1;脚本同一帧 restore,
  `Resized(真实尺寸)` 尚未进队列,egui 已按窗口尺寸算 scissor ⇒ 尺寸错配。
- **修复**(`crates/yu-platform/host-desktop/src/lib.rs`):
  1. `draw()` 在窗口最小化时直接返回(不向 1x1 surface 出帧);
  2. `draw()` 构建 egui 输入前调用 `presenter.resize(window_w, window_h)`;
  3. `Resized(0,0)` 不再夹成 1x1(`Presenter::resize` 本就忽略 0)。
- **修复后**:同剧本 720 帧,`exit 0`,无 panic;六个动作全部按帧执行,
  恢复后继续出帧到 720。
- 失焦 = `WindowEvent::Focused(false)` 的 `release_all()`;本次用脚本动作
  注入同一函数(不是伪造 OS 事件),日志标注 `focus_lost`。

### S7 挂机:黑屏 + 永久停住

- 复现(桌面,debug,无输入):`--frames 500`,末帧 PNG = **纯黑**。
- 日志:
  ```text
  FATAL rt.expr.div_zero at data\script\eris\es_select.yst:
        es.ScenarioScript@153 LET engine_frame=269
  ```
- 与 S2 同一位置、不同触发:S2 是点 `スタート` 后 ~1.7s;S7 是**无输入挂机
  ~8.5s** 后由标题的演示/超时路径触发。
- 非确定性:同一脚本 3 次运行分别 host frame 223 / 281 / 无;
  无输入 260 帧(早于触发点)没有 Fatal。
- **实时 vs 固定步**:headless `tick()`(now_ms = frame×16.7ms)600 帧(9.6s
  逻辑)不复现;桌面 `tick_realtime`(墙钟)复现 ⇒ 触发挂在实时时钟路径上。
  **取证阻塞**(需 P4 对照两者的时钟语义)。
- 桌面 release 长挂机复核(`--frames 3600`,games 时间 60s,墙钟 93.2s):
  `main_finished=false`,`fatal=None`,末帧回到标题(`a6db617b…`);
  日志显示演示在 `last_frame≈1295–1360` 启动(`es_text` IF / `es_sprite`
  CGINFO 缺失)但**没有**命中 div_zero。⇒ 触发条件比“时间到”更窄,
  可能与演示内某条分支/光标位置有关(残余未知,§6)。
- 10 分钟连续 `serve` 口径见 §5.7:进程/音频/出帧稳定;该次运行标题保持静态,
  未触发演示(残余未知,§6)。

## 3. 宿主稳定层(本次实现)

### 3.1 panic 防线

- `panic_guard::install()`:`run()` 最前安装进程级 hook;取 message/location/
  thread + `Backtrace::force_capture()` 前 16 行,经
  `log::error!(target:"yu-desktop")` 进 §3.3 的同一份 JSONL;不调用默认 hook
  (避免绕过 `yu_cli::output` 纪律 + 重复输出)。
- `panic_guard::catch(phase, host_frame, engine_frame, f)` 包住 **tick 与 draw**
  两条热路径;`Phase::{Tick,Draw,SelfTest}` 三阶段标注。
- panic 后 `Freeze { record, at, drawn_frames, draw_panics }`:
  - 引擎**停止推进**(保留最后一帧);
  - 红色横幅(egui `Panel::top`;单测断言 `fills` 含 `PANIC_RED`),
    显示 message/位置/frame + `Quit (Esc)`;
  - 冻结期间继续出帧(实测 2.5–3.3ms/draw);draw 再崩最多 3 次后停止重试;
  - `Esc`/Quit 退出;`--frames` 到限也正常退出(实测 exit 0)。
- 音频回调复核:`yu-cli/src/engine/sound_device.rs` 回调用
  `current().ok()` + `match state.lock() { Err(_) => 静音 }`,本身不 panic;
  残余风险 = `SoundState::render_and_advance` panic 穿 cpal FFI(abort),
  见 H-P1-01 附注。

### 3.2 egui 诊断面板(F1)

1. **为什么卡**:`StuckReport::hint()` 按 `Fatal > 逻辑帧停滞 > 无可绘制 >
   主任务 END > 画面不变 > 正常` 给一句话;并显式列出
   `waiting task = n/a · H-P1-02`、`DomainCall pc = n/a · P3`(缺 API 就缺席)。
2. **挂号表**:按最近帧排序的前 50 条 `code/severity/count/last_frame`,
   外加“最近一条”的 `where` + detail。
3. **本帧丢弃**:`skipped ops top-N`、缺失资产、未栅格化文本计数
   (标注 DIV-GFX-012/033/034/035 → P2)、本帧/累计 Unimplemented top-N。
4. **输入**:光标、`_KEYLAST/_MOUSELAST` 的宿主等价物(按钮名+帧号)、
   按下沿总数、单帧最大连点、clicks/wheel、held、`MOUSE[MODE/TIME]`、
   光标可见性;标注内核值需 H-P1-02。
5. **红色横幅**(panic)/**琥珀横幅**(卡住):琥珀只在高置信条件出现
   (`Fatal`、逻辑帧停滞 ≥60、无可绘制 + 停滞、或**刚点过**且画面 ≥120 帧不变),
   避免把“静态标题等输入”误报为卡死。

### 3.3 log 门面初始化

- `yu-desktop` main 最前调用 `yu_log::facade::init(...)`(P4 M-E 的桥);
  路径:`--log-file` > `YU_LOG_JSONL` > `%TEMP%/yu-desktop-host.jsonl`
  (每次启动截断)。级别 `YU_LOG`(缺省 `warn`;复现用 `info`)。
- panic、wgpu/winit 的 `log::error!`、宿主脚本动作、退出摘要进**同一份 JSONL**;
  终端仍走 `yu_cli::output`(facade 的 console 回调)。
- 退出摘要(`App::finish`):挂号表 top-12 + “为什么卡”,例如:
  ```text
  exit diag: total=598 engine_frame=32 stuck="推进正常" fatal=None
  diag snd.query.unsupported severity=Unimplemented count=540 last_frame=32 …
  exit frozen: phase=tick msg=yu-desktop panic selftest … engine_frame=30
  ```

### 3.4 Web 呈现对等

`www/` 只消费**现有** WS 帧字段(`drawable/skipped/frame`)+ 现有 `/api/report`:
- overlay 新增 `diag` 行:`无可绘制输出` / `N ops skipped` /
  `画面 N 帧未变化(可能等待输入)` / `ok`;
- 引擎 Diag 码表、每帧 Unimplemented、宿主 panic 横幅需要推送通道 ⇒
  H-P1-03(`--diag-ws`,P4)。浏览器侧不复制诊断语义。

### 3.5 用例与宿主脚本

- 门禁用例(`case --all`,8/8 PASS):
  `cases/symptom-enter-story.json`(S2)、`cases/symptom-ingame-config.json`(S3a)、
  `cases/symptom-ingame-save.json`(S4)。
- 宿主脚本(`cases/host/`,不进 `case --all`):S1–S7 七条 + README。
- 单测:`panic_guard`(捕获/冻结/关闭策略/backtrace 上限)、
  `diagnostics`(挂号聚合/卡住判定/指纹/输入计数)、
  `script`(帧对齐/连点展开/未知动作拒绝)、`ui`(红色横幅填充色、
  文案含 Fatal 与 `n/a · H-P1-02`)、`tests/input_host.rs`
  (真语料极短点击/脉冲,`#[ignore]` + release 手跑)。

### 3.6 S6 的最小化/还原修复(宿主 bug)

见 §2/S6;改动集中在 `draw()` 与 `Resized` 处理,断言由
`symptom-6-stress.json` 的完整回放承担(修复后 exit 0、无 panic)。

## 4. 交接件

### H-P1-01 · `engine.rs` 音频锁中毒连锁(归 P4)

- 原状:`sound.lock().unwrap()`(AU4 `engine.rs:488` 及 `sound_snapshot`/
  `sound_layers`)—— 音频线程 panic 后锁中毒,此后每次 tick 连锁 panic。
- **本会话观测到 P4 已并发落地**(`engine.rs` 12:45):全部改为
  `lock().unwrap_or_else(|poisoned| poisoned.into_inner())`,取回中毒前状态继续。
  P1 复核:**行为正确,但缺“一次性 Diag”留痕**(任务书要求“降级静音 + Diag”)。
  建议首次中毒发一条 `snd.lock.poisoned`(Recoverable),后续静默。
- 附注:回调本身已确认不 panic;残余风险 = `SoundState::render_and_advance`
  panic 穿 cpal FFI(abort)。建议 P4 在回调内 `catch_unwind` + 静音降级。
- P1 侧兜底:tick panic 会被 `panic_guard` 接住并冻结,不再“连锁每帧 panic”。

### H-P1-02 · 诊断/呈现所需的 Engine 公开 API(归 P4)

F1 面板当前显式标 `n/a`;需要的透出(只读优先):

```rust
// 1) DIALOG 呈现(P3 的内核状态机已就绪,只差透出)
pub fn pending_dialogs(&self) -> &[yu_rt::dialog::DialogRequest];
pub fn answer_dialog(&mut self, id: u64, button: u8, text: Option<&[u8]>)
    -> Result<(), yu_log::Diag>;

// 2) “谁在等”:等待任务(label/script/pc/等待原因)
pub struct WaitingTaskView { pub id: String, pub label: String,
    pub script: u32, pub pc: u32, pub reason: WaitReason }
pub fn waiting_tasks(&self) -> Vec<WaitingTaskView>;

// 3) 输入计数器的脚本侧值(与面板的宿主等价物对账)
pub fn key_last(&self) -> Option<(u32 /*button code*/, u32 /*frame*/)>;
pub fn mouse_last(&self) -> Option<(u32, u32)>;
```

`DomainCall` 的 `script/pc` 已由 P3 落地(本会话观测到 `yu-gfx` 因新增字段
编译失败),P4 透出后 F1 的 `DomainCall pc` 行从 `n/a · P3` 升级为实际值。

### H-P1-03 · `serve --diag-ws` 诊断通道(归 P4)

- 现状:`www` 只有 `frame/ops/skipped/executed/ms` + `/api/report`。
- 需要:每 N 帧(或状态变化时)推一条文本消息
  `{"t":"diag","frame":N,"codes":[{"code","severity","count","last_frame","where"}],
    "skipped":[{"kind","reason","count"}],"unimplemented":[{"code","count"}],
    "fatal":{...}?,"dialogs":[{"id","buttons","text"}]}`;
  `www` 只渲染,不推断语义。
- 附带:宿主 panic 时服务端进程可能整体退出;`www` 目前只显示断线重连。
  若 `serve` 加 `catch_unwind`(与桌面宿主同策略),`--diag-ws` 应能推
  `{"t":"panic",...}`。

### H-P1-04 · DIALOG 呈现(P3 契约已就绪;集成轮)

- VM 侧:`yu_rt::vm::Vm::pending_dialogs/answer_dialog`(已落地)。
- 缺:`Engine` 透出(H-P1-02)+ 宿主 UI(egui 模态:按钮集合
  `DialogButtons`、`INPUT=1` 文本框;写回 `LET/LETSTR` 由核内完成)。
- 首要场景:标题“終了する”/窗口关闭确认(本台账 S5)。
  完成前保持“第二次关闭强制退出”的宿主兜底。
- 注意:S3b 的确认框是**脚本自绘**,不经过 `DIALOG`,不要重复实现。

### H-P1-05 · `serve` 实时步长上限(归 P4)

- 桌面宿主 `App::new` 里 `set_max_step_ms(2 × frame_ms)`;`serve` 保持默认 500ms。
  实测:连接后首帧 1.86s(一次性场景装载),期间无帧下发。
- 建议:`serve` 同样设 2 帧上限;或首帧前先推一帧“loading/黑帧”并标注
  `boot` 状态,避免“没反应”的错觉。该上限会改变长帧后的补帧量,需 P4 定口径。

### H-P1-06 · `case` 支持 `close` 动作(归 P4)

- 任务书要求“关闭两次”进 `case --all`;`yu-cli case` 目前只有 `inputs[]`。
- 建议:用例格式新增 `"actions":[{"frame":N,"action":"close"}]`,runner 调
  `engine.request_window_close()`(headless 合法,只写 `@_WINDOW_EXEC_CLOSE=1`)。
- P1 已提供宿主侧等价脚本(`cases/host/symptom-5-close-twice.json`)与纯函数
  单测(`close_decision`),行为已验证,只差 runner 透出。

### H-P1-07 · `es_select.yst:153` 整数除 0(归 P3/P4)

- 同一位置 `LET` 整数除 0 的两条触发路径:
  1. 点 `スタート` 进本篇后 ~1.7s(headless 固定步,帧 404);
  2. 无输入挂机的标题演示/超时(桌面实时,host frame 223/269/281,**非确定**)。
- 同帧 `CGINFO[TEX] :/W0/SPLBUF1 不存在` ×100。
- **第三个位置**(S4 触发):`es_number.yst:SYSTEM@56 LET`,点击セーブ后
  ~2 帧命中,并使**主任务** END(桌面宿主直接关窗)。
- 假设:`CGINFO[TEX]` 对不存在缓冲的返回值被当尺寸/数量参与除法。
  需要 `LET` 左右操作数进 Diag 才能定论(P3 的 `DomainEvent` 已带 script/pc)。
- 官方语义未取证(x87 整数除 0,DIV-RT-003);禁止宿主“跳过该 LET”或伪造返回值。
- 影响面:背景/文本推进任务终止 ⇒ S2(黑屏/无字)、S3d(推进无反应)、
  S7(挂机黑屏);**UI 底栏任务不受影响**(S3a/S3b 正常)。
- 附加取证请求:右键菜单路径(S3c)在 Fatal 前后都无反应,需要 P3 定位
  `es.BT.MOUSECHECK` 对 `MOUSE_R` 的分支是否被调用。

### H-P1-08 · 存档界面合成不可见(S4;归 P2 + P4)

- 现象:点セーブ后 display 新增 `CGS100`/`SLTEMP` 的 `rectpaint2` 与 8 个按钮
  拷贝,但屏幕像素完全不变;`skipped=0`(所有 op 都“执行”了)。
- 需要 P2 核对:`MODE 2`(`rectpaint2`)在固定管线状态表的可见性/写掩码;
  `SLTEMP` 之后是否有缺失的合成/COPY;offscreen 组到主组的上屏路径。
- P4 侧:`yu-save` 域宿主并发落地中;`SAVE` 的槽位 UI 是否依赖 `SAVEINFO` 写回。
- P1 只锁行为(`cases/symptom-ingame-save.json`),不改 gfx。

## 5. 证据附录(原始输出)

### 5.0 会话环境(2026-10-23)

```bash
export TEMP/TMP=<side-repo>/workbench/tmp
export YURIS_SAMPLE_ERIS_ROOT=<side-repo>/workbench/project-eris
# 勘误后的真实散盘 / SDK 根(见 §7.1)
export YURIS_SAMPLE_ERIS_YSBIN=<project-eris>/save/data/ysbin
export YURIS_SAMPLE_SDK0479=<side-repo>/target/sdk/0.479/yu-ris_0479_014
export YURIS_SAMPLE_SDK0495=<side-repo>/target/sdk/0.495/yu-ris_0495_032
# MSVC 头/库(并发改动引入 flate2 → libz-sys 后需要;见 §7.1)
export INCLUDE="<VS>/VC/Tools/MSVC/14.44.35207/include;<SDK>/Include/10.0.26100.0/{ucrt,um,shared,winrt}"
export LIB="<VS>/VC/Tools/MSVC/14.44.35207/lib/x64;<SDK>/Lib/10.0.26100.0/{ucrt,um}/x64"
```

### 5.1 基线(§0 要求)

```text
$ cargo test --workspace --no-fail-fast     (环境见 §5.0)
passed=559 failed=0                         (exit 0)
```

### 5.2 S2 headless 摘要(`scenario_probe`)

```json
{"name":"title-start","backend":"wgpu","frames":420,"engine_frame":422,
 "main_finished":false,"drawable_frames":419,"first_drawable_frame":3,
 "final_ops":158,"final_skipped":0,"skipped_total":0,
 "final_sha256":"f0c2706d90…","static_run_max_frames":82}
shots: 290 ops147 a5a618d0 | 302 ops140 37bf4648 | 312 ops140 7c4d8c89
       342 ops137 a7aff0f8(纯黑) | 422 ops158 f0c2706d(in-game,text_runs=0)
fatal: rt.expr.div_zero @ es_select.yst:es.ScenarioScript@153 LET (frame 404)
```

### 5.3 S3 矩阵(headless,进本篇后)

```text
設定 (494,583)@440     → ops 450, sha 62c6c65d  ✓(165 帧静态等输入)
タイトル (654,583)@440 → ops 143, sha 8471188e  ✓(确认框)
終了/メニュー (690,583)@440 → ops 143, sha 9345e914 ✓
はい (352,302)@490     → main_finished=true, ops 11, sha a7aff0f8
右键 (400,520)@440     → ops 158, sha f0c2706d(不变) ✗
推进 (400,520)@440/@500 → ops 158, sha f0c2706d(不变) ✗
セーブ (45,585)@440    → ops 196, sha f0c2706d(不变) ✗
```

### 5.4 S1 桌面(设定/戻る)

```text
s1-340.png sha256 6bc7a605fcaa3e96fc47a8bfcd74918ba90a3877b3a291211a877d720ed74c0f
s1-440.png sha256 a6db617b896dfb8a2f478238c3ef190fe4b0e5e4e825bb19771c8a940d05fc72  (= title)
fatal: 无
```

### 5.5 S5 关闭两次 + panic 自检

```text
script frame=400 close #1 → @_WINDOW_EXEC_CLOSE=1
script frame=430 close #2 → exit
frames=429 elapsed=40.13s avg_fps=10.7 backend=wgpu viewport=800x600
exit diag: … stuck="画面像素连续 294 宿主帧相同…" fatal=None

# panic 自检(--panic-selftest 30 --frames 60)
PANIC 捕获 phase=tick frame=30 engine_frame=30 msg=yu-desktop panic selftest …
frame 31: frozen after panic (draw=15.4ms); engine stopped, last frame kept, Esc/Quit exits
frame limit 60 reached while frozen; 保留最后一帧并退出(进程未崩溃)   EXIT=0
JSONL 20 行:含 "panic thread=main at … backtrace= 0: … | 1: …" 与
             "exit frozen: phase=tick … engine_frame=30"
```

### 5.6 S6 修复前后

```text
# 修复前(restore 当帧 draw panic)
wgpu error: Validation Error
  In a CommandEncoder, label = 'yu-host-desktop frame'
    In a set_scissor_rect command
      Scissor Rect { x: 0, y: 0, w: 960, h: 50 } is not contained in the
      render target (1, 1, 1)
# 修复后(同一 720 帧剧本)
resize 1024x768@360 / 640x480@380 / minimize@400 / restore@430 /
focus_lost@450 / resize 800x600@470
frames=720 … 无 PANIC,EXIT=0
```

### 5.7 10 分钟 `serve` 连续会话

```text
$ python target/tmp/p1/ws_probe.py --port 4200 --seconds 620 --name serve-idle10m
{"frames":36528,"first_frame":1,"last_frame":36646,
 "frame_regressions":0,"max_gap_ms":1858.0,
 "drawable_frames":36527,"distinct_fingerprints":102,"stall_frames":36297,
 "closed":false,"text_messages":1}
/api/status: uptime_ms=640441 frame=36646 fps=60 clients=1 dropped_frames=116
             tick_ms_avg=4.95 render_ms_avg=1.76 ops=146 skipped=0 paused=false
```

- 0 帧回退、0 断流、进程存活;tick/render 均值稳定;音轨首块非静音
  (`[sound] first non-silent block: peak=0.0044 …`)。
- 桌面宿主同口径(较短):`--frames 3600` = 60s 游戏时间 / 93.2s 墙钟,
  `avg_fps=38.6`,末帧标题,无 panic/无 Fatal(见 S7)。
- `max_gap 1.858s` = 连接后首帧(一次性场景装载,H-P1-05)。
- `stall_frames=36297` = 标题静态等输入(正常);该次未触发 S7 的演示
  (残余未知,§6)。

### 5.7b `serve` 动作回放(S1→S2→S3a,同一会话)

```text
$ python target/tmp/p1/ws_probe.py --port 4201 --seconds 35 --name serve-actions     --actions '[{"at_frame":300,"pointer":0,"x":217,"y":368},
               {"at_frame":390,"pointer":0,"x":750,"y":565},
               {"at_frame":450,"pointer":0,"x":217,"y":281},
               {"at_frame":520,"pointer":0,"x":494,"y":583}]'
{"frames":1735,"first_frame":1,"last_frame":1737,"max_gap_ms":1770.0,
 "drawable_frames":1734,"distinct_fingerprints":152,"stall_frames":1186,
 "actions_sent_at":[300,390,450,520],
 "change_frames":[472..481,550,551]}
/api/report: ops_total=156 ops_drawn=156 skipped_total=0 text_runs=0 missing=[]
```

- 设定(300)→戻る(390)→スタート(450)→本篇設定(520)全部在 `serve` 生效
  (472–481、550–551 有像素变化);故事帧 `text_runs=0`(与 S2 一致)。
- WS 抖动/首帧 1.77s 同 P1-B;35s 内 0 断流。serve 无窗口,故 S5/S6 的
  窗口动作不适用(台账 §0.1 已注明)。

### 5.8 门禁(主树)

```text
$ cargo test --workspace --no-fail-fast
passed=647 failed=0                          (主树,2026-10-23 树恢复后)

$ cargo test -p yu-host-desktop -p yu-platform-input
21 passed (lib) + 3 passed (input) + 0 failed + 1 ignored(tests/input_host.rs)

$ cargo test --release -p yu-host-desktop --test input_host -- --ignored
test sub_frame_click_and_pulse_survive_a_single_host_frame ... ok
1 passed; 0 failed(17.83s,真语料)

$ ./target/release/yu-cli.exe case <project-eris> --all
8 passed; 0 failed

# 主树 release 宿主回放(全部 exit 0,无 PANIC)
panic selftest: frames=120 frozen after panic@40,log 含 backtrace + exit frozen
S1 title-settings-back : frames=520  fatal=None
S2 enter-story         : frames=470  FATAL es_select:153 @381(见 S2)
S3 ingame-buttons      : frames=620  FATAL es_select:153 @381
S4 save-load           : frames=440  FATAL es_number:56 @442,main_finished=true(见 S4)
S5 close-twice         : frames=429  close#1 信号 / close#2 exit
S6 stress              : frames=720  resize/minimize/restore/focus_lost 全执行,无 panic
```

### 5.9 隔离快照验证(主树不可编译期间,§0.4)

```text
$ cd target/tmp/shadow-head &&
  cargo test -p yu-host-desktop -p yu-platform-input
21 passed (lib) + 3 passed (input) + 0 failed;1 ignored
```

## 6. 残余未知 / 待补

1. **S7 的实时/固定步差异**:只有 `tick_realtime` 路径触发演示的
   `es_select` Fatal;需要 P4 对照 `tick()`/`tick_realtime()` 的时钟语义。
2. **`es_select:153` 的除数来源**:假设链
   `CGINFO[TEX] SPLBUF1 缺失 → 0 尺寸 → LET ÷0`,未取证。
3. **右键菜单(S3c)**:Fatal 前后都无反应,需要 P3 定位 `MOUSE_R` 分支。
4. **官方引擎对原生 DIALOG / x87 除 0 的行为**:无 trace,无法定性。
5. **S4 的存档界面合成**:`MODE 2` 的可见性与 offscreen 上屏路径(归 P2);
   `yu-save` 域并发落地中,未做端到端存档→读档实测。
6. **音频回调残余风险**:`SoundState::render_and_advance` panic 穿 FFI = abort,
   本会话未触发,也无证据排除。

## 7. 勘误

1. **`AGENTS.md` §2 的样本路径已失效**(2026-10-23 实测):
   - `YURIS_SAMPLE_ERIS_YSBIN` 写的是 `<project-eris>/data/ysbin`,实际不存在;
     散盘在 `<project-eris>/save/data/ysbin`(143 个文件)。
   - `YURIS_SAMPLE_SDK0495` / `0479` 写的是 `…/システム`,但
     `crates/yu-format/tests/corpus_eris.rs` 会再拼一次 `システム/system/...`
     ⇒ 传 SDK **根目录**(`yu-ris_0495_032`)才通过。
   - 采用 §5.0 的值后,`cargo test --workspace` 从 2 个 target 失败变为
     559 passed / 0 failed。
   - 附:并发改动引入 `flate2 → libz-sys` 后,本机 `cc` 找不到 MSVC/SDK 头
     (`stddef.h`);显式给 `INCLUDE`/`LIB`(§5.0)即可,不是代码问题。
2. **AU2 §1 的“`src/` 无 stdout 宏”不成立**:
   `crates/yu-cli/src/engine/sound_device.rs` 有两处 `eprintln!`
   (首块非静音、设备流错误)。不在 P1 文件范围,记录待 P4 统一。
3. **任务书 §1 的“黑屏 = 文本链路未落地”在本次语料未被证实**:
   S2 的 in-game 帧 `text_runs=0` 且 `skipped_text=0` ⇒ 没有 TEXT 被跳过;
   黑屏发生在背景/文本任务死亡之后。文本链路缺口(DIV-GFX-012/033/034/035)
   仍在,但不是本次“黑屏”的直接原因。
4. **“本篇按钮全灭”不成立**(本次实测修正):設定/タイトル/終了 都正常
   (脚本自绘 UI);只有右键、故事推进、セーブ 有问题。见 §2/S3 矩阵。
5. **`yu-desktop` 的 F1 面板首帧不显示**:egui 0.37 的 `Window` 首遍只测量,
   第二遍才落绘制(单测 `ui::tests::run_ui` 固定这一行为,跑两遍);
   横幅(`Panel`)首遍即绘制。
6. **§5.7 的 38.6fps 是修复前基线,不是当前状态**(2026-10-23 P6):
   根因是 Windows 默认 15.6ms 定时器粒度 + `WaitUntil(now+frame)` 串行;
   抬高到 1ms 并把落后对齐到未来槽位后,600 帧稳态 p50=16.65ms /
   排除场景尖峰 avg=16.665ms(60.0fps)。egui 改为隐藏时惰性跳过;
   `serve` 的 `/diag` 只在有订阅者时构造快照。数据与原始输出见
   `docs/perf-report.md`;P1 台账里的 38.6fps 保留为历史证据。
