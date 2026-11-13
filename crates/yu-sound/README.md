# yu-sound · 声音域宿主

`SOUND` / `SOUNDINFO` / `SOUNDEND` 的状态机与 headless 混音。`yu-rt` 只发
`VmEvent::DomainCall`；本 crate 安装为 `SoundHost` 后同步消费并回答查询。

## 证据等级

| 等级 | 内容 |
|---|---|
| **Confirmed** | 层 ID 键、文件名 ASCII 小写 + 探测序（无扩展名→`.ogg`→`.wav`，已带后缀不追加）、`SOUNDINFO[FILE]` 返回最终候选名、E/PLAY/PAUSE/VOLUME/PAN/PITCH/LOOP/START/RESTART/END/FADE、LOOPCOUNT 在 END 边界 +1、LOOP=N 停止并清零、TIME/TOTALTIME、CODEC/FILETYPE、淡变线性、音量振幅 `(v/256)^2`、PLAY→PAUSE 顺序、`DATALEFT/DATARIGHT` 帧采样（REA `0x44fe3c`/`0x44ff58`，手册无页）、`CODEC=2` 非 PCM WAVE 走 win32 ACM 转换链（REA `0x422e04` else 分支） |
| **Likely** | 默认音量 256 |
| **Unverified** | 线性重采样插值、最终限幅（代码处标注） |
| **aligned-stub** | MIDI / MIDIEND / MIDIINFO（官方 `0x44aa40/60/80` 处理体本身是空桩）、CDDA |
| **unsupported-by-official** | `SOUND[SPEED=]`（yscm 槽 13，官方 SOUND 处理体只读不消费） |

负 START/RESTART 已实现静音区间 + TIME 冻结态，动态样本到位后再校准精确
时长（DIV-SND-002）。逐条地址与公式见 [`docs/sound-ledger.md`](../../docs/sound-ledger.md)。

## 用法

```rust
use std::rc::Rc;
use std::cell::RefCell;
use yu_sound::{SoundHost, SoundState, loader::SoundLoader};

# struct MyLoader;
# impl SoundLoader for MyLoader {
#     fn load(&self, _: &[u8]) -> Result<Option<Vec<u8>>, yu_log::Diag> { Ok(None) }
# }
let state = Rc::new(RefCell::new(SoundState::new(Box::new(MyLoader))));
vm.set_sound_host(Box::new(SoundHost::new(state.clone(), vm.tables().boot.yscm.clone())));
```

- 装载器由装配层注入（`yu-cli` 用工程 `DataSource`，含 `data.ypf`）；
  `yu-sound` 不碰文件系统、不认识 ypf，也不认识设备。
- 本 crate 只提供 `render_and_advance(out, rate, channels)`;设备输出归宿主:
  桌面 `host-desktop` 的 `SoundSink` 在 cpal 回调里**直接锁 `SoundState`
  渲染**（音频线程即时钟,按实际输出帧数推进播放头,UI 线程阻塞不影响声音）;
  Android `host-android` 是 mixer 线程预混 → 无锁 SPSC 环 → oboe 实时回调
  （回调零锁零分配,欠载补静音）。
- 状态机由宿主的渲染调用推进;`advance(now_ms)` 在外部时钟模式下只更新时钟。
- `SOUNDINFO` 多字段同调：`EXIST..PITCH` 按固定顺序首个命中即返回；不含这
  7 个字段时其余字段连续写 `LET`，最后者胜出。
- 未装宿主时 `yu-rt` 行为与 M-2 逐位一致（600 帧普查 531/7,200/1）。

## 影片音频

`MOVIE` 的 MP2 音轨也汇入本 crate：`MovieHost::take_audio` 交付交错 PCM，
`movie` 模块用环形缓冲 + 线性重采样 + 官方 COM 音量曲线混音，桌面 cpal 与
Android cpal→oboe 同一路径。影片视频时钟走逻辑帧、音频走设备实时时钟，
两者漂移与 `MOVIE[PLAY=0]` 的剩余项见 `docs/todo-inventory.md` §3.3。

## 依赖

| crate | 许可 | 理由 |
|---|---|---|
| `hound` 3 | Apache-2.0 | RIFF WAVE PCM 解析；纯 Rust、无系统库，wasm 目标可用 |
| `lewton` 0.10 | MIT/Apache-2.0 | OGG Vorbis 解码；纯 Rust，与官方 vorbisfile 语义对应 |

两者均为 GPL-3.0-or-later 兼容的宽松许可。ACM WAVE 不用第三方库：Windows
下直接调 `msacm32` 的 7 个导入（`acmFormatSuggest` → `acmStreamOpen` →
`acmStreamSize` → `acmStreamConvert`），非 Windows 目标按 `cfg(windows)`
门控，不引入系统依赖。

## 测试

```bash
YURIS_SAMPLE_SDK0495=<sdk495> cargo test -p yu-sound
```

OGG 用例在样本缺失时显式 `SKIP` 并打印原因，不空跑通过。
`tests/golden/loop_timeline.txt` 是循环状态时间线 + render 哈希的 golden。

真实语料（需 `YURIS_SAMPLE_ERIS_ROOT`）：

```bash
# 600 帧带渲染：确认 BGM 装载/解码/推进
cargo run -p yu-cli --example sound_census -- 600
# VM-only 6000 帧：跨 90.8s END 边界看 LOOPCOUNT/TIME
cargo run -p yu-cli --example sound_loop_census -- 6000
```
