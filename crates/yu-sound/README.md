# yu-sound · 声音域宿主

`SOUND` / `SOUNDINFO` / `SOUNDEND` 的状态机与 headless 混音。`yu-rt` 只发
`VmEvent::DomainCall`；本 crate 安装为 `SoundHost` 后同步消费并回答查询。

## 证据等级

| 等级 | 内容 |
|---|---|
| **Confirmed** | 层 ID 键、文件名 ASCII 小写 + 探测序（无扩展名→`.ogg`→`.wav`，已带后缀不追加）、`SOUNDINFO[FILE]` 返回最终候选名、E/PLAY/PAUSE/VOLUME/PAN/PITCH/LOOP/START/RESTART/END/FADE、LOOPCOUNT 在 END 边界 +1、LOOP=N 停止并清零、TIME/TOTALTIME、CODEC/FILETYPE、淡变线性、音量振幅 `(v/256)^2`、PLAY→PAUSE 顺序 |
| **Likely** | 默认音量 256 |
| **Unverified** | 线性重采样插值、最终限幅（代码处标注） |
| **Unimplemented** | 负 START/RESTART 延迟、ACM WAVE、MIDI/MIDIEND/MIDIINFO/CDDA、DATALEFT/DATARIGHT（REA 已取证，缺手册层）、SOUND[SPEED=] |

逐条地址与公式见 [`docs/sound-ledger.md`](../../docs/sound-ledger.md)。

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
  `yu-sound` 不碰文件系统、不认识 ypf。
- 状态机只由 `SoundHost::advance(now_ms)` 推进；同一时刻重复调用无副作用。
- 设备输出在 `yu-cli`（默认 `device` feature；`--no-default-features` 关闭）：
  主线程每帧 `advance → render` 写入环形缓冲，cpal 回调只消费样本
  （欠载补 0），状态机不跨线程、不反哺。
- `SOUNDINFO` 多字段同调：`EXIST..PITCH` 按固定顺序首个命中即返回；不含这
  7 个字段时其余字段连续写 `LET`，最后者胜出。
- 未装宿主时 `yu-rt` 行为与 M-2 逐位一致（600 帧普查 531/7,200/1）。

## 依赖

| crate | 许可 | 理由 |
|---|---|---|
| `hound` 3 | Apache-2.0 | RIFF WAVE PCM 解析；纯 Rust、无系统库，wasm 目标可用 |
| `lewton` 0.10 | MIT/Apache-2.0 | OGG Vorbis 解码；纯 Rust，与官方 vorbisfile 语义对应 |

两者均为 GPL-3.0-or-later 兼容的宽松许可；不引入 ACM/MCI 系统依赖。

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
