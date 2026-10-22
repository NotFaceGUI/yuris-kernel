# DIV-SND-002 · 负 START/RESTART（延迟播放）未实现

状态：**open（Unimplemented）**

## 现象

手册 `sound/sound.html`：`START`/`RESTART` 为负值时是“遅延再生”。
0.495 处理体把负值交给 `0x423f9c`（OGG）/`0x424424`（WAVE）：

- `abs(param)*bytes_per_sec/1000` 累加到 64 位延迟累加器 `+0x190/+0x194`；
- 按 `block align` 取模；
- 解码器 seek 的实参被置 0。

这证明“延迟”是**按字节累加的静音区间**，但延迟期间
`SOUNDINFO[TIME]`/`PLAY` 的返回值没有动态样本；`+0x190` 的消费点在
混音线程里未逐行取证。

## 裁决

`yu-sound` 接受参数并把负值记为 `snd.delay.unimplemented`（Severity
Unimplemented），**不排延迟、不推进位置**。正 START/RESTART 已实现并有
单测（手册例 3/4）。

## 解除条件

取得官方引擎延迟播放期间逐帧 `TIME/PLAY/LOOPCOUNT` 的曲线
（`DIV-SND-003` 的复现步骤），或在混音线程定位 `+0x190` 的消费点。
