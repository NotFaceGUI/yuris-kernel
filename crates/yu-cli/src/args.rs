//! 无依赖参数解析(`--name value` 与 `--name=value` 都接受;位置参数按序)。
//!
//! 只覆盖本 crate 的命令行;未知 `--flag` 显式报错,不静默忽略。

use std::collections::BTreeSet;

use crate::{CliResult, Failure};

pub struct ArgReader {
    tokens: Vec<String>,
    used: Vec<bool>,
}

impl ArgReader {
    pub fn new(tokens: Vec<String>) -> Self {
        let used = vec![false; tokens.len()];
        Self { tokens, used }
    }

    /// 第 `index` 个位置参数(跳过 `--flag` 与被 flag 消费的值)。
    pub fn positional(&mut self, index: usize) -> Option<String> {
        let mut seen = 0usize;
        for i in 0..self.tokens.len() {
            if self.used[i] || self.tokens[i].starts_with("--") {
                continue;
            }
            if seen == index {
                self.used[i] = true;
                return Some(self.tokens[i].clone());
            }
            seen += 1;
        }
        None
    }

    /// `--name value` / `--name=value`。
    pub fn flag(&mut self, name: &str) -> Option<String> {
        let eq = format!("--{name}=");
        for i in 0..self.tokens.len() {
            if self.used[i] {
                continue;
            }
            if let Some(value) = self.tokens[i].strip_prefix(&eq) {
                self.used[i] = true;
                return Some(value.to_string());
            }
            if self.tokens[i] == format!("--{name}") {
                self.used[i] = true;
                if i + 1 < self.tokens.len() && !self.tokens[i + 1].starts_with("--") {
                    self.used[i + 1] = true;
                    return Some(self.tokens[i + 1].clone());
                }
                return Some(String::new());
            }
        }
        None
    }

    /// 布尔开关:`--name` → `Some("")`(调用方视为 on),
    /// `--name=on|off` → `Some(value)`。
    ///
    /// 与 [`Self::flag`] 的区别:**不消费**下一个 token。这样
    /// `yu-cli run --vm-profile D:/game` 不会把工程根当成取值吃掉。
    pub fn toggle(&mut self, name: &str) -> Option<String> {
        let eq = format!("--{name}=");
        let bare = format!("--{name}");
        for i in 0..self.tokens.len() {
            if self.used[i] {
                continue;
            }
            if let Some(value) = self.tokens[i].strip_prefix(&eq) {
                self.used[i] = true;
                return Some(value.to_string());
            }
            if self.tokens[i] == bare {
                self.used[i] = true;
                return Some(String::new());
            }
        }
        None
    }

    /// 布尔开关。
    pub fn switch(&mut self, name: &str) -> bool {
        for i in 0..self.tokens.len() {
            if !self.used[i] && self.tokens[i] == format!("--{name}") {
                self.used[i] = true;
                return true;
            }
        }
        false
    }

    /// 未被消费的 `--flag` 一律报错(防止拼写错误静默生效)。
    pub fn finish(&self) -> CliResult<()> {
        let mut unknown = BTreeSet::new();
        for (i, token) in self.tokens.iter().enumerate() {
            if !self.used[i] && token.starts_with("--") {
                unknown.insert(token.split('=').next().unwrap_or(token).to_string());
            }
        }
        if unknown.is_empty() {
            Ok(())
        } else {
            Err(Failure::usage(format!(
                "未知参数:{}",
                unknown.into_iter().collect::<Vec<_>>().join(" ")
            )))
        }
    }

    /// 解析 `on|off`(大小写不敏感);其它值显式报错。
    pub fn on_off(name: &str, value: &str) -> CliResult<bool> {
        parse_on_off_flag(name, value)
    }
}

/// `--flag on|off|true|false|1|0` 的唯一解析(headless/desktop/serve 共用)。
///
/// 以前 [`ArgReader::on_off`] 与 `logging::parse_on_off` 是两份逐字相同的
/// 实现(只差 `trim` 与错误文案里有没有 `--`);现在只有这一份。
pub fn parse_on_off_flag(name: &str, value: &str) -> CliResult<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "on" | "true" | "1" => Ok(true),
        "off" | "false" | "0" => Ok(false),
        other => Err(Failure::usage(format!(
            "--{name} 只接受 on|off,收到 `{other}`"
        ))),
    }
}

/// `--save-codec`:强制存档块压缩层(`zlib|snappy`),`None` = 不改写自动判定。
///
/// 取值解析只有 [`yu_engine::engine::BlockCodec::parse`] 一份(headless /
/// desktop 两个宿主以前各写一段相同 match,Android 是第三段)。
pub fn save_codec_flag(
    value: Option<String>,
) -> CliResult<Option<yu_engine::engine::BlockCodec>> {
    match value {
        Some(value) => yu_engine::engine::BlockCodec::parse(&value)
            .map(Some)
            .map_err(|error| Failure::usage(format!("--save-codec: {error}"))),
        None => Ok(None),
    }
}

/// 宿主运行参数(渲染/诊断)。与 `backend`/`gpu_backend` 一样直接进
/// [`yu_engine::engine::EngineOptions`],不再要求用户先 export 环境变量。
#[derive(Clone, Debug, Default)]
pub struct HostTuning {
    pub render: yu_render_core::RenderTuning,
    pub gpu_backend: yu_backend_wgpu::GpuBackend,
    pub gpu_adapter: Option<String>,
}

/// 旧环境变量的布尔语义(保留兼容,仅在没有显式旗标时读取)。
#[derive(Clone, Copy)]
enum EnvBool {
    /// 只要设置过就是 on(`var_os(..).is_some()`)。
    Presence,
    /// 空串/0/off/false = off。
    EmptyOff,
    /// 0/off/false = off(空串 = on,用于默认开启的项)。
    ZeroOff,
}

fn env_bool(env: &str, style: EnvBool) -> Option<bool> {
    let raw = std::env::var(env).ok()?;
    let value = raw.trim().to_ascii_lowercase();
    Some(match style {
        EnvBool::Presence => true,
        EnvBool::EmptyOff => {
            !matches!(value.as_str(), "" | "0" | "off" | "false")
        }
        EnvBool::ZeroOff => !matches!(value.as_str(), "0" | "off" | "false"),
    })
}

fn bool_flag(
    args: &mut ArgReader,
    cli: &str,
    env: Option<(&str, EnvBool)>,
    default: bool,
) -> CliResult<bool> {
    if let Some(raw) = args.toggle(cli) {
        if raw.trim().is_empty() {
            return Ok(true);
        }
        return ArgReader::on_off(cli, &raw);
    }
    if let Some((env, style)) = env {
        if let Some(value) = env_bool(env, style) {
            return Ok(value);
        }
    }
    Ok(default)
}

fn uint_flag(
    args: &mut ArgReader,
    cli: &str,
    env: &str,
    default: usize,
) -> CliResult<usize> {
    if let Some(raw) = args.flag(cli) {
        return raw
            .trim()
            .parse::<usize>()
            .map_err(|_| Failure::usage(format!("--{cli} 需要非负整数,收到 `{raw}`")));
    }
    if let Ok(raw) = std::env::var(env) {
        if let Ok(value) = raw.trim().parse::<usize>() {
            return Ok(value);
        }
    }
    Ok(default)
}

fn text_flag(args: &mut ArgReader, cli: &str, env: &str) -> Option<String> {
    if let Some(raw) = args.flag(cli) {
        return Some(raw);
    }
    std::env::var(env)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

impl HostTuning {
    /// 解析所有渲染/诊断旗标(未给的项回退旧环境变量,再回退默认值)。
    pub fn from_args(args: &mut ArgReader) -> CliResult<Self> {
        use yu_backend_wgpu::GpuBackend;
        use yu_render_core::{PngBackend, RenderTuning};

        let mut render = RenderTuning::default();
        render.prefetch = bool_flag(
            args,
            "prefetch",
            Some(("YURIS_PREFETCH", EnvBool::EmptyOff)),
            render.prefetch,
        )?;
        render.prefetch_mb = uint_flag(
            args,
            "prefetch-mb",
            "YURIS_PREFETCH_MB",
            render.prefetch_mb,
        )?;
        render.prefetch_window = uint_flag(
            args,
            "prefetch-window",
            "YURIS_PREFETCH_WINDOW",
            render.prefetch_window,
        )?;
        render.asset_cache_mb = uint_flag(
            args,
            "asset-cache-mb",
            "YURIS_ASSET_CACHE_MB",
            render.asset_cache_mb,
        )?;
        render.asset_name_fallback = bool_flag(
            args,
            "asset-name-fallback",
            Some(("YURIS_ASSET_NAME_FALLBACK", EnvBool::ZeroOff)),
            render.asset_name_fallback,
        )?;
        render.bilinear_scale =
            bool_flag(args, "bilinear-scale", None, render.bilinear_scale)?;
        render.snapshot_defer = bool_flag(
            args,
            "snapshot-defer",
            Some(("YURIS_SNAPSHOT_DEFER", EnvBool::EmptyOff)),
            render.snapshot_defer,
        )?;
        render.text_base_cache = bool_flag(
            args,
            "text-base-cache",
            Some(("YURIS_TEXT_BASE_CACHE", EnvBool::ZeroOff)),
            render.text_base_cache,
        )?;
        render.text_gpu = bool_flag(
            args,
            "text-gpu",
            Some(("YURIS_TEXT_GPU", EnvBool::ZeroOff)),
            render.text_gpu,
        )?;
        render.pipeline_prewarm = bool_flag(
            args,
            "pipeline-prewarm",
            Some(("YURIS_PIPELINE_PREWARM", EnvBool::ZeroOff)),
            render.pipeline_prewarm,
        )?;
        render.png_backend = match text_flag(args, "png-backend", "YURIS_PNG_BACKEND") {
            Some(value) => PngBackend::parse(&value)
                .map_err(|error| Failure::usage(format!("--png-backend: {error}")))?,
            None => PngBackend::Image,
        };

        render.asset_timing = bool_flag(
            args,
            "asset-timing",
            Some(("YURIS_ASSET_TIMING", EnvBool::Presence)),
            false,
        )?;
        render.tick_split = bool_flag(
            args,
            "tick-split",
            Some(("YURIS_TICK_SPLIT", EnvBool::Presence)),
            false,
        )?;
        render.cg_timing = bool_flag(
            args,
            "cg-timing",
            Some(("YURIS_CG_TIMING", EnvBool::Presence)),
            false,
        )?;
        render.readback_debug = bool_flag(
            args,
            "readback-debug",
            Some(("YURIS_READBACK_WHY", EnvBool::Presence)),
            false,
        )?;
        render.trans_debug = bool_flag(
            args,
            "trans-debug",
            Some(("YURIS_TRANS_DEBUG", EnvBool::Presence)),
            false,
        )?;
        render.text_gpu_debug = bool_flag(
            args,
            "text-gpu-debug",
            Some(("YURIS_TEXT_GPU_DEBUG", EnvBool::Presence)),
            false,
        )?;
        render.render_timing = bool_flag(
            args,
            "render-timing",
            Some(("YU_RENDER_TIMING", EnvBool::Presence)),
            false,
        )?;
        render.render_trace_geometry = bool_flag(
            args,
            "render-trace-geometry",
            Some(("YU_RENDER_TRACE_GEOMETRY", EnvBool::Presence)),
            false,
        )?;
        render.p8_timing = bool_flag(
            args,
            "p8-timing",
            Some(("YU_P8_TIMING", EnvBool::Presence)),
            false,
        )?;
        render.font_trace = bool_flag(
            args,
            "font-trace",
            Some(("YU_FONT_TRACE", EnvBool::Presence)),
            false,
        )?;
        render.png_cmp = bool_flag(
            args,
            "png-cmp",
            Some(("YURIS_PNG_CMP", EnvBool::Presence)),
            false,
        )?;
        render.vm_profile = bool_flag(
            args,
            "vm-profile",
            Some(("YURIS_VM_PROFILE", EnvBool::Presence)),
            false,
        )?;

        let gpu_backend = match text_flag(args, "wgpu-backend", "YURIS_WGPU_BACKEND") {
            Some(value) => GpuBackend::parse(&value)
                .map_err(|error| Failure::usage(format!("--wgpu-backend: {error}")))?,
            None => GpuBackend::Auto,
        };
        let gpu_adapter = text_flag(args, "wgpu-adapter", "YURIS_WGPU_ADAPTER");

        Ok(Self {
            render,
            gpu_backend,
            gpu_adapter,
        })
    }

    /// 转成 CLI 旗标(只写非默认值;`yu-cli dev` 透传给 `yu-serve`)。
    pub fn to_cli_args(&self) -> Vec<String> {
        use yu_backend_wgpu::GpuBackend;

        fn push_flag(out: &mut Vec<String>, name: &str, on: bool, default: bool) {
            if on != default {
                out.push(if on {
                    format!("--{name}")
                } else {
                    format!("--{name}=off")
                });
            }
        }
        let defaults = yu_render_core::RenderTuning::default();
        let mut out = Vec::new();
        let r = &self.render;
        push_flag(&mut out, "prefetch", r.prefetch, defaults.prefetch);
        if r.prefetch_mb != defaults.prefetch_mb {
            out.push(format!("--prefetch-mb={}", r.prefetch_mb));
        }
        if r.prefetch_window != defaults.prefetch_window {
            out.push(format!("--prefetch-window={}", r.prefetch_window));
        }
        if r.asset_cache_mb != defaults.asset_cache_mb {
            out.push(format!("--asset-cache-mb={}", r.asset_cache_mb));
        }
        push_flag(
            &mut out,
            "asset-name-fallback",
            r.asset_name_fallback,
            defaults.asset_name_fallback,
        );
        push_flag(&mut out, "bilinear-scale", r.bilinear_scale, defaults.bilinear_scale);
        push_flag(&mut out, "snapshot-defer", r.snapshot_defer, defaults.snapshot_defer);
        push_flag(
            &mut out,
            "text-base-cache",
            r.text_base_cache,
            defaults.text_base_cache,
        );
        push_flag(&mut out, "text-gpu", r.text_gpu, defaults.text_gpu);
        push_flag(
            &mut out,
            "pipeline-prewarm",
            r.pipeline_prewarm,
            defaults.pipeline_prewarm,
        );
        if r.png_backend != defaults.png_backend {
            out.push(format!("--png-backend={}", r.png_backend.as_str()));
        }
        push_flag(&mut out, "asset-timing", r.asset_timing, false);
        push_flag(&mut out, "tick-split", r.tick_split, false);
        push_flag(&mut out, "cg-timing", r.cg_timing, false);
        push_flag(&mut out, "readback-debug", r.readback_debug, false);
        push_flag(&mut out, "trans-debug", r.trans_debug, false);
        push_flag(&mut out, "text-gpu-debug", r.text_gpu_debug, false);
        push_flag(&mut out, "render-timing", r.render_timing, false);
        push_flag(&mut out, "render-trace-geometry", r.render_trace_geometry, false);
        push_flag(&mut out, "p8-timing", r.p8_timing, false);
        push_flag(&mut out, "font-trace", r.font_trace, false);
        push_flag(&mut out, "png-cmp", r.png_cmp, false);
        push_flag(&mut out, "vm-profile", r.vm_profile, false);
        if self.gpu_backend != GpuBackend::default() {
            out.push(format!("--wgpu-backend={}", self.gpu_backend.as_str()));
        }
        if let Some(adapter) = &self.gpu_adapter {
            out.push(format!("--wgpu-adapter={adapter}"));
        }
        out
    }
}

/// `--help` 里的运行参数段(与 [`HostTuning::from_args`] 同一批旗标)。
pub const HOST_TUNING_HELP: &str = "\n诊断/性能参数(与 Android run-config.json 同名;环境变量仅为兼容别名):
  --prefetch[=on|off]           资产异步预取(默认 off)
  --prefetch-mb N               预取池预算 MiB(默认 128)
  --prefetch-window N           预取前瞻组数(默认 2048)
  --asset-cache-mb N            资产缓存预算 MiB(默认 512;0 = 不限制)
  --asset-name-fallback[=on|off] 编码不一致时的唯一同名回退(默认 on)
  --bilinear-scale[=on|off]     SIZE 也按双线性采样(默认 off)
  --snapshot-defer[=on|off]     COPY2 快照延迟物化(默认 off)
  --text-base-cache[=on|off]    文本基面 CPU 缓存(默认 on)
  --text-gpu[=on|off]           GPU 文本 pass(默认 on)
  --pipeline-prewarm[=on|off]   启动期管线预热(默认 on)
  --png-backend image|libpng    PNG 解码后端(默认 image;libpng 仅 Android)
  --wgpu-backend NAME           GPU 接口:auto|primary|dx12|vulkan|gl|all
  --wgpu-adapter NAME           适配器名称子串
  --vm-profile[=on|off]         VM opcode 逐条耗时归因
  --asset-timing[=on|off]       资产装载/预取计时
  --tick-split[=on|off]         tick 内部分段计时
  --cg-timing[=on|off]          CG 差分计时
  --readback-debug[=on|off]     GPU 读回原因
  --trans-debug[=on|off]        过渡合成取证
  --text-gpu-debug[=on|off]     GPU 文本失败/回退原因
  --render-timing[=on|off]      渲染阶段 timing
  --render-trace-geometry[=on|off] 渲染几何 trace
  --p8-timing[=on|off]          文本 P8 阶段 timing
  --font-trace[=on|off]         字体栅格 trace
  --png-cmp[=on|off]            PNG 双后端逐张对拍(Android libpng)
";

/// `--font` 的统一宿主映射:family 名 → `FontChoice::Family`;
/// 字体文件路径 → 读成字节 + `FontChoice::HostFont`。
///
/// 相对路径按工程根解析(字体拖进游戏目录后直接写文件名即可)。
/// 文件 IO 只发生在宿主侧;引擎只接收 `font_bytes` + `FontChoice`,
/// 与 Android 注入 assets 字体完全同一条路径。
pub fn font_options(
    root: &std::path::Path,
    value: Option<String>,
) -> CliResult<(yu_engine::engine::FontChoice, Vec<Vec<u8>>)> {
    use yu_engine::engine::FontChoice;

    let Some(value) = value.filter(|v| !v.trim().is_empty()) else {
        return Ok((FontChoice::Script, Vec::new()));
    };
    let value = value.trim();
    let path = std::path::Path::new(value);
    let lower = value.to_ascii_lowercase();
    let looks_like_file = path.is_file()
        || value.contains('/')
        || value.contains('\\')
        || [".ttf", ".otf", ".ttc", ".otc"]
            .iter()
            .any(|ext| lower.ends_with(ext));
    if !looks_like_file {
        return Ok((FontChoice::Family(value.to_string()), Vec::new()));
    }
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let bytes = std::fs::read(&resolved).map_err(|e| {
        Failure::usage(format!("读取字体 {} 失败: {e}", resolved.display()))
    })?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err(Failure::usage(format!(
            "字体 {} 超过 64MiB",
            resolved.display()
        )));
    }
    Ok((FontChoice::HostFont(0), vec![bytes]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reader(tokens: &[&str]) -> ArgReader {
        ArgReader::new(tokens.iter().map(|token| token.to_string()).collect())
    }

    #[test]
    fn toggle_does_not_eat_positional() {
        let mut args = reader(&["--vm-profile", "D:/game"]);
        assert_eq!(args.toggle("vm-profile").as_deref(), Some(""));
        assert_eq!(args.positional(0).as_deref(), Some("D:/game"));
    }

    #[test]
    fn host_tuning_parses_explicit_flags() {
        let mut args = reader(&[
            "--vm-profile",
            "--prefetch",
            "--text-gpu=off",
            "--asset-cache-mb",
            "256",
            "--png-backend=libpng",
            "--wgpu-backend=vulkan",
            "--wgpu-adapter=RTX",
        ]);
        let tuning = HostTuning::from_args(&mut args).expect("解析应成功");
        args.finish().expect("不应有未知参数");
        assert!(tuning.render.vm_profile);
        assert!(tuning.render.prefetch);
        assert!(!tuning.render.text_gpu);
        assert_eq!(tuning.render.asset_cache_mb, 256);
        assert_eq!(
            tuning.render.png_backend,
            yu_render_core::PngBackend::Libpng
        );
        assert_eq!(
            tuning.gpu_backend,
            yu_backend_wgpu::GpuBackend::Vulkan
        );
        assert_eq!(tuning.gpu_adapter.as_deref(), Some("RTX"));
    }

    #[test]
    fn host_tuning_rejects_bad_values() {
        let mut args = reader(&["--png-backend=bogus"]);
        assert!(HostTuning::from_args(&mut args).is_err());
        let mut args = reader(&["--asset-cache-mb=NaN"]);
        assert!(HostTuning::from_args(&mut args).is_err());
        let mut args = reader(&["--vm-profile=maybe"]);
        assert!(HostTuning::from_args(&mut args).is_err());
    }

    #[test]
    fn host_tuning_round_trips_non_default_flags() {
        let mut args = reader(&[
            "--prefetch",
            "--prefetch-mb=64",
            "--text-gpu=off",
            "--png-backend=libpng",
            "--vm-profile",
            "--wgpu-backend=gl",
        ]);
        let tuning = HostTuning::from_args(&mut args).unwrap();
        let forwarded = tuning.to_cli_args();
        let mut again = ArgReader::new(forwarded);
        let parsed = HostTuning::from_args(&mut again).unwrap();
        assert_eq!(parsed.render, tuning.render);
        assert_eq!(parsed.gpu_backend, tuning.gpu_backend);
        assert_eq!(parsed.gpu_adapter, tuning.gpu_adapter);
    }
}
