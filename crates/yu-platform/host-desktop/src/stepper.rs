//! debug-ui 帧步进推进器:暂停 / 推进 N 帧 / 录制 / 抓取证据。
//!
//! 为什么要有它:眨眼这类现场只出现在**中间某几帧**(kotoneiro 的立绘
//! 合成在第 348、465 帧),实时跑根本按不住,靠人眼瞄 + 截图既不可复现
//! 也拿不到后端内部读数。步进器把「停住 → 定帧推进 → 把这一帧的全部
//! 证据落盘」做成宿主能力。
//!
//! # 两个暂停
//!
//! 1. **录制暂停**(`record_pause`):点「录制」后自动进入。录制的目的是
//!    取现场,所以一开始录就把时钟停住,由主人决定推进到哪一帧;
//! 2. **游戏暂停**(`paused`):面板「暂停」按钮 / F5。与录制无关,随时可用。
//!
//! 两者**任一为真**引擎就不推进;`暂停=` 读数把两者分开显示,不混成
//! 一个布尔(否则「为什么停着」不可解释)。
//!
//! 暂停期间**逻辑帧号不涨、逻辑时钟不走、`--frames` 口径不变**;推进用
//! 固定步长([`yu_engine::engine::Engine::tick`],每帧 `1000/fps` 逻辑
//! 毫秒)而不是墙钟 —— 同一脚本在任何机器上的第 348 帧是同一个画面,
//! 这正是复现中间帧需要的可复现性。恢复实时时宿主调用
//! `reset_realtime_clock`,暂停的那段墙钟不会被一次性灌进下一帧。
//!
//! # 证据(「有效信息都需要抓取」)
//!
//! 每帧一条 JSONL(`frames.jsonl`),一行一帧、键序稳定:
//!
//! | 段 | 内容 |
//! |---|---|
//! | 顶层 | `host_frame` / `engine_frame` / `gfx_frame` / `clock_ms` / `interval` / `tick` / `render` / `draw` |
//! | `frame_report` | `FrameReport`:ops/drawn/skipped/passes/layers/visible/drawable/lag |
//! | `render_report` | 跳过项(kind:reason×count)、缺资产、文本计数与失败原因、滤镜口径 |
//! | `gfx_diags` | gfx 帧诊断(码/严重级/命令/detail/count/脚本位置 `at`) |
//! | `diags` | 本帧新挂号(码/严重级/位置/detail) |
//! | `input` | 按住集合、光标、**本帧按下沿增量**、累计点击/滚轮 |
//! | `sound` | 声音层快照(id/file/state/ms/total/volume/paused) |
//! | `ops_top` | 本帧 op 目标层 top-N(哪几个层在吃 op) |
//! | `gfx_gpu` | GPU 目标占用/计数/对账(soft 后端为 `null`,不猜) |
//! | `render_phases` | wgpu 规划/图像/文本/编码/读回各阶段微秒 |
//! | `screen` | 宽高 + 采样指纹 + PNG 文件名 |
//! | `display` | 合成帧 JSON(可选,体积大,默认关) |
//!
//! # 落盘策略
//!
//! - **推进的帧**:每帧立即写(JSONL + PNG)。推进本身就是抓取动作。
//! - **录制中的实时帧**:进环形缓冲(默认保留最近
//!   [`DEFAULT_RING_FRAMES`] 帧,含整屏 RGBA),点「抓取」才回溯落盘 ——
//!   人眼看到问题再按键,证据不会因为晚按了一秒就没了。
//! - 退出时(`finish`)把环形缓冲里还没落盘的一并写出,不丢现场。

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use yu_engine::engine::Engine;
use yu_engine::json::{self, Value};
use yu_render_core::surface::Surface;

use crate::diagnostics::Diagnostics;

/// 「推进 N 帧」的默认 N。
pub const DEFAULT_STEP_FRAMES: u32 = 10;
/// 单次推进的硬上限:手滑输 100000 不该把宿主卡死。
pub const MAX_STEP_FRAMES: u32 = 600;
/// 环形缓冲默认保留帧数(只在录制时占用内存;每帧一张整屏 RGBA)。
pub const DEFAULT_RING_FRAMES: usize = 60;
/// 环形缓冲的**字节预算**上限(每帧一张整屏 RGBA,1080p 约 8 MB)。
///
/// `ring_frames` 允许配到 600,但那在 1080p 下是 ~5 GB —— 面板可调不等于
/// 应该无上限吃内存。超过预算时按最旧的先丢,`ring_len` 会小于设定值,
/// 面板上的实际帧数即真实驻留量。
pub const RING_BYTE_BUDGET: usize = 512 * 1024 * 1024;
/// 录制开始时写入的会话头文件名。
pub const SESSION_FILE: &str = "session.json";
/// 帧记录流(JSONL:一行一帧)。
pub const FRAMES_FILE: &str = "frames.jsonl";

/// 本帧该不该推进引擎。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TickKind {
    /// 正常实时推进(墙钟)。
    #[default]
    Live,
    /// 消耗一次步进:固定步长推进一帧。
    Step,
    /// 暂停:不推进引擎,只重绘最后一帧。
    Halt,
}

impl TickKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Step => "step",
            Self::Halt => "halt",
        }
    }

    pub fn advances(self) -> bool {
        !matches!(self, Self::Halt)
    }
}

/// 本帧的宿主侧耗时(毫秒)。
///
/// 引擎侧 `tick_ms`/`render_ms` 不从宿主传:它们与 `FrameReport` 同源,
/// 在 [`Stepper::build_record`] 里直接读 —— 否则暂停帧会因宿主传 0 而
/// 把「上一 tick 实际耗时」抹成 0,读起来像引擎没干活。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Timings {
    pub interval_ms: f64,
    pub draw_ms: f64,
}

/// 一帧记录的产生方式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 实时推进中被录制进环形缓冲。
    Live,
    /// 「推进 N 帧」产生的一帧(立即落盘)。
    Step,
}

impl Phase {
    fn as_str(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Step => "step",
        }
    }
}

/// 面板每帧读写的步进器视图(值语义;egui 改完由 [`Stepper::apply_ui`] 回写)。
#[derive(Clone, Debug, Default)]
pub struct StepperUi {
    pub tick_kind: TickKind,
    /// 手动暂停(F5 / 按钮)。
    pub paused: bool,
    /// 录制开始后的自动暂停。
    pub record_pause: bool,
    pub recording: bool,
    /// 「推进 N 帧」的 N。
    pub step_frames: u32,
    /// 本次推进还剩几帧。
    pub pending_steps: u32,
    /// 已落盘帧数(累计)。
    pub captured: u64,
    pub ring_len: usize,
    pub ring_frames: usize,
    pub save_png: bool,
    pub include_display: bool,
    /// 输出目录(可编辑)。
    pub dir: String,
    pub last_file: Option<String>,
    pub last_error: Option<String>,
    pub engine_frame: u64,
    pub clock_ms: u32,
    /// 因画面未变而被跳过的实时帧数(去重生效的证据)。
    pub live_skipped: u64,
}

/// 面板上点出来的动作。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StepperActions {
    pub toggle_pause: bool,
    /// 推进 `step_frames` 帧。
    pub step: bool,
    /// 推进 1 帧。
    pub step_one: bool,
    pub toggle_record: bool,
    /// 显式设置「录制暂停」(`Some(true)` = 开始录制并暂停,`Some(false)` =
    /// 结束录制)。不能用 toggle 代替:`--record` 起跑时 `recording=true`
    /// 但 `record_pause=false`,此时勾上复选框若走 toggle 会把录制**关掉**。
    pub record_pause: Option<bool>,
    /// 把环形缓冲里的帧落盘(实时录制时用)。
    pub capture_now: bool,
    pub clear_ring: bool,
}

/// 步进推进器。
#[derive(Debug)]
pub struct Stepper {
    paused: bool,
    record_pause: bool,
    step_frames: u32,
    pending_steps: u32,
    recording: bool,
    captured: u64,
    dir: PathBuf,
    ring: VecDeque<RingFrame>,
    ring_frames: usize,
    /// `ring` 里全部 surface 的像素字节合计(增量维护,避免每帧 O(n) 重算)。
    ring_bytes: usize,
    save_png: bool,
    include_display: bool,
    /// 上一宿主帧是否处于暂停(用于识别「刚恢复」并重置实时时钟)。
    halted_last: bool,
    resumed: bool,
    /// 面板/快捷键请求「抓取」(在宿主帧边界执行)。
    capture_requested: bool,
    /// 会话头是否已写。
    session_written: bool,
    last_file: Option<String>,
    last_error: Option<String>,
    /// 上一帧各按钮的累计按下沿(用于算本帧增量)。
    last_press_edges: BTreeMap<String, u64>,
    last_clicks: u64,
    last_wheel: u64,
    /// 上一张实时录制帧的画面指纹(相同就整帧跳过,见 `observe`)。
    last_live_fingerprint: u64,
    /// 因画面未变而被跳过的实时帧数(面板显示,证明去重在生效)。
    live_skipped: u64,
}

/// 环形缓冲里的一帧:证据在入环时就冻结,落盘只是写出来。
#[derive(Debug)]
struct RingFrame {
    phase: Phase,
    host_frame: u64,
    /// 整屏像素(`Arc` 与引擎 `last_surface` 共享,不复制)。
    surface: Option<Arc<Surface>>,
    /// 已构建好的一帧记录(不含 `phase`,落盘时插入)。
    record: BTreeMap<String, Value>,
}

impl RingFrame {
    /// 本帧整屏像素占用的字节数(无 surface 时为 0)。
    fn surface_bytes(&self) -> usize {
        self.surface
            .as_ref()
            .map_or(0, |surface| surface.rgba.len())
    }
}

impl Default for Stepper {
    fn default() -> Self {
        Self::new()
    }
}

impl Stepper {
    pub fn new() -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Self {
            paused: false,
            record_pause: false,
            step_frames: DEFAULT_STEP_FRAMES,
            pending_steps: 0,
            recording: false,
            captured: 0,
            dir: PathBuf::from(format!("target/tmp/debug-ui/s{stamp}")),
            ring: VecDeque::new(),
            ring_frames: DEFAULT_RING_FRAMES,
            ring_bytes: 0,
            save_png: true,
            include_display: false,
            halted_last: false,
            resumed: false,
            capture_requested: false,
            session_written: false,
            last_file: None,
            last_error: None,
            last_press_edges: BTreeMap::new(),
            last_clicks: 0,
            last_wheel: 0,
            last_live_fingerprint: 0,
            live_skipped: 0,
        }
    }

    // ------------------------------------------------------------ 状态机

    /// 引擎是否应停住(不看 `pending_steps`)。用于抑制「卡住」横幅。
    pub fn halted(&self) -> bool {
        self.paused || self.record_pause
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    pub fn recording(&self) -> bool {
        self.recording
    }

    pub fn step_frames(&self) -> u32 {
        self.step_frames
    }

    pub fn pending_steps(&self) -> u32 {
        self.pending_steps
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// 宿主每帧问一次:本帧该不该推进引擎。
    ///
    /// 顺带维护「刚恢复」标记(`take_resumed`),供宿主重置实时时钟。
    pub fn tick_kind(&mut self) -> TickKind {
        let kind = if self.pending_steps > 0 {
            TickKind::Step
        } else if self.halted() {
            TickKind::Halt
        } else {
            TickKind::Live
        };
        self.resumed = self.halted_last && kind.advances();
        self.halted_last = kind == TickKind::Halt;
        kind
    }

    /// 本帧是否刚从暂停恢复(取走即清)。
    pub fn take_resumed(&mut self) -> bool {
        std::mem::take(&mut self.resumed)
    }

    /// 一次 tick 完成后调用:consumed 一帧步进。
    pub fn after_tick(&mut self) {
        if self.pending_steps > 0 {
            self.pending_steps -= 1;
        }
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
        if !paused {
            self.pending_steps = 0;
        }
    }

    /// 配置「推进 N 帧」的默认帧数(宿主启动参数;面板里仍可改)。
    pub fn set_step_frames(&mut self, frames: u32) {
        self.step_frames = frames.clamp(1, MAX_STEP_FRAMES);
    }

    /// 配置取证输出目录。
    pub fn set_dir(&mut self, dir: PathBuf) {
        self.dir = dir;
        self.session_written = false;
    }

    pub fn toggle_pause(&mut self) {
        self.set_paused(!self.paused);
    }

    /// 请求推进 `frames` 帧(替换尚未走完的旧请求)。
    ///
    /// **推进必然落在暂停态**:这是调试器的「单步后停住」语义 —— 否则
    /// 推进的帧会在下一宿主帧立刻被实时推进冲走,等于没停。推进期间
    /// 录制的自动暂停仍然成立,推完自动落回暂停。
    pub fn request_step(&mut self, frames: u32) {
        self.paused = true;
        self.pending_steps = frames.clamp(1, MAX_STEP_FRAMES);
    }

    pub fn toggle_record(&mut self) {
        self.set_recording(!self.recording);
    }

    pub fn set_recording(&mut self, on: bool) {
        self.recording = on;
        // 两个暂停:`录制开始` 自动暂停取现场;`停止录制` 撤销它,
        // 但**不动**手动暂停位(那是主人自己的意思)。
        self.record_pause = on;
        if !on {
            self.reset_ring();
            self.session_written = false;
        }
        self.last_live_fingerprint = 0;
    }

    /// 启动时开启录制(`--record`):只开录制,**不进**「录即暂停」。
    ///
    /// 命令行显式要录制 = 「先跑起来,看到问题按 F9 抓」;若在这里顺手
    /// 停住,就永远走不到要抓的那一帧。手动暂停位也不动。
    pub fn start_recording_live(&mut self) {
        self.recording = true;
        self.record_pause = false;
    }

    /// 面板上「录制(自动)」复选框的显式设值(勾上 = 开始录制并暂停,
    /// 取消 = 结束录制)。
    ///
    /// 不能走 [`Stepper::toggle_record`]:`--record` 起跑时 `recording`
    /// 已为真而 `record_pause` 为假,此时勾上复选框若 toggle 会把录制
    /// **关掉** —— 语义正好反了。
    pub fn set_record_pause(&mut self, on: bool) {
        self.set_recording(on);
    }

    /// 当前是否处于「录制自动暂停」。
    pub fn record_paused(&self) -> bool {
        self.record_pause
    }

    /// 请求把环形缓冲落盘(宿主帧边界执行,见 [`Stepper::dump_ring`])。
    pub fn request_capture(&mut self) {
        self.capture_requested = true;
    }

    pub fn take_capture_request(&mut self) -> bool {
        std::mem::take(&mut self.capture_requested)
    }

    pub fn clear_ring(&mut self) {
        self.reset_ring();
    }

    /// 面板读数。
    pub fn ui(&self) -> StepperUi {
        StepperUi {
            tick_kind: if self.pending_steps > 0 {
                TickKind::Step
            } else if self.halted() {
                TickKind::Halt
            } else {
                TickKind::Live
            },
            paused: self.paused,
            record_pause: self.record_pause,
            recording: self.recording,
            step_frames: self.step_frames,
            pending_steps: self.pending_steps,
            captured: self.captured,
            ring_len: self.ring.len(),
            ring_frames: self.ring_frames,
            save_png: self.save_png,
            include_display: self.include_display,
            dir: self.dir.display().to_string(),
            last_file: self.last_file.clone(),
            last_error: self.last_error.clone(),
            engine_frame: 0,
            clock_ms: 0,
            live_skipped: self.live_skipped,
        }
    }

    /// 把面板改过的值写回(只认可编辑项 + 动作位)。
    pub fn apply_ui(&mut self, ui: &StepperUi, actions: StepperActions) {
        self.step_frames = ui.step_frames.clamp(1, MAX_STEP_FRAMES);
        self.save_png = ui.save_png;
        self.include_display = ui.include_display;
        self.ring_frames = ui.ring_frames.clamp(1, 600);
        let dir = ui.dir.trim();
        if !dir.is_empty() && Path::new(dir) != self.dir {
            self.dir = PathBuf::from(dir);
        }
        if actions.toggle_pause {
            self.toggle_pause();
        }
        if actions.step {
            self.request_step(self.step_frames);
        }
        if actions.step_one {
            self.request_step(1);
        }
        if actions.toggle_record {
            self.toggle_record();
        }
        if let Some(on) = actions.record_pause {
            self.set_record_pause(on);
        }
        if actions.capture_now {
            self.request_capture();
        }
        if actions.clear_ring {
            self.clear_ring();
        }
    }

    /// 上一帧是否录制了环形缓冲(退出前决定要不要补写)。
    pub fn has_ring(&self) -> bool {
        !self.ring.is_empty()
    }

    // ---------------------------------------------------------------- 抓取

    /// 一帧结束后调用:按当前模式决定入环 / 立即落盘。
    ///
    /// `phase` 由宿主给(`Step` = 推进帧,`Live` = 实时帧)。
    ///
    /// **实时帧只在画面真的变了才抓**。这不只是省开销:暂停/静止时每宿主
    /// 帧都是同一张图,逐帧构成还完全一样 —— 录 60 张一样的东西既卡又没用。
    /// 而且证据构建(`frame_op_summary` 遍历全部 pass/op、`sound_snapshot`、
    /// `frame_diagnostics`)在 60Hz 下是实打实的开销,录制一开就掉帧。
    ///
    /// 推进帧永远抓(推进本身就是取证动作),不做去重。
    pub fn observe(
        &mut self,
        engine: &Engine,
        diagnostics: &Diagnostics,
        host_frame: u64,
        timings: Timings,
        phase: Phase,
        surface: Option<Arc<Surface>>,
    ) {
        match phase {
            Phase::Live if !self.recording => return,
            Phase::Live | Phase::Step => {}
        }
        if phase == Phase::Live {
            // 指纹先用整屏采样哈希判,画面没变就直接跳过整条证据构建。
            let fingerprint = surface
                .as_ref()
                .map(|surface| crate::diagnostics::fingerprint(&surface.rgba))
                .unwrap_or(0);
            if fingerprint == self.last_live_fingerprint {
                self.live_skipped = self.live_skipped.saturating_add(1);
                return;
            }
            self.last_live_fingerprint = fingerprint;
        }
        let record = self.build_record(engine, diagnostics, host_frame, timings);
        match phase {
            Phase::Step => {
                let mut item = RingFrame {
                    phase,
                    host_frame,
                    surface,
                    record,
                };
                match self.write_frame(&mut item) {
                    Ok(name) => {
                        self.captured = self.captured.saturating_add(1);
                        self.last_file = Some(name);
                    }
                    Err(error) => self.last_error = Some(error),
                }
            }
            Phase::Live => {
                self.push_ring(RingFrame {
                    phase,
                    host_frame,
                    surface,
                    record,
                });
            }
        }
    }

    /// 入环并按 `ring_frames` 与 [`RING_BYTE_BUDGET`] 双上限丢最旧的。
    ///
    /// 帧数与字节数都要卡:只卡帧数时,1080p 的 600 帧能吃掉 ~5 GB;
    /// 只卡字节数时,极小窗口会留过多帧。两者取先到者。
    fn push_ring(&mut self, frame: RingFrame) {
        self.push_ring_budgeted(frame, RING_BYTE_BUDGET);
    }

    /// [`Stepper::push_ring`] 的本体,预算可注入(测试用小预算验证逻辑,
    /// 不必真的分配 512 MiB)。
    fn push_ring_budgeted(&mut self, frame: RingFrame, byte_budget: usize) {
        self.ring_bytes = self.ring_bytes.saturating_add(frame.surface_bytes());
        self.ring.push_back(frame);
        while self.ring.len() > self.ring_frames {
            self.pop_ring_front();
        }
        while self.ring_bytes > byte_budget && self.ring.len() > 1 {
            self.pop_ring_front();
        }
    }

    /// 丢掉最旧的一帧并同步扣减字节计数。
    fn pop_ring_front(&mut self) {
        if let Some(dropped) = self.ring.pop_front() {
            self.ring_bytes = self.ring_bytes.saturating_sub(dropped.surface_bytes());
        }
    }

    /// 清空环形缓冲(字节计数同步归零)。
    fn reset_ring(&mut self) {
        self.ring.clear();
        self.ring_bytes = 0;
    }

    /// 把环形缓冲里的帧全部落盘(最旧的在前)。返回写出的帧数。
    pub fn dump_ring(&mut self) -> Result<usize, String> {
        let frames: Vec<RingFrame> = self.ring.drain(..).collect();
        self.ring_bytes = 0;
        let total = frames.len();
        let mut last: Option<String> = None;
        for mut item in frames {
            match self.write_frame(&mut item) {
                Ok(name) => {
                    self.captured = self.captured.saturating_add(1);
                    last = Some(name);
                }
                Err(error) => {
                    self.last_error = Some(error);
                    return Err(self.last_error.clone().unwrap_or_default());
                }
            }
        }
        if last.is_some() {
            self.last_file = last;
        }
        self.last_error = None;
        Ok(total)
    }

    /// 写一帧:PNG(可选)+ 一行 JSONL。
    fn write_frame(&mut self, item: &mut RingFrame) -> Result<String, String> {
        let dir = self.dir.clone();
        std::fs::create_dir_all(&dir)
            .map_err(|error| format!("建目录 {} 失败: {error}", dir.display()))?;
        self.ensure_session(&dir)?;
        let mut png_name = Value::Null;
        if self.save_png {
            if let Some(surface) = &item.surface {
                let name = format!("f{:06}.png", item.host_frame);
                yu_render_core::png::write_png(dir.join(&name), surface)
                    .map_err(|error| format!("写 {} 失败: {error}", name))?;
                png_name = Value::String(name);
            }
        }
        if let Some(screen) = item.record.get_mut("screen").and_then(|v| match v {
            Value::Object(map) => Some(map),
            _ => None,
        }) {
            screen.insert("png".to_string(), png_name);
        }
        item.record.insert(
            "phase".to_string(),
            Value::String(item.phase.as_str().to_string()),
        );
        let record = Value::Object(item.record.clone());
        let line = format!("{}\n", json::to_compact(&record));
        append_line(&dir.join(FRAMES_FILE), &line)?;
        Ok(format!("{}#f{:06}", FRAMES_FILE, item.host_frame))
    }

    /// 会话头:只在目录里写一次(重启/换目录时重写)。
    fn ensure_session(&mut self, dir: &Path) -> Result<(), String> {
        if self.session_written {
            return Ok(());
        }
        let mut map: BTreeMap<String, Value> = BTreeMap::new();
        map.insert("schema".to_string(), str_v("yu-debug-ui-stepper/1"));
        map.insert(
            "started_unix".to_string(),
            num_v(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as f64)
                    .unwrap_or(0.0),
            ),
        );
        map.insert("step_frames".to_string(), num_v(f64::from(self.step_frames)));
        map.insert("ring_frames".to_string(), num_v(self.ring_frames as f64));
        map.insert("save_png".to_string(), Value::Bool(self.save_png));
        map.insert(
            "include_display".to_string(),
            Value::Bool(self.include_display),
        );
        let text = json::to_pretty(&Value::Object(map));
        std::fs::write(dir.join(SESSION_FILE), text)
            .map_err(|error| format!("写会话头失败: {error}"))?;
        self.session_written = true;
        Ok(())
    }

    /// 组装一帧的全部证据(`phase` 由调用方补)。
    fn build_record(
        &mut self,
        engine: &Engine,
        diagnostics: &Diagnostics,
        host_frame: u64,
        timings: Timings,
    ) -> BTreeMap<String, Value> {
        let report = engine.last_report();
        let mut map: BTreeMap<String, Value> = BTreeMap::new();
        map.insert("host_frame".to_string(), num_v(host_frame as f64));
        // 两个帧号**不是一回事**,分开记,免得对现场时把口径搞混:
        // - `engine_frame` = `Engine::frame()`,每次 tick +1,脚本/`--step-to`
        //   与 `--frames` 都用它;
        // - `gfx_frame` = `FrameReport.frame`(gfx 合成帧序号),软后端下与
        //   前者不保证逐帧相等(历史读数里出现过 1,3,4,6…)。
        map.insert("engine_frame".to_string(), num_v(engine.frame() as f64));
        map.insert("gfx_frame".to_string(), num_v(report.frame as f64));
        map.insert("clock_ms".to_string(), num_v(f64::from(report.clock_ms)));
        map.insert("interval_ms".to_string(), num_v(round3(timings.interval_ms)));
        map.insert("tick_ms".to_string(), num_v(round3(report.tick_ms)));
        map.insert("render_ms".to_string(), num_v(round3(report.render_ms)));
        map.insert("draw_ms".to_string(), num_v(round3(timings.draw_ms)));

        map.insert(
            "frame_report".to_string(),
            obj(vec![
                ("ops", num_v(f64::from(report.ops))),
                ("drawn", num_v(f64::from(report.executed))),
                ("skipped", num_v(f64::from(report.skipped))),
                ("passes", num_v(f64::from(report.passes))),
                ("layers", num_v(f64::from(report.layers))),
                ("visible", num_v(f64::from(report.visible))),
                ("drawable", Value::Bool(report.drawable)),
                ("lag_ms", num_v(f64::from(report.lag_ms))),
                ("ms", num_v(round3(report.ms))),
            ]),
        );

        map.insert(
            "preload".to_string(),
            obj(vec![
                ("ms", num_v(round3(engine.preload_ms()))),
                ("assets", num_v(engine.preload_counts().0 as f64)),
                ("errors", num_v(engine.preload_counts().1 as f64)),
                ("capture_ms", num_v(round3(engine.capture_ms()))),
                ("frame_build_ms", num_v(round3(engine.frame_build_ms()))),
            ]),
        );

        let gpu = match engine.gpu_resource_counts() {
            Some((composites, offscreen, snapshots)) => obj(vec![
                ("composites", num_v(composites as f64)),
                ("offscreen", num_v(offscreen as f64)),
                ("snapshots", num_v(snapshots as f64)),
                ("target_bytes", opt_num(engine.target_bytes().map(|b| b as f64))),
                (
                    "reconciled",
                    match engine.wgpu_reconciled() {
                        Some(ok) => Value::Bool(ok),
                        None => Value::Null,
                    },
                ),
            ]),
            None => Value::Null,
        };
        map.insert("gfx_gpu".to_string(), gpu);

        map.insert(
            "render_phases".to_string(),
            match engine.render_phases() {
                Some(phases) => obj(vec![
                    ("plan_us", num_v(phases.plan_us as f64)),
                    ("snap_us", num_v(phases.snap_us as f64)),
                    ("image_us", num_v(phases.image_us as f64)),
                    ("text_us", num_v(phases.text_us as f64)),
                    ("copy_text_us", num_v(phases.copy_text_us as f64)),
                    ("rest_us", num_v(phases.rest_us as f64)),
                    ("encode_us", num_v(phases.encode_us as f64)),
                    ("readback_us", num_v(phases.readback_us as f64)),
                    ("tail_us", num_v(phases.tail_us as f64)),
                    ("text_bake_us", num_v(phases.text_bake_us as f64)),
                ]),
                None => Value::Null,
            },
        );

        map.insert(
            "render_report".to_string(),
            match engine.render_report() {
                Some(render) => {
                    let skips: Vec<Value> = render
                        .skipped
                        .iter()
                        .map(|skip| {
                            obj(vec![
                                ("kind", str_v(skip.kind)),
                                ("reason", str_v(skip.reason.clone())),
                                ("target", str_v(skip.target.clone())),
                                ("count", num_v(f64::from(skip.count))),
                            ])
                        })
                        .collect();
                    let missing: Vec<Value> = render
                        .missing_assets
                        .iter()
                        .map(|issue| {
                            str_v(format!("{}:{}", issue.code, issue.key_display()))
                        })
                        .collect();
                    obj(vec![
                        ("ops_total", num_v(render.ops_total as f64)),
                        ("ops_drawn", num_v(render.ops_drawn as f64)),
                        ("offscreen_passes", num_v(render.offscreen_passes as f64)),
                        ("skipped_ops_total", num_v(f64::from(render.skipped_ops_total))),
                        ("skipped", Value::Array(skips)),
                        ("missing_assets", Value::Array(missing)),
                        ("skipped_text", num_v(render.skipped_text as f64)),
                        ("text_draws", num_v(render.text_draws as f64)),
                        ("text_glyphs", num_v(render.text_glyphs as f64)),
                        ("text_missing", num_v(render.text_missing as f64)),
                        (
                            "text_diags",
                            Value::Array(
                                render.text_diags.iter().cloned().map(str_v).collect(),
                            ),
                        ),
                        ("filter_source", str_v(render.filter_source)),
                        ("filter_mag", str_v(render.filter_mag)),
                        ("filter_min", str_v(render.filter_min)),
                        ("color_space", str_v(render.color_space)),
                    ])
                }
                None => Value::Null,
            },
        );

        // gfx 帧诊断:带脚本位置(`at`)——「哪条命令、哪个 pc 出的问题」。
        let gfx_diags: Vec<Value> = engine
            .frame_diagnostics()
            .into_iter()
            .map(|diag| {
                let at: Vec<Value> = diag
                    .at
                    .iter()
                    .map(|loc| str_v(format!("{} pc{}", loc.script, loc.pc)))
                    .collect();
                obj(vec![
                    ("code", str_v(diag.code)),
                    ("sev", str_v(diag.severity)),
                    ("command", str_v(diag.command)),
                    ("detail", str_v(diag.detail)),
                    ("count", num_v(f64::from(diag.count))),
                    ("at", Value::Array(at)),
                    ("at_more", num_v(diag.at_more as f64)),
                ])
            })
            .collect();
        map.insert("gfx_diags".to_string(), Value::Array(gfx_diags));

        // 本帧新挂号(引擎域 + 声音域 + 存档域,由 diagnostics.observe 聚合)。
        // `DiagEvent::frame` 是 `DiagnosticsHost` 用 `FrameReport.frame` 打的
        // 标(gfx 合成帧号),所以这里按 `report.frame` 过滤,不按 `engine_frame`。
        let diags: Vec<Value> = diagnostics
            .recent
            .iter()
            .filter(|event| event.frame == report.frame)
            .map(|event| {
                obj(vec![
                    ("code", str_v(event.code.clone())),
                    ("sev", str_v(event.severity.as_str())),
                    ("where", str_v(event.where_.clone())),
                    ("detail", str_v(event.detail.clone())),
                ])
            })
            .collect();
        map.insert("diags".to_string(), Value::Array(diags));

        // 输入:按住集合 + 本帧按下沿增量(累计差值,消费端不用自己算)。
        let mut edges: BTreeMap<String, Value> = BTreeMap::new();
        let mut edge_total = 0u64;
        for (button, total) in &diagnostics.input.press_edges {
            let previous = self.last_press_edges.get(button).copied().unwrap_or(0);
            let delta = total.saturating_sub(previous);
            if delta > 0 {
                edges.insert(button.clone(), num_v(delta as f64));
                edge_total += delta;
            }
        }
        self.last_press_edges = diagnostics.input.press_edges.clone();
        let click_delta = diagnostics.input.clicks.saturating_sub(self.last_clicks);
        let wheel_delta = diagnostics.input.wheel_pulses.saturating_sub(self.last_wheel);
        self.last_clicks = diagnostics.input.clicks;
        self.last_wheel = diagnostics.input.wheel_pulses;
        let (cursor_x, cursor_y) = engine.cursor();
        map.insert(
            "input".to_string(),
            obj(vec![
                (
                    "held",
                    Value::Array(engine.held_buttons().into_iter().map(str_v).collect()),
                ),
                ("cursor", Value::Array(vec![num_v(cursor_x as f64), num_v(cursor_y as f64)])),
                ("edges", Value::Object(edges)),
                ("edge_total", num_v(edge_total as f64)),
                ("clicks", num_v(click_delta as f64)),
                ("wheel", num_v(wheel_delta as f64)),
                (
                    "clicks_total",
                    num_v(diagnostics.input.clicks as f64),
                ),
            ]),
        );

        // 声音层:哪个 BGM/SE 在响、播到第几毫秒。
        let sound: Vec<Value> = engine
            .sound_snapshot()
            .into_iter()
            .map(|layer| {
                obj(vec![
                    ("id", str_v(String::from_utf8_lossy(&layer.id).to_string())),
                    ("file", str_v(String::from_utf8_lossy(&layer.file).to_string())),
                    ("state", str_v(format!("{:?}", layer.state))),
                    ("time_ms", num_v(layer.time_ms as f64)),
                    ("total_ms", num_v(layer.total_ms as f64)),
                    ("volume", num_v(layer.volume as f64)),
                    ("effective_volume", num_v(layer.effective_volume as f64)),
                    ("pan", num_v(layer.pan as f64)),
                    ("paused", Value::Bool(layer.paused)),
                ])
            })
            .collect();
        map.insert("sound".to_string(), Value::Array(sound));

        // op 目标分布:这一帧的 op 压在哪些层上。
        map.insert(
            "ops_top".to_string(),
            match engine.frame_op_summary(12) {
                Some(summary) => obj(vec![
                    (
                        "top",
                        Value::Array(
                            summary
                                .top_targets
                                .iter()
                                .map(|(target, count)| {
                                    Value::Array(vec![
                                        str_v(target.clone()),
                                        num_v(*count as f64),
                                    ])
                                })
                                .collect(),
                        ),
                    ),
                    ("screen_passes", num_v(summary.screen_passes as f64)),
                    ("offscreen_passes", num_v(summary.offscreen_passes as f64)),
                    ("texture_passes", num_v(summary.texture_passes as f64)),
                    ("layer_ops", num_v(summary.layer_ops as f64)),
                    ("group_ops", num_v(summary.group_ops as f64)),
                    ("transition_ops", num_v(summary.transition_ops as f64)),
                    ("mask_ops", num_v(summary.mask_ops as f64)),
                    ("copy_ops", num_v(summary.copy_ops as f64)),
                    ("image_ops", num_v(summary.image_ops as f64)),
                ]),
                None => Value::Null,
            },
        );

        // 屏幕:宽高 + 采样指纹(PNG 由 write_frame 补名)。
        map.insert(
            "screen".to_string(),
            match engine.last_surface() {
                Some(surface) => obj(vec![
                    ("w", num_v(f64::from(surface.width))),
                    ("h", num_v(f64::from(surface.height))),
                    (
                        "fingerprint",
                        str_v(format!("{:016x}", crate::diagnostics::fingerprint(&surface.rgba))),
                    ),
                    ("png", Value::Null),
                ]),
                None => Value::Null,
            },
        );

        if self.include_display {
            map.insert(
                "display".to_string(),
                match engine.display_json() {
                    Some(text) => Value::String(text),
                    None => Value::Null,
                },
            );
        }

        map
    }
}

/// 追加一行(自动建文件;每行立即 flush —— 崩溃也不丢已抓的帧)。
fn append_line(path: &Path, line: &str) -> Result<(), String> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| format!("打开 {} 失败: {error}", path.display()))?;
    file.write_all(line.as_bytes())
        .map_err(|error| format!("写 {} 失败: {error}", path.display()))?;
    file.flush()
        .map_err(|error| format!("flush {} 失败: {error}", path.display()))
}

fn round3(value: f64) -> f64 {
    if value.is_finite() {
        (value * 1000.0).round() / 1000.0
    } else {
        0.0
    }
}

fn num_v(value: f64) -> Value {
    Value::Number(if value.is_finite() { value } else { 0.0 })
}

fn opt_num(value: Option<f64>) -> Value {
    match value {
        Some(value) => num_v(value),
        None => Value::Null,
    }
}

fn str_v(value: impl Into<String>) -> Value {
    Value::String(value.into())
}

fn obj(pairs: Vec<(&str, Value)>) -> Value {
    let mut map = BTreeMap::new();
    for (key, value) in pairs {
        map.insert(key.to_string(), value);
    }
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pause_sources_are_independent() {
        let mut stepper = Stepper::new();
        assert_eq!(stepper.tick_kind(), TickKind::Live);
        stepper.toggle_pause();
        assert_eq!(stepper.tick_kind(), TickKind::Halt);
        // 录制开始 = 第二个暂停;手动暂停仍在,不能互相覆盖。
        stepper.toggle_record();
        assert!(stepper.paused() && stepper.record_pause);
        stepper.toggle_pause();
        assert!(!stepper.paused() && stepper.record_pause, "录制暂停必须还在");
        assert_eq!(stepper.tick_kind(), TickKind::Halt);
        stepper.set_recording(false);
        assert_eq!(stepper.tick_kind(), TickKind::Live);
    }

    #[test]
    fn step_walks_fixed_frames_then_falls_back_to_pause() {
        let mut stepper = Stepper::new();
        stepper.toggle_record(); // 录制暂停
        stepper.request_step(3);
        for _ in 0..3 {
            assert_eq!(stepper.tick_kind(), TickKind::Step);
            stepper.after_tick();
            assert!(!stepper.take_resumed(), "步进期间不算恢复");
        }
        assert_eq!(stepper.tick_kind(), TickKind::Halt, "步进完自动落回暂停");
        // 取消录制暂停 → 恢复实时,并且宿主应当重置实时时钟。
        stepper.set_recording(false);
        stepper.set_paused(false);
        assert_eq!(stepper.tick_kind(), TickKind::Live);
        assert!(stepper.take_resumed(), "恢复的第一帧要报 resumed");
        assert!(!stepper.take_resumed(), "只报一次");
    }

    /// 推进必须自带暂停:否则推完的帧会被实时推进瞬间冲走。
    #[test]
    fn step_from_live_pauses_afterwards() {
        let mut stepper = Stepper::new();
        assert_eq!(stepper.tick_kind(), TickKind::Live);
        stepper.request_step(2);
        assert!(stepper.paused(), "推进必须进入暂停态");
        for _ in 0..2 {
            assert_eq!(stepper.tick_kind(), TickKind::Step);
            stepper.after_tick();
        }
        assert_eq!(
            stepper.tick_kind(),
            TickKind::Halt,
            "走完 N 帧必须停在暂停态等人看"
        );
    }

    #[test]
    fn step_clamps_and_one_shot_replaces_pending() {
        let mut stepper = Stepper::new();
        stepper.request_step(0);
        assert_eq!(stepper.pending_steps(), 1, "0 帧按 1 帧处理");
        stepper.request_step(99_999);
        assert_eq!(stepper.pending_steps(), MAX_STEP_FRAMES);
        stepper.request_step(4);
        assert_eq!(stepper.pending_steps(), 4, "新请求替换旧请求而不是累加");
    }

    #[test]
    fn stopping_record_clears_ring_and_session_flag() {
        let mut stepper = Stepper::new();
        stepper.toggle_record();
        stepper
            .ring
            .push_back(RingFrame {
                phase: Phase::Live,
                host_frame: 1,
                surface: None,
                record: BTreeMap::new(),
            });
        stepper.session_written = true;
        stepper.set_recording(false);
        assert!(stepper.ring.is_empty());
        assert!(!stepper.session_written, "下次录制要重写会话头");
    }

    #[test]
    fn ui_roundtrip_writes_back_editable_fields() {
        let mut stepper = Stepper::new();
        let mut ui = stepper.ui();
        ui.step_frames = 25;
        ui.save_png = false;
        ui.include_display = true;
        ui.ring_frames = 30;
        ui.dir = "target/tmp/other".to_string();
        stepper.apply_ui(&ui, StepperActions::default());
        assert_eq!(stepper.step_frames(), 25);
        assert!(!stepper.save_png);
        assert!(stepper.include_display);
        assert_eq!(stepper.ring_frames, 30);
        assert_eq!(stepper.dir(), Path::new("target/tmp/other"));
    }

    #[test]
    fn ui_step_action_uses_configured_frame_count() {
        let mut stepper = Stepper::new();
        let mut ui = stepper.ui();
        ui.step_frames = 7;
        stepper.apply_ui(
            &ui,
            StepperActions {
                step: true,
                ..StepperActions::default()
            },
        );
        assert_eq!(
            stepper.pending_steps(),
            7,
            "配置的帧数要进待推进队列"
        );
    }

    /// `--record` 起跑后勾上「录制(自动)」= 开始录制**并暂停**;
    /// 再取消 = 结束录制。这条路径曾用 toggle,会把已开启的录制关掉。
    #[test]
    fn record_pause_checkbox_is_explicit_not_toggle() {
        let mut stepper = Stepper::new();
        stepper.start_recording_live();
        assert!(stepper.recording() && !stepper.record_paused());

        let mut ui = stepper.ui();
        ui.record_pause = true;
        stepper.apply_ui(
            &ui,
            StepperActions {
                record_pause: Some(true),
                ..StepperActions::default()
            },
        );
        assert!(stepper.recording(), "勾上不该把录制关掉");
        assert!(stepper.record_paused(), "勾上要进录制暂停");
        assert_eq!(stepper.tick_kind(), TickKind::Halt);

        let mut ui = stepper.ui();
        ui.record_pause = false;
        stepper.apply_ui(
            &ui,
            StepperActions {
                record_pause: Some(false),
                ..StepperActions::default()
            },
        );
        assert!(!stepper.recording(), "取消 = 结束录制");
        assert!(!stepper.record_paused());
    }

    /// 临时目录(每个用例独立;测试并行也不撞)。
    ///
    /// 落点必须是 `<repo>/target/tmp`:本机系统 `TEMP`(`E:\Temp`)不可写,
    /// `std::env::temp_dir()` 会直接「拒绝访问」。这也与仓库其余用例的
    /// 约定一致(产物只进 `target/tmp`,不入库)。
    fn temp_dir(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/tmp/stepper-tests")
            .join(format!("{tag}-{}-{serial}", std::process::id()))
    }

    fn synthetic_frame(host_frame: u64) -> RingFrame {
        let mut record = BTreeMap::new();
        record.insert("host_frame".to_string(), num_v(host_frame as f64));
        record.insert(
            "screen".to_string(),
            obj(vec![
                ("w", num_v(2.0)),
                ("h", num_v(2.0)),
                ("png", Value::Null),
            ]),
        );
        RingFrame {
            phase: Phase::Step,
            host_frame,
            surface: Some(Arc::new(Surface {
                width: 2,
                height: 2,
                rgba: vec![9u8; 2 * 2 * 4],
            })),
            record,
        }
    }

    /// 落盘必须是**真文件**:一行合法 JSONL + 一张能解出来的 PNG,
    /// 且 PNG 名回填进该行(`screen.png`),不是空字段。
    #[test]
    fn writes_jsonl_line_and_png_and_backfills_name() {
        let dir = temp_dir("write");
        let mut stepper = Stepper::new();
        stepper.set_dir(dir.clone());
        stepper.set_step_frames(10);

        let mut frame = synthetic_frame(42);
        let name = stepper.write_frame(&mut frame).unwrap();
        assert!(name.contains("f000042"), "{name}");

        let jsonl = std::fs::read_to_string(dir.join(FRAMES_FILE)).unwrap();
        let lines: Vec<&str> = jsonl.lines().collect();
        assert_eq!(lines.len(), 1, "一帧一行: {jsonl}");
        assert!(!lines[0].contains('\n'));
        let value = json::parse(lines[0]).unwrap();
        assert_eq!(value.get("phase").unwrap().as_str(), Some("step"));
        assert_eq!(value.get("host_frame").unwrap().as_u64(), Some(42));
        assert_eq!(
            value
                .get("screen")
                .and_then(|s| s.get("png"))
                .and_then(Value::as_str),
            Some("f000042.png"),
            "PNG 名必须回填进同一行"
        );

        let png = dir.join("f000042.png");
        let bytes = std::fs::read(&png).unwrap();
        assert!(bytes.starts_with(b"\x89PNG"), "不是 PNG: {bytes:?}");
        let session = std::fs::read_to_string(dir.join(SESSION_FILE)).unwrap();
        assert!(session.contains("yu-debug-ui-stepper/1"), "{session}");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 第二帧是**追加**,不能把第一帧覆盖掉(JSONL 逐帧累积)。
    #[test]
    fn second_frame_appends_instead_of_overwriting() {
        let dir = temp_dir("append");
        let mut stepper = Stepper::new();
        stepper.set_dir(dir.clone());

        let mut first = synthetic_frame(1);
        stepper.write_frame(&mut first).unwrap();
        let mut second = synthetic_frame(2);
        stepper.write_frame(&mut second).unwrap();

        let jsonl = std::fs::read_to_string(dir.join(FRAMES_FILE)).unwrap();
        let frames: Vec<u64> = jsonl
            .lines()
            .map(|line| json::parse(line).unwrap())
            .map(|value| value.get("host_frame").unwrap().as_u64().unwrap())
            .collect();
        assert_eq!(frames, vec![1, 2], "{jsonl}");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 录制的实时帧进环、`dump_ring` 一次全写出(最旧在前)。
    #[test]
    fn dump_ring_writes_every_buffered_frame_in_order() {
        let dir = temp_dir("ring");
        let mut stepper = Stepper::new();
        stepper.set_dir(dir.clone());
        stepper.set_recording(true);
        for host_frame in 10..14 {
            stepper.ring.push_back(synthetic_frame(host_frame));
        }
        assert_eq!(stepper.dump_ring().unwrap(), 4);
        assert!(stepper.ring.is_empty(), "落盘后缓冲清空");
        assert_eq!(stepper.captured, 4);

        let jsonl = std::fs::read_to_string(dir.join(FRAMES_FILE)).unwrap();
        let frames: Vec<u64> = jsonl
            .lines()
            .map(|line| json::parse(line).unwrap())
            .map(|value| value.get("host_frame").unwrap().as_u64().unwrap())
            .collect();
        assert_eq!(frames, vec![10, 11, 12, 13]);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 环形缓冲按 `ring_frames` 丢最旧的(内存有界)。
    ///
    /// 走真实的 [`Stepper::push_ring`],同时校验字节计数被同步扣减
    /// —— 早先测试自己手写裁剪循环,绕过了实现,`ring_bytes` 记账漏了也测不出来。
    #[test]
    fn ring_is_bounded_by_configured_capacity() {
        let mut stepper = Stepper::new();
        stepper.set_recording(true);
        stepper.ring_frames = 2;
        for host_frame in 0..5 {
            stepper.push_ring(synthetic_frame(host_frame));
        }
        let kept: Vec<u64> = stepper.ring.iter().map(|f| f.host_frame).collect();
        assert_eq!(kept, vec![3, 4], "只留最近 2 帧");
        let expected: usize = stepper.ring.iter().map(RingFrame::surface_bytes).sum();
        assert_eq!(stepper.ring_bytes, expected, "字节计数必须与驻留帧一致");
    }

    /// 帧数上限之外还有**字节预算**:超过预算时继续丢最旧的,即使还没到
    /// `ring_frames`。这里直接调 `push_ring_budgeted` 传入小预算 —— 用真实的
    /// 512 MiB 需要分配 ~5 亿字节,在 CI 上不合适。
    #[test]
    fn ring_is_also_bounded_by_byte_budget() {
        let mut stepper = Stepper::new();
        stepper.set_recording(true);
        stepper.ring_frames = 600; // 配到上限,让字节预算成为唯一约束
        let frame_bytes = 2 * 2 * 4; // `synthetic_frame` 的 2x2 RGBA = 16 字节
        let budget = frame_bytes * 2 + 1; // 只装得下 2 帧
        for host_frame in 0..5u64 {
            stepper.push_ring_budgeted(synthetic_frame(host_frame), budget);
        }
        assert_eq!(stepper.ring.len(), 2, "字节预算把驻留量压到 2 帧");
        assert!(stepper.ring_bytes <= budget, "不得越过字节预算");
        let kept: Vec<u64> = stepper.ring.iter().map(|f| f.host_frame).collect();
        assert_eq!(kept, vec![3, 4], "丢的是最旧的");
    }
}
