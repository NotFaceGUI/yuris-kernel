//! Headless 首帧:boot ERIS → wgpu 渲染 → PNG + GPU 报告 + 计数对账。
//!
//! 事件路径与 `yu-backend-soft` 的 `first_frame` 完全一致:**`GfxHost` 同步
//! 路径**;`Vm::take_events()` 只做计数/drain,不重复喂给 `GfxState`。
//!
//! 用法:
//!
//! ```text
//! set YURIS_SAMPLE_ERIS_YSBIN=<project-eris>/data/ysbin
//! set YURIS_WGPU_ADAPTER=5070        # 可选:适配器名称子串
//! set YURIS_WGPU_BACKEND=dx12        # 可选:dx12/vulkan/gl/primary/all
//! cargo run -p yu-backend-wgpu --example first_frame
//! ```
//!
//! 输出:
//! - `target/tmp/wgpu_first_frame.png`
//! - `target/tmp/wgpu_first_frame.report.json`
//!
//! 硬性对账:pass/op/executable/skipped 必须与 `target/tmp/first_frame.display.json`
//! 完全一致(文件存在时);输出尺寸 = viewport,且不是全黑。

use std::cell::RefCell;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Instant;

use yu_backend_soft::{frame_has_drawable_output, png};
use yu_backend_wgpu::device::code;
use yu_backend_wgpu::TextureStore;
use yu_backend_wgpu::{DeviceOptions, Surface, WgpuRenderer};
use yu_gfx::{GfxConfig, GfxHost, GfxState, Viewport};
use yu_rt::input::InputTimeline;
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource, Mount, YpfSource};

fn main() {
    match run() {
        Ok(()) => {}
        Err(message) => {
            eprintln!("wgpu first_frame: {message}");
            std::process::exit(2);
        }
    }
}

fn run() -> Result<(), String> {
    let root = eris_root().ok_or_else(|| {
        "缺少样本:请设置 YURIS_SAMPLE_ERIS_ROOT 或 YURIS_SAMPLE_ERIS_YSBIN".to_string()
    })?;
    let root_path = PathBuf::from(&root);
    let data_dir = root_path.join("data");

    // 数据入口两种合法形态:封包 data.ypf 优先,散盘 data/ 为备;哪个存在用哪个。
    let src: Box<dyn yu_vfs::DataSource> = match ypf_path(&root_path) {
        Some(ypf) => Box::new(
            YpfSource::open(&ypf, Codec::Cp932)
                .map_err(|d| format!("打开 {} 失败: {}", ypf.display(), d.detail))?,
        ),
        None => Box::new(
            DirSource::open(&data_dir, Codec::Cp932).map_err(|d| {
                format!("打开 {} 失败(且无 data.ypf): {}", data_dir.display(), d.detail)
            })?,
        ),
    };
    let aux = DirSource::open(&root_path, Codec::Cp932)
        .map_err(|d| format!("打开 {} 失败: {}", root_path.display(), d.detail))?;
    let boot = BootConfig {
        entry_label: "SYSTEM_START".to_string(),
        fps: 60,
        record_events: false,
        ..BootConfig::default()
    };
    let mut vm = Vm::boot_with_aux(src, Some(Box::new(aux)), boot)
        .map_err(|d| format!("boot 失败: {}", d.detail))?;
    let viewport = Viewport::new(
        vm.tables().boot.config.width,
        vm.tables().boot.config.height,
    );
    if viewport.width == 0 || viewport.height == 0 {
        return Err(format!(
            "工程 yscfg 视口非法: {}x{}",
            viewport.width, viewport.height
        ));
    }

    // 同步宿主:图形/窗口/文本/帧事件在产生时进入 GfxState。
    let state = Rc::new(RefCell::new(
        GfxState::from_tables(vm.tables(), GfxConfig::new(viewport))
            .map_err(|d| format!("GfxState 构造失败: {}", d.detail))?,
    ));
    vm.set_graphics_host(Box::new(GfxHost::new(state.clone())));

    let limit = env::var("YURIS_FIRST_FRAME_LIMIT")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(120)
        .max(1);
    // 取证用:强制渲染指定 tick(与 soft 示例同义;官方 title 不是首个
    // 结构可绘制帧,像素对照必须固定在同一 boot 锚点)。
    let forced_tick = env::var("YURIS_FIRST_FRAME_TICK")
        .ok()
        .and_then(|v| v.parse::<u32>().ok());
    let limit = forced_tick.map_or(limit, |t| limit.max(t.saturating_add(1)));
    let timeline = InputTimeline::new();
    let mut drained_events = 0usize;
    let mut ticks = 0u32;
    let mut selected_tick = None;
    let mut selected = None;
    let mut last = None;

    for tick in 0..limit {
        let _report = vm.tick_at(tick.saturating_mul(16), &timeline);
        drained_events += vm.take_events().len();
        ticks += 1;
        let frame = state.borrow().frame();
        let drawable = frame_has_drawable_output(&frame);
        last = Some(frame.clone());
        if forced_tick == Some(tick) || (forced_tick.is_none() && drawable) {
            selected_tick = Some(tick);
            selected = Some(frame);
            break;
        }
    }

    let frame = selected
        .or(last)
        .ok_or_else(|| "没有产出任何帧".to_string())?;
    let tick = selected_tick.unwrap_or(ticks.saturating_sub(1));
    if selected_tick.is_none() {
        println!("warning: {limit} 帧内没有找到可绘制屏幕;渲染最后一帧,摘要会列出跳过项");
    }

    // 纹理源:存在的形态入挂载栈;散盘在上,可覆盖封包同名资源。
    let mut mount = Mount::new(Codec::Cp932);
    if let Some(ypf) = ypf_path(&root_path) {
        let source = YpfSource::open(&ypf, Codec::Cp932)
            .map_err(|d| format!("打开 {} 失败: {}", ypf.display(), d.detail))?;
        mount.push(Box::new(source));
    }
    let loose_files = if data_dir.is_dir() {
        let loose = DirSource::open(&data_dir, Codec::Cp932)
            .map_err(|d| format!("打开 {} 失败: {}", data_dir.display(), d.detail))?;
        let count = loose.file_count();
        mount.push(Box::new(loose));
        count
    } else {
        0
    };

    let store = TextureStore::new(
        Box::new(mount),
        Codec::Cp932,
        frame.image_probe_order.clone(),
    );
    let options = device_options()?;
    let mut backend = match WgpuRenderer::new_with_options(store, &options) {
        Ok(backend) => backend,
        Err(diag) if diag.code == code::ADAPTER_UNAVAILABLE => {
            eprintln!("SKIP: 没有可用 GPU 适配器: {}", diag.detail);
            std::process::exit(3);
        }
        Err(diag) => return Err(format!("GPU 初始化失败 [{}]: {}", diag.code, diag.detail)),
    };

    let started = Instant::now();
    let first = backend
        .render_frame(&frame)
        .map_err(|d| format!("渲染失败 [{}]: {}", d.code, d.detail))?;
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;

    if first.surface.width != frame.viewport.width || first.surface.height != frame.viewport.height
    {
        return Err(format!(
            "输出尺寸 {}x{} 与 viewport {}x{} 不一致",
            first.surface.width, first.surface.height, frame.viewport.width, frame.viewport.height
        ));
    }
    let all_black = !has_non_black_pixel(&first.surface);
    if all_black {
        eprintln!(
            "warning: 首帧全黑(当前 boot 锚点未到 title/仅清屏);这不是 1:1 证据,\
             报告里记 all_black=true。用 YURIS_FIRST_FRAME_TICK 选非黑帧再对拍。"
        );
    }

    // 第二次渲染:缓存命中路径;GPU 结果应与首帧逐字节一致。
    let second = backend
        .render_frame(&frame)
        .map_err(|d| format!("第二次渲染失败 [{}]: {}", d.code, d.detail))?;
    let deterministic = first.surface.rgba == second.surface.rgba;

    let out_dir = workspace_root().join("target").join("tmp");
    fs::create_dir_all(&out_dir).map_err(|e| format!("创建 {} 失败: {e}", out_dir.display()))?;
    let png_path = out_dir.join("wgpu_first_frame.png");
    let report_path = out_dir.join("wgpu_first_frame.report.json");
    let display_path = out_dir.join("first_frame.display.json");
    let png_bytes = png::png_bytes(&first.surface).map_err(|e| format!("PNG 编码失败: {e}"))?;
    fs::write(&png_path, &png_bytes).map_err(|e| format!("写 {} 失败: {e}", png_path.display()))?;

    let check = match fs::read_to_string(&display_path) {
        Ok(json) => Some(check_display(&json, &frame.frame, &first)?),
        Err(err) => {
            println!(
                "warning: 无法读取 {} ({err});跳过 display JSON 对账",
                display_path.display()
            );
            None
        }
    };

    let report = &first.report;
    let stats = &first.stats;
    println!("event path: GfxHost sync (Vm::take_events drained for stats only)");
    println!("entry: SYSTEM_START; record_events=false");
    println!("frames ticked: {} (selected tick {tick})", ticks);
    println!(
        "stop rule: {} (limit {limit})",
        match forced_tick {
            Some(t) => format!("forced tick {t} (YURIS_FIRST_FRAME_TICK)"),
            None => "first frame with a drawable screen op".to_string(),
        }
    );
    println!(
        "frame: {} viewport: {}x{}",
        frame.frame, frame.viewport.width, frame.viewport.height
    );
    println!(
        "gpu: backend={} adapter={} type={} driver={}",
        stats.adapter.backend, stats.adapter.name, stats.adapter.device_type, stats.adapter.driver
    );
    println!(
        "layers: {} (visible {}), passes: {}, offscreen passes: {}",
        report.layers, report.visible_layers, report.passes, report.offscreen_passes
    );
    println!(
        "ops: {} total, {} drawn, {} skipped",
        report.ops_total, report.ops_drawn, report.skipped_ops_total
    );
    for skip in &report.skipped {
        println!(
            "  skip {:<10} x{:<4} {:<42} {}",
            skip.kind, skip.count, skip.reason, skip.target
        );
    }
    println!("missing assets: {}", report.missing_assets.len());
    for issue in &report.missing_assets {
        println!(
            "  asset {:<24} {:<20} {}",
            issue.key_display(),
            issue.code,
            issue.detail
        );
    }
    println!(
        "skipped text: {} runs (no glyph rasterizer)",
        report.skipped_text
    );
    println!(
        "filter: mag={} min={} mip={} source={}",
        report.filter_mag, report.filter_min, report.filter_mip, report.filter_source
    );
    println!("color space: {}", report.color_space);
    println!(
        "assets: probe={:?} loose_files={} ypf={}",
        frame
            .image_probe_order
            .iter()
            .map(|f| f.as_str())
            .collect::<Vec<_>>(),
        loose_files,
        ypf_path(&root_path).is_some()
    );
    println!(
        "textures: decoded={} cache_entries={}",
        backend.textures().decoded_count(),
        backend.textures().cache_len()
    );
    println!(
        "gpu counters: draws={} uploads={} upload_bytes={} pipelines={} (created {}) bind_groups={} (created {})",
        stats.draw_calls,
        stats.texture_uploads,
        stats.upload_bytes,
        stats.pipelines,
        stats.pipelines_created,
        stats.bind_groups,
        stats.bind_groups_created
    );
    println!(
        "frame bytes: vertices={} readback={} elapsed={:.3} ms deterministic={} all_black={}",
        stats.vertex_bytes, stats.readback_bytes, elapsed_ms, deterministic, all_black
    );
    println!(
        "events: drained={} dropped={}",
        drained_events,
        vm.events_dropped()
    );
    println!("png: {} ({} bytes)", png_path.display(), png_bytes.len());
    if let Some(check) = &check {
        println!(
            "display check: {} (passes {} ops {} executable {} skipped {})",
            if check.matches { "OK" } else { "MISMATCH" },
            check.passes,
            check.ops,
            check.executable,
            check.skipped
        );
    }

    write_report(
        &report_path,
        &frame,
        report,
        stats,
        elapsed_ms,
        deterministic,
        all_black,
        &png_path,
        check.as_ref(),
    )?;
    println!("report: {}", report_path.display());

    if !deterministic {
        return Err("同一帧两次 GPU 渲染的像素不一致".to_string());
    }
    if let Some(check) = check {
        if !check.matches {
            return Err(format!(
                "计数对账失败: wgpu(passes={} ops={} executable={} skipped={} layers={}) \
                 display(passes={} ops={} executable={} skipped={} layers={})",
                report.passes,
                report.ops_total,
                report.ops_drawn,
                report.skipped_ops_total,
                report.layers,
                check.passes,
                check.ops,
                check.executable,
                check.skipped,
                check.layers
            ));
        }
    }
    Ok(())
}

fn device_options() -> Result<DeviceOptions, String> {
    let mut options = DeviceOptions::default();
    if let Ok(name) = env::var("YURIS_WGPU_ADAPTER") {
        let name = name.trim();
        if !name.is_empty() {
            options.adapter_name = Some(name.to_string());
        }
    }
    if let Ok(backend) = env::var("YURIS_WGPU_BACKEND") {
        let backend = backend.trim().to_ascii_lowercase();
        options.backends = Some(match backend.as_str() {
            "" | "primary" => wgpu::Backends::PRIMARY,
            "all" => wgpu::Backends::all(),
            "dx12" => wgpu::Backends::DX12,
            "vulkan" => wgpu::Backends::VULKAN,
            "gl" => wgpu::Backends::GL,
            other => return Err(format!("未知 YURIS_WGPU_BACKEND={other}")),
        });
    }
    Ok(options)
}

fn has_non_black_pixel(surface: &Surface) -> bool {
    surface
        .rgba
        .chunks_exact(4)
        .any(|px| px[0] != 0 || px[1] != 0 || px[2] != 0)
}

struct DisplayCheck {
    matches: bool,
    passes: usize,
    ops: usize,
    executable: usize,
    skipped: usize,
    layers: usize,
}

fn check_display(
    json: &str,
    expected_frame: &u64,
    output: &yu_backend_wgpu::WgpuRenderOutput,
) -> Result<DisplayCheck, String> {
    let frame = extract_number(json, "\"frame\":").ok_or("display JSON 缺少 frame")?;
    let width = extract_number(json, "\"width\":").ok_or("display JSON 缺少 width")?;
    let height = extract_number(json, "\"height\":").ok_or("display JSON 缺少 height")?;
    let layers = extract_number(json, "\"layer_count\":").ok_or("display JSON 缺少 layer_count")?;
    let passes = count_occurrences(json, "\"target\":{");
    let ops = count_occurrences(json, "\"op\":\"");
    let executable = count_occurrences(json, "\"executable\":true");
    let skipped = count_occurrences(json, "\"executable\":false");

    let report = &output.report;
    let matches = frame == *expected_frame
        && width == u64::from(output.surface.width)
        && height == u64::from(output.surface.height)
        && layers as usize == report.layers
        && passes == report.passes
        && ops == report.ops_total
        && executable == report.ops_drawn
        && skipped as u32 == report.skipped_ops_total;
    Ok(DisplayCheck {
        matches,
        passes,
        ops,
        executable,
        skipped,
        layers: layers as usize,
    })
}

fn extract_number(json: &str, needle: &str) -> Option<u64> {
    let start = json.find(needle)? + needle.len();
    let rest = &json[start..];
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

fn count_occurrences(haystack: &str, needle: &str) -> usize {
    haystack.match_indices(needle).count()
}

#[allow(clippy::too_many_arguments)]
fn write_report(
    path: &Path,
    frame: &yu_gfx::CompositorFrame,
    report: &yu_backend_wgpu::RenderReport,
    stats: &yu_backend_wgpu::GpuStats,
    elapsed_ms: f64,
    deterministic: bool,
    all_black: bool,
    png_path: &Path,
    check: Option<&DisplayCheck>,
) -> Result<(), String> {
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str(&format!("  \"frame\": {},\n", frame.frame));
    out.push_str(&format!(
        "  \"viewport\": {{\"width\": {}, \"height\": {}}},\n",
        frame.viewport.width, frame.viewport.height
    ));
    out.push_str(&format!(
        "  \"gpu\": {{\"backend\": \"{}\", \"adapter\": \"{}\", \"device_type\": \"{}\", \"driver\": \"{}\"}},\n",
        escape(&stats.adapter.backend),
        escape(&stats.adapter.name),
        escape(&stats.adapter.device_type),
        escape(&stats.adapter.driver)
    ));
    out.push_str(&format!("  \"elapsed_ms\": {:.3},\n", elapsed_ms));
    out.push_str(&format!("  \"deterministic\": {},\n", deterministic));
    out.push_str(&format!("  \"all_black\": {},\n", all_black));
    out.push_str(&format!(
        "  \"counts\": {{\"passes\": {}, \"layers\": {}, \"ops_total\": {}, \"ops_drawn\": {}, \"skipped\": {}, \"offscreen_passes\": {}}},\n",
        report.passes,
        report.layers,
        report.ops_total,
        report.ops_drawn,
        report.skipped_ops_total,
        report.offscreen_passes
    ));
    out.push_str(&format!(
        "  \"gpu_counters\": {{\"draw_calls\": {}, \"texture_uploads\": {}, \"upload_bytes\": {}, \"total_texture_uploads\": {}, \"total_upload_bytes\": {}, \"pipelines\": {}, \"pipelines_created\": {}, \"bind_groups\": {}, \"bind_groups_created\": {}, \"cached_textures\": {}, \"vertex_bytes\": {}, \"readback_bytes\": {}}},\n",
        stats.draw_calls,
        stats.texture_uploads,
        stats.upload_bytes,
        stats.total_texture_uploads,
        stats.total_upload_bytes,
        stats.pipelines,
        stats.pipelines_created,
        stats.bind_groups,
        stats.bind_groups_created,
        stats.cached_textures,
        stats.vertex_bytes,
        stats.readback_bytes
    ));
    out.push_str(&format!(
        "  \"png\": \"{}\",\n",
        escape(&png_path.display().to_string())
    ));
    match check {
        Some(check) => out.push_str(&format!(
            "  \"display_check\": {{\"matches\": {}, \"passes\": {}, \"ops\": {}, \"executable\": {}, \"skipped\": {}, \"layers\": {}}}\n",
            check.matches, check.passes, check.ops, check.executable, check.skipped, check.layers
        )),
        None => out.push_str("  \"display_check\": null\n"),
    }
    out.push_str("}\n");
    fs::write(path, out).map_err(|e| format!("写 {} 失败: {e}", path.display()))
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn eris_root() -> Option<String> {
    if let Ok(root) = env::var("YURIS_SAMPLE_ERIS_ROOT") {
        return Some(root);
    }
    let ysbin = env::var("YURIS_SAMPLE_ERIS_YSBIN").ok()?;
    // 唯一约定:工程数据入口是 <工程根>/data;ysbin 位于 <工程根>/data/ysbin。
    Some(Path::new(&ysbin).ancestors().nth(2)?.display().to_string())
}

fn ypf_path(root: &Path) -> Option<PathBuf> {
    if let Ok(path) = env::var("YURIS_SAMPLE_ERIS_YPF") {
        return Some(PathBuf::from(path));
    }
    let candidate = root.join("data.ypf");
    candidate.exists().then_some(candidate)
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/yu-backend-wgpu -> workspace root")
        .to_path_buf()
}
