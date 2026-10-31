# 任务书 N · `yu-sound`:声音域宿主(SOUND / SOUNDINFO / SOUNDEND)

> 仓库:`<repo>`。先读 `AGENTS.md`、本文件、`PROGRESS.md` 最新成果、
> `docs/architecture.md` §0/§2/§8、`crates/yu-rt/docs/domain-event-contracts.md`、
> `crates/yu-rt/docs/coverage-matrix.md` §1(声音域各行)、
> `crates/yu-gfx/src/host.rs` 与 `crates/yu-cli/src/engine.rs`(宿主装配样板)、
> `docs/audit-2026-10-22.md`(工程纪律教训)。
>
> 独占新建:`crates/yu-sound/**`、`docs/sound-ledger.md`、
> `docs/divergences/DIV-SND-*.md`、`crates/yu-sound/docs/divergences/`。
> **加法式修改**:`crates/yu-rt`(仅声音域接线:`SoundHost` trait +
> `Domain::Sound` 同步路由 + SOUNDINFO 查询路径 + 单测)、
> `crates/yu-cli`(装配)、`crates/yu-trace`(仅当需要时先报告)。
> 其余 crate 只读。**禁止从 `<repo-old>` 复制代码**;其事实/结论可引用
> (注明出处)。零 git 写操作;`PROGRESS.md` 只追加自己的"成果 N…"。

## 0. 任务定位

声音域当前状态:VM 对 `SOUND`/`SOUNDINFO`/`SOUNDEND` **只发
`VmEvent::DomainCall` 域事件**(契约见 domain-event-contracts.md),
子系统语义归宿主——即本任务要建的 `yu-sound`。ERIS 语料 600 帧里
SOUNDINFO 触发 7,200 次(title 每帧轮询 BGM 状态)、SOUND 531 次,
全部以 Unimplemented 空转;title 的 BGM 模式按钮依赖声音状态查询。

目标:与官方引擎的声音域可观测行为 1:1——同一数据 + 同一帧时钟下,
**脚本可见的查询值**(SOUNDINFO 的 `LET` 写回)逐值一致;声音输出本身
不是 golden 对象(设备/混音听感不进 A3 断言),但**驱动查询值的层状态机
必须确定性**。

```text
yu-rt(声音域命令求值 → DomainCall)
        │ 同步 apply_event / query(SoundHost trait,本任务新增,仿 GraphicsHost)
        ▼
yu-sound SoundState(层表 = 状态机)
   ├─ 装载:loader(装配层注入,走 VFS)→ 解码(OGG Vorbis / RIFF WAVE)
   ├─ 推进:注入时钟逐帧 advance → 循环/延迟/淡变/位置
   └─ 查询:SOUNDINFO 19 字段 → Value 写回 LET
        │
        ├─ headless render(&mut [f32])(确定性;测试/golden 用)
        └─ 设备输出(cpal,feature 门控;不参与 golden;wasm 端后续任务)
```

### 0.1 硬性总则

1. **取证先行**:每个语义三层证据(手册页 / 真实语料 / REA)缺一 ⇒
   `Unimplemented` + DIV 档案,禁止猜(A4/A5 原文)。先做 Phase N0。
2. **A5 零硬编码**:层 ID(BGM/SE/VO/…)、文件名、目录名是数据不是代码;
   缺层表概念不存在——层是脚本用 `ID=` 现场声明的。允许进代码的只有
   引擎算法语义:参数表(手册)、扩展名探测序(手册原文)、CODEC 枚举
   (手册)、循环/淡变/混音算法(REA)。
3. **A3 确定性**:状态机只由注入时钟推进(帧时钟 `frame_ms`);同一数据 +
   同一时钟 ⇒ 同一查询值序列、同一 render 输出。混音核心单线程纯函数。
   设备输出只是消费同一状态的旁路,不反哺状态。
4. **A1 数据源不可见**:`yu-sound` 不碰文件系统、不认识 ypf;装载一律经
   装配层注入的 loader 抽象(参照旧线 `AudioLoader` 的形态,自写接口)。
5. **log 纪律**(architecture §8):一切错误/诊断走 `yu-log::Diag`;
   `snd.*` 码表 + `ALL_CODES` 等价唯一性单测;严重级语义对齐
   (`Unimplemented` = 已知语义未实装,按官方语义继续并留痕;
   `Recoverable` = 官方可继续,策略按官方观测填充)。`src/` 零
   `println!/eprintln!/dbg!`(audit C2 教训,CI grep 守卫同族)。
6. **代码质量**:`#![forbid(unsafe_code)]`(与 yu-gfx/yu-rt 同);
   `cargo clippy -p yu-sound -p yu-rt -p yu-cli --all-targets` 零新告警;
   `cargo test --workspace` 全绿;真样本缺失时测试**显式跳过并计数**,
   禁止空跑通过。
7. 被跟踪文件一律占位符;音频产物/截图不入库;新 example 命名不与现有
   example 重名(K2 教训:soft/wgpu 同名 `first_frame` 互相覆盖)。
8. 新依赖须在任务报告声明 license 与理由(解码后端选型见 §3)。

### 0.2 事实基线(2026-10-22 实测复核,引用前先重验)

#### 手册(SDK 0.495 `マニュアル/YU-RIS/html/sound/`,CP932 原文;只有三页)

**SOUND[]**(`sound.html`,REA 分发表 opcode 87 处理体 `0x44e1e8`):

| 参数 | 类型 | 手册语义 |
|---|---|---|
| `ID` | 字符串 | サウンドデータを置くレイヤＩＤ |
| `FILE` | 字符串 | 拡張子省略可;查找序「(拡張子なし)」「.OGG」「.WAV」 |
| `E` | 0/1 | 层有效/无效(0 = 无视音量的ミュート状态) |
| `PLAY` | 0/1 | 1 再生 / 0 停止 |
| `LOOP` | 整数 | 0 = 無限ループ、1 以上 = 再生回数 |
| `FADE` | 整数 | フェードイン／アウトの時間(单位手册未写,按引擎 ms 惯例待 REA 复核) |
| `PAUSE` | 0/1 | 1 一時停止 / 0 再開 |
| `START` | 整数 ms | 再生開始位置;负值 = 遅延再生 |
| `RESTART` | 整数 ms | ループ２度目以降の開始位置;负值 = 遅延 |
| `END` | 整数 ms | 終端位置／ループ位置 |
| `VOLUME` | 0～256 | 音量 |
| `PAN` | 0～512 | 左～右;デフォルト 256 |
| `PITCH` | 0～512 | 0.5倍～1.5倍;デフォルト 256 |

官方示例 12 条(权威行为断言,须逐条在单测/对拍中复现):
循环(例1)、淡入 1.5 秒 + 3 回再生(例2)、START=3200 起播(例3)、
START+RESTART 第二圈从 8 秒起(例4)、START=-10000 延迟 10 秒(例5)、
PAN=512 右(例6)、PITCH=512 即 1.5 倍速(例7)、双轨同播(例8)、
E=0 静音(例9)、PAUSE 停/再开(例10/11)、播放中
`FADE=1000 VOLUME=128` 淡到目标音量(例12)。

**SOUNDINFO[]**(`soundinfo.html`,处理体 `0x44f4bc`):`ID` + `LET` +
单字段查询标志:`EXIST / FILE / E / PLAY / LOOP / PAUSE / LOOPCOUNT /
START / RESTART / END / VOLUME / PAN / PITCH / TIME(现在再生位置 ms) /
TOTALTIME(サウンドの長さ ms) / CHANNEL / BIT / SAMPLE / CODEC(0=WAVE /
1=OGG / 2=ACM WAVE)`。手册 4 个示例全部**单字段查询**;多字段同调行为
未取证。`FILE` 返回字符串,其余整数。

**SOUNDEND[]**(`soundend.html`,处理体 `0x44f308`):按 `ID` 删层。

**声音域系统变量**:只有 `@_OS_SOUND`(0/1;手册 `basic/svariable.html`;
yu-rt 已实现 `sysvar.rs`,宿主注入 `BootConfig.os_sound`)。
分发表里同属 sound 域的还有 **MIDI / MIDIEND / MIDIINFO**
(`0x44aa40`/`0x44aa60`/`0x44aa80`)与 **CDDA**(`0x429fb0`):语料 0 次、
手册无页 ⇒ **未取证,本任务默认保持 Unimplemented**(只允许取证加深,
不允许无证据实现)。

#### 引擎二进制(0.495 `yu-ris.exe`,sha256 `4f5eefa3…`;2026-10-22 字符串级复核)

- 导入/静态库证据:DirectSound;winmm(`mciSendStringA`/`mciSendCommandA`/
  `waveOutGetNumDevs`/`waveOutGetDevCapsA`/`mmio*`/`timeEndPeriod`);
  ACM(`acmStreamOpen`/`acmStreamSize`/`acmFormatSuggest`/`acmStreamPrep…`);
  静态链接 **`ogg_static.lib vorbis_static.lib vorbisfile_static.lib`**。
  `"fmod"` 命中全部是 CRT 数学函数(fmod/fmodf/fmodl 的 DOMAIN error 文案),
  与 FMOD 音频库无关(已甄别,不要再当证据引用)。
- ⇒ 推断链:OGG 解码 = vorbisfile;特殊 WAVE = ACM(对应 CODEC=2);
  输出 = DirectSound(待 REA 确认);MIDI/CDDA 可能走 MCI(未取证)。
  VOLUME 0..256 → DSBVOLUME(-10000..0,对数)、PAN → DSBPAN、
  PITCH → SetFrequency 的**映射公式是 N0 必取证项**(旧线列为 UNVERIFIED)。

#### 语料(ERIS)

- 音频全部 OGG,在包内:`data/bgm/bgm01..03.ogg`(bgm01 = 907,689 字节)、
  `data/se/*.ogg`、`data/voice/*/*.ogg`。数据入口 = 封包 `data.ypf` 优先
  (AGENTS §2)。
- 运行期(600 帧,`coverage_census`):SOUND 531 / SOUNDINFO 7,200 /
  SOUNDEND 1;静态 34 / 25 / 2。域事件实测样本
  (domain-event-contracts §2):`ID=BGM/E2BGM/VO VOLUME=256 E=1`、
  `ID=:/S0/ VOLUME=0`。
- ERIS 样板含 `ES.*.VoicePlay.N` 音声钩子(VOICE 播放走 SOUND),是
  动态对拍的现实入口。title 链路有 `btn_bgmmode` 等按钮依赖 BGM 状态。
- `@_OS_SOUND=0` + `@_MEDIA_READONLY=1` 时 ERIS boot 首帧缩为 2 层/4 op
  (PROGRESS K1)——声音初始化是脚本按 `os_sound` 分支的,装不装宿主
  必须跟随该开关。

#### 契约与既有预留(本任务要接住的)

- `domain-event-contracts.md` §1:宿主**按名消费**(`VmEvent::domain_args`),
  槽号不跨版本稳定;§2 明示**值域校验(0..256/0..512)归 yu-sound**,
  yu-rt 不因越界脚本值改语义;FADE 单位待复核。
- **DIV-RT-042(LET 引用身份)对 SOUNDINFO 不构成阻塞**:`CGINFO` 的
  `op_info` 既有模式 = VM 自己 `eval_lvalue` 持引用 → `host.query`
  返回 `Value` → `write_target` 写回(`handlers.rs` INFO 路径)。
  SOUNDINFO 照此模式接即可;该档案对 `SAVE[SET]` 仍 open,与本任务无关。
- `GraphicsHost`(`yu-rt/src/event.rs`)是 trait 样板:同步
  `apply_event`(仅 FrameStart/FrameEnd/Text/Graphics|Window DomainCall)+
  `query`(单值返回;多字段顺序未取证 ⇒ 返回错误);调用方不得重复消费
  已同步喂过的事件。`SoundHost` 镜像此契约,`Domain::Sound` 加入同步路由。
- yu-log 架构 §8 已预留 `snd.*` 码域;trace schema v3 已含 `os_sound`
  (BootSummary)与 sound 域事件字符串 ⇒ **trace 不需要 schema 变更**
  (新增事件变体才需要,禁止)。
- 旧线 `<repo-old>/crates/yuris-audio`(仅事实参照,禁止搬码):层模型、
  解码(symphonia)+ 输出(cpal)+ headless 确定性渲染的三分结构、其
  UNVERIFIED 清单 = 本任务的取证清单:**淡变曲线、声像曲线、重采样插值、
  混音限幅**;另记录 v494 处理体层字段布局与 `FILETYPE OGG=1/WAVE=2`
  (Likely)——注意这与手册 `CODEC 0=WAVE/1=OGG` 的枚举值**不一致**,
  属两代引擎内部字段 vs 对外查询值之差,须用 0.495 REA 裁决。

## 1. Phase N0 · 取证先行(最先做;产出决定后面所有门控)

方法:静态 REA(ghidra,`<side-repo>/tools`)+ 官方引擎动态对拍
(64 位 Python 驱动 32 位引擎,方法论见 `<repo-old>/docs/windows-handoff.md`,
只读参考、脚本重写;探针脚本用 SDK YSCom 编译——ERIS 官方编译链已打通,
见 PROGRESS 成果 8/9)。

必取证项(逐条落 `docs/sound-ledger.md`,行格式仿 `docs/fidelity-ledger-j.md`):

| # | 取证项 | 手段 |
|---|---|---|
| N0-1 | `SOUND` 处理体 `0x44e1e8`:层容器结构(按 ID 键?上限?)、各参数写入偏移、`FILE/E/PLAY/…` 处理次序 | 静态 REA |
| N0-2 | 循环推进:`LOOP/RESTART/END` 在混音循环里的作用点;`LOOPCOUNT` 何时 +1;`LOOP=0` 无限与 N 次的判定 | 静态 REA + 动态 |
| N0-3 | 延迟播放:负 `START`/`RESTART` 的实现(负位置?定时器?);延迟期间 `TIME`/`PLAY` 返回什么 | 动态(探针脚本每帧 SOUNDINFO) |
| N0-4 | `VOLUME→DSBVOLUME`、`PAN→DSBPAN`、`PITCH→SetFrequency` 映射公式;越界值(>256/>512/<0)行为 | 静态 REA + 动态边界值 |
| N0-5 | `FADE`:单位(推定 ms)、曲线(线性?)、与 `VOLUME/E/PAUSE` 交互(例12 语义) | 静态 REA + 动态 |
| N0-6 | `PLAY=0` 停止后层状态(位置归零?保留?);`PAUSE` 与 `E=0` 的差异 | 动态 |
| N0-7 | SOUNDINFO 19 字段各自来源;`TIME` 暂停/延迟/停止时取值;`TOTALTIME` 精度;`FILE` 返回形态(原始参数 or 解析后文件名);多字段同调行为 | 静态 REA + 动态 |
| N0-8 | `FILE` 缺失/解码失败:官方报错策略(静默?`ERROR[COM_XX]`?层残留?)与 `SOUNDINFO[EXIST]` 随后返回值 | 动态 + 静态 |
| N0-9 | 输出通路确认(DirectSound 二级缓冲?)与 mixer 主循环(采样率/格式) | 静态 REA |
| N0-10 | MIDI 族 + CDDA 处理体:是否 MCI 直通;只要能写出"可证伪的语义描述"即可,不实现 | 静态 REA |

动态对拍基准(循环播放的权威证据):YSCom 编译最小探针——
`SOUND[ID=BGM FILE="bgm/bgm01" LOOP=0 PLAY=1]` 后每帧
`SOUNDINFO[ID=BGM LET=@T TIME=1]` + `LOOPCOUNT=1`,把值画到屏幕或写
可观测变量,官方引擎跑 ≥2 个循环周期,记录 T/LOOPCOUNT 随帧的阶梯曲线;
同脚本换 `LOOP=2`/`START`/`RESTART` 各跑一遍。

**退出条件**:台账每行给出 手册/语料/REA/结论/Confidence;
证据不足的项显式标 `Unimplemented`(不许实现)。
同步产出:`DIV-SND-001…` 档案(无证据/与手册矛盾的项)、
`coverage-matrix.md` 声音域行批注更新。

## 2. Phase N1 · `yu-sound` 骨架:状态机 + Diag 码 + 命令面

- `crates/yu-sound`(依赖:`yu-rt`(event/value/Diag 类型)、`yu-log`;
  **不依赖** vfs/gfx/后端)。`#![forbid(unsafe_code)]`。
- `state.rs`:`SoundState` 层表(`ID → Layer`),`Layer` 含
  file/E/play/loop/pause/start/restart/end/volume/pan/pitch/fade/位置/
  loopcount/codec/总长等;构造不依赖任何工程表(A5:无内置层)。
- `command.rs`:SOUND 参数结构(按名),含 §0.2 全参数;值域校验在此做
  (契约 §2 授权),越界行为按 N0-4 结论(无结论 ⇒ Unimplemented)。
- `query.rs`:SOUNDINFO 19 字段查询 → `Value`;多字段请求返回错误
  (镜像 GraphicsHost::query 契约注释)。
- `error.rs`:`snd.*` 码表(如 `snd.layer.missing`/`snd.file.missing`/
  `snd.codec.unsupported`/`snd.arg.range`/`snd.decode.*`/`snd.query.unsupported`),
  唯一性单测守卫(仿 `yu-gfx/src/error.rs`)。
- 时钟:`advance_ms(ms)` 由帧循环驱动;所有查询值都是时钟的纯函数。
- 单测:代码内合成 WAV 夹具(PCM16 正弦波)驱动状态机;
  OGG 路径用语料(env 门控,缺失显式跳过并计数)。

## 3. Phase N2 · 解码与混音核心(headless 确定性)

- 解码后端选型(报告里声明):纯 Rust、GPL 兼容;旧线事实 = symphonia
  可解 OGG/Vorbis 与 RIFF WAVE。WAVE 支持范围 = 手册"特殊コーデック形式
  の WAVE には対応しない" ⇒ 普通 PCM;**ACM WAVE(CODEC=2)Unimplemented**
  (直到有人取到 N0 级证据)。
- 解码产出:PCM + channel/bit/sample/总长 ms(TOTALTIME 的来源)。
- 混音器:单线程纯函数 `render(&mut [f32])`;循环推进按 N0-2/3;
  淡变按 N0-5;**证据未覆盖的环节(重采样插值、限幅)保持最简单实现并
  在代码处标注 Unverified**,不声称 1:1。
- golden 形态:**状态查询值时间线(JSON)+ 小型 render 快照哈希**,
  不含音频文件本体;同一夹具跑两遍逐字节一致(A3 自证测试)。

## 4. Phase N3 · yu-rt 接线(加法式;无宿主时行为逐位不变)

- `event.rs`:新增 `SoundHost` trait(`apply_event` + `query`,文档注释
  镜像 GraphicsHost,含"调用方不得重复消费"警告)。
- `vm.rs`:`set_sound_host/take_sound_host`;`Domain::Sound` 的
  DomainCall 在产生时同步进 sound host(仅当已安装;事件流照发一次)。
- `handlers.rs`:SOUNDINFO 走 INFO 同款路径(VM 持 LET 左值 → query →
  `write_target`);未安装宿主 ⇒ 维持现状(`rt.cmd.unimplemented` 诊断,
  LET 不写)。SOUND/SOUNDEND 维持"事件 + 宿主消费"。
- **回归红线**:不装宿主时,`coverage_census` 600 帧的命令计数/事件流
  与现状一致(重跑记录进成果);装宿主后新增声音查询 golden。
- SOUNDEND 的同步语义(`EXIST` 随后为 0)由 N0-6/N0-7 证据定。

## 5. Phase N4 · 声场/音调/淡变/暂停(证据门控逐项解锁)

PAN / PITCH / FADE / PAUSE / E / 延迟播放逐项实现,每项台账行引用
N0 证据;无证据项保持 Unimplemented + DIV。官方示例 1..12 各配一条
状态机单测(值断言,不是听感断言)。

## 6. Phase N5 · 装配与设备输出

- `yu-cli`:`boot()` 在 `os_sound=true` 时安装 `SoundHost`,loader 用
  工程 Mount 实现探测序(`(无扩展名)→ .OGG → .WAV`;探测序是引擎语义,
  归 yu-sound 驱动,vfs 只答存在/读取);`os_sound=false` ⇒ 不装(现状)。
- `run`/`serve` 增加可选真实输出(feature 门控,cpal);**headless 与
  golden 路径默认无设备**;报告线程模型与状态共享策略。
- wasm:浏览器端出声不在本任务(架构 B 呈现端不跑内核;音频流送浏览器
  是新链路,留待后续任务,接口只留注释位)。

## 7. Phase N6 · 收尾

- `README.md` 模块表 `yu-sound` 行改写;`docs/architecture.md` v1.4
  (crate 数 12、依赖图加 `yu-sound`,依赖 `yu-rt`+`yu-log`,与 gfx 同级)。
- `docs/todo-inventory.md`:§4 域事件清单把 SOUND 三条移出并计数更新;
  §3.2 未取证清单标注 MIDI/CDDA 现状。
- `crates/yu-sound/README.md`:用法、证据等级声明(Confirmed/Likely/
  Unverified/Unknown 四级,学旧线 lib.rs 头的写法)。
- `PROGRESS.md` 追加"成果 N…"(逐 Phase,含验证命令与原始输出);
  clippy/test/eprintln 守卫全绿。

## 8. 并行占用

| 线 | 占用 |
|---|---|
| N(本任务) | `crates/yu-sound/**`(新)、`yu-rt` 声音域加法、`yu-cli` 装配加法、`docs/sound-ledger.md`、`DIV-SND-*` |
| 其它会话 | `yu-gfx`/双后端/`yu-font` 不碰;`yu-rt` 非声音域区域若发现被并行占用 ⇒ 停手写报告 |

文件零交集;`PROGRESS.md` 追加前重读文件尾。

## 9. 环境

```bash
export TEMP="<side-repo>/workbench/tmp"; export TMP="$TEMP"
export YURIS_REQUIRE_SAMPLES=1
export YURIS_SAMPLE_ERIS_ROOT="<project-eris>"   # 数据入口:官方挂载规则
# REA:ghidra 在 <side-repo>/tools;分析对象 0.495 yu-ris.exe(sha256 4f5eefa3…)
```

## 10. 停止/上报条件

- 需要 `yu-rt` 非加法改动(如给 `DomainCall` 加字段、改 `ArgValue`)⇒
  停,写报告(DIV-RT-042 类契约变更须主人裁决);
- N0 三层证据取不满 ⇒ 该项保持 Unimplemented 挂账,**禁止实现**;
- 解码/设备依赖引入冲突(license/wasm 目标)⇒ 报告后换选型;
- 目标文件正被其它会话写入 ⇒ 停手注明。
