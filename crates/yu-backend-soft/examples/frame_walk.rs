//! 600 帧逐帧基线(任务 J3 P0):soft 后端。
//!
//! 输出 TSV 到 stdout:每个逻辑 tick 的 frame / ops / drawn / skipped /
//! transition / copy / image 计数 / RGBA sha256 / 渲染耗时(µs)。
//! 与 `yu-backend-wgpu` 的同名 example 使用完全相同的 tick 序列,
//! 供三端逐帧对拍(P3)。
//!
//! 用法:
//!
//! ```text
//! set YURIS_SAMPLE_ERIS_ROOT=<project-eris>
//! cargo run -p yu-backend-soft --example frame_walk -- [ticks]
//! ```

use std::cell::RefCell;
use std::env;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Instant;

use sha2::{Digest, Sha256};

use yu_backend_soft::{SoftRenderer, TextureStore};
use yu_gfx::{CompositorFrame, GfxConfig, GfxHost, GfxState, Op, Viewport};
use yu_rt::input::InputTimeline;
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource, Mount, YpfSource};

fn main() {
    if let Err(message) = run() {
        eprintln!("frame_walk(soft): {message}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let root = eris_root().ok_or_else(|| {
        "缺少样本:请设置 YURIS_SAMPLE_ERIS_ROOT 或 YURIS_SAMPLE_ERIS_YSBIN".to_string()
    })?;
    let root_path = PathBuf::from(&root);
    let data_dir = root_path.join("data");
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
        ..BootConfig::default()
    };
    let mut vm = Vm::boot_with_aux(src, Some(Box::new(aux)), boot)
        .map_err(|d| format!("boot 失败: {}", d.detail))?;
    let viewport = Viewport::new(
        vm.tables().boot.config.width,
        vm.tables().boot.config.height,
    );
    let state = Rc::new(RefCell::new(
        GfxState::from_tables(vm.tables(), GfxConfig::new(viewport).with_dx_mode(dx_mode()))
            .map_err(|d| format!("GfxState 构造失败: {}", d.detail))?,
    ));
    vm.set_graphics_host(Box::new(GfxHost::new(state.clone())));

    let mut mount = Mount::new(Codec::Cp932);
    if let Some(ypf) = ypf_path(&root_path) {
        mount.push(Box::new(
            YpfSource::open(&ypf, Codec::Cp932)
                .map_err(|d| format!("打开 {} 失败: {}", ypf.display(), d.detail))?,
        ));
    }
    if data_dir.is_dir() {
        mount.push(Box::new(
            DirSource::open(&data_dir, Codec::Cp932)
                .map_err(|d| format!("打开 {} 失败: {}", data_dir.display(), d.detail))?,
        ));
    }
    let mut renderer = SoftRenderer::new(TextureStore::new(
        Box::new(mount),
        Codec::Cp932,
        state.borrow().frame().image_probe_order.clone(),
    ));

    let ticks: u32 = env::var("YURIS_WALK_TICKS")
        .ok()
        .and_then(|v| v.parse().ok())
        .or_else(|| env::args().nth(1).and_then(|v| v.parse().ok()))
        .unwrap_or(600)
        .max(1);
    let timeline = InputTimeline::new();
    let dumps = dump_ticks();

    println!("tick\tframe\tops\tdrawn\tskipped\ttransition\tcopy\timage\thash\tms\tvisible\tlayers");
    for tick in 0..ticks {
        let _ = vm.tick_at(tick.saturating_mul(16), &timeline);
        let _ = vm.take_events();
        let frame = state.borrow().frame();
        let started = Instant::now();
        let output = renderer.render_frame(&frame);
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        let hash = hex(&Sha256::digest(&output.surface.rgba));
        let counts = counts(&frame);
        if dumps.contains(&tick) {
            let dir = workspace_root().join("target").join("tmp");
            let path = dir.join(format!("walk-soft-t{tick}.png"));
            let bytes = yu_backend_soft::png::png_bytes(&output.surface)
                .map_err(|e| format!("PNG 编码失败: {e}"))?;
            std::fs::write(&path, bytes).map_err(|e| format!("写 {} 失败: {e}", path.display()))?;
            let mut text = format!(
                "frame={} ops={} drawn={} skipped={}\n",
                output.report.frame,
                output.report.ops_total,
                output.report.ops_drawn,
                output.report.skipped_ops_total
            );
            for skip in &output.report.skipped {
                text.push_str(&format!(
                    "skip {:?} x{} {} {}\n",
                    skip.kind, skip.count, skip.reason, skip.target
                ));
            }
            let path = dir.join(format!("walk-soft-t{tick}.report.txt"));
            std::fs::write(&path, text).map_err(|e| format!("写 {} 失败: {e}", path.display()))?;
        }
        println!(
            "{tick}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.3}\t{}\t{}",
            frame.frame,
            output.report.ops_total,
            output.report.ops_drawn,
            output.report.skipped_ops_total,
            counts.0,
            counts.1,
            counts.2,
            hash,
            ms,
            output.report.visible_layers,
            output.report.layers,
        );
    }
    Ok(())
}

fn counts(frame: &CompositorFrame) -> (usize, usize, usize) {
    let mut transition = 0;
    let mut copy = 0;
    let mut image = 0;
    for pass in &frame.passes {
        for op in &pass.ops {
            match op {
                Op::Transition(_) => transition += 1,
                Op::Copy(_) => copy += 1,
                Op::Image(_) => image += 1,
                _ => {}
            }
        }
    }
    (transition, copy, image)
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn eris_root() -> Option<String> {
    if let Ok(root) = env::var("YURIS_SAMPLE_ERIS_ROOT") {
        return Some(root);
    }
    let ysbin = env::var("YURIS_SAMPLE_ERIS_YSBIN").ok()?;
    Some(Path::new(&ysbin).ancestors().nth(2)?.display().to_string())
}

fn ypf_path(root: &Path) -> Option<PathBuf> {
    if let Ok(path) = env::var("YURIS_SAMPLE_ERIS_YPF") {
        return Some(PathBuf::from(path));
    }
    let candidate = root.join("data.ypf");
    candidate.exists().then_some(candidate)
}

fn dump_ticks() -> Vec<u32> {
    env::var("YURIS_WALK_DUMP")
        .ok()
        .map(|value| {
            value
                .split(',')
                .filter_map(|item| item.trim().parse().ok())
                .collect()
        })
        .unwrap_or_default()
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates -> workspace root")
        .to_path_buf()
}

/// `_DXMODE` 的宿主注入(本示例用环境变量;CLI/wasm 由宿主参数传入)。
/// 0 = 固定管线(J3 基线);非 0 = 引擎 effect 转场路径。
fn dx_mode() -> u32 {
    std::env::var("YURIS_DX_MODE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}
