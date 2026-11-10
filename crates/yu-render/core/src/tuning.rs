//! 渲染/诊断运行参数(跨平台统一入口)。
//!
//! 这些参数以前散落在 `YURIS_*` 环境变量里,由渲染层在构造/解码时直接读:
//! 桌面只能靠 `set VAR=...` 开启,Android 则在宿主里 `set_var` 转一手。
//! 现在统一成 [`RenderTuning`] —— 与 `backend`/`gpu_backend` 一样是引擎参数,
//! 由宿主(CLI 旗标 / Android `RunConfig`)解析后经 `EngineOptions.tuning`
//! 传下来;渲染层只读字段,不再读环境变量。
//!
//! 默认值 = 原实现的缺省(见各字段注释);`RenderTuning::default()` 不依赖
//! 进程环境,同一份配置在任何平台/任何线程都得到同一结果。

/// PNG 解码后端(仅 Android 编译 libpng;其它平台忽略)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PngBackend {
    /// `image` crate(stb;x86 上更快,跨平台默认)。
    Image,
    /// 官方 libpng(arm64 NEON 反滤波;Android 可选)。
    Libpng,
}

impl PngBackend {
    /// 取值解析(大小写不敏感;唯一词表,宿主不再各自 match)。
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "image" | "stb" => Ok(Self::Image),
            "libpng" => Ok(Self::Libpng),
            other => Err(format!(
                "png_backend 只接受 image|libpng,收到 `{other}`"
            )),
        }
    }

    /// 规范名(诊断/配置回写用)。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Libpng => "libpng",
        }
    }
}

impl Default for PngBackend {
    fn default() -> Self {
        Self::Image
    }
}

/// 渲染/诊断运行参数。
///
/// 字段分两组:
/// - **性能/资源**:预取、缓存预算、文本 GPU 等,影响时序不影响像素;
/// - **诊断**:timing/profiling 开关,只在排查时打开。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderTuning {
    // ---- 性能 / 资源 ----
    /// 资产异步预取(`CG[FILE]` 声明即交后台解码)。默认关。
    pub prefetch: bool,
    /// 预取池预算(MiB,默认 128)。
    pub prefetch_mb: usize,
    /// 预取前瞻每脚本最大组数(默认 2048;只在 `prefetch` 开启时查询)。
    pub prefetch_window: usize,
    /// 资产缓存像素预算(MiB,默认 512;0 = 不限制)。
    pub asset_cache_mb: usize,
    /// 编码不一致时的唯一同名回退(默认开)。
    pub asset_name_fallback: bool,
    /// 手机画质:`SIZE` 也按双线性采样(默认关 = 官方最近邻)。
    pub bilinear_scale: bool,
    /// COPY2 快照延迟物化(默认关)。
    pub snapshot_defer: bool,
    /// 文本基面 CPU 缓存(默认开)。
    pub text_base_cache: bool,
    /// GPU 文本 pass(默认开;能力不足时后端自行回退 CPU)。
    pub text_gpu: bool,
    /// 启动期管线预热(默认开;关掉用于对比启动耗时)。
    pub pipeline_prewarm: bool,
    /// PNG 解码后端(默认 `image`;Android 可切 `libpng`)。
    pub png_backend: PngBackend,

    // ---- 诊断 ----
    /// 资产装载/预取计时。默认关。
    pub asset_timing: bool,
    /// tick 内部分段计时(慢帧拆项)。默认关。
    pub tick_split: bool,
    /// CG 差分计时。默认关。
    pub cg_timing: bool,
    /// GPU 读回原因。默认关。
    pub readback_debug: bool,
    /// 过渡合成取证。默认关。
    pub trans_debug: bool,
    /// GPU 文本失败/回退原因。默认关。
    pub text_gpu_debug: bool,
    /// 渲染阶段 timing。默认关。
    pub render_timing: bool,
    /// 渲染几何 trace。默认关。
    pub render_trace_geometry: bool,
    /// 文本 P8 阶段 timing。默认关。
    pub p8_timing: bool,
    /// 字体栅格 trace。默认关。
    pub font_trace: bool,
    /// PNG 双后端逐张对拍(libpng vs image)。默认关。
    pub png_cmp: bool,
    /// VM opcode 逐条耗时归因。默认关。
    pub vm_profile: bool,
}

impl Default for RenderTuning {
    fn default() -> Self {
        Self {
            prefetch: false,
            prefetch_mb: 128,
            prefetch_window: 2048,
            asset_cache_mb: 512,
            asset_name_fallback: true,
            bilinear_scale: false,
            snapshot_defer: false,
            text_base_cache: true,
            text_gpu: true,
            pipeline_prewarm: true,
            png_backend: PngBackend::Image,
            asset_timing: false,
            tick_split: false,
            cg_timing: false,
            readback_debug: false,
            trans_debug: false,
            text_gpu_debug: false,
            render_timing: false,
            render_trace_geometry: false,
            p8_timing: false,
            font_trace: false,
            png_cmp: false,
            vm_profile: false,
        }
    }
}
