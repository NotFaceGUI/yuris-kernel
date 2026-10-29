# 声音域取证台账（Phase N0）

> 对象：0.495 `yu-ris.exe`，sha256 `4f5eefa3544c7244a907477d89ccee97286f559df5e38b1b1fa674206981138f`
> （`<project-eris>/yu-ris.exe` 与 SDK `<sdk495>/システム/yu-ris.exe` 逐字节同哈希）。
> REA：Ghidra 12.1.4（`<side-repo>/tools`），静态反编译 + 导入表/字符串。
> 手册：`<sdk495>/マニュアル/YU-RIS/html/sound/{sound,soundinfo,soundend}.html`（CP932）。
> 语料：ERIS `data.ypf`（31,501,753 字节，FNV-1a64 `64d15de36b4fff30`）。
> 动态：见 §5（环境阻塞，已如实标注）。

## 0. 任务书 §0.2 事实基线复核

| # | 基线断言 | 复核结果 | 证据 |
|---|---|---|---|
| 0-1 | 手册只有三页 sound/soundinfo/soundend | ✅ 三页均存在，CP932 原文可读 | `ls .../html/sound/`；逐页转码 |
| 0-2 | SOUND 处理体 `0x44e1e8`、SOUNDINFO `0x44f4bc`、SOUNDEND `0x44f308` | ✅ 分发表 `DAT_007ace20[87/89/88]` 指向三者（`opcode_table.json`）；运行时 `0x44f4bc` 首字节 `57 56 55 53 83 ec 28` 与静态一致 | REA 反编译/读字节 |
| 0-3 | 导入表：DirectSound / winmm / ACM | ✅ `DSOUND.DLL::DirectSoundCreate`、`WINMM.DLL::{mciSendStringA,mciSendCommandA,waveOutGetNumDevs,waveOutGetDevCapsA,mmio*}`、`MSACM32.DLL::{acmStreamOpen,acmStreamSize,acmFormatSuggest,acmStreamPrepareHeader,…}` | REA `list_procedures`（EXTERNAL 项） |
| 0-4 | OGG 静态链接（ogg/vorbis/vorbisfile_static） | ✅ 字符串 `vorbis` @ `0x5b8e5c`；**OGG 装载 `0x422ab0` 只做 `ov_open` 族 + `ov_time_total`（`0x4806c5` 返回秒 ×`1000.0` 写 `+0x120/+0x134`），没有任何 `ov_read`/整段解码**；尾部 `0x422c65` 只写格式字段并 `malloc(0x1000)` + `malloc(bytes_per_sec/2 + 0x1000)` 后 `CreateSoundBuffer`（**0.5 秒环**）。2026-10-28 按 `procedure_pseudo_code` 逐行复核（旧表述「同步整段解码」已勘误，见 N0-13） | REA 字符串 + 反编译 `0x422ab0` / `0x422c65` / `0x4806c5` |
| 0-5 | `"fmod"` 与 FMOD 音频库无关 | ✅ 仅命中 CRT 文案：`fmodl`/`fmod`/`fmodf: DOMAIN error`（`0x80ce18..0x80ce60`） | REA `search_strings` |
| 0-6 | 语料音频全 OGG（bgm/se/voice） | ✅ 包内 `bgm/bgm01..03.ogg` 等；`bgm01.ogg` 907,689 字节（官方 exe 解包副本） | `data.ypf` / `save/data/bgm` |
| 0-7 | 600 帧运行期 SOUND 531 / SOUNDINFO 7,200 / SOUNDEND 1 | ✅ 本次重跑 `coverage_census 600 8`：531 / 7,200 / 1（总命令 7,735,670；与本次改动前逐项一致，见 §6） | `workbench/tmp/snd-rea/census2.log` |
| 0-8 | `@_OS_SOUND` 是声音域唯一系统变量 | ✅ `sysvar.rs` 已有；宿主不装时行为不变 | `crates/yu-rt/src/sysvar.rs` |
| 0-9 | MIDI 族 + CDDA 无手册页 | ✅ 0.495 处理体是**空桩**：`0x44aa40/0x44aa60/0x44aa80` 仅 `FUN_00429fd1` 守卫后 `return 0`；`0x429fb0` 同。**B6 终态 aligned-stub**：内核同型对齐（域事件照发、无媒体输出、诊断归属 `rt.cmd.aligned_stub`、不计挂号） | REA 反编译（各 26–29 字节）；`yu-rt/src/vm/handlers.rs` |
| 0-10 | 旧线 `FILETYPE OGG=1/WAVE=2` 与手册 `CODEC 0=WAVE/1=OGG` 冲突 | ✅ **两个不同字段**：`+0x105` 内部 FILETYPE（OGG=1/WAVE=2），`+0x107` 对外 CODEC（WAVE=0/OGG=1/ACM=2） | REA `0x422ab0`/`0x422e04` 写入点；SOUNDINFO 查询读点 |
| 0-11 | B6 收口复核（2026-10-24，REA/Ghidra 12.1.4 重开 `yu-ris.exe` 同哈希） | ✅ ① SOUND 处理体只读 `0x684160..0x68416c`（参数 0..12），**从不读 `0x68416d`（槽 13=SPEED）** ⇒ 官方静默忽略；② WAVE 装载 `0x422e04`：`fmt tag==1` 走 PCM，**其余全部 tag 走 ACM**（7 个 acm* 导入的全部调用点 `0x423086..0x4232a4`，IAT 跳板 `0x47f86e..0x47f892`）；`acmFormatSuggest(NULL,src,dst=PCM,0x12,WFORMATTAG)` → `StreamOpen(…,NONREALTIME)` → `Size` → `PrepareHeader` → 一次 `Convert` → `Unprepare` → `Close`，失败=装载失败，`+0x107=2`；③ 延迟累加器（`0x423f9c`/`0x424424` 反编译）：`+0x190/+0x194 += |param|*bps/1000` 后按 block align **向下取整**，WAVE 填充位置 `+0x184` 从字节 0 重灌、`+0x17c` 保留负起始字节；④ **DATARIGHT 16-bit 立体声读 `word [ptr+4]`＝下一帧左声道**（越 4 字节锁区，官方怪癖；8-bit 立体声读 `ptr[1]`）——勘误 N0-12 旧表述 | REA `procedure_pseudo_code 0x422e04/0x423f9c/0x424424`；原始字节 `0x44ff58..0x4500d0` 手工解码（IAT/节表脚本，`workbench/tmp/snd-rea/acm_iat.json`） |

## 1. N0 必取证项

| # | 取证项 | 结论 | 证据 | Confidence |
|---|---|---|---|---|
| N0-1 | SOUND 层容器/参数写回 | 层表 = 名字哈希表（MurmurHash2，`0x5bd1e995`）；节点 `+0x34` 指向声音对象；`FUN_004141e4` 由 ID 建路径，`FUN_00414559` 查表；新层 `0x38` 字节节点、`+0x1c` 存 hash（一分支 `+0x80000000`） | `0x44e1e8` 伪代码 `LAB_0044e269`；`0x414559` | Confirmed（静态） |
| N0-2 | 循环推进 | `TIME = position_bytes*1000/bytes_per_sec`；`TIME >= END` 时：`LOOPCOUNT(+0x110) += 1`；若 `LOOPCOUNT == LOOP(+0x10c)` 则 `LOOP=0, LOOPCOUNT=0, position=0, stop`；否则 `position = RESTART(+0x12c) ms → bytes`。`LOOP=0` 永不相等 ⇒ 无限；`LOOP=N` 在第 N 次边界停止。跨边界不携带余量 | `0x464960`（`__allmul(iStack_24, …, bytes/1000)`、`+0x110+1`、`+0x10c`） | Confirmed（静态） |
| N0-3 | 负 START/RESTART 延迟 | **B6 已实现**（DIV-SND-002 → fixed(Likely)）：字节累加器公式逐字节取证（0-11③）后，内核以 ms 域等价实现——负 START/RESTART 在 PLAY 起点或循环边界把位置钉 0、记 `delay=|param|`ms，推进时钟只烧延迟不推进位置（TIME 冻结），混音端渲染静音。Likely 项（动态样本未到）：延迟期间 TIME=0、PLAY=1、输出静音；`+0x190` 混音线程消费点未逐行取证 | `0x423f9c` / `0x424424`（REA 反编译 2026-10-24） | Confirmed（静态）/ Likely（动态） |
| N0-4 | VOLUME/PAN/PITCH 映射 | VOLUME：0..256；`DSBVOLUME = clamp(round(4000*log10(v/256)), -10000, 0)`（v=0 特判 -10000）；DirectSound 振幅 = `10^(dB/20)`，即 **(v/256)^2**（不是线性）。PAN：`gauge=min(pan,512-pan)`（>256 取反），`DSBPAN=clamp(round(4000*log10(gauge/256)),-10000,0)`；越界回 256。PITCH：`freq = base + round((pitch-256)*(base/2)/256) = base*(pitch+256)/512`；越界回 256；`SetFrequency` | `0x424e4c`、`0x424fbc`、`0x423614`；常量 `0x58ecc0=1/256, 0x58ecc8=40.0, 0x58ecd0=100.0` | Confirmed（静态） |
| N0-5 | FADE 单位/曲线/交互 | 单位 ms；线性；固定点 1/1024：`step=(target-current)*0x400/duration`，每 tick `current = base + elapsed*step/0x400`；淡入完成 state 2→3；淡出完成 state 4→0，音量字段重置 256；暂停时把淡变起始时刻顺延 | `0x423718`（`timeGetTime()-+0x160`、`iVar4*+0x15c`、`>>10`）；`0x464be4` | Confirmed（静态） |
| N0-6 | PLAY=0 / PAUSE / E=0 | `PLAY=0`：有 FADE→state=4 淡出，否则立即 state=0；`+0x118` 位置**不归零**（下次 PLAY 从 START 重新取）。`PAUSE=1`：`+0x109=1`，位置与淡变都冻结；`PLAY` 仍为 1（state 2..4）。处理体顺序 **PLAY(0x684163) → PAUSE(0x684164)**；PLAY 不清 `+0x109`，暂停只由 `PAUSE=0` 解除。`E=0`：`+0x106=0`，混音乘 0（静音），PLAY/位置继续 | `0x44e1e8` 分块；`0x423718`；`0x464960`；`0x424dfc` | Confirmed（静态） |
| N0-7 | SOUNDINFO 19/24 字段来源 | 见 §2 表；`TIME` 特判 `pos*1000/bytes_per_sec`（除数为 0 时清 LET）；`TOTALTIME` 来自 `+0x120`（ms）—— OGG 由 `0x422ab0` 用 `ov_time_total`（最后 page granule）写入，WAVE 由 `0x422e04` 用数据长度/ACM 输出长度写入；`FILE` 走字符串写回，值是**归一化后的最终候选名**（ASCII 小写、含 `.ogg/.wav`）；**多字段**：`EXIST..PITCH`（槽 1..7）各带独立 `ret`，按固定顺序**首个命中即返回**；若这 7 个都不在请求里，其余字段连续执行、最后者胜出；缺层把 LET 清 0（字符串清空） | `0x44f4bc` 原始汇编：`0x684161..0x684167` 的块各自以 `ret` 结束（`0x44fcb4`/`0x44fc69`/…/`0x44fadf`）；`0x684169/0x684168` 起连续到 `0x44fad5`；`0x44f79a/0x44f899` | Confirmed（静态） |
| N0-8 | 文件缺失/解码失败 | **无候选文件**：`FUN_0040c9b4()==1` 时 SOUND 处理体立即 `return 0`，同一命令的 PLAY/VOLUME/START 等全部不应用。此前若 `state != 0` 已入队 type 0xe：清 `+0x000`、Stop、`0x423940` 释放 PCM 并清 START/RESTART、state=0；**保留** `+0x105/+0x107/+0x120/+0x118/PAN/PITCH/LOOP/LOOPCOUNT`。若本来 `state==0`，完全不变。**解码失败**：type 1 已重置 PAN/PITCH/START/RESTART/END/位置并写入文件名，缓冲创建失败后 state 仍 0，`+0x105/+0x107/+0x120` 保留旧值；后续参数继续应用 | `0x40c9b4`、`0x44e1e8`（返回点 0x44f0xx）、`0x4253b8`（type 0xe）、`0x423940`（Stop+Release+清 0x128/0x12c）、`0x424614` | Confirmed（静态） |
| N0-9 | 输出通路/mixer | DirectSound 二级缓冲（`DirectSoundCreate` → `CreateSoundBuffer` 虚表 `+0xc`）；`GetCurrentPosition` 虚表 `+0x10` 推进字节位置；`Play` 虚表 `+0x30`；`SetVolume +0x3c`、`SetPan +0x40`、`SetFrequency +0x44`。缓冲格式由解码器给出（channels/rate/bytes），`+0x198 = channels*rate`（bytes/sec） | `0x422c65`、`0x464960`、`0x424e4c`、`0x424fbc`、`0x423614` | Confirmed（静态） |
| N0-10 | MIDI/CDDA | 空桩（见 0-9）；无 MCI 直通调用 | REA 反编译 | Confirmed（静态） |
| N0-11 | 装载重置 / VOLUME 无 PLAY / LOOPCOUNT 生命周期 | type 1 load（`0x424614`）重装时 PAN/PITCH→256、START/RESTART/END/位置清零；E/VOLUME/LOOP/LOOPCOUNT **保留**。type 5（`0x42511c`）无 PLAY 时立即写 `+0x148/+0x154/+0x150` 并 SetVolume；有 PLAY 时 type 6（`0x425164`）只更新目标，由 type 2 的 FADE 淡变。`+0x110`(LOOPCOUNT) 只由 `0x464960` 的边界写；type 1/2/14 都不清零。PLAY 的 state 0 分支若 `END <= START` 则把 START 写 0。释放辅助 `0x423940`（Stop + Release + 释放解码缓冲）只清 START/RESTART，不清元数据/音量/LOOP | `0x424614`、`0x42511c`、`0x425164`、`0x424950`、`0x464960`、`0x423940` | Confirmed（静态） |
| N0-12 | DATALEFT / DATARIGHT | **B6 已实现**（DIV-SND-001 → fixed(no-manual)）：`0x44fe3c/0x44ff58` 要求缓冲存在、CHANNEL∈{1,2}、BIT∈{8,16}；在播放光标处 Lock 一帧。16-bit 取首个 i16 的绝对值（-32768 回绕为 -32768）；8-bit 取 `abs(byte-0x80)`；DATARIGHT 单声道返回 -1；失败 -1。**勘误（2026-10-24 字节级复核，0x4500a1..0x4500be）**：16-bit 立体声 DATARIGHT 官方读 `word [ptr+4]`——越过 4 字节锁区＝**下一帧的左声道**（旧表述「取第二采样」只对 8-bit 的 `ptr[1]` 成立）；官方越界怪癖 1:1 保留。手册无页（2026-10-24 政策：按 REA 实现并标注） | `0x44fe3c`/`0x44ff58` 原始汇编；`0x44f9f1`/`0x44fa3d` 调用点；内核 `pcm_data_sample` + 单测 `dataleft_dataright_read_play_cursor_frame` | Confirmed（静态字节级） |
| N0-13 | 装载/播放架构(整段 vs 流式) | **官方装载不整段解码**:`0x422ab0` 仅 `ov_open` + `ov_time_total`;`0x422c65` 写入 `+0x1b8/+0x1ba/+0x1bc/+0x1c6` 格式字段(`+0x1b4/+0x198` = `channels*rate` 字节/秒)后,只 `malloc` 一个 4KB 块与 `bytes_per_sec/2 + 0x1000`(**0.5 秒**)块并 `CreateSoundBuffer`(虚表 `+0xc`)。解码是播放期按需填充,不在 SOUND 命令里。`TOTALTIME` = `ov_time_total` = 最后 page granule/rate。**旧内核在 SOUND 命令里整段解出 `Vec<i16>`(语料 90s BGM ≈ 93–98ms)是架构偏差,已按官方改为流式(`yu-sound/src/stream.rs`:头部 O(1) + 0.5s 预取窗口 + 按需 `read_dec_packet_itl`)。granule 与 lewton 末包裁剪在 98/6966 个文件上 ≤36ms 不一致(lewton 不按 granule 裁剪的已知偏差);总长以官方 granule 为准,多出的样本截断、不足补静音,保证 TIME/END/LOOP 自洽。 | REA `procedure_pseudo_code 0x422ab0/0x422c65/0x4806c5`(2026-10-28);语料审计 `cargo run -p yu-engine --example audio_duration_audit`(koto 6966 文件 `header_drift=98 max_drift_ms=36`,ERIS 8 文件 0 漂移);单测 `gated_ogg_load_reads_header_only_then_streams_on_play` / `gated_ogg_stream_render_matches_full_decode` / `gated_ogg_loop_wrap_back_to_zero_reopens_stream` | Confirmed(静态 + 语料自证 + 逐样本对拍) |
| N0-14 | 层路径树 / 音量继承 / SOUNDEND 语义 | **声音层是名字树,不是扁平表**:ID 先经 `FUN_004141e4` 规范化(前缀 `S0` = `.data:0x80bf54` → `[0x801718]`;字符表 `0x689b80` 运行时只有 `'.'`/`'/'`/`':'` 非零;`:/` 绝对写法、`//`/`./`/`../` 折叠、末尾 `/`;空格 ⇒ -1),`FUN_00414559` 按 `/` 逐段下行(节点 `+0x10` 父 / `+0x00` 首子 / `+0x0c` 兄弟 / `+0x30` 容器 / `+0x34` 声音对象)。有效音量 `FUN_00424dfc`:`v=(E==1)*VOLUME; v=v*parent_eff>>8`(每级截断),根层 `S0` 就是脚本的 `SOUND[ID=":/S0/"]`(主音量)。容器改音量(type 5 `0x42511c` → `FUN_004238e0`)时 `+0x30!=0` ⇒ `FUN_00424db8` 递归重写全部后代的有效音量。`SOUNDINFO[VOLUME]` 读 `+0x148`(自身字段)。`SOUNDEND 0x44f308` 不删节点:入队 type 0xe(`0x4253b8`,state==0 时 no-op),清文件名/停播/释放/清 START+RESTART、state=0,保留元数据/TOTALTIME/位置/PAN/PITCH/LOOP/LOOPCOUNT/E/VOLUME;ID 以 `/` 结尾时递归停止叶子后代(`0x44f388`→`0x44f3a4`)。 | REA `0x462566`(root malloc)/`0x462577`(root 声音对象)/`0x462592`(前缀复制)/`0x4625be`(前缀建根层)/`0x4141e4`/`0x414559`/`0x41478c`/`0x424dfc`/`0x424db8`/`0x4238e0`/`0x42511c`/`0x425164`/`0x4253b8`/`0x44f308`;运行时 ReadProcessMemory 层树(prefix=`S0`,S0→BGM→B1,15 频道容器,字符表 {'.','/',':'});脚本 `ERIS/Sound/es_Sound.yst` `es.SOUND.SETVOL`/`es.VOL.*.GET` | Confirmed(静态 + 运行时 + 语料) |

### 1.1 文件名归一化与扩展名探测（REA `0x40c9b4` / `0x45f348`）

- `0x45f348`：整条文件名 ASCII 大写→小写（非 ASCII 不动）。
  **勘误（2026-10-27）**：“ASCII 逐字节小写”的字面读法不成立——必须按
  **CP932 字符**推进。双字节字符的尾字节可以落在 `0x41..=0x5A`（`コ`=`83 52`、
  `イ`=`83 43`），逐字节折小写会把它改写成另一个字符（`コトネイロ`→`ビトネツロ`，
  即 `83 72`/`83 63`）。反证：`kotoneiro/data.ypf`（v494，12,212 条）里
  `voice\コトネイロ.ogg` 就按 `83 52`/`83 43` 存储，官方引擎能装载它；若处理体
  逐字节改写则包内名字也对不上。内核 `yu-sound::normalize_name` 已按
  `yu_rt::cp932_text::char_len_at` 跳步（成果 SND-CP932）。
- `0x45a064` 对 `0x8079dc/0x8079e4` 做**尾部**比较：已以 `.wav`/`.ogg`
  结尾则不再追加；否则依次追加小写 `.ogg`、`.wav` 探测。
  尾比较同样只应看末尾 4 字节：`.`(`0x2E`) 不是 CP932 lead 也不在尾字节
  区间，后缀不可能切进双字节字符。
- 最终候选名写入输出缓冲（`local_24`），即声音对象 `+0x000`；
  `SOUNDINFO[FILE]` 返回它，而不是脚本里的原始 `FILE` 参数。
- 探测全失败时 `local_24` 停在最后一个候选；但处理体此时直接返回，
  **不会入队 type 1**，所以这个候选名不会复制进 `+0x000`（见 N0-8）。


### 1.2 层路径树、音量继承与 SOUNDEND(2026-10-29)

**ID 规范化** `FUN_004141e4`(调用点 `0x44e235` SOUND / `0x44f500` SOUNDINFO /
`0x44f346` SOUNDEND;前缀 `[0x801718]` = `"S0"`,来源 `.data:0x80bf54`,
启动时 `0x462592..0x4625a6` 复制):

- 字符分类表 `0x689b80` 运行时实测只有 `'.'(0x2e)`、`'/'(0x2f)`、
  `':'(0x3a)` 非零(静态文件里是 BSS 零,启动期初始化;直接搜索立即数
  只有读点 `0x414205`,写点经计算地址)。
- ID 不含这三者 ⇒ 快速路径:`前缀 + "/" + ID`(空 ID ⇒ `"S0/"`)。
- 含任一者 ⇒ 慢路径:以 `:` 开头为绝对 ID(`:/S0/` → `S0`);
  折叠连续 `/`、去掉非末段 `./`、`../` 回退一级、去掉末尾 `/`;
  段内空格 ⇒ 返回 -1(SOUND/SOUNDINFO/SOUNDEND 处理体整条不应用)。
- 原 ID 以 `/` 结尾 ⇒ 返回 1(SOUNDEND 用它区分递归停止子树)。

**层树**:`FUN_00414559` 按 `/` 逐段下行,`FUN_0041478c` 用 MurmurHash2
查子节点(只查不建)。**建节点规则**(`0x44ef31..0x44f245`):带 FILE 且
ID 不以 `/` 结尾 ⇒ 叶子(`+0x30=0`,写入点 `0x44f1fd`);不带 FILE 且以
`/` 结尾 ⇒ 容器(`+0x30=1`,写入点 `0x44f08e`);带 FILE 且以 `/` 结尾
⇒ 错误 `0x18bac`;其余 ⇒ 整条命令丢弃。父链必须已存在(`FUN_00461328`
逐段查找,缺段 `0x18786` 失败),官方不补建中间节点。节点布局:`+0x00` 首子、
`+0x0c` 下一兄弟、`+0x10` 父、`+0x14/+0x18` 名字指针/长度、`+0x1c` hash、
`+0x30` 容器标记、`+0x34` 声音对象(容器也分配 `0x1d0` 对象,`0x462566..`)。

**有效音量** `FUN_00424dfc`:

```text
v = (E == 1) * VOLUME(自身 +0x148)
if 父节点存在且父节点有名字:
    v = v * FUN_00424dfc(父) >> 8      // 每级整数截断
```

脚本的 `SOUND[ID=":/S0/" VOLUME=主音量]` 就是根层 `S0` 自身,所以主音量
沿 `S0 → BGM → BGM/A1` 折进每一条声音;`SOUND[ID="BGM/" VOLUME=]` 是
频道整体音量。`SOUNDINFO[VOLUME]` 仍返回自身 `+0x148`(`0x44fb87`)。

**容器改音量** type 5 `0x42511c` 写 `+0x148/+0x154/+0x150` 后调
`FUN_004238e0`:节点 `+0x30 != 0` ⇒ `FUN_00424db8` 递归把每个后代的
有效音量重写进各自 DirectSound 缓冲;叶子 ⇒ 自身有效音量直接写。
所以主/频道滑杆一动,正在播的声音即时生效。

**SOUNDEND** `0x44f308`:不删节点。`0x44f3e8` 入队 type 0xe(`0x4253b8`):
`state==0` 时 no-op;否则清文件名 `+0x000`、停播、`0x423940` 释放缓冲、
清 START/RESTART、`state=0`;**保留** `+0x105/+0x107/+0x120/+0x118/
PAN/PITCH/LOOP/LOOPCOUNT/E/VOLUME`。ID 以 `/` 结尾时 `0x44f388` →
`0x44f3a4` 先递归停止所有叶子后代(容器保留),再释放目标自身。

**运行时层树**(官方引擎启动 ERIS 后 `ReadProcessMemory`):

```text
root(空名) → S0(容器, vol=256, E=1)
              ├─ BGM(容器) → B1(叶子, 播放中)
              ├─ SE / VO / EBGM / … / S2VO(15 条频道容器)
              └─ ROOT(引擎内部层, 非 SOUND 命令建)
prefix="S0"; 0x689b80 非零项={'.','/',':'}
```

`BGM/` 与 `:/S0/` 都**没有**产生 `S0/BGM/` 空名子节点或 `S0/:/S0` 节点,
证明规范化路径生效(否则主音量/频道音量会落到孤立节点上)。

**内核实现**:`crates/yu-sound/src/path.rs`(`normalize`)、
`state.rs`(`Layer.container`、根层 `S0`、`parent_exists` + 官方建节点规则、
`effective_for_key`、`apply_soundend`)、`mixer.rs`(按有效音量出增益)。单测见
`tests/state.rs` 的 `layer_ids_are_path_normalized` 等 7 项 + `path` 模块 7 项;
分歧档案 `DIV-SND-005`,设置面汇总 `docs/game-settings-ledger.md`。

## 2. 声音对象字段表（`layer+0x34`）

| 偏移 | 含义 | 写/读点 |
|---|---|---|
| `+0x000` | 装载文件名（`SOUNDINFO[FILE]` 来源） | `0x424614`（type 1 load 复制） |
| `+0x105` | 内部 FILETYPE：0=无, 1=OGG, 2=WAVE | 处理体 `0x44e1e8`；`0x422ab0` |
| `+0x106` | E（0/1） | 处理体 |
| `+0x107` | 对外 CODEC：0=WAVE,1=OGG,2=ACM | `0x422ab0` / `0x422e04` |
| `+0x108` | 播放状态：0=停,1=已装载,2=淡入/播放,3=稳定,4=淡出 | `0x424614/0x424950/0x423718` |
| `+0x109` | PAUSE（0/1） | `0x425090` |
| `+0x10c` | 配置 LOOP（0=无限） | 处理体；`0x464960` 停止时清零 |
| `+0x110` | LOOPCOUNT | `0x464960` |
| `+0x114` | LOOP 的另一个投影（type 7 写入） | `0x42519c`（未对外查询） |
| `+0x118/+0x11c` | 当前播放位置（字节，64 位） | `0x464960` |
| `+0x120/+0x124` | TOTALTIME（ms，64 位） | `0x422ab0`/`0x422e04` |
| `+0x128` | START（ms） | type 9 `0x4251d8` |
| `+0x12c` | RESTART（ms） | type 10 `0x425238` |
| `+0x130` | 暂停起始时刻（淡变时钟补偿） | `0x423718` |
| `+0x134` | END（ms），装载时默认 = TOTALTIME | 解码器；type 8 `0x425298` |
| `+0x138/+0x13c` | END 的 64 位覆盖（`+0x13c != 0` 时优先） | `0x464960` |
| `+0x140` | PAN（0..512） | type 11 `0x425300` |
| `+0x144` | PITCH（0..512） | type 12 `0x425350` |
| `+0x148` | 当前音量（0..256；淡变中实时值） | `0x423718`；type 5/6 |
| `+0x14c/+0x150/+0x154` | 淡变当前/目标/上一音量 | `0x424950`/`0x464be4`/`0x423718` |
| `+0x158` | FADE 时长（ms） | type 2/4 |
| `+0x15c` | 淡变步长（1/1024） | `0x423718` |
| `+0x160/+0x164` | 淡变起始/完成时刻 | `0x423718` |
| `+0x17c` | 本次播放起始字节 | `0x423f9c`/`0x424424` |
| `+0x180` | 缓冲半区状态（1/2，双缓冲） | `0x4239c8` |
| `+0x184` | 下次填充位置 | `0x423f9c`/`0x424424` |
| `+0x190/+0x194` | 负 START/RESTART 的延迟字节累加器 | `0x423f9c`/`0x424424` |
| `+0x198/+0x19c` | bytes/sec（64 位；TIME 除数） | `0x422c65` |
| `+0x1a0` | DirectSound 二级缓冲接口 | `0x422c65` |
| `+0x1ac/+0x1b0` | 解码器数据指针/长度 | WAVE 路径 |
| `+0x1b4` | 缓冲字节数（≈1 秒） | `0x422c65` |
| `+0x1b8/+0x1c8` | WAVE 格式 tag / 扩展 | `0x422c65` |
| `+0x1ba` | CHANNEL | `0x422c65` |
| `+0x1bc` | SAMPLE（采样率） | `0x422c65` |
| `+0x1c4` | block align | `0x422c65` |
| `+0x1c6` | BIT | `0x422c65` |

## 3. 参数槽（工程 yscm 实测）

官方 YSCom 0.495 编译的 ERIS `ysc.ybn`（`dump_yscm` 实测）：

```text
SOUND     (0:ID 1:E 2:FILE 3:PLAY 4:PAUSE 5:FADE 6:START 7:RESTART 8:END
           9:LOOP 10:VOLUME 11:PAN 12:PITCH 13:SPEED)
SOUNDINFO (0:ID 1:EXIST 2:E 3:PLAY 4:PAUSE 5:VOLUME 6:PAN 7:PITCH 8:FILE
           9:FILETYPE 10:START 11:RESTART 12:END 13:LOOP 14:LOOPCOUNT
           15:TIME 16:TOTALTIME 17:CHANNEL 18:BIT 19:SAMPLING
           20:DATALEFT 21:DATARIGHT 22:CODEC 23:LET)
```

**与手册的差异**（DIV-SND-001，B6 定案）：手册 SOUNDINFO 只有 19 个字段、顺序是
`ID LET EXIST FILE E PLAY LOOP PAUSE LOOPCOUNT START RESTART END VOLUME PAN PITCH
TIME TOTALTIME CHANNEL BIT SAMPLE CODEC`；0.495 编译器多出 `FILETYPE / SAMPLING /
DATALEFT / DATARIGHT`，且槽号不同。宿主按名消费，不按槽号。
`DATALEFT/DATARIGHT` ⇒ fixed(no-manual)（N0-12）；`SPEED` ⇒ **unsupported-by-
official**（官方静默忽略）。

SOUND 处理体读 `0x684160..0x68416c`（参数 0..12；2026-10-24 反编译逐槽核对，
**`0x68416d`=槽 13 SPEED 无任何读取点**——旧记法「..0x68416d」系端点含混，
以此为准）；SOUNDINFO 读 `0x684160..0x684176` 且
`0x684177` 是 LET 目标索引。由此确定：`0x684160 = 参数 0`，依次递增。

## 4. 公式（可直接落代码）

```text
VOLUME  : 0..256；E=0 ⇒ 0
DSBVOLUME(v) = clamp(round(4000*log10(v/256)), -10000, 0)，v=0 ⇒ -10000
振幅     = 10^(DSBVOLUME/2000) = (v/256)^2
PAN     : gauge = min(pan, 512-pan)；pan>256 取反
          DSBPAN = clamp(round(4000*log10(gauge/256)), -10000, 0)
          越界 ⇒ 256；pan=0/512 ⇒ ∓10000
PITCH   : freq = base*(pitch+256)/512；越界 ⇒ 256
FADE    : current = from + (to-from)*elapsed_ms/duration_ms（线性）
LOOP    : TIME >= END ⇒ LOOPCOUNT+1
          LOOPCOUNT == LOOP (LOOP>0) ⇒ LOOP=0, LOOPCOUNT=0, pos=0, stop
          否则 pos = RESTART
TIME    : pos_bytes*1000/bytes_per_sec（ms）
TOTAL   : round(frames*1000/sample_rate)（ms）
CODEC   : 0=WAVE,1=OGG,2=ACM（对外）；FILETYPE: 1=OGG,2=WAVE（内部）
```

## 5. 动态实测（Phase N0 要求）与阻塞

尝试：
1. 用官方 YSCom 0.495 编译最小探针工程（SDK 样板 + `system_start.yst`
   改为 `SOUND[ID=BGM FILE="bgm/probe" LOOP=0 PLAY=1]` + 每帧
   `SOUNDINFO[TIME/LOOPCOUNT]`），编译成功（82 个产物）。
2. 探针工程在 Locale Emulator 下引擎进程存活，但未进入脚本（窗口未创建）。
3. 改用正在运行的 ERIS 工程挂 WOW64 调试器（0x44f79a/0x44f899 断点）：
   - 从进程创建起调试：`SOUND` 处理体命中 1 次，`SOUNDINFO` 命中 **0** 次；
   - `0x8015c0`（声音初始化）运行期为 1，ring 空闲；
   - 结论：title 的 BGM 轮询在该 UI 状态下未执行，探针工程又受环境阻塞。

因此 **官方引擎的 LOOPCOUNT/TIME 逐帧曲线未取得动态样本**；台账对应项标
`Confirmed（静态 REA）`，并在 `DIV-SND-003` 记录阻塞与后续复现步骤。
没有把“静态可读”冒充成“动态已对拍”。

### 5.1 内核侧真实语料验证（不是官方引擎动态）

`crates/yu-cli/examples/sound_census.rs`（600 帧，带渲染）：

```text
frames=600 layers=167
id=BGM/B1 file=bgm/bgm01.ogg state=Playing time=6000 total=90802
loop=0 count=0 vol=256 e=true
snd_diags=0
```

`crates/yu-cli/examples/sound_loop_census.rs`（VM-only，6000 帧 × 16ms）：

```text
t=90144ms time=88176 total=90802 loop=0 count=0
t=95152ms time=2368  total=90802 loop=0 count=1
FINAL t=96000ms time=3200 total=90802 loop=0 count=1
snd_diags=0
```

这验证了真实 OGG 装载/解码、TIME 推进与 END 边界 LOOPCOUNT+1（`LOOP=0`
无限循环）。仍**不能**替代官方引擎动态对拍；`DIV-SND-003` 保持 open。

## 6. 回归基线（不装宿主）

`coverage_census 600 8`（`YURIS_SAMPLE_ERIS_ROOT=<project-eris>`）：

```text
合计: 命令 7735670 次(报告累计 0),事件丢弃 0
SOUND     531
SOUNDINFO 7200
SOUNDEND  1
```

与任务书 §0.2 的 531/7,200/1 逐项一致；未装 `SoundHost` 时 SOUNDINFO 仍走
M-2 的通用域路径，诊断文案保持 `域命令未接线:语义属宿主域状态机…`。

## 7. 清单（B6 收口后,2026-10-24）

原「Unimplemented 清单」五项全部落入终态（B6,`docs/tasks/roadmap-batches.md` §0）：

| 项 | 终态 | 锚点 | 档案 |
|---|---|---|---|
| 负 `START`/`RESTART` 延迟 | **fixed(Likely)**：静音区间 + TIME 冻结已实现,动态曲线待校准 | `0x423f9c/0x424424` 反编译 | DIV-SND-002 |
| ACM WAVE（CODEC=2） | **fixed(no-manual)**：win32 ACM 1:1(手册「不支持」句过时) | `0x422e04` else 分支;调用点 `0x423086..0x4232a4` | DIV-SND-004 |
| MIDI / MIDIEND / MIDIINFO | **aligned-stub**：官方空桩,内核同型(域事件照发,诊断 `rt.cmd.aligned_stub`) | `0x44aa40/0x44aa60/0x44aa80` | 台账 0-9 |
| CDDA | **aligned-stub**：同上(事件行为不变——CDDA 归类仍在 `Domain::Other`,不发域事件) | `0x429fb0` | 台账 0-9 |
| `SOUND[SPEED=]`（yscm 槽 13） | **unsupported-by-official**：处理体无读取点,静默忽略 | 反编译无 `0x68416d` 引用 | DIV-SND-001 |
| `DATALEFT` / `DATARIGHT` | **fixed(no-manual)**：按 REA 实现(含 DATARIGHT +4 勘误) | `0x44fe3c/0x44ff58` | DIV-SND-001 |

剩余挂账（维持）：

| 项 | 原因 | 档案 |
|---|---|---|
| 设备输出（cpal） | 默认 `device` feature：主线程 `render` → 环形缓冲 → cpal 回调（f32/i16/u16）；设备旁路不进 golden，`--no-default-features` 可关 | 任务书 N5 |

> **浏览器/`www` 声音（2026-10-22 决定）**：本仓库当前不是真正的 wasm 目标，
> `www` 只是胶水层；用户明确不需要浏览器出声。设备输出的终点是
> `yu-cli`（默认 `device` feature），`www` 侧不新增 AudioContext/AudioWorklet，
> 留到真 wasm 任务。

### 7.1 B6 验收记录（2026-10-24）

- `cargo test -p yu-sound`：**37/37 通过**（含 golden `loop_timeline` 不回归、
  真实 SDK OGG 门控用例、新增延迟 3 例/DATALEFT 1 例/**ACM 系统编码器
  MS-ADPCM 真实往返** 1 例）。
- `coverage_census 600 8`（不装宿主,ERIS 语料）：SOUND 531 / SOUNDINFO 7,200 /
  SOUNDEND 1 与基线逐项一致;M-2 诊断文案不变;MIDI/CDDA 计数 0。
  合计命令数与 10-22 基线的差值来自同树并行改动（B5/CGS revert）,与 B6 无关
  （B6 不触碰命令计数）。
- CLI 冒烟：`yu-cli run <project-eris> --frames 60` 正常进引擎渲染,exit 0;
  `sound_census`（装 SoundHost,真实语料）：BGM/B1 播 `bgm01.ogg`,
  `snd_diags=0`。
- `yu-rt` 单测 66/66;`tests/consistency.rs` 4 例因语料散盘目录
  `data/ysbin` 已不在（同树并行改动移除,封包 `data.ypf` 优先）而失败——
  与 B6 无关（skip 路径 `unreachable!` 为既有缺陷）,归 B5/工程侧。

## 8. 证据文件

- `workbench/tmp/snd-rea/0044e1e8.c`（SOUND 处理体）
- `workbench/tmp/snd-rea/0044f4bc.c`（SOUNDINFO）
- `workbench/tmp/snd-rea/0044f308.c`（SOUNDEND）
- `workbench/tmp/snd-rea/00464960.c`（位置/循环）
- `workbench/tmp/snd-rea/00423718.c`（淡变）
- `workbench/tmp/snd-rea/00424fbc.c` / `00423614.c` / `00424e4c.c`
- `workbench/tmp/snd-rea/decomp6.log`（REA 原始输出）
- `workbench/tmp/snd-rea/census2.log`（600 帧回归）
- B6 复核（2026-10-24,REA/Ghidra 12.1.4 会话内证据,ephemeral）：
  `0x422e04`/`0x423f9c`/`0x424424` 反编译（Evidence
  `ev_d3033121…`/`ev_f6ff2703…`/`ev_44fb7c6f…`）；MSACM32 IAT 地址表
  `workbench/tmp/snd-rea/acm_iat.json`；DATARIGHT 原始字节
  `0x44ff58..0x4500d0` 手工解码（PE 节表脚本,结论见 0-11④）
