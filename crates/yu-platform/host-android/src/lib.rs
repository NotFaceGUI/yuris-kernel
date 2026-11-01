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

use std::cell::RefCell;
use std::path::PathBuf;

use yu_engine::engine::{Backend, Engine, EngineOptions};
use yu_engine::project::Project;

/// 一个运行会话(初始化成功到 shutdown)。
struct Session {
    engine: Engine,
    /// 最近一帧的尺寸与像素(Java 侧按需拉取;不跨界共享引用)。
    frame_w: u32,
    frame_h: u32,
    frame_pixels: Vec<u8>,
    ticks: u64,
}

/// 日志控制台句柄(文件日志开启时非空;`flush_logs` 用它把 JSONL 缓冲落盘)。
static LOG_CONSOLE: std::sync::Mutex<Option<std::sync::Arc<std::sync::Mutex<yu_log::console::Console>>>> =
    std::sync::Mutex::new(None);

thread_local! {
    /// 单线程会话槽(见模块文档「线程模型」)。
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
    /// 最近一次失败原因(供 `nativeStatus` 报告;成功初始化时清空)。
    static LAST_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
}

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
    // ---- 声音 ----
    /// `@_OS_SOUND` 注入值。缺省 true(对拍口径)。
    pub os_sound: bool,
    /// 是否真的接音频输出端口。缺省 true。`false` 只静音,不影响 `@_OS_SOUND`。
    pub audio_output: bool,
    // ---- 视频 ----
    /// 渲染后端:`"soft"`(CPU)/ `"wgpu"`(GPU)。缺省 soft。
    ///
    /// GPU 是增强路径:设备不支持 Vulkan/GLES 时**显式失败**,不自动降级
    /// (自动降级会让"GPU 模式画错"永远对不上号)。
    pub backend: String,
    /// GPU 底层接口:`"auto"`(默认)/ `"gles"` / `"vulkan"`。
    ///
    /// Android 模拟器的 gfxstream Vulkan 在 `vkQueueSubmit` 里空指针崩溃
    /// (实测 `vulkan.ranchu.so` SIGSEGV),所以 `auto` 先试 GLES,失败再试
    /// Vulkan。真机上两者都能用时也优先 GLES —— 对本引擎(纯 2D 图元 +
    /// 文本纹理)GLES 的驱动兼容性更好。
    pub gpu_backend: String,
    /// 视口像素格式代号(`@_SCREEN_COLOR`)。缺省 32。
    pub screen_color: u32,
    /// 渲染模式代号(`@_DX_MODE`)。缺省 0。
    pub dx_mode: u32,
    /// 每秒逻辑帧数。缺省 60。
    pub fps: u32,
    // ---- 存档 ---
    /// 存档块压缩层:`"zlib"` / `"snappy"`。缺省 = 自动判定。
    pub save_codec: Option<String>,
    /// 媒体只读(`@_MEDIA_READONLY`)。缺省 false。
    pub media_readonly: bool,
    // ---- 诊断 ---
    /// 记录事件流(占内存;对拍用)。缺省 false。
    pub record_events: bool,
    /// 调试日志文件(JSONL)。非空即开启;路径相对 `data_root` 时落游戏目录,
    /// 便于 adb pull 取回分析。缺省 `None` = 不写文件。
    pub log_file: Option<PathBuf>,
    /// 日志门槛:`"quiet"|"error"|"warn"|"info"|"verbose"`。缺省 `"info"`。
    pub log_level: String,
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
            font_file: None,
            os_sound: true,
            audio_output: true,
            screen_color: 32,
            dx_mode: 0,
            fps: 60,
            save_codec: None,
            media_readonly: false,
            record_events: false,
            backend: "soft".to_string(),
            gpu_backend: "auto".to_string(),
            log_file: None,
            log_level: "info".to_string(),
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
    pub fn from_json(text: &str) -> Result<Self, String> {
        let value = yu_engine::json::parse(text)?;
        let Some(object) = value.as_object() else {
            return Err("配置必须是 JSON 对象".to_string());
        };
        let mut config = Self::default();
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
        if let Some(v) = object.get("os_sound").and_then(|v| v.as_bool()) {
            config.os_sound = v;
        }
        if let Some(v) = object.get("audio").and_then(|v| v.as_bool()) {
            config.audio_output = v;
        }
        if let Some(v) = object.get("backend").and_then(|v| v.as_str()) {
            let lowered = v.trim().to_ascii_lowercase();
            if !lowered.is_empty() {
                if lowered != "soft" && lowered != "wgpu" {
                    return Err(format!("backend 只接受 soft|wgpu,收到 `{v}`"));
                }
                config.backend = lowered;
            }
        }
        if let Some(v) = object.get("gpu_backend").and_then(|v| v.as_str()) {
            let lowered = v.trim().to_ascii_lowercase();
            if !lowered.is_empty() {
                if !matches!(lowered.as_str(), "auto" | "gles" | "gl" | "vulkan") {
                    return Err(format!(
                        "gpu_backend 只接受 auto|gles|vulkan,收到 `{v}`"
                    ));
                }
                config.gpu_backend = lowered;
            }
        }
        if let Some(v) = object.get("log_file").and_then(|v| v.as_str()) {
            if !v.trim().is_empty() {
                config.log_file = Some(PathBuf::from(v));
            }
        }
        if let Some(v) = object.get("log_level").and_then(|v| v.as_str()) {
            let lowered = v.trim().to_ascii_lowercase();
            if !matches!(lowered.as_str(), "" | "quiet" | "error" | "warn" | "info" | "verbose")
            {
                return Err(format!(
                    "log_level 只接受 quiet|error|warn|info|verbose,收到 `{v}`"
                ));
            }
            if !lowered.is_empty() {
                config.log_level = lowered;
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
            let lowered = v.trim().to_ascii_lowercase();
            if !lowered.is_empty() {
                if lowered != "zlib" && lowered != "snappy" {
                    return Err(format!("save_codec 只接受 zlib|snappy,收到 `{v}`"));
                }
                config.save_codec = Some(lowered);
            }
        }
        if let Some(v) = object.get("media_readonly").and_then(|v| v.as_bool()) {
            config.media_readonly = v;
        }
        if let Some(v) = object.get("record_events").and_then(|v| v.as_bool()) {
            config.record_events = v;
        }
        Ok(config)
    }
}

/// `--script-key` 同口径:8 位十六进制,允许空白/下划线/冒号分隔与 `0x` 前缀。
fn parse_script_key(value: &str) -> Result<[u8; 4], String> {
    let cleaned: String = value
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '_' && *c != ':')
        .collect();
    let hex = cleaned
        .strip_prefix("0x")
        .or_else(|| cleaned.strip_prefix("0X"))
        .unwrap_or(&cleaned);
    if hex.len() != 8 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("script_key 需要 8 位十六进制(4 字节),收到 `{value}`"));
    }
    let mut key = [0u8; 4];
    for (index, slot) in key.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16)
            .map_err(|_| format!("script_key 解析失败:`{value}`"))?;
    }
    Ok(key)
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
    let (engine, summary) = start_engine(config, path_overrides)?;
    let viewport = engine.viewport();
    SESSION.with(|slot| {
        *slot.borrow_mut() = Some(Session {
            engine,
            frame_w: viewport.width,
            frame_h: viewport.height,
            frame_pixels: Vec::new(),
            ticks: 0,
        });
    });
    Ok(summary)
}

/// 按 `gpu_backend` 建 GPU 上下文,带**显式回退链**。
///
/// `auto` = GLES → Vulkan。之所以先 GLES:Android 模拟器的 gfxstream Vulkan
/// 在 `vkQueueSubmit` 内空指针崩溃(进程级 SIGSEGV,无法 catch),而 GLES
/// 路径在同一模拟器上稳定。真机两者都能跑时 GLES 也够用(本引擎是纯 2D
/// 图元 + 纹理合成,不吃 Vulkan 的现代特性)。
fn create_gpu_context(gpu_backend: &str) -> Result<yu_backend_wgpu::GpuContext, String> {
    use yu_backend_wgpu::DeviceOptions;
    let ladder: Vec<(&str, wgpu::Backends)> = match gpu_backend {
        "gles" | "gl" => vec![("gles", wgpu::Backends::GL)],
        "vulkan" => vec![("vulkan", wgpu::Backends::VULKAN)],
        _ => vec![("gles", wgpu::Backends::GL), ("vulkan", wgpu::Backends::VULKAN)],
    };
    let mut failures = Vec::new();
    for (name, backends) in ladder {
        let options = DeviceOptions {
            backends: Some(backends),
            ..DeviceOptions::default()
        };
        match yu_backend_wgpu::GpuContext::new(&options) {
            Ok(context) => {
                log_line(
                    "yu-host-android",
                    &format!(
                        "gpu: requested={name} actual={:?} adapter={}",
                        context.backend, context.adapter_info.name
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

/// 初始化文件日志(JSONL):`log_file` 指向的文件 + 门槛。
///
/// 与 CLI `--log-file` 同一条通道(`yu_log::facade` + `JsonlFile`),所以
/// Android 抓到的 JSONL 与桌面完全同格式,可直接用同一套工具分析。
fn init_file_logging(path: &std::path::Path, level: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("创建日志目录 {} 失败: {e}", parent.display()))?;
        }
    }
    let jsonl = yu_log::console::JsonlFile::create(path)
        .map_err(|e| format!("创建日志文件 {} 失败: {e}", path.display()))?;
    // 控制台输出走 logcat(Android 侧由壳读取);门面单例重复调用时保持现状。
    let level = yu_log::console::MinLevel::from_level_value(Some(level));
    let console = yu_log::console::Console::stderr_live(level, 0, Some(jsonl));
    // 门面要求 `Arc<dyn Fn..>`;Console 是日志单写者,用 Mutex 串行化。
    let handle = std::sync::Arc::new(std::sync::Mutex::new(console));
    let callback_handle = std::sync::Arc::clone(&handle);
    let callback: yu_log::facade::ConsoleFn = std::sync::Arc::new(
        move |level: yu_log::facade::FacadeLevel, channel: &str, body: &str| {
            if let Ok(mut console) = callback_handle.lock() {
                console.emit_facade(level, channel, body);
            }
        },
    );
    // 保存句柄:JSONL 走 `BufWriter`,不 flush 就会一直留在缓冲里
    // (实测:文件建出来了但 0 字节)。
    if let Ok(mut slot) = LOG_CONSOLE.lock() {
        *slot = Some(std::sync::Arc::clone(&handle));
    }
    let _ = yu_log::facade::init(callback);
    flush_logs();
    Ok(())
}

/// 把文件日志缓冲落盘(JSONL 是 `BufWriter`,需显式 flush)。
///
/// 调用点:`start_engine` 收尾、`shutdown`、以及壳每次取 `status` 时。
pub fn flush_logs() {
    if let Ok(slot) = LOG_CONSOLE.lock() {
        if let Some(console) = slot.as_ref() {
            if let Ok(mut console) = console.lock() {
                console.flush();
            }
        }
    }
}

/// 向文件日志写一行宿主标记(没有开启文件日志时静默忽略)。
fn log_line(target: &str, message: &str) {
    if let Ok(slot) = LOG_CONSOLE.lock() {
        if let Some(console) = slot.as_ref() {
            if let Ok(mut console) = console.lock() {
                console.emit_host(yu_log::console::HostLevel::Info, target, message);
            }
        }
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
    let path_overrides = yu_engine::host_paths::HostPathOverrides {
        appdata_path: Some(config.save_root.clone()),
        personal_path: Some(config.data_root.clone()),
        ..Default::default()
    };
    let (engine, summary) = start_engine(config, path_overrides)?;
    let viewport = engine.viewport();
    SESSION.with(|slot| {
        *slot.borrow_mut() = Some(Session {
            engine,
            frame_w: viewport.width,
            frame_h: viewport.height,
            frame_pixels: Vec::new(),
            ticks: 0,
        });
    });
    Ok(summary)
}

/// 按 [`RunConfig`] 启动引擎,返回 `(Engine, 摘要)`。
///
/// 所有运行参数都在这里落到 [`EngineOptions`];`path_overrides` 由调用方给
/// (桌面宿主可留默认)。
pub fn start_engine(
    config: RunConfig,
    path_overrides: yu_engine::host_paths::HostPathOverrides,
) -> Result<(Engine, String), String> {
    let RunConfig {
        data_root,
        save_root,
        entry,
        debug_mode,
        script_key,
        force_font,
        mut font_bytes,
        font_file,
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
    } = config;

    // 日志:非空 `log_file` 即开启。相对路径落游戏目录,便于 adb pull。
    // 门面是进程级单例,重复初始化保持首次(so 在进程内只应 init 一次)。
    let log_note = match &log_file {
        Some(path) => {
            let resolved = if path.is_absolute() {
                path.clone()
            } else {
                data_root.join(path)
            };
            match init_file_logging(&resolved, &log_level) {
                Ok(()) => format!(" log={}", resolved.display()),
                Err(error) => return Err(error),
            }
        }
        None => String::new(),
    };
    // 用户导入的字体文件:读不到就显式失败(不静默忽略 —— 否则用户会
    // 以为换上了其实没换)。
    if let Some(path) = font_file {
        let bytes = std::fs::read(&path)
            .map_err(|e| format!("字体读取失败 {}: {e}", path.display()))?;
        font_bytes.push(bytes);
    }
    // 存档根必须存在:缺目录会让 SAVE 与既读记录全部静默失败。
    std::fs::create_dir_all(&save_root)
        .map_err(|e| format!("创建存档目录失败 {}: {e}", save_root.display()))?;
    let project = Project::discover(&data_root, None)
        .map_err(|failure| format!("数据入口装配失败: {failure}"))?;
    let dataset = project.describe();
    // 存档块压缩层:与 CLI `--save-codec` 同义(只影响写盘)。
    let codec = match save_codec.as_deref() {
        Some("zlib") => Some(yu_engine::engine::BlockCodec::Zlib),
        Some("snappy") => Some(yu_engine::engine::BlockCodec::Snappy),
        _ => None,
    };
    // 后端:GPU 失败必须显式报错,不自动降级(自动降级 = 用户以为在看 GPU
    // 结果其实是 CPU)。
    let chosen_backend = match backend.as_str() {
        "wgpu" => Backend::Wgpu,
        _ => Backend::Soft,
    };
    // GPU 上下文由宿主建(引擎侧 `gpu_context` 注入口):这样才能控制
    // 底层接口与回退顺序,而不是交给进程级环境变量。
    let gpu_context = if chosen_backend == Backend::Wgpu {
        Some(create_gpu_context(&gpu_backend)?)
    } else {
        None
    };
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
        script_key,
        save_codec: codec,
        save_root: Some(save_root.clone()),
        path_overrides,
        font_bytes,
        force_font,
        ..EngineOptions::default()
    };
    let audio_note = if audio_output { "" } else { " audio=off" };
    let mut engine =
        Engine::new(project, options).map_err(|failure| format!("引擎启动失败: {failure}"))?;
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
        save_codec.as_deref().unwrap_or("auto"),
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
    Ok((engine, summary))
}


/// 推进一帧。返回 `0` 成功 / `-1` 未初始化 / `-2` tick 失败。
pub fn tick() -> i32 {
    SESSION.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(session) = slot.as_mut() else {
            set_error("nativeTick 在 nativeInit 之前被调用");
            return -1;
        };
        match session.engine.tick_realtime() {
            Ok(rendered) => {
                session.ticks += 1;
                session.frame_w = rendered.surface.width;
                session.frame_h = rendered.surface.height;
                session.frame_pixels = rendered.surface.rgba.clone();
                0
            }
            Err(failure) => {
                set_error(format!("tick 失败: {failure}"));
                -2
            }
        }
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
            .map(|session| session.frame_pixels.clone())
            .unwrap_or_default()
    })
}

/// RGBA → `ARGB_8888`(`0xAARRGGBB`),可直接交给
/// `Bitmap.createBitmap(pixels, w, h, ARGB_8888)`。
///
/// 换算放在内核侧只有一处,Java 壳不必再关心通道序。
pub fn rgba_to_argb(rgba: &[u8]) -> Vec<u32> {
    rgba.chunks_exact(4)
        .map(|px| {
            (u32::from(px[3]) << 24)
                | (u32::from(px[0]) << 16)
                | (u32::from(px[1]) << 8)
                | u32::from(px[2])
        })
        .collect()
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
                let report = session.engine.last_report().clone();
                let diags = session.engine.take_diags().len();
                out.push_str(&format!(
                    "status=running backend={} entry={} frame={} ticks={} viewport={}x{} \
                     layers={} ops={} skipped={} drawable={} boot_ms={:.1} diags={}                      tick_ms={:.2} render_ms={:.2}",
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
                ));
                // 渲染阶段拆解:`readback_us` 是 GPU→CPU 整帧读回的耗时,
                // 也是 Android 上能否跑满 60fps 的关键项。
                if let Some(phases) = session.engine.render_phases() {
                    out.push_str(&format!(
                        " phases[plan={} encode={} readback={} tail={}]us",
                        phases.plan_us, phases.encode_us, phases.readback_us, phases.tail_us
                    ));
                }
                if session.engine.main_finished() {
                    out.push_str(" main_finished=1");
                }
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

/// 释放会话(可再次 `init_session`)。返回是否真的释放了一个会话。
pub fn shutdown() -> bool {
    flush_logs();
    SESSION.with(|slot| slot.borrow_mut().take().is_some())
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
    #[no_mangle]
    pub extern "system" fn Java_org_yuris_kernel_NativeHost_nativeCopyFrame<'local>(
        env: JNIEnv<'local>,
        _class: JClass<'local>,
        out: JIntArray<'local>,
    ) -> jint {
        let rgba = frame_rgba();
        if rgba.is_empty() {
            return -1;
        }
        let packed = rgba_to_argb(&rgba);
        let len = env.get_array_length(&out).unwrap_or(0) as usize;
        if len < packed.len() {
            set_error(format!(
                "nativeCopyFrame 目标数组太小:需要 {},收到 {len}",
                packed.len()
            ));
            return -2;
        }
        let raw: Vec<jint> = packed.iter().map(|px| *px as jint).collect();
        match env.set_int_array_region(&out, 0, &raw) {
            Ok(()) => packed.len() as jint,
            Err(error) => {
                set_error(format!("nativeCopyFrame 写入失败: {error}"));
                -3
            }
        }
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

    #[no_mangle]
    pub extern "system" fn Java_org_yuris_kernel_NativeHost_nativeShutdown<'local>(
        _env: JNIEnv<'local>,
        _class: JClass<'_>,
    ) -> jint {
        if shutdown() {
            0
        } else {
            -1
        }
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
        assert_eq!(config.save_codec.as_deref(), Some("snappy"));
        assert!(config.media_readonly);
        assert!(config.record_events);
    }

    #[test]
    fn run_config_parses_backend_and_logging() {
        let config = RunConfig::from_json(
            r#"{"data_root":"/g","backend":"wgpu","log_file":"debug/run.jsonl","log_level":"verbose"}"#,
        )
        .expect("解析应成功");
        assert_eq!(config.backend, "wgpu");
        assert_eq!(config.log_file, Some(PathBuf::from("debug/run.jsonl")));
        assert_eq!(config.log_level, "verbose");
        // 缺省 = CPU + 不写日志文件。
        let default = RunConfig::from_json(r#"{"data_root":"/g"}"#).unwrap();
        assert_eq!(default.backend, "soft");
        assert_eq!(default.log_file, None);
        assert_eq!(default.log_level, "info");
    }

    #[test]
    fn run_config_rejects_unknown_backend() {
        assert!(RunConfig::from_json(r#"{"backend":"vulkan"}"#).is_err());
        assert!(RunConfig::from_json(r#"{"log_level":"trace"}"#).is_err());
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
