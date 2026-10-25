//! 取证探针:逐帧增量打印 `GfxState` 的 image/copy 日志新增项与
//! `text_draws` 分布;失败项打完整参数(`sx/sy/区域/状态`)。
//!
//! 目的:定位「SIZE 拿不到尺寸」到底来自哪条脚本命令,以及文本为什么不上屏。
//!
//! 用法:
//!   `cargo run -p yu-gfx --example probe_op_growth -- <game_root> [frames]
//!    [entry_label] [click_frame,x,y,interval]`
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
    let click: Option<(u32, i64, i64, u32)> = std::env::args().nth(4).and_then(|v| {
        let mut parts = v.split(',');
        Some((
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
            parts.next().and_then(|s| s.parse().ok()).unwrap_or(0),
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
    if let Some((start, x, y, interval)) = click {
        let mut f = start;
        while f < frames {
            timeline.set(u64::from(f), InputFrame::new(["MOUSE_L"]).with_cursor(x, y));
            timeline.set(
                u64::from(f + 1),
                InputFrame::new(std::iter::empty::<&str>()).with_cursor(x, y),
            );
            if interval == 0 {
                break;
            }
            f += interval;
        }
    }

    let mut events = Vec::new();
    let (mut seen_img, mut seen_copy) = (0usize, 0usize);
    let (mut fail_img, mut fail_copy) = (0u64, 0u64);
    for f in 0..frames {
        vm.tick_at(f * 16, &timeline);
        events.clear();
        vm.drain_events(&mut events);
        let gfx = state.borrow();
        let images = gfx.images();
        let copies = gfx.copies();
        if images.len() > seen_img || copies.len() > seen_copy {
            println!(
                "--- f{f}: images {} (+{}), copies {} (+{})",
                images.len(),
                images.len() - seen_img,
                copies.len(),
                copies.len() - seen_copy
            );
            for i in &images[seen_img..] {
                let bad = format!("{:?}", i.status);
                if bad.contains("Unimplemented") {
                    fail_img += 1;
                    println!(
                        "    IMG {} src={} tgt={} sx={:?} sy={:?} region={:?} status={:?}",
                        i.action.name(),
                        i.source.display(),
                        i.target.as_ref().map(|t| t.display()).unwrap_or_default(),
                        i.sx,
                        i.sy,
                        (i.x, i.y, i.x2, i.y2),
                        i.status
                    );
                }
            }
            for c in &copies[seen_copy..] {
                if format!("{:?}", c.status).contains("Unimplemented") {
                    fail_copy += 1;
                    println!(
                        "    COPY {:?} {} <- {} status={:?}",
                        c.kind,
                        c.dest.display(),
                        match &c.source {
                            yu_gfx::op::CopySource::Layer(l) => l.display(),
                            yu_gfx::op::CopySource::Group(g) => format!("group:{}", g.display()),
                        },
                        c.status
                    );
                }
            }
            seen_img = images.len();
            seen_copy = copies.len();
        }
        if f + 1 == frames {
            let frame = gfx.frame();
            println!(
                "\n== f{f} 汇总: layers={} passes={} images={} copies={} 失败IMG={} 失败COPY={}",
                frame.layers.len(),
                frame.passes.len(),
                images.len(),
                copies.len(),
                fail_img,
                fail_copy
            );
            // 每层 text_draws 数量(按层名排序取前 40)。
            let mut texts: Vec<(String, usize)> = frame
                .text_draws
                .iter()
                .map(|d| (d.target.display(), 0))
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .collect();
            for (id, n) in texts.iter_mut() {
                *n = frame.text_draws.iter().filter(|d| &d.target.display() == id).count();
            }
            texts.sort_by(|a, b| b.1.cmp(&a.1));
            println!("   text_draws 共 {} 层:", texts.len());
            for (id, n) in texts.iter().take(40) {
                println!("     {n:5}  {id}");
            }
            println!("   text 运行层: {}", frame.text.len());
            for t in frame.text.iter().take(5) {
                println!(
                    "     id={} bytes={} flags={:?} origin={:?}",
                    t.id.display(),
                    t.bytes.len(),
                    t.flags.as_ref().map(|f| f.len()),
                    t.origin
                );
            }
            println!("   诊断:");
            for d in frame.diagnostics.iter().take(15) {
                println!("     {} {}", d.code, d.detail);
            }
        }
    }
}
