//! 文本链路跨后端对拍(P2 验收):soft vs wgpu 同帧逐像素一致。
//!
//! 与 `yu-backend-soft --example text_probe` 相同的 boot/时钟/点击序列,
//! 每 tick 把 `Vm::text_state()` 同步进 gfx,分别用 soft 与 wgpu 渲染并比较
//! SHA-256。输出 TSV + 不一致帧的 DP(第一个差异像素)。
//!
//! 用法:
//!
//! ```text
//! set YURIS_SAMPLE_ERIS_ROOT=<project-eris>
//! set YURIS_ENTRY_LABEL=SCENARIO_EPILOGUE
//! cargo run -p yu-backend-wgpu --example wgpu_text_probe -- 40
//! ```

use std::cell::RefCell;
use std::env;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use sha2::{Digest, Sha256};

use yu_backend_soft::{png, SoftRenderer, Surface, TextureStore};
use yu_backend_wgpu::WgpuRenderer;
use yu_gfx::{CompositorFrame, GfxConfig, GfxHost, GfxState, Viewport};
use yu_rt::input::{InputFrame, InputTimeline};
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DataSource, DirSource, Mount, YpfSource};

fn main() {
    if let Err(message) = run() {
        eprintln!("wgpu_text_probe: {message}");
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
    let click_interval: u32 = 10;
    let dump_every: u32 = 10;

    let mut timeline = InputTimeline::new();
    if entry.contains("RP_") {
        let mut click = click_interval;
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

    let src = open_source(&root_path)?;
    let aux = DirSource::open(&root_path, Codec::Cp932)
        .map_err(|d| format!("打开 {} 失败: {}", root_path.display(), d.detail))?;
    let boot = BootConfig {
        entry_label: entry.clone(),
        fps: 60,
        frame_ms: 16,
        screen_color: 32,
        record_events: false,
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
        GfxState::from_tables(vm.tables(), GfxConfig::new(viewport))
            .map_err(|d| format!("GfxState 构造失败: {}", d.detail))?,
    ));
    vm.set_graphics_host(Box::new(GfxHost::new(state.clone())));
    let probe_order = state.borrow().frame().image_probe_order.clone();

    let mut soft = SoftRenderer::new(TextureStore::new(
        Box::new(mount(&root_path)?),
        Codec::Cp932,
        probe_order.clone(),
    ));
    let mut wgpu = WgpuRenderer::new(TextureStore::new(
        Box::new(mount(&root_path)?),
        Codec::Cp932,
        probe_order,
    ))
    .map_err(|d| format!("GPU 初始化失败: {}", d.detail))?;

    println!("entry={entry} ticks={ticks}\ntick\tglyphs\tsoft_ink\twgpu_ink\tmatch\tsoft_hash\twgpu_hash");
    let mut mismatches = 0u32;
    for tick in 0..ticks {
        let result = vm.tick_at(tick.saturating_mul(16), &timeline);
        let _ = vm.take_events();
        let snapshot = vm.text_state();
        state.borrow_mut().sync_text_state(&snapshot);
        let frame = state.borrow().frame();
        let soft_out = soft.render_frame(&frame);
        let wgpu_out = wgpu
            .render_frame(&frame)
            .map_err(|d| format!("wgpu 渲染失败: {}", d.detail))?;
        let soft_hash = hex(&Sha256::digest(&soft_out.surface.rgba));
        let wgpu_hash = hex(&Sha256::digest(&wgpu_out.surface.rgba));
        let match_ = soft_hash == wgpu_hash;
        if !match_ {
            mismatches += 1;
        }
        let glyphs = frame.text_layout.as_ref().map_or(0, |l| l.glyphs.len());
        println!(
            "{tick}\t{}\t{}\t{}\t{}\t{}\t{}",
            glyphs,
            ink(&soft_out.surface),
            ink(&wgpu_out.surface),
            if match_ { "yes" } else { "NO" },
            soft_hash,
            wgpu_hash
        );
        if tick % dump_every == 0 || tick + 1 == ticks {
            dump(tick, &frame, &soft_out.surface, &wgpu_out.surface)?;
        }
        if !match_ {
            if let Some((x, y, a, b)) = first_diff(&soft_out.surface, &wgpu_out.surface) {
                println!(
                    "# diff t{tick} at ({x},{y}) soft={a:?} wgpu={b:?} report soft glyphs={} wgpu glyphs={}",
                    soft_out.report.text_glyphs, wgpu_out.report.text_glyphs
                );
            }
            if mismatches >= 5 {
                println!("# 前 5 个不一致帧已列出,停止");
                break;
            }
        }
        if result.main_finished {
            println!("# main finished at tick {tick}");
            break;
        }
    }
    println!("# mismatches={mismatches}");
    if mismatches > 0 {
        return Err(format!("soft/wgpu 有 {mismatches} 帧不一致"));
    }
    Ok(())
}

fn dump(tick: u32, frame: &CompositorFrame, soft: &Surface, wgpu: &Surface) -> Result<(), String> {
    let dir = workspace_root().join("target").join("tmp");
    std::fs::create_dir_all(&dir).map_err(|e| format!("建 {} 失败: {e}", dir.display()))?;
    for (name, surface) in [("soft", soft), ("wgpu", wgpu)] {
        let bytes = png::png_bytes(surface).map_err(|e| format!("PNG 编码失败: {e}"))?;
        std::fs::write(dir.join(format!("p2-text-{name}-t{tick}.png")), bytes)
            .map_err(|e| format!("写 PNG 失败: {e}"))?;
    }
    std::fs::write(
        dir.join(format!("p2-text-wgpu-t{tick}.json")),
        yu_gfx::json::to_display_json(frame),
    )
    .map_err(|e| format!("写 display.json 失败: {e}"))?;
    Ok(())
}

fn first_diff(a: &Surface, b: &Surface) -> Option<(u32, u32, [u8; 4], [u8; 4])> {
    if a.width != b.width || a.height != b.height {
        return None;
    }
    for y in 0..a.height {
        for x in 0..a.width {
            let pa = a.pixel(x, y)?;
            let pb = b.pixel(x, y)?;
            if pa != pb {
                return Some((x, y, pa, pb));
            }
        }
    }
    None
}

fn ink(surface: &Surface) -> u64 {
    surface
        .rgba
        .chunks_exact(4)
        .filter(|px| px[0] > 0 || px[1] > 0 || px[2] > 0)
        .count() as u64
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

fn open_source(root: &Path) -> Result<Box<dyn DataSource>, String> {
    match ypf_path(root) {
        Some(ypf) => Ok(Box::new(
            YpfSource::open(&ypf, Codec::Cp932)
                .map_err(|d| format!("打开 {} 失败: {}", ypf.display(), d.detail))?,
        )),
        None => Ok(Box::new(
            DirSource::open(root.join("data"), Codec::Cp932)
                .map_err(|d| format!("打开 data/ 失败: {}", d.detail))?,
        )),
    }
}

fn mount(root: &Path) -> Result<Mount, String> {
    let mut m = Mount::new(Codec::Cp932);
    if let Some(ypf) = ypf_path(root) {
        m.push(Box::new(YpfSource::open(&ypf, Codec::Cp932).map_err(
            |d| format!("打开 {} 失败: {}", ypf.display(), d.detail),
        )?));
    }
    if root.join("data").is_dir() {
        m.push(Box::new(
            DirSource::open(root.join("data"), Codec::Cp932)
                .map_err(|d| format!("打开 data/ 失败: {}", d.detail))?,
        ));
    }
    Ok(m)
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
