# 任务书 J3 · 完美跑通 Title(WebGPU 一等公民,与 native 同严格)

> 仓库:`<repo>`。先读 `AGENTS.md`、本文件、`docs/title-motion-ledger.md`、
> `docs/fidelity-ledger-j.md`、`docs/render-parity.md`、`docs/todo-inventory.md`、
> `PROGRESS.md` 成果 J1/J2/M1/M2/L1。
>
> 目标:从进程启动到 title 稳定态、以及 title 期间的全部动画,在
> **native soft / native wgpu / 浏览器 WebGPU** 三条路径上一致通过;
> WebGPU 不是降级路径,验收标准与 native 相同。
>
> 文件占用:
> - 独占 `crates/yu-gfx`、`crates/yu-backend-soft`、`crates/yu-backend-wgpu`;
> - **允许对 `crates/yu-rt` 的唯一改动**:实现 `FILEINFO[EXIST] → LET`
>   (File 域查询,加法式;不改其它 rt 语义);
> - **不改** `crates/yu-cli`、`crates/yu-host-wasm`、`www/`(K2 宿主;
>   发现宿主问题 ⇒ 写交接,不抢文件);
> - 零 git 写操作;PROGRESS 只追加“成果 J3”。

## 0. 现状(2026-10-21,先复核)

- J2 已实现:转场双纹理(soft/wgpu)、COPY/COPY2、REVERSE/HORIZONTAL、
  命令交错序;title 稳定帧(tick 300)soft 与 native wgpu PNG **逐字节相同**
  (`6b03238a…`,1,920,813 B),背景/人物/LOGO 区域对官方 max≤3。
- 仍缺:
  1. **按钮列 10 个 `asset.missing`**:脚本用 `FILEINFO[EXIST=1]` 探测
     `_off/_on/_over/_na` 后缀;`yu-rt` 目前只发 `DomainCall`,没有 LET 写回。
     交接规格见 `docs/title-motion-ledger.md` §6。
  2. **boot/title 锚点**:`cargo test -p yu-gfx` 的 golden 仍 `visible 8 vs 34`
     失败(rt 启动路径 + `debug_mode` 未对齐);J2 未改 golden。
  3. DIV-GFX-045:TA≠128 的覆盖度残差(~0.12,低风险;title 用 TA=128)。
  4. 组 RT alpha/主组直绘(DIV-GFX-008/041)blocked,等契约裁决。
  5. 浏览器宿主尚未按 J3 标准验收(title/按钮/动画/WebGPU 一致性)。

## 1. 验收标准(严格,先声明后测量)

### 1.1 功能

- 启动 → boot 转场(SPL101/SPL401 区间)→ title 稳定态全程无异常黑帧
  (清屏帧除外);title 首次出现 tick 与官方基准的偏差有解释。
- title 稳定帧:背景、人物、LOGO、**按钮列**(全部状态图)齐全;
  `asset.missing(title) = 0`。
- 动画:转场按 `T/TA` 逐帧推进;至少覆盖 title 前 600 tick 内出现的全部
  transition 区间;逐帧与官方微场景采样对照。

### 1.2 一致性

- native soft 与 native wgpu:同帧 PNG 逐字节相同(同适配器/同参数)。
- 浏览器 WebGPU 与 native wgpu:同帧 RGBA 哈希一致;若适配器不同,
  逐像素差必须能归因到驱动/舍入,并写进差异账本;禁止语义性差异。
- 三端 RenderReport 的 `ops_total/drawn/skipped` 与跳过原因逐字段一致。

### 1.3 回归

- `cargo test -p yu-gfx -p yu-backend-soft -p yu-backend-wgpu` **全绿**,
  包括 `eris_first_screen` 两条 golden(先解锚点再定 golden,禁止
  `UPDATE_GFX_GOLDEN` 掩盖)。
- clippy 三个 crate 0 新增告警;`src/` 零 `println!/eprintln!/dbg!`。
- 浏览器宿主 smoke:title 出现、按钮可点区域存在、Restart 后复现;
  帧率与 P0 声明阈值对照。

### 1.4 性能(WebGPU)

- P0 声明基线:title 稳定帧渲染耗时、纹理上传字节、draw 数、管线数。
- 验收:连续 600 帧无逐帧重传;帧时间 p95 不超过声明阈值;
  录制一次 WebGPU 时间线/报告写入 PROGRESS。

## 2. 阶段

### P0 · 三端复现与基线(先做)

1. native soft / native wgpu:跑 `YURIS_FIRST_FRAME_TICK=300` 与 600 tick,
   记录 ops/skipped、asset.missing、每帧 transition/copy/image、帧耗时。
2. 浏览器:用 K2 宿主跑 boot→title,记录 `/api/report`、帧哈希、按钮缺失、
   控制台错误;**不改宿主**。
3. 产出 `docs/title-motion-ledger.md` 增补节:三端基线表 + 阈值声明。

**门禁 G0**:三端基线可复现,才进入 P1。

### P1 · 关闭按钮列(唯一允许动 rt 的点)

- 证据:`file/fileinfo.html` 原文 + REA FILEINFO 处理体 + `es_Button.yst`
  的五次探测(已在 EVID-T-20..22)。
- 实现:`FILEINFO[EXIST=1 FILE=... LET=@x]` —— 按工程 yscfg 扩展名探测序
  判断存在性,写 LET;不存在写 0 或按证据;不改其它 File 命令语义。
- 验收:title 稳定帧 `asset.missing` = 0;按钮纹理名分别为
  `cgsys/title/btn_*_na|_off|_on|_over`;三端按钮区域对官方图
  (阈值先声明,建议平坦区 max≤4、diff>4=0;含按钮列)。
- 回归:新增 fixture/真实语料测试;`cargo test -p yu-rt` 全绿。

### P2 · 解 boot/title 锚点(跨 crate,但只改允许的文件)

- 用官方证据(DIV-L-001:窗口标题 `【デバッグモード】`)确定运行配置;
  给 gfx/后端示例与测试注入 `debug_mode=true`(必要时同步 K2 宿主的
  启动参数,以交接方式)。
- 重跑 census:
  - 若回到 title 锚点且与官方 trace 一致 ⇒ 更新 gfx golden(先审 census 断言);
  - 若仍不一致 ⇒ 用官方 L1 trace 在 script 20 pc237 等锚点二分,定位
    `1365918` 的 YSVR/GOSUB 修正是否引入偏差;需要改 rt ⇒ 停下写报告。
- 验收:`cargo test -p yu-gfx` 全绿;golden 更新有证据、无 UPDATE 掩盖。

### P3 · WebGPU 一等公民验收

- native soft vs native wgpu:title 稳定帧 + 转场区间逐帧 PNG 一致。
- 浏览器 WebGPU:同一帧哈希与 native wgpu 对照;差异写账本。
- 动画:三端逐帧对照官方微场景(至少 title 前 600 tick 内全部 transition),
  记录每帧 T/TA/fade 与像素差。
- 性能:按 §1.4 记录并对照阈值。
- 验收命令与原始输出进 PROGRESS。

### P4 · 收尾

- 更新 `docs/gfx-unimplemented-todo.md`、`crates/yu-gfx/docs/divergences/README.md`、
  `docs/todo-inventory.md`:关闭 043(按钮)、锚点相关项;记录 045/008/041 终态。
- PROGRESS“成果 J3”:三端对照表、按钮区域对拍、动画逐帧结果、性能数字、
  仍 blocked 项的证据。
- 被跟踪文件一律占位符;截图/宿主产物不入库。

## 3. 证据纪律

- 新语义必须有四件套(手册/真实脚本/REA/官方截图或微场景);
  “未取证”不是停机理由:去取证再实现;只有硬阻塞才挂账并上报。
- 代码注释用 `EVID-T-xx`(沿用 J2 台账)或新增 `EVID-J3-xx`。
- 不把“浏览器看起来一样”当证据:WebGPU 必须给哈希/像素差数字。

## 4. 环境

```bash
export TEMP="<side-repo>/workbench/tmp"; export TMP="$TEMP"
export YURIS_REQUIRE_SAMPLES=1
export YURIS_SAMPLE_ERIS_ROOT="<side-repo>/workbench/project-eris"
export YURIS_SAMPLE_ERIS_YPF="<side-repo>/workbench/project-eris/data.ypf"
export YURIS_WGPU_BACKEND=dx12          # native
# 浏览器宿主按 K2 的 README;J3 只读其 /api/report 与帧哈希
```

## 5. 停止/上报条件

- 需要改 `yu-cli`/`yu-host-wasm`/`www` ⇒ 写交接(K2 的文件),不抢改;
- 需要改 rt 的 FILEINFO 之外语义 ⇒ 停,写报告;
- 组 RT alpha 需要契约变更 ⇒ 先设计小节等确认;
- 硬阻塞(官方引擎/REA/样本不可用)⇒ 挂账 + 已尝试路径。
