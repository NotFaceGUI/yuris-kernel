# DIV-SND-006 · 0.500 的 Ogg Opus 支持

- 状态: **fixed(no-manual,2026-11-24;`<v500-relirium>` 取证)**
- 优先级: 高(0.500 商业族语音/BGM 全为 Opus,缺解码器即全程静音)
- 发现日期 / 方式: 2026-11-24 / 桌面实跑 + 封包资产普查
- 关联: DIV-SND-001、DIV-GFX-094

## 1. 现象与事实

- 官方 `<v500-relirium>` 封包里 **25,307 个 `.opus`**、859 个 `.ogg`、
  1 个 `.wav`;`pac/bgm.ypf` 的 `bgm/op_inst.opus` 头为
  `OggS ... OpusHead 01 02 38 01 80 BB 00 00`(48 kHz、pre-skip=312)。
- 官方 exe 的扩展名表(`0x1d95e0`,12 项):
  `.bmp .png .jpg .gif .ydg .webp .jxl .qoi .avif | .wav .ogg .opus`;
  旧表(`0x1d9660`)为 `.png .bmp ... | .ogg .wav .opus`。
  本作 `yscfg.sound_ext_order = [4,0,0,0]`,与「4 = Opus」一致。
- 旧内核的候选名只有 `(原名) → .ogg → .wav`,Opus 全部报
  `snd.file.missing`,桌面/Android 均无声。

## 2. 实现

- 引入纯 Rust 解码器 **`opus-pure 0.2.5`**(BSD-3-Clause;RFC 6716 / 7845 /
  8251),`yu-sound` 直接依赖,不新增系统库。
- `meta.rs`:
  - `ogg_is_opus`:首个 Ogg page 含 `OpusHead` 即按 Opus 处理;
  - `probe_opus`:`OpusHead` 取声道/pre-skip,尾页 granule 取总样本,
    `total_frames = granule - pre_skip`,采样率恒 **48 kHz**(RFC 7845 §4);
  - `CODEC` 仍报 **1 = OGG**(容器是 Ogg,手册 0=WAVE/1=OGG/2=ACM)。
- `decode.rs`:整段解码分支 `decode_opus`(`Trim` 处理 pre-skip/end-trim,
  `decode_s16` 与 libopus 的 `opus_decode` 同型先 soft-clip)。
- `stream.rs`:新增 `Kind::Opus`,与 Vorbis 相同的窗口/预取/seek 语义;
  `rewind()` + `decoder.reset_state()` + 重建 `Trim` 实现 RESTART/循环定位;
  `discard_before` 同时覆盖 Opus(长 BGM 不留整段 PCM)。
- `state.rs`:候选序按版本档切换——
  - 0.500/0.555(`SemanticProfile::is_v555_family()`):
    `(原名) → .wav → .ogg → .opus`;
  - 0.495 及更早:`(原名) → .ogg → .wav`(行为不变)。
  门控由引擎在构造 `SoundState` 后调用 `set_probe_opus` 设置,不按游戏名特化。
- 第三方许可:`THIRD-PARTY-NOTICES.md` 已由
  `tools/release/gen_third_party_notices.py` 重新生成(opus-pure 与
  image-webp 入表;BSD-3-Clause 全文在 `third_party/licenses/`)。

## 3. 验证

- `cargo test -p yu-sound --test state opus`(`YURIS_SAMPLE_RELIRIUM` 指向
  `<v500-relirium>`):
  - `probe_order_v555_appends_opus_last` — 候选序 `.wav → .ogg → .opus`;
  - `real_v500_opus_decodes_to_pcm` — `bgm/op_inst.opus` 解出
    10,974,546 帧 / 48 kHz / 立体声,与 granule 差 <100 ms;
  - `gated_v500_opus_loads_header_only_then_streams` — 装载只预取
    ≤2 s(不整段解码),`TOTALTIME` 精确,混音 2 s 输出非静音。
- `cargo test -p yu-sound` 全量通过(54 passed)。
- 桌面 release 实跑:标题 BGM 与按钮音效出声,日志不再出现
  `snd.file.missing`。

## 4. 残余

- `probe_opus` 只接受 mapping family 0(单/双声道);family 1/255 需要
  `OpusMSDecoder`,当前语料未出现,遇到会显式报解码失败(不猜)。
- 纯 Rust 解码比 libopus 慢(整段 3 MB 样本约 3.5 s);流式路径只预取
  1.5 s,不影响交互;超长 BGM 的定位代价与 Vorbis 同型(从头解码到 target)。
- `sound_ext_order` 的码值仍按语料/表位反推(4=Opus);`yscfg` 里的
  旧码 1/2 语义未变。
