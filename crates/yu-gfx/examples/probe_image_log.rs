//! 取证探针:跟踪 `GfxState::images()` 里图像加工项的 `status/sx/sy/区域`,
//! 并按 `(动作, 状态原因)` 汇总;同时报告每帧 pass 数与增长来源。
//!
//! 目的:`gfx.imageop.size.missing` 是「SIZE 动作拿不到正尺寸」的统称,但它
//! 分不清「参数缺失」「参数为 0」「层尺寸查询失败」。本探针把每条 ImageOp
//! 的原始参数打出来,并统计哪些层反复被写入(逐帧增长的那批)。
//!
//! 用法:
//!   `cargo run -p yu-gfx --example probe_image_log -- <game_root> [frames]
//!    [entry_label] [click_frame,x,y]`
use std::collections::BTreeMap;

use yu_gfx::{GfxConfig, GfxHost, GfxState, Viewport};
use yu_rt::input::{InputFrame, InputTimeline};
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource, YpfSource};

fn main() {
    let root = std::env::args().nth(1).expect("game root");
    let frames: u32 = std::env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(420);
    let entry = std::env::args()
        .nth(3)
        .unwrap_or_else(|| "SYSTEM_START".to_string());
    let click: Option<(u64, i64, i64)> = std::env::args().nth(4).and_then(|v| {
        let mut parts = v.split(',');
        Some((
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
        ))
    });

    let ypf = std::path::PathBuf::from(&root).join("data.ypf");
    let src: Box<dyn yu_vfs::DataSource> = if ypf.is_file() {
        Box::new(YpfSource::open(&ypf, Codec::Cp932).expect("data.ypf"))
    } else {
        Box::new(DirSource::open(format!("{root}/data"), Codec::Cp932).expect("data"))
    };
    let aux = DirSource::open(&root, Codec::Cp932).expect("game root");
    let cfg = BootConfig {
        entry_label: entry,
        fps: 60,
        paths: yu_rt::vm::HostPaths {
            system_path: Some(format!("{root}/").into_bytes()),
            save_path: Some(format!("{root}/save/").into_bytes()),
            appdata_path: Some(format!("{root}/").into_bytes()),
            personal_path: Some(format!("{root}/").into_bytes()),
            project_path: Some(format!("{root}/").into_bytes()),
        },
        ..BootConfig::default()
    };
    let mut vm = Vm::boot_with_aux(src, Some(Box::new(aux)), cfg).expect("boot");
    let viewport = Viewport::new(
        vm.tables().boot.config.width,
        vm.tables().boot.config.height,
    );
    let state = std::rc::Rc::new(std::cell::RefCell::new(
        GfxState::from_tables(vm.tables(), GfxConfig::new(viewport)).expect("gfx"),
    ));
    vm.set_graphics_host(Box::new(GfxHost::new(state.clone())));

    let mut timeline = InputTimeline::new();
    if let Some((f, x, y)) = click {
        timeline.set(f, InputFrame::new(["MOUSE_L"]).with_cursor(x, y));
        timeline.set(f + 1, InputFrame::new(std::iter::empty::<&str>()).with_cursor(x, y));
    }

    let mut events = Vec::new();
    let mut last_pass = 0usize;
    for f in 0..frames {
        vm.tick_at(f * 16, &timeline);
        events.clear();
        vm.drain_events(&mut events);
        // 事件已由 GraphicsHost 同步消费;此处只数数。
        let gfx = state.borrow();
        let frame = gfx.frame();
        let passes = frame.passes.len();
        if passes != last_pass {
            println!(
                "f{f}: passes {last_pass} -> {passes} layers={} images={} copies={}",
                frame.layers.len(),
                gfx.images().len(),
                gfx.copies().len()
            );
            last_pass = passes;
        }
        if f + 1 == frames {
            println!("\n== 最终 image_log({}) ==", gfx.images().len());
            let mut by_key: BTreeMap<String, (u32, Option<f64>, Option<f64>)> = BTreeMap::new();
            for i in gfx.images() {
                let key = format!(
                    "{} src={} tgt={} sx={:?} sy={:?} region={:?} status={:?}",
                    i.action.name(),
                    i.source.display(),
                    i.target.as_ref().map(|t| t.display()).unwrap_or_default(),
                    i.sx,
                    i.sy,
                    (i.x, i.y, i.x2, i.y2),
                    i.status
                );
                let e = by_key.entry(key).or_insert((0, i.sx, i.sy));
                e.0 += 1;
            }
            let mut rows: Vec<_> = by_key.into_iter().collect();
            rows.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
            for (key, (count, _, _)) in rows.iter().take(40) {
                println!("  x{count:<5} {key}");
            }
            println!("\n== 本帧诊断 ==");
            for d in frame.diagnostics.iter().take(20) {
                println!("  {} {}", d.code, d.detail);
            }
        }
    }
}
