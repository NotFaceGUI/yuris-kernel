//! 文本链路探针(P2 验收):boot 到文本标签 → `_TEXT_FLAG` 排版 → soft 渲染。
//!
//! 方法照抄 `crates/yu-rt/docs/coverage-matrix.md` §7(P0):
//!
//! ```text
//! SCENARIO_EPILOGUE(s129): 30 tick, 1 行「エピローグ。」
//! RP_101(s133): 每 10 tick 点击, 10 行文本逐行推进
//! ```
//!
//! 关键点:VM 的 `_TEXT_FLAG` 只在 `Vm::text_state()` 里;宿主每帧同步到 gfx
//! (`GfxState::sync_text_state`),gfx 按逐字 `'1'/'2'` 排版,后端只栅格化。
//!
//! 用法:
//!
//! ```text
//! set YURIS_SAMPLE_ERIS_ROOT=<project-eris>
//! cargo run -p yu-backend-soft --example text_probe -- [ticks]
//! YURIS_ENTRY_LABEL=RP_101 cargo run -p yu-backend-soft --example text_probe -- 130
//! ```
//!
//! 输出 TSV(stdout)+ `target/tmp/p2-text-soft-t<tick>.{png,json,report.txt}`。

use std::cell::RefCell;
use std::env;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use sha2::{Digest, Sha256};

use yu_backend_soft::{png, SoftRenderer, Surface, TextureStore};
use yu_gfx::{CompositorFrame, GfxConfig, GfxHost, GfxState, Viewport};
use yu_rt::event::VmEvent;
use yu_rt::input::{InputFrame, InputTimeline};
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource, Mount, YpfSource};

fn main() {
    if let Err(message) = run() {
        eprintln!("text_probe(soft): {message}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let root = eris_root().ok_or_else(|| {
        "缺少样本:请设置 YURIS_SAMPLE_ERIS_ROOT 或 YURIS_SAMPLE_ERIS_YSBIN".to_string()
    })?;
    let root_path = PathBuf::from(&root);
    let entry = env::var("YURIS_ENTRY_LABEL").unwrap_or_else(|_| "SCENARIO_EPILOGUE".to_string());
    let default_ticks = if entry.contains("RP_") { 130 } else { 40 };
    let ticks: u32 = env::var("YURIS_TEXT_TICKS")
        .ok()
        .and_then(|v| v.parse().ok())
        .or_else(|| env::args().nth(1).and_then(|v| v.parse().ok()))
        .unwrap_or(default_ticks)
        .max(1);
    let click_interval: u32 = env::var("YURIS_TEXT_CLICK")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10)
        .max(1);
    let dump_every: u32 = env::var("YURIS_TEXT_DUMP")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10)
        .max(1);

    let mut timeline = InputTimeline::new();
    let mut click = click_interval;
    // 只有多行场景需要周期点击;单行场景保持画面常驻(§7 的 30 tick)。
    if entry.contains("RP_") {
        while click < ticks {
            timeline.set(
                u64::from(click),
                InputFrame::new(["ENTER", "MOUSE_L"]).with_cursor(400, 300),
            );
            timeline.set(
                u64::from(click + 1),
                InputFrame::new(std::iter::empty::<&str>()).with_cursor(400, 300),
            );
            click += click_interval;
        }
    }

    let src: Box<dyn yu_vfs::DataSource> = match ypf_path(&root_path) {
        Some(ypf) => Box::new(
            YpfSource::open(&ypf, Codec::Cp932)
                .map_err(|d| format!("打开 {} 失败: {}", ypf.display(), d.detail))?,
        ),
        None => Box::new(
            DirSource::open(root_path.join("data"), Codec::Cp932)
                .map_err(|d| format!("打开 data/ 失败: {}", d.detail))?,
        ),
    };
    let aux = DirSource::open(&root_path, Codec::Cp932)
        .map_err(|d| format!("打开 {} 失败: {}", root_path.display(), d.detail))?;
    let boot = BootConfig {
        entry_label: entry.clone(),
        fps: 60,
        frame_ms: 16,
        screen_color: 32,
        record_events: false,
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
    let state = Rc::new(RefCell::new(
        GfxState::from_tables(vm.tables(), GfxConfig::new(viewport))
            .map_err(|d| format!("GfxState 构造失败: {}", d.detail))?,
    ));
    vm.set_graphics_host(Box::new(GfxHost::new(state.clone())));

    let mut mount = Mount::new(Codec::Cp932);
    if let Some(ypf) = ypf_path(&root_path) {
        mount.push(Box::new(YpfSource::open(&ypf, Codec::Cp932).map_err(
            |d| format!("打开 {} 失败: {}", ypf.display(), d.detail),
        )?));
    }
    if root_path.join("data").is_dir() {
        mount.push(Box::new(
            DirSource::open(root_path.join("data"), Codec::Cp932)
                .map_err(|d| format!("打开 data/ 失败: {}", d.detail))?,
        ));
    }
    let mut renderer = SoftRenderer::new(TextureStore::new(
        Box::new(mount),
        Codec::Cp932,
        state.borrow().frame().image_probe_order.clone(),
    ));

    println!(
        "entry={entry} ticks={ticks} click={click_interval}\n\
         tick\tbytes\tflags\tglyphs\tlast_x\tx_extent\treport_glyphs\tink\thash"
    );
    let mut total_text_events = 0u64;
    let mut last_text = Vec::new();
    let mut wrote = 0u32;
    for tick in 0..ticks {
        let result = vm.tick_at(tick.saturating_mul(16), &timeline);
        for ev in vm.take_events() {
            if let VmEvent::Text { bytes, .. } = ev {
                total_text_events += 1;
                last_text = bytes;
            }
        }
        // 权威 `_TEXT_FLAG` 流(VM 的 `[state+0x3c]`)。
        let snapshot = vm.text_state();
        state.borrow_mut().sync_text_state(&snapshot);
        let frame = state.borrow().frame();
        let output = renderer.render_frame(&frame);
        let hash = hex(&Sha256::digest(&output.surface.rgba));
        let layout = frame.text_layout.as_ref();
        let glyphs = layout.map_or(0, |l| l.glyphs.len());
        let last_x = layout.and_then(|l| l.glyphs.last()).map_or(0, |g| g.x);
        let x_extent = layout.map_or(0, |l| {
            l.glyphs
                .last()
                .map_or(0, |g| g.x + l.style.cell_width(g.full))
        });
        let ink = ink_pixels(&output.surface);
        println!(
            "{tick}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            snapshot.bytes.len(),
            snapshot.flags.len(),
            glyphs,
            last_x,
            x_extent,
            output.report.text_glyphs,
            ink,
            hash
        );
        if tick % dump_every == 0 || tick + 1 == ticks {
            dump(tick, &frame, &output.surface, &output.report, &mut wrote)?;
        }
        if result.main_finished {
            println!("# main finished at tick {tick}");
            break;
        }
    }
    println!(
        "# text_events={total_text_events} last_text={}",
        show(&last_text)
    );
    Ok(())
}

fn dump(
    tick: u32,
    frame: &CompositorFrame,
    surface: &Surface,
    report: &yu_backend_soft::RenderReport,
    wrote: &mut u32,
) -> Result<(), String> {
    let dir = workspace_root().join("target").join("tmp");
    std::fs::create_dir_all(&dir).map_err(|e| format!("建 {} 失败: {e}", dir.display()))?;
    let bytes = png::png_bytes(surface).map_err(|e| format!("PNG 编码失败: {e}"))?;
    std::fs::write(dir.join(format!("p2-text-soft-t{tick}.png")), bytes)
        .map_err(|e| format!("写 PNG 失败: {e}"))?;
    let json = yu_gfx::json::to_display_json(frame);
    std::fs::write(dir.join(format!("p2-text-soft-t{tick}.json")), &json)
        .map_err(|e| format!("写 display.json 失败: {e}"))?;
    let mut text = format!(
        "frame={} text_draws={} text_glyphs={} text_missing={} skipped_text={}\n",
        frame.frame,
        report.text_draws,
        report.text_glyphs,
        report.text_missing,
        report.skipped_text
    );
    text.push_str("text_diags=");
    text.push_str(&report.text_diags.join(","));
    text.push('\n');
    if let Some(layout) = &frame.text_layout {
        let extent = layout
            .glyphs
            .last()
            .map_or(0, |g| g.x + layout.style.cell_width(g.full));
        text.push_str(&format!(
            "layout target={:?} chars={} glyphs={} origin=({},{}) extent={}\n",
            layout.target.as_ref().map(|t| t.display()),
            layout.chars,
            layout.glyphs.len(),
            layout.origin.0,
            layout.origin.1,
            extent
        ));
    }
    std::fs::write(dir.join(format!("p2-text-soft-t{tick}.report.txt")), text)
        .map_err(|e| format!("写 report 失败: {e}"))?;
    *wrote += 1;
    Ok(())
}

/// 非背景像素数(黑底 + 白字验收;alpha 已由后端强制 255)。
fn ink_pixels(surface: &Surface) -> u64 {
    let mut n = 0u64;
    for px in surface.rgba.chunks_exact(4) {
        if px[0] > 0 || px[1] > 0 || px[2] > 0 {
            n += 1;
        }
    }
    n
}

fn show(bytes: &[u8]) -> String {
    let mut s = String::new();
    for &b in bytes.iter().take(64) {
        if (0x20..0x7f).contains(&b) {
            s.push(b as char);
        } else {
            s.push_str(&format!("\\x{b:02x}"));
        }
    }
    if bytes.len() > 64 {
        s.push('…');
    }
    s
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

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("crates/<group>/<crate> -> workspace root")
        .to_path_buf()
}

fn host_paths(root: &Path) -> yu_rt::vm::HostPaths {
    let mut sys = root.to_string_lossy().replace('\\', "/");
    if !sys.ends_with('/') {
        sys.push('/');
    }
    let save = format!("{sys}save/");
    let home = env::var("USERPROFILE").ok();
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
