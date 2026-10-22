//! Headless 首帧:boot ERIS → tick 到首个可绘制帧 → soft 渲染 → PNG + display JSON。
//!
//! 事件路径:**`GfxHost` 同步路径**。`GfxState` 在事件产生时同步消费,
//! `Vm::take_events()` 只用于统计事件数量(drain 仅防止有界队列丢事件),
//! **不会**再把同一批事件喂给 `GfxState`。
//!
//! 用法:
//!
//! ```text
//! set YURIS_SAMPLE_ERIS_YSBIN=<project-eris>/data/ysbin
//! set YURIS_SAMPLE_ERIS_YPF=<project-eris>/data.ypf      (可选)
//! cargo run -p yu-backend-soft --example first_frame
//! ```
//!
//! 输出:
//! - `target/tmp/first_frame.png`
//! - `target/tmp/first_frame.display.json`

use std::cell::RefCell;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use yu_backend_soft::{frame_has_drawable_output, png, SoftRenderer, TextureStore};
use yu_gfx::{json, GfxConfig, GfxHost, GfxState, Viewport};
use yu_rt::input::InputTimeline;
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource, Mount, YpfSource};

fn main() {
    if let Err(message) = run() {
        eprintln!("first_frame: {message}");
        std::process::exit(2);
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
        // 标题锚点用工程配置运行值 @_DEBUGMODE=0
        // (<project-eris>/save/data/config/projectconfig.txt DEBUGMODE=0);
        // debug_mode=true 在本语料会走 debug UI,1200 tick 内不进 title(黑帧)。
        // 证据与对照见 docs/title-motion-ledger.md §9。
        debug_mode: false,
        paths: host_paths(&root_path),
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
    // 取证用:强制渲染指定 tick(不改变默认的“首个可绘制帧”语义)。
    // 官方 title 是 boot 链跑完后的画面,不是首个结构可绘制帧;用它在同一
    // 锚点上比较 soft/wgpu/官方截图。
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
        // 只统计/drain;GfxHost 已经同步消费过同一批事件。
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
        println!(
            "warning: {limit} 帧内没有找到可绘制屏幕;渲染最后一帧,摘要会列出跳过项"
        );
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
    let mut backend = SoftRenderer::new(store);
    let first = backend.render_frame(&frame);
    if first.surface.width != frame.viewport.width
        || first.surface.height != frame.viewport.height
    {
        return Err(format!(
            "输出尺寸 {}x{} 与 viewport {}x{} 不一致",
            first.surface.width,
            first.surface.height,
            frame.viewport.width,
            frame.viewport.height
        ));
    }
    let png_a = png::png_bytes(&first.surface)
        .map_err(|e| format!("PNG 编码失败: {e}"))?;
    // 同一帧再渲染一次:确定性验收(缓存命中路径也必须逐字节一致)。
    let second = backend.render_frame(&frame);
    let png_b = png::png_bytes(&second.surface)
        .map_err(|e| format!("PNG 编码失败: {e}"))?;
    let deterministic = png_a == png_b;

    let out_dir = workspace_root().join("target").join("tmp");
    fs::create_dir_all(&out_dir)
        .map_err(|e| format!("创建 {} 失败: {e}", out_dir.display()))?;
    let png_path = out_dir.join("first_frame.png");
    let display_path = out_dir.join("first_frame.display.json");
    fs::write(&png_path, &png_a).map_err(|e| format!("写 {} 失败: {e}", png_path.display()))?;
    fs::write(&display_path, json::to_display_json(&frame))
        .map_err(|e| format!("写 {} 失败: {e}", display_path.display()))?;

    let report = &first.report;
    println!("event path: GfxHost sync (Vm::take_events drained for stats only)");
    println!("entry: SYSTEM_START; record_events=false");
    println!("frames ticked: {} (selected tick {tick})", ticks);
    println!(
        "stop rule: {} (limit {limit}); frame_has_drawable_output is structural, no pixel work",
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
    println!("skipped text: {} runs (no glyph rasterizer)", report.skipped_text);
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
        "events: drained={} dropped={}",
        drained_events,
        vm.events_dropped()
    );
    println!(
        "png: {} ({} bytes) deterministic={}",
        png_path.display(),
        png_a.len(),
        deterministic
    );
    println!("display json: {}", display_path.display());

    if !deterministic {
        return Err("同一帧两次渲染的 PNG 字节不一致".to_string());
    }
    Ok(())
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
        .expect("crates/yu-backend-soft -> workspace root")
        .to_path_buf()
}

/// 宿主注入的路径系统变量(手册 `basic/svariable.html`)。
///
/// - `$_SYSTEM_PATH` = YU-RIS 起動パス → 工程根(官方 `yu-ris.exe` 所在目录);
/// - `$_SAVE_PATH` = YU-RIS セーブデータパス → 官方引擎在本工程**实际生成**
///   的存档目录 `<root>/save/`(原版数据自证:`save/*.sd` 由官方 exe 写入);
/// - `$_APPDATA_PATH` = ユーザのホームディレクトリパス → `USERPROFILE`;
/// - `$_PERSONAL_PATH` = マイドキュメントのパス → `USERPROFILE\Documents`。
///
/// `$_PROJECT_PATH` 手册无页(DIV-RT-010),不猜值 ⇒ `None`(仅 debug 脚本读)。
fn host_paths(root: &std::path::Path) -> yu_rt::vm::HostPaths {
    let mut sys = root.to_string_lossy().replace('\\', "/");
    if !sys.ends_with('/') {
        sys.push('/');
    }
    let save = format!("{sys}save/");
    let home = std::env::var("USERPROFILE").ok();
    yu_rt::vm::HostPaths {
        system_path: Some(sys.clone().into_bytes()),
        save_path: Some(save.into_bytes()),
        appdata_path: home.as_ref().map(|h| h.clone().into_bytes()),
        personal_path: home
            .as_ref()
            .map(|h| format!("{h}\\Documents").into_bytes()),
        project_path: None,
    }
}
