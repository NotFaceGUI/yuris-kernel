//! egui 诊断面:数据是 [`crate::diagnostics::Diagnostics`] 的只读快照,
//! 这里不持有 [`crate::App`],所以可以脱离窗口做无头 UI 测试
//! (`egui::Context::run_pass` 不需要 GPU)。
//!
//! 面板分四段(对应任务书 §3.2):
//!
//! 1. **为什么卡** —— `StuckReport::hint()`,四路证据(无可绘制输出 /
//!    逻辑帧不推进 / Fatal / 画面不变)+ 已知 API 缺口;
//! 2. **挂号表** —— 按最近帧排序的前 50 条 Diag(码、严重级、次数、最近帧);
//! 3. **本帧丢弃** —— skipped ops top-N、缺失资产、未栅格化文本、
//!    Unimplemented top-N;
//! 4. **输入** —— 按下沿计数、`_KEYLAST/_MOUSELAST` 的宿主等价物、
//!    光标与按住集合。

use egui::{Color32, RichText};

use crate::diagnostics::Diagnostics;
use crate::stepper::{StepperActions, StepperUi, TickKind};

/// 红色横幅(panic)。
pub const PANIC_RED: Color32 = Color32::from_rgb(176, 24, 24);
/// 琥珀横幅(卡住 / Fatal 挂号)。
pub const ATTENTION_AMBER: Color32 = Color32::from_rgb(150, 96, 8);

/// 渲染曲线保留的样本数(宿主帧);约 5 秒 @60fps。
pub const RENDER_GRAPH_SAMPLES: usize = 300;
/// 曲线分级阈值:低于 8ms 绿,8–16.7ms 黄,≥16.7ms(60fps 帧预算)红。
const GRAPH_GREEN_MS: f32 = 8.0;
const GRAPH_RED_MS: f32 = 16.7;
const GRAPH_GREEN: Color32 = Color32::from_rgb(72, 190, 96);
const GRAPH_YELLOW: Color32 = Color32::from_rgb(226, 184, 64);
const GRAPH_RED: Color32 = Color32::from_rgb(226, 72, 64);

fn graph_color(ms: f32) -> Color32 {
    if ms < GRAPH_GREEN_MS {
        GRAPH_GREEN
    } else if ms < GRAPH_RED_MS {
        GRAPH_YELLOW
    } else {
        GRAPH_RED
    }
}

/// 保留 RGB、换掉 alpha(面积渐变从线色渐隐到全透明)。
fn with_alpha(color: Color32, alpha: u8) -> Color32 {
    let [r, g, b, _] = color.to_array();
    Color32::from_rgba_unmultiplied(r, g, b, alpha)
}

/// 面板/横幅需要的全部读数(逐帧复制;`diagnostics` 借用,不 clone 大表)。
pub struct UiSnapshot<'a> {
    pub backend: String,
    pub frame: u64,
    pub viewport: (u32, u32),
    pub window: (u32, u32),
    pub tick_ms: f64,
    pub render_ms: f64,
    pub tick_avg: f64,
    pub render_avg: f64,
    pub ops: u32,
    pub drawn: u32,
    pub skipped: u32,
    pub passes: u32,
    pub layers: u32,
    pub visible: u32,
    pub drawable: bool,
    pub textures_cached: usize,
    pub textures_decoded: usize,
    pub events_drained: u64,
    pub events_dropped: u64,
    pub sound_layers: usize,
    pub cursor: (i64, i64),
    pub held: String,
    pub gpu: String,
    pub main_finished: bool,
    pub diagnostics: &'a Diagnostics,
    /// `MOUSE[MODE/TIME/CURSOR]` 的脚本当前值。
    pub mouse_mode: Option<i64>,
    pub mouse_time_ms: Option<i64>,
    pub cursor_visible: Option<bool>,
    /// 是否显示卡住横幅(用户可在面板关掉)。
    pub show_attention: bool,
    /// 统一控制台的最近行快照(明文无色;Q1 R4「日志」页签)。
    pub log_lines: Vec<String>,
    /// 帧步进器读数(面板可编辑项也走这里:改完由 `apply_ui` 回写)。
    pub stepper: StepperUi,
}

/// 面板上可触发的动作(宿主在 UI 闭包外执行)。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UiActions {
    pub restart: bool,
    pub hide: bool,
    pub quit: bool,
    /// `Some(false)` = 用户关掉卡住横幅;`Some(true)` = 重新打开。
    pub show_attention: Option<bool>,
    /// 清空「日志」页签的环形缓冲(只清面板视图,不动 JSONL 文件)。
    pub clear_log: bool,
}

/// F1 主面板。
pub fn debug_window(ctx: &egui::Context, snap: &UiSnapshot<'_>) -> UiActions {
    let mut actions = UiActions::default();
    egui::Window::new("yu-desktop")
        .default_pos([12.0, 12.0])
        .default_width(430.0)
        .resizable(true)
        .show(ctx, |ui| {
            why_stuck(ui, snap);
            ui.separator();
            stats(ui, snap);
            ui.separator();
            diag_table(ui, snap.diagnostics);
            ui.separator();
            log_tab(ui, snap, &mut actions);
            ui.separator();
            frame_drops(ui, snap.diagnostics);
            ui.separator();
            inputs(ui, snap);
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Restart").clicked() {
                    actions.restart = true;
                }
                if ui.button("Hide (F1)").clicked() {
                    actions.hide = true;
                }
                if ui.button("Quit").clicked() {
                    actions.quit = true;
                }
            });
            ui.horizontal(|ui| {
                if snap.show_attention {
                    if ui.button("Hide stuck banner").clicked() {
                        actions.show_attention = Some(false);
                    }
                } else if ui.button("Show stuck banner").clicked() {
                    actions.show_attention = Some(true);
                }
                ui.label("F1 toggles this panel");
            });
            ui.separator();
            ui.label(format!("GPU: {}", snap.gpu));
        });
    actions
}

/// 帧步进器窗口(独立窗口:`--debug-ui`/F1 同开关)。
///
/// 两个暂停分开显示、分开控制,因为它们回答的是两个不同问题:
///
/// - **录制暂停**:「我要取现场,但还没到那一刻」—— 点录制自动进入,
///   停住后由「推进 N 帧」一帧帧走到目标帧;
/// - **游戏暂停**:「先别动」—— 随时可用,不依赖录制。
///
/// 任一为真引擎都不推进(缓存见 [`crate::stepper::Stepper::tick_kind`])。
/// 推进用固定步长,所以第 N 帧的画面在任何机器上都一致 —— 这才是复现
/// 中间帧(如眨眼)需要的口径。
pub fn stepper_window(ctx: &egui::Context, stepper: &mut StepperUi) -> StepperActions {
    let mut actions = StepperActions::default();
    egui::Window::new("帧步进器")
        .default_pos([470.0, 170.0])
        .default_width(430.0)
        .resizable(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                let (color, tag) = match stepper.tick_kind {
                    TickKind::Live => (Color32::from_rgb(64, 160, 64), "LIVE"),
                    TickKind::Step => (Color32::from_rgb(200, 150, 40), "STEP"),
                    TickKind::Halt => (Color32::from_rgb(200, 80, 60), "HALT"),
                };
                ui.colored_label(color, RichText::new(tag).strong());
                ui.monospace(format!(
                    "engine frame {} · clock {} ms",
                    stepper.engine_frame, stepper.clock_ms
                ));
            });
            ui.horizontal(|ui| {
                ui.label("暂停");
                if ui.checkbox(&mut stepper.paused, "手动 (F5)").changed() {
                    actions.toggle_pause = true;
                }
                if ui
                    .checkbox(&mut stepper.record_pause, "录制 (自动)")
                    .on_hover_text("点「录制」自动进入;取消它等于结束录制")
                    .changed()
                {
                    // 显式设值:勾上 = 开始录制并暂停,取消 = 结束录制。
                    actions.record_pause = Some(stepper.record_pause);
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("⏵ 推进 N 帧 (F6)").clicked() {
                    actions.step = true;
                }
                if ui.button("⏵ 1 帧 (F7)").clicked() {
                    actions.step_one = true;
                }
                if stepper.pending_steps > 0 {
                    ui.colored_label(
                        Color32::from_rgb(200, 150, 40),
                        format!("剩 {}", stepper.pending_steps),
                    );
                }
            });
            ui.horizontal(|ui| {
                ui.label("每次推进");
                ui.add(
                    egui::DragValue::new(&mut stepper.step_frames)
                        .speed(1.0)
                        .range(1..=crate::stepper::MAX_STEP_FRAMES),
                );
                ui.label("帧(默认 10)");
            });
            ui.separator();
            ui.horizontal(|ui| {
                let record_label = if stepper.recording {
                    "⏹ 停止录制 (F8)"
                } else {
                    "⏺ 录制 (F8)"
                };
                if ui.button(record_label).clicked() {
                    actions.toggle_record = true;
                }
                if ui
                    .add_enabled(stepper.ring_len > 0, egui::Button::new("⤓ 抓取 (F9)"))
                    .on_hover_text("把环形缓冲里的帧全部落盘")
                    .clicked()
                {
                    actions.capture_now = true;
                }
                if ui
                    .add_enabled(stepper.ring_len > 0, egui::Button::new("清空缓冲"))
                    .clicked()
                {
                    actions.clear_ring = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label("环形缓冲");
                ui.add(
                    egui::DragValue::new(&mut stepper.ring_frames)
                        .speed(1.0)
                        .range(1..=600usize),
                );
                ui.label("帧");
                ui.monospace(format!(
                    "· 现有 {} · 已落盘 {} · 静态跳过 {}",
                    stepper.ring_len, stepper.captured, stepper.live_skipped
                ));
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut stepper.save_png, "每帧存 PNG");
                ui.checkbox(&mut stepper.include_display, "含 display JSON")
                    .on_hover_text("合成帧 JSON;体积大,只在需要逐 op 复盘时开");
            });
            ui.horizontal(|ui| {
                ui.label("输出目录");
                ui.add(
                    egui::TextEdit::singleline(&mut stepper.dir)
                        .desired_width(f32::INFINITY)
                        .hint_text("target/tmp/debug-ui/s<unix>"),
                );
            });
            if let Some(file) = &stepper.last_file {
                ui.monospace(format!("最近: {file}"));
            }
            if let Some(error) = &stepper.last_error {
                let error = error.clone();
                ui.colored_label(PANIC_RED, format!("抓取失败: {error}"));
            }
            ui.separator();
            ui.weak(
                "HALT 时引擎不 tick、逻辑帧与时钟都不走;推进用固定步长 \
                 (1000/fps)保证第 N 帧可复现。",
            );
        });
    actions
}

/// 渲染耗时曲线(独立窗口,随 F1 / `--debug-ui` 一起显隐)。
///
/// 每个宿主帧一个样本,横轴按固定容量滚动、最新样本贴右边;线色按
/// 当前样本耗时分级:绿 < 8ms、黄 < 16.7ms、红 ≥ 16.7ms。曲线下方
/// 是同色到透明的渐变面积。
pub fn render_graph_window(ctx: &egui::Context, samples: &std::collections::VecDeque<f32>) {
    egui::Window::new("render time (ms)")
        .default_pos([470.0, 12.0])
        .default_width(430.0)
        .resizable(true)
        .collapsible(true)
        .show(ctx, |ui| {
            let last = samples.back().copied().unwrap_or(0.0);
            let max = samples.iter().copied().fold(0.0_f32, f32::max);
            let avg = if samples.is_empty() {
                0.0
            } else {
                samples.iter().sum::<f32>() / samples.len() as f32
            };
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{last:.2} ms"))
                        .color(graph_color(last))
                        .strong(),
                );
                ui.weak(format!(
                    "avg {avg:.2} · max {max:.2} · {} frames",
                    samples.len()
                ));
            });

            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width().max(160.0), 130.0),
                egui::Sense::hover(),
            );
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 4.0, Color32::from_rgb(20, 20, 22));

            // 纵轴按窗口内最大值自适应;至少留到 1.5 倍红线,避免曲线贴顶。
            let top = max.max(GRAPH_RED_MS * 1.5).max(1.0);
            let y_of = |ms: f32| rect.bottom() - (ms / top) * rect.height();
            // 横轴固定容量:x 间距不随样本数变化,曲线从右往左生长。
            let dx = rect.width() / (RENDER_GRAPH_SAMPLES - 1) as f32;

            // 同色到透明的渐变面积:每段两个三角形,顶点色由线色渐隐到底边透明。
            let area_alpha = 64;
            let mut mesh = egui::Mesh::default();
            let mut prev: Option<(egui::Pos2, Color32)> = None;
            for (i, ms) in samples.iter().enumerate() {
                let from_right = samples.len() - 1 - i;
                let pos = egui::pos2(rect.right() - from_right as f32 * dx, y_of(*ms));
                let color = graph_color(*ms);
                if let Some((prev_pos, prev_color)) = prev {
                    let base = mesh.vertices.len() as u32;
                    mesh.colored_vertex(prev_pos, with_alpha(prev_color, area_alpha));
                    mesh.colored_vertex(
                        egui::pos2(prev_pos.x, rect.bottom()),
                        with_alpha(prev_color, 0),
                    );
                    mesh.colored_vertex(egui::pos2(pos.x, rect.bottom()), with_alpha(color, 0));
                    mesh.colored_vertex(pos, with_alpha(color, area_alpha));
                    mesh.add_triangle(base, base + 1, base + 2);
                    mesh.add_triangle(base, base + 2, base + 3);
                }
                prev = Some((pos, color));
            }
            painter.add(egui::Shape::mesh(mesh));

            for (ms, color) in [(GRAPH_GREEN_MS, GRAPH_YELLOW), (GRAPH_RED_MS, GRAPH_RED)] {
                let y = y_of(ms);
                painter.line_segment(
                    [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                    egui::Stroke::new(1.0, color.gamma_multiply(0.45)),
                );
            }

            for i in 1..samples.len() {
                let from_right_prev = samples.len() - i;
                let from_right_cur = samples.len() - 1 - i;
                painter.line_segment(
                    [
                        egui::pos2(rect.right() - from_right_prev as f32 * dx, y_of(samples[i - 1])),
                        egui::pos2(rect.right() - from_right_cur as f32 * dx, y_of(samples[i])),
                    ],
                    egui::Stroke::new(1.5, graph_color(samples[i])),
                );
            }
            if let Some(last) = samples.back() {
                painter.circle_filled(
                    egui::pos2(rect.right(), y_of(*last)),
                    2.0,
                    graph_color(*last),
                );
            }
            painter.text(
                egui::pos2(rect.left() + 4.0, rect.top() + 2.0),
                egui::Align2::LEFT_TOP,
                format!("0–{top:.1} ms"),
                egui::FontId::monospace(10.0),
                Color32::GRAY,
            );

            ui.horizontal(|ui| {
                ui.colored_label(GRAPH_GREEN, format!("< {GRAPH_GREEN_MS:.0} ms"));
                ui.colored_label(GRAPH_YELLOW, format!("< {GRAPH_RED_MS:.1} ms"));
                ui.colored_label(GRAPH_RED, format!("≥ {GRAPH_RED_MS:.1} ms"));
            });
        });
}

fn why_stuck(ui: &mut egui::Ui, snap: &UiSnapshot<'_>) {
    let stuck = &snap.diagnostics.stuck;
    let hint = stuck.hint();
    let normal = hint == "推进正常";
    ui.horizontal(|ui| {
        ui.label(RichText::new("为什么卡").strong());
        let (color, tag) = if normal {
            (Color32::from_rgb(64, 160, 64), "OK")
        } else if stuck.last_fatal.is_some() {
            (PANIC_RED, "FATAL")
        } else {
            (ATTENTION_AMBER, "WAIT")
        };
        ui.colored_label(color, tag);
    });
    ui.colored_label(
        if normal {
            Color32::GRAY
        } else {
            Color32::WHITE
        },
        hint,
    );
    egui::Grid::new("yu-desktop-stuck")
        .num_columns(2)
        .spacing([12.0, 2.0])
        .show(ui, |ui| {
            ui.label("logic stall");
            ui.monospace(format!(
                "{} host frames (engine frame {})",
                stuck.logic_stall_frames, stuck.engine_frame
            ));
            ui.end_row();
            ui.label("pixel stall");
            ui.monospace(format!("{} host frames", stuck.pixel_stall_frames));
            ui.end_row();
            ui.label("drawable");
            ui.monospace(stuck.drawable.to_string());
            ui.end_row();
            ui.label("main_finished");
            ui.monospace(snap.main_finished.to_string());
            ui.end_row();
            ui.label("waiting task");
            ui.monospace("n/a · H-P1-02");
            ui.end_row();
            ui.label("DomainCall pc");
            ui.monospace("n/a · P3");
            ui.end_row();
        });
    if let Some(error) = &stuck.last_script_error {
        ui.colored_label(ATTENTION_AMBER, format!("script error: {error}"));
    }
}

fn stats(ui: &mut egui::Ui, snap: &UiSnapshot<'_>) {
    egui::CollapsingHeader::new("frame / render")
        .default_open(true)
        .show(ui, |ui| {
            egui::Grid::new("yu-desktop-stats")
                .num_columns(2)
                .spacing([12.0, 2.0])
                .show(ui, |ui| {
                    let mut row = |name: &str, value: String| {
                        ui.label(name);
                        ui.monospace(value);
                        ui.end_row();
                    };
                    row("frame", snap.frame.to_string());
                    row("backend", snap.backend.clone());
                    row(
                        "viewport",
                        format!("{}x{}", snap.viewport.0, snap.viewport.1),
                    );
                    row("window", format!("{}x{}", snap.window.0, snap.window.1));
                    row(
                        "tick",
                        format!("{:.2} ms (avg {:.2})", snap.tick_ms, snap.tick_avg),
                    );
                    row(
                        "render",
                        format!("{:.2} ms (avg {:.2})", snap.render_ms, snap.render_avg),
                    );
                    row(
                        "ops",
                        format!(
                            "{} drawn / {} skipped ({} total)",
                            snap.drawn, snap.skipped, snap.ops
                        ),
                    );
                    row("passes", snap.passes.to_string());
                    row(
                        "layers",
                        format!("{} / {} visible", snap.layers, snap.visible),
                    );
                    row("drawable", snap.drawable.to_string());
                    row(
                        "textures",
                        format!(
                            "{} cached / {} decoded",
                            snap.textures_cached, snap.textures_decoded
                        ),
                    );
                    row(
                        "events",
                        format!(
                            "{} drained / {} dropped",
                            snap.events_drained, snap.events_dropped
                        ),
                    );
                    row("sound", format!("{} layers", snap.sound_layers));
                });
        });
}

fn diag_table(ui: &mut egui::Ui, diagnostics: &Diagnostics) {
    egui::CollapsingHeader::new(format!(
        "挂号表 ({} codes / {} events)",
        diagnostics.diag_rows.len(),
        diagnostics.total_diags
    ))
    .default_open(true)
    .show(ui, |ui| {
        if diagnostics.diag_rows.is_empty() {
            ui.label("(无诊断)");
            return;
        }
        egui::ScrollArea::vertical()
            .max_height(190.0)
            .show(ui, |ui| {
                egui::Grid::new("yu-desktop-diags")
                    .num_columns(4)
                    .striped(true)
                    .spacing([8.0, 2.0])
                    .show(ui, |ui| {
                        ui.label(RichText::new("code").strong());
                        ui.label(RichText::new("sev").strong());
                        ui.label(RichText::new("count").strong());
                        ui.label(RichText::new("last").strong());
                        ui.end_row();
                        for row in &diagnostics.diag_rows {
                            let color = severity_color(row.severity);
                            ui.colored_label(color, &row.code);
                            ui.colored_label(color, row.severity.as_str());
                            ui.monospace(row.count.to_string());
                            ui.monospace(row.last_frame.to_string());
                            ui.end_row();
                        }
                    });
            });
        if let Some(last) = diagnostics.recent.back() {
            ui.separator();
            ui.label(RichText::new("最近一条").strong());
            ui.monospace(format!("#{} {}", last.frame, last.code));
            ui.label(&last.where_);
            ui.label(&last.detail);
        }
    });
}

/// 「日志」页签(Q1 R4):统一控制台的最近行(明文无色),滚动缓冲 +
/// 清空按钮。清空只影响面板环形缓冲,不动 JSONL 文件记录。
fn log_tab(ui: &mut egui::Ui, snap: &UiSnapshot<'_>, actions: &mut UiActions) {
    egui::CollapsingHeader::new(format!("日志({})", snap.log_lines.len()))
        .default_open(false)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button("清空").clicked() {
                    actions.clear_log = true;
                }
                ui.weak("stderr 控制台同源;文件记录不受清空影响");
            });
            if snap.log_lines.is_empty() {
                ui.weak("(暂无;内核挂号/门面/生命周期行会出现在这里)");
                return;
            }
            egui::ScrollArea::vertical()
                .max_height(220.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    for line in &snap.log_lines {
                        ui.monospace(line);
                    }
                });
        });
}

fn frame_drops(ui: &mut egui::Ui, diagnostics: &Diagnostics) {
    egui::CollapsingHeader::new("本帧丢弃 / Unimplemented")
        .default_open(false)
        .show(ui, |ui| {
            ui.label(RichText::new("skipped ops top-N").strong());
            if diagnostics.frame_skips.is_empty() {
                ui.label("(0 skipped)");
            } else {
                for (key, count) in &diagnostics.frame_skips {
                    ui.monospace(format!("x{count:<5} {key}"));
                }
            }
            if !diagnostics.frame_missing_assets.is_empty() {
                ui.separator();
                ui.label(RichText::new("missing assets").strong());
                for asset in &diagnostics.frame_missing_assets {
                    ui.monospace(asset);
                }
            }
            if diagnostics.frame_skipped_text > 0 {
                ui.colored_label(
                    ATTENTION_AMBER,
                    format!(
                        "text runs not rasterized: {} (DIV-GFX-012/033/034/035,归 P2)",
                        diagnostics.frame_skipped_text
                    ),
                );
            }
            ui.separator();
            ui.label(RichText::new("Unimplemented 本帧 top-N").strong());
            if diagnostics.frame_unimplemented.is_empty() {
                ui.label("(0)");
            } else {
                for (code, count) in &diagnostics.frame_unimplemented {
                    ui.monospace(format!("x{count:<5} {code}"));
                }
            }
            ui.separator();
            ui.label(RichText::new("Unimplemented 累计 top-N").strong());
            for (code, count) in &diagnostics.total_unimplemented {
                ui.monospace(format!("x{count:<8} {code}"));
            }
        });
}

fn inputs(ui: &mut egui::Ui, snap: &UiSnapshot<'_>) {
    egui::CollapsingHeader::new("输入计数器")
        .default_open(true)
        .show(ui, |ui| {
            let counters = &snap.diagnostics.input;
            egui::Grid::new("yu-desktop-input")
                .num_columns(2)
                .spacing([12.0, 2.0])
                .show(ui, |ui| {
                    ui.label("cursor");
                    ui.monospace(format!("{}, {}", snap.cursor.0, snap.cursor.1));
                    ui.end_row();
                    ui.label("_KEYLAST (host)");
                    ui.monospace(last_button(&counters.last_key));
                    ui.end_row();
                    ui.label("_MOUSELAST (host)");
                    ui.monospace(last_button(&counters.last_mouse));
                    ui.end_row();
                    ui.label("press edges");
                    ui.monospace(counters.press_edges.values().sum::<u64>().to_string());
                    ui.end_row();
                    ui.label("max edges/frame");
                    ui.monospace(counters.max_press_edges_in_frame.to_string());
                    ui.end_row();
                    ui.label("clicks / wheel");
                    ui.monospace(format!("{} / {}", counters.clicks, counters.wheel_pulses));
                    ui.end_row();
                    ui.label("held");
                    ui.monospace(snap.held.clone());
                    ui.end_row();
                    ui.label("MOUSE[MODE/TIME]");
                    ui.monospace(format!("{:?} / {:?}", snap.mouse_mode, snap.mouse_time_ms));
                    ui.end_row();
                    ui.label("cursor visible");
                    ui.monospace(format!("{:?}", snap.cursor_visible));
                    ui.end_row();
                });
            let top = counters.top(8);
            if !top.is_empty() {
                ui.separator();
                for (button, count) in top {
                    ui.monospace(format!("x{count:<5} {button}"));
                }
            }
            ui.separator();
            ui.small("内核 `_KEYLAST/_MOUSELAST` 未公开 → 此处为宿主等价物 (H-P1-02)");
        });
}

fn last_button(value: &Option<(String, u64)>) -> String {
    match value {
        Some((name, frame)) => format!("{name} @ frame {frame}"),
        None => "-".to_string(),
    }
}

/// 红色 panic 横幅。返回 `true` = 用户点了 Quit。
pub fn panic_banner(ctx: &egui::Context, title: &str, body: &str) -> bool {
    let mut quit = false;
    // egui 0.37 统一成 `Panel`(旧的 `TopBottomPanel` 已移除);只有
    // `Context` 的地方用 `root_ui` 拿根 `Ui`(官方文档推荐路径)。
    ctx.root_ui(|ui| {
        egui::Panel::top("yu-desktop-panic")
            .frame(
                egui::Frame::new()
                    .fill(PANIC_RED)
                    .inner_margin(egui::Margin::symmetric(12, 8)),
            )
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(title)
                            .color(Color32::WHITE)
                            .strong()
                            .size(16.0),
                    );
                    if ui.button("Quit (Esc)").clicked() {
                        quit = true;
                    }
                });
                ui.label(RichText::new(body).color(Color32::from_rgb(255, 226, 226)));
            });
    });
    quit
}

/// 琥珀“卡住”横幅(不是 panic;引擎仍在运行,只是画面/逻辑不推进)。
pub fn attention_banner(ctx: &egui::Context, hint: &str) {
    ctx.root_ui(|ui| {
        egui::Panel::top("yu-desktop-attention")
            .frame(
                egui::Frame::new()
                    .fill(ATTENTION_AMBER)
                    .inner_margin(egui::Margin::symmetric(12, 6)),
            )
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("为什么卡").color(Color32::WHITE).strong());
                    ui.label(RichText::new(hint).color(Color32::from_rgb(255, 244, 214)));
                });
            });
    });
}

// ------------------------------------------------------------- DIALOG 模态(H-P1-04)

/// 一条待回答 DIALOG 的**呈现视图**(CP932 已解码;内核语义仍是原始字节)。
#[derive(Clone, Debug, PartialEq)]
pub struct DialogView {
    pub id: u64,
    pub caption: String,
    pub text: String,
    /// [`yu_rt::dialog::DialogButtons::as_str`](ok/ok_cancel/yes_no/yes_no_cancel)。
    pub buttons: &'static str,
    /// 1 基默认按钮(0 = 未指定)。
    pub default_button: u8,
    /// 图标名(none/information/question/warning/error)。
    pub icon: &'static str,
    /// `INPUT=1`(按钮固定 OK,文本框)。
    pub input: bool,
    /// `INPUTLIMIT`(0 = 不限)。
    pub input_limit: u32,
    /// `DEFSTR`(输入框初始文本)。
    pub default_input: String,
}

/// 用户在本帧给出的回答。
#[derive(Clone, Debug, PartialEq)]
pub struct DialogAnswer {
    pub id: u64,
    /// 1 基按钮序号(官方 MessageBox 语义,写回 `LET`)。
    pub button: u8,
    /// INPUT 模式的编辑文本(宿主负责 CP932 编码后交给内核)。
    pub input_text: Option<String>,
}

/// DIALOG 模态框(对齐官方 `MessageBoxA` 的呈现面:标题/正文/图标/
/// 按钮组/默认按钮;Esc = 含 Cancel 组合的取消)。
///
/// 官方模态期间引擎计时器暂停、不收玩家输入 —— 宿主侧的“不收输入”
/// 由 [`crate::App`] 在挂起任务层保证;这里只负责呈现与回答。
pub fn dialog_modal(
    ctx: &egui::Context,
    dialogs: &[DialogView],
    inputs: &mut std::collections::BTreeMap<u64, String>,
) -> Vec<DialogAnswer> {
    let mut answers = Vec::new();
    let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    for dialog in dialogs {
        // INPUT 型的编辑缓冲:首次出现时播种 DEFSTR。
        let mut text = if dialog.input {
            Some(
                inputs
                    .entry(dialog.id)
                    .or_insert_with(|| dialog.default_input.clone())
                    .clone(),
            )
        } else {
            None
        };
        let mut clicked: Option<u8> = None;
        egui::Window::new(dialog_window_title(dialog))
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .resizable(false)
            .default_width(420.0)
            .show(ctx, |ui| {
                if dialog.icon != "none" {
                    ui.label(icon_text(dialog.icon));
                }
                ui.add(egui::Label::new(RichText::new(&dialog.text).weak()).wrap());
                if let Some(buffer) = &mut text {
                    ui.add_space(4.0);
                    let field = egui::TextEdit::singleline(buffer)
                        .desired_width(f32::INFINITY)
                        .char_limit(dialog.input_limit.max(1) as usize);
                    ui.add(field);
                }
                ui.add_space(6.0);
                ui.with_layout(
                    egui::Layout::top_down_justified(egui::Align::Center),
                    |ui| {
                        if dialog.input {
                            // INPUT 型固定 OK(官方自绘对话框只有确定键;LET=1)。
                            if ui.button("OK").clicked() {
                                clicked = Some(1);
                            }
                        } else {
                            let count = button_count(dialog.buttons);
                            for index in 1..=count {
                                let response = ui.button(button_label(dialog.buttons, index));
                                // 官方 MessageBox 的 DEFBUTTON:回车触发默认键。
                                // 模态期间焦点归对话框,逐帧重取是安全的。
                                if index == dialog.default_button {
                                    response.request_focus();
                                }
                                if response.clicked() {
                                    clicked = Some(index);
                                }
                            }
                        }
                    },
                );
            });
        if clicked.is_none() && esc {
            // Esc = Cancel(仅组合里真的有 Cancel;MB_YESNO/MB_OK 官方不响应)。
            clicked = match dialog.buttons {
                "ok_cancel" => Some(2),
                "yes_no_cancel" => Some(3),
                _ => None,
            };
        }
        if let Some(button) = clicked {
            if dialog.input {
                inputs.remove(&dialog.id);
            }
            answers.push(DialogAnswer {
                id: dialog.id,
                button,
                input_text: text,
            });
        }
    }
    answers
}

fn dialog_window_title(dialog: &DialogView) -> String {
    if dialog.caption.is_empty() {
        "DIALOG".to_string()
    } else {
        dialog.caption.clone()
    }
}

fn button_count(buttons: &str) -> u8 {
    match buttons {
        "ok" => 1,
        "ok_cancel" | "yes_no" => 2,
        "yes_no_cancel" => 3,
        _ => 1,
    }
}

/// 按钮的日文标签(官方 MessageBox 在日文 Windows 上的 OS 文案)。
fn button_label(buttons: &str, index: u8) -> &'static str {
    match (buttons, index) {
        ("ok", 1) | ("ok_cancel", 1) => "OK",
        ("ok_cancel", 2) => "キャンセル",
        ("yes_no", 1) | ("yes_no_cancel", 1) => "はい",
        ("yes_no", 2) | ("yes_no_cancel", 2) => "いいえ",
        ("yes_no_cancel", 3) => "キャンセル",
        _ => "?",
    }
}

/// 图标占位(调试字体不保证 emoji 字形;用可渲染的 ASCII 记号)。
fn icon_text(icon: &str) -> RichText {
    let mark = match icon {
        "information" => "(i)",
        "question" => "(?)",
        "warning" => "(!)",
        "error" => "(X)",
        _ => "",
    };
    RichText::new(mark).strong()
}

fn severity_color(severity: yu_log::Severity) -> Color32 {
    match severity {
        yu_log::Severity::Fatal => PANIC_RED,
        yu_log::Severity::Recoverable => Color32::from_rgb(200, 120, 24),
        yu_log::Severity::Unimplemented => Color32::from_rgb(200, 200, 60),
        yu_log::Severity::Info => Color32::GRAY,
        // Verbose 属于可选噪声(默认不出),面板里用更暗的灰区分。
        yu_log::Severity::Verbose => Color32::from_rgb(110, 110, 110),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::{DiagRow, Diagnostics, InputCounters, StuckReport};
    use yu_log::Severity;

    fn base_snapshot<'a>(diag: &'a Diagnostics) -> UiSnapshot<'a> {
        UiSnapshot {
            backend: "wgpu".to_string(),
            frame: 404,
            viewport: (800, 600),
            window: (1280, 960),
            tick_ms: 3.0,
            render_ms: 1.0,
            tick_avg: 4.0,
            render_avg: 1.5,
            ops: 158,
            drawn: 158,
            skipped: 0,
            passes: 137,
            layers: 379,
            visible: 115,
            drawable: true,
            textures_cached: 31,
            textures_decoded: 31,
            events_drained: 100,
            events_dropped: 0,
            sound_layers: 2,
            cursor: (400, 300),
            held: "-".to_string(),
            gpu: "test".to_string(),
            main_finished: false,
            diagnostics: diag,
            mouse_mode: Some(1),
            mouse_time_ms: Some(2500),
            cursor_visible: Some(true),
            show_attention: true,
            log_lines: vec![
                "[   +0.102s] INFO  [host] data.ypf mounted".to_string(),
                "[  +12.345s] UNIMP [gfx] gfx.command.unimplemented @ s66 pc6028: …".to_string(),
            ],
            stepper: crate::stepper::StepperUi::default(),
        }
    }

    /// 无头跑 UI,返回(动作, 绘制出的全部文本, 全部填充色)。
    ///
    /// 跑**两遍**:egui 0.37 的 `Window` 在首遍只测量尺寸,第二遍才落
    /// 绘制指令(与宿主真实帧一致 —— 面板晚一帧出现,不影响诊断)。
    ///
    /// 动作类型泛型化:主面板给 [`UiActions`],步进器窗口给
    /// [`StepperActions`],两者都要求 `Default`(未点击 = 空动作)。
    fn run_ui<T: Default, F: FnMut(&egui::Context) -> T>(
        mut snapshot: F,
    ) -> (T, Vec<String>, Vec<Color32>) {
        let ctx = egui::Context::default();
        let mut actions = T::default();
        // `RawInput::default()` 没有 screen_rect:Window 会被裁成 0 面积。
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 960.0),
            )),
            ..Default::default()
        };
        let mut output = ctx.run_pass(raw.clone(), |ctx| {
            actions = snapshot(ctx);
        });
        output.textures_delta.clear();
        let mut output = ctx.run_pass(raw, |ctx| {
            actions = snapshot(ctx);
        });
        // epaint 的 `TexturesDelta` 有 Drop 守卫:测试里不上传纹理,
        // 但必须显式清空后再丢弃。
        output.textures_delta.clear();
        let mut texts = Vec::new();
        let mut fills = Vec::new();
        for shape in output.shapes {
            collect(&shape.shape, &mut texts, &mut fills);
        }
        (actions, texts, fills)
    }

    fn collect(shape: &egui::epaint::Shape, texts: &mut Vec<String>, fills: &mut Vec<Color32>) {
        match shape {
            egui::epaint::Shape::Text(text) => texts.push(text.galley.text().to_string()),
            egui::epaint::Shape::Rect(rect) => fills.push(rect.fill),
            egui::epaint::Shape::Vec(list) => {
                for item in list {
                    collect(item, texts, fills);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn panic_banner_paints_red_and_quit_button() {
        let (actions, texts, fills) = run_ui(|ctx| {
            let quit = panic_banner(ctx, "PANIC 捕获于 tick", "boom @ src/lib.rs:1:1");
            UiActions {
                quit,
                ..UiActions::default()
            }
        });
        assert!(!actions.quit, "没有点击时不退出");
        assert!(fills.contains(&PANIC_RED), "横幅必须用红色填充: {fills:?}");
        assert!(
            texts.iter().any(|text| text.contains("PANIC 捕获于 tick")),
            "横幅标题必须可见: {texts:?}"
        );
    }

    #[test]
    fn attention_banner_paints_amber_with_hint() {
        let (_actions, texts, fills) = run_ui(|ctx| {
            attention_banner(ctx, "逻辑帧停在 12 已 60 宿主帧:主任务在等待输入或阻塞");
            UiActions::default()
        });
        assert!(fills.contains(&ATTENTION_AMBER));
        assert!(
            texts.iter().any(|text| text.contains("逻辑帧停在")),
            "{texts:?}"
        );
    }

    #[test]
    fn panel_surfaces_diag_codes_and_stuck_hint() {
        let mut diag = Diagnostics::default();
        diag.diag_rows.push(DiagRow {
            code: "rt.cmd.unimplemented".to_string(),
            severity: Severity::Unimplemented,
            count: 11,
            first_frame: 1,
            last_frame: 404,
            last_detail: "ERROR: 命令无处理器".to_string(),
            last_where: "es_eris.yst:SYSTEM@12 ERROR".to_string(),
        });
        diag.stuck = StuckReport {
            engine_frame: 404,
            logic_stall_frames: 200,
            pixel_stall_frames: 158,
            drawable: true,
            main_finished: false,
            last_fatal: Some((
                "rt.expr.div_zero".to_string(),
                404,
                "es_select.yst:es.ScenarioScript@153 LET".to_string(),
            )),
            last_script_error: None,
        };
        diag.total_diags = 11;
        let snapshot = base_snapshot(&diag);
        let (_actions, texts, _fills) = run_ui(|ctx| debug_window(ctx, &snapshot));
        let joined = texts.join("\n");
        assert!(joined.contains("rt.cmd.unimplemented"), "{joined}");
        assert!(
            joined.contains("rt.expr.div_zero"),
            "Fatal 必须出现在提示里: {joined}"
        );
        assert!(joined.contains("es_select.yst"), "{joined}");
        assert!(
            joined.contains("n/a · H-P1-02"),
            "缺 API 必须显式标注: {joined}"
        );
    }

    #[test]
    fn panel_buttons_report_actions() {
        // 直接调用 build 逻辑无法模拟点击;这里验证返回值默认为空动作,
        // 真实点击路径由 egui 的交互状态驱动(窗口级人工验收)。
        let diag = Diagnostics::default();
        let snapshot = base_snapshot(&diag);
        let (actions, _texts, _fills) = run_ui(|ctx| debug_window(ctx, &snapshot));
        assert_eq!(actions, UiActions::default());
    }

    #[test]
    fn last_button_formatting_is_readable() {
        assert_eq!(last_button(&None), "-");
        assert_eq!(
            last_button(&Some(("MOUSE_L".to_string(), 300))),
            "MOUSE_L @ frame 300"
        );
    }

    // ------------------------------------------------------ 帧步进器面板

    /// 面板必须同时暴露**两个暂停**(手动 / 录制自动)、推进控件、帧数
    /// 配置与抓取按钮 —— 少一个就说明接线掉了。
    #[test]
    fn stepper_panel_shows_both_pauses_and_step_controls() {
        let mut stepper = crate::stepper::StepperUi {
            engine_frame: 348,
            clock_ms: 5800,
            ..crate::stepper::StepperUi::default()
        };
        let (actions, texts, _fills) = run_ui(|ctx| stepper_window(ctx, &mut stepper));
        assert_eq!(actions, StepperActions::default(), "未点击时不产生动作");
        let all = texts.join("\n");
        for needle in [
            "手动 (F5)",
            "录制 (自动)",
            "推进 N 帧 (F6)",
            "1 帧 (F7)",
            "录制 (F8)",
            "抓取 (F9)",
            "环形缓冲",
            "每帧存 PNG",
            "输出目录",
            "HALT",
        ] {
            assert!(all.contains(needle), "面板缺少 `{needle}`: {all}");
        }
        assert!(
            all.contains("engine frame 348"),
            "必须显示当前逻辑帧(定帧排查要看它): {all}"
        );
    }

    /// HALT 与 LIVE 的标签必须能区分:暂停时读数不能还写着 LIVE。
    #[test]
    fn stepper_panel_tag_follows_tick_kind() {
        let mut stepper = crate::stepper::StepperUi::default();
        let (_a, texts, _f) = run_ui(|ctx| stepper_window(ctx, &mut stepper));
        assert!(texts.iter().any(|t| t.contains("LIVE")), "{texts:?}");

        stepper.paused = true;
        stepper.tick_kind = TickKind::Halt;
        let (_a, texts, _f) = run_ui(|ctx| stepper_window(ctx, &mut stepper));
        let all = texts.join("\n");
        assert!(all.contains("HALT"), "{all}");
        assert!(!all.contains("LIVE"), "暂停时不该还报 LIVE: {all}");
    }

    // -------------------------------------------------- DIALOG 模态(H-P1-04)

    fn yes_no_view() -> DialogView {
        DialogView {
            id: 7,
            caption: "確認".to_string(),
            text: "ゲームを終了します。よろしいですか？".to_string(),
            buttons: "yes_no",
            default_button: 0,
            icon: "question",
            input: false,
            input_limit: 0,
            default_input: String::new(),
        }
    }

    fn screen_raw() -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 960.0),
            )),
            ..Default::default()
        }
    }

    /// 找一段绘制文本的中心点(无头点击用)。
    fn text_center(shapes: &[egui::epaint::ClippedShape], want: &str) -> Option<egui::Pos2> {
        fn walk(shape: &egui::epaint::Shape, want: &str) -> Option<egui::Pos2> {
            match shape {
                egui::epaint::Shape::Text(text) => text
                    .galley
                    .text()
                    .contains(want)
                    .then(|| text.pos + egui::vec2(0.0, text.galley.size().y * 0.5)),
                egui::epaint::Shape::Vec(list) => list.iter().find_map(|item| walk(item, want)),
                _ => None,
            }
        }
        shapes.iter().find_map(|s| walk(&s.shape, want))
    }

    #[test]
    fn dialog_modal_renders_caption_and_button_labels() {
        let ctx = egui::Context::default();
        let mut inputs = std::collections::BTreeMap::new();
        let raw = screen_raw();
        let mut first = ctx.run_pass(raw.clone(), |ctx| {
            dialog_modal(ctx, &[yes_no_view()], &mut inputs);
        });
        first.textures_delta.clear();
        let mut out = ctx.run_pass(raw, |ctx| {
            dialog_modal(ctx, &[yes_no_view()], &mut inputs);
        });
        out.textures_delta.clear();
        let mut texts = Vec::new();
        for shape in &out.shapes {
            collect(&shape.shape, &mut texts, &mut Vec::new());
        }
        let joined = texts.join("\n");
        assert!(joined.contains("確認"), "标题可见: {joined}");
        assert!(joined.contains("ゲームを終了します"), "正文可见: {joined}");
        assert!(joined.contains("はい"), "YesNo 按钮日文标签: {joined}");
        assert!(joined.contains("いいえ"), "{joined}");
        assert!(inputs.is_empty(), "非 INPUT 型不建编辑缓冲");
    }

    #[test]
    fn dialog_modal_answers_clicked_button() {
        let ctx = egui::Context::default();
        let mut inputs = std::collections::BTreeMap::new();
        // 两遍定位「いいえ」(首遍只测量);窗口锚定 CENTER_CENTER,布局稳定。
        for _ in 0..2 {
            let mut out = ctx.run_pass(screen_raw(), |ctx| {
                dialog_modal(ctx, &[yes_no_view()], &mut inputs);
            });
            out.textures_delta.clear();
        }
        let pos = {
            let mut probe = ctx.run_pass(screen_raw(), |ctx| {
                dialog_modal(ctx, &[yes_no_view()], &mut inputs);
            });
            let pos = text_center(&probe.shapes, "いいえ").expect("いいえ 按钮应已绘制");
            probe.textures_delta.clear();
            pos
        };
        // 按下+抬起各一遍(egui click 语义)。
        let press = egui::RawInput {
            screen_rect: screen_raw().screen_rect,
            events: vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            }],
            ..Default::default()
        };
        let mut answers = Vec::new();
        let mut out = ctx.run_pass(press, |ctx| {
            answers = dialog_modal(ctx, &[yes_no_view()], &mut inputs);
        });
        out.textures_delta.clear();
        let release = egui::RawInput {
            screen_rect: screen_raw().screen_rect,
            events: vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
            ..Default::default()
        };
        let mut out = ctx.run_pass(release, |ctx| {
            answers = dialog_modal(ctx, &[yes_no_view()], &mut inputs);
        });
        out.textures_delta.clear();
        assert_eq!(
            answers,
            vec![DialogAnswer {
                id: 7,
                button: 2,
                input_text: None
            }],
            "点击「いいえ」= 按钮 2"
        );
    }

    #[test]
    fn dialog_modal_escape_answers_only_cancelable_groups() {
        // Esc:ok_cancel → cancel(2);yes_no → 无响应(官方 MB_YESNO 不响应 Esc)。
        let ok_cancel = DialogView {
            buttons: "ok_cancel",
            ..yes_no_view()
        };
        let ctx = egui::Context::default();
        let mut inputs = std::collections::BTreeMap::new();
        let esc = egui::RawInput {
            screen_rect: screen_raw().screen_rect,
            events: vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
            ..Default::default()
        };
        let mut answers = Vec::new();
        let mut out = ctx.run_pass(esc.clone(), |ctx| {
            answers = dialog_modal(ctx, std::slice::from_ref(&ok_cancel), &mut inputs);
        });
        out.textures_delta.clear();
        assert_eq!(
            answers,
            vec![DialogAnswer {
                id: 7,
                button: 2,
                input_text: None
            }],
            "ok_cancel 的 Esc = キャンセル(2)"
        );
        let ctx = egui::Context::default();
        let mut answers = Vec::new();
        let mut out = ctx.run_pass(esc, |ctx| {
            answers = dialog_modal(ctx, &[yes_no_view()], &mut inputs);
        });
        out.textures_delta.clear();
        assert!(answers.is_empty(), "yes_no 的 Esc 不产生回答(官方语义)");
    }

    #[test]
    fn dialog_modal_input_mode_buffers_text() {
        let view = DialogView {
            input: true,
            input_limit: 5,
            default_input: "はじめ".to_string(),
            ..yes_no_view()
        };
        let ctx = egui::Context::default();
        let mut inputs = std::collections::BTreeMap::new();
        let mut first = ctx.run_pass(screen_raw(), |ctx| {
            dialog_modal(ctx, &[view.clone()], &mut inputs);
        });
        first.textures_delta.clear();
        let mut out = ctx.run_pass(screen_raw(), |ctx| {
            dialog_modal(ctx, std::slice::from_ref(&view), &mut inputs);
        });
        out.textures_delta.clear();
        assert_eq!(
            inputs.get(&7).map(String::as_str),
            Some("はじめ"),
            "INPUT 型首次出现播种 DEFSTR"
        );
    }

    #[test]
    fn input_counters_top_is_sorted() {
        let mut counters = InputCounters::default();
        counters.press("MOUSE_L", 1);
        counters.press("KEY_ENTER", 2);
        counters.press("KEY_ENTER", 3);
        assert_eq!(counters.top(2)[0], ("KEY_ENTER".to_string(), 2));
    }
}
