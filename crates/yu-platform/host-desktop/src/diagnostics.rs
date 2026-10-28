//! 宿主侧诊断账本:把 `Engine` 已公开的读数聚合成 F1 面板能显示的形状。
//!
//! # 边界
//!
//! 本模块**只消费** `yu-cli::Engine` 的公开 API(`last_report` /
//! `render_report` / `take_diags` / `take_sound_diags` / `input_settings` /
//! `script_errors` / `main_finished` / `frame` / `held_buttons`)。
//! 内核里没有公开接口的读数(等待任务 label、`_KEYLAST/_MOUSELAST`、
//! `DomainCall` 的 script/pc)在这里**显式缺席**,由面板标成
//! `n/a · H-P1-02`,不猜值(A5)。
//!
//! 这是“为什么卡”的唯一判定处:画面指纹、逻辑帧推进、Fatal 挂号与
//! script 错误四路证据合成一条人话提示。

use std::collections::{BTreeMap, VecDeque};

use yu_engine::engine::{Engine, FrameReport};
use yu_log::{Diag, Loc, Severity};

/// 面板最多显示多少条挂号(按最近帧排序)。
pub const MAX_DIAG_ROWS: usize = 50;
/// 最近 Diag 明细保留条数。
const RECENT_EVENTS: usize = 200;
/// “画面连续 N 帧不变”进入提示的阈值(60fps ≈ 2 秒)。
const STUCK_PIXEL_FRAMES: u64 = 120;
/// “逻辑帧连续 N 帧未推进”的阈值。
const STUCK_LOGIC_FRAMES: u64 = 60;

/// 一条挂号(按 `code` 聚合)。
#[derive(Clone, Debug)]
pub struct DiagRow {
    pub code: String,
    pub severity: Severity,
    pub count: u64,
    pub first_frame: u64,
    pub last_frame: u64,
    pub last_detail: String,
    pub last_where: String,
}

/// 最近一条 Diag 明细。
#[derive(Clone, Debug)]
pub struct DiagEvent {
    pub frame: u64,
    pub code: String,
    pub severity: Severity,
    pub detail: String,
    pub where_: String,
}

/// 输入计数器快照(宿主观测量)。
#[derive(Clone, Debug, Default)]
pub struct InputCounters {
    /// 每个引擎按钮名的“按下沿”次数。
    pub press_edges: BTreeMap<String, u64>,
    pub release_edges: u64,
    pub wheel_pulses: u64,
    pub clicks: u64,
    /// 最近一次键盘按下(按钮名,帧号)。
    pub last_key: Option<(String, u64)>,
    /// 最近一次鼠标按键按下(按钮名,帧号)。
    pub last_mouse: Option<(String, u64)>,
    /// 最近一次按下沿发生的逻辑帧(判断“刚点过但画面没变”)。
    pub last_press_frame: Option<u64>,
    /// 同一逻辑帧内收到的按下沿数(快速连点观测)。
    pub max_press_edges_in_frame: u64,
    pub current_frame_edges: u64,
    pub current_frame_index: u64,
}

impl InputCounters {
    /// 记录一次按下沿。`frame` 是宿主观测到的逻辑帧号。
    pub fn press(&mut self, button: &str, frame: u64) {
        if self.current_frame_index != frame {
            self.current_frame_index = frame;
            self.current_frame_edges = 0;
        }
        self.current_frame_edges += 1;
        self.max_press_edges_in_frame = self.max_press_edges_in_frame.max(self.current_frame_edges);
        self.last_press_frame = Some(frame);
        *self.press_edges.entry(button.to_string()).or_insert(0) += 1;
        if button.starts_with("KEY_") {
            self.last_key = Some((button.to_string(), frame));
        } else if button.starts_with("MOUSE_") {
            self.last_mouse = Some((button.to_string(), frame));
            if !is_wheel(button) {
                self.clicks += 1;
            }
        }
    }

    pub fn release(&mut self) {
        self.release_edges += 1;
    }

    /// 按次数排序的前 N 个按钮。
    pub fn top(&self, limit: usize) -> Vec<(String, u64)> {
        let mut list: Vec<(String, u64)> = self
            .press_edges
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect();
        list.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        list.truncate(limit);
        list
    }
}

/// 滚轮脉冲(`_MOUSE_U/_MOUSE_D`)不是“点击”。
fn is_wheel(button: &str) -> bool {
    button == "MOUSE_U" || button == "MOUSE_D"
}

/// 一帧的卡住判定。
#[derive(Clone, Debug, Default)]
pub struct StuckReport {
    /// 引擎实际渲染出的逻辑帧号。
    pub engine_frame: u64,
    /// 逻辑帧连续未推进的宿主帧数。
    pub logic_stall_frames: u64,
    /// 画面指纹连续相同的宿主帧数。
    pub pixel_stall_frames: u64,
    /// 本帧是否可绘制。
    pub drawable: bool,
    /// 主任务是否已 END。
    pub main_finished: bool,
    /// 最近一条 Fatal(码,帧,位置)。
    pub last_fatal: Option<(String, u64, String)>,
    /// 最近一条脚本错误。
    pub last_script_error: Option<String>,
}

impl StuckReport {
    /// 人话提示:为什么画面不动 / 为什么点了没反应。
    ///
    /// 证据优先级:Fatal > 逻辑帧未推进 > 无可绘制输出 > 主任务结束 >
    /// 画面未变化 > skipped > 正常。
    pub fn hint(&self) -> String {
        if let Some((code, frame, where_)) = &self.last_fatal {
            return format!(
                "Fatal `{code}` @ {where_} (frame {frame}) ⇒ 该脚本任务已终止;\
                 后续输入不会推进"
            );
        }
        if self.logic_stall_frames >= STUCK_LOGIC_FRAMES {
            return format!(
                "逻辑帧停在 {} 已 {} 宿主帧:主任务在等待输入或阻塞(等待任务 label 需 H-P1-02)",
                self.engine_frame, self.logic_stall_frames
            );
        }
        if !self.drawable {
            return "本帧无可绘制输出(drawable=false):脚本未产出图层(黑屏)".to_string();
        }
        if self.main_finished {
            return "主任务已 END:引擎不再推进(宿主应退出)".to_string();
        }
        if self.pixel_stall_frames >= STUCK_PIXEL_FRAMES {
            return format!(
                "画面像素连续 {} 宿主帧相同(逻辑帧仍在推进):在等输入;若刚点击过,说明该点击没有命中按钮",
                self.pixel_stall_frames
            );
        }
        if let Some(error) = &self.last_script_error {
            return format!("执行器报错:{error}");
        }
        "推进正常".to_string()
    }
}

/// F1 面板的一帧快照(在 UI 闭包外复制,避免借用冲突)。
#[derive(Clone, Debug, Default)]
pub struct Diagnostics {
    pub diag_rows: Vec<DiagRow>,
    pub recent: VecDeque<DiagEvent>,
    pub total_diags: u64,
    /// 本帧 skipped ops 聚合(`kind:reason` → count),降序。
    pub frame_skips: Vec<(String, u32)>,
    pub frame_missing_assets: Vec<String>,
    pub frame_skipped_text: usize,
    /// 本帧 Unimplemented 计数(码 → 次数),降序。
    pub frame_unimplemented: Vec<(String, u64)>,
    /// 累计 Unimplemented 计数(码 → 次数),降序。
    pub total_unimplemented: Vec<(String, u64)>,
    pub input: InputCounters,
    pub stuck: StuckReport,
    /// 画面采样指纹(非密码学;仅用于“是否变化”)。
    pub fingerprint: u64,
    pub host_frame: u64,
}

/// 账本本体(跨帧累计)。
#[derive(Default)]
pub struct DiagLedger {
    rows: BTreeMap<String, DiagRow>,
    recent: VecDeque<DiagEvent>,
    total_diags: u64,
    cumulative_unimplemented: BTreeMap<String, u64>,
    last_engine_frame: u64,
    logic_stall_frames: u64,
    last_fingerprint: u64,
    pixel_stall_frames: u64,
    last_fatal: Option<(String, u64, String)>,
}

impl DiagLedger {
    /// 记录本帧诊断(引擎域 + 声音域)。
    fn ingest(&mut self, engine_frame: u64, diags: Vec<Diag>) -> Vec<(String, u64)> {
        let mut frame_unimplemented: BTreeMap<String, u64> = BTreeMap::new();
        for diag in diags {
            self.total_diags += 1;
            let code = diag.code.to_string();
            let where_ = format_loc(&diag.where_);
            let row = self.rows.entry(code.clone()).or_insert_with(|| DiagRow {
                code: code.clone(),
                severity: diag.severity,
                count: 0,
                first_frame: engine_frame,
                last_frame: engine_frame,
                last_detail: String::new(),
                last_where: String::new(),
            });
            row.count += 1;
            row.last_frame = engine_frame;
            row.last_detail = diag.detail.clone();
            row.last_where = where_.clone();
            if diag.severity == Severity::Unimplemented {
                *frame_unimplemented.entry(code.clone()).or_insert(0) += 1;
                *self
                    .cumulative_unimplemented
                    .entry(code.clone())
                    .or_insert(0) += 1;
            }
            if diag.severity == Severity::Fatal {
                self.last_fatal = Some((code.clone(), engine_frame, where_.clone()));
                // Fatal 是“脚本任务终止”的唯一确定信号:立刻进日志,
                // 不等到退出摘要(否则日志顺序无法定位发生时刻)。
                log::error!(
                    target: "yu-desktop",
                    "FATAL {} at {} engine_frame={} detail={}",
                    code,
                    where_,
                    engine_frame,
                    diag.detail
                );
            }
            if self.recent.len() == RECENT_EVENTS {
                self.recent.pop_front();
            }
            self.recent.push_back(DiagEvent {
                frame: engine_frame,
                code,
                severity: diag.severity,
                detail: diag.detail,
                where_,
            });
        }
        let mut list: Vec<(String, u64)> = frame_unimplemented.into_iter().collect();
        list.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        list
    }

    /// 推进“画面是否变化”的判定。
    fn observe_pixels(&mut self, fingerprint: u64) {
        if fingerprint == self.last_fingerprint {
            self.pixel_stall_frames = self.pixel_stall_frames.saturating_add(1);
        } else {
            self.pixel_stall_frames = 0;
            self.last_fingerprint = fingerprint;
        }
    }

    fn observe_logic(&mut self, engine_frame: u64) {
        if engine_frame == self.last_engine_frame {
            self.logic_stall_frames = self.logic_stall_frames.saturating_add(1);
        } else {
            self.logic_stall_frames = 0;
            self.last_engine_frame = engine_frame;
        }
    }

    /// 生成面板快照。
    fn snapshot(
        &self,
        report: &FrameReport,
        main_finished: bool,
        error: Option<String>,
    ) -> Diagnostics {
        let mut rows: Vec<DiagRow> = self.rows.values().cloned().collect();
        rows.sort_by(|a, b| {
            b.last_frame
                .cmp(&a.last_frame)
                .then_with(|| b.count.cmp(&a.count))
                .then_with(|| a.code.cmp(&b.code))
        });
        rows.truncate(MAX_DIAG_ROWS);

        let mut cumulative: Vec<(String, u64)> = self
            .cumulative_unimplemented
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect();
        cumulative.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        cumulative.truncate(12);

        Diagnostics {
            diag_rows: rows,
            recent: self.recent.clone(),
            total_diags: self.total_diags,
            frame_skips: Vec::new(),
            frame_missing_assets: Vec::new(),
            frame_skipped_text: 0,
            frame_unimplemented: Vec::new(),
            total_unimplemented: cumulative,
            input: InputCounters::default(),
            stuck: StuckReport {
                engine_frame: report.frame,
                logic_stall_frames: self.logic_stall_frames,
                pixel_stall_frames: self.pixel_stall_frames,
                drawable: report.drawable,
                main_finished,
                last_fatal: self.last_fatal.clone(),
                last_script_error: error,
            },
            fingerprint: self.last_fingerprint,
            host_frame: 0,
        }
    }
}

/// 宿主诊断总成:一帧一次 `observe`。
#[derive(Default)]
pub struct DiagnosticsHost {
    ledger: DiagLedger,
    input: InputCounters,
    frame_skips: Vec<(String, u32)>,
    frame_missing_assets: Vec<String>,
    frame_skipped_text: usize,
    frame_unimplemented: Vec<(String, u64)>,
    last_snapshot: Diagnostics,
    last_engine_frame: u64,
    /// 统一控制台(Q1 R2):同一次 drain 两路消费——面板聚合 + 控制台行;
    /// `None` = 旧行为(只进面板;测试/嵌入)。
    console: Option<yu_log::console::ConsoleHandle>,
}

impl DiagnosticsHost {
    /// 注入统一控制台(桌面宿主启动时一次;重启重建账本后须重注)。
    pub fn set_console(&mut self, console: Option<yu_log::console::ConsoleHandle>) {
        self.console = console;
    }

    /// 每宿主帧一次:取走引擎与声音诊断、聚合 skipped、更新卡住判定。
    pub fn observe(&mut self, engine: &mut Engine, surface: &[u8], host_frame: u64) {
        let report = engine.last_report().clone();
        let fingerprint = fingerprint(surface);
        self.ledger.observe_logic(report.frame);
        self.ledger.observe_pixels(fingerprint);

        let mut diags = engine.take_diags();
        diags.extend(engine.take_sound_diags());
        // 存档域诊断(P4 M-C 的 `sv.*`;未接 SaveHost 时为空)。
        diags.extend(engine.take_save_diags());
        // Q1 R2:同一次 drain 两路消费——控制台行(统一格式/聚合/JSONL)
        // 与面板聚合互不影响。gfx 帧诊断(S1:带脚本位置)同源出行。
        if let Some(console) = &self.console {
            for diag in &diags {
                console.emit_diag(diag);
            }
            for frame_diag in engine.frame_diagnostics() {
                console.emit_diag_counted(
                    &yu_engine::engine::frame_total_to_diag(&frame_diag),
                    frame_diag.count as u64,
                );
            }
        }
        let frame_unimplemented = self.ledger.ingest(report.frame, diags);

        let (skips, missing, skipped_text) = skip_totals(engine);
        self.frame_skips = skips;
        self.frame_missing_assets = missing;
        self.frame_skipped_text = skipped_text;
        self.frame_unimplemented = frame_unimplemented;
        self.last_engine_frame = report.frame;

        let error = engine.script_errors().last().cloned();
        let mut snapshot = self.ledger.snapshot(&report, engine.main_finished(), error);
        snapshot.frame_skips = self.frame_skips.clone();
        snapshot.frame_missing_assets = self.frame_missing_assets.clone();
        snapshot.frame_skipped_text = self.frame_skipped_text;
        snapshot.frame_unimplemented = self.frame_unimplemented.clone();
        snapshot.input = self.input.clone();
        snapshot.host_frame = host_frame;
        snapshot.fingerprint = fingerprint;
        self.last_snapshot = snapshot;
    }

    /// 面板/横幅读取的最近快照。
    pub fn snapshot(&self) -> &Diagnostics {
        &self.last_snapshot
    }

    /// 记录一次输入事件(宿主观测)。
    pub fn note_input(&mut self, button: &str, pressed: bool, frame: u64) {
        if pressed {
            self.input.press(button, frame);
        } else {
            self.input.release();
        }
    }

    pub fn note_wheel(&mut self, frame: u64) {
        self.input.wheel_pulses = self.input.wheel_pulses.saturating_add(1);
        let _ = frame;
    }

    /// 当前逻辑帧(宿主观测)。
    pub fn last_engine_frame(&self) -> u64 {
        self.last_engine_frame
    }

    /// 声音域诊断是否还有积压(`take_sound_diags` 已在 observe 里取走)。
    pub fn total_diags(&self) -> u64 {
        self.ledger.total_diags
    }
}

/// 本帧 skipped / 缺失资产 / 未栅格化文本(消费公开的 `RenderReport`)。
fn skip_totals(engine: &Engine) -> (Vec<(String, u32)>, Vec<String>, usize) {
    let Some(report) = engine.render_report() else {
        return (Vec::new(), Vec::new(), 0);
    };
    let mut map: BTreeMap<String, u32> = BTreeMap::new();
    for skip in &report.skipped {
        *map.entry(format!("{}:{}", skip.kind, skip.reason))
            .or_insert(0) += skip.count;
    }
    let mut skips: Vec<(String, u32)> = map.into_iter().collect();
    skips.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let missing = report
        .missing_assets
        .iter()
        .take(8)
        .map(|issue| format!("{}:{}", issue.code, issue.key_display()))
        .collect();
    (skips, missing, report.skipped_text)
}

/// 画面采样指纹:FNV-1a over 每 `stride` 字节。**不是**密码学哈希,
/// 只回答“这一帧与上一帧是否可能相同”;用于“画面停住”判定。
pub fn fingerprint(rgba: &[u8]) -> u64 {
    if rgba.is_empty() {
        return 0;
    }
    let stride = (rgba.len() / 512).max(1);
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut index = 0;
    while index < rgba.len() {
        hash ^= u64::from(rgba[index]);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        index += stride;
    }
    hash ^= rgba.len() as u64;
    hash
}

/// `Loc` → 稳定的一行显示。
pub fn format_loc(loc: &Loc) -> String {
    match loc {
        Loc::Script {
            file,
            label,
            pc,
            cmd,
        } => {
            let label = label.as_deref().unwrap_or("-");
            let cmd = cmd.as_deref().unwrap_or("-");
            format!("{file}:{label}@{pc} {cmd}")
        }
        Loc::Data { path, offset } => {
            format!("{}@{offset}", String::from_utf8_lossy(path))
        }
        Loc::Table { name } => format!("table:{name}"),
        Loc::None => "-".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yu_log::Boundary;

    fn diag(code: &'static str, severity: Severity, detail: &str) -> Diag {
        Diag::new(code, severity, Boundary::Script).with_detail(detail)
    }

    #[test]
    fn diag_registry_aggregates_by_code_and_keeps_last_frame() {
        let mut ledger = DiagLedger::default();
        ledger.ingest(
            10,
            vec![diag("rt.cmd.unimplemented", Severity::Unimplemented, "a")],
        );
        ledger.ingest(
            12,
            vec![diag("rt.cmd.unimplemented", Severity::Unimplemented, "b")],
        );
        ledger.ingest(
            12,
            vec![diag("snd.query.unsupported", Severity::Unimplemented, "c")],
        );
        let report = FrameReport {
            frame: 12,
            ops: 1,
            skipped: 0,
            executed: 1,
            passes: 1,
            layers: 1,
            visible: 1,
            tick_ms: 0.0,
            render_ms: 0.0,
            ms: 0.0,
            drawable: true,
            clock_ms: 0,
            lag_ms: 0,
        };
        let snap = ledger.snapshot(&report, false, None);
        assert_eq!(snap.diag_rows.len(), 2);
        let row = snap
            .diag_rows
            .iter()
            .find(|row| row.code == "rt.cmd.unimplemented")
            .unwrap();
        assert_eq!(row.count, 2);
        assert_eq!(row.first_frame, 10);
        assert_eq!(row.last_frame, 12);
        assert_eq!(row.last_detail, "b", "最近一次的 detail 覆盖旧值");
        assert_eq!(snap.recent.len(), 3, "明细事件逐条保留");
        assert_eq!(snap.recent[2].frame, 12);
    }

    #[test]
    fn fatal_wins_the_stuck_hint() {
        let mut ledger = DiagLedger::default();
        ledger.ingest(
            7,
            vec![diag("rt.expr.div_zero", Severity::Fatal, "整数除以 0")],
        );
        let report = FrameReport {
            frame: 7,
            ops: 0,
            skipped: 0,
            executed: 0,
            passes: 0,
            layers: 0,
            visible: 0,
            tick_ms: 0.0,
            render_ms: 0.0,
            ms: 0.0,
            drawable: false,
            clock_ms: 0,
            lag_ms: 0,
        };
        let snap = ledger.snapshot(&report, false, None);
        let hint = snap.stuck.hint();
        assert!(hint.contains("rt.expr.div_zero"), "hint={hint}");
        assert!(hint.contains("任务已终止"), "hint={hint}");
    }

    #[test]
    fn logic_and_pixel_stalls_are_counted_separately() {
        let mut ledger = DiagLedger::default();
        for _ in 0..3 {
            ledger.observe_logic(5);
            ledger.observe_pixels(42);
        }
        // 第一次 observe 记录基线,后两次才算“相同”。
        assert_eq!(ledger.logic_stall_frames, 2);
        assert_eq!(ledger.pixel_stall_frames, 2);
        ledger.observe_logic(6);
        ledger.observe_pixels(43);
        assert_eq!(ledger.logic_stall_frames, 0);
        assert_eq!(ledger.pixel_stall_frames, 0);
    }

    #[test]
    fn fingerprint_detects_single_pixel_change() {
        let mut frame = vec![0u8; 800 * 600 * 4];
        let a = fingerprint(&frame);
        frame[0] = 1;
        let b = fingerprint(&frame);
        assert_ne!(a, b, "首字节变化必须改变指纹");
        frame[0] = 0;
        assert_eq!(fingerprint(&frame), a, "恢复原值后指纹相同(确定性)");
    }

    #[test]
    fn input_counters_track_edges_and_last_buttons() {
        let mut counters = InputCounters::default();
        counters.press("MOUSE_L", 10);
        counters.press("MOUSE_L", 10);
        counters.press("KEY_ENTER", 11);
        counters.press("MOUSE_U", 12);
        counters.release();
        assert_eq!(counters.press_edges["MOUSE_L"], 2);
        assert_eq!(counters.clicks, 2, "滚轮不算点击");
        assert_eq!(counters.wheel_pulses, 0, "wheel 脉冲由 note_wheel 计数");
        assert_eq!(counters.last_mouse.clone().unwrap().0, "MOUSE_U");
        assert_eq!(
            counters.last_key.clone().unwrap(),
            ("KEY_ENTER".to_string(), 11)
        );
        assert_eq!(counters.max_press_edges_in_frame, 2, "同帧连点被记录");
        assert_eq!(counters.last_press_frame, Some(12), "最近按下帧");
        assert_eq!(counters.top(1)[0], ("MOUSE_L".to_string(), 2));
    }

    #[test]
    fn loc_display_is_stable() {
        assert_eq!(
            format_loc(&Loc::script(
                "es_select.yst",
                Some("es.ScenarioScript".into()),
                153,
                Some("LET".into())
            )),
            "es_select.yst:es.ScenarioScript@153 LET"
        );
        assert_eq!(format_loc(&Loc::table("ysc")), "table:ysc");
        assert_eq!(format_loc(&Loc::None), "-");
    }
}
