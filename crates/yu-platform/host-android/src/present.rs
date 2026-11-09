//! 零拷贝上屏:wgpu 直接把引擎的屏幕纹理画到 Android 原生窗口。
//!
//! # 为什么需要它
//!
//! 默认路径是「引擎渲染 → 读回 CPU(`read_screen_now`)→ 壳画 Bitmap」。
//! 实测 800x600 每帧读回 1.9MB,在模拟器上耗时 9.8ms,占帧预算的一半以上
//! (见 `android/NATIVE_API.md` §性能)。
//!
//! 本模块改成「引擎渲染到 GPU 纹理 → 同 device 的 surface 直接采样上屏」,
//! 与桌面宿主 `Presenter` 完全同构:免掉读回与重上传两次整帧拷贝。
//!
//! # 线程与生命周期
//!
//! - `ANativeWindow_fromSurface` 会 acquire 一次引用,必须在释放时
//!   `ANativeWindow_release`(`NativeWindow` 的 `Drop` 负责)。
//! - `wgpu::Surface` 持有窗口的强引用,所以 surface 必须在 window 之前失效;
//!   结构体字段顺序保证 drop 顺序(surface 先 drop)。
//! - 全部调用在渲染线程(与内核单线程约束一致)。
//!
//! # 边界
//!
//! 只在 `target_os = "android"` 编译;其它平台用空实现,保证宿主 crate
//! 在开发机上仍可编译与测试。
//!
//! # unsafe 边界(本 crate 唯一)
//!
//! 只有三处,全部必要且局部:
//! 1. `ANativeWindow_fromSurface` —— JNI 原生调用,必须 unsafe;
//! 2. `create_surface_unsafe` —— wgpu 要求调用方保证窗口句柄的生命周期;
//! 3. `ANativeWindow_getWidth/Height` —— 原生句柄查询。
//!
//! 三者都由 `NativeWindow` 的 RAII(acquire → release)与其字段顺序保证
//! 生命周期正确;其余代码保持安全 Rust。

#![allow(unsafe_code)]

use yu_backend_wgpu::GpuContext;

/// 呈现错误(宿主只用于诊断,不回传 Java)。
#[derive(Debug)]
pub enum PresentError {
    /// 当前平台没有原生窗口呈现能力。
    Unsupported,
    /// 取得 `ANativeWindow` 失败(Surface 已失效等)。
    NoNativeWindow,
    /// 建立 wgpu surface 失败。
    Surface(String),
    /// 取得交换链纹理失败(通常是 transient,调用方可跳过本帧)。
    Acquire(String),
}

impl std::fmt::Display for PresentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PresentError::Unsupported => f.write_str("当前平台不支持原生窗口上屏"),
            PresentError::NoNativeWindow => f.write_str("ANativeWindow 取得失败"),
            PresentError::Surface(detail) => write!(f, "创建 surface 失败: {detail}"),
            PresentError::Acquire(detail) => write!(f, "取得呈现纹理失败: {detail}"),
        }
    }
}

/// 原生窗口上屏器。
///
/// 持有一个全屏 blit 管线:采样引擎屏幕纹理 → 写入交换链。着色器与桌面
/// 宿主同源(`present.wgsl`),保证两平台的缩放/采样行为一致。
pub struct NativePresenter {
    inner: inner::Inner,
    /// 已绑定的引擎屏幕代际 `(id, 宽, 高)`;不变则不重建视图。
    bound: Option<(u64, u32, u32)>,
    /// 呈现尺寸(交换链大小)。
    size: (u32, u32),
    /// 本帧是否成功呈现(诊断用)。
    presented_frames: u64,
}

impl NativePresenter {
    /// 用 Java `Surface`(JNI jobject)建立呈现器。
    ///
    /// # Safety
    ///
    /// `env` 必须是当前线程有效的 `JNIEnv` 指针,`surface` 必须是有效的
    /// `android.view.Surface` jobject。由 JNI 导出函数保证。
    ///
    /// # Errors
    ///
    /// 窗口无效或该适配器不支持窗口 surface 时返回错误。
    pub unsafe fn from_java_surface(
        env: *mut jni::sys::JNIEnv,
        surface: jni::sys::jobject,
        gpu: &GpuContext,
    ) -> Result<Self, PresentError> {
        inner::Inner::new(env, surface, gpu).map(|inner| Self {
            inner,
            bound: None,
            size: (0, 0),
            presented_frames: 0,
        })
    }

    /// 按窗口当前尺寸重建交换链(尺寸变化 / 恢复后调用)。
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), PresentError> {
        if width == 0 || height == 0 {
            return Ok(());
        }
        self.inner.resize(width, height)?;
        self.size = (width, height);
        Ok(())
    }

    /// 把引擎的屏幕纹理呈现到窗口。
    ///
    /// `id` 是引擎屏幕目标的代际键:变化时(视口重建)才重建视图与绑定组。
    /// 采样在队列提交顺序上安全 —— 上一提交的采样先于下一提交的写入。
    pub fn present(
        &mut self,
        texture: &wgpu::Texture,
        id: u64,
        width: u32,
        height: u32,
    ) -> Result<(), PresentError> {
        self.inner.present(texture, id, width, height, &mut self.bound)?;
        self.presented_frames += 1;
        Ok(())
    }

    /// 已呈现帧数(诊断)。
    #[must_use]
    pub fn presented_frames(&self) -> u64 {
        self.presented_frames
    }

    /// 交换链尺寸。
    #[must_use]
    pub fn size(&self) -> (u32, u32) {
        self.size
    }
}

#[cfg(target_os = "android")]
mod inner {
    //! Android 实现:ANativeWindow + wgpu::Surface。

    use super::PresentError;
    use yu_backend_wgpu::GpuContext;

    pub struct Inner {
        /// 字段顺序即 drop 顺序:surface 必须先于 window 释放
        /// (surface 持有窗口的强引用)。
        surface: wgpu::Surface<'static>,
        /// 只作 RAII 持有方(见上方 drop 顺序说明),代码里不直接读取。
        #[allow(dead_code)]
        window: NativeWindow,
        device: wgpu::Device,
        queue: wgpu::Queue,
        config: wgpu::SurfaceConfiguration,
        pipeline: wgpu::RenderPipeline,
        layout: wgpu::BindGroupLayout,
        sampler: wgpu::Sampler,
        bind_group: Option<wgpu::BindGroup>,
        view: Option<wgpu::TextureView>,
        /// 等比缩放参数:`[viewport_w, viewport_h, window_w, window_h]`。
        params: wgpu::Buffer,
    }

    /// `ANativeWindow*` 的 RAII 包装(acquire 一次,drop 时 release)。
    struct NativeWindow(*mut ndk_sys::ANativeWindow);

    // wgpu 的 surface 持有窗口引用;渲染线程单线程使用,故可跨线程移动
    // 由宿主保证(本 crate 的会话本身就是单线程的)。
    unsafe impl Send for NativeWindow {}
    unsafe impl Sync for NativeWindow {}

    impl Drop for NativeWindow {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: 指针来自 ANativeWindow_fromSurface,非空,且只释放一次。
                unsafe { ndk_sys::ANativeWindow_release(self.0) };
            }
        }
    }

    impl Inner {
        pub fn new(
            env: *mut jni::sys::JNIEnv,
            surface: jni::sys::jobject,
            gpu: &GpuContext,
        ) -> Result<Self, PresentError> {
            if env.is_null() || surface.is_null() {
                return Err(PresentError::NoNativeWindow);
            }
            // SAFETY: env/surface 由 JNI 导出函数传入且在本调用内有效。
            let raw = unsafe { ndk_sys::ANativeWindow_fromSurface(env, surface) };
            let window = NativeWindow(raw);
            if window.0.is_null() {
                return Err(PresentError::NoNativeWindow);
            }

            // Android 的原生窗口就是 ANativeWindow 指针;wgpu 要求调用方
            // 保证该指针在 surface 生命周期内有效 —— 由 `window` 字段
            // (在 surface 之后声明、更晚 drop)保证。
            let raw_window_handle =
                raw_window_handle::RawWindowHandle::AndroidNdk(raw_window_handle::AndroidNdkWindowHandle::new(
                    std::ptr::NonNull::new(window.0.cast()).ok_or(PresentError::NoNativeWindow)?,
                ));
            // SAFETY: 句柄非空且在 surface 存活期间有效(见上)。
            let surface = unsafe {
                gpu.instance
                    .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                        // Android 的 display 是零大小标记,安全构造。
                        raw_display_handle: Some(
                            raw_window_handle::RawDisplayHandle::Android(
                                raw_window_handle::AndroidDisplayHandle::new(),
                            ),
                        ),
                        raw_window_handle,
                    })
            }
            .map_err(|e| PresentError::Surface(e.to_string()))?;

            let caps = surface.get_capabilities(&gpu.adapter);
            if caps.formats.is_empty() {
                return Err(PresentError::Surface(format!(
                    "适配器 {} 不支持窗口 surface",
                    gpu.adapter_info.name
                )));
            }
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

            let (width, height) = (
                ndk_width(window.0).max(1),
                ndk_height(window.0).max(1),
            );
            // 呈现模式:**不用 vsync**。
            //
            // 帧节奏由壳的 `MainActivity` 自己控制(`sleepUntil` 补足到
            // 1/60 秒,且错过截止时"吸收欠时、下一帧重新计时")。如果这里
            // 再叠一层 `AutoVsync`,两层节拍会互相锁死:
            // - 壳某一帧因 GPU 排队略超 16.7ms ⇒ vsync 呈现要等**下一个**
            //   vblank ⇒ 这一帧变成 33ms;
            // - 下一帧壳只睡到 +16.7ms,又撞在 vsync 窗口外 ⇒ 再等一个
            //   vblank ⇒ 稳定掉到 30fps。
            //
            // 真机实测(`tick+render` 只有 6~11ms 却 `fps=31.8/37.5/39.8`)
            // 就是这条:CPU 侧完全没有超时,帧率却腰斩。
            //
            // `Immediate` 让 `queue.present()` 不阻塞等 vblank,撕裂风险由
            // 壳的固定 60Hz 节拍兜住(与桌面 `yu-cli run --mode desktop`
            // 的 `PresentMode::AutoNoVsync` 同口径)。驱动不支持时回退到
            // 能力表里的第一项。
            let present_mode = if caps
                .present_modes
                .contains(&wgpu::PresentMode::Immediate)
            {
                wgpu::PresentMode::Immediate
            } else if caps
                .present_modes
                .contains(&wgpu::PresentMode::AutoNoVsync)
            {
                wgpu::PresentMode::AutoNoVsync
            } else {
                caps.present_modes
                    .first()
                    .copied()
                    .unwrap_or(wgpu::PresentMode::Fifo)
            };
            let config = wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format,
                color_space: wgpu::SurfaceColorSpace::Auto,
                width,
                height,
                present_mode,
                // 单帧在途:不排队,降低呈现延迟(逐帧全量重绘不需要流水)。
                desired_maximum_frame_latency: 1,
                alpha_mode: caps.alpha_modes[0],
                view_formats: vec![],
            };
            surface.configure(&gpu.device, &config);

            let device = gpu.device.clone();
            let queue = gpu.queue.clone();
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("yu-host-android present"),
                source: wgpu::ShaderSource::Wgsl(include_str!("present.wgsl").into()),
            });
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("yu-host-android present bgl"),
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
                    // 等比缩放的参数(视口/窗口尺寸)。
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("yu-host-android present layout"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("yu-host-android present pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::REPLACE),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            });
            let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("yu-host-android present sampler"),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Nearest,
                ..Default::default()
            });

            // 4 个 f32 = 16 字节;内容在 present 时按实际尺寸写入。
            let params = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("yu-host-android present params"),
                size: 16,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });

            Ok(Self {
                surface,
                window,
                device,
                queue,
                config,
                pipeline,
                layout,
                sampler,
                bind_group: None,
                view: None,
                params,
            })
        }

        pub fn resize(&mut self, width: u32, height: u32) -> Result<(), PresentError> {
            if self.config.width == width && self.config.height == height {
                return Ok(());
            }
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
            Ok(())
        }

        pub fn present(
            &mut self,
            texture: &wgpu::Texture,
            id: u64,
            _width: u32,
            _height: u32,
            bound: &mut Option<(u64, u32, u32)>,
        ) -> Result<(), PresentError> {
            // 自愈:旋转/分屏/首帧布局完成前,Java 侧的 `nativeResizeSurface`
            // 可能按旧尺寸(甚至 1×1)配置了交换链。每帧核对原生窗口的实际
            // 尺寸,不一致就重配 —— 否则画面会按旧宽高比缩到屏幕一侧。
            let window = (
                ndk_width(self.window.0).max(1),
                ndk_height(self.window.0).max(1),
            );
            if window != (self.config.width, self.config.height) {
                self.resize(window.0, window.1)?;
            }
            // 引擎屏幕纹理的代际键变化时才重建视图/绑定组。
            if bound.map(|(bound_id, _, _)| bound_id) != Some(id) || self.bind_group.is_none() {
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                self.bind_group = Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("yu-host-android present bind group"),
                    layout: &self.layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&self.sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: self.params.as_entire_binding(),
                        },
                    ],
                }));
                self.view = Some(view);
                *bound = Some((id, _width, _height));
            }

            // 等比缩放参数:游戏视口 vs 窗口尺寸(letterbox 由 shader 算)。
            let params: [f32; 4] = [
                _width.max(1) as f32,
                _height.max(1) as f32,
                self.config.width.max(1) as f32,
                self.config.height.max(1) as f32,
            ];
            self.queue
                .write_buffer(&self.params, 0, bytemuck::cast_slice(&params));

            let frame = match self.surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(frame)
                | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
                wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                    self.surface.configure(&self.device, &self.config);
                    match self.surface.get_current_texture() {
                        wgpu::CurrentSurfaceTexture::Success(frame)
                        | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
                        other => return Err(PresentError::Acquire(format!("{other:?}"))),
                    }
                }
                other => return Err(PresentError::Acquire(format!("{other:?}"))),
            };

            let view = frame
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("yu-host-android present encoder"),
                });
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("yu-host-android present pass"),
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
                pass.set_pipeline(&self.pipeline);
                if let Some(bind_group) = &self.bind_group {
                    pass.set_bind_group(0, bind_group, &[]);
                    // 四边形两三角形(shader 内部按下标取顶点)。
                    pass.draw(0..6, 0..1);
                }
            }
            self.queue.submit(std::iter::once(encoder.finish()));
            // wgpu 30:提交后调用 `present` 把纹理还给交换链。
            self.queue.present(frame);
            Ok(())
        }
    }

    fn ndk_width(window: *mut ndk_sys::ANativeWindow) -> u32 {
        // SAFETY: 调用方保证 window 非空且有效。
        unsafe { ndk_sys::ANativeWindow_getWidth(window) }.max(0) as u32
    }

    fn ndk_height(window: *mut ndk_sys::ANativeWindow) -> u32 {
        // SAFETY: 同上。
        unsafe { ndk_sys::ANativeWindow_getHeight(window) }.max(0) as u32
    }
}

#[cfg(not(target_os = "android"))]
mod inner {
    //! 非 Android:保留类型以便宿主 crate 在开发机上编译/测试。

    use super::PresentError;
    use yu_backend_wgpu::GpuContext;

    pub struct Inner;

    impl Inner {
        pub fn new(
            _env: *mut jni::sys::JNIEnv,
            _surface: jni::sys::jobject,
            _gpu: &GpuContext,
        ) -> Result<Self, PresentError> {
            Err(PresentError::Unsupported)
        }

        pub fn resize(&mut self, _width: u32, _height: u32) -> Result<(), PresentError> {
            Err(PresentError::Unsupported)
        }

        pub fn present(
            &mut self,
            _texture: &wgpu::Texture,
            _id: u64,
            _width: u32,
            _height: u32,
            _bound: &mut Option<(u64, u32, u32)>,
        ) -> Result<(), PresentError> {
            Err(PresentError::Unsupported)
        }
    }
}
