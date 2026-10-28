//! 声音流:按官方 0.495 架构装载(头部元数据 + 0.5s 环)与按需解码。
//!
//! # 证据(REA,`project-eris/yu-ris.exe` sha256 `4f5eefa3…`)
//!
//! - OGG 装载 `0x422ab0`:`FUN_0047f9a2`(ov_open)→ `FUN_00480654` →
//!   `FUN_0048064c` → `FUN_004815c8` → `FUN_004806c5`(时刻总量,即
//!   `ov_time_total` 语义)×1000 写 `+0x120`(TOTALTIME)/ `+0x134`。
//!   **没有任何 ov_read / 整段解码**。
//! - 装载尾部 `0x422c65`:写入 `+0x1b8/+0x1ba/+0x1bc/+0x1c6`(格式字段),
//!   `+0x1b4` = 每秒字节数,`+0x198` = 每秒字节数,然后
//!   `malloc(0x1000)` + `malloc(bytes_per_sec/2 + 0x1000)` 并
//!   `IDirectSound::CreateSoundBuffer`(结构体首字 0x14 = WAVEFORMATEX 长度)
//!   —— **0.5 秒环形缓冲**。解码是往环里按需填充,不在装载命令里整段做。
//!
//! # 本实现
//!
//! - [`Stream::ogg`] 只解析容器头(OggStreamReader 的 3 个 header packet),
//!   O(1) 返回;`TOTAL_TIME` 用最后一个 page 的 granule(与 `ov_time_total`
//!   同口径)。**不缓存整段 PCM**。
//! - 播放/查询时 [`Stream::sample`] 按需解码;tick 侧 [`Stream::prefetch`]
//!   提前填 `LEAD_MS`(=0.5s,对齐官方环大小),正常播放时音频回调只拷贝
//!   已解好的窗口。
//! - WAVE PCM / ACM:官方是 `mmio` 直读 / 一次 ACM 转换(REA `0x422e04`),
//!   本实现沿用整段读入的 [`Pcm`](代价是一次 memcpy/转换,语料 WAV 仅
//!   445KB);行为与旧路径逐样本一致。
//! - 解码与 granule 的已知偏差:lewton 对末包裁剪与 `ov_time_total` 有
//!   1.4% 文件 ≤36ms 的差异。本流以官方 granule 为总长,解码超出即截断、
//!   不足即补静音,保证 `TOTAL_TIME/END/循环` 与官方一致(见
//!   `docs/sound-ledger.md` N0-13)。

use std::io::Cursor;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use yu_log::Diag;

use crate::decode::{Codec, Pcm};
use crate::error::{self, code};
use crate::meta::AudioMeta;

/// 预解码提前量:官方 `0x422c65` 的 `bytes_per_sec/2` = 0.5 秒。
pub(crate) const LEAD_MS: i64 = 500;

/// 流解码计数(跨线程:`render` 是 `&self`,用原子量记账)。
#[derive(Default)]
pub(crate) struct StreamCounters {
    decode_us: AtomicU64,
    decoded_frames: AtomicU64,
    seeks: AtomicU64,
    seek_us: AtomicU64,
    slow_seeks: AtomicU64,
    underruns: AtomicU64,
    errors: AtomicU64,
}

impl StreamCounters {
    fn add_us(&self, us: u64) {
        self.decode_us.fetch_add(us, Ordering::Relaxed);
    }

    /// 装载路径的整段解码耗时(不经过 `Stream`,由状态机直接记)。
    pub(crate) fn add_us_direct(&self, us: u64) {
        self.decode_us.fetch_add(us, Ordering::Relaxed);
    }

    fn add_frames(&self, frames: u64) {
        self.decoded_frames.fetch_add(frames, Ordering::Relaxed);
    }

    fn add_seek(&self, us: u64) {
        self.seeks.fetch_add(1, Ordering::Relaxed);
        self.seek_us.fetch_add(us, Ordering::Relaxed);
        if us > 2_000 {
            self.slow_seeks.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn add_underrun(&self) {
        self.underruns.fetch_add(1, Ordering::Relaxed);
    }

    fn add_error(&self) {
        self.errors.fetch_add(1, Ordering::Relaxed);
    }

    /// `(decode_us, decoded_frames, seeks, seek_us, slow_seeks, underruns, errors)`,
    /// 取走即清零。
    pub(crate) fn take(&self) -> (u64, u64, u64, u64, u64, u64, u64) {
        (
            self.decode_us.swap(0, Ordering::Relaxed),
            self.decoded_frames.swap(0, Ordering::Relaxed),
            self.seeks.swap(0, Ordering::Relaxed),
            self.seek_us.swap(0, Ordering::Relaxed),
            self.slow_seeks.swap(0, Ordering::Relaxed),
            self.underruns.swap(0, Ordering::Relaxed),
            self.errors.swap(0, Ordering::Relaxed),
        )
    }
}

enum Kind {
    /// OGG:保留原始字节,供 seek 时重建 reader(头部解析 O(1))。
    Ogg {
        bytes: Arc<[u8]>,
        reader: lewton::inside_ogg::OggStreamReader<Cursor<Arc<[u8]>>>,
    },
    /// WAVE PCM / ACM:官方 mmio 直读 / 一次 ACM 转换,整段样本在手。
    Pcm(Arc<Pcm>),
}

pub(crate) struct Stream {
    name: Vec<u8>,
    channels: u16,
    bits: u16,
    sample_rate: u32,
    codec: Codec,
    total_frames: u64,
    total_ms: i64,
    kind: Kind,
    /// OGG 已解码窗口:`[window_start, window_start + window_frames)`。
    window: Vec<i16>,
    window_start: u64,
    eof: bool,
    pending_seek: Option<u64>,
    error: Option<Diag>,
    /// 首次欠载的现场(诊断用;`take_underrun_note` 取走)。
    underrun_note: Option<String>,
    /// 本段播放是否已上报过欠载(定位后重置;避免每帧刷屏)。
    underrun_reported: bool,
    counters: Arc<StreamCounters>,
}

impl std::fmt::Debug for Stream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stream")
            .field("name", &String::from_utf8_lossy(&self.name))
            .field("channels", &self.channels)
            .field("sample_rate", &self.sample_rate)
            .field("total_ms", &self.total_ms)
            .field("window_frames", &self.window_frames())
            .field("eof", &self.eof)
            .finish()
    }
}

impl Stream {
    /// OGG:只解析头部。失败 = 装载失败(与官方 ov_open 失败同型)。
    pub fn ogg(
        name: Vec<u8>,
        bytes: Vec<u8>,
        meta: &AudioMeta,
        counters: Arc<StreamCounters>,
    ) -> Result<Self, Diag> {
        let bytes: Arc<[u8]> = Arc::from(bytes.into_boxed_slice());
        let reader = lewton::inside_ogg::OggStreamReader::new(Cursor::new(Arc::clone(&bytes)))
            .map_err(|e| {
                error::recoverable(
                    code::DECODE_FAILED,
                    format!("OGG 头解析失败({}): {e}", display(&name)),
                )
            })?;
        Ok(Self {
            name,
            channels: meta.channels,
            bits: meta.bits,
            sample_rate: meta.sample_rate,
            codec: meta.codec,
            total_frames: meta.total_frames.unwrap_or(0),
            total_ms: meta.total_ms().unwrap_or(0),
            kind: Kind::Ogg { bytes, reader },
            window: Vec::new(),
            window_start: 0,
            eof: false,
            pending_seek: None,
            error: None,
            underrun_note: None,
            underrun_reported: false,
            counters,
        })
    }

    /// WAVE PCM / ACM:整段样本(与旧路径逐样本一致)。
    pub fn pcm(
        name: Vec<u8>,
        pcm: Arc<Pcm>,
        counters: Arc<StreamCounters>,
    ) -> Self {
        let channels = pcm.channels;
        let bits = pcm.bits;
        let sample_rate = pcm.sample_rate;
        let codec = pcm.codec;
        let total_ms = pcm.total_ms();
        let total_frames = pcm.frames();
        Self {
            name,
            channels,
            bits,
            sample_rate,
            codec,
            total_frames,
            total_ms,
            kind: Kind::Pcm(pcm),
            window: Vec::new(),
            window_start: 0,
            eof: false,
            pending_seek: None,
            error: None,
            underrun_note: None,
            underrun_reported: false,
            counters,
        }
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    pub fn bits(&self) -> u16 {
        self.bits
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn codec(&self) -> Codec {
        self.codec
    }

    pub fn total_frames(&self) -> u64 {
        self.total_frames
    }

    pub fn total_ms(&self) -> i64 {
        self.total_ms
    }

    /// 取走流内错误(装载后解码失败;由状态机转成 Diag)。
    pub fn take_error(&mut self) -> Option<Diag> {
        self.error.take()
    }

    /// 取走首次欠载现场(供状态机挂 Info 诊断;不重复刷屏)。
    pub fn take_underrun_note(&mut self) -> Option<String> {
        self.underrun_note.take()
    }

    fn note_underrun(&mut self, frame: u64) {
        if !self.underrun_reported && self.underrun_note.is_none() {
            self.underrun_reported = true;
            self.underrun_note = Some(format!(
                "frame={frame} window=[{},{}] total={} eof={} pending={:?}",
                self.window_start,
                self.window_end(),
                self.total_frames,
                self.eof,
                self.pending_seek
            ));
        }
    }

    /// millisecond → 源帧(与混音同一换算:floor)。
    fn frame_at(&self, ms: i64) -> u64 {
        if self.sample_rate == 0 {
            return 0;
        }
        let ms = ms.max(0) as u128;
        ((ms * u128::from(self.sample_rate)) / 1000) as u64
    }

    /// 请求定位到毫秒(懒执行:真正解码发生在下一次 `sample`/`prefetch`)。
    pub fn request_seek_ms(&mut self, ms: i64) {
        if !matches!(self.kind, Kind::Ogg { .. }) {
            return;
        }
        let target = self.frame_at(ms).min(self.total_frames);
        if self.window.is_empty() && self.window_start == 0 && !self.eof {
            // 刚装载/刚重开:reader 就在 0,无需重建。
            // 注意 `window_start != 0` 时窗口虽空但 reader 已在末尾(播完/
            // 循环回绕),必须走 seek 重开,否则 sample(0) 会欠载。
            return;
        }
        if target >= self.window_start && target < self.window_start + self.window_frames() {
            // 窗口内随机访问,无需 seek。
            return;
        }
        self.pending_seek = Some(target);
    }

    fn window_frames(&self) -> u64 {
        if self.channels == 0 {
            0
        } else {
            (self.window.len() / usize::from(self.channels)) as u64
        }
    }

    fn window_end(&self) -> u64 {
        self.window_start + self.window_frames()
    }

    /// tick 侧预解码:保证窗口覆盖 `playhead_ms + lead_ms`。
    pub fn prefetch(&mut self, playhead_ms: i64, lead_ms: i64) {
        if self.sample_rate == 0 {
            return;
        }
        // 先把 PLAY/START/RESTART/循环回绕留下的定位在 tick 里做掉:
        // 否则第一次 `sample`(可能在查询/音频回调里)才重建 reader,
        // 慢定位的成本会落到错误的线程且不进 tick 账单。
        if let Some(target) = self.pending_seek.take() {
            self.apply_seek(target);
        }
        let target = self.frame_at(playhead_ms.saturating_add(lead_ms));
        if target <= self.window_end() {
            return;
        }
        self.ensure(target);
    }

    /// 丢弃窗口内已消费的前缀(有界内存;官方环 0.5s,这里留 1s 余量)。
    pub fn discard_before(&mut self, frame: u64) {
        if !matches!(self.kind, Kind::Ogg { .. }) || self.channels == 0 {
            return;
        }
        let keep_from = frame.min(self.window_end());
        if keep_from <= self.window_start {
            return;
        }
        let drop = (keep_from - self.window_start) as usize;
        let samples = drop * usize::from(self.channels);
        if samples >= self.window.len() {
            self.window.clear();
            self.window_start = keep_from;
        } else {
            self.window.drain(..samples);
            self.window_start = keep_from;
        }
    }

    /// 取 `frame` 处的样本;越界/解码失败返回 `None`(越界 = 文件尾)。
    pub fn sample(&mut self, frame: u64, ch: usize) -> Option<i16> {
        if frame >= self.total_frames {
            return None;
        }
        if let Some(target) = self.pending_seek.take() {
            self.apply_seek(target);
        }
        match &self.kind {
            Kind::Pcm(pcm) => {
                let idx = (frame as usize)
                    .checked_mul(usize::from(self.channels))?
                    .checked_add(ch)?;
                pcm.samples.get(idx).copied()
            }
            Kind::Ogg { .. } => {
                if !self.ensure(frame) {
                    self.counters.add_underrun();
                    self.note_underrun(frame);
                    return None;
                }
                if frame < self.window_start {
                    // 倒退采样(未走 request_seek)是内部不变量破坏:显式失败。
                    self.counters.add_underrun();
                    self.note_underrun(frame);
                    return None;
                }
                let idx = (frame - self.window_start) as usize * usize::from(self.channels) + ch;
                self.window.get(idx).copied()
            }
        }
    }

    /// 确保窗口覆盖 `frame`(含)。
    fn ensure(&mut self, frame: u64) -> bool {
        if !matches!(self.kind, Kind::Ogg { .. }) {
            return frame < self.total_frames;
        }
        while self.window_end() <= frame {
            if self.eof {
                if self.window_end() < self.total_frames {
                    // 解码帧数少于官方 granule:按官方总长补静音(≤36ms 已知偏差)。
                    let need = (frame + 1).min(self.total_frames) - self.window_end();
                    self.window
                        .resize(self.window.len() + need as usize * usize::from(self.channels), 0);
                    continue;
                }
                return false;
            }
            if !self.decode_next() {
                continue;
            }
        }
        true
    }

    /// 解码一个 OGG packet 进窗口。
    fn decode_next(&mut self) -> bool {
        let channels = usize::from(self.channels);
        let Kind::Ogg { reader, .. } = &mut self.kind else {
            return false;
        };
        let started = Instant::now();
        let result = reader.read_dec_packet_itl();
        self.counters.add_us(
            u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
        );
        match result {
            Ok(Some(packet)) => {
                let room = self.total_frames.saturating_sub(self.window_end()) as usize;
                let frames = (packet.len() / channels.max(1)).min(room);
                let take = frames * channels;
                self.window.extend_from_slice(&packet[..take]);
                self.counters.add_frames(frames as u64);
                true
            }
            Ok(None) => {
                self.eof = true;
                false
            }
            Err(e) => {
                self.error = Some(error::recoverable(
                    code::DECODE_FAILED,
                    format!("OGG 解码失败({}): {e}", display(&self.name)),
                ));
                self.counters.add_error();
                self.eof = true;
                false
            }
        }
    }

    /// 重建 reader 并从 0 解码到 `target`(官方 `ov_pcm_seek` 语义;
    /// 头部解析 O(1),首播/循环 RESTART=0 只解一个 packet)。
    fn apply_seek(&mut self, target: u64) {
        let target = target.min(self.total_frames);
        self.underrun_reported = false;
        let started = Instant::now();
        match &mut self.kind {
            Kind::Pcm(_) => {
                self.window_start = target;
            }
            Kind::Ogg { bytes, reader } => {
                let Ok(fresh) = lewton::inside_ogg::OggStreamReader::new(
                    Cursor::new(Arc::clone(bytes)),
                ) else {
                    self.error = Some(error::recoverable(
                        code::DECODE_FAILED,
                        format!("OGG 重开失败({})", display(&self.name)),
                    ));
                    self.counters.add_error();
                    return;
                };
                *reader = fresh;
                self.window.clear();
                self.window_start = 0;
                self.eof = false;
                // 解码并丢弃到 target;target 通常 = START/RESTART(多为 0)。
                while self.window_end() < target && !self.eof {
                    if !self.decode_next() && self.eof {
                        break;
                    }
                }
                if target > self.window_start {
                    let drop_frames = (target - self.window_start).min(self.window_frames());
                    let samples = drop_frames as usize * usize::from(self.channels);
                    self.window.drain(..samples);
                    self.window_start += drop_frames;
                }
            }
        }
        // 大 START/RESTART 的定位成本只做计数(不写 stderr;诊断走状态机)。
        let us = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
        self.counters.add_seek(us);
    }
}

fn display(bytes: &[u8]) -> String {
    let mut s = String::new();
    for &b in bytes {
        if (0x20..0x7f).contains(&b) {
            s.push(b as char);
        } else {
            s.push_str(&format!("\\x{b:02x}"));
        }
    }
    s
}

