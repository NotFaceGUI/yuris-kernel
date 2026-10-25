//! `yu-host-desktop` · 原生桌面宿主(Windows 先行)。
//!
//! # 定位
//!
//! 窗口 = winit 0.30;游戏帧仍由 `yu_engine::Engine`(wgpu 30 / soft)产出;
//! 本 crate 只做「窗口 + surface 上屏 + 输入转发 + egui 调试面板 + cpal
//! 音频输出」(`sound_device` 实现 `yu_engine::engine::AudioOut` 端口)。
//! 渲染语义、VM、音频状态机都不在这里。
//!
//! 入口在 `yu-cli --mode desktop`(2026-10-25 架构 v2:H3,独立
//! `yu-desktop` exe 已退役);本 crate 只剩 lib。
//!
//! # 上屏路径
//!
//! 当前版本复用 `Engine::tick_realtime()` 的 RGBA 读回帧,上传到一张
//! `Rgba8Unorm` 纹理后全屏 blit 到窗口 surface。GPU 设备与 engine 的
//! `WgpuRenderer` **共用同一个 instance/device/queue**(见 `Engine::wgpu_renderer`),
//! 因此不存在第二个 GPU 设备。
//!
//! 下一阶段会把最后的显示缩放 pass 直接写 surface(去掉 CPU readback),
//! 本 crate 的 `Presenter` 就是那个接入点。
//!
//! # 输入
//!
//! winit 事件 → `Engine::set_input` / `set_cursor`;键名与
//! `crates/yu-rt/data/button-code-abi.tsv` 一致(`KEY_ENTER` / `KEY_BS` / `MOUSE_L` …)。
//! egui 消费事件或需要键盘/指针时,不再转发给引擎。
//!
//! # 调试 UI
//!
//! F1 开关 egui 面板;`--debug-ui` 启动即开。面板只读 `Engine` 的公开报告,
//! 不改变引擎状态(除 Restart 按钮)。

#![allow(clippy::result_large_err)]

pub mod diagnostics;
pub mod panic_guard;
pub mod script;
pub mod sound_device;
pub mod ui;

pub use sound_device::SoundSink;

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use egui_wgpu::ScreenDescriptor;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorIcon, Fullscreen, Window, WindowId};

use yu_backend_soft::Surface;
use yu_engine::engine::Engine;
use yu_platform_input::{
    cursor_shape, cursor_visible, key_button, mouse_activity_reactive, wheel_button, CursorClock,
    CursorShape, MouseActivity, PointerButton,
};

use crate::diagnostics::DiagnosticsHost;
use crate::panic_guard::{CloseAction, Phase};
use crate::script::{ScriptAction, ScriptRunner};

// 宿主日志:与 `yu_log::console` 统一格式合流(Q1 复审,主人 2026-10-25
// 指出的「未对齐格式」即本处)。宏看不到 self,经进程级**注入槽**取
// `run()` 安装的 ConsoleHandle;未安装(极早期/嵌入/测试)退化为纯
// `[host] …` stderr 行——凡宿主行必带频道。A3 注:这是宿主运维层元数据
// 槽(同 `console::STARTED`/`build_info` 一类),内核语义不读取。
static HOST_CONSOLE: OnceLock<yu_log::console::ConsoleHandle> = OnceLock::new();

/// 宿主启动时安装统一控制台(`run()` 最先调用;重复安装是温和 no-op)。
pub fn install_host_console(handle: yu_log::console::ConsoleHandle) {
    let _ = HOST_CONSOLE.set(handle);
}

fn host_emit(level: yu_log::console::HostLevel, message: String) {
    match HOST_CONSOLE.get() {
        Some(console) => console.emit_host(level, "host", &message),
        None => yu_engine::output::error(format!("[host] {message}")),
    }
}

/// 生命周期/进度行(Info,绿)。
macro_rules! host_line {
    ($($arg:tt)*) => { host_emit(yu_log::console::HostLevel::Info, format!($($arg)*)) };
}
/// 注意/警告行(Warn,黄)。
macro_rules! host_error {
    ($($arg:tt)*) => { host_emit(yu_log::console::HostLevel::Warn, format!($($arg)*)) };
}
/// 致命/冻结行(Error,红):fail()、panic 冻结等不可继续场景。
macro_rules! host_fatal {
    ($($arg:tt)*) => { host_emit(yu_log::console::HostLevel::Error, format!($($arg)*)) };
}

/// Windows 定时器粒度(实测:P1 固定节拍已改为 `next_frame += 1/fps`,
/// 但 600 帧仍只有 ~41fps;`--fps 1000` 同样 ~42fps ⇒ 不是 work+睡眠
/// 串行,而是 `ControlFlow::WaitUntil` 的等待被系统 15.6ms 粒度放大)。
/// 游戏/图形宿主按惯例升到 1ms;离开作用域恢复进程原设置。
#[cfg(windows)]
mod timer_resolution {
    use super::TIMER_PERIOD_MS;

    #[link(name = "winmm")]
    unsafe extern "system" {
        fn timeBeginPeriod(period: u32) -> u32;
        fn timeEndPeriod(period: u32) -> u32;
    }

    pub struct Guard {
        active: bool,
    }

    pub fn raise() -> Guard {
        // SAFETY: winmm 的成对调用;period=1 是文档化取值。
        let active = unsafe { timeBeginPeriod(TIMER_PERIOD_MS) } == 0;
        Guard { active }
    }

    pub fn active(guard: &Guard) -> bool {
        guard.active
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            if self.active {
                // SAFETY: 与 raise() 的 timeBeginPeriod(TIMER_PERIOD_MS) 配对。
                unsafe {
                    timeEndPeriod(TIMER_PERIOD_MS);
                }
            }
        }
    }
}

#[cfg(not(windows))]
mod timer_resolution {
    pub struct Guard;

    pub fn raise() -> Guard {
        Guard
    }

    pub fn active(_guard: &Guard) -> bool {
        false
    }
}

/// Windows 定时器粒度(ms)。实测默认 15.6ms 把 60Hz 压到 ~41fps;
/// 见 `timer_resolution` 的注释与 `docs/perf-report.md`。
#[cfg_attr(not(windows), allow(dead_code))]
const TIMER_PERIOD_MS: u32 = 1;
/// `WaitUntil` 的提前唤醒量(微秒)。实测 Windows 唤醒比截止时间晚
/// ~0.3ms;提前 0.8ms 让工作落在截止时间上(58.8→59.1fps)。
const WAIT_LEAD_US: u64 = 800;
/// 逐帧统计保留上限(约 55 分钟@60fps)。
const FRAME_STATS_CAP: usize = 200_000;
/// 未结算输入事件上限(异常情况下也不无界增长)。
const PENDING_INPUT_CAP: usize = 4096;
/// 输入延迟样本上限(超出丢弃前一半)。
const INPUT_LATENCY_CAP: usize = 8192;

/// 一帧节奏样本(`--frame-stats` / 退出摘要)。
struct FrameRow {
    host_frame: u64,
    engine_frame: u64,
    interval_ms: f64,
    work_ms: f64,
    tick_ms: f64,
    render_ms: f64,
    draw_ms: f64,
}

/// 宿主参数(由 `yu-desktop` CLI 填充)。
#[derive(Clone, Debug)]
pub struct HostOptions {
    /// 窗口标题。
    pub title: String,
    /// 初始窗口尺寸;`None` = 引擎视口。
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// 逻辑帧率(用于调度;引擎自己的 fps 也要一致)。
    pub fps: u32,
    /// 跑满 N 帧后自动退出;`None` = 不退出。
    pub frames: Option<u64>,
    /// 退出时把最后一帧写成 PNG。
    pub shot: Option<PathBuf>,
    pub fullscreen: bool,
    /// 最近邻放大(像素风)。
    pub pixel: bool,
    /// 启动即显示 egui 调试面板。
    pub debug_ui: bool,
    /// 复现脚本:按逻辑帧注入输入/窗口操作(见 `script` 模块)。
    pub script: Option<PathBuf>,
    /// 自检:在指定宿主帧**故意 panic**,验证 panic 防线(不进产品路径)。
    pub panic_selftest: Option<u64>,
    /// 逐帧节奏采样 CSV(宿主帧/间隔/工作/引擎 tick/render/draw)。
    pub frame_stats: Option<PathBuf>,
    /// 在这些宿主帧打印 VM 逐命令耗时 top-10(`take_command_profile`)。
    pub profile_frames: BTreeSet<u64>,
    /// 每任务每次泵的命令上限(脚本突发兜底;`None` = 官方同构无限泵)。
    /// 开启会让长脚本跨帧续跑(DIV-HOST-002),默认关闭。
    pub pump_cap: Option<usize>,
    /// 统一控制台(Q1 R2):宿主 observe 的诊断同步出控制台行(与 egui
    /// 面板同源、两路消费);`None` = 保持旧只进面板行为(测试/嵌入)。
    pub console: Option<yu_log::console::ConsoleHandle>,
}

impl Default for HostOptions {
    fn default() -> Self {
        Self {
            title: "YU-RIS".to_string(),
            width: None,
            height: None,
            fps: 60,
            frames: None,
            shot: None,
            fullscreen: false,
            pixel: false,
            debug_ui: false,
            script: None,
            panic_selftest: None,
            frame_stats: None,
            profile_frames: BTreeSet::new(),
            pump_cap: None,
            console: None,
        }
    }
}

/// 启动桌面窗口。返回时窗口已关闭;`Err` 表示启动/运行失败。
///
/// panic 防线在 `run()` 里安装(进程级 hook),早于任何窗口/GPU 调用:
/// 这样 wgpu/winit 初始化期的 panic 也走同一条日志路径。
pub fn run(engine: Engine, opts: HostOptions) -> Result<(), String> {
    // 统一控制台先行:此后一切 host 行(宏)都走统一格式+JSONL。
    if let Some(console) = &opts.console {
        install_host_console(console.clone());
    }
    panic_guard::install();
    // 定时器粒度必须在事件循环启动前抬高(见 `timer_resolution`)。
    let timer = timer_resolution::raise();
    host_line!(
        "yu-desktop: timer resolution 1ms {}",
        if timer_resolution::active(&timer) {
            "enabled"
        } else {
            "unavailable (system default)"
        }
    );
    let mut app = App::new(engine, opts)?;
    let event_loop = EventLoop::new().map_err(|e| format!("创建事件循环失败: {e}"))?;
    event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now()));
    event_loop
        .run_app(&mut app)
        .map_err(|e| format!("事件循环失败: {e}"))?;
    app.finish();
    match app.fatal {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

// ---------------------------------------------------------------- presenter

struct Presenter {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    format: wgpu::TextureFormat,
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
    size: (u32, u32),
    adapter: String,
}

impl Presenter {
    fn new(
        window: Arc<Window>,
        pixel: bool,
        gpu: &yu_backend_wgpu::GpuContext,
    ) -> Result<Self, String> {
        let size = window.inner_size();
        let width = size.width.max(1);
        let height = size.height.max(1);

        // 复用 engine 的 GPU 上下文:窗口纹理和引擎纹理必须同一个 device。
        let surface = gpu
            .instance
            .create_surface(window)
            .map_err(|e| format!("创建窗口 surface 失败: {e}"))?;
        let caps = surface.get_capabilities(&gpu.adapter);
        if caps.formats.is_empty() {
            return Err(format!(
                "引擎适配器 {} 不支持该窗口 surface",
                gpu.adapter_info.name
            ));
        }
        let adapter_name = gpu.adapter_info.name.clone();
        let device = gpu.device.clone();
        let queue = gpu.queue.clone();
        // 优先非 sRGB:引擎按存储值混合,不能隐式线性化。
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| {
                matches!(
                    f,
                    wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm
                )
            })
            .unwrap_or(caps.formats[0]);
        if format.is_srgb() {
            host_error!(
                "yu-desktop: 警告: surface 只有 sRGB 格式({format:?}),颜色会经过一次 sRGB 编码"
            );
        }
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width,
            height,
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("yu-host-desktop present"),
            source: wgpu::ShaderSource::Wgsl(include_str!("present.wgsl").into()),
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("yu-host-desktop present bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("yu-host-desktop present layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("yu-host-desktop present pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("yu-host-desktop present sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: if pixel {
                wgpu::FilterMode::Nearest
            } else {
                wgpu::FilterMode::Linear
            },
            min_filter: if pixel {
                wgpu::FilterMode::Nearest
            } else {
                wgpu::FilterMode::Linear
            },
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let (texture, view, bind_group) =
            create_target(&device, &bind_group_layout, &sampler, 1, 1, [0, 0, 0, 255]);

        Ok(Self {
            surface,
            device,
            queue,
            config,
            format,
            pipeline,
            bind_group_layout,
            sampler,
            texture,
            view,
            bind_group,
            size: (1, 1),
            adapter: adapter_name,
        })
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        if self.config.width == width && self.config.height == height {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    fn upload(&mut self, surface: &Surface) {
        if surface.width == 0 || surface.height == 0 {
            return;
        }
        if self.size != (surface.width, surface.height) {
            let (texture, view, bind_group) = create_target(
                &self.device,
                &self.bind_group_layout,
                &self.sampler,
                surface.width,
                surface.height,
                [0, 0, 0, 255],
            );
            self.texture = texture;
            self.view = view;
            self.bind_group = bind_group;
            self.size = (surface.width, surface.height);
        }
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &surface.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(surface.width * 4),
                rows_per_image: Some(surface.height),
            },
            wgpu::Extent3d {
                width: surface.width,
                height: surface.height,
                depth_or_array_layers: 1,
            },
        );
    }

    fn acquire(&mut self) -> Result<wgpu::SurfaceTexture, String> {
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => Ok(frame),
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                match self.surface.get_current_texture() {
                    wgpu::CurrentSurfaceTexture::Success(frame)
                    | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => Ok(frame),
                    other => Err(format!("获取 surface 纹理失败: {other:?}")),
                }
            }
            other => Err(format!("获取 surface 纹理失败: {other:?}")),
        }
    }
}

fn create_target(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    width: u32,
    height: u32,
    clear: [u8; 4],
) -> (wgpu::Texture, wgpu::TextureView, wgpu::BindGroup) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("yu-host-desktop game frame"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("yu-host-desktop present bind group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });
    let _ = clear;
    (texture, view, bind_group)
}

// ---------------------------------------------------------------- app

/// 窗口 → 内容矩形等比的**唯一**映射(渲染与输入共用)。
///
/// `scale` 是窗口像素/逻辑像素;`(ox, oy)` 是黑边偏移。渲染的
/// `set_viewport` 与输入的 `logical_from_client` 都从这里取参数,保证
/// 两者严格互逆 —— 之前两处各写一份公式,`round()` 与浮点路径不同,
/// hover 会与画面对不上。
struct FitRect {
    scale: f64,
    ox: f64,
    oy: f64,
    width: f64,
    height: f64,
}

fn fit_rect(win_w: u32, win_h: u32, game_w: u32, game_h: u32) -> FitRect {
    if win_w == 0 || win_h == 0 || game_w == 0 || game_h == 0 {
        return FitRect {
            scale: 1.0,
            ox: 0.0,
            oy: 0.0,
            width: f64::from(win_w),
            height: f64::from(win_h),
        };
    }
    let scale = (f64::from(win_w) / f64::from(game_w)).min(f64::from(win_h) / f64::from(game_h));
    let width = f64::from(game_w) * scale;
    let height = f64::from(game_h) * scale;
    FitRect {
        scale,
        ox: (f64::from(win_w) - width) * 0.5,
        oy: (f64::from(win_h) - height) * 0.5,
        width,
        height,
    }
}

/// 计算游戏画面在窗口内的内容矩形(等比缩放 + 居中黑边)。
fn content_rect(win_w: u32, win_h: u32, game_w: u32, game_h: u32) -> (f32, f32, f32, f32) {
    let fit = fit_rect(win_w, win_h, game_w, game_h);
    (
        fit.ox as f32,
        fit.oy as f32,
        fit.width.max(1.0) as f32,
        fit.height.max(1.0) as f32,
    )
}

/// 调试面板字体。
///
/// egui 0.37 的 `default_fonts` 不是默认特性(之前连 ASCII 都是方块);
/// 这里再叠一份系统 CJK 字体,让日文/中文标题也能显示。
fn install_debug_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    for path in [
        "C:/Windows/Fonts/Deng.ttf",
        "C:/Windows/Fonts/simhei.ttf",
        "C:/Windows/Fonts/NotoSansSC-VF.ttf",
        "C:/Windows/Fonts/msgothic.ttc",
    ] {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let name = "cjk".to_string();
        fonts
            .font_data
            .insert(name.clone(), egui::FontData::from_owned(bytes).into());
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts
                .families
                .entry(family)
                .or_default()
                .insert(0, name.clone());
        }
        host_error!("yu-desktop: debug UI font: {path}");
        break;
    }
    ctx.set_fonts(fonts);
}

struct App {
    engine: Engine,
    opts: HostOptions,
    viewport: (u32, u32),
    window: Option<Arc<Window>>,
    presenter: Option<Presenter>,
    egui_ctx: egui::Context,
    egui_state: Option<egui_winit::State>,
    egui_renderer: Option<egui_wgpu::Renderer>,
    last_surface: Option<Arc<Surface>>,
    next_frame: Instant,
    started: Instant,
    frames: u64,
    show_debug: bool,
    stopping: bool,
    fatal: Option<String>,
    cursor: (i64, i64),
    held: std::collections::BTreeSet<String>,
    /// 已收到几次关闭请求(第二次强制退出,避免脚本不处理时窗口关不掉)。
    close_requests: u32,
    cursor_visible: Option<bool>,
    cursor_icon: Option<CursorIcon>,
    /// 最近一次“会唤醒光标”的鼠标输入(毫秒,相对 `started`)。
    last_mouse_activity_ms: u64,
    /// 最近一次键盘按下(毫秒,相对 `started`)。
    last_keyboard_activity_ms: u64,
    /// 诊断账本(挂号表 / skipped / 输入计数 / 卡住判定)。
    diagnostics: DiagnosticsHost,
    /// 统一控制台(Q1;重启重建账本后要重新注入,所以 App 也持有一份)。
    console: Option<yu_log::console::ConsoleHandle>,
    /// 被防线接住的 panic;`Some` = 引擎冻结,只出横幅。
    freeze: Option<panic_guard::Freeze>,
    /// 场景脚本(复现用;`None` = 正常交互)。
    script: Option<ScriptRunner>,
    /// 卡住横幅开关(面板可关)。
    show_attention: bool,
    /// 脚本给出的帧数上限(`--frames` 缺省时使用)。
    script_frames: Option<u32>,
    /// 上一宿主帧开始时刻(start-to-start 间隔的锚点)。
    last_frame_start: Instant,
    /// 逐帧节奏样本(退出时汇总/写 CSV;`--frame-stats`)。
    frame_rows: Vec<FrameRow>,
    /// 已发生但尚未被 tick 消费的真实输入事件(时间戳、类别、事件时逻辑帧)。
    pending_inputs: Vec<(Instant, &'static str, u64)>,
    /// “输入时间戳 → 含该输入的帧完成绘制”的延迟样本
    /// `(毫秒, 生效跨越的逻辑帧数, 类别)`。
    input_latencies: Vec<(f64, u64, &'static str)>,
    /// 需要打印 VM 命令耗时的宿主帧(`--profile-frames`)。
    profile_frames: BTreeSet<u64>,
    /// INPUT 型 DIALOG 的编辑缓冲(对话框存续期跨帧;回答后移除)。
    dialog_inputs: std::collections::BTreeMap<u64, String>,
    /// 已打过「呈现」日志的 DIALOG id(每个 id 只记一次)。
    presented_dialogs: std::collections::BTreeSet<u64>,
    /// 渲染耗时曲线样本(`--debug-ui` 的独立窗口;每宿主帧一个)。
    render_samples: std::collections::VecDeque<f32>,
}

impl App {
    fn new(mut engine: Engine, mut opts: HostOptions) -> Result<Self, String> {
        let egui_ctx = egui::Context::default();
        install_debug_fonts(&egui_ctx);
        // 桌面宿主按固定两帧的步进推进逻辑:长帧后不再一次性跑 500ms
        // 的脚本批次(那是 title 切换时 2.5s 卡顿的来源)。
        let frame_ms = (1000 / opts.fps.max(1)).max(1);
        engine.set_max_step_ms(frame_ms * 2);
        if let Some(cap) = opts.pump_cap {
            engine.set_pump_cap(Some(cap.max(1)));
            host_error!(
                "yu-desktop: pump cap {cap} commands/task/frame — EXPERIMENTAL \
                 (DIV-HOST-002: can break frame-atomic scripts; settings corpus \
                 hits rt.label Fatal)"
            );
        }
        let viewport = engine.viewport();
        let show_debug = opts.debug_ui;
        let script = match opts.script.as_deref() {
            Some(path) => Some(ScriptRunner::from_file(path)?),
            None => None,
        };
        let script_frames = script.as_ref().and_then(|runner| runner.frames);
        let profile_frames = opts.profile_frames.clone();
        if !profile_frames.is_empty() {
            engine.set_command_profile(true);
        }
        if let Some(runner) = &script {
            host_line!(
                "yu-desktop: script `{}` loaded ({} actions, frames={:?})",
                runner.name,
                runner.remaining(),
                runner.frames
            );
        }
        // 统一控制台(Q1 R2):账本与 App 各持一份句柄(重启重建账本后重注)。
        let console = opts.console.take();
        let mut diagnostics = DiagnosticsHost::default();
        diagnostics.set_console(console.clone());
        Ok(Self {
            engine,
            opts,
            viewport: (viewport.width, viewport.height),
            window: None,
            presenter: None,            egui_ctx,
            egui_state: None,
            egui_renderer: None,
            last_surface: None,
            next_frame: Instant::now(),
            started: Instant::now(),
            frames: 0,
            show_debug,
            stopping: false,
            fatal: None,
            cursor: (0, 0),
            held: Default::default(),
            close_requests: 0,
            cursor_visible: None,
            cursor_icon: None,
            last_mouse_activity_ms: 0,
            last_keyboard_activity_ms: 0,
            diagnostics,
            console,
            freeze: None,
            script,
            show_attention: true,
            script_frames,
            last_frame_start: Instant::now(),
            frame_rows: Vec::new(),
            pending_inputs: Vec::new(),
            input_latencies: Vec::new(),
            profile_frames,
            dialog_inputs: Default::default(),
            presented_dialogs: Default::default(),
            render_samples: std::collections::VecDeque::with_capacity(ui::RENDER_GRAPH_SAMPLES),
        })
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, message: String) {
        host_fatal!("yu-desktop: {message}");
        self.fatal = Some(message);
        self.stopping = true;
        event_loop.exit();
    }

    fn window_size(&self) -> (u32, u32) {
        self.window
            .as_ref()
            .map(|w| {
                let size = w.inner_size();
                (size.width.max(1), size.height.max(1))
            })
            .unwrap_or(self.viewport)
    }

    fn sync_cursor(&mut self) {
        self.engine.set_cursor(self.cursor.0, self.cursor.1);
    }

    fn set_button(&mut self, name: &str, pressed: bool) {
        if pressed {
            self.held.insert(name.to_string());
        } else {
            self.held.remove(name);
        }
        self.diagnostics
            .note_input(name, pressed, self.engine.frame());
        self.engine.set_input(name, pressed);
    }

    fn pulse_button(&mut self, name: &str) {
        self.diagnostics.note_wheel(self.engine.frame());
        self.engine.pulse_input(name);
    }

    fn release_all(&mut self) {
        self.engine.release_all_inputs();
        self.held.clear();
    }

    /// 冻结:记录现场、写日志、保留最后一帧。
    ///
    /// 已经在冻结状态时只累加计数(例如冻结期间 draw 又崩),不覆盖首条
    /// 现场 —— 第一条 panic 才是根因。
    fn enter_freeze(&mut self, record: panic_guard::PanicRecord) {
        if self.freeze.is_some() {
            if let Some(freeze) = self.freeze.as_mut() {
                freeze.draw_panics = freeze.draw_panics.saturating_add(1);
            }
            log::error!(
                target: "yu-desktop",
                "second panic while frozen: phase={} msg={} at {}",
                record.phase.as_str(),
                record.message,
                record.location
            );
            return;
        }
        host_fatal!(
            "yu-desktop: PANIC 捕获 phase={} frame={} engine_frame={} msg={} at {}",
            record.phase.as_str(),
            record.host_frame,
            record.engine_frame,
            record.message,
            record.location
        );
        if !record.backtrace.is_empty() {
            for line in record.backtrace.lines().take(8) {
                log::error!(target: "yu-desktop", "backtrace {}", line);
            }
        }
        self.freeze = Some(panic_guard::Freeze::new(record));
        // 冻结期间引擎不再消费输入;挂起的延迟样本作废,避免解冻后
        // 出现几十秒的假延迟。
        self.pending_inputs.clear();
    }

    /// 应用到期的场景脚本动作(逻辑帧对齐)。
    fn apply_script(&mut self, event_loop: &ActiveEventLoop) {
        let engine_frame = self.engine.frame();
        let due = match self.script.as_mut() {
            Some(runner) => runner.due(engine_frame),
            None => return,
        };
        for item in due {
            match item.action {
                ScriptAction::Press { button, x, y } => {
                    log::info!(
                        target: "yu-desktop",
                        "script frame={} press {button} at ({x},{y})",
                        item.frame
                    );
                    self.cursor = (x, y);
                    self.engine.set_cursor(x, y);
                    self.note_mouse_activity(MouseActivity::Click);
                    self.set_button(&button, true);
                    self.note_input_event("script");
                }
                ScriptAction::Release { button } => {
                    log::info!(
                        target: "yu-desktop",
                        "script frame={} release {button}",
                        item.frame
                    );
                    self.set_button(&button, false);
                    self.note_input_event("script");
                }
                ScriptAction::Pulse { button } => {
                    log::info!(
                        target: "yu-desktop",
                        "script frame={} pulse {button}",
                        item.frame
                    );
                    self.note_mouse_activity(MouseActivity::Wheel);
                    self.pulse_button(&button);
                    self.note_input_event("script");
                }
                ScriptAction::Resize { width, height } => {
                    host_line!(
                        "yu-desktop: script frame={} resize {width}x{height}",
                        item.frame
                    );
                    if let Some(window) = &self.window {
                        let _ = window.request_inner_size(LogicalSize::new(
                            f64::from(width),
                            f64::from(height),
                        ));
                    }
                }
                ScriptAction::Minimize => {
                    host_line!("yu-desktop: script frame={} minimize", item.frame);
                    if let Some(window) = &self.window {
                        window.set_minimized(true);
                    }
                }
                ScriptAction::Restore => {
                    host_line!("yu-desktop: script frame={} restore", item.frame);
                    if let Some(window) = &self.window {
                        window.set_minimized(false);
                        window.focus_window();
                    }
                }
                ScriptAction::FocusLost => {
                    // 宿主路径注入:winit 的 `Focused(false)` 也走这里。
                    host_line!(
                        "yu-desktop: script frame={} focus_lost (release_all)",
                        item.frame
                    );
                    self.release_all();
                }
                ScriptAction::Close => {
                    self.close_requests += 1;
                    let decision = panic_guard::close_decision(
                        self.engine.main_finished(),
                        self.close_requests,
                    );
                    match decision {
                        CloseAction::Exit => {
                            host_line!(
                                "yu-desktop: script frame={} close #{} → exit",
                                item.frame,
                                self.close_requests
                            );
                            self.stopping = true;
                            event_loop.exit();
                        }
                        CloseAction::SignalEngine => {
                            host_line!(
                                "yu-desktop: script frame={} close #{} → @_WINDOW_EXEC_CLOSE=1",
                                item.frame,
                                self.close_requests
                            );
                            self.engine.request_window_close();
                        }
                    }
                }
            }
        }
    }

    /// 窗口客户区像素 → 逻辑坐标(`fit_rect` 的逆映射)。
    ///
    /// **不 clamp**:引擎里 `@_MOUSE_X/Y` 在黑边上就是负值/越界值,脚本
    /// 自己用 `@_MOUSE_X>=0 && <GSX2` 判断“是否在画布内”
    /// (ERIS `es.BT.MOUSECHECK` / `es_Text`)。夹到边缘会把黑边上的 hover
    /// 误判成画面边缘。
    fn logical_from_client(&self, x: f64, y: f64) -> (i64, i64) {
        let (w, h) = self.window_size();
        let (vw, vh) = self.viewport;
        let fit = fit_rect(w, h, vw, vh);
        (
            ((x - fit.ox) / fit.scale) as i64,
            ((y - fit.oy) / fit.scale) as i64,
        )
    }

    fn update_cursor_from_position(&mut self, x: f64, y: f64) {
        let next = self.logical_from_client(x, y);
        if next != self.cursor {
            self.cursor = next;
            self.sync_cursor();
        }
    }

    fn now_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    /// 记录“会唤醒光标”的鼠标输入(`MOUSE[MOVE/CLICK/WHEEL]` 是门控)。
    fn note_mouse_activity(&mut self, activity: MouseActivity) {
        let Some(settings) = self.engine.input_settings() else {
            self.last_mouse_activity_ms = self.now_ms();
            return;
        };
        if mouse_activity_reactive(settings, activity) {
            self.last_mouse_activity_ms = self.now_ms();
        }
    }

    /// 把脚本 `MOUSE[MODE/TIME/CURSOR]` 映射到窗口光标。
    ///
    /// 可见性策略在 `yu-platform-input`(与平台无关),这里只做 winit 适配。
    fn update_cursor_presentation(&mut self) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let Some(settings) = self.engine.input_settings() else {
            return;
        };
        let visible = cursor_visible(
            settings,
            CursorClock {
                now_ms: self.now_ms(),
                last_keyboard_ms: self.last_keyboard_activity_ms,
                last_mouse_ms: self.last_mouse_activity_ms,
            },
        );
        if self.cursor_visible != Some(visible) {
            window.set_cursor_visible(visible);
            self.cursor_visible = Some(visible);
        }
        let icon = winit_cursor_icon(cursor_shape(settings.cursor));
        if self.cursor_icon != Some(icon) {
            window.set_cursor(icon);
            self.cursor_icon = Some(icon);
        }
    }

    fn draw(&mut self, event_loop: &ActiveEventLoop) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let (width, height) = self.window_size();
        if width == 0 || height == 0 {
            return;
        }
        // 最小化时窗口是 1x1/不可见:继续出 egui 帧会撞 surface 尺寸与
        // scissor 不一致的 wgpu 校验错误(S6 的原始崩溃)。
        if window.is_minimized().unwrap_or(false) {
            return;
        }
        // 还原后 `Resized` 可能还没进事件队列(脚本在同一帧 restore):
        // 画之前把 surface 配置同步到当前窗口尺寸,acquire 到的纹理才与
        // egui 的 scissor 一致。
        if let Some(presenter) = self.presenter.as_mut() {
            presenter.resize(width, height);
        }
        let (device, queue) = match self.presenter.as_ref() {
            Some(presenter) => (presenter.device.clone(), presenter.queue.clone()),
            None => return,
        };

        let frozen = self
            .freeze
            .as_ref()
            .map(|freeze| (freeze.title(), freeze.body()));
        let attention = self.attention_hint();
        // DIALOG 模态(S5/H-P1-04):待回答对话框存在时 egui 必须每帧跑,
        // 呈现窗口并接受点击;官方 MessageBox 是同步模态,挂起的任务在
        // 内核侧等 `answer_dialog`。
        let dialog_views = self.dialog_views();
        let has_dialogs = !dialog_views.is_empty();
        if has_dialogs {
            let ids: Vec<u64> = dialog_views.iter().map(|view| view.id).collect();
            let mut newly_presented = false;
            for id in &ids {
                if self.presented_dialogs.insert(*id) {
                    newly_presented = true;
                    if let Some(view) = dialog_views.iter().find(|view| view.id == *id) {
                        host_line!(
                            "yu-desktop: DIALOG#{} 呈现 buttons={} icon={} caption={:?} text={:?}",
                            view.id,
                            view.buttons,
                            view.icon,
                            view.caption,
                            view.text
                        );
                    }
                }
            }
            if newly_presented {
                // 官方模态会吞掉打开对话框的那次点击;不释放会把按下沿
                // 带进模态后面的场景。
                self.release_all();
            }
        }
        let show_debug = self.show_debug || self.freeze.is_some();
        // egui 惰性:面板隐藏、无冻结、无卡住横幅、无对话框时,整条 egui
        // 管线 (run_pass / tessellate / update_buffers / egui render pass)都不跑,
        // 只做游戏帧 blit。输入事件仍要排空:egui_winit 的缓冲在隐藏期间
        // 若不取会无界增长,重新显示时还会一次回放陈旧的点击。
        let wants_egui = has_dialogs || frozen.is_some() || show_debug || attention.is_some();
        let mut actions = ui::UiActions::default();
        let mut banner_quit = false;
        let mut paint_jobs = Vec::new();
        let mut sets = Vec::new();
        let mut free_ids = Vec::new();
        let mut screen = ScreenDescriptor {
            size_in_pixels: [width, height],
            pixels_per_point: 1.0,
        };

        if wants_egui {
            let raw_input = match self.egui_state.as_mut() {
                Some(state) => state.take_egui_input(&window),
                None => egui::RawInput::default(),
            };
            let ctx = self.egui_ctx.clone();
            let mut full_output = ctx.run_pass(raw_input, |ctx| {
                if let Some((title, body)) = &frozen {
                    banner_quit = ui::panic_banner(ctx, title, body);
                } else if let Some(hint) = &attention {
                    ui::attention_banner(ctx, hint);
                }
                if has_dialogs {
                    for answer in ui::dialog_modal(ctx, &dialog_views, &mut self.dialog_inputs) {
                        self.apply_dialog_answer(answer);
                    }
                }
                if show_debug {
                    let panel = self.build_ui(ctx);
                    actions.restart |= panel.restart;
                    actions.hide |= panel.hide;
                    actions.quit |= panel.quit;
                    if panel.show_attention.is_some() {
                        actions.show_attention = panel.show_attention;
                    }
                }
            });
            if actions.hide {
                self.show_debug = false;
            }
            if let Some(show) = actions.show_attention {
                self.show_attention = show;
            }
            if actions.restart {
                match self.engine.restart() {
                    Ok(()) => {
                        // 重启 = 新运行代际:清空账本与冻结(现场已写日志)。
                        self.diagnostics = DiagnosticsHost::default();
                        self.diagnostics.set_console(self.console.clone());
                        self.render_samples.clear();
                        if let Some(freeze) = self.freeze.take() {
                            host_line!(
                                "yu-desktop: restart 已恢复冻结运行(原 panic: {})",
                                freeze.record.message
                            );
                        }
                        self.release_all();
                        self.pending_inputs.clear();
                        self.last_surface = None;
                    }
                    Err(error) => host_error!("yu-desktop: restart 失败: {error}"),
                }
            }
            if let Some(state) = self.egui_state.as_mut() {
                state.handle_platform_output(&window, full_output.platform_output);
            }
            paint_jobs = ctx.tessellate(full_output.shapes, full_output.pixels_per_point);
            // egui 0.37 的 TexturesDelta 有 Drop 守卫:消费后必须清空。
            let mut textures_delta = std::mem::take(&mut full_output.textures_delta);
            sets = textures_delta.set.drain().collect();
            free_ids = textures_delta.free.drain().collect();
            textures_delta.clear();
            screen = ScreenDescriptor {
                size_in_pixels: [width, height],
                pixels_per_point: full_output.pixels_per_point,
            };
        } else if let Some(state) = self.egui_state.as_mut() {
            // 只排空、不构造帧(见上)。
            let _ = state.take_egui_input(&window);
        }

        // 先上传游戏帧并拿到 swapchain 纹理;borrow 收敛在块内,
        // 之后的 render pass / egui 只借用互不重叠的字段。
        let game = self.last_surface.take();
        let acquired = {
            let Some(presenter) = self.presenter.as_mut() else {
                self.last_surface = game;
                return;
            };
            if let Some(game) = &game {
                presenter.upload(game);
            }
            presenter.acquire()
        };
        let frame = match acquired {
            Ok(frame) => frame,
            Err(error) => {
                self.last_surface = game;
                self.fail(event_loop, error);
                return;
            }
        };

        let Some(presenter) = self.presenter.as_mut() else {
            self.last_surface = game;
            return;
        };
        let Some(egui_renderer) = self.egui_renderer.as_mut() else {
            self.last_surface = game;
            return;
        };
        for (id, deltas) in &sets {
            for delta in deltas {
                egui_renderer.update_texture(&device, &queue, *id, delta);
            }
        }

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("yu-host-desktop frame"),
        });
        let command_buffers = if wants_egui {
            egui_renderer.update_buffers(&device, &queue, &mut encoder, &paint_jobs, &screen)
        } else {
            Vec::new()
        };

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let (game_w, game_h) = game
            .as_ref()
            .map(|game| (game.width, game.height))
            .unwrap_or((self.viewport.0, self.viewport.1));
        let (vx, vy, vw, vh) = content_rect(width, height, game_w, game_h);
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("yu-host-desktop game present"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            // 保持源画面比例:整窗 clear 成黑,只在内容矩形里画游戏帧。
            pass.set_viewport(vx, vy, vw, vh, 0.0, 1.0);
            pass.set_pipeline(&presenter.pipeline);
            pass.set_bind_group(0, &presenter.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        if wants_egui && !paint_jobs.is_empty() {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("yu-host-desktop egui"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            egui_renderer.render(&mut pass, &paint_jobs, &screen);
        }
        let mut submit = command_buffers;
        submit.push(encoder.finish());
        presenter.queue.submit(submit);
        presenter.queue.present(frame);
        self.last_surface = game;

        for id in &free_ids {
            egui_renderer.free_texture(id);
        }
        if let Some(freeze) = self.freeze.as_mut() {
            freeze.drawn_frames = freeze.drawn_frames.saturating_add(1);
        }
        if banner_quit || actions.quit {
            host_line!("yu-desktop: quit requested from UI");
            self.stopping = true;
            event_loop.exit();
        }
    }

    /// 构造 F1 面板快照并交给 `ui::debug_window`。
    ///
    /// 所有 `Engine` 读数在这里复制成值;`Diagnostics` 以引用传入(它每帧
    /// 已在 `diagnostics.observe` 里聚合好,不在这里复制大表)。
    fn build_ui(&mut self, ctx: &egui::Context) -> ui::UiActions {
        let report = self.engine.last_report().clone();
        let viewport = self.engine.viewport();
        let (win_w, win_h) = self.window_size();
        let held = if self.held.is_empty() {
            "-".to_string()
        } else {
            self.held.iter().cloned().collect::<Vec<_>>().join(", ")
        };
        let gpu = self
            .presenter
            .as_ref()
            .map(|p| p.adapter.clone())
            .unwrap_or_else(|| "-".to_string());
        let (mouse_mode, mouse_time_ms) = self
            .engine
            .input_settings()
            .map(|settings| {
                (
                    Some(settings.mouse_mode as i64),
                    Some(settings.mouse_time_ms),
                )
            })
            .unwrap_or((None, None));
        let cursor_visible = self.cursor_visible;
        let diagnostics = self.diagnostics.snapshot();
        let snapshot = ui::UiSnapshot {
            backend: self.engine.backend().to_string(),
            frame: report.frame,
            viewport: (viewport.width, viewport.height),
            window: (win_w, win_h),
            tick_ms: report.tick_ms,
            render_ms: report.render_ms,
            tick_avg: self.engine.tick_avg_ms(),
            render_avg: self.engine.render_avg_ms(),
            ops: report.ops,
            drawn: report.executed,
            skipped: report.skipped,
            passes: report.passes,
            layers: report.layers,
            visible: report.visible,
            drawable: report.drawable,
            textures_cached: self.engine.textures_cached(),
            textures_decoded: self.engine.textures_decoded(),
            events_drained: self.engine.events_drained(),
            events_dropped: self.engine.events_dropped(),
            sound_layers: self.engine.sound_layers(),
            cursor: self.cursor,
            held,
            gpu,
            main_finished: self.engine.main_finished(),
            diagnostics,
            mouse_mode,
            mouse_time_ms,
            cursor_visible,
            show_attention: self.show_attention,
            log_lines: self
                .console
                .as_ref()
                .map(|c| c.recent_snapshot())
                .unwrap_or_default(),
        };
        let actions = ui::debug_window(ctx, &snapshot);
        ui::render_graph_window(ctx, &self.render_samples);
        if actions.clear_log {
            if let Some(console) = &self.console {
                console.clear_recent();
            }
        }
        actions
    }

    /// 卡住横幅的触发条件(不是 panic;引擎仍在跑,只是不推进)。
    fn attention_hint(&self) -> Option<String> {
        if !self.show_attention || self.freeze.is_some() {
            return None;
        }
        // DIALOG 模态本身就在解释「为什么不动」;琥珀横幅会盖在模态上,
        // 等待回答期间不报卡住。
        if self.dialog_pending() {
            return None;
        }
        let snapshot = self.diagnostics.snapshot();
        let stuck = &snapshot.stuck;
        // 静态标题/静态场景是正常的“等输入”,只在**刚点过**且画面仍不变时
        // 才把它当成“点了没反应”;否则琥珀横幅会一直盖在标题上。
        let clicked_recently = snapshot
            .input
            .last_press_frame
            .is_some_and(|frame| self.engine.frame().saturating_sub(frame) < 300);
        let notable = stuck.last_fatal.is_some()
            || stuck.logic_stall_frames >= 60
            || (!stuck.drawable && stuck.logic_stall_frames > 5)
            || (stuck.pixel_stall_frames >= 120 && clicked_recently);
        if notable {
            Some(stuck.hint())
        } else {
            None
        }
    }

    /// 是否有待回答的 DIALOG(模态期:egui 呈现 + 抑制游戏输入)。
    fn dialog_pending(&self) -> bool {
        !self.engine.pending_dialogs().is_empty()
    }

    /// 把内核的 CP932 对话框请求转成 egui 呈现视图(H-P1-04)。
    fn dialog_views(&self) -> Vec<ui::DialogView> {
        self.engine
            .pending_dialogs()
            .iter()
            .map(|request| ui::DialogView {
                id: request.id,
                caption: request.caption_display(),
                text: request.text_display(),
                buttons: request.buttons.as_str(),
                default_button: request.default_button,
                icon: request.icon.as_str(),
                input: request.input,
                input_limit: request.input_limit,
                default_input: request.default_input_display(),
            })
            .collect()
    }

    /// 把用户的回答交回内核(`LET/LETSTR` 写回由内核完成)。
    fn apply_dialog_answer(&mut self, answer: ui::DialogAnswer) {
        let text = match answer.input_text {
            Some(input) => match yu_rt::dialog::DialogRequest::encode_input(&input) {
                Ok(bytes) => Some(bytes),
                Err(diag) => {
                    host_error!(
                        "yu-desktop: DIALOG#{} 入力テキストを CP932 にエンコード失敗: {diag}",
                        answer.id
                    );
                    return;
                }
            },
            None => None,
        };
        match self
            .engine
            .answer_dialog(answer.id, answer.button, text.as_deref())
        {
            Ok(()) => host_line!(
                "yu-desktop: DIALOG#{} answered button={} (task resumes next frame)",
                answer.id,
                answer.button
            ),
            Err(diag) => host_error!("yu-desktop: DIALOG#{} 回答被拒: {diag}", answer.id),
        }
    }

    /// 宿主帧数上限:`--frames` 优先,其次脚本的 `frames`。
    fn frames_limit(&self) -> Option<u64> {
        match self.opts.frames {
            Some(limit) if limit > 0 => Some(limit),
            _ => self.script_frames.map(u64::from),
        }
    }

    /// 记录一帧节奏样本(逐帧 CSV + 退出摘要)。
    fn record_frame(
        &mut self,
        interval_ms: f64,
        work_ms: f64,
        tick_ms: f64,
        render_ms: f64,
        draw_ms: f64,
    ) {
        let host_frame = self.frames;
        let engine_frame = self.engine.last_report().frame;
        // 渲染曲线:每宿主帧一个样本,环形只留最近 N 个。面板关闭期间
        // 也采样,打开时看到的是真实历史而不是重新开始的空曲线。
        self.render_samples.push_back(render_ms as f32);
        while self.render_samples.len() > ui::RENDER_GRAPH_SAMPLES {
            self.render_samples.pop_front();
        }
        self.frame_rows.push(FrameRow {
            host_frame,
            engine_frame,
            interval_ms,
            work_ms,
            tick_ms,
            render_ms,
            draw_ms,
        });
        // 长跑有界:只保留最近 20 万帧(约 55 分钟@60fps)。
        if self.frame_rows.len() > FRAME_STATS_CAP {
            self.frame_rows.drain(..FRAME_STATS_CAP / 2);
        }
    }

    /// 真实输入事件入队(时间戳 + 当前已完成的逻辑帧);下一次 tick
    /// 消费后由 `flush_input_latency` 结算“事件 → 画完”的毫秒与逻辑帧差。
    fn note_input_event(&mut self, kind: &'static str) {
        if self.pending_inputs.len() < PENDING_INPUT_CAP {
            self.pending_inputs
                .push((Instant::now(), kind, self.engine.frame()));
        }
    }

    fn flush_input_latency(&mut self) {
        if self.pending_inputs.is_empty() {
            return;
        }
        let now = Instant::now();
        let now_frame = self.engine.frame();
        for (at, kind, event_frame) in self.pending_inputs.drain(..) {
            let lag_frames = now_frame.saturating_sub(event_frame);
            self.input_latencies.push((
                now.duration_since(at).as_secs_f64() * 1000.0,
                lag_frames,
                kind,
            ));
        }
        if self.input_latencies.len() > INPUT_LATENCY_CAP {
            self.input_latencies.drain(..INPUT_LATENCY_CAP / 2);
        }
    }

    /// 打印指定宿主帧的 VM 命令耗时 top-10(`--profile-frames`)。
    fn print_profile(&mut self) {
        if !self.profile_frames.contains(&self.frames) {
            return;
        }
        let (rows, total_ns) = self.engine.take_command_profile(10);
        if rows.is_empty() {
            return;
        }
        host_error!(
            "[profile] command time (host frame {}, engine frame {}, sampled total {:.1}ms):",
            self.frames,
            self.engine.last_report().frame,
            total_ns as f64 / 1e6
        );
        for (name, hits, ns) in rows {
            let ms = ns as f64 / 1e6;
            let share = if total_ns > 0 {
                ns as f64 * 100.0 / total_ns as f64
            } else {
                0.0
            };
            let per_op = if hits > 0 {
                ns as f64 / hits as f64 / 1000.0
            } else {
                0.0
            };
            host_error!("  {name}: {ms:.1}ms ({share:.1}%) hits={hits} avg={per_op:.3}us/op");
        }
        let [calls, init, ty, other, skips, rows] = self.engine.varact_stats();
        host_error!(
            "  [varact] calls={calls} init={init} type={ty} other={other} \
             restore_skips={skips} restore_rows={rows}"
        );
        for ((script, pc), hits) in self.engine.take_varact_sites(5) {
            host_error!(
                "  [varact site] {} pc{pc} hits={hits}",
                self.engine.script_name(script)
            );
        }
        let (ops, bytes) = self.engine.varact_op_stats();
        host_error!(
            "  [varact ops] copy={} cut={} upper={} upper2={} lower={} lower2={} \
             hantozen={} zentohan={} src_bytes={bytes}",
            ops[0],
            ops[1],
            ops[2],
            ops[3],
            ops[4],
            ops[5],
            ops[6],
            ops[7]
        );
        // 调用点:回答“这 50 万条 LET 到底来自哪”。
        for (name, script, pc, hits) in self.engine.take_profile_sites(10) {
            let line = self
                .engine
                .script_position(script, pc)
                .map(|line| format!(" line {line}"))
                .unwrap_or_default();
            host_error!(
                "  [site] {name} {} pc{pc}{line} hits={hits}",
                self.engine.script_name(script)
            );
        }
    }

    /// 退出时的节奏/输入延迟摘要,可选逐帧 CSV(`--frame-stats`)。
    fn finish_timing(&mut self) {
        // 前 5 帧含首帧场景装载,单列;稳态口径从第 6 帧起。
        let mut steady: Vec<f64> = self
            .frame_rows
            .iter()
            .filter(|row| row.host_frame > 5)
            .map(|row| row.interval_ms)
            .collect();
        if !steady.is_empty() {
            let n = steady.len();
            let avg = steady.iter().sum::<f64>() / n as f64;
            steady.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let p50 = steady[n / 2];
            let p95 = steady[(n * 95 / 100).min(n - 1)];
            let max = steady[n - 1];
            host_line!(
                "frame pacing (steady, skip first 5): n={n} avg={avg:.2}ms p50={p50:.2}ms \
                 p95={p95:.2}ms max={max:.2}ms => {:.1}fps",
                1000.0 / avg
            );
        }
        let first: Vec<String> = self
            .frame_rows
            .iter()
            .take(5)
            .map(|row| format!("{:.1}", row.interval_ms))
            .collect();
        if !first.is_empty() {
            host_line!("frame pacing (first 5 intervals): {} ms", first.join(", "));
        }
        if !self.input_latencies.is_empty() {
            let mut by_kind: std::collections::BTreeMap<&'static str, Vec<(f64, u64)>> =
                std::collections::BTreeMap::new();
            for (ms, lag_frames, kind) in &self.input_latencies {
                by_kind.entry(*kind).or_default().push((*ms, *lag_frames));
            }
            for (kind, mut samples) in by_kind {
                let n = samples.len();
                let avg_ms = samples.iter().map(|(ms, _)| *ms).sum::<f64>() / n as f64;
                let avg_frames = samples.iter().map(|(_, lag)| *lag as f64).sum::<f64>() / n as f64;
                samples.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
                let p95_ms = samples[(n * 95 / 100).min(n - 1)].0;
                let max_ms = samples[n - 1].0;
                let mut lag_samples: Vec<u64> = samples.iter().map(|(_, lag)| *lag).collect();
                lag_samples.sort_unstable();
                let p95_frames = lag_samples[(n * 95 / 100).min(n - 1)];
                let max_frames = lag_samples[n - 1];
                host_line!(
                    "input latency [{kind}]: n={n} avg={avg_ms:.2}ms p95={p95_ms:.2}ms \
                     max={max_ms:.2}ms | frames avg={avg_frames:.2} p95={p95_frames} \
                     max={max_frames}"
                );
            }
        }
        if let Some(path) = self.opts.frame_stats.clone() {
            let mut text = String::from(
                "host_frame,engine_frame,interval_ms,work_ms,tick_ms,render_ms,draw_ms\n",
            );
            for row in &self.frame_rows {
                use std::fmt::Write as _;
                let _ = writeln!(
                    text,
                    "{},{},{:.3},{:.3},{:.3},{:.3},{:.3}",
                    row.host_frame,
                    row.engine_frame,
                    row.interval_ms,
                    row.work_ms,
                    row.tick_ms,
                    row.render_ms,
                    row.draw_ms
                );
            }
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() {
                    let _ = std::fs::create_dir_all(parent);
                }
            }
            match std::fs::write(&path, text) {
                Ok(()) => host_line!(
                    "frame stats: {} rows → {}",
                    self.frame_rows.len(),
                    path.display()
                ),
                Err(error) => {
                    host_error!("yu-desktop: 写逐帧统计 {} 失败: {error}", path.display())
                }
            }
        }
    }

    fn finish(&mut self) {
        if let Some(path) = self.opts.shot.clone() {
            match self.engine.write_png(&path) {
                Ok(bytes) => host_line!("shot: {} ({bytes} bytes)", path.display()),
                Err(error) => host_error!("yu-desktop: 写截图失败: {error}"),
            }
        }
        let elapsed = self.started.elapsed().as_secs_f64();
        let fps = if elapsed > 0.0 {
            self.frames as f64 / elapsed
        } else {
            0.0
        };
        host_line!(
            "yu-desktop: frames={} elapsed={elapsed:.2}s avg_fps={fps:.1} backend={} viewport={}x{}",
            self.frames,
            self.engine.backend(),
            self.viewport.0,
            self.viewport.1
        );
        self.finish_timing();
        // 退出时把诊断摘要写进日志(JSONL + 控制台):即使窗口一闪而过,
        // 复盘也能看到挂号表与“为什么卡”的判定。
        let snapshot = self.diagnostics.snapshot();
        log::error!(
            target: "yu-desktop",
            "exit diag: total={} engine_frame={} main_finished={} stuck=\"{}\" fatal={:?}",
            self.diagnostics.total_diags(),
            snapshot.stuck.engine_frame,
            self.engine.main_finished(),
            snapshot.stuck.hint(),
            snapshot.stuck.last_fatal
        );
        for row in snapshot.diag_rows.iter().take(12) {
            log::error!(
                target: "yu-desktop",
                "diag {} severity={} count={} last_frame={} at {}",
                row.code,
                row.severity.as_str(),
                row.count,
                row.last_frame,
                row.last_where
            );
        }
        if let Some(freeze) = &self.freeze {
            log::error!(
                target: "yu-desktop",
                "exit frozen: phase={} msg={} location={} engine_frame={}",
                freeze.record.phase.as_str(),
                freeze.record.message,
                freeze.record.location,
                freeze.record.engine_frame
            );
        }
    }

    /// 唤醒目标:`next_frame` 前 0.8ms。
    ///
    /// Windows 的 `WaitUntil` 实测比截止时间晚 ~0.3ms;提前量让工作正好
    /// 落在截止时间上,避免 60Hz 下每十几帧错过一次 vblank(58.8fps)。
    /// 只提前唤醒,不改变 `next_frame` 的节拍相位。
    fn wait_target(&self) -> Instant {
        self.next_frame
            .checked_sub(Duration::from_micros(WAIT_LEAD_US))
            .unwrap_or(self.next_frame)
    }

    /// 固定节拍推进(目标 60Hz):截止时间按帧长递增,而不是“本帧工作
    /// 结束后再睡满一帧”。后者把每帧耗时与睡眠串行,实测把 60Hz 压到
    /// ~38fps(P1 §5.7)。
    ///
    /// 错过截止时间时**对齐到下一个未来槽位**(保留相位),而不是
    /// `now + frame`——后者等于把本帧工作和一整帧睡眠再次串行。
    /// 落后太多时也不会补跑逻辑(P1 的“不追帧”仍由引擎 `max_step`
    /// 上限保证)。
    fn schedule_next_frame(&mut self, event_loop: &ActiveEventLoop) {
        let frame = Duration::from_secs_f64(1.0 / f64::from(self.opts.fps.max(1)));
        self.next_frame += frame;
        let now = Instant::now();
        if self.next_frame <= now {
            let behind = now.duration_since(self.next_frame);
            let missed = (behind.as_nanos() / frame.as_nanos().max(1)) as u32 + 1;
            self.next_frame += frame.saturating_mul(missed);
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.wait_target()));
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let (vw, vh) = self.viewport;
        let logical = LogicalSize::new(
            f64::from(self.opts.width.unwrap_or(vw)),
            f64::from(self.opts.height.unwrap_or(vh)),
        );
        let mut attrs = Window::default_attributes()
            .with_title(self.opts.title.clone())
            .with_inner_size(logical)
            .with_min_inner_size(LogicalSize::new(320.0, 240.0));
        if self.opts.fullscreen {
            attrs = attrs.with_fullscreen(Some(Fullscreen::Borderless(None)));
        }
        let window = match event_loop.create_window(attrs) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                self.fail(event_loop, format!("创建窗口失败: {error}"));
                return;
            }
        };
        let presenter = match self.engine.wgpu_renderer() {
            Some(renderer) => Presenter::new(window.clone(), self.opts.pixel, renderer.gpu()),
            None => {
                Err("桌面宿主需要 --backend wgpu(当前引擎是 soft);请用 --backend wgpu".to_string())
            }
        };
        let presenter = match presenter {
            Ok(presenter) => presenter,
            Err(error) => {
                self.fail(event_loop, error);
                return;
            }
        };
        let egui_state = egui_winit::State::new(
            self.egui_ctx.clone(),
            egui::ViewportId::ROOT,
            window.as_ref(),
            Some(window.scale_factor() as f32),
            None,
            None,
        );
        let egui_renderer = egui_wgpu::Renderer::new(
            &presenter.device,
            presenter.format,
            egui_wgpu::RendererOptions::default(),
        );
        host_line!(
            "yu-desktop: window={}x{} viewport={}x{} gpu={} format={:?}",
            window.inner_size().width,
            window.inner_size().height,
            vw,
            vh,
            presenter.adapter,
            presenter.format
        );
        self.window = Some(window.clone());
        self.presenter = Some(presenter);
        self.egui_state = Some(egui_state);
        self.egui_renderer = Some(egui_renderer);
        // 先给一张黑帧,避免首 tick(~2s)期间窗口空白。
        self.last_surface = Some(Arc::new(Surface {
            width: vw.max(1),
            height: vh.max(1),
            rgba: [0, 0, 0, 255].repeat(vw.max(1) as usize * vh.max(1) as usize),
        }));
        self.next_frame = Instant::now();
        window.request_redraw();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(window) = self.window.clone() else {
            return;
        };
        if window.id() != window_id {
            return;
        }
        // egui 只在面板/冻结横幅/DIALOG 模态真正可见时才允许消费游戏输入。
        // 惰性化后 `egui_ctx` 保留“上一帧指针在面板上”的记忆;若仍按它
        // 拦截事件,隐藏面板后游戏会永久收不到鼠标/键盘。
        // DIALOG 模态期间官方引擎是同步阻塞(0x440370 暂停计时器):
        // 指针/滚轮/键盘一律不进游戏,只喂 egui(模态按钮/输入框)。
        let dialog_pending = self.dialog_pending();
        let egui_ui_active = self.show_debug || self.freeze.is_some() || dialog_pending;
        let egui_consumed = {
            let response = self
                .egui_state
                .as_mut()
                .map(|state| state.on_window_event(&window, &event));
            egui_ui_active && response.is_some_and(|response| response.consumed)
        };

        match event {
            WindowEvent::CloseRequested => {
                // 引擎语义:WM_CLOSE 只推 `@_WINDOW_EXEC_CLOSE=1`,由脚本
                // 弹确认框/决定是否 END;宿主不得直接退出。脚本不处理时,
                // 第二次关闭请求强制退出(宿主兜底,不是引擎语义)。
                self.close_requests += 1;
                match panic_guard::close_decision(self.engine.main_finished(), self.close_requests)
                {
                    CloseAction::Exit => {
                        self.stopping = true;
                        event_loop.exit();
                    }
                    CloseAction::SignalEngine => {
                        self.engine.request_window_close();
                        host_error!("yu-desktop: close requested → @_WINDOW_EXEC_CLOSE=1");
                    }
                }
            }
            WindowEvent::Resized(size) => {
                // 0x0(=最小化)不重配 surface:`Presenter::resize` 显式忽略,
                // 保持上一张有效配置;还原时的 Resized 会带回真实尺寸。
                if let Some(presenter) = self.presenter.as_mut() {
                    presenter.resize(size.width, size.height);
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                let size = window.inner_size();
                if let Some(presenter) = self.presenter.as_mut() {
                    presenter.resize(size.width, size.height);
                }
            }
            WindowEvent::Focused(false) => self.release_all(),
            WindowEvent::RedrawRequested => self.draw(event_loop),
            WindowEvent::CursorMoved { position, .. } => {
                if !(egui_consumed
                    || egui_ui_active && self.egui_ctx.egui_wants_pointer_input()
                    || dialog_pending)
                {
                    self.note_mouse_activity(MouseActivity::Move);
                    self.update_cursor_from_position(position.x, position.y);
                    self.note_input_event("cursor");
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if egui_consumed
                    || (egui_ui_active && self.egui_ctx.egui_wants_pointer_input())
                    || dialog_pending
                {
                    return;
                }
                if let Some(name) = winit_pointer_button(button).name() {
                    if state == ElementState::Pressed {
                        self.note_mouse_activity(MouseActivity::Click);
                    }
                    self.set_button(name, state == ElementState::Pressed);
                    self.note_input_event("mouse");
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if egui_consumed
                    || (egui_ui_active && self.egui_ctx.egui_wants_pointer_input())
                    || dialog_pending
                {
                    return;
                }
                // 引擎 `FUN_0045a3b4` WM_MOUSEWHEEL → 一格脉冲;脚本读
                // `_MOUSE_U/_MOUSE_D`(ycd 码 5/6)。winit 正 y = 向上。
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => f64::from(y),
                    MouseScrollDelta::PixelDelta(position) => position.y,
                };
                if let Some(name) = wheel_button(dy) {
                    self.note_mouse_activity(MouseActivity::Wheel);
                    self.engine.pulse_input(name);
                    self.note_input_event("wheel");
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else {
                    return;
                };
                let pressed = event.state == ElementState::Pressed;
                // 冻结后 Esc = 退出(横幅上的 “Esc / Quit” 提示)。
                if self.freeze.is_some() && pressed && code == KeyCode::Escape {
                    host_line!("yu-desktop: Esc while frozen → exit");
                    self.stopping = true;
                    event_loop.exit();
                    return;
                }
                // F1 只在显式启用调试面板(`--debug-ui`)时归宿主;否则
                // 透传给游戏(引擎不把 F1 当调试键)。
                if code == KeyCode::F1 && pressed && !event.repeat && self.opts.debug_ui {
                    self.show_debug = !self.show_debug;
                    return;
                }
                if egui_consumed
                    || (egui_ui_active
                        && (self.egui_ctx.egui_wants_keyboard_input()
                            || self.egui_ctx.egui_wants_pointer_input()))
                    || dialog_pending
                {
                    return;
                }
                if pressed && !event.repeat {
                    self.last_keyboard_activity_ms = self.now_ms();
                }
                if let Some(name) = key_button(&format!("{code:?}")) {
                    self.set_button(&name, pressed);
                    self.note_input_event("key");
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.stopping {
            event_loop.exit();
            return;
        }
        let now = Instant::now();
        let wait_target = self.wait_target();
        if now < wait_target {
            event_loop.set_control_flow(ControlFlow::WaitUntil(wait_target));
            return;
        }
        let frame_started = Instant::now();
        let interval_ms = frame_started
            .duration_since(self.last_frame_start)
            .as_secs_f64()
            * 1000.0;
        self.last_frame_start = frame_started;

        // ---- 冻结:不再推进引擎,只出横幅帧(保留最后一帧) ----
        let keep_drawing = self
            .freeze
            .as_ref()
            .map(panic_guard::Freeze::keep_drawing)
            .unwrap_or(false);
        if self.freeze.is_some() {
            if keep_drawing {
                let draw_started = Instant::now();
                let outcome =
                    panic_guard::catch(Phase::Draw, self.frames, self.engine.frame(), || {
                        self.draw(event_loop)
                    });
                let draw_ms = draw_started.elapsed().as_secs_f64() * 1000.0;
                if let Err(record) = outcome {
                    self.enter_freeze(record);
                }
                if self.frames <= 8 || self.frames.is_multiple_of(60) {
                    host_error!("frame {} (frozen): draw={draw_ms:.1}ms", self.frames);
                }
            }
            self.frames = self.frames.saturating_add(1);
            let work_ms = frame_started.elapsed().as_secs_f64() * 1000.0;
            self.record_frame(interval_ms, work_ms, 0.0, 0.0, 0.0);
            if let Some(limit) = self.frames_limit() {
                if self.frames >= limit {
                    host_line!(
                        "yu-desktop: frame limit {limit} reached while frozen;                          保留最后一帧并退出(进程未崩溃)"
                    );
                    self.stopping = true;
                    event_loop.exit();
                    return;
                }
            }
            self.schedule_next_frame(event_loop);
            return;
        }

        // 脚本在 tick 内读 `@_MOUSE_X/Y`;坐标来源只有 `CursorMoved`
        // (与 winit/系统一致的普通鼠标路径,不做额外轮询)。
        self.apply_script(event_loop);
        if self.stopping {
            event_loop.exit();
            return;
        }

        // `--profile-frames` 的窗口 = 这一帧(否则第一次打印会把此前
        // 所有帧的累计都算进来,曾把設定点击误读成 1.8s)。
        if !self.profile_frames.is_empty() {
            self.engine.reset_command_profile();
        }
        let tick_started = Instant::now();
        let selftest = self.opts.panic_selftest == Some(self.frames);
        let engine_frame = self.engine.frame();
        let outcome = panic_guard::catch(Phase::Tick, self.frames, engine_frame, || {
            if selftest {
                panic!(
                    "yu-desktop panic selftest (injected at host frame {})",
                    self.frames
                );
            }
            self.engine.tick_realtime()
        });
        match outcome {
            Ok(Ok(rendered)) => {
                self.last_surface = Some(rendered.surface);
                self.frames += 1;
            }
            Ok(Err(error)) => {
                self.fail(event_loop, format!("tick 失败: {error}"));
                return;
            }
            Err(record) => {
                self.enter_freeze(record);
                self.frames += 1;
                let draw_started = Instant::now();
                if let Err(record) =
                    panic_guard::catch(Phase::Draw, self.frames, self.engine.frame(), || {
                        self.draw(event_loop)
                    })
                {
                    self.enter_freeze(record);
                }
                let draw_ms = draw_started.elapsed().as_secs_f64() * 1000.0;
                host_error!(
                    "frame {}: frozen after panic (draw={draw_ms:.1}ms); engine stopped,                      last frame kept, Esc/Quit exits",
                    self.frames
                );
                self.schedule_next_frame(event_loop);
                return;
            }
        }
        // 诊断账本:取走引擎/声音诊断、聚合 skipped、更新卡住判定。
        if let Some(surface) = self.last_surface.as_ref() {
            let host_frame = self.frames;
            self.diagnostics
                .observe(&mut self.engine, &surface.rgba, host_frame);
        }
        let tick_host_ms = tick_started.elapsed().as_secs_f64() * 1000.0;
        let draw_started = Instant::now();
        if let Err(record) =
            panic_guard::catch(Phase::Draw, self.frames, self.engine.frame(), || {
                self.draw(event_loop)
            })
        {
            self.enter_freeze(record);
            self.frames = self.frames.saturating_add(1);
            self.schedule_next_frame(event_loop);
            return;
        }
        let draw_ms = draw_started.elapsed().as_secs_f64() * 1000.0;
        let work_ms = frame_started.elapsed().as_secs_f64() * 1000.0;
        let (engine_tick_ms, engine_render_ms) = {
            let report = self.engine.last_report();
            (report.tick_ms, report.render_ms)
        };
        self.record_frame(
            interval_ms,
            work_ms,
            engine_tick_ms,
            engine_render_ms,
            draw_ms,
        );
        self.flush_input_latency();
        if self.frames <= 8 || self.frames.is_multiple_of(60) {
            let report = self.engine.last_report();
            // 周期采样是进度不是警告:INFO(绿);spike 另走 host_error!(WARN)。
            host_line!(
                "frame {}: host_tick={tick_host_ms:.1}ms engine_tick={:.1}ms engine_render={:.1}ms draw={draw_ms:.1}ms",
                self.frames, report.tick_ms, report.render_ms
            );
        }
        // 渲染侧长帧取证(文本页签曾出现 175ms render):把后端报告里
        // 的文本/跳过项打出来,区分“文本栅格化”与“VM 慢”。
        let render_ms = self.engine.last_report().render_ms;
        if render_ms > 30.0 {
            let detail = self.engine.render_report().map(|report| {
                (
                    report.ops_drawn,
                    report.layers,
                    report.passes,
                    report.skipped_text,
                    report.missing_assets.len(),
                    report.text_diags.clone(),
                    report
                        .skipped
                        .iter()
                        .take(3)
                        .map(|skip| format!("{}:{} x{}", skip.kind, skip.reason, skip.count))
                        .collect::<Vec<_>>(),
                )
            });
            if let Some((ops, layers, passes, text, missing, text_diags, skips)) = detail {
                host_error!(
                    "[render spike] frame {} render={render_ms:.1}ms ops={ops} layers={layers} \
                     passes={passes} text_runs={text} missing_assets={missing} \
                     skip_top={} text_diags={:?}",
                    self.frames,
                    skips.join(" | "),
                    text_diags.iter().take(3).collect::<Vec<_>>()
                );
            }
        }
        if self.frames == 1 || self.frames == 4 {
            let top = self.engine.top_commands(10);
            if !top.is_empty() {
                host_error!("[profile] top commands (frame {}):", self.frames);
                for (name, hits) in top {
                    host_error!("  {name}: {hits}");
                }
            }
        }
        // `--profile-frames` 指定的帧按**耗时**打印 top-10(与上面的
        // 频度表互补:P0-6)。
        self.print_profile();
        if self.fatal.is_some() {
            return;
        }
        self.update_cursor_presentation();
        // 脚本 `END`(主任务结束)→ 正常收尾。
        if self.engine.main_finished() {
            self.stopping = true;
            event_loop.exit();
            return;
        }
        if let Some(limit) = self.frames_limit() {
            if self.frames >= limit {
                self.stopping = true;
                event_loop.exit();
                return;
            }
        }
        self.schedule_next_frame(event_loop);
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        // 真正收尾在 `run()` 里调用 `finish()`(此时 engine 仍可用)。
    }
}

/// winit 鼠标按钮 → 平台中性的指针按钮(命名与 `event.button` 同源)。
fn winit_pointer_button(button: MouseButton) -> PointerButton {
    match button {
        MouseButton::Left => PointerButton::Left,
        MouseButton::Middle => PointerButton::Middle,
        MouseButton::Right => PointerButton::Right,
        MouseButton::Back => PointerButton::Other(3),
        MouseButton::Forward => PointerButton::Other(4),
        MouseButton::Other(n) => PointerButton::Other(n),
    }
}

/// 平台中性光标形状 → winit 光标。
fn winit_cursor_icon(shape: CursorShape) -> CursorIcon {
    match shape {
        CursorShape::Default => CursorIcon::Default,
        CursorShape::Text => CursorIcon::Text,
        CursorShape::Wait => CursorIcon::Wait,
        CursorShape::Crosshair => CursorIcon::Crosshair,
        CursorShape::VerticalArrows => CursorIcon::NsResize,
        CursorShape::DiagonalDown => CursorIcon::NwseResize,
        CursorShape::DiagonalUp => CursorIcon::NeswResize,
        CursorShape::HorizontalArrows => CursorIcon::EwResize,
        CursorShape::FourWay => CursorIcon::AllScroll,
        CursorShape::NotAllowed => CursorIcon::NotAllowed,
        CursorShape::Hand => CursorIcon::Pointer,
        CursorShape::Progress => CursorIcon::Progress,
        CursorShape::Help => CursorIcon::Help,
    }
}
