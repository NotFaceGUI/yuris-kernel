//! `yu-host-desktop` · 原生桌面宿主(Windows 先行)。
//!
//! # 定位
//!
//! 窗口 = winit 0.30;游戏帧仍由 `yu-cli::Engine`(wgpu 30 / soft)产出;
//! 本 crate 只做「窗口 + surface 上屏 + 输入转发 + egui 调试面板」。
//! 渲染语义、VM、音频状态机都不在这里。
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

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui_wgpu::ScreenDescriptor;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorIcon, Fullscreen, Window, WindowId};

use yu_backend_soft::Surface;
use yu_cli::engine::Engine;
use yu_platform_input::{
    cursor_shape, cursor_visible, key_button, mouse_activity_reactive, wheel_button, CursorClock,
    CursorShape, MouseActivity, PointerButton,
};

// 宿主日志与终端输出统一走 `yu-cli::output`(项目纪律:src 内不直接
// `println!/eprintln!`)。
macro_rules! host_error {
    ($($arg:tt)*) => { yu_cli::output::error(format!($($arg)*)) };
}
macro_rules! host_line {
    ($($arg:tt)*) => { yu_cli::output::line(format!($($arg)*)) };
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
        }
    }
}

/// 启动桌面窗口。返回时窗口已关闭;`Err` 表示启动/运行失败。
pub fn run(engine: Engine, opts: HostOptions) -> Result<(), String> {
    let event_loop = EventLoop::new().map_err(|e| format!("创建事件循环失败: {e}"))?;
    event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now()));
    let mut app = App::new(engine, opts);
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
            .find(|f| matches!(f, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm))
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
        let bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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
    let scale = (f64::from(win_w) / f64::from(game_w))
        .min(f64::from(win_h) / f64::from(game_h));
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
    last_surface: Option<Surface>,
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
}

impl App {
    fn new(mut engine: Engine, opts: HostOptions) -> Self {
        let egui_ctx = egui::Context::default();
        install_debug_fonts(&egui_ctx);
        // 桌面宿主按固定两帧的步进推进逻辑:长帧后不再一次性跑 500ms
        // 的脚本批次(那是 title 切换时 2.5s 卡顿的来源)。
        let frame_ms = (1000 / opts.fps.max(1)).max(1);
        engine.set_max_step_ms(frame_ms * 2);
        let viewport = engine.viewport();
        let show_debug = opts.debug_ui;
        Self {
            engine,
            opts,
            viewport: (viewport.width, viewport.height),
            window: None,
            presenter: None,
            egui_ctx,
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
        }
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, message: String) {
        host_error!("yu-desktop: {message}");
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
        self.engine.set_input(name, pressed);
    }

    fn release_all(&mut self) {
        self.engine.release_all_inputs();
        self.held.clear();
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

    fn client_from_logical(&self, x: i64, y: i64) -> (f64, f64) {
        let (w, h) = self.window_size();
        let (vw, vh) = self.viewport;
        let fit = fit_rect(w, h, vw, vh);
        (x as f64 * fit.scale + fit.ox, y as f64 * fit.scale + fit.oy)
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
        let (device, queue) = match self.presenter.as_ref() {
            Some(presenter) => (presenter.device.clone(), presenter.queue.clone()),
            None => return,
        };

        let raw_input = match self.egui_state.as_mut() {
            Some(state) => state.take_egui_input(&window),
            None => egui::RawInput::default(),
        };
        let ctx = self.egui_ctx.clone();
        let mut full_output = ctx.run_pass(raw_input, |ctx| {
            if self.show_debug {
                self.build_ui(ctx);
            }
        });
        if let Some(state) = self.egui_state.as_mut() {
            state.handle_platform_output(&window, full_output.platform_output);
        }
        let paint_jobs = ctx.tessellate(full_output.shapes, full_output.pixels_per_point);
        // egui 0.37 的 TexturesDelta 有 Drop 守卫:消费后必须清空。
        let mut textures_delta = std::mem::take(&mut full_output.textures_delta);
        let sets: Vec<_> = textures_delta.set.drain().collect();
        let free_ids: Vec<_> = textures_delta.free.drain().collect();
        textures_delta.clear();
        let screen = ScreenDescriptor {
            size_in_pixels: [width, height],
            pixels_per_point: full_output.pixels_per_point,
        };

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
        let command_buffers = egui_renderer.update_buffers(
            &device,
            &queue,
            &mut encoder,
            &paint_jobs,
            &screen,
        );

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
        if !paint_jobs.is_empty() {
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
    }

    fn build_ui(&mut self, ctx: &egui::Context) {
        // 先把 Engine 的读数复制出来,避免 UI 闭包同时持有 &mut engine 和 &self。
        let report = self.engine.last_report().clone();
        let viewport = self.engine.viewport();
        let backend = self.engine.backend();
        let tick_avg = self.engine.tick_avg_ms();
        let render_avg = self.engine.render_avg_ms();
        let textures_cached = self.engine.textures_cached();
        let textures_decoded = self.engine.textures_decoded();
        let events_drained = self.engine.events_drained();
        let events_dropped = self.engine.events_dropped();
        let sound_layers = self.engine.sound_layers();
        let (win_w, win_h) = self.window_size();
        let cursor = self.cursor;
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

        let mut restart = false;
        let mut hide = false;
        egui::Window::new("yu-desktop")
            .default_pos([12.0, 12.0])
            .default_width(360.0)
            .resizable(true)
            .show(ctx, |ui| {
                egui::Grid::new("yu-desktop-stats")
                    .num_columns(2)
                    .spacing([12.0, 2.0])
                    .show(ui, |ui| {
                        let mut row = |name: &str, value: String| {
                            ui.label(name);
                            ui.monospace(value);
                            ui.end_row();
                        };
                        row("frame", report.frame.to_string());
                        row("backend", backend.to_string());
                        row("viewport", format!("{}x{}", viewport.width, viewport.height));
                        row("window", format!("{win_w}x{win_h}"));
                        row(
                            "tick",
                            format!("{:.2} ms (avg {:.2})", report.tick_ms, tick_avg),
                        );
                        row(
                            "render",
                            format!("{:.2} ms (avg {:.2})", report.render_ms, render_avg),
                        );
                        row(
                            "ops",
                            format!("{} drawn / {} skipped", report.executed, report.skipped),
                        );
                        row("passes", report.passes.to_string());
                        row(
                            "layers",
                            format!("{} / {} visible", report.layers, report.visible),
                        );
                        row("drawable", report.drawable.to_string());
                        row(
                            "textures",
                            format!("{textures_cached} cached / {textures_decoded} decoded"),
                        );
                        row(
                            "events",
                            format!("{events_drained} drained / {events_dropped} dropped"),
                        );
                        row("sound", format!("{sound_layers} layers"));
                        row("cursor", format!("{}, {}", cursor.0, cursor.1));
                        row("held", held.clone());
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Restart").clicked() {
                        restart = true;
                    }
                    if ui.button("Hide (F1)").clicked() {
                        hide = true;
                    }
                });
                ui.separator();
                ui.label(format!("GPU: {gpu}"));
                ui.label("click the game area to play; F1 toggles this panel");
            });

        if restart {
            if let Err(error) = self.engine.restart() {
                host_error!("yu-desktop: restart 失败: {error}");
            }
        }
        if hide {
            self.show_debug = false;
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
            Some(renderer) => {
                Presenter::new(window.clone(), self.opts.pixel, renderer.gpu())
            }
            None => Err(
                "桌面宿主需要 --backend wgpu(当前引擎是 soft);请用 --backend wgpu".to_string(),
            ),
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
        self.last_surface = Some(Surface {
            width: vw.max(1),
            height: vh.max(1),
            rgba: vec![0, 0, 0, 255].repeat(vw.max(1) as usize * vh.max(1) as usize),
        });
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
        let egui_consumed = match self.egui_state.as_mut() {
            Some(state) => state.on_window_event(&window, &event).consumed,
            None => false,
        };

        match event {
            WindowEvent::CloseRequested => {
                // 引擎语义:WM_CLOSE 只推 `@_WINDOW_EXEC_CLOSE=1`,由脚本
                // 弹确认框/决定是否 END;宿主不得直接退出。脚本不处理时,
                // 第二次关闭请求强制退出(宿主兜底,不是引擎语义)。
                self.close_requests += 1;
                if self.engine.main_finished() || self.close_requests >= 2 {
                    self.stopping = true;
                    event_loop.exit();
                } else {
                    self.engine.request_window_close();
                    host_error!("yu-desktop: close requested → @_WINDOW_EXEC_CLOSE=1");
                }
            }
            WindowEvent::Resized(size) => {
                if let Some(presenter) = self.presenter.as_mut() {
                    presenter.resize(size.width.max(1), size.height.max(1));
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                let size = window.inner_size();
                if let Some(presenter) = self.presenter.as_mut() {
                    presenter.resize(size.width.max(1), size.height.max(1));
                }
            }
            WindowEvent::Focused(false) => self.release_all(),
            WindowEvent::RedrawRequested => self.draw(event_loop),
            WindowEvent::CursorMoved { position, .. } => {
                if !egui_consumed && !self.egui_ctx.egui_wants_pointer_input() {
                    self.note_mouse_activity(MouseActivity::Move);
                    self.update_cursor_from_position(position.x, position.y);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if egui_consumed || self.egui_ctx.egui_wants_pointer_input() {
                    return;
                }
                if let Some(name) = winit_pointer_button(button).name() {
                    if state == ElementState::Pressed {
                        self.note_mouse_activity(MouseActivity::Click);
                    }
                    self.set_button(name, state == ElementState::Pressed);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if egui_consumed || self.egui_ctx.egui_wants_pointer_input() {
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
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else {
                    return;
                };
                let pressed = event.state == ElementState::Pressed;
                // F1 只在显式启用调试面板(`--debug-ui`)时归宿主;否则
                // 透传给游戏(引擎不把 F1 当调试键)。
                if code == KeyCode::F1 && pressed && !event.repeat && self.opts.debug_ui {
                    self.show_debug = !self.show_debug;
                    return;
                }
                if egui_consumed
                    || self.egui_ctx.egui_wants_keyboard_input()
                    || self.egui_ctx.egui_wants_pointer_input()
                {
                    return;
                }
                if pressed && !event.repeat {
                    self.last_keyboard_activity_ms = self.now_ms();
                }
                if let Some(name) = key_button(&format!("{code:?}")) {
                    self.set_button(&name, pressed);
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
        if now < self.next_frame {
            event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
            return;
        }
        // 脚本在 tick 内读 `@_MOUSE_X/Y`;坐标来源只有 `CursorMoved`
        // (与 winit/系统一致的普通鼠标路径,不做额外轮询)。
        let tick_started = Instant::now();
        match self.engine.tick_realtime() {
            Ok(rendered) => {
                self.last_surface = Some(rendered.surface);
                self.frames += 1;
            }
            Err(error) => {
                self.fail(event_loop, format!("tick 失败: {error}"));
                return;
            }
        }
        let tick_host_ms = tick_started.elapsed().as_secs_f64() * 1000.0;
        let draw_started = Instant::now();
        self.draw(event_loop);
        let draw_ms = draw_started.elapsed().as_secs_f64() * 1000.0;
        if self.frames <= 8 || self.frames % 60 == 0 {
            let report = self.engine.last_report();
            host_error!(
                "frame {}: host_tick={tick_host_ms:.1}ms engine_tick={:.1}ms engine_render={:.1}ms draw={draw_ms:.1}ms",
                self.frames, report.tick_ms, report.render_ms
            );
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
        if let Some(limit) = self.opts.frames {
            if limit > 0 && self.frames >= limit {
                self.stopping = true;
                event_loop.exit();
                return;
            }
        }
        let frame = Duration::from_secs_f64(1.0 / f64::from(self.opts.fps.max(1)));
        self.next_frame = Instant::now() + frame;
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
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
        MouseButton::Other(n) => PointerButton::Other(u16::from(n)),
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
