//! 设备输出（默认 `device` feature）。
//!
//! **音频线程是时钟**：cpal 回调直接锁 `Arc<Mutex<SoundState>>` 渲染,并按
//! 实际输出帧数推进播放头。主线程只应用 SOUND 命令/查询,不生产 PCM。
//!
//! 旧实现由视频帧 `pump` 环形缓冲:拖动窗口时事件循环进入模态移动、
//! 长帧时缓冲耗尽 → 声音停;整数毫秒取整还会长期漂移。现在回调拉取,
//! UI 线程被阻塞也不会中断音频。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use yu_sound::SoundState;

/// 可热替换的状态槽：restart 时换入新 `SoundState`，不重开设备。
type Slot = Arc<Mutex<Option<Arc<Mutex<SoundState>>>>>;

pub struct SoundSink {
    slot: Slot,
    /// 持有流即保持播放；drop 时停止。
    _stream: cpal::Stream,
}

fn current(slot: &Slot) -> Option<Arc<Mutex<SoundState>>> {
    slot.lock().ok().and_then(|slot| slot.clone())
}

fn render_f32(slot: &Slot, data: &mut [f32], rate: u32, channels: usize, first_audio: &AtomicBool) {
    let Some(state) = current(slot) else {
        data.fill(0.0);
        return;
    };
    match state.lock() {
        Ok(mut state) => state.render_and_advance(data, rate, channels),
        Err(_) => data.fill(0.0),
    };
    let peak = data.iter().fold(0.0f32, |a, &b| a.max(b.abs()));
    if peak > 0.0 && !first_audio.swap(true, Ordering::Relaxed) {
        // `src/` 纪律:不走 stdout/stderr 宏;经 log 门面进宿主统一 sink。
        log::info!(
            target: "yu-sound",
            "[sound] first non-silent block: peak={peak:.4} frames={} rate={rate}",
            data.len() / channels.max(1)
        );
    }
}

fn render_i16(
    slot: &Slot,
    data: &mut [i16],
    scratch: &mut Vec<f32>,
    rate: u32,
    channels: usize,
    first_audio: &AtomicBool,
) {
    scratch.resize(data.len(), 0.0);
    render_f32(slot, scratch, rate, channels, first_audio);
    for (out, sample) in data.iter_mut().zip(scratch.iter()) {
        *out = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
    }
}

fn render_u16(
    slot: &Slot,
    data: &mut [u16],
    scratch: &mut Vec<f32>,
    rate: u32,
    channels: usize,
    first_audio: &AtomicBool,
) {
    scratch.resize(data.len(), 0.0);
    render_f32(slot, scratch, rate, channels, first_audio);
    for (out, sample) in data.iter_mut().zip(scratch.iter()) {
        *out = (sample.clamp(-1.0, 1.0) * 32767.0 + 32768.0) as u16;
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
        let slot: Slot = Arc::new(Mutex::new(None));
        let first_audio = Arc::new(AtomicBool::new(false));
        // 回调线程不能 panic;错误经 log 门面留痕(宿主 main 已初始化桥)。
        let err_fn = |err| log::warn!(target: "yu-sound", "[sound] 设备流错误: {err}");
        let stream = match sample_format {
            cpal::SampleFormat::F32 => {
                let slot = slot.clone();
                let first = Arc::clone(&first_audio);
                device.build_output_stream(
                    &config,
                    move |data: &mut [f32], _| render_f32(&slot, data, rate, channels, &first),
                    err_fn,
                    None,
                )
            }
            cpal::SampleFormat::I16 => {
                let slot = slot.clone();
                let first = Arc::clone(&first_audio);
                let mut scratch = Vec::<f32>::new();
                device.build_output_stream(
                    &config,
                    move |data: &mut [i16], _| {
                        render_i16(&slot, data, &mut scratch, rate, channels, &first)
                    },
                    err_fn,
                    None,
                )
            }
            cpal::SampleFormat::U16 => {
                let slot = slot.clone();
                let first = Arc::clone(&first_audio);
                let mut scratch = Vec::<f32>::new();
                device.build_output_stream(
                    &config,
                    move |data: &mut [u16], _| {
                        render_u16(&slot, data, &mut scratch, rate, channels, &first)
                    },
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
            slot,
            _stream: stream,
        })
    }

    /// 换入新的声音状态。旧状态在回调下一次取槽后自然释放。
    ///
    /// 同时标记外部时钟:VM 的 `advance(now_ms)` 只更新时钟,层位置由
    /// 本回调按精确帧数推进。
    pub fn attach(&self, state: Arc<Mutex<SoundState>>) {
        state.lock().unwrap().set_external_clock(true);
        *self.slot.lock().unwrap() = Some(state);
    }

    /// 静音(restart 的 boot 期间)。
    pub fn detach(&self) {
        *self.slot.lock().unwrap() = None;
    }
}
