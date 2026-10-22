//! 设备输出（默认 `device` feature）。
//!
//! 线程模型：主线程在每帧 `tick` 后 `SoundState::render` → 环形缓冲；
//! cpal 音频回调只从环形缓冲取样本（欠载补 0），**不反哺**状态机。
//! 因此设备输出不影响 headless/golden 的确定性。

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use yu_sound::SoundState;

pub struct SoundSink {
    ring: Arc<Mutex<VecDeque<f32>>>,
    /// 持有流即保持播放；drop 时停止。
    _stream: cpal::Stream,
    rate: u32,
    channels: usize,
}

fn fill_f32(ring: &Mutex<VecDeque<f32>>, data: &mut [f32]) {
    let mut q = ring.lock().unwrap();
    for x in data {
        *x = q.pop_front().unwrap_or(0.0);
    }
}

fn fill_i16(ring: &Mutex<VecDeque<f32>>, data: &mut [i16]) {
    let mut q = ring.lock().unwrap();
    for x in data {
        let v = q.pop_front().unwrap_or(0.0).clamp(-1.0, 1.0);
        *x = (v * f32::from(i16::MAX)) as i16;
    }
}

fn fill_u16(ring: &Mutex<VecDeque<f32>>, data: &mut [u16]) {
    let mut q = ring.lock().unwrap();
    for x in data {
        let v = q.pop_front().unwrap_or(0.0).clamp(-1.0, 1.0);
        *x = (v * 32767.0 + 32768.0) as u16;
    }
}

impl SoundSink {
    pub fn open() -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "没有默认输出设备".to_string())?;
        let supported = device
            .default_output_config()
            .map_err(|e| format!("默认输出配置不可用: {e}"))?;
        let sample_format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let rate = config.sample_rate.0;
        let channels = usize::from(config.channels);
        // 初始静音垫：生产与消费同速，若从 0 开始，任何调度抖动都会
        // 立刻欠载。200ms 垫让回调有稳定余量（BGM 可接受这点延迟）。
        let cushion = rate as usize * channels * 200 / 1000;
        let mut initial = VecDeque::<f32>::with_capacity(rate as usize * channels * 2);
        initial.resize(cushion, 0.0);
        let ring = Arc::new(Mutex::new(initial));
        let err_fn = |err| eprintln!("[sound] 设备流错误: {err}");
        let stream = match sample_format {
            cpal::SampleFormat::F32 => {
                let r = ring.clone();
                device.build_output_stream(
                    &config,
                    move |data: &mut [f32], _| fill_f32(&r, data),
                    err_fn,
                    None,
                )
            }
            cpal::SampleFormat::I16 => {
                let r = ring.clone();
                device.build_output_stream(
                    &config,
                    move |data: &mut [i16], _| fill_i16(&r, data),
                    err_fn,
                    None,
                )
            }
            cpal::SampleFormat::U16 => {
                let r = ring.clone();
                device.build_output_stream(
                    &config,
                    move |data: &mut [u16], _| fill_u16(&r, data),
                    err_fn,
                    None,
                )
            }
            other => {
                return Err(format!("默认输出格式 {other:?} 未接入（支持 f32/i16/u16）"));
            }
        }
        .map_err(|e| format!("创建输出流失败: {e}"))?;
        stream.play().map_err(|e| format!("启动输出流失败: {e}"))?;
        Ok(Self {
            ring,
            _stream: stream,
            rate,
            channels,
        })
    }

    /// 渲染 `delta_ms` 对应的 PCM 并推入环形缓冲；超过 2 秒丢弃最旧。
    ///
    /// `delta_ms` 必须与 `SoundState::advance` 的实际增量一致，否则声音
    /// 位置与输出块会互相追赶（卡顿/电音）。
    pub fn pump(&self, state: &Rc<RefCell<SoundState>>, delta_ms: u32) {
        let frames = ((u64::from(self.rate) * u64::from(delta_ms)) / 1000) as usize;
        if frames == 0 {
            return;
        }
        let mut buf = vec![0.0f32; frames * self.channels];
        state.borrow().render(&mut buf, self.rate, self.channels);
        let mut q = self.ring.lock().unwrap();
        let cap = self.rate as usize * self.channels * 2;
        let overflow = (q.len() + buf.len()).saturating_sub(cap);
        if overflow > 0 {
            q.drain(..overflow);
        }
        q.extend(buf);
    }

    pub fn clear(&self) {
        self.ring.lock().unwrap().clear();
    }

    /// 把环形缓冲补到 200ms 静音垫（暂停恢复前调用）。
    pub fn prime(&self) {
        let target = self.rate as usize * self.channels * 200 / 1000;
        let mut q = self.ring.lock().unwrap();
        if q.len() < target {
            q.resize(target, 0.0);
        }
    }
}
