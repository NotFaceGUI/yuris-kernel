//! 确定性混音核心（headless）。设备输出是旁路，不参与本模块。
//!
//! 已取证：音量经 DSBVOLUME 后振幅 = (v/256)^2、E=0 静音、淡变线性（1/1024 固定点）、
//! 声像 = `DSBPAN`（0..512 → -10000..+10000 百分之一分贝）、
//! 音调 = `SetFrequency(base*(pitch+256)/512)`。
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
        for (_, layer) in self.layers_iter() {
            mix_layer(layer, out, frames, out_rate, channels);
        }
        // Unverified：引擎最终由 DirectSound 混音器限幅；这里只做 [-1,1] 钳制。
        for v in out.iter_mut() {
            *v = v.clamp(-1.0, 1.0);
        }
    }
}

fn mix_layer(layer: &Layer, out: &mut [f32], frames: usize, out_rate: u32, channels: usize) {
    let Some(pcm) = layer.pcm() else { return };
    if !layer.state().audible() || layer.paused() || pcm.sample_rate == 0 || pcm.channels == 0 {
        return;
    }
    let gain = volume_gain(layer.audible_volume());
    if gain == 0.0 {
        return;
    }
    let (left_gain, right_gain) = pan_gains(layer.pan());
    // 与状态机同一映射：base*(pitch+256)/512。
    let pitch = (layer.pitch().clamp(0, 512) + 256) as f64 / 512.0;
    let src_frames = pcm.frames() as f64;
    if src_frames <= 0.0 {
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
        }
        let src_pos = pos_ms * pcm.sample_rate as f64 / 1000.0;
        if src_pos >= src_frames {
            break;
        }
        let i0 = src_pos.floor() as usize;
        let frac = (src_pos - i0 as f64) as f32;
        let i1 = (i0 + 1).min(src_frames as usize - 1);
        let base0 = i0 * pcm.channels as usize;
        let base1 = i1 * pcm.channels as usize;
        let o = f * channels;
        if channels == 1 {
            let s = lerp(pcm, base0, base1, frac, 0);
            out[o] += s * gain;
        } else {
            let l = lerp(pcm, base0, base1, frac, 0);
            let r = if pcm.channels >= 2 {
                lerp(pcm, base0, base1, frac, 1)
            } else {
                l
            };
            out[o] += l * gain * left_gain;
            out[o + 1] += r * gain * right_gain;
        }
        pos_ms += step_ms * pitch;
    }
}

fn lerp(pcm: &crate::decode::Pcm, base0: usize, base1: usize, frac: f32, ch: usize) -> f32 {
    let a = pcm.samples.get(base0 + ch).copied().unwrap_or(0) as f32 / 32768.0;
    let b = pcm.samples.get(base1 + ch).copied().unwrap_or(0) as f32 / 32768.0;
    a * (1.0 - frac) + b * frac
}
