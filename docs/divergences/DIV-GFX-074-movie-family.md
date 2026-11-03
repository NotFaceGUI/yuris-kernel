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
多字段同时置位时**槽号最小的字段生效**;一个字段都没给则什么都不写。

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

## 4. 后端选型(待定:不影响内核,换后端不改语义)

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

将来要支持 AVI/MP4 等,再加一个 `MovieHost` 实现(FFmpeg 可作可选 feature),
内核与端口不变。

## 5. 限制(显式)

- 后端只解 **MPEG-1 program stream**;`MOVIEINFO[CODEC]` 为 `2`/`0xFF` 的
  影片(MPEG-2、AVI、MP4…)会被内核诊断说明,不假装播放(官方对 `>1` 也只是
  警告「可能播不了」)。
- **音频未接**:pl_mpeg 能解 MP2,但 PCM → `yu-sound` 的注入通道属于声音域
  接线(下一批);`MOVIE[VOLUME]` 的语义值已保存。
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
- **存档容器版本兜底**:`SAVE` 没有可参照 `.sd` 时,引擎用工程 `yscfg`
  版本(`vm.tables().boot.config.version`)初始化容器版本,不再以
  `sv.container` 拒绝写盘;盘上同名文件仍优先沿用其自身版本。
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
- **待办**:`\MOVIE(...)` 宏路径的 es 影片态调度(roadmap-batches B14
  「待办」);脚本侧绕行为「`system_start` 前置播片」或「`MOVIEEND` 后
  `\UI.RELOAD` 重建按钮组」。
