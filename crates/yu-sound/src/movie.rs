//! 影片音频混音:MP2 解码后的交错 PCM → 设备回调零分配混音。
//!
//! # 语义
//!
//! - 官方 `MOVIE[VOLUME=n]` 的 COM 路径(`0x44b664` → `IBasicAudio::put_Volume`)
//!   把 0..256 映射为 `4000*log10(n/256)`(1/100 dB),DirectSound 振幅即
//!   `(n/256)^2`;MCI 路径(`0x44b7cc`)是 `volume to 1000*n/256`(线性)。
//!   本内核与声音域一致采用 COM/DirectSound 曲线(`volume_gain`)。
//! - 影片音频是独立媒体流(官方 MCI/COM),不与 SOUND 层树共享音量;
//!   脚本 `es_MoviePlay.yst` 已把 BGM 主音量折进 `MOVIE[VOLUME]`。
//! - 重采样用线性插值(与 `mixer` 其余路径同口径;官方由系统混音器做)。
//!
//! # 实时约束
//!
//! 音频设备回调(`render_and_advance`)只做环形缓冲读取、线性插值与加法:
//! 无堆分配、无解码、无锁外等待。解码在 tick 线程,写入同一把 `SoundState`
//! 互斥锁内的环形缓冲(整数索引,容量固定 2^18 样本 ≈ 2.7s@48k 立体声)。

use crate::mixer::volume_gain;

/// 环形缓冲样本数(2 的幂;立体声交错 i16)。2^18 = 262144 样本 ≈ 2.73s@48k。
const RING_SAMPLES: usize = 1 << 18;
const RING_MASK: usize = RING_SAMPLES - 1;

/// 影片音频统计(诊断/测试)。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MovieAudioStats {
    pub pushed_frames: u64,
    pub played_frames: u64,
    pub dropped_frames: u64,
    pub underrun_frames: u64,
}

/// 影片音频环 + 重采样相位。
pub struct MovieAudio {
    ring: Vec<i16>,
    /// 读位置(样本下标,恒为偶数)。
    read: usize,
    /// 可读帧数(立体声帧)。
    len: usize,
    /// 源采样率(Hz;0 = 无影片音频)。
    rate: u32,
    /// 源声道数(当前后端恒 2;兼容 1/2)。
    channels: u16,
    /// `MOVIE[VOLUME]` 原值(0..256)。
    volume: i64,
    /// `MOVIE[PLAY]` 的播放位。
    playing: bool,
    /// 重采样相位 ∈ [0,1)。
    phase: f64,
    stats: MovieAudioStats,
}

impl Default for MovieAudio {
    fn default() -> Self {
        Self::new()
    }
}

impl MovieAudio {
    #[must_use]
    pub fn new() -> Self {
        Self {
            ring: Vec::new(),
            read: 0,
            len: 0,
            rate: 0,
            channels: 2,
            volume: 256,
            playing: false,
            phase: 0.0,
            stats: MovieAudioStats::default(),
        }
    }

    /// 清空并允许下一段影片重新设定格式。
    pub fn clear(&mut self) {
        self.read = 0;
        self.len = 0;
        self.rate = 0;
        self.channels = 2;
        self.phase = 0.0;
    }

    pub fn set_volume(&mut self, volume: i64) {
        self.volume = volume;
    }

    pub fn set_playing(&mut self, playing: bool) {
        self.playing = playing;
    }

    /// 追加交错 PCM(立体声/单声道)。源采样率变化时清空旧队列。
    pub fn push(&mut self, sample_rate: u32, channels: u16, samples: &[i16]) {
        if sample_rate == 0 || channels == 0 || samples.is_empty() {
            return;
        }
        if self.rate != sample_rate || self.channels != channels {
            self.clear();
            self.rate = sample_rate;
            self.channels = channels;
        }
        if self.ring.is_empty() {
            // 惰性分配:没有影片的工程零成本。
            self.ring = vec![0i16; RING_SAMPLES];
        }
        let src_ch = usize::from(channels);
        let frames = samples.len() / src_ch;
        let cap_frames = RING_SAMPLES / 2;
        // 队列满:丢最旧的整帧(保持与视频时钟同步,宁可跳过也不越积越多)。
        // 单次 push 大于整个环时只保留末尾 cap_frames 帧。
        let skip = frames.saturating_sub(cap_frames);
        let frames = frames - skip;
        if self.len + frames > cap_frames {
            let drop = self.len + frames - cap_frames;
            self.read = (self.read + drop * 2) & RING_MASK;
            self.len -= drop;
            self.stats.dropped_frames += drop as u64;
        }
        if skip > 0 {
            self.stats.dropped_frames += skip as u64;
        }
        for i in skip..skip + frames {
            let (l, r) = if src_ch == 1 {
                let v = samples[i];
                (v, v)
            } else {
                let base = i * src_ch;
                (samples[base], samples[base + 1])
            };
            let at = (self.read + self.len * 2) & RING_MASK;
            self.ring[at] = l;
            self.ring[at + 1] = r;
            self.len += 1;
        }
        self.stats.pushed_frames += frames as u64;
    }

    /// 混入一个输出块(设备回调路径)。
    ///
    /// `out` 已是其它声音层的混音结果;本函数只做加法。环形缓冲不足时
    /// 剩余输出保持原样(静音)并计入 `underrun_frames`。
    pub fn render(&mut self, out: &mut [f32], out_rate: u32, channels: usize) {
        if out_rate == 0 || channels == 0 || self.rate == 0 || !self.playing {
            return;
        }
        let out_frames = out.len() / channels;
        if out_frames == 0 {
            return;
        }
        let gain = volume_gain(self.volume);
        let ratio = f64::from(self.rate) / f64::from(out_rate);
        let cap_frames = RING_SAMPLES / 2;
        let mut produced = 0usize;
        // 音量 0 也继续消费:否则恢复音量时会播放陈旧音频并与画面脱节。
        // 混音总线是 f32 [-1,1];影片 PCM 是交错 i16,必须与 `mixer` 的
        // SOUND 层同口径归一化(`/32768`),否则原始 i16 直接加到总线上
        // 会削波成爆音。
        const I16_SCALE: f32 = 1.0 / 32768.0;
        while produced < out_frames {
            if self.len == 0 {
                self.stats.underrun_frames += (out_frames - produced) as u64;
                break;
            }
            let a = self.read & RING_MASK;
            let (l0, r0) = (
                self.ring[a] as f32 * I16_SCALE,
                self.ring[a + 1] as f32 * I16_SCALE,
            );
            // 只有 1 帧时用同一帧插值(末尾最后一帧不丢)。
            let (l1, r1) = if self.len >= 2 {
                let b = (self.read + 2) & RING_MASK;
                (
                    self.ring[b] as f32 * I16_SCALE,
                    self.ring[b + 1] as f32 * I16_SCALE,
                )
            } else {
                (l0, r0)
            };
            let t = self.phase as f32;
            let l = l0 + (l1 - l0) * t;
            let r = r0 + (r1 - r0) * t;
            if channels == 1 {
                out[produced] += (l + r) * 0.5 * gain;
            } else {
                let base = produced * channels;
                out[base] += l * gain;
                out[base + 1] += r * gain;
            }
            produced += 1;
            self.phase += ratio;
            while self.phase >= 1.0 {
                self.read = (self.read + 2) & RING_MASK;
                self.len -= 1;
                self.phase -= 1.0;
                if self.len == 0 {
                    break;
                }
            }
            if self.phase >= 1.0 {
                // 源已耗尽:相位钳到 0,等下一块补数据。
                self.phase = 0.0;
                if produced < out_frames {
                    self.stats.underrun_frames += (out_frames - produced) as u64;
                }
                break;
            }
            debug_assert!(self.len <= cap_frames);
        }
        self.stats.played_frames += produced as u64;
    }

    #[must_use]
    pub fn stats(&self) -> MovieAudioStats {
        self.stats
    }

    #[must_use]
    pub fn queued_frames(&self) -> usize {
        self.len
    }

    #[must_use]
    pub fn rate(&self) -> u32 {
        self.rate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_rate_passthrough_and_volume_curve() {
        let mut m = MovieAudio::new();
        m.set_playing(true);
        m.set_volume(256);
        // 480 帧立体声:左 = 1000,右 = -1000。
        let mut pcm = Vec::with_capacity(960);
        for _ in 0..480 {
            pcm.push(1000);
            pcm.push(-1000);
        }
        m.push(48_000, 2, &pcm);
        let mut out = vec![0.0f32; 960];
        m.render(&mut out, 48_000, 2);
        let g = volume_gain(256) / 32768.0;
        assert!((out[0] - 1000.0 * g).abs() < 0.001);
        assert!((out[1] + 1000.0 * g).abs() < 0.001);
        assert_eq!(m.stats().played_frames, 480);
        assert_eq!(m.queued_frames(), 0);
    }

    #[test]
    fn volume_zero_consumes_silently() {
        let mut m = MovieAudio::new();
        m.set_playing(true);
        m.set_volume(0);
        m.push(48_000, 2, &[1234i16; 960]);
        let mut out = vec![0.0f32; 960];
        m.render(&mut out, 48_000, 2);
        assert!(out.iter().all(|v| *v == 0.0));
        assert_eq!(m.queued_frames(), 0, "静音也必须消费,避免恢复后滞后");
    }

    #[test]
    fn resample_half_rate_consumes_half_source() {
        let mut m = MovieAudio::new();
        m.set_playing(true);
        m.set_volume(256);
        // 48k 源 → 24k 输出:480 输出帧应消费 960 源帧。
        let mut pcm = Vec::with_capacity(2 * 2000);
        for i in 0..2000 {
            let v = i as i16;
            pcm.push(v);
            pcm.push(v);
        }
        m.push(48_000, 2, &pcm);
        let mut out = vec![0.0f32; 480 * 2];
        m.render(&mut out, 24_000, 2);
        assert_eq!(m.stats().played_frames, 480);
        assert_eq!(m.queued_frames(), 2000 - 960);
    }

    #[test]
    fn mono_downmix_and_underrun_silence() {
        let mut m = MovieAudio::new();
        m.set_playing(true);
        m.set_volume(256);
        m.push(48_000, 2, &[1000i16; 2]); // 仅 1 帧
        let mut out = vec![0.0f32; 2];
        m.render(&mut out, 48_000, 1);
        let g = volume_gain(256) / 32768.0;
        assert!((out[0] - 1000.0 * g).abs() < 0.001);
        assert_eq!(out[1], 0.0, "第 2 帧欠载保持静音");
        assert!(m.stats().underrun_frames > 0);
    }

    #[test]
    fn clear_drops_queue_and_format() {
        let mut m = MovieAudio::new();
        m.push(48_000, 2, &[100i16; 96]);
        assert_eq!(m.queued_frames(), 48);
        m.clear();
        assert_eq!(m.queued_frames(), 0);
        assert_eq!(m.rate(), 0);
    }
}
