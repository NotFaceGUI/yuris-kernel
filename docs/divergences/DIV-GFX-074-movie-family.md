# DIV-GFX-074 · `MOVIE` 族(全屏影片):内核语义 + 跨平台播放端口

> 状态:**语义层 fixed(no-manual→手册有页)**;播放后端**待主人选型**
> (2026-11-03,B14 续)。手册页:`movie/movie.html`、`movie/movieend.html`、
> `movie/movieinfo.html`(0.495 SDK `マニュアル/YU-RIS/html/movie/`)。

## 1. 结论

`MOVIE` 与 `YMV` 是两套东西,别混:

| | `YMV`(已落地,DIV-GFX-073) | `MOVIE`(本文) |
|---|---|---|
| 形态 | **普通图层**上的帧动画 | **全屏画面**,不能当图层,盖在所有层之上 |
| 数据 | `YSMV` 容器(逐帧双 JPEG) | MPG/AVI 等**系统媒体栈能播的任意格式** |
| 解码 | 内核自解(`yu-render-core`) | **交给系统媒体栈**(官方 MCI/COM) |
| 帧驱动 | 脚本按 `YMVINFO[FPS]` 推进 | 播放器自己走时钟 |
| 模式 | 无 | **影片模式**:`MOVIE` 后所有层不显示,直到 `MOVIEEND` |

## 2. 官方语义(REA 0.495,逐段锚点)

处理体:`MOVIE`=`0x44b134`、`MOVIEEND`=`0x44b9d8`、`MOVIEINFO`=`0x44ba2c`
(分发表 `FUN_0045cf90` 槽 71/72/73;`MOVIEINFO` 参数序
`PLAY,X,Y,SX,SY,VOLUME,CODEC,LET`)。

### 2.1 `MOVIE[FILE, PLAY, X, Y, SX, SY, VOLUME]`

1. **几何先落全局**(`0x44b2b0..0x44b2fc`),**缺省也覆盖**:
   - `X` 缺省 → `0x801764 = 0`,`Y` 缺省 → `0x801768 = 0`;
   - `SX` 缺省 → `0x80176c = 影片原始宽`,`SY` 缺省 → `0x801770 = 影片原始高`;
   - 播放时再按窗口等比换算(`0x44b418`:`目标宽 = SX * 窗口宽 / 影片宽`)
     ⇒ 缺省 SX/SY 等价于**铺满窗口**(手册例 1「ウィンドウいっぱい」)。
2. **FILE**:`fopen` 成功才把路径拷进 `0x7fab00` 并清播放位
   (`0x44b302..0x44b322`);失败则**原状返回**(不进影片模式)。
3. **VOLUME**:存 `0x5b48a0`,并下发播放器(见 2.4)。
4. **PLAY**:`1` = 开始(置 `0x801760 = 1`),`0` = 停止并清播放位。

### 2.2 `MOVIEEND[]`

清文件名(`0x7fab00`)、播放位(`0x801760`),**音量重置 256**(`0x44ba0f`);
几何(X/Y/SX/SY)**不清**。停止后仍处于影片模式(手册例 3 明示:
「停止しても MOVIEEND するまで他のレイヤは一切表示されない」)。

### 2.3 `MOVIEINFO[PLAY, X, Y, SX, SY, VOLUME, CODEC, LET]`

七个字段是**顺序 `if` + 各自写 `LET` + `ret`**(`0x44ba60..0x44bbde`):
多字段同时给出时**槽号最小的字段生效**;一个字段都没给则什么都不写。
字段选择看参数**存在标志**(`0x684160..0x684166` 逐字节 `test/jne`),
不看数值是否非零——`MOVIEINFO[PLAY=0 LET=@a]` 同样应写回 0。

`CODEC`(`0x44bac9..0x44bcfc`)只读文件头 5 字节:

```text
打不开文件                      → 0
前 4 字节 != 00 00 01 BA        → 0xFF
第 5 字节 & 0xF0 == 0x20        → 1     (MPEG-1 program stream)
第 5 字节 & 0xC0 == 0x40        → 2     (MPEG-2 program stream)
其余                            → 0xFF
```

证据:官方样例 `data/pac/op_movie.mpg` 头 = `00 00 01 BA 21` → 1。
ERIS 脚本据此分流(`es_MoviePlay.yst`:`==0` 文件不存在、`>1` 非 MPEG-1)。

**2026-11-11 勘误(探测对象)**:官方 `CODEC` 探测的是 **`CODEC` 参数本身**,
不是 `MOVIE[FILE]` 的当前文件名:

```text
44bac9  mov ecx,[edx*4+0x683fc0]   ; CODEC 参数字符串
44bad4  mov edi,0x681300           ; CWD/工程基路径
44bad9  …把基路径拷到栈缓冲…
44baf3  …把参数串追加到基路径尾部…
44bb42  call 0x469df2              ; fopen(基路径 + CODEC 参数)
```

`MOVIE[FILE]` 的文件名在 `0x7fab00`,本分支不读。ERIS `es_MoviePlay.yst`
正是在 `MOVIE[FILE]` **之前**用 `MOVIEINFO[CODEC=$FN]` 做存在性/编码预检;
旧内核读 `self.movie.file`(此时为空)恒得 0,宏路径随即
`IF[@codeccheck==0] IFBREAK[LV=3]` 放弃播放。内核 VFS 根即官方基路径,
实现改为探测参数文件。回归:`yu-rt/tests/fixture.rs`
`movie_state_machine_and_info_queries`(CODEC 查询在 `MOVIE[FILE]` 之前)。

### 2.4 官方把解码/呈现交给系统媒体栈

- 导入表:`winmm.dll` 的 `mciSendStringA`;运行时字符串
  `" ysmcimovie volume to %d"`、设备类型 `MPEGVideo`(别名 `ysmcimovie`);
- 同时有 COM 路径:`0x44b664` 对 `[0x801d34]` 调 `[ecx+0x1c]`
  (音量按 1/100 dB,即 `IBasicAudio::put_Volume`),`[0x5b4874]` 是
  「COM / MCI」选择开关;
- 手册原文:「メディアプレイヤーで再生できる全てのムービーファイル
  (MPG,AVI等)に対応」⇒ 格式支持**取决于系统装了什么解码器**。

## 3. 内核实现(分层:内核只有语义)

| 层 | 内容 |
|---|---|
| `yu-format::movie` | `probe_codec(head)`:上述 5 字节规则 → `MovieCodec{Missing,Mpeg1,Mpeg2,Other}`,`official_code()` 给 0/1/2/255 |
| `yu-rt`(VM) | `MovieState{file,playing,geometry,volume}`(逐字段对应官方全局);`MOVIE`/`MOVIEEND`/`MOVIEINFO` 处理器;`Vm::movie_mode()`(非空文件名 = 影片模式)与 `Vm::movie_state()` |
| `yu-rt::event::MovieHost` | **跨平台播放端口**:`open/set_playing/set_volume/set_geometry/is_playing/close`;**内核不含任何平台 API** |
| `yu-gfx` | 不参与(`MOVIE*` 事件直接忽略;它不是图层) |

`MOVIEINFO[SX/SY]` 在「该命令未给 SX/SY」时内核返回 **0**(= 未指定);
官方此时返回影片原始宽高(它在播放时已经知道影片尺寸)。差异仅在
「不指定 SX/SY 又立刻查询 SX/SY」时可见,语料无此用法(`es_MoviePlay`
只查 `PLAY` 与 `CODEC`)。宿主按 `sx/sy == 0` 换算为铺满。

## 4. 后端选型(2026-11-17 扩展:内置 + 外置可选)

`MovieHost` 只描述"要什么",不规定"怎么解"。可行后端:

| 后端 | 覆盖 | 代价 | 备注 |
|---|---|---|---|
| 平台系统栈 | Windows(MCI/MF)、Android(MediaPlayer/MediaCodec) | 每平台一份实现 | **与官方同构**(官方就是系统媒体栈),格式支持同样取决于系统 |
| FFmpeg(`ffmpeg-next` 等) | 桌面 + Android 一套代码 | 构建依赖(需预编译库/交叉编译) | 格式最全;建议做成**可选 feature(默认关)**,不破坏离线 `cargo build` |
| 纯 Rust | — | — | MPEG-1/AVI 没有成熟解码器,不可行 |

**已选型(2026-11-03,主人拍板):内嵌 `southstar-codecs` 的 `plmpeg` 模块**
(pl_mpeg 的纯 Rust 移植,MIT 血统;crates.io 正式依赖,`#![forbid(unsafe_code)]`)。

理由:业界对「只播 MPEG-1」的惯用解法是小型专用解码器(ONScripter 的
SMPEG/libmpeg2、pl_mpeg),而不是 10–80MB 的 FFmpeg;官方 `CODEC` 探测本身
也只认 MPEG-1/2。实测:官方样例 `op_movie.mpg` → 60 帧 / 800×600 / 30fps /
1.933s,与 `ffprobe` 完全一致(逐帧解码 + YUV→RGBA 后颜色数 >64)。

实现:`crates/yu-platform/movie-plmpeg`(`PlmMovieHost` 实现 [`MovieHost`]);
引擎每 tick `sync_movie_overlay`:取帧 → `TextureStore::insert_rgba` 注入
合成资产 `:movie/current` → `GfxState::set_movie_overlay`(影片模式只投影
「清屏 + 影片层」)。显示尺寸按官方 `0x44b418`:`目标宽 = SX * 窗口宽 /
影片宽`,缺省 `SX = 影片宽` ⇒ 铺满。

**2026-11-17 扩展**:AVI/MP4 等不再等“换掉 pl_mpeg”,而是加**可选的
外置后端**。内核新增跨平台设置 `movie_backend`(`auto|builtin|external`)
与 `MovieHostFactory` 注入口;`auto` 先试宿主注入的外置后端,`open` 失败
回退内置并记 info,`external` 不静默回退。Android 已实现系统后端
(MediaExtractor + MediaCodec ByteBuffer YUV),见 §10;桌面当前没有注入
外置工厂,显式 `external` 会得到可恢复诊断。`MOVIEINFO[CODEC]` 的官方
0/1/2/255 语义不变(MP4/AVI 仍报 0xFF,与官方一致)。

## 5. 限制(显式)

- **内置**后端只解 **MPEG-1 program stream**;`MOVIEINFO[CODEC]` 为
  `2`/`0xFF` 的影片(MPEG-2、AVI、MP4…)需要外置后端。Android 外置的
  容器/编码支持取决于设备的 MediaExtractor/MediaCodec(官方保证格式表
  不含 MPEG-PS;AVI 不保证);不支持时 `auto` 回退内置,`external` 报
  `movie.decode` 诊断。
- 音频已接(B16):内置后端解 MP2;外置后端解设备支持的音轨并降混为
  立体声 i16。音频解码失败只降级为无音频,不阻断视频。
- 影片模式下「遮住所有图层」由 `GfxState::set_movie_overlay` 实现(整帧只
  投影影片层),宿主负责每 tick 注入当前帧。
- 影片模式下"遮住所有图层"是**宿主呈现责任**:宿主读 `Vm::movie_mode()`,
  为真时只呈现影片画面。当前 `yu-engine` 尚未接线(等后端选型一起做)。
- `CODEC` 只认 MPEG-PS;AVI 等一律 `0xFF`(与官方一致,不是缺陷)。

## 6. 复核

```text
cargo test -p yu-format --lib movie          # 5 通过(含官方 op_movie.mpg → Mpeg1)
cargo test -p yu-rt --test fixture movie_    # 3 通过(状态机/查询/端口契约/缺文件)
cargo test -p yu-movie-plmpeg                # 2 通过(官方样例解码 60 帧 800×600;
                                             #   暂停保留帧/close 清空/非 MPEG-1 显式失败)
cargo test -p yu-gfx --test ymv_movie        # 9 通过(含 overlay 遮住所有层)
```

## 6. 实机验收与审查修正(2026-11-03)

- **wgpu 资产纹理必须按内容版本失效**:`TextureCache::ensure_asset` 原先只按
  key 缓存 `TextureId`,宿主每帧 `insert_rgba(:movie/current)` 换帧后 GPU 仍
  复用第一张 ⇒ soft 会动、wgpu 静止。现按
  `TextureStore::generation`(`insert_cached` 推进、`touch` 不动)比较:内容
  变了就 `write_texture` 就地覆写同一张 GPU 纹理;尺寸变化时旧纹理退为
  transient(由 `clear_transient` 回收并 `forget_texture`)。
  **不用 `Rc::as_ptr`**:旧 `Rc` 释放后新分配可能复用地址,会把换帧误判为
  未变。
- **`asset.size.unsupported` 移除的依据**:手册 `cg/cgact.html` 例 3
  「`CGACT[ID=BG SIZE=1 SX=315 SY=190]` … 横315、縦190に拡大縮小」——
  `SIZE/SIZE2` 的目标尺寸就是缩放绘制。`CG[SX/SY]` 另属图层组参数
  (`cg/cg.html`「※レイヤセットに対してのみ有効」),gfx 的
  `apply_layer_params` 已按 `is_group` 分派,普通图像层不带 `image_size`。
- **存档容器版本兜底(2026-11-15 勘误)**:当时用工程 `yscfg` 版本
  (`vm.tables().boot.config.version`)兜底是**错的**——容器版本是官方 exe
  的写盘器常量(0.495=`0x1E0`、0.479=`0x138`、0.466=`0x0C0`、0.481=`0x1E1`),
  与 yscfg 版本无换算关系,写错会让官方拒收存档。现行实现见
  `docs/divergences/DIV-RT-058-save-container-version.md`(官方 exe 探测
  > 盘上学习 > 目标文件 > 压缩层默认)。
- **既有缺口(另案)**:按钮条/存档缩略图依赖 `CGACT[COPY]` 的
  `screen_snapshots`;实机日志出现 `snapshot.unsupported x3` 与
  `复制源 :/W0/SLTEMP 不存在`,属快照域未完成的物化路径,与 `MOVIE` 无关。

## 7. 审查记录(2026-11-03)

- **`layer_predict_drawable` 与渲染不一致(已修)**:soft 的整帧可绘制
  预测仍把 `Asset + 显式 SX/SY` 判为不可绘制,而渲染分支已按
  `CGACT[SIZE]` 缩放绘制(手册 `cgact.html` 例 3);两者矛盾会让整帧
  被判"无输出"。改为 `Asset`/`Movie` 一律可绘制。
- **wgpu 资产缓存失效**:按 `TextureStore::generation` 比较(见 §6);
  `inserted` 表随 `evict`/`remove` 同步清理;尺寸变化的旧纹理摘掉
  `asset_texture_ids` 后成为 transient,由 `clear_transient` 回收并
  `forget_texture`,不留悬空 view。
- **清理**:临时 `DBG` 探针全部移除;`src/` 下无 `eprintln!/println!`,
  stderr 输出只保留在 examples。
- **`\MOVIE(...)` 宏路径(已修,2026-11-11)**:两处根因与修法见 §8;
  不再需要脚本侧绕行(前置播片 / `MOVIEEND` 后 `\UI.RELOAD`)。

## 8. `\MOVIE(...)` 宏路径收口(2026-11-11)

真实 MOV 工程 `target/tmp/movie-clean`(官方 SDK 0.495 编译,
`SCENARIO_START → SCENARIO_LOGO → SCENARIO_OPMOVIE → \MOVIE(pac/op_movie.mpg)`)
逐 tick 任务相位 + 画面验收:

- 调度链本身正常:场景任务(Z=2)在 `WAIT[FRAME=1]` 后,系统任务(Z=100)
  下一逻辑帧命中 `ES.GAMEMAIN.LOOP` 的 `SCENARIOSTOP==@e.MOVIE` 并
  `es.S.GOSUB("es.MOVIEPLAY")`;影片期间场景任务被 `_task.pause` 置 `E=0`,
  不可能先跑 `\TITLE`。**不改调度器/WAIT 语义**。
- **断点 1:`MOVIEINFO[CODEC]` 探测错对象**。官方 `0x44bac9` 读 `CODEC`
  参数并在其后追加到基路径 `0x681300` 后 `fopen`;旧实现读
  `MOVIE[FILE]` 的当前文件名(`0x7fab00`),此时为空 ⇒ 恒 0 ⇒
  `es_MoviePlay.yst` 的 `IF[@codeccheck==0] IFBREAK[LV=3]` 直接放弃播片。
  已改为探测 `CODEC` 参数文件;字段选择也改为按参数**存在**而非值非零。
  回归:`yu-rt/tests/fixture.rs::movie_state_machine_and_info_queries`
  (CODEC 查询移到 `MOVIE[FILE]` 之前)。
- **断点 2:`MOVIEEND` 后底部按钮条/UI 快照不恢复**。影片帧
  (`movie_only_frame`)只投影影片层,后端 `prune_snapshots` 把按钮快照
  全部回收;`GfxState.baked_copies` 仍认为 COPY 已冻结,退出影片模式后
  `frame()` 继续跳过重发 ⇒ `BT.OFF...GAMEMAIN.BTN.*`(快照纹理)永久黑,
  只剩 SAVE(资产纹理)。已改为 `set_movie_overlay(None)` 时清空
  `baked_copies`,下一帧重发一次 COPY;后端按 `source_texture`(按钮
  sprite sheet + `COPY2` 区域)重建快照。回归:
  `yu-gfx/tests/ymv_movie.rs::movie_overlay_exit_replays_baked_copy_snapshots`。

实机(桌面 wgpu,`movie-clean`):frame 200 为 OP 影片实际帧,frame 500
为标题 UI;debug 模式进入主菜单时底部 15 个按钮(セーブ…メニュー)与官方
截图逐项一致。`target/tmp/movie-fix-*.png`、`shot-f1100-soft.png` 为证据。

## 9. 影片音频(2026-11-11,B16)

官方音频由系统媒体栈与画面一起播放:COM 路径
`IBasicAudio::put_Volume`(`0x44b664`,`[ecx+0x1c]`)把 `MOVIE[VOLUME]`
0..256 映射为 `round(4000*log10(n/256))`(1/100 dB),DirectSound 振幅
即 `(n/256)^2`;MCI 回退 `0x44b7cc` 是 `volume to 1000*n/256`(线性)。
`MOVIEEND` 重置音量 256(`0x44ba0f`)。

实现分层:

- `MovieHost::take_audio`(默认 `None`)交付交错 i16 PCM;
  `PlmMovieHost` 用 pl_mpeg 解 MP2(48/44.1/32kHz,立体声),每 tick
  预解码到 `target + 200ms`(最多 32 包),与视频共用 demux。
- `yu-sound::movie::MovieAudio` 环形缓冲(2^18 样本 ≈ 2.7s@48k),
  设备回调内只做线性插值 + `volume_gain`(COM/DSBVOLUME 曲线)+ 加法,
  零分配/零解码。
- `Engine::sync_movie_audio` 每 tick 交付,换片/`MOVIEEND` 清队列;
  图形状态与音频解耦(`render=false` 也推进影片时钟与音频)。
- 平台只经既有 `AudioOut`:桌面 cpal(`sound_device.rs`),Android
  cpal→oboe(`sound.rs`,mixer 线程→无锁 SPSC 环→回调);无 Android
  专属改动,`arm64-v8a + x86_64` APK 已交叉编译通过。

回归:`yu-sound::movie` 5 项(归一化/音量 0/重采样/欠载/清队列)、
`yu-movie-plmpeg::decodes_mp2_audio_track`(真实 48kHz MP2)。
微基准:MP2 解码 73.3ns/帧(60Hz tick ≈0.059ms);push 0.0006ms/tick;
render 0.0012ms/960 帧块。

已知限制:线性重采样(与 `mixer` 同口径,非系统重采样器);只支持
MPEG-1 PS/MP2;`MOVIE[PLAY=0]` 的官方 `stop` 位置语义未对齐(语料
无 `PLAY=0`);视频用逻辑帧时钟、音频用设备实时时钟,<60fps 时轻微
漂移;整段影片字节仍由 VM + 解码器各持一份。

任务书与完整审查:`docs/tasks/task-b16-movie-audio.md`。

## 10. 外置解码后端(2026-11-17)

### 10.1 选择与回退

- 内核:`yu_rt::event::MovieBackend`(`auto|builtin|external`)+
  `MovieHostFactory`;`yu-engine::movie` 的 `MovieHostSelector` 负责分派。
  `auto` 外置 `open` 失败 → 内置 pl_mpeg(记 info);`external` 失败不
  回退;宿主未注入工厂时 `external` 返回 `movie.decode` 可恢复诊断。
  另:`auto` 对 **MPEG-1 program stream**(`yu_format::movie::probe_codec`
  的 5 字节规则)直接走内置,不落临时文件、不试外置 —— Android 不保证
  MPEG-PS,这是官方 `CODEC=1` 的主路径。
- 配置入口:CLI `--movie-backend`;Android JSON `movie_backend` + 配置
  界面“视频解码”三档(自动/内置/外置)。缺省 `auto`。

### 10.2 Android 实现(`yu-movie-android`)

```text
MOVIE[FILE] bytes
  → cache_dir/yu-movie-<pid>-<n>.tmp(fd)
  → AMediaExtractor:选 video/* 与可选 audio/*
  → AMediaCodec:
       视频 ByteBuffer(不传 Surface;线性 YUV)
       音频 ByteBuffer
  → 输出格式 color-format/stride/slice-height/crop → 安全转换 → RGBA
  → PCM i16 → 立体声降混
  → MovieHost 端口(与内置后端同一条内核路径)
```

选型过程(真机取证):最初按 Android 文档走 `AImageReader`(Surface)
取原始帧,在高通设备上 `AImage_getFormat=0x23 / planes=3`,但
`AImage_getPlaneData(plane1)` 的 data 指针为 null(底层实际是 UBWC
`0x7fa30c06`),U/V 平面无法 CPU 读取,画面全黑;`AImageReader_newWithUsage`
(CPU_READ_OFTEN)也被厂商格式覆写。改为 **ByteBuffer 视频输出**后,
实测 `color-format=0x15`(NV12)、`stride=1280/slice=720`,
`yuv420_buffer_to_rgba` 按输出格式转换,真机 26.7s H.264+AAC 正常;
`mv[dropped=0 under=0]`。`flexible`/未知厂商格式先按 NV12,长度不够再按
I420。设备能力探测:逐文件 `createDecoderByType` 判定外置是否可用;
设备清单由 `AMediaCodec_createDecoderByType`(引擎侧
`nativeMovieCodecs`)与 Java `MediaCodecList`(硬件/软件标记)合并展示。
NDK `AMediaCodecInfo` 的细粒度查询要 API 36,因此不作主路径。

时序:同步模式、每 tick 非阻塞(`timeout=0`);每 tick 最多喂 24 个样本、
解复用提前 0.35s、视频排空 8 帧、音频排空 16 包并提前 0.2s 交付;
帧队列按 PTS 升序,只显示 `pts <= target` 的最新帧。暂停冻结进度,
恢复从暂停点继续(`start = now - position`),不复刻 pl_mpeg 的
`start_ms` 重置问题。

资源:codec/extractor/临时文件全部 RAII;`open` 中途失败由局部守卫释放。
ByteBuffer 每个输出样本在 `releaseOutputBuffer(..., false)` 前完成转换;
`AMediaFormat` 在 `AMediaCodec_getOutputFormat`/`getTrackFormat` 后立即
delete。临时文件在 `close`/Drop 删除。`libmediandk` 由 `ndk-sys` 的
`media` feature 显式链接(否则只有未定义符号、没有 DT_NEEDED)。

### 10.3 验证与限制

- `cargo test -p yu-engine --lib movie`:8 项(自动回退/优先外置/
  MPEG-1 PS 直走内置/显式外置不回退/无工厂诊断/pending geometry-volume/
  内置忽略外置)。
- `cargo test -p yu-movie-android --lib`:14 项(I420/NV12/NV21 等价、
  crop offset、BT.601 端点、单声道/5.1 降混、ByteBuffer I420/NV12/
  flexible/crop/短缓冲)。
- `YURIS_REQUIRE_SAMPLES=1 cargo test -p yu-movie-plmpeg`:3 项,含官方
  `op_movie.mpg` 与真实 MP2 48kHz 音轨。
- `movie-clean`(官方 0.495 编译,soft):auto 320 帧 display JSON 为
  单层 `:/MOVIE` + `:movie/current`,与 builtin 逐字节一致;500 帧
  回到标题 UI;显式 external 在 `es_movieplay.yst pc43` 报
  `movie.decode` 后回到标题、不挂死。
- **真机(Android 15,arm64)**:内置 `trance_Ignis.mpg`(MPEG-1
  1280×720 + MP2 48k,26.7s)画面/声音正常;外置同片转 H.264+AAC
  (1280×720/30fps + AAC 48k)正常,日志
  `color-format=0x15 stride=1280 slice=720 crop=1280x720+0,0`,
  `mv[pushed=1283072 played=1174824 dropped=0 under=0]`,
  `snd[clipped=0]`。首次外置黑屏的根因是 ImageReader/UBWC,见 §10.2。
- `YURIS_SO_ONLY=1 tools/android/build.sh arm64-v8a`:交叉编译通过;
  `llvm-nm` 可见 `Java_..._nativeMovieCodecs`,
  `llvm-readelf -d` 可见 `libmediandk.so`。
- `BUILD_TYPE=debug tools/android/build.sh arm64-v8a`:Gradle 打包成功
  (`app-debug.apk`,40MB;含链接 `libmediandk` 后的 release so)。
- 未做:AVI/MPEG-2 等其余格式的真机验证;桌面外置后端;
  `MOVIE[PLAY=0]` 官方 `stop` 位置语义。
- 已知近似:外置颜色转换固定 **BT.601 limited range**(与内置 pl_mpeg
  同口径);H.264/MP4 常见 BT.709,画面可能有轻微色偏。后续可按
  `color-standard`/`color-range` 选择矩阵,当前不猜。
