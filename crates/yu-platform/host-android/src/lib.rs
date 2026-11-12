//! `yu-host-android` · Android 宿主(NDK cdylib + JNI 入口)。
//!
//! 与桌面/浏览器宿主**共用同一份 `yu-engine` 装配**(架构 v2 §2.5):平台差异
//! 只在①端口装配与④出口出现。
//!
//! ```text
//! Java/Kotlin 侧                         本 crate(so)
//!   nativeInit(dataRoot, saveRoot, entry)  → Project::discover + Engine::new
//!   nativeTick()                           → Engine::tick_realtime()
//!   nativeFrameSize()                      → [w, h]
//!   nativeCopyFrame(int[])                 → RGBA → ARGB_8888 像素
//!   nativeSetInput(button, pressed)        → Engine::set_input
//!   nativeSetCursor(x, y)                  → Engine::set_cursor
//!   nativeStatus()                         → 诊断摘要(浮层显示)
//!   nativeVersion()                        → 构建目标标记
//!   nativeShutdown()                       → drop Engine
//! ```
//!
//! # 为什么用 soft 后端
//!
//! 首个 Android 目标要验证的是**跨平台能力等价**(数据读取、存档写入、字体
//! 枚举、脚本执行),不是 GPU 像素性能。soft 后端是确定性参考实现、零 GPU
//! 依赖,帧以 RGBA 交给 Java 侧画到 Bitmap —— 这条链路能把内核整条跑通,
//! 且与 native `--backend soft` 出**同构**的帧。wgpu(Surface/Vulkan)是后续
//! 增强路径,不阻塞本阶段的验证目标。
//!
//! # 线程模型
//!
//! `Engine` 内部是 `Rc`/`RefCell`(单线程),因此所有 `native*` 调用**必须在
//! 同一个线程**上发生(Android 侧用一个专用渲染线程)。状态放在
//! `thread_local`,跨线程调用会拿到「未初始化」而不是未定义行为。
//!
//! # 数据与存档
//!
//! Android 有真实文件系统,因此**不引入 AAsset 内存映射**:
//! `dataRoot` 是游戏目录(app 私有目录内 / 外置存储皆可),`saveRoot` 是可写
//! 存档目录。二者语义与桌面完全一致(A1/A2),存档字节也逐字节一致。

// JNI 导出必须是 C ABI 符号,`#[no_mangle]` 属于 unsafe 范畴;unsafe 只允许
// 出现在 `jni_entry` 模块里(与 `yu-sound::acm` 同一纪律)。
#![deny(unsafe_code)]
#![allow(clippy::result_large_err)]

// 全局分配器:只替换本 so 内 Rust 侧的 malloc(mimalloc),不触碰 JVM/Java 堆。
// 依赖与 `local_dynamic_tls` 见 Cargo.toml 的 android target 段。
#[cfg(target_os = "android")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

pub mod logging;
pub mod present;
pub mod sound;

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::Arc;

use yu_backend_wgpu::GpuBackend;
use yu_engine::engine::{Backend, BlockCodec, Engine, EngineOptions};
use yu_engine::project::Project;
use yu_log::LogLevel;

/// 一个运行会话(初始化成功到 shutdown)。
struct Session {
    engine: Engine,
    /// 最近一帧的尺寸与像素(Java 侧按需拉取;不跨界共享引用)。
    frame_w: u32,
    frame_h: u32,
    /// 最近一帧的呈现面(Arc 共享;CPU 回退路径从这里取 RGBA,#71)。
    frame_pixels: Option<Arc<yu_render_core::Surface>>,
    ticks: u64,
    /// 已写进 logcat 的长帧条数(上限,避免慢场景把日志刷爆)。
    spikes_logged: u32,
    /// 零拷贝上屏器。`Some` = 已绑定原生窗口,`tick` 时直接上屏(不读回
    /// CPU);`None` = 走 `nativeCopyFrame` 的旧路径(CPU 后端必须如此)。
    presenter: Option<present::NativePresenter>,
    /// 建上屏器要用的 GPU 上下文(GPU 后端才 `Some`;与引擎共享同一 device)。
    gpu: Option<yu_backend_wgpu::GpuContext>,
    /// 音频输出设备(持有 stream 即出声;drop 即停)。
    audio: Option<sound::SoundSink>,
    /// 上屏失败计数(诊断;nativeStatus 可见)。
    present_failures: u64,
    /// 逐 opcode 耗时采样是否已开启(`tuning.vm_profile`)。
    ///
    /// 真机实测 tick p50=7.7ms、每帧约 1.5 万条命令 ⇒ **每条命令 380ns**,
    /// 而 PC 上同一份代码只要 88ns。这个 4.3 倍差距必须按 opcode 拆开看:
    /// 是分派慢(`LET`/`IF`),还是图形命令慢(`CG`/`CGEND`),修法完全不同。
    vm_profile: bool,
    /// 上次 reset 到现在的帧数(每 60 帧 dump 一次)。
    vm_profile_frames: u32,
    /// 是否渲染上屏(归因实验开关)。
    video: bool,
    /// 发布模式(状态串最小化)。
    quiet: bool,
    /// 音频抓取:已启动时的目标样本数(诊断爆音)。
    audio_capture_target: usize,
    /// 抓取结果的落盘路径(数据目录下 debug/audio-capture.wav)。
    capture_path: Option<PathBuf>,
}

thread_local! {
    /// 单线程会话槽(见模块文档「线程模型」)。
    ///
    /// 初始化器**已经**是 `const {}`;`missing_const_for_thread_local` 在多条目
    /// 宏里对已 const 的项仍会报一次(clippy 误报),逐项关掉。
    #[allow(clippy::missing_const_for_thread_local)]
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
    /// 最近一次失败原因(供 `nativeStatus` 报告;成功初始化时清空)。
    #[allow(clippy::missing_const_for_thread_local)]
    static LAST_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
    /// `nativeCopyFrame` 的转换缓冲(容量跨帧复用,稳态零分配)。
    ///
    /// 只在 CPU 回退路径上用到;零拷贝上屏时壳不读像素,这里一直空着。
    #[allow(clippy::missing_const_for_thread_local)]
    static ARGB_SCRATCH: RefCell<Vec<i32>> = const { RefCell::new(Vec::new()) };
}

/// 挂起请求(跨线程)。
///
/// `Engine` 是 `Rc`/`RefCell` 单线程状态,只能在持有它的线程(渲染线程)上
/// 调 `suspend()`。而 Android 的 `onPause`/`onResume` 跑在 **UI 线程**:
/// 直接 `SESSION.with(..)` 会拿到该线程的**空槽** ⇒ 永远返回 -1、引擎根本
/// 没被挂起。这里只置原子请求,由渲染线程在下一帧 tick 前自己执行
/// (见 `apply_pending_lifecycle`)。
static SUSPEND_REQUEST: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static RESUME_REQUEST: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// 上屏尺寸变更请求(跨线程)。
///
/// 与挂起同因:`onSurfaceTextureSizeChanged` 在 **UI 线程**上跑,而
/// presenter 属于渲染线程的会话。旧的实现直接在调用线程 `SESSION.with(..)`
/// ⇒ 拿到空槽、返回 -1,**这次尺寸变更被静默丢弃**(旋转/分屏后交换链
/// 停在旧尺寸)。这里只记尺寸,由渲染线程执行。
///
/// 上屏尺寸变更请求(跨线程)。
///
/// 与挂起同因:`onSurfaceTextureSizeChanged` 在 **UI 线程**上跑,而
/// presenter 属于渲染线程的会话。旧的实现直接在调用线程 `SESSION.with(..)`
/// ⇒ 拿到空槽、返回 -1,**这次尺寸变更被静默丢弃**(旋转/分屏后交换链
/// 停在旧尺寸)。这里只记尺寸,由渲染线程执行。
///
/// 打包成一个 `u64`(宽在高 32 位、高在低 32 位)而不是两个原子量:
/// 两次独立写入之间被消费会读到**半新半旧**的一对,交换链尺寸就错了。
/// `0` = 无请求(调用方一律 `max(1, ..)`,宽不可能是 0)。
static RESIZE_REQUEST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 会话销毁请求(跨线程;见 [`request_shutdown`])。
static SHUTDOWN_REQUEST: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// `tick` 的「宿主请求销毁」返回码。
///
/// 与引擎的生命周期码(0 running / 1 finished / 2 exited / 3 exited+restart)
/// 共用同一个 `int` 出口,取一个它们不用的值。壳收到它就**静默退出线程**,
/// 不能当成「游戏结束」去 `finish()`(那会把正在销毁的 Activity 重入)。
pub const TICK_HOST_SHUTDOWN: i32 = 4;

fn set_error(message: impl Into<String>) {
    LAST_ERROR.with(|slot| *slot.borrow_mut() = Some(message.into()));
}

fn clear_error() {
    LAST_ERROR.with(|slot| *slot.borrow_mut() = None);
}

fn last_error() -> Option<String> {
    LAST_ERROR.with(|slot| slot.borrow().clone())
}

// ------------------------------------------------------------ 会话管理(纯 Rust)

/// 读取 `dataRoot/fonts/` 下的字体字节(宿主字体来源;空/缺目录 = 不注入)。
///
/// 只认字体后缀,按文件名排序保证跨设备确定性(同 family 后载者覆盖先载者,
/// 与桌面「系统枚举 + 附加字节」的合并顺序同口径)。
fn load_host_fonts(data_root: &std::path::Path) -> Vec<Vec<u8>> {
    let dir = data_root.join("fonts");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| {
                    matches!(
                        ext.to_ascii_lowercase().as_str(),
                        "ttf" | "otf" | "ttc" | "otc"
                    )
                })
                .unwrap_or(false)
        })
        .collect();
    paths.sort();
    paths
        .into_iter()
        .filter_map(|path| std::fs::read(&path).ok())
        .collect()
}

/// 枚举本机可用的字体族(去重、排序;供 Android 侧做选择下拉)。
///
/// Android 的系统字体经 [`yu_font::FontSource`] 的系统目录扫描得到(见
/// DIV-AND-001);桌面同函数枚举系统字体。失败时返回空表 —— 调用方据此
/// 只提供「导入字体文件」这一条路。
pub fn list_font_families() -> Vec<String> {
    let mut families: Vec<String> = match yu_font::FontSource::load_with_extra_bytes(&[]) {
        Ok(source) => source.family_names(),
        Err(_) => Vec::new(),
    };
    families.sort_by_key(|name| name.to_lowercase());
    families.dedup();
    families
}

/// 初始化会话。返回给 Java 的摘要(`OK ...` / `ERR: ...`)。
pub fn init_session(
    data_root: PathBuf,
    save_root: PathBuf,
    entry: String,
) -> Result<String, String> {
    init_session_with_fonts(data_root, save_root, entry, Vec::new(), None)
}

/// 运行配置(与 `yu-cli` 的旗标一一对应;JSON 键名见各字段文档)。
///
/// 之所以做成「一个 JSON」而不是几十个 JNI 参数:壳(不同模拟器 app)只需
/// 拼一组键值,**新增能力不必改 JNI 签名**,老壳也不会因为缺键而失败。
/// 未给的键取 [`RunConfig::default`](与 CLI 缺省一致)。
#[derive(Clone, Debug, PartialEq)]
pub struct RunConfig {
    // ---- 数据与存档 ----
    /// 游戏根目录(含 `data.ypf` 或 `data/`)。
    pub data_root: PathBuf,
    /// 存档根。缺省 = `data_root/save`。
    pub save_root: PathBuf,
    // ---- 脚本 ----
    /// 入口标签,缺省 `SYSTEM_START`(与 CLI 一致)。
    pub entry: String,
    /// 调试模式:`@_DEBUGMODE=1`(DIV-L-001)。缺省 false。
    pub debug_mode: bool,
    /// 显式脚本密钥(8 位十六进制)。缺省 = 启动时自动解析。
    pub script_key: Option<[u8; 4]>,
    /// 强制字体 family(等价 CLI `--font`)。
    pub force_font: Option<String>,
    /// 宿主附加字体字节(用户导入的字体文件内容)。
    pub font_bytes: Vec<Vec<u8>>,
    /// 用户导入的字体文件路径(JSON 键 `font_file`;读不到显式报错)。
    pub font_file: Option<PathBuf>,
    /// 文本编码路线(JSON 键 `patch`:`auto|original|gbk|tunnel`)。
    ///
    /// 与 CLI `--patch` 同义;`auto` 才会读产物签名。
    pub patch: yu_engine::patch::PatchKind,
    // ---- 声音 ----
    /// `@_OS_SOUND` 注入值。缺省 true(对拍口径)。
    pub os_sound: bool,
    /// 是否真的接音频输出端口。缺省 true。`false` 只静音,不影响 `@_OS_SOUND`。
    pub audio_output: bool,
    // ---- 视频 ----
    /// 渲染后端(缺省 soft)。取值解析/文案由 [`Backend::parse`] 唯一提供。
    ///
    /// GPU 是增强路径:设备不支持 Vulkan/GLES 时**显式失败**,不自动降级
    /// (自动降级会让"GPU 模式画错"永远对不上号)。
    pub backend: Backend,
    /// GPU 底层接口(缺省 [`GpuBackend::Auto`] = 先 GLES 再 Vulkan)。
    ///
    /// Android 模拟器的 gfxstream Vulkan 在 `vkQueueSubmit` 里空指针崩溃
    /// (实测 `vulkan.ranchu.so` SIGSEGV),所以 `auto` 先试 GLES,失败再试
    /// Vulkan。真机上两者都能用时也优先 GLES —— 对本引擎(纯 2D 图元 +
    /// 文本纹理)GLES 的驱动兼容性更好。
    pub gpu_backend: GpuBackend,
    /// 视口像素格式代号(`@_SCREEN_COLOR`)。缺省 32。
    pub screen_color: u32,
    /// 渲染模式代号(`@_DX_MODE`)。缺省 0。
    pub dx_mode: u32,
    /// 每秒逻辑帧数。缺省 60。
    pub fps: u32,
    // ---- 存档 ---
    /// 存档块压缩层。缺省 `None` = 自动判定。
    pub save_codec: Option<BlockCodec>,
    /// 媒体只读(`@_MEDIA_READONLY`)。缺省 false。
    pub media_readonly: bool,
    // ---- 诊断 ----
    /// 记录事件流(占内存;对拍用)。缺省 false。
    pub record_events: bool,
    /// 调试日志文件(JSONL)。非空即开启;路径相对 `data_root` 时落游戏目录,
    /// 便于 adb pull 取回分析。缺省 `None` = 不写文件。
    pub log_file: Option<PathBuf>,
    /// 日志门槛(缺省 [`LogLevel::Info`])。取值解析只有 `yu_log::LogLevel`
    /// 一份 —— 与 CLI `--log-level` 完全同一组,不再有「CLI 收 debug/trace
    /// 而 Android 拒收」的差异。
    pub log_level: LogLevel,
    /// 抓取混音输出到 `<data_root>/debug/audio-capture.wav`(诊断爆音)。
    /// 值 = 抓取多少毫秒(0 = 不抓)。
    pub audio_capture_ms: u32,
    /// 纯音自测:旁路引擎输出,直接给设备灌 1kHz 正弦(诊断爆音)。
    pub audio_tone: bool,
    /// 强制输出采样率(设备支持才生效;`None` = 用设备默认)。
    pub audio_rate: Option<u32>,
    /// 是否渲染上屏(默认 true)。`false` = 只跑 VM + 声音,用于归因实验:
    /// 把合成/上屏整条链路从时序里摘掉,单独判断卡顿是否由渲染抢 CPU 引起。
    pub render: bool,
    /// 渲染/诊断运行参数(与桌面 `EngineOptions.tuning` 同一份结构)。
    ///
    /// JSON 键与 CLI 旗标同名:`prefetch` `prefetch_mb` `asset_cache_mb`
    /// `text_gpu` `png_backend` `vm_profile` `asset_timing` … 完整列表见
    /// [`yu_render_core::RenderTuning`] 字段;不再经宿主 setenv 转手。
    pub tuning: yu_render_core::RenderTuning,
    /// **发布模式**(默认 true):关掉一切只为排查存在的开销。
    ///
    /// - logcat 只放 `warn` 以上(不再每帧/每秒写 info);
    /// - 不启用混音抓取(WAV 落盘 = 1MB 内存 + 写 /sdcard);
    /// - `nativeStatus()` 只算最小字段(不再每秒拼整条状态串、取诊断、读阶段计数)。
    ///
    /// 诊断时置 `false`(或 `log_level=verbose`,会自动置 false)。
    pub quiet: bool,
}

impl Default for RunConfig {
    fn default() -> Self {
        Self {
            data_root: PathBuf::new(),
            save_root: PathBuf::new(),
            entry: "SYSTEM_START".to_string(),
            debug_mode: false,
            script_key: None,
            force_font: None,
            font_bytes: Vec::new(),
            patch: yu_engine::patch::PatchKind::Auto,
            font_file: None,
            os_sound: true,
            audio_output: true,
            screen_color: 32,
            dx_mode: 0,
            fps: 60,
            save_codec: None,
            media_readonly: false,
            record_events: false,
            backend: Backend::Soft,
            gpu_backend: GpuBackend::Auto,
            log_file: None,
            log_level: LogLevel::Info,
            audio_capture_ms: 0,
            audio_tone: false,
            audio_rate: None,
            render: true,
            tuning: yu_render_core::RenderTuning::default(),
            quiet: true,
        }
    }
}

impl RunConfig {
    /// 从 JSON 文本解析;缺省键保持 [`Default`]。
    ///
    /// 支持的键(全部可选):
    /// `data_root` `save_root` `entry` `debug_mode` `script_key` `font`
    /// `font_file` `os_sound` `audio` `backend` `fps` `screen_color` `dx_mode`
    /// `save_codec` `media_readonly` `record_events` `log_file` `log_level`
    /// `prefetch` `prefetch_mb` `prefetch_window` `asset_cache_mb`
    /// `asset_name_fallback` `bilinear_scale` `snapshot_defer`
    /// `text_base_cache` `text_gpu` `pipeline_prewarm` `png_backend`
    /// `vm_profile` `asset_timing` `tick_split` `cg_timing` `readback_debug`
    /// `trans_debug` `text_gpu_debug` `render_timing` `render_trace_geometry`
    /// `p8_timing` `font_trace` `png_cmp`
    pub fn from_json(text: &str) -> Result<Self, String> {
        let mut config = Self::default();
        config.apply_json(text)?;
        Ok(config)
    }

    /// 把一段 JSON 铺到已有配置上(未出现的键保持原值)。
    ///
    /// 宿主用它做 `<训练目录>/debug/run-config.json` 覆盖:调壳不方便时
    /// 直接改文件即可打开诊断开关(纯音、关渲染、指定采样率等)。
    pub fn apply_json(&mut self, text: &str) -> Result<(), String> {
        let value = yu_engine::json::parse(text)?;
        let Some(object) = value.as_object() else {
            return Err("配置必须是 JSON 对象".to_string());
        };
        let config = self;
        // `data_root` / `save_root` 是路径键(不是字符串开关),必须先解析,
        // 否则 `start_session_json` 的必填校验永远看到空路径。
        if let Some(v) = object.get("data_root").and_then(|v| v.as_str()) {
            if !v.trim().is_empty() {
                config.data_root = PathBuf::from(v);
            }
        }
        if let Some(v) = object.get("save_root").and_then(|v| v.as_str()) {
            if !v.trim().is_empty() {
                config.save_root = PathBuf::from(v);
            }
        }
        if let Some(v) = object.get("entry").and_then(|v| v.as_str()) {
            if !v.trim().is_empty() {
                config.entry = v.to_string();
            }
        }
        if let Some(v) = object.get("debug_mode").and_then(|v| v.as_bool()) {
            config.debug_mode = v;
        }
        if let Some(v) = object.get("script_key").and_then(|v| v.as_str()) {
            config.script_key = Some(parse_script_key(v)?);
        }
        if let Some(v) = object.get("font").and_then(|v| v.as_str()) {
            if !v.trim().is_empty() {
                config.force_font = Some(v.to_string());
            }
        }
        if let Some(v) = object.get("font_file").and_then(|v| v.as_str()) {
            if !v.trim().is_empty() {
                config.font_file = Some(PathBuf::from(v));
            }
        }
        if let Some(v) = object.get("patch").and_then(|v| v.as_str()) {
            config.patch = yu_engine::patch::PatchKind::parse(v)?;
        }
        if let Some(v) = object.get("os_sound").and_then(|v| v.as_bool()) {
            config.os_sound = v;
        }
        if let Some(v) = object.get("audio").and_then(|v| v.as_bool()) {
            config.audio_output = v;
        }
        if let Some(v) = object.get("backend").and_then(|v| v.as_str()) {
            if !v.trim().is_empty() {
                config.backend = Backend::parse(v).map_err(|error| error.message)?;
            }
        }
        if let Some(v) = object.get("gpu_backend").and_then(|v| v.as_str()) {
            if !v.trim().is_empty() {
                config.gpu_backend = GpuBackend::parse(v)?;
            }
        }
        if let Some(v) = object.get("audio_capture_ms").and_then(|v| v.as_u64()) {
            config.audio_capture_ms = v.min(u64::from(u32::MAX)) as u32;
        }
        // 纯音自测:旁路引擎,直接给设备灌 1kHz 正弦。用来把「引擎/混音」与
        // 「环 → oboe → 系统」两段彻底分开(爆音定位的决定性手段)。
        if let Some(v) = object.get("audio_tone").and_then(|v| v.as_bool()) {
            config.audio_tone = v;
        }
        if let Some(v) = object.get("render").and_then(|v| v.as_bool()) {
            config.render = v;
        }
        apply_tuning_json(&mut config.tuning, object)?;
        let quiet_explicit = object.get("quiet").and_then(|v| v.as_bool());
        if let Some(v) = object.get("audio_rate").and_then(|v| v.as_u64()) {
            if v > 0 {
                config.audio_rate = Some(v.min(u64::from(u32::MAX)) as u32);
            }
        }
        if let Some(v) = object.get("log_file").and_then(|v| v.as_str()) {
            if !v.trim().is_empty() {
                config.log_file = Some(PathBuf::from(v));
            }
        }
        if let Some(v) = object.get("log_level").and_then(|v| v.as_str()) {
            // 取值表与 CLI 完全同一份(含 debug/trace);空串 = 不改。
            if !v.trim().is_empty() {
                config.log_level = LogLevel::parse(v)?;
            }
        }
        if let Some(v) = object.get("fps").and_then(|v| v.as_u64()) {
            if v > 0 {
                config.fps = v.min(u64::from(u32::MAX)) as u32;
            }
        }
        if let Some(v) = object.get("screen_color").and_then(|v| v.as_u64()) {
            config.screen_color = v.min(u64::from(u32::MAX)) as u32;
        }
        if let Some(v) = object.get("dx_mode").and_then(|v| v.as_u64()) {
            config.dx_mode = v.min(u64::from(u32::MAX)) as u32;
        }
        if let Some(v) = object.get("save_codec").and_then(|v| v.as_str()) {
            if !v.trim().is_empty() {
                config.save_codec = Some(BlockCodec::parse(v)?);
            }
        }
        if let Some(v) = object.get("media_readonly").and_then(|v| v.as_bool()) {
            config.media_readonly = v;
        }
        if let Some(v) = object.get("record_events").and_then(|v| v.as_bool()) {
            config.record_events = v;
        }
        // 显式 `quiet` 优先;否则只在本次真的给了 `log_level` 时才推断
        // —— `apply_json` 会被调用两次(壳的 JSON + 调试覆盖文件),
        // 无条件按 `log_level` 重算会把上一次的显式设置冲掉。
        if let Some(quiet) = quiet_explicit {
            config.quiet = quiet;
        } else if object.get("log_level").is_some() {
            // `log_level=verbose`(壳打开「调试日志」时设置)= 明确要诊断,
            // 自动退出发布模式。`debug`/`trace` 同理(它们比缺省更松)。
            config.quiet = !matches!(
                config.log_level,
                LogLevel::Debug | LogLevel::Trace | LogLevel::Verbose
            );
        }
        Ok(())
    }
}

/// `--script-key` 同口径(解析本体只有 `yu_engine::keys::ScriptKey::parse_hex`)。
fn parse_script_key(value: &str) -> Result<[u8; 4], String> {
    yu_engine::keys::ScriptKey::parse_hex(value)
}

/// 把 JSON 里的运行参数铺到 [`RenderTuning`](yu_render_core::RenderTuning)。
///
/// 键名与桌面 CLI 旗标一一对应(见 `docs/cross-platform.md` 的宿主参数表);
/// 取值类型错(例如 `prefetch: "yes"`)直接报错,不静默忽略。
fn apply_tuning_json(
    tuning: &mut yu_render_core::RenderTuning,
    object: &std::collections::BTreeMap<String, yu_engine::json::Value>,
) -> Result<(), String> {
    let flag = |key: &str, dst: &mut bool| {
        if let Some(value) = object.get(key) {
            let on = value
                .as_bool()
                .ok_or_else(|| format!("{key} 需要 true/false"))?;
            *dst = on;
        }
        Ok::<(), String>(())
    };
    let number = |key: &str, dst: &mut usize| {
        if let Some(value) = object.get(key) {
            let n = value
                .as_u64()
                .ok_or_else(|| format!("{key} 需要非负整数"))?;
            *dst = n as usize;
        }
        Ok::<(), String>(())
    };
    flag("prefetch", &mut tuning.prefetch)?;
    number("prefetch_mb", &mut tuning.prefetch_mb)?;
    number("prefetch_window", &mut tuning.prefetch_window)?;
    number("asset_cache_mb", &mut tuning.asset_cache_mb)?;
    flag("asset_name_fallback", &mut tuning.asset_name_fallback)?;
    flag("bilinear_scale", &mut tuning.bilinear_scale)?;
    flag("snapshot_defer", &mut tuning.snapshot_defer)?;
    flag("text_base_cache", &mut tuning.text_base_cache)?;
    flag("text_gpu", &mut tuning.text_gpu)?;
    flag("pipeline_prewarm", &mut tuning.pipeline_prewarm)?;
    if let Some(value) = object.get("png_backend") {
        let text = value
            .as_str()
            .ok_or_else(|| "png_backend 需要字符串".to_string())?;
        tuning.png_backend = yu_render_core::PngBackend::parse(text)?;
    }
    flag("asset_timing", &mut tuning.asset_timing)?;
    flag("tick_split", &mut tuning.tick_split)?;
    flag("cg_timing", &mut tuning.cg_timing)?;
    flag("readback_debug", &mut tuning.readback_debug)?;
    flag("trans_debug", &mut tuning.trans_debug)?;
    flag("text_gpu_debug", &mut tuning.text_gpu_debug)?;
    flag("render_timing", &mut tuning.render_timing)?;
    flag("render_trace_geometry", &mut tuning.render_trace_geometry)?;
    flag("p8_timing", &mut tuning.p8_timing)?;
    flag("font_trace", &mut tuning.font_trace)?;
    flag("png_cmp", &mut tuning.png_cmp)?;
    flag("vm_profile", &mut tuning.vm_profile)?;
    Ok(())
}

/// [`init_session`] + 宿主字体(用户导入的字体字节 / 用户选的字体族)。
///
/// `font_family` 非空时走 [`EngineOptions::force_font`]:把所有 `FONT` 请求
/// 解析到该 family。这不是"兜底默认字体",而是用户显式指定的替换 ——
/// 游戏通常点名 Windows 日文字体(ＭＳ ゴシック),Android 上没有。
pub fn init_session_with_fonts(
    data_root: PathBuf,
    save_root: PathBuf,
    entry: String,
    extra_fonts: Vec<Vec<u8>>,
    font_family: Option<String>,
) -> Result<String, String> {
    // 路径系统变量:Android 没有 USERPROFILE,主目录/个人目录由宿主给值。
    let path_overrides = yu_engine::host_paths::HostPathOverrides {
        appdata_path: Some(save_root.clone()),
        personal_path: Some(data_root.clone()),
        ..Default::default()
    };
    // 宿主字体字节:游戏通常点名 Windows 日文字体(ＭＳ ゴシック),Android
    // 系统里没有。用户导入的与 `dataRoot/fonts/` 下的都作为宿主能力注入。
    let font_bytes = extra_fonts;
    let mut config = RunConfig {
        data_root,
        save_root: save_root.clone(),
        entry,
        font_bytes,
        force_font: font_family.filter(|name| !name.trim().is_empty()),
        ..RunConfig::default()
    };
    config.font_bytes.extend(load_host_fonts(&config.data_root));
    // 老接口同样要应用 run-config.json(与 JSON 入口同一套键)。
    apply_run_config_file(&mut config);
    let started = start_engine(config, path_overrides)?;
    store_session(started)
}

/// 把启动产物装进线程本地会话,返回摘要。
fn store_session(started: StartedEngine) -> Result<String, String> {
    let viewport = started.engine.viewport();
    let summary = started.summary.clone();
    let capture = started.audio_capture;
    let capture_path = started.capture_path.clone();
    // 新会话初始状态:清掉所有跨线程请求位的残留。
    //
    // 为什么必需:上一轮 `onDestroy` 可能登记过销毁/挂起请求,而那个线程
    // 已经退出、没人消费。不清掉的话,新会话的**第一帧**就会被这些陈旧的
    // 请求销毁或挂起(表现为「重进游戏立刻闪退/黑屏」)。
    SHUTDOWN_REQUEST.store(false, std::sync::atomic::Ordering::Release);
    SUSPEND_REQUEST.store(false, std::sync::atomic::Ordering::Release);
    RESUME_REQUEST.store(false, std::sync::atomic::Ordering::Release);
    SESSION.with(|slot| {
        *slot.borrow_mut() = Some(Session {
            engine: started.engine,
            frame_w: viewport.width,
            frame_h: viewport.height,
            frame_pixels: None,
            ticks: 0,
            spikes_logged: 0,
            presenter: None,
            gpu: started.gpu,
            audio: started.audio,
            present_failures: 0,
            vm_profile: started.vm_profile,
            vm_profile_frames: 0,
            video: started.render,
            quiet: started.quiet,
            audio_capture_target: capture,
            capture_path,
        });
    });
    Ok(summary)
}

/// 按 [`GpuBackend`] 建 GPU 上下文,带**显式回退链**。
///
/// 回退链本身在 [`GpuBackend::ladder`](Android 目标下 `auto` = GLES →
/// Vulkan):之所以先 GLES,是因为模拟器的 gfxstream Vulkan 在
/// `vkQueueSubmit` 内空指针崩溃(进程级 SIGSEGV,无法 catch),而 GLES
/// 路径在同一模拟器上稳定。真机两者都能跑时 GLES 也够用(本引擎是纯 2D
/// 图元 + 纹理合成,不吃 Vulkan 的现代特性)。
fn create_gpu_context(gpu_backend: GpuBackend) -> Result<yu_backend_wgpu::GpuContext, String> {
    use yu_backend_wgpu::DeviceOptions;
    let mut failures = Vec::new();
    for (name, backends) in gpu_backend.ladder() {
        log_line("gpu", &format!("尝试后端 {name}"));
        let options = DeviceOptions {
            backends: Some(backends),
            // GLES 建窗口 surface 必须能从 instance 取到 display handle;
            // Android 是零大小标记,构造完全安全。
            display: Some(yu_backend_wgpu::AndroidNdkDisplay),
            ..DeviceOptions::default()
        };
        log_line("gpu", "creating instance/adapter/device...");
        match yu_backend_wgpu::GpuContext::new(&options) {
            Ok(context) => {
                log_line(
                    "yu-host-android",
                    &format!(
                        "gpu: requested={name} actual={:?} adapter={} text_storage={}                          (text_storage=false ⇒ 文本走 CPU 合成,plan 会明显变慢)",
                        context.backend, context.adapter_info.name, context.text_storage
                    ),
                );
                return Ok(context);
            }
            Err(diag) => {
                log_line(
                    "yu-host-android",
                    &format!("gpu: {name} 不可用: {} {}", diag.code, diag.detail),
                );
                failures.push(format!("{name}: {} {}", diag.code, diag.detail));
            }
        }
    }
    Err(format!(
        "GPU 后端都不可用({});该设备不支持 GPU 加速,请改用 CPU",
        failures.join("; ")
    ))
}

/// 把交错 f32 样本写成 16-bit PCM WAV(诊断:拉回电脑做波形分析)。
fn write_wav_f32(
    path: &std::path::Path,
    samples: &[f32],
    rate: u32,
    channels: u16,
) -> Result<usize, String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("创建目录 {} 失败: {e}", parent.display()))?;
        }
    }
    let data_bytes = samples.len() * 2;
    let mut out = Vec::with_capacity(44 + data_bytes);
    let ch = u32::from(channels);
    let byte_rate = rate * ch * 2;
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&((36 + data_bytes) as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    // 16-bit PCM 的 block align = 声道数 × 2 字节。
    out.extend_from_slice(&(channels * 2).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_bytes as u32).to_le_bytes());
    for sample in samples {
        let clamped = sample.clamp(-1.0, 1.0);
        out.extend_from_slice(&((clamped * 32767.0) as i16).to_le_bytes());
    }
    std::fs::write(path, &out).map_err(|e| format!("写 {} 失败: {e}", path.display()))?;
    Ok(out.len())
}

/// 安装 panic hook:Rust panic 会直接 abort 进程,而默认 hook 的输出在
/// Android 上不一定进 logcat。这里显式把 panic 位置与消息写到日志通道,
/// 否则现场只剩一个 SIGABRT 栈、看不到原因(实测:只有 `Fatal signal 6`)。
/// 初始化 `log` 门面(Android 上直通 logcat)。
///
/// 不做这步的话,引擎内部所有 `log::info!/warn!/error!` 都会被静默丢弃 ——
/// 排查时只能看到我们自己手写的行,看不到引擎/字体/渲染的诊断(实测教训)。
#[cfg(target_os = "android")]
/// 安装安卓独立调试日志(logcat + 文件双写)。
///
/// 只有**一个** logger、两条出口,不存在 `log` 宏被抢占的问题
/// (见 [`crate::logging`] 的模块文档)。
#[cfg(target_os = "android")]
fn init_logcat(quiet: bool) {
    crate::logging::init(if quiet {
        log::Level::Warn
    } else {
        log::Level::Info
    });
}

#[cfg(not(target_os = "android"))]
fn init_logcat(_quiet: bool) {}

fn install_panic_hook() {
    use std::sync::Once;
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        std::panic::set_hook(Box::new(|info| {
            log::error!(target: "yu-host-android", "[panic] {info}");
            flush_logs();
        }));
    });
}

/// 初始化文件日志(JSONL):`log_file` 指向的文件 + 门槛。
///
/// 与 CLI `--log-file` 同一条通道(`yu_log::facade` + `JsonlFile`),所以
/// Android 抓到的 JSONL 与桌面完全同格式,可直接用同一套工具分析。
fn init_file_logging(path: &std::path::Path, _level: LogLevel) -> Result<(), String> {
    #[cfg(target_os = "android")]
    crate::logging::set_file(path).map_err(|e| format!("创建日志文件 {} 失败: {e}", path.display()))?;
    #[cfg(not(target_os = "android"))]
    let _ = path;
    Ok(())
}

/// 把文件日志缓冲落盘(JSONL 是 `BufWriter`,需显式 flush)。
///
/// 调用点:`start_engine` 收尾、`shutdown`、以及壳每次取 `status` 时。
pub fn flush_logs() {
    crate::logging::flush();
}

/// 宿主诊断行:统一走新日志器(自动进 logcat + 文件)。
fn log_line(target: &str, message: &str) {
    log::warn!(target: "yu-kernel", "[{target}] {message}");
}

/// 应用 `<游戏目录>/debug/run-config.json`(与壳 JSON 同一套键)。
///
/// 两个启动入口(`nativeInit` 老接口与 `nativeInitJson`)都调用这里:
/// 调壳不方便时直接改文件即可打开诊断开关。解析失败不静默 —— 写进
/// logcat 一条 warn(此时文件日志可能还没装好)。
fn apply_run_config_file(config: &mut RunConfig) {
    let path = config.data_root.join("debug").join("run-config.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    if let Err(error) = config.apply_json(&text) {
        log::warn!(
            target: "yu-kernel",
            "[boot] {} 解析失败: {error}",
            path.display()
        );
    }
}

/// 从 JSON 配置启动会话(壳侧唯一推荐入口)。
///
/// `json` 里的 `data_root` / `save_root` 是必填(壳知道自己选的目录);
/// 其余键见 [`RunConfig::from_json`]。返回摘要或 `ERR: ...`。
pub fn start_session_json(json: &str) -> Result<String, String> {
    let mut config = RunConfig::from_json(json)?;
    if config.data_root.as_os_str().is_empty() {
        return Err("配置缺 data_root(游戏目录)".to_string());
    }
    if config.save_root.as_os_str().is_empty() {
        config.save_root = config.data_root.join("save");
    }
    // 调试覆盖:壳(JAVA)不方便重编时,把 `<游戏目录>/debug/run-config.json`
    // 铺在启动参数之上即可打开诊断开关(纯音/关渲染/指定采样率…)。
    // 文件不存在是常态,不报错、不留日志噪声。
    let override_path = config.data_root.join("debug").join("run-config.json");
    if let Ok(text) = std::fs::read_to_string(&override_path) {
        config
            .apply_json(&text)
            .map_err(|error| format!("{} 解析失败: {error}", override_path.display()))?;
    }
    let path_overrides = yu_engine::host_paths::HostPathOverrides {
        appdata_path: Some(config.save_root.clone()),
        personal_path: Some(config.data_root.clone()),
        ..Default::default()
    };
    let started = start_engine(config, path_overrides)?;
    store_session(started)
}

/// 按 [`RunConfig`] 启动引擎,返回 `(Engine, 摘要)`。
///
/// 所有运行参数都在这里落到 [`EngineOptions`];`path_overrides` 由调用方给
/// (桌面宿主可留默认)。
pub fn start_engine(
    config: RunConfig,
    path_overrides: yu_engine::host_paths::HostPathOverrides,
) -> Result<StartedEngine, String> {
    let RunConfig {
        data_root,
        save_root,
        entry,
        debug_mode,
        script_key,
        force_font,
        mut font_bytes,
        font_file,
        patch,
        backend,
        gpu_backend,
        os_sound,
        audio_output,
        screen_color,
        dx_mode,
        fps,
        save_codec,
        media_readonly,
        record_events,
        log_file,
        log_level,
        audio_capture_ms,
        audio_tone,
        audio_rate,
        render,
        tuning,
        quiet,
    } = config;
    let vm_profile = tuning.vm_profile;

    // 日志:非空 `log_file` 即开启。相对路径落游戏目录,便于 adb pull。
    // 门面是进程级单例,重复初始化保持首次(so 在进程内只应 init 一次)。
    let log_note = match &log_file {
        Some(path) => {
            let resolved = if path.is_absolute() {
                path.clone()
            } else {
                data_root.join(path)
            };
            match init_file_logging(&resolved, log_level) {
                Ok(()) => format!(" log={}", resolved.display()),
                Err(error) => return Err(error),
            }
        }
        None => String::new(),
    };
    // 用户导入的字体文件:读不到就显式失败(不静默忽略 —— 否则用户会
    // 以为换上了其实没换)。
    let mut host_font_index = None;
    if let Some(path) = font_file {
        let bytes = std::fs::read(&path)
            .map_err(|e| format!("字体读取失败 {}: {e}", path.display()))?;
        host_font_index = Some(font_bytes.len());
        font_bytes.push(bytes);
    }
    // 统一字体选择:显式 family 优先;只给了字体文件则强制该文件的 face。
    let font = match force_font.filter(|name| !name.trim().is_empty()) {
        Some(family) => yu_engine::engine::FontChoice::Family(family),
        None => match host_font_index {
            Some(index) => yu_engine::engine::FontChoice::HostFont(index),
            None => yu_engine::engine::FontChoice::Script,
        },
    };
    init_logcat(quiet);
    install_panic_hook();
    // 自证:日志链路一旦装好就立刻打一条。新日志器**单一路径双写**
    // (logcat + 文件),所以这一条必然两边都出现,不再需要「两条出口
    // 分别验证」的绕路。
    log::warn!(
        target: "yu-kernel",
        "[boot] logging ready (quiet={quiet}, level={log_level:?}, png_backend={}, tuning={{prefetch={} snapshot_defer={} vm_profile={} asset_timing={} tick_split={} cg_timing={}}})",
        tuning.png_backend.as_str(),
        tuning.prefetch,
        tuning.snapshot_defer,
        tuning.vm_profile,
        tuning.asset_timing,
        tuning.tick_split,
        tuning.cg_timing
    );
    flush_logs();
    // 存档根必须存在:缺目录会让 SAVE 与既读记录全部静默失败。
    std::fs::create_dir_all(&save_root)
        .map_err(|e| format!("创建存档目录失败 {}: {e}", save_root.display()))?;
    let project = Project::discover(&data_root, None)
        .map_err(|failure| format!("数据入口装配失败: {failure}"))?;
    let dataset = project.describe();
    // 存档块压缩层:与 CLI `--save-codec` 同义(只影响写盘);`None` = 自动。
    let codec = save_codec;
    // 后端:GPU 失败必须显式报错,不自动降级(自动降级 = 用户以为在看 GPU
    // 结果其实是 CPU)。
    let chosen_backend = backend;
    // GPU 上下文由宿主建(引擎侧 `gpu_context` 注入口):这样才能控制
    // 底层接口与回退顺序,而不是交给进程级环境变量。
    let gpu_context = if chosen_backend == Backend::Wgpu {
        Some(create_gpu_context(gpu_backend)?)
    } else {
        None
    };
    // 引擎与上屏器必须共用同一个 device,所以这里留一份克隆给宿主。
    let gpu_for_present = gpu_context.clone();
    let options = EngineOptions {
        backend: chosen_backend,
        gpu_context,
        entry: entry.clone(),
        // 长跑宿主:首帧前预热字体,避免第一次文本场景的枚举停顿。
        warm_text: true,
        debug_mode,
        screen_color,
        dx_mode,
        fps,
        // `@_OS_SOUND` 注入与是否真出声解耦(架构 §2.5 R5)。
        os_sound,
        media_readonly,
        record_events,
        // 归因实验:关掉渲染时引擎复用首帧呈现面,VM/声音照常推进。
        render,
        // 渲染/诊断参数与桌面同一份结构(Android 不再 setenv 转手)。
        tuning,
        gpu_backend,
        script_key,
        save_codec: codec,
        save_root: Some(save_root.clone()),
        path_overrides,
        font_bytes,
        font,
        patch: yu_engine::patch::PatchSelection::new(patch),
        // `@_TIME` 注入系统本地时间(存档菜单的现实日期)。以前这里缺省
        // `None` ⇒ Android 上恒 1970,与桌面出现脚本可见的行为分歧。
        local_time: yu_engine::host_clock::local_now(),
        ..EngineOptions::default()
    };
    log::warn!(
        target: "yu-kernel",
        "[boot] render features: prefetch={} snapshot_defer={} bilinear_scale={} text_gpu={} png_backend={}",
        tuning.prefetch,
        tuning.snapshot_defer,
        tuning.bilinear_scale,
        tuning.text_gpu,
        tuning.png_backend.as_str()
    );
    let mut engine =
        Engine::new(project, options).map_err(|failure| format!("引擎启动失败: {failure}"))?;
    // 音频输出:与 `@_OS_SOUND` 解耦 —— `os_sound` 决定 VM 可见变量,
    // `audio_output` 才决定是否真的打开设备(架构 §2.5 R5)。
    //
    // 打不开设备不致命:静音继续跑,但必须留痕(否则用户只会觉得"没声音"
    // 而查不到原因)。
    let mut audio_sink = None;
    let audio_note = if !audio_output {
        " audio=off".to_string()
    } else {
        match sound::SoundSink::open_with_rate(audio_rate) {
            Ok(sink) => {
                let description = sink.describe();
                // 纯音自测:旁路引擎,只验「环 → oboe → 系统」这段。
                if audio_tone {
                    sink.set_tone(true);
                }
                engine.attach_audio(Box::new(sink.handle()));
                // 真正的 sink 留在这里(它持有 stream,drop 即停声)。
                audio_sink = Some(sink);
                format!(" audio={description}")
            }
            Err(error) => {
                log_line("yu-host-android", &format!("音频设备打开失败: {error}"));
                format!(" audio=FAILED({error})")
            }
        }
    };
    if engine.font_summary().is_none() {
        engine.warm_text();
    }
    let viewport = engine.viewport();
    let summary = format!(
        "OK backend={} entry={} viewport={}x{} fps={} boot_ms={:.1} os_sound={}          save_codec={} data={} save={}{}{}",
        engine.backend(),
        engine.entry(),
        viewport.width,
        viewport.height,
        fps,
        engine.boot_ms(),
        os_sound,
        save_codec.map_or("auto", BlockCodec::as_str),
        dataset.summary(),
        save_root.display(),
        audio_note,
        log_note,
    );
    let summary = if chosen_backend == Backend::Wgpu {
        let gpu = engine.wgpu_renderer().map(|r| r.adapter().backend.clone());
        format!("{summary} gpu={}", gpu.unwrap_or_else(|| "?".to_string()))
    } else {
        summary
    };
    log_line("yu-host-android", &summary);
    flush_logs();
    // 发布模式下忽略抓取请求(壳无条件传 3000ms,那是诊断用的)。
    let audio_capture_ms = if quiet { 0 } else { audio_capture_ms };
    let capture_samples = (u64::from(audio_capture_ms) * 44100 / 1000 * 2) as usize;
    let capture_path = (capture_samples > 0)
        .then(|| data_root.join("debug").join("audio-capture.wav"));
    Ok(StartedEngine {
        engine,
        summary,
        render,
        quiet,
        gpu: gpu_for_present,
        audio: audio_sink,
        audio_capture: capture_samples,
        capture_path,
        vm_profile,
    })
}

/// [`start_engine`] 的产物:引擎 + 摘要 + 可复用的 GPU 上下文。
pub struct StartedEngine {
    pub engine: Engine,
    pub summary: String,
    /// 是否渲染上屏(`false` = 纯音频归因模式)。
    pub render: bool,
    /// 发布模式(见 [`RunConfig::quiet`])。
    pub quiet: bool,
    /// GPU 后端时为 `Some`,零拷贝上屏用它建 surface(必须同一 device)。
    pub gpu: Option<yu_backend_wgpu::GpuContext>,
    /// 音频输出设备。`Some` 时引擎已挂上,回调驱动播放位置。
    /// 必须在会话存活期间持有(drop 即停声)。
    pub audio: Option<sound::SoundSink>,
    /// 音频抓取目标样本数(`RunConfig::audio_capture` 非 0 时)。
    pub audio_capture: usize,
    /// 抓取落盘路径。
    pub capture_path: Option<PathBuf>,
    /// 逐 opcode 耗时归因(宿主每 60 帧打印一次 top-8)。
    pub vm_profile: bool,
}


/// 推进一帧。返回 `0` 成功 / `-1` 未初始化 / `-2` tick 失败。
pub fn tick() -> i32 {
    SESSION.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(session) = slot.as_mut() else {
            set_error("nativeTick 在 nativeInit 之前被调用");
            return -1;
        };
        // 跨线程销毁请求:壳的 `onDestroy` 在 UI 线程上请求,这里(会话所属
        // 线程)真正释放。返回 [`TICK_HOST_SHUTDOWN`] —— **不能**复用 1:
        // 1 是「脚本正常走完」,壳收到它会 `finish()` 回配置界面,而这时
        // Activity 正在销毁,那是无意义的重入。
        if SHUTDOWN_REQUEST.swap(false, std::sync::atomic::Ordering::AcqRel) {
            log_line("yu-host-android", "shutdown requested from another thread");
            drop(slot.take());
            flush_logs();
            return TICK_HOST_SHUTDOWN;
        }
        // 跨线程挂起/恢复:必须在 tick 之前、在**本线程**执行。
        if apply_pending_lifecycle(session) {
            return 0;
        }
        match session.engine.tick_realtime() {
            Ok(rendered) => {
                session.ticks += 1;
                // 逐 opcode 耗时归因(`tuning.vm_profile`):每 60 帧把本窗口
                // 累计耗时 top-8 打进 logcat。
                //
                // 采样器本身有开销(每条命令两次 `Instant::now()` ≈ 40~50ns),
                // 所以只在显式开启时运行 —— 但**结论仍有效**:它给出的是
                // 各 opcode 的**相对**占比,正好用来判断该优化哪一条。
                if session.vm_profile {
                    session.vm_profile_frames += 1;
                    if session.vm_profile_frames >= 60 {
                        session.vm_profile_frames = 0;
                        let (rows, total_ns) = session.engine.take_command_profile(8);
                        let top: Vec<String> = rows
                            .iter()
                            .filter(|(_, _, ns)| *ns > 0)
                            .map(|(name, hits, ns)| {
                                format!(
                                    "{name}={:.1}ms/{}x/{:.0}ns",
                                    *ns as f64 / 1e6,
                                    hits,
                                    if *hits > 0 { *ns as f64 / *hits as f64 } else { 0.0 }
                                )
                            })
                            .collect();
                        log_line(
                            "yu-kernel",
                            &format!(
                                "[vmprof] 60帧 total={:.1}ms | {}",
                                total_ns as f64 / 1e6,
                                top.join(" ")
                            ),
                        );
                        session.engine.reset_command_profile();
                    }
                }
                // 长帧取证:只记超过阈值的帧(logcat),正常帧零额外开销。
                //
                // 安卓侧此前**没有任何逐帧耗时出口**:`nativeStatus()` 只在
                // 壳开调试时才调,发布模式完全看不到。于是「角色出场卡」
                // 「切动作卡」只能靠猜。这里按桌面 `--spike-log` 的口径把
                // 慢帧写进 logcat —— 修不修得动先放一边,至少要能量到。
                //
                // 阈值固定 20ms(= 60fps 下丢一帧),只读 `RenderedFrame.report`
                // 里的既有字段,不做额外取数。
                {
                    let report = &rendered.report;
                    if report.ms >= 8.0 && session.spikes_logged < 2000 {
                        session.spikes_logged += 1;
                        // 慢帧必须自带**归因**:只有 tick/render 两个数无法判断
                        // 是 VM 慢还是渲染规划慢,更分不出 `plan` 里是图像加工、
                        // 文本合成(含整层同步读回)还是绘制规划。
                        // 真机实测 `render=76ms 而 ops 只有 14` —— 就是靠这
                        // 组字段才定位到「文本基面同步读回」。
                        let ph = session.engine.render_phases().unwrap_or_default();
                        // `dec/up` 只统计本帧 render 阶段的解码/上传
                        // (tick 末尾的 preload 已在 `begin_frame` 前完成并
                        // 被清零);preload 本身看上面的 `preload=`。
                        log::warn!(
                            target: "yu-kernel",
                            "[spike] frame={} tick={:.1}ms render={:.1}ms total={:.1}ms \
                             preload={:.1}ms/{} err={} build={:.1}ms \
                             cmds={} ops={} passes={} layers={} visible={} \
                             plan[snap={} image={} text={} copy_text={} rest={}]us \
                             text_cpu[rb={}us x{} asset={}us] rb={}us enc={}us \
                             dec={}us/{}B up={}/{}B cpu={} res=[{}]",
                            report.frame,
                            report.tick_ms,
                            report.render_ms,
                            report.ms,
                            // **`tick_ms` 包含 preload**:它从 `vm.tick_at()` 之前
                            // 开始、到 `frame()` 之后结束,中间夹着
                            // `preload_assets()`(纹理解码;GPU 上传已延迟到
                            // `plan` 首绘)。
                            // 真机 566 层场景一次 `tick=2883ms` 里 render 只占
                            // 257ms —— 差额就在预加载。不打出这几项无法归因。
                            session.engine.preload_ms(),
                            session.engine.preload_counts().0,
                            session.engine.preload_counts().1,
                            session.engine.frame_build_ms(),
                            report.commands,
                            report.ops,
                            report.passes,
                            report.layers,
                            report.visible,
                            ph.snap_us,
                            ph.image_us,
                            ph.text_us,
                            ph.copy_text_us,
                            ph.rest_us,
                            ph.text_cpu_readback_us,
                            ph.text_cpu_readbacks,
                            ph.text_cpu_asset_us,
                            ph.readback_us,
                            ph.encode_us,
                            ph.decode_us,
                            ph.decode_bytes,
                            ph.uploads,
                            ph.upload_bytes,
                            ph.cpu_fallbacks,
                            session
                                .engine
                                .render_resource_snapshot()
                                .unwrap_or_default()
                        );
                    }
                }
                // 音频抓取:启动满 2 秒后抓 3 秒,再落盘供数值分析。
                if session.audio_capture_target > 0
                    && session.ticks == 120
                {
                    session.engine.start_audio_capture(session.audio_capture_target);
                }
                if session.audio_capture_target > 0 && session.ticks == 300 {
                    if let Some(samples) = session.engine.take_audio_capture() {
                        let path = session.capture_path.clone();
                        if let Some(path) = path {
                            match write_wav_f32(&path, &samples, 44100, 2) {
                                Ok(bytes) => log_line(
                                    "yu-host-android",
                                    &format!("audio capture: {} samples -> {} ({} bytes)", samples.len(), path.display(), bytes),
                                ),
                                Err(error) => log_line("yu-host-android", &format!("audio capture 写盘失败: {error}")),
                            }
                        }
                    }
                    session.audio_capture_target = 0;
                }
                session.frame_w = rendered.surface.width;
                session.frame_h = rendered.surface.height;
                // 零拷贝路径:GPU 后端 + 已绑定原生窗口时,直接把引擎屏幕
                // 纹理呈现到窗口,**不做 CPU 读回**(省掉整帧 1.9MB 拷贝)。
                // CPU 后端没有 GPU 纹理,仍走 `frame_pixels` 给壳画 Bitmap。
                let mut presented = false;
                if !session.video {
                    // 纯音频归因:不做任何上屏工作(呈现面复用首帧)。
                    presented = true;
                }
                if session.presenter.is_some() && !presented {
                    // 借出 presenter 与 engine 的屏幕纹理(不同字段,可同时借用)。
                    // 分别借用两个字段(不该用裸指针):engine 只读取纹理,
                    // presenter 可变。Rust 允许对不同字段同时借用。
                    let (engine, presenter_slot) = (&session.engine, &mut session.presenter);
                    let screen = engine.screen_texture();
                    if let (Some(presenter), Some((id, w, h, texture))) =
                        (presenter_slot.as_mut(), screen)
                    {
                        match presenter.present(texture, id, w, h) {
                            Ok(()) => presented = true,
                            Err(error) => {
                                session.present_failures += 1;
                                if session.present_failures <= 3 {
                                    let message = format!("上屏失败: {error}");
                                    log_line("yu-host-android", &message);
                                    set_error(message);
                                }
                            }
                        }
                    }
                }
                if !presented && session.video {
                    // 回退:保留 CPU 像素供壳按旧路径绘制。GPU 路径下这只是
                    // 诊断副本,不影响上屏。
                    session.frame_pixels = Some(Arc::clone(&rendered.surface));
                }
                // 退出信号(跨平台统一):`1` = 游戏已结束,壳据此
                // `finish()` 回到自己的界面,而不是继续 tick 一个死会话。
                // 以前只有非 quiet 状态串里带 `main_finished=1`,发布模式
                // 永远看不到 ⇒ 游戏 END 后画面停住、音乐继续。
                session.engine.lifecycle().host_code()
            }
            Err(failure) => {
                set_error(format!("tick 失败: {failure}"));
                -2
            }
        }
    })
}

/// 用 Java `Surface` 建立零拷贝上屏(由 `jni_entry` 内的 unsafe 包装调用)。
///
/// `attach` 是「把 JNI 句柄转成 presenter」的闭包:把 unsafe 隔离在
/// [`jni_entry`] 里,本函数保持安全 Rust。
///
/// # Errors
///
/// 未初始化、CPU 后端、或窗口/surface 创建失败时返回错误文本。
pub fn attach_surface(
    attach: impl FnOnce(&yu_backend_wgpu::GpuContext) -> Result<present::NativePresenter, String>,
) -> Result<String, String> {
    SESSION.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(session) = slot.as_mut() else {
            return Err("attachSurface 在 nativeInit 之前被调用".to_string());
        };
        // CPU 后端没有 GPU 纹理可上屏,直接拒绝(壳据此继续用 Bitmap 路径)。
        let Some(context) = session.gpu.as_ref() else {
            return Err("当前是 CPU 后端,零拷贝上屏需要 GPU".to_string());
        };
        let presenter = attach(context)?;
        let (width, height) = presenter.size();
        session.presenter = Some(presenter);
        let summary = format!("surface attached: {}x{}", width, height);
        log_line("yu-host-android", &summary);
        Ok(summary)
    })
}

/// 当前帧尺寸。
pub fn frame_size() -> (u32, u32) {
    SESSION.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|session| (session.frame_w, session.frame_h))
            .unwrap_or((0, 0))
    })
}

/// 当前帧 RGBA 副本(空 = 还没有帧)。
pub fn frame_rgba() -> Vec<u8> {
    SESSION.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|session| {
                session
                    .frame_pixels
                    .as_ref()
                    .map(|surface| surface.rgba.clone())
                    .unwrap_or_default()
            })
            .unwrap_or_default()
    })
}

/// RGBA → `ARGB_8888`(`0xAARRGGBB`),可直接交给
/// `Bitmap.createBitmap(pixels, w, h, ARGB_8888)`。
///
/// 换算放在内核侧只有一处,Java 壳不必再关心通道序。
pub fn rgba_to_argb(rgba: &[u8]) -> Vec<u32> {
    rgba.chunks_exact(4)
        .map(|pixel| {
            (u32::from(pixel[3]) << 24)
                | (u32::from(pixel[0]) << 16)
                | (u32::from(pixel[1]) << 8)
                | u32::from(pixel[2])
        })
        .collect()
}

/// 把当前帧就地转换成 `ARGB_8888` 写进调用方给的缓冲(**复用容量**)。
///
/// # 为什么不是 `frame_rgba()` + [`rgba_to_argb`]
///
/// 旧写法每帧做 **3 次整帧分配 + 4 次全量拷贝**
/// (`frame_pixels.clone()` → `rgba_to_argb` 新 `Vec` → `Vec<jint>` 再来一份)。
/// 1080p 一帧是 8.3MB,三次分配四次拷贝在手机上就是每帧几毫秒的纯浪费,
/// 还让分配器长期高位运行(GC/碎片压力直接表现为掉帧)。
///
/// 这里只做一次转换,目标缓冲容量跨帧复用 —— 稳态下零分配。
///
/// 返回写入的像素数;`0` = 还没有帧。
pub fn fill_argb(out: &mut Vec<i32>) -> usize {
    // 先清空:无论有没有帧,调用方拿到的都必须是「本次的结果」。
    // 留着上一帧的像素会让壳在会话刚死掉时继续把旧画面送上屏(看着像卡住)。
    out.clear();
    SESSION.with(|slot| {
        let slot = slot.borrow();
        let Some(session) = slot.as_ref() else {
            return 0;
        };
        let Some(surface) = session.frame_pixels.as_ref() else {
            return 0;
        };
        let rgba = surface.rgba.as_slice();
        let pixels = rgba.len() / 4;
        if pixels == 0 {
            return 0;
        }
        out.reserve(pixels.saturating_sub(out.capacity()));
        for pixel in rgba.chunks_exact(4) {
            let argb = (u32::from(pixel[3]) << 24)
                | (u32::from(pixel[0]) << 16)
                | (u32::from(pixel[1]) << 8)
                | u32::from(pixel[2]);
            out.push(argb as i32);
        }
        pixels
    })
}

/// 注入输入按钮。
pub fn set_input(button: &str, pressed: bool) -> i32 {
    SESSION.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(session) = slot.as_mut() else {
            return -1;
        };
        session.engine.set_input(button, pressed);
        0
    })
}

/// 注入光标位置(引擎视口像素坐标)。
pub fn set_cursor(x: i64, y: i64) -> i32 {
    SESSION.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(session) = slot.as_mut() else {
            return -1;
        };
        session.engine.set_cursor(x, y);
        0
    })
}

/// 诊断摘要(浮层 / logcat 用)。
pub fn status_text() -> String {
    let mut out = String::new();
    SESSION.with(|slot| {
        let mut slot = slot.borrow_mut();
        match slot.as_mut() {
            Some(session) => {
                // 发布模式:只算两个廉价字段。整条状态串要拼十几个 format、
                // 取诊断、读渲染阶段原子量 —— 每秒一次对帧时间没意义,对一个
                // 要上架的产品却是纯噪声。
                if session.quiet {
                    // 发布模式仍保留状态判据:壳/自动化用 `status=running`
                    // 判断会话是否活着,不能因为省字段就把它删掉。
                    // 生命周期同样必须留在发布模式 —— 它是退出信号,
                    // 不是诊断(少了它壳就不知道游戏已经结束)。
                    out.push_str(&format!(
                        "status={} frame={} fps=60",
                        session.engine.lifecycle().as_str(),
                        session.engine.last_report().frame
                    ));
                    if let yu_engine::engine::Lifecycle::Exited { restart } =
                        session.engine.lifecycle()
                    {
                        out.push_str(&format!(" restart={}", u8::from(restart)));
                    }
                    return;
                }
                let report = session.engine.last_report().clone();
                let diags = session.engine.take_diags().len();
                let clipped = session.engine.sound_clipped_samples();
                let rt_silence = session.engine.sound_rt_silence_samples();
                out.push_str(&format!(
                    "status=running backend={} entry={} frame={} ticks={} viewport={}x{} \
                     layers={} ops={} skipped={} drawable={} boot_ms={:.1} diags={}                      tick_ms={:.2} render_ms={:.2} snd[underrun={} clipped={} rtsil={} dec_ms={:.2}]",
                    session.engine.backend(),
                    session.engine.entry(),
                    report.frame,
                    session.ticks,
                    session.engine.viewport().width,
                    session.engine.viewport().height,
                    report.layers,
                    report.ops,
                    report.skipped,
                    report.drawable,
                    session.engine.boot_ms(),
                    diags,
                    report.tick_ms,
                    report.render_ms,
                    report.sound_underruns,
                    clipped,
                    rt_silence,
                    report.sound_stream_ms,
                ));
                // 设备侧交付:short>0 = 环被抽干(我们补了静音)。
                if let Some(audio) = session.audio.as_ref() {
                    out.push_str(&format!(" dev[{}]", audio.stats_line()));
                }
                // 渲染阶段拆解:`readback_us` 是 GPU→CPU 整帧读回的耗时,
                // 也是 Android 上能否跑满 60fps 的关键项。
                //
                // `plan` 必须拆开看:`plan_us` 是总时长,里面 `image`(图像
                // 加工)/`text`(文本合成,**含整层同步读回**)/`snap`/`rest`
                // 是完全不同的瓶颈,只看 plan 总数无法定位「角色出场 35ms」
                // 这类尖峰(实测那一帧 ops 只有 14)。
                if let Some(phases) = session.engine.render_phases() {
                    out.push_str(&format!(
                        " phases[plan={} snap={} image={} text={} copy_text={} rest={} \
                         encode={} readback={} tail={}]us \
                         text_cpu[rb={}us x{} asset={}us/{}B cache={} store={}]",
                        phases.plan_us,
                        phases.snap_us,
                        phases.image_us,
                        phases.text_us,
                        phases.copy_text_us,
                        phases.rest_us,
                        phases.encode_us,
                        phases.readback_us,
                        phases.tail_us,
                        phases.text_cpu_readback_us,
                        phases.text_cpu_readbacks,
                        phases.text_cpu_asset_us,
                        phases.text_cpu_asset_bytes,
                        phases.text_cpu_cached,
                        phases.text_cpu_stores
                    ));
                }
                if !session.video {
                    out.push_str(" video=off");
                }
                // 生命周期(统一退出信号):running / finished / exited。
                let lifecycle = session.engine.lifecycle();
                out.push_str(&format!(" lifecycle={}", lifecycle.as_str()));
                if lifecycle.is_terminal() {
                    out.push_str(" main_finished=1");
                }
                if let yu_engine::engine::Lifecycle::Exited { restart } = lifecycle {
                    out.push_str(&format!(" restart={}", u8::from(restart)));
                }
                if session.engine.is_suspended() {
                    out.push_str(" suspended=1");
                }
                // 资产缓存取证:常驻字节/预算 + 已回收数。
                // 「越打越卡」时先看这两项 —— 常驻字节贴预算说明在反复解码,
                // 停在低位说明卡顿另有来源(不再是纹存)。
                let (cache_bytes, evicted) = session.engine.texture_cache_bytes();
                out.push_str(&format!(
                    " assets[cached={} decoded={} resident={}MB evicted={}]",
                    session.engine.textures_cached(),
                    session.engine.textures_decoded(),
                    cache_bytes / (1024 * 1024),
                    evicted
                ));
            }
            None => out.push_str("status=idle"),
        }
    });
    if let Some(error) = last_error() {
        out.push_str(&format!(" error={error}"));
    }
    // 每帧状态行都会调用 status_text;顺手 flush,避免日志长时间滞留缓冲。
    flush_logs();
    out
}

/// 挂起会话(壳 `onPause` / 页面隐藏 / 后台)。
///
/// 语义见 [`yu_engine::engine::Engine::suspend`]:丢弃暂停期间的真实时间
/// (恢复后不快进)并静音音频输出。**不做存档 flush** —— 存档在 `SAVE`
/// 命令时刻就已落盘,挂起时再写一次只会制造第二份不一致的现场。
///
/// 返回 `0` 成功、`-1` 会话未初始化。重复调用幂等。
pub fn suspend() -> i32 {
    // 跨线程只置请求标志;真正的 `engine.suspend()` 由渲染线程执行
    // (见 [`tick`] 开头的 `apply_pending_lifecycle`)。
    SUSPEND_REQUEST.store(true, std::sync::atomic::Ordering::Release);
    0
}

/// 恢复会话(壳 `onResume`)。与 [`suspend`] 配对,未挂起时是 no-op。
///
/// 返回 `0` 成功、`-1` 会话未初始化。
pub fn resume() -> i32 {
    RESUME_REQUEST.store(true, std::sync::atomic::Ordering::Release);
    0
}

/// 渲染线程在每帧 tick 前执行挂起/恢复/改尺寸请求。
///
/// 返回 `true` 表示本帧已被挂起(调用方应跳过 tick)。
fn apply_pending_lifecycle(session: &mut Session) -> bool {
    use std::sync::atomic::Ordering;
    // 尺寸变更先做:它与挂起无关,后台回来那次 resize 不该被丢掉。
    let packed = RESIZE_REQUEST.swap(0, Ordering::AcqRel);
    if packed != 0 {
        let w = (packed >> 32) as u32;
        let h = (packed & 0xFFFF_FFFF) as u32;
        if let Some(presenter) = session.presenter.as_mut() {
            match presenter.resize(w, h) {
                Ok(()) => log_line(
                    "yu-host-android",
                    &format!("surface resized to {w}x{h} (on render thread)"),
                ),
                Err(error) => log_line(
                    "yu-host-android",
                    &format!("surface resize {w}x{h} 失败: {error}"),
                ),
            }
        }
    }
    if SUSPEND_REQUEST.swap(false, Ordering::AcqRel) {
        session.engine.suspend();
        log_line("yu-host-android", "session suspended (on render thread)");
        return true;
    }
    if RESUME_REQUEST.swap(false, Ordering::AcqRel) {
        session.engine.resume();
        log_line("yu-host-android", "session resumed (on render thread)");
    }
    false
}

/// 请求调整上屏尺寸(可在任意线程调用)。
///
/// 只记请求,真正的 `presenter.resize` 由渲染线程在下一帧 tick 前执行
/// (见 [`apply_pending_lifecycle`])。返回 `0` = 已登记。
pub fn request_resize(width: u32, height: u32) -> i32 {
    use std::sync::atomic::Ordering;
    let w = u64::from(width.max(1));
    let h = u64::from(height.max(1));
    RESIZE_REQUEST.store((w << 32) | h, Ordering::Release);
    0
}

/// 消费一次「跨线程销毁请求」,并在本线程真正释放会话。
///
/// 供壳在**不调用 `nativeTick`** 的空转路径上轮询(挂起/无 surface 时
/// 壳只 sleep,不 tick;而销毁请求只在 tick 里被取走 ⇒ 那个状态下退出
/// 应用,会话永远不会释放)。
///
/// 返回 `1` = 已释放(壳应退出渲染循环),`0` = 没有请求。
pub fn take_shutdown_request() -> i32 {
    if !SHUTDOWN_REQUEST.swap(false, std::sync::atomic::Ordering::AcqRel) {
        return 0;
    }
    let released = SESSION.with(|slot| slot.borrow_mut().take().is_some());
    log_line(
        "yu-host-android",
        &format!("shutdown request consumed while idle (released={released})"),
    );
    flush_logs();
    TICK_HOST_SHUTDOWN
}

/// 会话生命周期代号(供壳在 `nativeTick` 之外主动查询)。
///
/// - `0` = running(可继续 tick)
/// - `1` = finished(主脚本走完)
/// - `2` = exited(脚本 `END`)
/// - `3` = exited 且带 `RESTART` 标志
/// - `-1` = 会话未初始化
///
/// 与 `nativeTick` 的返回值同一套判据(`Lifecycle::host_code`):
/// tick 返回 `1` ⇔ 这里返回 `1/2/3`。壳只需要看其中一个。
pub fn lifecycle_code() -> i32 {
    SESSION.with(|slot| {
        let slot = slot.borrow();
        let Some(session) = slot.as_ref() else {
            return -1;
        };
        match session.engine.lifecycle() {
            yu_engine::engine::Lifecycle::Running => 0,
            yu_engine::engine::Lifecycle::Finished => 1,
            yu_engine::engine::Lifecycle::Exited { restart: false } => 2,
            yu_engine::engine::Lifecycle::Exited { restart: true } => 3,
        }
    })
}

/// 释放会话(可再次 `init_session`)。返回是否真的释放了一个会话。
///
/// **只能在本线程调用**(会话在 `thread_local` 里)。壳在 `onDestroy`
/// (UI 线程)调用时走 [`request_shutdown`],由渲染线程自己收尾。
pub fn shutdown() -> bool {
    flush_logs();
    SESSION.with(|slot| slot.borrow_mut().take().is_some())
}

/// 请求销毁会话(可在任意线程调用)。
///
/// 壳的 `onDestroy` 在 **UI 线程**上,而会话在渲染线程的 `thread_local`
/// 里:那里直接 `nativeShutdown()` 只能拿到空槽、返回 -1,**会话根本没被
/// 释放**(旧实现就是这样 —— 退出后引擎资源、音频设备、GPU 上下文全靠
/// 线程结束时的析构兜底)。这里只置标志,渲染线程在下一帧 tick 前看到它
/// 就自己收尾。
pub fn request_shutdown() -> i32 {
    SHUTDOWN_REQUEST.store(true, std::sync::atomic::Ordering::Release);
    0
}

/// 构建目标标记(Java 侧版本核对用)。
pub fn version_tag() -> String {
    format!(
        "yu-host-android/{} host={} arch={}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
}

// ---------------------------------------------------------------- JNI exports
//
// 类名固定 `org.yuris.kernel.NativeHost`;返回 `OK` / `ERR: ...` 表示成败
// (不抛 Java 异常,壳侧只看字符串)。
mod jni_entry {
    #![allow(unsafe_code)]

    use super::*;
    use jni::objects::{JClass, JIntArray, JString};
    use jni::sys::{jboolean, jint};
    use jni::JNIEnv;

    /// 把 Rust 字符串交给 Java(分配失败时给空串,调用方按返回值判错)。
    fn to_jstring<'local>(env: &mut JNIEnv<'local>, text: impl AsRef<str>) -> JString<'local> {
        match env.new_string(text.as_ref()) {
            Ok(value) => value,
            Err(_) => env
                .new_string("")
                .expect("allocating an empty Java string must succeed"),
        }
    }

    #[no_mangle]
    pub extern "system" fn Java_org_yuris_kernel_NativeHost_nativeInit<'local>(
        mut env: JNIEnv<'local>,
        _class: JClass<'local>,
        data_root: JString<'local>,
        save_root: JString<'local>,
        entry: JString<'local>,
        font_path: JString<'local>,
        font_family: JString<'local>,
    ) -> JString<'local> {
        let data_root: String = match env.get_string(&data_root) {
            Ok(text) => text.into(),
            Err(error) => {
                let message = format!("dataRoot 无法读取: {error}");
                set_error(message.clone());
                return to_jstring(&mut env, format!("ERR: {message}"));
            }
        };
        let save_root: String = match env.get_string(&save_root) {
            Ok(text) => text.into(),
            Err(error) => {
                let message = format!("saveRoot 无法读取: {error}");
                set_error(message.clone());
                return to_jstring(&mut env, format!("ERR: {message}"));
            }
        };
        let entry: String = match env.get_string(&entry) {
            Ok(text) => text.into(),
            Err(error) => {
                let message = format!("entry 无法读取: {error}");
                set_error(message.clone());
                return to_jstring(&mut env, format!("ERR: {message}"));
            }
        };
        let entry = if entry.trim().is_empty() {
            "SYSTEM_START".to_string()
        } else {
            entry
        };
        // 用户在配置界面选的字体(空 = 不指定)。读不到就显式失败,不静默忽略。
        let font_path: String = match env.get_string(&font_path) {
            Ok(text) => text.into(),
            Err(_) => String::new(),
        };
        let extra_fonts = if font_path.trim().is_empty() {
            Vec::new()
        } else {
            match std::fs::read(&font_path) {
                Ok(bytes) => vec![bytes],
                Err(error) => {
                    let message = format!("字体读取失败 {font_path}: {error}");
                    set_error(message.clone());
                    return to_jstring(&mut env, format!("ERR: {message}"));
                }
            }
        };

        // 用户在下拉里选的字体族(空 = 不强制,走引擎默认回退表)。
        let font_family: String = match env.get_string(&font_family) {
            Ok(text) => text.into(),
            Err(_) => String::new(),
        };
        let font_family = if font_family.trim().is_empty() {
            None
        } else {
            Some(font_family)
        };

        match init_session_with_fonts(
            PathBuf::from(&data_root),
            PathBuf::from(&save_root),
            entry,
            extra_fonts,
            font_family,
        ) {
            Ok(summary) => {
                clear_error();
                to_jstring(&mut env, summary)
            }
            Err(message) => {
                set_error(message.clone());
                to_jstring(&mut env, format!("ERR: {message}"))
            }
        }
    }

    #[no_mangle]
    pub extern "system" fn Java_org_yuris_kernel_NativeHost_nativeTick<'local>(
        _env: JNIEnv<'local>,
        _class: JClass<'_>,
    ) -> jint {
        tick() as jint
    }

    #[no_mangle]
    pub extern "system" fn Java_org_yuris_kernel_NativeHost_nativeFrameSize<'local>(
        env: JNIEnv<'local>,
        _class: JClass<'local>,
    ) -> JIntArray<'local> {
        let (width, height) = frame_size();
        let Ok(array) = env.new_int_array(2) else {
            return JIntArray::default();
        };
        let _ = env.set_int_array_region(&array, 0, &[width as jint, height as jint]);
        array
    }

    /// 把当前帧写进调用方给的 `IntArray`(长度必须 ≥ `w*h`)。
    ///
    /// 写入 **ARGB_8888** 打包像素,可直接交给
    /// `Bitmap.createBitmap(pixels, w, h, ARGB_8888)`。
    /// 返回写入的像素数;负值 = 失败。
    ///
    /// 转换缓冲是本线程的 scratch,**跨帧复用容量**:稳态下这条路径零分配
    /// (见 [`fill_argb`])。它只在 CPU 回退路径上调用 —— 零拷贝上屏时
    /// 壳根本不读像素。
    #[no_mangle]
    pub extern "system" fn Java_org_yuris_kernel_NativeHost_nativeCopyFrame<'local>(
        env: JNIEnv<'local>,
        _class: JClass<'local>,
        out: JIntArray<'local>,
    ) -> jint {
        ARGB_SCRATCH.with(|scratch| {
            let mut scratch = scratch.borrow_mut();
            let written = fill_argb(&mut scratch);
            if written == 0 {
                return -1;
            }
            let len = env.get_array_length(&out).unwrap_or(0) as usize;
            if len < written {
                set_error(format!(
                    "nativeCopyFrame 目标数组太小:需要 {written},收到 {len}"
                ));
                return -2;
            }
            match env.set_int_array_region(&out, 0, &scratch) {
                Ok(()) => written as jint,
                Err(error) => {
                    set_error(format!("nativeCopyFrame 写入失败: {error}"));
                    -3
                }
            }
        })
    }

    #[no_mangle]
    pub extern "system" fn Java_org_yuris_kernel_NativeHost_nativeSetInput<'local>(
        mut env: JNIEnv<'local>,
        _class: JClass<'local>,
        button: JString<'local>,
        pressed: jboolean,
    ) -> jint {
        let button: String = match env.get_string(&button) {
            Ok(text) => text.into(),
            Err(_) => return -1,
        };
        set_input(&button, pressed != 0) as jint
    }

    #[no_mangle]
    pub extern "system" fn Java_org_yuris_kernel_NativeHost_nativeSetCursor<'local>(
        _env: JNIEnv<'local>,
        _class: JClass<'_>,
        x: jint,
        y: jint,
    ) -> jint {
        set_cursor(i64::from(x), i64::from(y)) as jint
    }

    /// 从 JSON 配置启动(壳侧推荐入口;与 `yu-cli` 旗标一一对应)。
    ///
    /// 返回 `OK ...` 或 `ERR: ...`。新增能力只加 JSON 键,不改 JNI 签名。
    #[no_mangle]
    pub extern "system" fn Java_org_yuris_kernel_NativeHost_nativeInitJson<'local>(
        mut env: JNIEnv<'local>,
        _class: JClass<'local>,
        json: JString<'local>,
    ) -> JString<'local> {
        let json: String = match env.get_string(&json) {
            Ok(text) => text.into(),
            Err(error) => {
                let message = format!("配置无法读取: {error}");
                set_error(message.clone());
                return to_jstring(&mut env, format!("ERR: {message}"));
            }
        };
        match start_session_json(&json) {
            Ok(summary) => {
                clear_error();
                to_jstring(&mut env, summary)
            }
            Err(message) => {
                set_error(message.clone());
                to_jstring(&mut env, format!("ERR: {message}"))
            }
        }
    }

    /// 初始化 NDK 上下文(JavaVM + Application Context)。
    ///
    /// **必须在任何音频调用之前**由壳调用一次。oboe(cpal 的 Android 后端)
    /// 通过 `ndk-context` 取 JavaVM/Context 来创建 AudioTrack;不初始化就在
    /// 打开音频设备时 panic(`android context was not initialized`,已实测)。
    ///
    /// `activity` 传 Activity 实例即可(它的 Context 能拿到 AudioManager)。
    #[cfg(target_os = "android")]
    #[no_mangle]
    pub extern "system" fn Java_org_yuris_kernel_NativeHost_nativeInitAndroidContext<'local>(
        env: JNIEnv<'local>,
        _class: JClass<'local>,
        activity: jni::objects::JObject<'local>,
    ) -> jint {
        let java_vm = env.get_java_vm();
        let Ok(vm) = java_vm else {
            set_error("取 JavaVM 失败");
            return -1;
        };
        let raw_vm = vm.get_java_vm_pointer() as *mut std::ffi::c_void;
        let raw_context = activity.as_raw() as *mut std::ffi::c_void;
        // `initialize_android_context` 内部 `assert!(previous.is_none())` ——
        // 调两次会 panic。配置界面与游戏界面都会调用,所以这里自己去重。
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            // SAFETY: 句柄来自当前 JNIEnv/Activity 参数,进程内只在首次调用时
            // 交给 ndk-context;它只保存指针供 oboe 使用,不接管所有权。
            unsafe {
                ndk_context::initialize_android_context(raw_vm, raw_context);
            }
        });
        0
    }

    /// 绑定 Java 的 `Surface` 做零拷贝上屏。
    ///
    /// 必须在 `nativeInitJson` 之后、且 GPU 后端下调用;失败时壳继续用
    /// `nativeCopyFrame` 的 Bitmap 路径(不致命)。
    #[no_mangle]
    pub extern "system" fn Java_org_yuris_kernel_NativeHost_nativeAttachSurface<'local>(
        env: JNIEnv<'local>,
        _class: JClass<'local>,
        surface: jni::objects::JObject<'local>,
    ) -> jint {
        let raw_env = env.get_native_interface();
        let raw_surface = surface.as_raw();
        // SAFETY: `raw_env` 来自当前线程的 JNIEnv;`raw_surface` 是本调用
        // 参数里的 `android.view.Surface`,在调用期间有效。`attach_surface`
        // 内部只在构造期间使用这两个句柄。
        let result = attach_surface(|gpu| unsafe {
            super::present::NativePresenter::from_java_surface(raw_env, raw_surface, gpu)
                .map_err(|error| error.to_string())
        });
        match result {
            Ok(_) => 0,
            Err(message) => {
                set_error(message);
                -1
            }
        }
    }

    /// 窗口尺寸变化(壳在 `surfaceChanged` 里调用)。
    ///
    /// **可在任意线程调用**:`onSurfaceTextureSizeChanged` 跑在 UI 线程,
    /// 而 presenter 在渲染线程的会话里。这里只登记请求,渲染线程在下一帧
    /// 执行(见 [`request_resize`]);旧的直接 `SESSION.with` 在 UI 线程上
    /// 必然拿到空槽 ⇒ 尺寸变更被静默丢弃。
    #[no_mangle]
    pub extern "system" fn Java_org_yuris_kernel_NativeHost_nativeResizeSurface<'local>(
        _env: JNIEnv<'local>,
        _class: JClass<'local>,
        width: jint,
        height: jint,
    ) -> jint {
        request_resize(width.max(1) as u32, height.max(1) as u32)
    }
    /// 列出本机可用字体族(换行分隔;空 = 一个都没有)。
    #[no_mangle]
    pub extern "system" fn Java_org_yuris_kernel_NativeHost_nativeListFonts<'local>(
        mut env: JNIEnv<'local>,
        _class: JClass<'local>,
    ) -> JString<'local> {
        let families = list_font_families();
        to_jstring(&mut env, families.join("
"))
    }

    #[no_mangle]
    pub extern "system" fn Java_org_yuris_kernel_NativeHost_nativeStatus<'local>(
        mut env: JNIEnv<'local>,
        _class: JClass<'local>,
    ) -> JString<'local> {
        to_jstring(&mut env, status_text())
    }

    #[no_mangle]
    pub extern "system" fn Java_org_yuris_kernel_NativeHost_nativeVersion<'local>(
        mut env: JNIEnv<'local>,
        _class: JClass<'local>,
    ) -> JString<'local> {
        to_jstring(&mut env, version_tag())
    }

    /// 挂起会话(壳 `onPause`):停时钟、静音音频。返回 `0` 成功 / `-1` 未初始化。
    #[no_mangle]
    pub extern "system" fn Java_org_yuris_kernel_NativeHost_nativeSuspend<'local>(
        _env: JNIEnv<'local>,
        _class: JClass<'local>,
    ) -> jint {
        suspend()
    }

    /// 恢复会话(壳 `onResume`)。返回 `0` 成功 / `-1` 未初始化。
    #[no_mangle]
    pub extern "system" fn Java_org_yuris_kernel_NativeHost_nativeResume<'local>(
        _env: JNIEnv<'local>,
        _class: JClass<'local>,
    ) -> jint {
        resume()
    }

    /// 生命周期代号:0 running / 1 finished / 2 exited / 3 exited+restart / -1 未初始化。
    #[no_mangle]
    pub extern "system" fn Java_org_yuris_kernel_NativeHost_nativeLifecycle<'local>(
        _env: JNIEnv<'local>,
        _class: JClass<'local>,
    ) -> jint {
        lifecycle_code()
    }

    /// 销毁会话。**可从任意线程调用**:
    ///
    /// - 会话在本线程(渲染线程自己收尾)⇒ 立即释放,返回 0/-1 表示成败;
    /// - 会话不在本线程(壳在 `onDestroy` 的 UI 线程调用)⇒ 只登记请求
    ///   (见 [`request_shutdown`]),由渲染线程在下一帧执行,返回 0。
    ///
    /// 旧实现无条件走 `shutdown()`,在 UI 线程上必然拿到空槽 ⇒ 返回 -1
    /// 且**会话根本没释放**(退出时全靠线程析构兜底)。
    #[no_mangle]
    pub extern "system" fn Java_org_yuris_kernel_NativeHost_nativeShutdown<'local>(
        _env: JNIEnv<'local>,
        _class: JClass<'_>,
    ) -> jint {
        let owns_session = SESSION.with(|slot| slot.borrow().is_some());
        if owns_session {
            return if shutdown() { 0 } else { -1 };
        }
        request_shutdown()
    }

    /// 消费跨线程销毁请求(挂起空转时壳轮询;返回 `4` = 已释放会话)。
    #[no_mangle]
    pub extern "system" fn Java_org_yuris_kernel_NativeHost_nativeTakeShutdownRequest<'local>(
        _env: JNIEnv<'local>,
        _class: JClass<'_>,
    ) -> jint {
        take_shutdown_request()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_config_defaults_match_cli() {
        let config = RunConfig::from_json("{}").expect("空对象 = 全缺省");
        assert_eq!(config.entry, "SYSTEM_START", "与 CLI 缺省一致");
        assert!(config.os_sound, "@_OS_SOUND 对拍口径恒 true");
        assert!(config.audio_output, "默认真的出声");
        assert_eq!(config.fps, 60);
        assert_eq!(config.screen_color, 32);
        assert_eq!(config.save_codec, None, "None = 自动判定");
        assert!(!config.debug_mode);
        assert!(!config.media_readonly);
    }

    #[test]
    fn run_config_parses_every_cli_equivalent_key() {
        let json = r#"{
            "entry": "SYSTEM_TEST",
            "debug_mode": true,
            "script_key": "0x11223344",
            "font": "Noto Sans CJK JP",
            "os_sound": false,
            "audio": false,
            "fps": 30,
            "screen_color": 24,
            "dx_mode": 1,
            "save_codec": "snappy",
            "media_readonly": true,
            "record_events": true
        }"#;
        let config = RunConfig::from_json(json).expect("解析应成功");
        assert_eq!(config.entry, "SYSTEM_TEST");
        assert!(config.debug_mode);
        assert_eq!(config.script_key, Some([0x11, 0x22, 0x33, 0x44]));
        assert_eq!(config.force_font.as_deref(), Some("Noto Sans CJK JP"));
        assert!(!config.os_sound);
        assert!(!config.audio_output, "@_OS_SOUND 与出声解耦");
        assert_eq!(config.fps, 30);
        assert_eq!(config.screen_color, 24);
        assert_eq!(config.dx_mode, 1);
        assert_eq!(config.save_codec, Some(BlockCodec::Snappy));
        assert!(config.media_readonly);
        assert!(config.record_events);
    }

    #[test]
    fn run_config_parses_render_feature_switches() {
        let config = RunConfig::from_json(
            r#"{"data_root":"/g","prefetch":true,"snapshot_defer":true,
                "bilinear_scale":true,"text_gpu":false,"asset_cache_mb":256,
                "png_backend":"libpng","vm_profile":true,"asset_timing":true,
                "tick_split":true,"cg_timing":true,"readback_debug":true}"#,
        )
        .expect("解析应成功");
        assert!(config.tuning.prefetch);
        assert!(config.tuning.snapshot_defer);
        assert!(config.tuning.bilinear_scale);
        assert!(!config.tuning.text_gpu);
        assert_eq!(config.tuning.asset_cache_mb, 256);
        assert_eq!(
            config.tuning.png_backend,
            yu_render_core::PngBackend::Libpng
        );
        assert!(config.tuning.vm_profile);
        assert!(config.tuning.asset_timing);
        assert!(config.tuning.tick_split);
        assert!(config.tuning.cg_timing);
        assert!(config.tuning.readback_debug);
        // 缺省关闭:老壳不传键时行为与引入前一致。
        let default = RunConfig::from_json(r#"{"data_root":"/g"}"#).unwrap();
        assert!(!default.tuning.prefetch);
        assert!(!default.tuning.snapshot_defer);
        assert!(!default.tuning.bilinear_scale);
        assert_eq!(
            default.tuning.png_backend,
            yu_render_core::PngBackend::Image
        );
    }

    #[test]
    fn run_config_parses_backend_and_logging() {
        let config = RunConfig::from_json(
            r#"{"data_root":"/g","backend":"wgpu","log_file":"debug/run.jsonl","log_level":"verbose"}"#,
        )
        .expect("解析应成功");
        assert_eq!(config.backend, Backend::Wgpu);
        assert_eq!(config.log_file, Some(PathBuf::from("debug/run.jsonl")));
        assert_eq!(config.log_level, LogLevel::Verbose);
        // 缺省 = CPU + 不写日志文件。
        let default = RunConfig::from_json(r#"{"data_root":"/g"}"#).unwrap();
        assert_eq!(default.backend, Backend::Soft);
        assert_eq!(default.log_file, None);
        assert_eq!(default.log_level, LogLevel::Info);
    }

    /// 取值集合必须与 CLI `--log-level` 完全一致:`debug`/`trace` 曾被
    /// Android 侧单独拒收(两份解析漂移),现在共用一个解析器。
    #[test]
    fn run_config_log_level_matches_cli_value_set() {
        for value in ["quiet", "error", "warn", "info", "debug", "trace", "verbose"] {
            let json = format!(r#"{{"log_level":"{value}"}}"#);
            let config = RunConfig::from_json(&json)
                .unwrap_or_else(|error| panic!("`{value}` 应被接受: {error}"));
            assert_eq!(config.log_level, yu_log::LogLevel::parse(value).unwrap());
        }
        assert!(RunConfig::from_json(r#"{"log_level":"loud"}"#).is_err());
    }

    /// `debug`/`trace` 与 `verbose` 一样要退出发布模式(它们比缺省更松)。
    #[test]
    fn run_config_debug_levels_leave_quiet_mode() {
        for value in ["debug", "trace", "verbose"] {
            let json = format!(r#"{{"log_level":"{value}"}}"#);
            let config = RunConfig::from_json(&json).expect("解析应成功");
            assert!(!config.quiet, "`{value}` 应退出发布模式");
        }
        let warn = RunConfig::from_json(r#"{"log_level":"warn"}"#).unwrap();
        assert!(warn.quiet, "warn 不退出发布模式");
    }

    /// 文本编码路线与 CLI `--patch` 同一份词表(含本轮新增的 big5)。
    #[test]
    fn run_config_patch_matches_cli_value_set() {
        for value in ["auto", "original", "gbk", "big5", "tunnel", "cp950"] {
            let json = format!(r#"{{"patch":"{value}"}}"#);
            let config = RunConfig::from_json(&json).expect("解析应成功");
            assert_eq!(
                config.patch,
                yu_engine::patch::PatchKind::parse(value).expect("引擎侧同一份解析"),
            );
        }
        assert!(RunConfig::from_json(r#"{"patch":"euc-kr"}"#).is_err());
    }

    /// 后端取值与 CLI 同一份解析(大小写/空白容错也一致)。
    #[test]
    fn run_config_backend_aliases_match_engine_parser() {
        for value in ["soft", "wgpu", "SOFT", " Wgpu "] {
            let json = format!(r#"{{"backend":"{value}"}}"#);
            let config = RunConfig::from_json(&json).expect("解析应成功");
            assert_eq!(
                config.backend,
                Backend::parse(value).expect("引擎侧同一份解析"),
            );
        }
        assert!(RunConfig::from_json(r#"{"backend":"vulkan"}"#).is_err());
    }

    /// GPU 接口取值与引擎 `YURIS_WGPU_BACKEND` 同一份(`GpuBackend`)。
    #[test]
    fn run_config_gpu_backend_parses_through_shared_table() {
        for value in ["auto", "primary", "dx12", "vulkan", "gles", "gl", "all"] {
            let json = format!(r#"{{"gpu_backend":"{value}"}}"#);
            let config = RunConfig::from_json(&json).expect("解析应成功");
            assert_eq!(config.gpu_backend, GpuBackend::parse(value).unwrap());
        }
        assert!(RunConfig::from_json(r#"{"gpu_backend":"metal"}"#).is_err());
    }

    #[test]
    fn run_config_parses_path_keys() {
        let config = RunConfig::from_json(
            r#"{"data_root":"/sdcard/g","save_root":"/sdcard/g/sv"}"#,
        )
        .expect("解析应成功");
        assert_eq!(config.data_root, PathBuf::from("/sdcard/g"));
        assert_eq!(config.save_root, PathBuf::from("/sdcard/g/sv"));
    }

    #[test]
    fn run_config_rejects_bad_values() {
        assert!(RunConfig::from_json(r#"{"save_codec":"lz4"}"#).is_err());
        assert!(RunConfig::from_json(r#"{"script_key":"zz"}"#).is_err());
        assert!(RunConfig::from_json("[]").is_err(), "必须是对象");
        assert!(RunConfig::from_json("not json").is_err());
    }

    #[test]
    fn json_entry_requires_data_root() {
        let error = start_session_json(r#"{"entry":"SYSTEM_START"}"#).unwrap_err();
        assert!(error.contains("data_root"), "缺游戏目录要显式报错: {error}");
    }

    #[test]
    fn rgba_converts_to_argb_packing() {
        let rgba = [1u8, 2, 3, 4, 255, 0, 0, 255];
        let argb = rgba_to_argb(&rgba);
        assert_eq!(argb[0], 0x0401_0203, "A=4 R=1 G=2 B=3");
        assert_eq!(argb[1], 0xFFFF_0000, "不透明红");
    }

    /// `fill_argb` 是 `rgba_to_argb` 的零分配替身,二者必须同口径。
    #[test]
    fn fill_argb_without_session_is_empty() {
        // 无会话时:返回 0,不 panic,且必须清空 —— 否则壳会拿到上一帧的
        // 陈旧像素(看着像画面卡住)。
        let mut out: Vec<i32> = vec![7, 7, 7];
        assert_eq!(fill_argb(&mut out), 0);
        assert!(out.is_empty(), "无帧时必须清空而不是留下陈旧像素");
    }

    #[test]
    fn tick_before_init_is_reported_not_panicking() {
        // 未初始化:tick 返回 -1 并留下错误,不 panic、不 UB。
        assert_eq!(tick(), -1);
        assert!(status_text().contains("error="));
    }

    #[test]
    fn frame_size_and_status_are_safe_when_idle() {
        assert_eq!(frame_size(), (0, 0));
        assert!(frame_rgba().is_empty());
        assert!(status_text().contains("status=idle"));
        assert!(!shutdown(), "没有会话可释放");
    }
}
