//! Android 宿主的音频输出(cpal;Android 上走 oboe)。
//!
//! # 分层
//!
//! ```text
//! 混音(官方语义)         交付(本模块)
//! yu-sound::SoundState  →  mixer 线程  →  无锁 SPSC 环  →  oboe 回调
//! ```
//!
//! 本模块**不解释任何声音语义**:只负责把 `render_and_advance` 产出的 PCM
//! 可靠地送到设备。语义层是 `yu-sound` 对官方 SOUND 命令的 1:1 复刻
//! (音量 `(v/256)²`、音高 `base*(pitch+256)/512`、END/RESTART/LOOP、
//! 延迟窗口、层树继承),换成通用音频库就得重做,所以这里只借鉴
//! 成熟库的**交付**经验,不替换混音。
//!
//! # 三件从 kira 学来的事
//!
//! 1. **回调零锁**:oboe/AAudio 回调跑在实时优先级、预算只有几毫秒;
//!    阻塞(等锁/分配/解码)就是爆音。因此回调只做无锁环拷贝。
//! 2. **解码不在回调里**:回调读不到窗口就补静音,解码交给 tick 侧预取。
//! 3. **流要有监督者**(`StreamManager`)——本模块的 `audio_thread`:
//!    AAudio 在设备断开/音频路由切换(插拔耳机、蓝牙)时会关闭流并报
//!    `ErrorDisconnected`,**不重建就永远没声音**。kira 正是每 500ms 轮询
//!    设备名与采样率并重启流;我们照做。
//!
//! # 三个互不相同的故障类别(必须分开计数)
//!
//! | 计数 | 含义 | 听感 |
//! |---|---|---|
//! | `clipped`(yu-sound) | 混音叠加过量,硬削波 | 刺耳、破音 |
//! | `rtsil`(yu-sound) | 源窗口未就绪,整块/逐帧静音 | 断续、块边界咔哒 |
//! | `short`(本模块) | 环形缓冲被抽干,回调补静音 | 卡顿、爆音 |
//!
//! `short` 只在**交付侧**产生;`rtsil` 只在**混音侧**产生。两者都为 0
//! 而仍听到杂音,故障就在 oboe 之后(设备驱动/模拟器转发)。
//!
//! # 纯音自测(`set_tone`)
//!
//! 打开后 mixer 线程不再问引擎,直接往环里灌 1kHz 正弦。若此时仍爆音,
//! 故障 100% 在「环 → oboe → 系统」这段。

// 无锁 SPSC 环形缓冲需要裸指针写入(标准库没有现成实现)。安全性论证见
// `SpscRing` 的字段注释与 `unsafe impl` 上方说明:crate 级是 `deny(unsafe_code)`,
// 这里局部放宽,与 `present.rs`(ANativeWindow FFI)同一纪律。
#![allow(unsafe_code)]

use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use yu_engine::engine::AudioOut;
use yu_sound::SoundState;

/// 可热替换的状态槽:restart 时换入新 `SoundState`,不重开设备。
type Slot = Arc<Mutex<Option<Arc<Mutex<SoundState>>>>>;

/// 目标缓冲水位(毫秒)。
///
/// 要盖住三件事:mixer 线程被调度延迟、VM tick 线程持锁解码(最长几十毫秒)、
/// 渲染线程抢占 CPU。视觉小说对音画同步不敏感,几百毫秒延迟完全无感,
/// 而水位不足换来的就是**卡顿**。宁可多留余量。
const TARGET_MS: u64 = 300;
/// mixer 线程单次最多混多少毫秒(限制单次持锁时长:60ms 音频≈一万多帧,
/// 渲染本身是毫秒级;真正耗时的是混音循环,持锁过久会挡住 tick 线程)。
const BURST_MS: u64 = 60;
/// 环形缓冲容量(毫秒)。给到目标的 3 倍,吸收尖峰。
const RING_MS: u64 = 900;
/// 环形缓冲按此采样率预留(设备可能上报 44.1k/48k 两者)。
const RING_RATE: u32 = 48_000;
/// 设备轮询间隔:Detect 路由切换/设备断开(kira 用 500ms,这里更紧一点)。
const DEVICE_POLL: Duration = Duration::from_millis(300);
/// 自测正弦频率与幅度(纯音通道)。
const TONE_HZ: f64 = 1000.0;
const TONE_AMP: f32 = 0.25;

fn current(slot: &Slot) -> Option<Arc<Mutex<SoundState>>> {
    slot.lock().ok().and_then(|slot| slot.clone())
}

// ---------------------------------------------------------------- 设备侧计量

/// 回调侧的真实交付统计。
///
/// 全部是原子量:回调线程只做 `fetch_add` / `fetch_min` / `fetch_max`,
/// 不分配、不拿锁,因此不会拖慢实时线程。
struct DevStats {
    origin: Instant,
    callbacks: AtomicU64,
    total_callbacks: AtomicU64,
    frames: AtomicU64,
    /// 回调请求但环里没有、由我们补静音的样本数 —— **设备侧真实断供**。
    short_samples: AtomicU64,
    /// 相邻回调的时间间隔(微秒)最大值。抖动大 = 设备调度不稳。
    gap_max_us: AtomicU64,
    /// 回调入口时环内存量(样本)的最小值 = 最险的水位。
    level_min: AtomicU64,
    last_at_us: AtomicU64,
    /// 流重建次数(路由切换/设备断开的直接证据)。
    restarts: AtomicU64,
}

impl DevStats {
    fn new() -> Self {
        Self {
            origin: Instant::now(),
            callbacks: AtomicU64::new(0),
            total_callbacks: AtomicU64::new(0),
            frames: AtomicU64::new(0),
            short_samples: AtomicU64::new(0),
            gap_max_us: AtomicU64::new(0),
            level_min: AtomicU64::new(u64::MAX),
            last_at_us: AtomicU64::new(0),
            restarts: AtomicU64::new(0),
        }
    }

    /// 累计回调次数(看门狗用;`callbacks` 本身按窗口清零)。
    fn callbacks_total(&self) -> u64 {
        self.total_callbacks.load(Ordering::Relaxed)
    }

    /// 回调入口调用一次(实时线程,只做原子操作)。
    fn on_block(&self, requested: usize, delivered: usize, level: usize) {
        self.callbacks.fetch_add(1, Ordering::Relaxed);
        self.total_callbacks.fetch_add(1, Ordering::Relaxed);
        self.frames.fetch_add(requested as u64, Ordering::Relaxed);
        if delivered < requested {
            self.short_samples
                .fetch_add((requested - delivered) as u64, Ordering::Relaxed);
        }
        self.level_min.fetch_min(level as u64, Ordering::Relaxed);
        let now = self.origin.elapsed().as_micros() as u64;
        let last = self.last_at_us.swap(now, Ordering::Relaxed);
        // last == 0 是首块(未初始化),不参与间隔统计。
        if last != 0 {
            self.gap_max_us
                .fetch_max(now.saturating_sub(last), Ordering::Relaxed);
        }
    }
}

// ---------------------------------------------------------------- 无锁环形缓冲

/// 单生产者/单消费者的交错 f32 环形缓冲。
///
/// 只有原子读写,回调线程**永远不会等待**:空间/数据不足时返回实际处理量,
/// 由调用方处理剩余部分(生产侧下轮再填,消费侧补静音)。
struct SpscRing {
    /// 容量为 2 的幂,索引用掩码取模。
    ///
    /// 必须用 `UnsafeCell`:单生产者会经由 `&self` 写入自己独占的区段。
    /// 直接对 `Box<[f32]>` 做 `as_ptr() as *mut` 是**未定义行为** —— 编译器
    /// 可以假设 `&self` 背后不变,把读写优化错,表现为音频数据损坏/爆音。
    buf: Box<[std::cell::UnsafeCell<f32>]>,
    mask: usize,
    /// 累计写入/读出的样本数(单调递增,差值 = 当前存量)。
    written: AtomicU64,
    read: AtomicU64,
}

impl SpscRing {
    fn new(capacity_samples: usize) -> Self {
        let capacity = capacity_samples.next_power_of_two().max(1024);
        Self {
            buf: (0..capacity)
                .map(|_| std::cell::UnsafeCell::new(0.0f32))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            mask: capacity - 1,
            written: AtomicU64::new(0),
            read: AtomicU64::new(0),
        }
    }

    fn capacity(&self) -> usize {
        self.buf.len()
    }

    /// 当前存量(样本数)。
    fn level(&self) -> usize {
        let written = self.written.load(Ordering::Acquire);
        let read = self.read.load(Ordering::Acquire);
        written.saturating_sub(read) as usize
    }

    /// 生产:写入 `samples`,返回实际写入数(空间不足时截断)。
    fn push(&self, samples: &[f32]) -> usize {
        let written = self.written.load(Ordering::Relaxed);
        let read = self.read.load(Ordering::Acquire);
        let free = self.capacity() - written.saturating_sub(read) as usize;
        let take = samples.len().min(free);
        if take == 0 {
            return 0;
        }
        let mask = self.mask;
        let base = written as usize;
        for (i, sample) in samples[..take].iter().enumerate() {
            // 生产侧独占 `[written, written+take)`,消费侧只读 `[read, written)`,
            // 两段不相交 ⇒ 无数据竞争。
            let cell = &self.buf[(base + i) & mask];
            // SAFETY: 该槽位归生产侧独占(见上);`UnsafeCell` 允许经共享引用写入。
            unsafe { *cell.get() = *sample };
        }
        self.written.store(written + take as u64, Ordering::Release);
        take
    }

    /// 丢弃全部存量(重建流/换状态时调用,避免旧音频串进新局面)。
    fn discard_all(&self) {
        let written = self.written.load(Ordering::Acquire);
        self.read.store(written, Ordering::Release);
    }

    /// 消费:读出 `out.len()` 个样本,不足部分补静音。返回实际读到的样本数。
    fn pop_into(&self, out: &mut [f32]) -> usize {
        let read = self.read.load(Ordering::Relaxed);
        let written = self.written.load(Ordering::Acquire);
        let available = written.saturating_sub(read) as usize;
        let take = out.len().min(available);
        let mask = self.mask;
        let base = read as usize;
        for (i, slot) in out[..take].iter_mut().enumerate() {
            let cell = &self.buf[(base + i) & mask];
            // SAFETY: 该槽位在 `[read, written)` 内,只归消费侧读;生产侧此时
            // 不会碰它(它只写 `>= written` 的区段)。
            *slot = unsafe { *cell.get() };
        }
        if take < out.len() {
            // 欠载:补静音。绝不阻塞回调(阻塞 = 爆音)。
            out[take..].fill(0.0);
        }
        self.read.store(read + take as u64, Ordering::Release);
        take
    }
}

// `SpscRing` 的并发安全性:
// - `written` 只由生产侧写,`read` 只由消费侧写(单写单读);
// - 生产侧只访问 `[written, written+take)`,消费侧只访问 `[read, written)`,
//   两个区段不相交(容量足够容纳未消费数据),因此对 `buf` 的访问天然分区;
// - 用 `Release/Acquire` 建立「先写数据、后发布索引」的可见性顺序。
unsafe impl Send for SpscRing {}
unsafe impl Sync for SpscRing {}

// ---------------------------------------------------------------- 共享句柄

/// mixer 线程、音频监督线程、回调三方共享的一组句柄。
struct Shared {
    slot: Slot,
    ring: Arc<SpscRing>,
    running: Arc<AtomicBool>,
    tone: Arc<AtomicBool>,
    stats: Arc<DevStats>,
    first: Arc<AtomicBool>,
    /// 当前流的采样率/声道(监督线程重开流后更新,mixer 每轮读取)。
    rate: Arc<AtomicU32>,
    channels: Arc<AtomicU16>,
    /// 回调报错后置位,监督线程据此重建流。
    restart: Arc<AtomicBool>,
}

// ---------------------------------------------------------------- 设备

/// Android 音频输出设备。
///
/// 内部 `Arc`:引擎侧拿 [`SoundHandle`] 去挂/摘状态;宿主侧保留本体以持有
/// 音频监督线程的存活信号(drop 即停声、停线程)。
pub struct SoundSink {
    inner: Arc<SinkInner>,
}

struct SinkInner {
    slot: Slot,
    /// 监督线程的退出信号(drop 即停止)。
    running: Arc<AtomicBool>,
    /// 自测纯音开关(见模块文档)。
    tone: Arc<AtomicBool>,
    /// 设备侧交付统计。
    stats: Arc<DevStats>,
    rate: Arc<AtomicU32>,
    channels: Arc<AtomicU16>,
}

impl Drop for SinkInner {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
    }
}

/// 交给引擎的 [`AudioOut`] 句柄(只操作槽位,不持有设备/线程)。
pub struct SoundHandle {
    slot: Slot,
}

impl AudioOut for SoundHandle {
    fn attach(&mut self, state: Arc<Mutex<SoundState>>) {
        // 外部时钟:播放位置由 mixer 线程按真实交付的帧数推进。
        if let Ok(mut guard) = state.lock() {
            guard.set_external_clock(true);
        }
        if let Ok(mut slot) = self.slot.lock() {
            *slot = Some(state);
        }
    }

    fn detach(&mut self) {
        if let Ok(mut slot) = self.slot.lock() {
            *slot = None;
        }
    }
}

impl SoundSink {
    /// 打开默认输出设备并启动 mixer 线程。失败返回错误文本。
    pub fn open() -> Result<Self, String> {
        Self::open_with_rate(None)
    }

    /// `force_rate` = `Some(hz)` 时优先请求该采样率(诊断/兼容用);
    /// 设备不支持则回落到默认配置。
    pub fn open_with_rate(force_rate: Option<u32>) -> Result<Self, String> {
        let channels = 2u16;
        let shared = Shared {
            slot: Arc::new(Mutex::new(None)),
            ring: Arc::new(SpscRing::new(
                (u64::from(RING_RATE) * RING_MS / 1000) as usize * usize::from(channels),
            )),
            running: Arc::new(AtomicBool::new(true)),
            tone: Arc::new(AtomicBool::new(false)),
            stats: Arc::new(DevStats::new()),
            first: Arc::new(AtomicBool::new(false)),
            rate: Arc::new(AtomicU32::new(44_100)),
            channels: Arc::new(AtomicU16::new(channels)),
            restart: Arc::new(AtomicBool::new(false)),
        };

        // 监督线程负责建流/重开流;首次建流结果同步回报,保证 `open()`
        // 的失败语义与直连设备时一致(不静默降级成"没声音")。
        let (tx, rx) = std::sync::mpsc::channel();
        spawn_audio_thread(shared.clone_shallow(), force_rate, tx);
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                shared.running.store(false, Ordering::Release);
                return Err(error);
            }
            Err(_) => {
                shared.running.store(false, Ordering::Release);
                return Err("音频流初始化超时(5s)".to_string());
            }
        }

        spawn_mixer(shared.clone_shallow());

        Ok(Self {
            inner: Arc::new(SinkInner {
                slot: shared.slot,
                running: shared.running,
                tone: shared.tone,
                stats: shared.stats,
                rate: shared.rate,
                channels: shared.channels,
            }),
        })
    }

    /// 输出设备摘要(`rate/channels`;诊断用)。
    #[must_use]
    pub fn describe(&self) -> String {
        format!(
            "{}Hz {}ch",
            self.inner.rate.load(Ordering::Relaxed),
            self.inner.channels.load(Ordering::Relaxed)
        )
    }

    /// 设备侧交付摘要(**读取即清窗**:数值 = 上次状态行以来的一秒)。
    ///
    /// 累积计数会把一次启动抖动永远钉在某个数上,掩盖运行中真正的断供;
    /// 改成窗口增量后,任何一秒只要 `short>0` 就是一次真实供数中断。
    #[must_use]
    pub fn stats_line(&self) -> String {
        let s = &self.inner.stats;
        let cbs = s.callbacks.swap(0, Ordering::Relaxed);
        let frames = s.frames.swap(0, Ordering::Relaxed);
        let short = s.short_samples.swap(0, Ordering::Relaxed);
        let gap_max = s.gap_max_us.swap(0, Ordering::Relaxed);
        let lvl_min = s.level_min.swap(u64::MAX, Ordering::Relaxed);
        let restarts = s.restarts.load(Ordering::Relaxed);
        let ch = u64::from(self.inner.channels.load(Ordering::Relaxed)).max(1);
        let rate = u64::from(self.inner.rate.load(Ordering::Relaxed)).max(1);
        // 样本数 → 毫秒(便于直接对照目标水位与听感)。
        let to_ms = |samples: u64| samples as f64 / (rate * ch) as f64 * 1000.0;
        let lvl_min = if lvl_min == u64::MAX { 0 } else { lvl_min };
        format!(
            "short={:.1}ms cbs={} out={:.0}ms lvlmin={:.1}ms gapmax={:.1}ms re={} tone={}",
            to_ms(short),
            cbs,
            to_ms(frames),
            to_ms(lvl_min),
            gap_max as f64 / 1000.0,
            restarts,
            if self.inner.tone.load(Ordering::Relaxed) {
                "on"
            } else {
                "off"
            }
        )
    }

    /// 打开/关闭纯音自测(见模块文档)。开关在 mixer 线程下一轮生效。
    pub fn set_tone(&self, on: bool) {
        self.inner.tone.store(on, Ordering::Release);
        if on {
            log::info!(
                target: "yu-android",
                "[sound] 纯音自测开启:{}Hz amp={TONE_AMP}(引擎输出被旁路)",
                TONE_HZ
            );
        }
    }

    /// 交给引擎的句柄(与本体共享同一槽位)。
    #[must_use]
    pub fn handle(&self) -> SoundHandle {
        SoundHandle {
            slot: Arc::clone(&self.inner.slot),
        }
    }
}

impl Shared {
    fn clone_shallow(&self) -> Shared {
        Shared {
            slot: Arc::clone(&self.slot),
            ring: Arc::clone(&self.ring),
            running: Arc::clone(&self.running),
            tone: Arc::clone(&self.tone),
            stats: Arc::clone(&self.stats),
            first: Arc::clone(&self.first),
            rate: Arc::clone(&self.rate),
            channels: Arc::clone(&self.channels),
            restart: Arc::clone(&self.restart),
        }
    }
}

// ---------------------------------------------------------------- 音频监督线程

/// 设备配置指纹:任一项变化都要重开流(与 kira `check_stream` 同口径)。
#[derive(Clone, PartialEq, Eq)]
struct DeviceSignature {
    name: String,
    rate: u32,
    channels: u16,
    format: cpal::SampleFormat,
}

fn describe_signature(sig: &DeviceSignature) -> String {
    format!(
        "device={} rate={} ch={} fmt={:?}",
        sig.name, sig.rate, sig.channels, sig.format
    )
}

/// 查询默认输出设备及其目标配置。
///
/// `force_rate` 只在设备确实支持该采样率时生效(oboe 的 supported configs
/// 是逐个采样率的枚举,不支持就直接回落,不做静默重采样)。
fn query_device(
    force_rate: Option<u32>,
) -> Result<(cpal::Device, cpal::StreamConfig, DeviceSignature), String> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| "没有默认输出设备".to_string())?;
    let supported = device
        .default_output_config()
        .map_err(|e| format!("默认输出配置不可用: {e}"))?;
    let format = supported.sample_format();
    let channels = supported.channels();
    let mut config: cpal::StreamConfig = supported.into();

    if let Some(want) = force_rate {
        if want != config.sample_rate.0 {
            if let Ok(ranges) = device.supported_output_configs() {
                for range in ranges {
                    if range.channels() == channels
                        && range.sample_format() == format
                        && range.min_sample_rate().0 <= want
                        && want <= range.max_sample_rate().0
                    {
                        config.sample_rate = cpal::SampleRate(want);
                        break;
                    }
                }
            }
        }
    }
    // 缓冲大小交给平台决定:oboe 要求固定缓冲是**设备周期的整数倍**,
    // 不匹配时它内部拉伸/补齐,本身就是一个爆音源。
    config.buffer_size = cpal::BufferSize::Default;

    // Android 的默认设备 `name()` 恒为 "default",所以采样率/声道才是
    // 路由变化(耳机↔扬声器、蓝牙)的实际指纹。
    let name = device.name().unwrap_or_else(|_| "default".to_string());
    let signature = DeviceSignature {
        name,
        rate: config.sample_rate.0,
        channels,
        format,
    };
    Ok((device, config, signature))
}

/// 建一条输出流(回调零锁、零分配)。
fn build_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    format: cpal::SampleFormat,
    shared: &Shared,
) -> Result<cpal::Stream, String> {
    let ring = Arc::clone(&shared.ring);
    let stats = Arc::clone(&shared.stats);
    let first = Arc::clone(&shared.first);
    // 流错误(设备断开/路由切换)必须让监督者知道:只记日志等于永远静音。
    let restart = Arc::clone(&shared.restart_flag());
    let err_fn = move |err: cpal::StreamError| {
        log::warn!(target: "yu-sound", "[sound] 设备流错误(将重建): {err}");
        restart.store(true, Ordering::Release);
    };

    let channels = usize::from(config.channels);
    let rate = config.sample_rate.0;
    // 中转缓冲一次性预分配到足够大:回调里不再扩容。
    // 实时线程上做堆分配可能触发全局分配器锁/缺页,是爆音的经典来源。
    let scratch_capacity = (u64::from(rate) / 2) as usize * channels;

    let stream = match format {
        // f32 直出:零中转、零分配。
        cpal::SampleFormat::F32 => device.build_output_stream(
            config,
            move |data: &mut [f32], _| {
                // 实时线程:无锁拷贝;不分配、不拿锁、不解码。
                let level = ring.level();
                let take = ring.pop_into(data);
                stats.on_block(data.len(), take, level);
                note_first_audio(&first, data);
            },
            err_fn,
            None,
        ),
        // 整型输出:用预分配中转缓冲(长度固定,不扩容)。
        cpal::SampleFormat::I16 => {
            let mut scratch = vec![0.0f32; scratch_capacity];
            device.build_output_stream(
                config,
                move |data: &mut [i16], _| {
                    let cap = scratch.len().min(data.len());
                    let (level, take) = {
                        let buf = &mut scratch[..cap];
                        let level = ring.level();
                        let take = ring.pop_into(buf);
                        for (out, sample) in data.iter_mut().zip(buf.iter()) {
                            *out = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
                        }
                        (level, take)
                    };
                    stats.on_block(data.len(), take, level);
                    for out in data[cap..].iter_mut() {
                        *out = 0;
                    }
                    note_first_audio(&first, &scratch[..cap]);
                },
                err_fn,
                None,
            )
        }
        cpal::SampleFormat::U16 => {
            let mut scratch = vec![0.0f32; scratch_capacity];
            device.build_output_stream(
                config,
                move |data: &mut [u16], _| {
                    let cap = scratch.len().min(data.len());
                    let (level, take) = {
                        let buf = &mut scratch[..cap];
                        let level = ring.level();
                        let take = ring.pop_into(buf);
                        for (out, sample) in data.iter_mut().zip(buf.iter()) {
                            *out = (sample.clamp(-1.0, 1.0) * 32767.0 + 32768.0) as u16;
                        }
                        (level, take)
                    };
                    stats.on_block(data.len(), take, level);
                    for out in data[cap..].iter_mut() {
                        *out = 32768;
                    }
                    note_first_audio(&first, &scratch[..cap]);
                },
                err_fn,
                None,
            )
        }
        other => return Err(format!("默认输出格式 {other:?} 未接入(支持 f32/i16/u16)")),
    }
    .map_err(|e| format!("创建输出流失败: {e}"))?;
    stream.play().map_err(|e| format!("启动输出流失败: {e}"))?;
    Ok(stream)
}

/// 音频监督线程:建流、轮询设备变化、必要时重建。
///
/// 这是本模块最关键的一处工程细节 —— AAudio 在音频路由切换(插拔耳机、
/// 蓝牙连接)或设备断开时会**关闭流**,此后回调不再触发。不重建就永远
/// 没声音,而且表现为"突然静音"而不是报错。
///
/// 首次建流结果通过 `ready` 同步回报给 [`SoundSink::open`]。
fn spawn_audio_thread(
    shared: Shared,
    force_rate: Option<u32>,
    ready: std::sync::mpsc::Sender<Result<(), String>>,
) {
    let _ = std::thread::Builder::new()
        .name("yu-audio-out".to_string())
        .spawn(move || {
            raise_thread_priority();
            let mut stream: Option<cpal::Stream> = None;
            let mut signature: Option<DeviceSignature> = None;
            let mut reported = false;
            let mut warned = false;
            // 看门狗:AAudio 有时**不报错**就停止回调(路由切换的边角情况),
            // 表现是"突然没声音"。只要流还在跑、红线内回调数不增长,就重建。
            let mut last_callbacks = 0u64;
            let mut stale_ticks = 0u32;

            loop {
                if !shared.running.load(Ordering::Acquire) {
                    break;
                }
                // 回调数在 DEVICE_POLL × 8(≈2.4s)内不动 ⇒ 流已僵死。
                let callbacks = shared.stats.callbacks_total();
                if stream.is_some() && callbacks == last_callbacks {
                    stale_ticks = stale_ticks.saturating_add(1);
                } else {
                    stale_ticks = 0;
                }
                last_callbacks = callbacks;
                let restart = shared.restart_flag().swap(false, Ordering::Acquire)
                    || stale_ticks >= 8;
                if stale_ticks >= 8 {
                    log::warn!(
                        target: "yu-sound",
                        "[sound] 回调 2.4s 未触发,判定流僵死,强制重建"
                    );
                }
                let queried = query_device(force_rate);
                match queried {
                    Ok((device, config, sig)) => {
                        let changed = signature.as_ref() != Some(&sig);
                        if stream.is_none() || changed || restart {
                            // 先放掉旧流再建新的:同一时间只持有一条流
                            // (`take()` 是显式丢弃旧流,不是"赋 None 后立刻覆盖")。
                            drop(stream.take());
                            match build_stream(&device, &config, sig.format, &shared) {
                                Ok(built) => {
                                    shared
                                        .rate
                                        .store(sig.rate, Ordering::Release);
                                    shared
                                        .channels
                                        .store(sig.channels, Ordering::Release);
                                    // 换流等于换时钟:环里的旧数据已无意义。
                                    shared.ring.discard_all();
                                    shared.stats.restarts.fetch_add(1, Ordering::Relaxed);
                                    let rebuilt = signature.is_some();
                                    signature = Some(sig.clone());
                                    stream = Some(built);
                                    log::info!(
                                        target: "yu-android",
                                        "[sound] {}: {}{}",
                                        if rebuilt { "音频流已重建" } else { "音频流就绪" },
                                        describe_signature(&sig),
                                        if rebuilt { "(路由变化/设备断开)" } else { "" }
                                    );
                                    if !reported {
                                        reported = true;
                                        let _ = ready.send(Ok(()));
                                    }
                                }
                                Err(error) => {
                                    stream = None;
                                    log::warn!(
                                        target: "yu-sound",
                                        "[sound] 建流失败(300ms 后重试): {error}"
                                    );
                                    if !reported {
                                        reported = true;
                                        let _ = ready.send(Err(error));
                                        // 首次就失败:监督线程继续轮询,后面若
                                        // 设备恢复会自动建起来(不把宿主卡死)。
                                    }
                                }
                            }
                        }
                        warned = false;
                    }
                    Err(error) => {
                        // 设备查询失败通常是瞬时的(路由切换中)。首次失败要
                        // 同步回报给 `open()`;之后只记一条日志避免刷屏。
                        if !reported {
                            reported = true;
                            let _ = ready.send(Err(error.clone()));
                        }
                        if !warned {
                            warned = true;
                            log::warn!(target: "yu-sound", "[sound] 设备查询失败: {error}");
                        }
                        stream = None;
                        signature = None;
                    }
                }
                std::thread::sleep(DEVICE_POLL);
            }
            drop(stream);
            log::info!(target: "yu-android", "[sound] 音频监督线程退出");
        });
}

impl Shared {
    /// 回调侧拿到的「请重建流」信号(与监督线程共享)。
    fn restart_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.restart)
    }
}

// ---------------------------------------------------------------- mixer 线程

/// mixer 线程主体:把 `SoundState` 的混音结果填进环形缓冲。
///
/// 只有这里调用 `render_and_advance`,所以「渲染用的位置」与「推进后的位置」
/// 是同一次调用的前后态,不会被并发的消费侧打断(这是早期 bug 的教训)。
fn spawn_mixer(shared: Shared) {
    let _ = std::thread::Builder::new()
        .name("yu-audio-mix".to_string())
        .spawn(move || {
            // 普通优先级会被渲染/VM 线程抢占;安卓上必须显式提到音频优先级,
            // 否则一次长帧就可能把环抽干(nice 值无法跨进程生效,走 JNI)。
            raise_thread_priority();

            let mut scratch =
                vec![0.0f32; (u64::from(RING_RATE) * BURST_MS / 1000) as usize * 2];
            let mut last_state_id: usize = 0;
            let mut tone_phase: f64 = 0.0;
            let mut tone_rate = 0u32;

            while shared.running.load(Ordering::Acquire) {
                let rate = shared.rate.load(Ordering::Acquire).max(1);
                let channels = usize::from(shared.channels.load(Ordering::Acquire).max(1));
                if rate != tone_rate {
                    tone_rate = rate;
                    tone_phase = 0.0;
                }
                let tone_step = TONE_HZ / f64::from(rate);
                let target_samples = (u64::from(rate) * TARGET_MS / 1000) as usize * channels;
                let burst_samples = (u64::from(rate) * BURST_MS / 1000) as usize * channels;

                // 迟滞:水位没掉到「目标 − 2 块」之前不补货。
                //
                // 早先每 2ms 醒一次、每次只补 2ms,等于每秒上千次唤醒,
                // 在本来就被 GPU/VM 抢满 CPU 的安卓上(尤其模拟器)纯粹是
                // 添乱;而且一有停顿就断供。批量补满后,唤醒频率降到 1/20。
                let level = shared.ring.level();
                let low_water = target_samples.saturating_sub(burst_samples);
                if level > low_water {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }

                // 只混「能立刻写进环」的量:否则推进的位置与实际交付不符。
                // 一次补到目标水位(受单块上限与环内空间约束)。
                let free = shared.ring.capacity().saturating_sub(level);
                let want = (target_samples - level).min(free).min(burst_samples);
                let frames = want / channels;
                if frames == 0 {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }
                let samples = frames * channels;
                // 设备采样率/声道数不是固定 48k/2:cpal 在桌面可能给出
                // 96k/4ch,`burst_samples` 会超过按 `RING_RATE`/2ch 预留的
                // scratch。mixer 线程不是实时回调,扩容安全;只增不减,
                // 避免每次换流都重新分配。
                if scratch.len() < samples {
                    scratch.resize(samples, 0.0);
                }

                if shared.tone.load(Ordering::Acquire) {
                    // 自测通道:完全不碰引擎,直接灌正弦。
                    for f in 0..frames {
                        let v = (tone_phase * std::f64::consts::TAU).sin() as f32 * TONE_AMP;
                        tone_phase += tone_step;
                        if tone_phase >= 1.0 {
                            tone_phase -= 1.0;
                        }
                        for c in 0..channels {
                            scratch[f * channels + c] = v;
                        }
                    }
                    shared.ring.push(&scratch[..samples]);
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }

                // 未挂载状态(启动/重启间隙):不生产,让设备播静音。
                let Some(state) = current(&shared.slot) else {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                };

                // restart 换了新 SoundState:清掉旧数据,避免串音。
                let state_id = Arc::as_ptr(&state) as usize;
                if state_id != last_state_id {
                    last_state_id = state_id;
                    shared.ring.discard_all();
                }

                let started = Instant::now();
                {
                    let mut guard = match state.lock() {
                        Ok(guard) => guard,
                        Err(poisoned) => poisoned.into_inner(),
                    };
                    guard.render_and_advance(&mut scratch[..samples], rate, channels);
                }
                shared.ring.push(&scratch[..samples]);

                // 单轮过短就小睡,避免空转把 CPU 吃满(也减少与 tick 抢锁)。
                if started.elapsed() < Duration::from_millis(1) {
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        });
}

/// 把当前线程提到 Android 的音频优先级(`THREAD_PRIORITY_AUDIO = -16`)。
///
/// Java 侧跑在应用进程里,`android.os.Process.setThreadPriority` 无需额外权限;
/// 走 JNI 是因为 Rust 创建的线程不受 `Process.setThreadPriority` 管,只能自己
/// 调。失败只记一条日志,不影响功能(只是抗抢占能力差一些)。
#[cfg(target_os = "android")]
fn raise_thread_priority() {
    let ctx = ndk_context::android_context();
    // SAFETY: `vm()` 是 Java 侧在 `nativeInitAndroidContext` 交进来的
    // JavaVM 指针,进程生命周期内有效。
    let vm = match unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) } {
        Ok(vm) => vm,
        Err(e) => {
            log::warn!(target: "yu-android", "[sound] 取 JavaVM 失败,跳过线程提权: {e}");
            return;
        }
    };
    let mut env = match vm.attach_current_thread() {
        Ok(env) => env,
        Err(e) => {
            log::warn!(target: "yu-android", "[sound] attach 音频线程失败: {e}");
            return;
        }
    };
    let result = (|| -> jni::errors::Result<()> {
        let class = env.find_class("android/os/Process")?;
        env.call_static_method(
            class,
            "setThreadPriority",
            "(I)V",
            &[jni::objects::JValue::Int(-16)],
        )?;
        Ok(())
    })();
    // 提到优先级后不再 detach:线程长期存活,detach 反而会拖慢退出。
    match result {
        Ok(()) => log::info!(
            target: "yu-android",
            "[sound] mixer 线程优先级已提升到 THREAD_PRIORITY_AUDIO"
        ),
        Err(e) => log::warn!(
            target: "yu-android",
            "[sound] 线程提权失败(继续用普通优先级): {e}"
        ),
    }
}

/// 非 Android 目标(本 crate 的 rlib 供宿主测试/示例链接):无提权概念。
#[cfg(not(target_os = "android"))]
fn raise_thread_priority() {}

/// 首次出现非静音时留一条日志(排查"到底有没有出声")。
fn note_first_audio(first: &AtomicBool, data: &[f32]) {
    if first.load(Ordering::Relaxed) {
        return;
    }
    let peak = data.iter().fold(0.0f32, |a, &b| a.max(b.abs()));
    if peak > 0.0 && !first.swap(true, Ordering::Relaxed) {
        log::info!(target: "yu-android", "[sound] first non-silent block: peak={peak:.4}");
    }
}
