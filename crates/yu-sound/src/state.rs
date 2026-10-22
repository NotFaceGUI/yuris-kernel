//! 声音域状态机：层表 + 注入时钟推进 + SOUNDINFO 查询。
//!
//! 证据等级（详见 `docs/sound-ledger.md`）：
//! - **Confirmed（静态 REA + 手册）**：层 ID 键、扩展名探测序、E/PLAY/PAUSE/
//!   VOLUME/PAN/PITCH/LOOP/START/RESTART/END/FADE 的字段与公式、
//!   LOOPCOUNT 在 END 边界 +1、LOOP=N 在计数达到 N 时停止并清零、
//!   TIME = 播放位置(ms)、TOTALTIME = 总长(ms)、CODEC/FILETYPE 枚举。
//! - **Unverified**：重采样插值、混音限幅、负 START/RESTART 的延迟语义。
//! - **Unimplemented**：ACM WAVE、MIDI/MIDIEND/MIDIINFO、CDDA。

use std::collections::BTreeMap;

use yu_log::Diag;
use yu_rt::value::Value;

use crate::decode::{decode, Codec, Pcm};
use crate::error::{self, code};
use crate::loader::SoundLoader;

/// 引擎 `+0x105` 的内部文件类型（**不是**对外 CODEC）。
///
/// 0.495 处理体：FILE 解析结果 5 → `+0x105=2`，6 → `+0x105=1`；
/// 解码器再写 `+0x107`（对外 CODEC 0/1/2）。旧线 v494 的
/// “FILETYPE OGG=1/WAVE=2” 就是这个内部字段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileType {
    None,
    Ogg = 1,
    Wav = 2,
}

impl FileType {
    pub const fn value(self) -> i64 {
        self as i64
    }
}

/// 播放状态；数值与引擎 `+0x108` 对齐。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayState {
    Stopped = 0,
    Loaded = 1,
    Playing = 2,
    Steady = 3,
    FadingOut = 4,
}

impl PlayState {
    /// 引擎 `SOUNDINFO[PLAY]`：`+0x108 ∈ {2,3,4}` 为 1。
    pub const fn audible(self) -> bool {
        matches!(
            self,
            PlayState::Playing | PlayState::Steady | PlayState::FadingOut
        )
    }
}

#[derive(Clone, Debug)]
struct Fade {
    active: bool,
    start_ms: i64,
    duration_ms: i64,
    from: i64,
    to: i64,
}

impl Fade {
    const fn none() -> Self {
        Self {
            active: false,
            start_ms: 0,
            duration_ms: 0,
            from: 0,
            to: 0,
        }
    }
}

/// 解码器建立的格式元数据（引擎 `+0x105/+0x107/+0x1ba/+0x1bc/+0x1c6`）。
///
/// 卸载（type 0xe → `0x423940`）只释放 PCM 缓冲，**不清**这些字段；
/// 因此 `EXIST/CODEC/CHANNEL/BIT/SAMPLING` 在卸载后仍返回旧值。
#[derive(Clone, Debug)]
struct SoundMeta {
    file_type: FileType,
    codec: Codec,
    channels: u16,
    bits: u16,
    sample_rate: u32,
}

impl SoundMeta {
    fn from_pcm(pcm: &Pcm) -> Self {
        Self {
            file_type: match pcm.codec {
                Codec::Ogg => FileType::Ogg,
                Codec::Wav | Codec::Acm => FileType::Wav,
            },
            codec: pcm.codec,
            channels: pcm.channels,
            bits: pcm.bits,
            sample_rate: pcm.sample_rate,
        }
    }
}

/// 一个声音层 = 引擎 `layer->+0x34` 的声音对象 + 名字节点。
#[derive(Clone, Debug)]
pub struct Layer {
    /// `SOUND[FILE=]` 的原始参数（`SOUNDINFO[FILE]` 的来源；见台账 N0-7）。
    file_arg: Vec<u8>,
    /// 扩展名探测后真正命中的名字（诊断/渲染用，不对外查询）。
    resolved: Vec<u8>,
    /// 解码器元数据；卸载后保留（引擎 `+0x105…` 不被 `0x423940` 清）。
    meta: Option<SoundMeta>,
    enabled: bool,
    state: PlayState,
    paused: bool,
    loop_setting: i64,
    loop_count: i64,
    fade_ms: i64,
    start_ms: i64,
    restart_ms: i64,
    end_ms: i64,
    pan: i64,
    pitch: i64,
    /// `SOUNDINFO[VOLUME]` = 引擎 `+0x148`（淡变中的**当前**音量）。
    volume_current: i64,
    /// 引擎 `+0x150`（淡变目标）。
    volume_target: i64,
    pcm: Option<Pcm>,
    position_ms: i64,
    total_ms: i64,
    fade: Fade,
    /// 负 START/RESTART 的延迟剩余（0 = 无延迟）。
    delay_ms: i64,
}

impl Layer {
    fn new() -> Self {
        Self {
            file_arg: Vec::new(),
            resolved: Vec::new(),
            meta: None,
            enabled: true,
            state: PlayState::Stopped,
            paused: false,
            loop_setting: 0,
            loop_count: 0,
            fade_ms: 0,
            start_ms: 0,
            restart_ms: 0,
            end_ms: 0,
            // 手册只给 PAN/PITCH 默认 256；VOLUME 未写默认。引擎停止时把
            // 音量字段重置为 0x100，故取 256（Likely，见台账 N0-6）。
            pan: 256,
            pitch: 256,
            volume_current: 256,
            volume_target: 256,
            pcm: None,
            position_ms: 0,
            total_ms: 0,
            fade: Fade::none(),
            delay_ms: 0,
        }
    }

    pub fn file_type(&self) -> FileType {
        self.meta
            .as_ref()
            .map(|m| m.file_type)
            .unwrap_or(FileType::None)
    }

    pub fn state(&self) -> PlayState {
        self.state
    }

    pub fn loop_count(&self) -> i64 {
        self.loop_count
    }

    pub fn position_ms(&self) -> i64 {
        self.position_ms
    }

    pub fn total_ms(&self) -> i64 {
        self.total_ms
    }

    pub fn loop_setting(&self) -> i64 {
        self.loop_setting
    }

    pub(crate) fn pcm(&self) -> Option<&Pcm> {
        self.pcm.as_ref()
    }

    pub(crate) fn paused(&self) -> bool {
        self.paused
    }

    pub(crate) fn pan(&self) -> i64 {
        self.pan
    }

    pub(crate) fn pitch(&self) -> i64 {
        self.pitch
    }

    pub(crate) fn end_ms(&self) -> i64 {
        self.end_ms
    }

    pub(crate) fn restart_ms(&self) -> i64 {
        self.restart_ms
    }

    /// 当前可听音量（0..256）：E=0 时强制 0（引擎 `+0x106` × `+0x148`）。
    pub fn audible_volume(&self) -> i64 {
        if self.enabled {
            self.volume_current.clamp(0, 256)
        } else {
            0
        }
    }
}

/// 层快照（诊断/真实语料验证用；不参与查询语义）。
#[derive(Clone, Debug)]
pub struct SoundSnapshot {
    pub id: Vec<u8>,
    pub file: Vec<u8>,
    pub state: PlayState,
    pub time_ms: i64,
    pub total_ms: i64,
    pub loop_setting: i64,
    pub loop_count: i64,
    pub volume: i64,
    pub pan: i64,
    pub pitch: i64,
    pub enabled: bool,
    pub paused: bool,
}

/// `SOUND` 的按名参数（`yu-rt` 的 `DomainArg` 转换后）。
#[derive(Clone, Debug, Default)]
pub struct SoundCommand {
    pub id: Vec<u8>,
    pub file: Option<Vec<u8>>,
    pub e: Option<i64>,
    pub play: Option<i64>,
    pub pause: Option<i64>,
    pub fade: Option<i64>,
    pub start: Option<i64>,
    pub restart: Option<i64>,
    pub end: Option<i64>,
    pub loop_: Option<i64>,
    pub volume: Option<i64>,
    pub pan: Option<i64>,
    pub pitch: Option<i64>,
    /// 0.495 yscm 额外参数槽 13（手册无页；语料未用）。无证据 ⇒ 不动。
    pub speed: Option<i64>,
}

/// SOUNDINFO 单字段查询。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryField {
    Exist,
    E,
    Play,
    Pause,
    Volume,
    Pan,
    Pitch,
    FileType,
    File,
    Start,
    Restart,
    End,
    Loop,
    LoopCount,
    Time,
    TotalTime,
    Channel,
    Bit,
    Sampling,
    DataLeft,
    DataRight,
    Codec,
}

impl QueryField {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "EXIST" => Self::Exist,
            "E" => Self::E,
            "PLAY" => Self::Play,
            "PAUSE" => Self::Pause,
            "VOLUME" => Self::Volume,
            "PAN" => Self::Pan,
            "PITCH" => Self::Pitch,
            "FILETYPE" => Self::FileType,
            "FILE" => Self::File,
            "START" => Self::Start,
            "RESTART" => Self::Restart,
            "END" => Self::End,
            "LOOP" => Self::Loop,
            "LOOPCOUNT" => Self::LoopCount,
            "TIME" => Self::Time,
            "TOTALTIME" => Self::TotalTime,
            "CHANNEL" => Self::Channel,
            "BIT" => Self::Bit,
            "SAMPLE" | "SAMPLING" => Self::Sampling,
            "DATALEFT" => Self::DataLeft,
            "DATARIGHT" => Self::DataRight,
            "CODEC" => Self::Codec,
            _ => return None,
        })
    }

    /// 引擎 SOUNDINFO 处理体的固定检查顺序（REA `0x44f4bc`）。
    /// 前 7 项（`Exist..Pitch`）短路；其余按此顺序连续写，后者覆盖前者。
    pub const fn order(self) -> u8 {
        match self {
            Self::Exist => 0,
            Self::E => 1,
            Self::Play => 2,
            Self::Pause => 3,
            Self::Volume => 4,
            Self::Pan => 5,
            Self::Pitch => 6,
            Self::FileType => 7,
            Self::File => 8,
            Self::Start => 9,
            Self::Restart => 10,
            Self::End => 11,
            Self::Loop => 12,
            Self::LoopCount => 13,
            Self::Time => 14,
            Self::TotalTime => 15,
            Self::Channel => 16,
            Self::Bit => 17,
            Self::Sampling => 18,
            Self::DataLeft => 19,
            Self::DataRight => 20,
            Self::Codec => 21,
        }
    }
}

/// 声音域状态机。单线程；只由 [`SoundState::advance`] 的注入时钟推进。
pub struct SoundState {
    layers: BTreeMap<Vec<u8>, Layer>,
    loader: Box<dyn SoundLoader>,
    clock_ms: i64,
    diags: Vec<Diag>,
    frame: u64,
}

const DIAG_CAP: usize = 4096;

impl SoundState {
    pub fn new(loader: Box<dyn SoundLoader>) -> Self {
        Self {
            layers: BTreeMap::new(),
            loader,
            clock_ms: 0,
            diags: Vec::new(),
            frame: 0,
        }
    }

    pub fn set_frame(&mut self, frame: u64) {
        self.frame = frame;
    }

    pub fn layers_len(&self) -> usize {
        self.layers.len()
    }

    pub fn layer(&self, id: &[u8]) -> Option<&Layer> {
        self.layers.get(id)
    }

    pub(crate) fn layers_iter(&self) -> impl Iterator<Item = (&Vec<u8>, &Layer)> {
        self.layers.iter()
    }

    /// 当前所有层的只读快照（按 ID 排序，确定性）。
    pub fn snapshot(&self) -> Vec<SoundSnapshot> {
        self.layers
            .iter()
            .map(|(id, l)| SoundSnapshot {
                id: id.clone(),
                file: l.file_arg.clone(),
                state: l.state,
                time_ms: l.position_ms,
                total_ms: l.total_ms,
                loop_setting: l.loop_setting,
                loop_count: l.loop_count,
                volume: l.volume_current,
                pan: l.pan,
                pitch: l.pitch,
                enabled: l.enabled,
                paused: l.paused,
            })
            .collect()
    }

    pub fn remove_layer(&mut self, id: &[u8]) -> bool {
        self.layers.remove(id).is_some()
    }

    pub fn report_unimplemented(&mut self, command: &str) {
        self.report(error::unimplemented(
            code::COMMAND_UNIMPLEMENTED,
            format!("{command}: 声音域命令未取证/未实现(MIDI/CDDA 与未知命令)"),
        ));
    }

    pub fn report_arg_type(&mut self, name: &str, value: &Value) {
        self.report(error::recoverable(
            code::ARG_TYPE,
            format!("SOUND 参数 {name} 类型不符: {value:?}"),
        ));
    }

    pub fn take_diags(&mut self) -> Vec<Diag> {
        std::mem::take(&mut self.diags)
    }

    pub fn diags(&self) -> &[Diag] {
        &self.diags
    }

    fn report(&mut self, d: Diag) {
        if self.diags.len() >= DIAG_CAP {
            let batch = (DIAG_CAP / 4).max(1);
            self.diags.drain(..batch);
            self.diags.push(error::info(
                code::LOG_TRUNCATED,
                format!("声音诊断超过 {DIAG_CAP} 条，丢弃最旧 {batch} 条"),
            ));
        }
        self.diags.push(error::at_command(d, self.frame, "SOUND"));
    }

    /// 注入帧时钟（毫秒，绝对时刻）。只按**差值**推进；同一时刻无副作用。
    pub fn advance(&mut self, now_ms: u32) {
        let now = i64::from(now_ms);
        if now < self.clock_ms {
            // 32 位时钟回绕/重启：按无推进处理，等待下一帧。
            self.clock_ms = now;
            return;
        }
        let delta = now - self.clock_ms;
        self.clock_ms = now;
        self.advance_delta(delta);
    }

    /// 按毫秒差值推进（单测/离线回放；与 [`SoundState::advance`] 同语义）。
    pub fn advance_ms(&mut self, delta_ms: u32) {
        self.clock_ms = self.clock_ms.saturating_add(i64::from(delta_ms));
        self.advance_delta(i64::from(delta_ms));
    }

    fn advance_delta(&mut self, delta: i64) {
        if delta <= 0 {
            return;
        }
        for layer in self.layers.values_mut() {
            advance_layer(layer, delta);
        }
    }

    /// 应用一条 `SOUND` 命令。
    pub fn apply_sound(&mut self, cmd: &SoundCommand) {
        // 引擎不校验空 ID：空名字同样进哈希表（REA `0x44e1e8` 直接建节点）。
        // 这里不拒绝，避免把数据问题变成宿主策略。
        if cmd.speed.is_some() {
            self.report(error::unimplemented(
                code::COMMAND_UNIMPLEMENTED,
                "SOUND[SPEED=]（0.495 yscm 槽 13）无手册页/语料/REA 语义，保持未实现",
            ));
        }

        // 先把层取出为局部值：参数应用期间允许 `self.report`/`self.loader`
        // 再借用 self；结束后放回层表（见借用检查器错误的历史）。
        let mut layer = self.layers.remove(&cmd.id).unwrap_or_else(Layer::new);

        // 1) FILE：装载 + 解码（引擎在参数写回前完成文件装入）。
        if let Some(file) = &cmd.file {
            if self.load_file(&mut layer, file) == LoadOutcome::NotFound {
                // 引擎 `FUN_0040c9b4()==1` 时 SOUND 处理体直接 return 0，
                // 同一命令里的 PLAY/VOLUME/START 等参数都不应用。
                self.layers.insert(cmd.id.clone(), layer);
                return;
            }
        }
        // 2) E
        if let Some(e) = cmd.e {
            layer.enabled = e != 0;
        }
        // 3) VOLUME
        if let Some(v) = cmd.volume {
            if !(0..=256).contains(&v) {
                self.report(error::recoverable(
                    code::ARG_RANGE,
                    format!("SOUND[VOLUME={v}] 越界(0..256)，按引擎语义钳制"),
                ));
            }
            let target = v.clamp(0, 256);
            layer.volume_target = target;
            if cmd.play.is_some() {
                // 有 PLAY：引擎 type 6 只写目标（`+0x150`；旧目标→`+0x154`），
                // 当前音量由 PLAY 的 FADE 决定（REA `0x425164`）。
                layer.fade_ms = cmd.fade.unwrap_or(0).max(0);
            } else {
                // 无 PLAY：引擎 type 5 立即写 `+0x148/+0x154/+0x150` 并 SetVolume
                // （REA `0x42511c`）；正在进行的淡入被目标覆盖后收敛（state 2→3）。
                layer.volume_current = target;
                if layer.state == PlayState::Playing {
                    layer.state = PlayState::Steady;
                    layer.fade = Fade::none();
                } else if layer.state != PlayState::FadingOut {
                    // 淡出中（state 4）引擎不改状态，淡出仍会走完并停止。
                    layer.fade = Fade::none();
                }
            }
        }
        // 4) PAN / PITCH：引擎越界回 256（REA 0x424fbc / 0x423614）。
        if let Some(p) = cmd.pan {
            if !(0..=512).contains(&p) {
                self.report(error::recoverable(
                    code::ARG_RANGE,
                    format!("SOUND[PAN={p}] 越界(0..512)，引擎回默认 256"),
                ));
                layer.pan = 256;
            } else {
                layer.pan = p;
            }
        }
        if let Some(p) = cmd.pitch {
            if !(0..=512).contains(&p) {
                self.report(error::recoverable(
                    code::ARG_RANGE,
                    format!("SOUND[PITCH={p}] 越界(0..512)，引擎回默认 256"),
                ));
                layer.pitch = 256;
            } else {
                layer.pitch = p;
            }
        }
        // 5) LOOP
        if let Some(l) = cmd.loop_ {
            if l < 0 {
                self.report(error::recoverable(
                    code::ARG_RANGE,
                    format!("SOUND[LOOP={l}] <0；引擎未取证，按 0(无限)处理"),
                ));
                layer.loop_setting = 0;
            } else {
                layer.loop_setting = l;
            }
            layer.loop_count = 0;
        }
        // 6) START / RESTART / END
        // 引擎写回规则（REA `0x4251d8/0x425238/0x425298`）：
        // START/RESTART: 负值原样保留（延迟播放）；>= total 写 0；否则原值。
        // END: <= total 原样；> total 写 total。
        // 三者都只写字段，**不移动**当前位置 `+0x118`（TIME 不变）。
        if let Some(s) = cmd.start {
            layer.start_ms = normalize_start(s, layer.total_ms);
            if s < 0 {
                self.report(error::unimplemented(
                    code::DELAY_UNIMPLEMENTED,
                    format!("SOUND[START={s}] 负值=延迟播放；N0-3 未取得 TIME/PLAY 动态证据"),
                ));
                layer.delay_ms = 0;
            } else {
                layer.delay_ms = 0;
            }
        }
        if let Some(r) = cmd.restart {
            layer.restart_ms = normalize_start(r, layer.total_ms);
            if r < 0 {
                self.report(error::unimplemented(
                    code::DELAY_UNIMPLEMENTED,
                    format!("SOUND[RESTART={r}] 负值=延迟；N0-3 未取得动态证据"),
                ));
                layer.restart_ms = 0;
            }
        }
        if let Some(e) = cmd.end {
            layer.end_ms = if e > layer.total_ms {
                layer.total_ms
            } else {
                e
            };
        }
        // 7) PLAY（FADE 只在这里生效）
        if let Some(play) = cmd.play {
            let fade = cmd.fade.unwrap_or(0).max(0);
            layer.fade_ms = fade;
            if play != 0 {
                start_play(&mut layer, fade);
            } else {
                stop_play(&mut layer, fade);
            }
        }
        // 8) PAUSE：引擎在 PLAY 之后检查（`0x684163` 先于 `0x684164`）；
        // PLAY 不会清 `+0x109`，暂停只由 `PAUSE=0` 解除。
        if let Some(p) = cmd.pause {
            layer.paused = p != 0;
        }

        if cmd.file.is_some() && cmd.play.is_none() && layer.pcm.is_some() {
            // 手册例 8：SOUND[FILE=] 只装载，不自动播放。
            // 装载/解码失败时保持 Stopped（没有可播放数据）。
            layer.state = PlayState::Loaded;
        }
        self.layers.insert(cmd.id.clone(), layer);
    }

    /// 装载。返回“文件解析是否成功”（不代表解码成功）。
    ///
    /// 引擎 `FUN_0040c9b4` 返回 1（无候选文件）时，SOUND 处理体直接
    /// `return 0`，后续参数（PLAY/VOLUME/…）全部不应用；返回 0 时才入队
    /// type 1 并等待完成，解码失败也会继续应用参数。
    fn load_file(&mut self, layer: &mut Layer, file: &[u8]) -> LoadOutcome {
        // REA `0x40c9b4`（mode 1）：
        // 1. `0x45f348` 把整条文件名 ASCII 小写；
        // 2. 尾部已是 `.wav`/`.ogg`（`0x8079dc/0x8079e4`）则原样使用；
        // 3. 否则依次追加 `.ogg`、`.wav` 探测；
        // 4. 最终候选名被复制进声音对象 `+0x000`，即 `SOUNDINFO[FILE]` 来源。
        let base = normalize_name(file);
        let candidates = probe_names(&base);
        let mut found: Option<(Vec<u8>, Vec<u8>)> = None;
        for name in &candidates {
            match self.loader.load(name) {
                Ok(Some(bytes)) => {
                    found = Some((name.clone(), bytes));
                    break;
                }
                Ok(None) => {}
                Err(d) => {
                    self.report(d);
                }
            }
        }
        let Some((resolved, bytes)) = found else {
            // 无候选文件：引擎先执行过 type 0xe（仅当 state != 0）：
            // 清 `+0x000`、Stop、释放 PCM/缓冲、清 START/RESTART、state=0；
            // **保留** `+0x105/+0x107/+0x120/+0x118/PAN/PITCH/LOOP/LOOPCOUNT`。
            // 若本来 state==0，则连这一步也没有，原状态完全不变。
            if layer.state != PlayState::Stopped {
                layer.file_arg.clear();
                layer.resolved.clear();
                layer.pcm = None;
                layer.start_ms = 0;
                layer.restart_ms = 0;
                layer.fade = Fade::none();
                layer.state = PlayState::Stopped;
            }
            self.report(error::recoverable(
                code::FILE_MISSING,
                format!("SOUND 文件未找到: {}", display(file)),
            ));
            return LoadOutcome::NotFound;
        };
        // 引擎 type 1（`0x424614`）在装载前重置：PAN/PITCH 回 256、
        // START/RESTART/END/位置清零；E/VOLUME/LOOP/LOOPCOUNT 保留。
        reset_for_load(layer);
        layer.file_arg = resolved.clone();
        layer.resolved = resolved.clone();
        match decode(&resolved, &bytes) {
            Ok(pcm) => {
                layer.total_ms = pcm.total_ms();
                layer.end_ms = layer.total_ms;
                layer.position_ms = 0;
                layer.state = PlayState::Loaded;
                layer.meta = Some(SoundMeta::from_pcm(&pcm));
                layer.pcm = Some(pcm);
            }
            Err(d) => {
                // 解码失败：名字已写入，PCM 清空；type 1 不重置
                // `+0x105/+0x107/+0x120`，元数据与 TOTALTIME 保留旧值。
                layer.pcm = None;
                layer.end_ms = 0;
                layer.position_ms = 0;
                layer.state = PlayState::Stopped;
                self.report(d);
            }
        }
        LoadOutcome::Loaded
    }

    /// SOUNDINFO 查询。返回写回 `LET` 的值；未取证字段返回错误。
    ///
    /// 多字段同调（REA `0x44f4bc` 原始汇编）：
    /// - 前 7 个字段 `EXIST..PITCH` 各带独立 `ret`，按固定顺序**首个命中即返回**，
    ///   后续字段不再写；
    /// - 若这 7 个都不在请求里，`FILETYPE/FILE/START/.../CODEC` 连续执行，
    ///   全部写同一个 LET，**最后者胜出**。
    pub fn query(&mut self, id: &[u8], fields: &[QueryField]) -> Result<Value, Diag> {
        if fields.is_empty() {
            return Err(error::recoverable(
                code::ARG_MISSING,
                "SOUNDINFO 缺查询字段",
            ));
        }
        let mut ordered: Vec<QueryField> = fields.to_vec();
        ordered.sort_by_key(|f| f.order());
        ordered.dedup();
        // EXIST/E/PLAY/PAUSE/VOLUME/PAN/PITCH 是引擎的短路字段。
        let effective: Vec<QueryField> = match ordered.iter().copied().find(|f| f.order() <= 6) {
            Some(first) => vec![first],
            None => ordered.clone(),
        };
        if effective.len() > 1 {
            self.report(error::info(
                code::QUERY_MULTI,
                "SOUNDINFO 多字段同调（无短路字段）：按引擎检查顺序逐个写 LET，最后者胜出",
            ));
        }
        let Some(layer) = self.layers.get(id) else {
            // 引擎缺层分支：把 LET 写成 0（字符串目标写空串）。
            return Ok(if ordered.contains(&QueryField::File) {
                Value::Str(Vec::new())
            } else {
                Value::Int(0)
            });
        };
        let mut value: Option<Value> = None;
        let mut unsupported = false;
        for field in effective {
            value = Some(match field {
                QueryField::Exist => Value::Int(i64::from(layer.file_type() != FileType::None)),
                QueryField::E => Value::Int(i64::from(layer.enabled)),
                QueryField::Play => Value::Int(i64::from(layer.state.audible())),
                QueryField::Pause => Value::Int(i64::from(layer.paused)),
                QueryField::Volume => Value::Int(layer.volume_current),
                QueryField::Pan => Value::Int(layer.pan),
                QueryField::Pitch => Value::Int(layer.pitch),
                QueryField::FileType => Value::Int(layer.file_type().value()),
                QueryField::File => Value::Str(layer.file_arg.clone()),
                QueryField::Start => Value::Int(layer.start_ms),
                QueryField::Restart => Value::Int(layer.restart_ms),
                QueryField::End => Value::Int(layer.end_ms),
                QueryField::Loop => Value::Int(layer.loop_setting),
                QueryField::LoopCount => Value::Int(layer.loop_count),
                QueryField::Time => Value::Int(layer.position_ms),
                QueryField::TotalTime => Value::Int(layer.total_ms),
                QueryField::Channel => Value::Int(
                    layer
                        .meta
                        .as_ref()
                        .map(|m| i64::from(m.channels))
                        .unwrap_or(0),
                ),
                QueryField::Bit => {
                    Value::Int(layer.meta.as_ref().map(|m| i64::from(m.bits)).unwrap_or(0))
                }
                QueryField::Sampling => Value::Int(
                    layer
                        .meta
                        .as_ref()
                        .map(|m| i64::from(m.sample_rate))
                        .unwrap_or(0),
                ),
                QueryField::DataLeft | QueryField::DataRight => {
                    // REA 0x44fe3c/0x44ff58 已逐行取证（读播放光标处一帧、
                    // 16-bit 取 abs、8-bit 取 |byte-0x80|、单声道无右值），
                    // 但 0.495 手册没有这两个字段 ⇒ 三层证据缺手册层，
                    // 按任务书 §0.1 保持 Unimplemented（只加深取证，不实现）。
                    unsupported = true;
                    continue;
                }
                QueryField::Codec => {
                    Value::Int(layer.meta.as_ref().map(|m| m.codec.value()).unwrap_or(0))
                }
            });
        }
        match value {
            Some(v) => Ok(v),
            None if unsupported => Err(error::unimplemented(
                code::QUERY_UNSUPPORTED,
                "DATALEFT/DATARIGHT 是 0.495 yscm 扩展字段，手册无页；REA 0x44fe3c/0x44ff58 已取证但缺手册层，按三层规则保持 Unimplemented",
            )),
            None => Ok(Value::Int(0)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LoadOutcome {
    /// 文件名探测全失败（引擎 `FUN_0040c9b4()==1`）。
    NotFound,
    /// 找到文件并进入 type 1（含解码失败）。
    Loaded,
}

fn reset_for_load(layer: &mut Layer) {
    // REA `0x424614`（type 1 load）：装载前重置的字段。
    // 注意 E(`+0x106`)/VOLUME(`+0x148`)/LOOP(`+0x10c`)/LOOPCOUNT(`+0x110`)
    // 不在此处重置；`+0x114`（循环镜像）清零，本实现未建模该镜像。
    layer.pan = 256;
    layer.pitch = 256;
    layer.start_ms = 0;
    layer.restart_ms = 0;
    layer.end_ms = 0;
    layer.position_ms = 0;
    layer.delay_ms = 0;
    layer.fade = Fade::none();
}

fn start_play(layer: &mut Layer, fade_ms: i64) {
    if layer.pcm.is_none() {
        // 有 ID 无 FILE：保持 Stopped；引擎 type 2 在 `*pcVar2==0` 时返回 1。
        return;
    }
    match layer.state {
        PlayState::Stopped | PlayState::Loaded => {
            // 引擎 type 2（`0x424950`）state 0 分支：
            // `if (END <= START) START = 0;` 然后 seek 到 START。
            if layer.end_ms <= layer.start_ms {
                layer.start_ms = 0;
            }
            layer.position_ms = layer.start_ms.max(0).min(layer.total_ms.max(0));
            // type 2 不写 `+0x110`（LOOPCOUNT 只由循环边界 0x464960 清零）。
            layer.state = PlayState::Playing;
            if fade_ms > 0 {
                layer.volume_current = 0;
                layer.fade = Fade {
                    active: true,
                    start_ms: 0,
                    duration_ms: fade_ms,
                    from: 0,
                    to: layer.volume_target,
                };
            } else {
                layer.volume_current = layer.volume_target;
                layer.fade = Fade::none();
            }
        }
        PlayState::Playing | PlayState::Steady => {
            // 引擎 state 2/3 分支：从当前音量向目标重算淡变。
            if fade_ms > 0 {
                layer.fade = Fade {
                    active: true,
                    start_ms: 0,
                    duration_ms: fade_ms,
                    from: layer.volume_current,
                    to: layer.volume_target,
                };
            } else {
                layer.volume_current = layer.volume_target;
            }
        }
        PlayState::FadingOut => {
            // 引擎 state 4 分支：交换淡变当前/目标后立即停止（`0x424950`）。
            layer.fade = Fade::none();
            layer.state = PlayState::Stopped;
        }
    }
}

fn stop_play(layer: &mut Layer, fade_ms: i64) {
    if !layer.state.audible() {
        return;
    }
    if fade_ms > 0 {
        layer.state = PlayState::FadingOut;
        layer.fade = Fade {
            active: true,
            start_ms: 0,
            duration_ms: fade_ms,
            from: layer.volume_current,
            to: 0,
        };
    } else {
        layer.state = PlayState::Stopped;
        layer.fade = Fade::none();
        // 引擎淡出完成后把音量字段重置为 0x100（FUN_00423718）。
        layer.volume_current = 256;
        layer.volume_target = 256;
    }
}

fn advance_layer(layer: &mut Layer, delta_ms: i64) {
    if !layer.state.audible() || layer.paused {
        return;
    }
    // 淡变（引擎 FUN_00423718：线性、单位 ms、固定点 1/1024）。
    if layer.fade.active {
        layer.fade.start_ms += delta_ms;
        if layer.fade.start_ms >= layer.fade.duration_ms {
            layer.volume_current = layer.fade.to;
            layer.fade.active = false;
            if layer.state == PlayState::Playing {
                layer.state = PlayState::Steady;
            } else if layer.state == PlayState::FadingOut {
                layer.state = PlayState::Stopped;
                layer.volume_current = 256;
                layer.volume_target = 256;
                layer.fade = Fade::none();
                return;
            }
        } else {
            let num = (layer.fade.to - layer.fade.from) * layer.fade.start_ms;
            layer.volume_current = layer.fade.from + div_round(num, layer.fade.duration_ms.max(1));
        }
    }

    // 位置推进：PITCH 512=1.5x / 0=0.5x（REA 0x423614）。
    if layer.delay_ms > 0 {
        let step = delta_ms.min(layer.delay_ms);
        layer.delay_ms -= step;
        return;
    }
    // 引擎 SetFrequency：base*(pitch+256)/512（0→0.5x, 256→1x, 512→1.5x）。
    let speed = layer.pitch.clamp(0, 512);
    let advance = div_round(delta_ms * (speed + 256), 512);
    layer.position_ms = layer.position_ms.saturating_add(advance);

    // END 边界：每次时钟推进检查一次（引擎在每个音频缓冲回调里检查一次，
    // 不是无限 while；跨边界的余量不携带，直接置 RESTART）。
    if layer.position_ms >= layer.end_ms {
        let next = layer.loop_count.saturating_add(1);
        // 引擎是 `count+1 == LOOP`，不是 `>=`（REA 0x464960）。
        if layer.loop_setting != 0 && next == layer.loop_setting {
            layer.loop_setting = 0;
            layer.loop_count = 0;
            layer.position_ms = 0;
            // 0x464960 在这条分支调用 FUN_00423940：释放缓冲并清
            // START(+0x128)/RESTART(+0x12c)。
            layer.start_ms = 0;
            layer.restart_ms = 0;
            layer.state = PlayState::Stopped;
        } else {
            layer.loop_count = next;
            layer.position_ms = layer.restart_ms;
        }
    }
}

fn div_round(num: i64, den: i64) -> i64 {
    if den == 0 {
        return 0;
    }
    if num >= 0 {
        (num + den / 2) / den
    } else {
        -((-num + den / 2) / den)
    }
}

/// 引擎的文件名归一化（REA `0x45f348`）：ASCII 大写→小写，`\`→`/`。
pub fn normalize_name(file: &[u8]) -> Vec<u8> {
    file.iter()
        .map(|&b| match b {
            b'\\' => b'/',
            b'A'..=b'Z' => b + 0x20,
            other => other,
        })
        .collect()
}

/// 引擎扩展名探测序（手册原文 + REA `0x40c9b4`）：
/// 已带 `.ogg`/`.wav` 则原样；否则 `(无扩展名) → .ogg → .wav`。
/// 二进制里的追加串是小写（`0x8079dc/0x8079e4`）。
pub fn probe_names(file: &[u8]) -> Vec<Vec<u8>> {
    let mut out = vec![file.to_vec()];
    let lower = file.to_ascii_lowercase();
    if !(lower.ends_with(b".ogg") || lower.ends_with(b".wav")) {
        let mut ogg = file.to_vec();
        ogg.extend_from_slice(b".ogg");
        out.push(ogg);
        let mut wav = file.to_vec();
        wav.extend_from_slice(b".wav");
        out.push(wav);
    }
    out
}

/// START/RESTART 的引擎边界（REA `0x4251d8/0x425238`）：
/// 负值保留（延迟）；`>= total` 写 0；否则原值。
fn normalize_start(value: i64, total_ms: i64) -> i64 {
    if value < 0 {
        value
    } else if value >= total_ms {
        0
    } else {
        value
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
