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
            mix_layer(layer, gain, out, frames, out_rate, channels);
        }
        // Unverified：引擎最终由 DirectSound 混音器限幅；这里只做 [-1,1] 钳制。
        for v in out.iter_mut() {
            *v = v.clamp(-1.0, 1.0);
        }
    }
}

fn mix_layer(
    layer: &Layer,
    gain: f32,
    out: &mut [f32],
    frames: usize,
    out_rate: u32,
    channels: usize,
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

    for f in 0..frames {
        if f < silent_frames {
            continue;
        }
        // 循环边界（与状态机相同的 END/RESTART 规则；只在本块内回绕）。
        if end_ms > 0.0 && pos_ms >= end_ms {
            pos_ms = restart_ms;
            stream.request_seek_ms(restart_ms as i64);
        }
        let src_pos = pos_ms * src_rate as f64 / 1000.0;
        if src_pos >= total as f64 {
            break;
        }
        let i0 = src_pos.floor() as u64;
        let frac = (src_pos - i0 as f64) as f32;
        let i1 = (i0 + 1).min(total - 1);
        let Some(l0) = stream.sample(i0, 0) else { break };
        let Some(l1) = stream.sample(i1, 0) else { break };
        let o = f * channels;
        if channels == 1 {
            let s = lerp(l0, l1, frac);
            out[o] += s * gain;
        } else {
            let l = lerp(l0, l1, frac);
            let r = if src_channels >= 2 {
                match (stream.sample(i0, 1), stream.sample(i1, 1)) {
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
    // 有界窗口:丢已消费前缀(官方环 0.5s;backward seek 由 request_seek 重建)。
    stream.discard_before((pos_ms.max(0.0) * src_rate as f64 / 1000.0).floor() as u64);
}

fn lerp(a: i16, b: i16, frac: f32) -> f32 {
    let a = a as f32 / 32768.0;
    let b = b as f32 / 32768.0;
    a * (1.0 - frac) + b * frac
}
