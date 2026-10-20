//! wgpu instance/adapter/device 初始化。
//!
//! 本模块只做三件事:
//! 1. 按 [`DeviceOptions`] 选后端与适配器(可指定名称子串,供环境变量覆盖);
//! 2. 请求 device/queue;不开任何 experimental feature;
//! 3. 把适配器信息固化成可写进报告的快照。
//!
//! 目标格式固定 [`wgpu::TextureFormat::Rgba8Unorm`]:引擎在 gamma 编码的
//! 8-bit 存储值上混合(D3D9 默认,未设 `D3DRS_SRGBWRITEENABLE`),所以这里
//! **禁止**任何 sRGB 格式与隐式线性化。见 `docs/render-parity.md` §10/§12。

use yu_log::{Boundary, Diag};

/// 本 crate 新增的 `platform.gpu.*` 诊断码。
///
/// `yu-log` 当前只登记 `vfs.*`/`log.*`;GPU 码域按任务书要求在本 crate 内
/// 声明,不静默改 `yu-log`。README 的“错误码清单”逐条列出理由。
pub mod code {
    /// 没有满足条件的适配器(含名称过滤无匹配)。
    pub const ADAPTER_UNAVAILABLE: &str = "platform.gpu.adapter_unavailable";
    /// 适配器存在但 device 请求失败。
    pub const DEVICE: &str = "platform.gpu.device";
    /// 目标尺寸超过设备 `max_texture_dimension_2d`。
    pub const LIMIT: &str = "platform.gpu.limit";
    /// 目标纹理/管线创建被 wgpu 验证拒绝。
    pub const TARGET: &str = "platform.gpu.target";
    /// 读回缓冲映射失败。
    pub const READBACK: &str = "platform.gpu.readback";
}

/// 选卡与建设备参数。
#[derive(Clone, Debug)]
pub struct DeviceOptions {
    /// 启用的后端;`None` = wgpu 默认(`PRIMARY`,Windows 上 DX12/Vulkan)。
    pub backends: Option<wgpu::Backends>,
    /// 电源偏好(默认高性能独显)。
    pub power_preference: wgpu::PowerPreference,
    /// 适配器名称子串(ASCII 大小写不敏感);`None` = 交给 wgpu 选。
    pub adapter_name: Option<String>,
    /// 强制软件适配器(测试/无独显环境用)。
    pub force_fallback_adapter: bool,
    /// 请求的 limits;默认 [`wgpu::Limits::default`]。
    pub limits: wgpu::Limits,
}

impl Default for DeviceOptions {
    fn default() -> Self {
        Self {
            backends: None,
            power_preference: wgpu::PowerPreference::HighPerformance,
            adapter_name: None,
            force_fallback_adapter: false,
            limits: wgpu::Limits::default(),
        }
    }
}

/// 写进渲染报告的适配器快照(稳定字符串,不含指针/时间)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdapterSummary {
    pub name: String,
    pub backend: String,
    pub device_type: String,
    pub driver: String,
    pub driver_info: String,
    pub vendor: u32,
    pub device: u32,
}

impl AdapterSummary {
    fn from_info(info: &wgpu::AdapterInfo) -> Self {
        Self {
            name: info.name.clone(),
            backend: backend_name(info.backend).to_string(),
            device_type: device_type_name(info.device_type).to_string(),
            driver: info.driver.clone(),
            driver_info: info.driver_info.clone(),
            vendor: info.vendor,
            device: info.device,
        }
    }
}

/// 已初始化的 GPU 上下文。
pub struct GpuContext {
    /// Instance 必须比由它创建的 adapter/device 活得久。
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub adapter_info: AdapterSummary,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    /// 所选后端对应的原始枚举(报告/测试用)。
    pub backend: wgpu::Backend,
}

impl GpuContext {
    /// 按 `options` 建 instance → adapter → device。
    ///
    /// 适配器名称过滤在 `enumerate_adapters` 上做;没有匹配时返回
    /// [`code::ADAPTER_UNAVAILABLE`],调用方应把它当作“显式 SKIP”而不是
    /// 崩溃或空跑。
    pub fn new(options: &DeviceOptions) -> Result<Self, Diag> {
        let backends = options.backends.unwrap_or(wgpu::Backends::PRIMARY);
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });

        let adapter = if let Some(wanted) = options.adapter_name.as_deref() {
            select_adapter_by_name(&instance, backends, wanted)?
        } else {
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: options.power_preference,
                force_fallback_adapter: options.force_fallback_adapter,
                compatible_surface: None,
                apply_limit_buckets: false,
            }))
            .map_err(|err| {
                Diag::fatal(code::ADAPTER_UNAVAILABLE, Boundary::Platform).with_detail(format!(
                    "request_adapter 失败(backends={}): {err}",
                    backends_name(backends)
                ))
            })?
        };

        let info = adapter.get_info();
        let adapter_info = AdapterSummary::from_info(&info);

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("yu-backend-wgpu device"),
            required_features: wgpu::Features::empty(),
            required_limits: options.limits.clone(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))
        .map_err(|err| {
            Diag::fatal(code::DEVICE, Boundary::Platform).with_detail(format!(
                "request_device 失败(adapter={} backend={}): {err}",
                info.name,
                backend_name(info.backend)
            ))
        })?;

        Ok(Self {
            instance,
            adapter,
            adapter_info,
            device,
            queue,
            backend: info.backend,
        })
    }

    /// `max_texture_dimension_2d`;所有离屏/屏幕目标都要先过这个门。
    pub fn max_texture_dimension_2d(&self) -> u32 {
        self.device.limits().max_texture_dimension_2d
    }

    /// 校验目标尺寸;超限返回 [`code::LIMIT`] 而不是让 wgpu panic。
    pub fn check_dimensions(&self, width: u32, height: u32, what: &str) -> Result<(), Diag> {
        if width == 0 || height == 0 {
            return Err(Diag::fatal(code::TARGET, Boundary::Platform)
                .with_detail(format!("{what} 尺寸为 {width}x{height},无法创建 GPU 目标")));
        }
        let max = self.max_texture_dimension_2d();
        if width > max || height > max {
            return Err(
                Diag::fatal(code::LIMIT, Boundary::Platform).with_detail(format!(
                    "{what} 尺寸 {width}x{height} 超过 max_texture_dimension_2d={max}"
                )),
            );
        }
        Ok(())
    }

    /// 在 wgpu 验证作用域里执行 `f`;验证错误转成 [`Diag`]。
    pub fn checked<T>(
        &self,
        code: &'static str,
        what: &str,
        f: impl FnOnce() -> T,
    ) -> Result<T, Diag> {
        let guard = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let value = f();
        match pollster::block_on(guard.pop()) {
            Some(err) => Err(Diag::fatal(code, Boundary::Platform)
                .with_detail(format!("{what} 被 wgpu 拒绝: {err}"))),
            None => Ok(value),
        }
    }
}

fn select_adapter_by_name(
    instance: &wgpu::Instance,
    backends: wgpu::Backends,
    wanted: &str,
) -> Result<wgpu::Adapter, Diag> {
    let wanted_lower = wanted.to_ascii_lowercase();
    let adapters = pollster::block_on(instance.enumerate_adapters(backends));
    let mut available = Vec::with_capacity(adapters.len());
    for adapter in adapters {
        let info = adapter.get_info();
        let display = format!("{} [{}]", info.name, backend_name(info.backend));
        if info.name.to_ascii_lowercase().contains(&wanted_lower) {
            return Ok(adapter);
        }
        available.push(display);
    }
    available.sort();
    Err(
        Diag::fatal(code::ADAPTER_UNAVAILABLE, Boundary::Platform).with_detail(format!(
            "没有名称包含 {wanted:?} 的适配器;可用: {}",
            if available.is_empty() {
                "(无)".to_string()
            } else {
                available.join(", ")
            }
        )),
    )
}

/// 稳定后端名(报告/环境变量共用)。
pub fn backend_name(backend: wgpu::Backend) -> &'static str {
    match backend {
        wgpu::Backend::Noop => "noop",
        wgpu::Backend::Vulkan => "vulkan",
        wgpu::Backend::Metal => "metal",
        wgpu::Backend::Dx12 => "dx12",
        wgpu::Backend::Gl => "gl",
        wgpu::Backend::BrowserWebGpu => "browser_webgpu",
    }
}

fn device_type_name(device_type: wgpu::DeviceType) -> &'static str {
    match device_type {
        wgpu::DeviceType::Other => "other",
        wgpu::DeviceType::IntegratedGpu => "integrated_gpu",
        wgpu::DeviceType::DiscreteGpu => "discrete_gpu",
        wgpu::DeviceType::VirtualGpu => "virtual_gpu",
        wgpu::DeviceType::Cpu => "cpu",
    }
}

fn backends_name(backends: wgpu::Backends) -> &'static str {
    if backends == wgpu::Backends::PRIMARY {
        "primary"
    } else if backends == wgpu::Backends::all() {
        "all"
    } else if backends == wgpu::Backends::DX12 {
        "dx12"
    } else if backends == wgpu::Backends::VULKAN {
        "vulkan"
    } else if backends == wgpu::Backends::GL {
        "gl"
    } else {
        "custom"
    }
}
