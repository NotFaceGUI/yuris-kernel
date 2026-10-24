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
        None => Box::new(DirSource::open(&data_dir, Codec::Cp932).map_err(|d| {
            format!(
                "打开 {} 失败(且无 data.ypf): {}",
                data_dir.display(),
                d.detail
            )
        })?),
    };
    let aux = DirSource::open(&root_path, Codec::Cp932)
        .map_err(|d| format!("打开 {} 失败: {}", root_path.display(), d.detail))?;
    let boot = BootConfig {
        entry_label: "SYSTEM_START".to_string(),
        fps: 60,
        record_events: false,
        // 标题锚点用工程配置运行值 @_DEBUGMODE=0
        // (<project-eris>/save/data/config/projectconfig.txt DEBUGMODE=0);
        // `YURIS_DEBUG_MODE=1` 走 debug 分支对照(B4:debug=true 已可进 title,
        // DIV-L-001 复核;黑帧根因 = _MOUSE_X2 误判按钮,已修)。
        debug_mode: std::env::var("YURIS_DEBUG_MODE")
            .map(|v| v == "1")
            .unwrap_or(false),
        paths: host_paths(&root_path),
        ..BootConfig::default()
    };
    let mut vm = Vm::boot_with_aux(src, Some(Box::new(aux)), boot)
        .map_err(|d| format!("boot 失败: {}", d.detail))?;
    let viewport = Viewport::new(
        vm.tables().boot.config.width,
        vm.tables().boot.config.height,
    );
    let state = Rc::new(RefCell::new(
        GfxState::from_tables(
            vm.tables(),
            GfxConfig::new(viewport).with_dx_mode(dx_mode()),
        )
        .map_err(|d| format!("GfxState 构造失败: {}", d.detail))?,
    ));
    vm.set_graphics_host(Box::new(GfxHost::new(state.clone())));

    let mut mount = Mount::new(Codec::Cp932);
    if let Some(ypf) = ypf_path(&root_path) {
        mount.push(Box::new(YpfSource::open(&ypf, Codec::Cp932).map_err(
            |d| format!("打开 {} 失败: {}", ypf.display(), d.detail),
        )?));
    }
    if data_dir.is_dir() {
        mount.push(Box::new(DirSource::open(&data_dir, Codec::Cp932).map_err(
            |d| format!("打开 {} 失败: {}", data_dir.display(), d.detail),
        )?));
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
    let timeline = walk_timeline();
    let dumps = dump_ticks();

    println!(
        "tick\tframe\tops\tdrawn\tskipped\ttransition\tcopy\timage\thash\tms\tvisible\tlayers"
    );
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
            let json = yu_gfx::json::to_display_json(&frame);
            let path = dir.join(format!("walk-soft-t{tick}.json"));
            std::fs::write(&path, &json).map_err(|e| format!("写 {} 失败: {e}", path.display()))?;
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

/// 可选的点击时间线:`YURIS_WALK_CLICKS="frame:x:y,frame:x:y"`。
///
/// 与 `cases/*.json` 的 inputs 同语义(`yu-cli case` 的 build_timeline):
/// 按下发生在 frame、frame+1 松开;光标从该帧起保持
/// (`InputTimeline::effective`)。缺省 = 无输入(600 帧基线)。
fn walk_timeline() -> InputTimeline {
    use yu_rt::input::InputFrame;
    let spec = env::var("YURIS_WALK_CLICKS").unwrap_or_default();
    if spec.trim().is_empty() {
        return InputTimeline::new();
    }
    // frame → 按下集合;frame+1 → 松开(显式空集)。
    let mut presses: std::collections::BTreeMap<u64, Vec<String>> = Default::default();
    let mut cursors: std::collections::BTreeMap<u64, (i64, i64)> = Default::default();
    for item in spec.split(',') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        let parts: Vec<&str> = item.split(':').collect();
        let (Some(frame), Some(x), Some(y)) = (
            parts.first().and_then(|v| v.parse::<u64>().ok()),
            parts.get(1).and_then(|v| v.parse::<i64>().ok()),
            parts.get(2).and_then(|v| v.parse::<i64>().ok()),
        ) else {
            panic!("YURIS_WALK_CLICKS 项 `{item}` 应为 frame:x:y");
        };
        presses
            .entry(frame)
            .or_default()
            .push("MOUSE_L".to_string());
        presses.entry(frame + 1).or_default();
        cursors.insert(frame, (x, y));
    }
    let mut frames: std::collections::BTreeSet<u64> = presses.keys().copied().collect();
    frames.extend(cursors.keys().copied());
    let mut timeline = InputTimeline::new();
    let mut held: Vec<String> = Vec::new();
    let mut cursor = (0i64, 0i64);
    for frame in frames {
        if let Some(set) = presses.get(&frame) {
            held = set.clone();
        }
        if let Some(&(x, y)) = cursors.get(&frame) {
            cursor = (x, y);
        }
        timeline.set(
            frame,
            InputFrame::new(held.iter().cloned()).with_cursor(cursor.0, cursor.1),
        );
    }
    timeline
}

fn workspace_root() -> PathBuf {
    // 含 Cargo.toml 的**严格祖先** = workspace 根(包目录自身也有
    // Cargo.toml,要跳过;固定层数会在嵌套路径下停错)。
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .skip(1)
        .find(|dir| dir.join("Cargo.toml").is_file())
        .expect("workspace root with Cargo.toml")
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
