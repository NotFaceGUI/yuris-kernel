# DIV-AND-003 · 音频交付层:AAudio 断流必须重建;模拟器音频不可作为听感判据

- 状态:`fixed(port)`(2026-11-01)
- 影响面:`yu-platform/host-android`、`yu-sound`(仅新增计数,语义不变)
- 关联:DIV-AND-002(跨平台硬编码收口)、`PROGRESS.md` 成果
  `AUDIO-ANDROID-DEVICE`

## 问题一(真 bug):换输出设备后永久静音

### 现象

插上耳机(或在系统里切输出设备)后 App **彻底没声音**,拔掉重插也不恢复;
重启 App 才恢复。

### 证据(真机 logcat 原文)

```text
D AudioStreamLegacy: onAudioDeviceUpdate(deviceId = 462)
D AudioStreamLegacy: onAudioDeviceUpdate() request DISCONNECT in data callback, device 3 => 462
D AAudioStream: setDisconnected setting disconnected, current disconnected: 0, current state: 4
W AudioStreamLegacy: onMoreData() data, stream disconnected
E AudioTrack: processAudioBuffer(461): EVENT_MORE_DATA requested 28288 bytes but callback returned -1 bytes
D AAudio  : AAudioStream_requestStop(s#1) called
```

AAudio 在音频路由变化时**主动断开并关闭流**(`ErrorDisconnected`),此后
回调不再触发。

### 原实现

cpal 的 `Stream` 错误回调里只有一行 `log::warn!`。流对象仍然存活,但永不
回调 ⇒ 表现为「突然静音且没有任何报错」,用户与日志都看不出原因。

### 处置

`crates/yu-platform/host-android/src/sound.rs` 增加**音频监督线程**
(`yu-audio-out`,口径对齐 `siglus_rs` 所用 kira 0.9 的 `StreamManager`):

1. 每 300ms 查询默认输出设备,取签名 `(name, rate, channels, format)`;
   签名变化或错误回调置位 ⇒ 丢弃旧流、重建新流、`ring.discard_all()`、
   更新共享的 `rate/channels`(mixer 每轮读取,不重启线程)。
2. **僵死看门狗**:累计回调数在 `300ms × 8 ≈ 2.4s` 内不增长 ⇒ 判定流僵死
   并强制重建(覆盖「不报错但不再回调」的边角情况)。
3. 建流在监督线程内完成,首次结果经 channel 同步回报给 `open()`,保持
   「设备打不开要显式失败」的既有语义。

## 问题二(不是我们的 bug):模拟器音频不可信

### 现象

同一份 APK、同一份 `data.ypf`、同一套 so:模拟器上持续「电流声 + 卡顿」,
真机上完全正常。

### 证据

| 指标 | 模拟器(x86_64) | 真机(arm64) |
|---|---|---|
| fps | 50~57 | **59.9** |
| `phases.readback` | 2232~5588 µs | **556~865 µs** |
| 宿主 `dev[short]`(环被抽干) | 0 | **0** |
| `snd[rtsil]`(源窗口缺口) | 0 | **0** |
| 听感 | 电流 + 卡顿 | **正常** |

即**交付侧两类欠载计数在两边都是 0**,唯一差异是听感 ⇒ 故障在模拟器的
虚拟 HAL / 主机音频转发,不在引擎、混音或解码。

### 交叉验证:我们产出的 PCM 是干净的

```text
引擎输出抓取(3.0s, 44100/2ch):
  rms=-12.8 dBFS  peak=0.0 dBFS  L/R corr=0.624
  spectral flatness=0.0000                    ← 不是噪声
  200–1000Hz 占 89.4%,主峰 441Hz + 谐波 880Hz ← 真实音乐
  相邻样本最大跳变 0.18 满幅;>5ms 静音段仅首段 1.54s(启动)

模拟器宿主回环(扬声器端点,6s):
  相邻样本最大跳变 0.019,孤立尖峰(>25×邻域)= 0
模拟器 AudioFlinger:
  Our track: SRate 44100, PCM_FLOAT, Underruns 0
  Normal mixer raw underrun counters: partial=0 empty=0
```

### 结论 / 纪律

- **听感判定只在真机做**;模拟器只用于功能与计数验证。
- 报告任何「爆音/卡顿」时,先看三个计数,归属一目了然:

  | 计数 | 归属 | 处置方向 |
  |---|---|---|
  | `clipped` | 混音叠加过量 | 查层音量/主音量结构 |
  | `rtsil` / `rtmiss` | 源解码窗口未就绪 | 查预取(LEAD_MS / 解码耗时) |
  | `short` | 环形缓冲被抽干 | 查线程调度/水位/宿主转发 |

  三者全 0 而仍听到杂音 ⇒ 故障在 `oboe` 之后(设备/驱动/模拟器转发)。

## 附带:采样率

真机与模拟器的 HAL 都是 48000Hz,我们请求 44100(与 OGG 源一致)⇒
AudioFlinger 会做一次 44.1→48k 重采样。这是 Android 常规路径,不是缺陷;
`audio_rate=48000` 可绕过,但那样会改用我们自己的线性插值(高频混叠、
质量更差),**默认不开**。
