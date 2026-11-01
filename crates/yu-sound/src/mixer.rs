//! 确定性混音核心（headless）。设备输出是旁路，不参与本模块。
//!
//! 已取证：音量经 DSBVOLUME 后振幅 = (v/256)^2、E=0 静音、淡变线性（1/1024 固定点）、
//! 声像 = `DSBPAN`（0..512 → -10000..+10000 百分之一分贝）、
//! 音调 = `SetFrequency(base*(pitch+256)/512)`。
//!
//! 音量**不是**单层字段：官方 `FUN_00424dfc` 把自身 `(E==1)*VOLUME` 沿
//! 祖先链逐级 `>>8` 折叠（主音量 `S0` → 频道 `BGM` → 单条声音），
//! 见 [`SoundState::effective_for_key`]；本模块每块按有效音量出增益。
//!
//! **Unverified**：线性重采样插值、最终限幅。证据未覆盖，代码处标注。

use crate::state::{Layer, SoundState};

const PAN_FULL: i64 = 10_000;

/// 声像 dB（0.01 dB 单位，DSBPAN 约定）。REA `0x424fbc`：
/// `round(log10(min(pan,512-pan)/256) * 40 * 100)`，钳制 [-10000,0]，
/// `pan>256` 取反。`pan=0/512` 特判 -10000。
pub fn pan_hundredths_db(pan: i64) -> i64 {
    let pan = pan.clamp(0, 512);
    let gauge = if pan <= 256 { pan } else { 512 - pan };
    if gauge <= 0 {
        return if pan > 256 { PAN_FULL } else { -PAN_FULL };
    }
    let db = ((gauge as f64 / 256.0).log10() * 4000.0).round() as i64;
    let db = db.clamp(-PAN_FULL, 0);
    if pan > 256 {
        -db
    } else {
        db
    }
}

/// 声像 → 左右线性增益（DSBPAN：正值衰减左声道，负值衰减右声道）。
pub fn pan_gains(pan: i64) -> (f32, f32) {
    let db = pan_hundredths_db(pan) as f64 / 100.0;
    let left = if db < 0.0 { 10f64.powf(db / 20.0) } else { 1.0 };
    let right = if db > 0.0 {
        10f64.powf(-db / 20.0)
    } else {
        1.0
    };
    (left as f32, right as f32)
}

/// 音量 → 振幅增益。
///
/// 引擎 `0x424e4c` 把 0..256 映射成 `DSBVOLUME = round(4000*log10(v/256))`
/// （单位 1/100 dB）；DirectSound 的振幅是 `10^(dB/20)`，所以
/// `amplitude = (v/256)^2`，不是线性。
pub fn volume_gain(volume: i64) -> f32 {
    let v = (volume.clamp(0, 256) as f32) / 256.0;
    v * v
}

impl SoundState {
    /// 渲染到交错 f32 缓冲；`out.len()` 必须是 `channels` 的整数倍。
    ///
    /// 纯函数：不推进状态。调用方先 [`SoundState::advance`] 再渲染
    /// `delta_ms` 对应的帧数。
    pub fn render(&self, out: &mut [f32], out_rate: u32, channels: usize) {
        self.render_with(out, out_rate, channels, true)
    }

    /// [`SoundState::render`] 的内部形态。
    ///
    /// `allow_decode = false` = 硬实时路径(设备回调):**绝不解码**,窗口没
    /// 准备好就静音,靠 tick 侧 `prefetch` 补齐。
    /// `allow_decode = true` = 离线路径(headless/单测):可以直接解码,因为
    /// 调用方不在实时线程上。
    pub fn render_with(
        &self,
        out: &mut [f32],
        out_rate: u32,
        channels: usize,
        allow_decode: bool,
    ) {
        for v in out.iter_mut() {
            *v = 0.0;
        }
        if out_rate == 0 || channels == 0 {
            return;
        }
        let frames = out.len() / channels;
        for (id, layer) in self.layers_iter() {
            // 官方在设置音量时用 `FUN_00424dfc` 把祖先链折进 DSB 音量
            // （主音量 `:/S0/` → 频道 `BGM/` → 单条声音）；这里按同一
            // 公式取有效音量，容器/父层改音量对全部后代即时生效。
            let gain = volume_gain(self.effective_for_key(id));
            mix_layer(
                layer,
                gain,
                out,
                frames,
                out_rate,
                channels,
                allow_decode,
                &self.rt_silence_samples,
            );
        }
        // 调试抓取:把限幅**之前**的原始混音结果留一份(爆音分析用)。
        if let Ok(mut slot) = self.debug_capture.lock() {
            if let Some(buffer) = slot.as_mut() {
                buffer.extend_from_slice(out);
            }
        }
        // Unverified：引擎最终由 DirectSound 混音器限幅；这里只做 [-1,1] 钳制。
        //
        // 硬削波会把叠加过量的波形切成方波 —— 听感就是刺耳爆音。计数出来
        // 才能区分"没声音问题"与"增益结构不对"(后者要查各层音量/主音量)。
        let mut clipped = 0u64;
        for v in out.iter_mut() {
            if *v > 1.0 || *v < -1.0 {
                clipped += 1;
            }
            *v = v.clamp(-1.0, 1.0);
        }
        if clipped > 0 {
            self.clipped_samples
                .fetch_add(clipped, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

/// 记账:硬实时路径整块静音(源窗口未覆盖)。
fn note_rt_silence(
    counter: &std::sync::atomic::AtomicU64,
    frames: usize,
    channels: usize,
) {
    counter.fetch_add(
        frames.saturating_mul(channels) as u64,
        std::sync::atomic::Ordering::Relaxed,
    );
}

fn mix_layer(
    layer: &Layer,
    gain: f32,
    out: &mut [f32],
    frames: usize,
    out_rate: u32,
    channels: usize,
    allow_decode: bool,
    rt_silence: &std::sync::atomic::AtomicU64,
) {
    if !layer.state().audible() || layer.paused() {
        return;
    }
    if gain == 0.0 {
        return;
    }
    let (left_gain, right_gain) = pan_gains(layer.pan());
    // 与状态机同一映射：base*(pitch+256)/512。
    let pitch = (layer.pitch().clamp(0, 512) + 256) as f64 / 512.0;
    // 流式源:窗口内随机访问,不足则按需解码(官方 0.5s 环的同构;
    // 正常播放时 tick 的 `prefetch` 已把窗口填好,回调只拷贝)。
    let mut guard = layer.stream_mut();
    let Some(stream) = guard.as_mut() else { return };
    let src_rate = stream.sample_rate();
    let src_channels = usize::from(stream.channels());
    let total = stream.total_frames();
    if src_rate == 0 || src_channels == 0 || total == 0 {
        return;
    }
    let end_ms = layer.end_ms().max(0) as f64;
    let restart_ms = layer.restart_ms().max(0) as f64;
    let mut pos_ms = layer.playhead_ms();
    let step_ms = 1000.0 / out_rate as f64;
    // 延迟窗口（`+0x190/+0x194`）＝按字节填充的静音区间（Likely，DIV-SND-002）：
    // 本块开头 floor(剩余延迟×rate/1000) 帧保持静音且不推进位置。
    // 注意：设备路径 `render_and_advance` 是先渲染后推进，延迟在本块内可见；
    // headless 的 advance-then-render 序列会先烧掉延迟，静音区间不体现在
    // 该路径的渲染输出（TIME 查询语义不受影响）。
    let silent_frames = if layer.delay_ms() > 0.0 {
        ((layer.delay_ms() * f64::from(out_rate) / 1000.0).floor() as usize).min(frames)
    } else {
        0
    };

    // ---- 硬实时前置条件:本块需要的源样本必须**已在已解码窗口内** ----
    //
    // 音频回调绝不解码。历史教训:调用 `Stream::sample()` 会在窗口不足时
    // 就地解码(lewton 解一个 packet 是毫秒级),在 128~1024 帧的块预算里
    // 直接超时 ⇒ 欠载 ⇒ 爆音。浏览器(AudioWorklet 128 帧)与安卓(oboe
    // 小块)因此必爆,桌面(cpal 大缓冲)侥幸不爆 —— 这正是"桌面不爆音、
    // 其它平台爆音、刚开始最严重(窗口空)"的统一解释。
    //
    // 现在:窗口没准备好就整块静音,由 tick 线程的 `prefetch` 补齐;解码
    // 永远不在回调里发生。代价只是启动/定位后极短的静音,不会有噪声。
    if !allow_decode && silent_frames < frames {
        // 整块都是延迟静音时不需要任何样本(位置也不推进),跳过检查。
        if stream.seek_pending() {
            note_rt_silence(rt_silence, frames - silent_frames, channels);
            return;
        }
        let sounded = frames - silent_frames;
        let last_pos_ms = pos_ms + step_ms * pitch * sounded as f64;
        let need_frame = (last_pos_ms.max(0.0) * src_rate as f64 / 1000.0).ceil() as u64 + 2;
        if need_frame < total && !stream.covers(need_frame) {
            // 窗口不够覆盖到本块结尾:静音本块,等预取。
            note_rt_silence(rt_silence, sounded, channels);
            return;
        }
    }

    for f in 0..frames {
        if f < silent_frames {
            continue;
        }
        // 循环边界（与状态机相同的 END/RESTART 规则）。
        //
        // 这里**不能**就地 seek:seek 会重建解码器,必须在 tick 线程做。
        // 本块读到 END 就停(余下静音),下一块由预取在 restart 位置重新填。
        if end_ms > 0.0 && pos_ms >= end_ms {
            if allow_decode {
                pos_ms = restart_ms;
                stream.request_seek_ms(restart_ms as i64);
            } else {
                break;
            }
        }
        let src_pos = pos_ms * src_rate as f64 / 1000.0;
        if src_pos >= total as f64 {
            // 到文件尾:本块剩余留静音(正常收尾),但**不能**无限累加位置 ——
            // 交给状态机在下一个 advance 里按 END/LOOP 规则处理。
            break;
        }
        let i0 = src_pos.floor() as u64;
        let frac = (src_pos - i0 as f64) as f32;
        let i1 = (i0 + 1).min(total - 1);
        // 实时路径只读已解窗口(不解码);离线路径允许按需解码。
        let (l0, l1) = if allow_decode {
            match (stream.sample(i0, 0), stream.sample(i1, 0)) {
                (Some(a), Some(b)) => (a, b),
                _ => {
                    pos_ms += step_ms * pitch;
                    continue;
                }
            }
        } else {
            match (stream.peek(i0, 0), stream.peek(i1, 0)) {
                (Some(a), Some(b)) => (a, b),
                _ => {
                    // 逐帧落窗(未被上面的整块前置检查拦住):这是**不可闻单帧
                    // 缺口**的来源,必须单独计数,否则整块静音为 0 时会漏判。
                    note_rt_silence(rt_silence, 1, channels);
                    pos_ms += step_ms * pitch;
                    continue;
                }
            }
        };
        let o = f * channels;
        if channels == 1 {
            let s = lerp(l0, l1, frac);
            out[o] += s * gain;
        } else {
            let l = lerp(l0, l1, frac);
            let r = if src_channels >= 2 {
                match (stream.peek(i0, 1), stream.peek(i1, 1)) {
                    (Some(r0), Some(r1)) => lerp(r0, r1, frac),
                    _ => l,
                }
            } else {
                l
            };
            out[o] += l * gain * left_gain;
            out[o + 1] += r * gain * right_gain;
        }
        pos_ms += step_ms * pitch;
    }
    // 有界窗口:丢已消费前缀,但**保留约 0.25s 余量**。
    //
    // 原来按 pos_ms 精确丢弃:音频回调(44100Hz 连续吃样本)与 tick 侧预取
    // (60Hz 才跑一次)节奏不同,精确丢到播放头会让下一次回调在窗口头部
    // 找不到样本 —— 每块都可能触发,听感就是持续爆音。留余量后回调始终能
    // 命中已解窗口,预取只需覆盖新增部分。
    let keep_from = (pos_ms.max(0.0) * src_rate as f64 / 1000.0).floor() as u64;
    let safety_margin = (src_rate as u64) / 4; // 0.25s
    stream.discard_before(keep_from.saturating_sub(safety_margin));
}

fn lerp(a: i16, b: i16, frac: f32) -> f32 {
    let a = a as f32 / 32768.0;
    let b = b as f32 / 32768.0;
    a * (1.0 - frac) + b * frac
}
