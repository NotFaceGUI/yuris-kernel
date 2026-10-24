//! 诊断:打印 CG / CGACT / CGEND 命令的求值参数(定位 CGS 栈生命周期)。
//!
//! 用法:
//! ```text
//! set YURIS_SAMPLE_ERIS_ROOT=<project-eris>
//! cargo run -p yu-backend-soft --example cg_trace -- [from] [to] [frame,x,y ...]
//! ```
//!
//! 只输出 `CG`/`CGACT`/`CGEND` 三类命令与帧末仍存活的 `CGS*` 图层;
//! 用于和官方 `probe_dispatch.py` 的同类 trace 对拍(DIV-RT-050)。

use std::cell::RefCell;
use std::env;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use yu_backend_soft::{SoftRenderer, TextureStore};
use yu_gfx::{GfxConfig, GfxHost, GfxState, Viewport};
use yu_rt::event::VmEvent;
use yu_rt::input::{InputFrame, InputTimeline};
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource, Mount, YpfSource};

fn main() {
    if let Err(message) = run() {
        eprintln!("cg_trace: {message}");
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
        record_events: true,
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

    let from: u32 = env::args()
        .nth(1)
        .and_then(|v| v.parse().ok())
        .unwrap_or(450);
    let to: u32 = env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(545);
    let clicks: Vec<(u64, i64, i64)> = env::args()
        .skip(3)
        .filter_map(|spec| {
            let p: Vec<&str> = spec.split(',').collect();
            if p.len() != 3 {
                return None;
            }
            Some((p[0].parse().ok()?, p[1].parse().ok()?, p[2].parse().ok()?))
        })
        .collect();
    let mut timeline = InputTimeline::new();
    for &(f, x, y) in &clicks {
        if f > 0 {
            timeline.set(
                f - 1,
                InputFrame::new(std::iter::empty::<&str>()).with_cursor(x, y),
            );
        }
        timeline.set(f, InputFrame::new(["MOUSE_L"]).with_cursor(x, y));
        timeline.set(
            f + 1,
            InputFrame::new(std::iter::empty::<&str>()).with_cursor(x, y),
        );
    }

    for tick in 0..=to {
        let _ = vm.tick_at(tick.saturating_mul(16), &timeline);
        let events = vm.take_events();
        let frame = state.borrow().frame();
        if tick < from {
            let _ = renderer.render_frame(&frame);
            continue;
        }
        for ev in &events {
            if let VmEvent::Command {
                name,
                args,
                script,
                pc,
                ..
            } = ev
            {
                if name != "CG" && name != "CGACT" && name != "CGEND" {
                    continue;
                }
                let show = |slot: usize| -> String {
                    args.iter()
                        .find(|a| usize::from(a.slot) == slot)
                        .map(|a| format!("{:?}", a.value))
                        .unwrap_or_default()
                };
                println!("f{script}:{pc} {name} ID={} ID2={}", show(0), show(3));
            }
        }
        let _ = renderer.render_frame(&frame);
    }

    let frame = state.borrow().frame();
    println!("--- frame {} : layers with CGS ---", frame.frame);
    for l in &frame.layers {
        if l.id.display().contains("CGS") {
            println!("  {}", l.id.display());
        }
    }
    Ok(())
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
