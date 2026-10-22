//! 诊断:装 GfxHost 后打印 CG/CGACT 域命令(含 SPL 图层),看真实命令序。
use std::cell::RefCell;
use std::env;
use std::path::PathBuf;
use std::rc::Rc;

use yu_gfx::{GfxConfig, GfxHost, GfxState, Viewport};
use yu_rt::event::VmEvent;
use yu_rt::input::InputTimeline;
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource, YpfSource};

fn main() {
    let root = env::var("YURIS_SAMPLE_ERIS_ROOT").expect("root");
    let root_path = PathBuf::from(&root);
    let data_dir = root_path.join("data");
    let ypf = root_path.join("data.ypf");
    let src: Box<dyn yu_vfs::DataSource> = if ypf.is_file() {
        Box::new(YpfSource::open(&ypf, Codec::Cp932).unwrap())
    } else {
        Box::new(DirSource::open(&data_dir, Codec::Cp932).unwrap())
    };
    let aux = DirSource::open(&root_path, Codec::Cp932).unwrap();
    let cfg = BootConfig {
        entry_label: "SYSTEM_START".into(),
        fps: 60,
        record_events: true,
        ..BootConfig::default()
    };
    let mut vm = Vm::boot_with_aux(src, Some(Box::new(aux)), cfg).unwrap();
    let viewport = Viewport::new(vm.tables().boot.config.width, vm.tables().boot.config.height);
    let state = Rc::new(RefCell::new(GfxState::from_tables(vm.tables(), GfxConfig::new(viewport)).unwrap()));
    vm.set_graphics_host(Box::new(GfxHost::new(state.clone())));
    let timeline = InputTimeline::new();
    let ticks: u32 = env::args().nth(1).and_then(|v| v.parse().ok()).unwrap_or(20);
    for tick in 0..ticks {
        vm.tick_at(tick * 16, &timeline);
        for ev in vm.take_events() {
            if let VmEvent::DomainCall { frame, name, args, .. } = &ev {
                if name == "CG" || name == "CGACT" || name == "CGEND" {
                    let text = args.iter().map(|a| format!("{}={}", a.slot, a.value.to_event_repr())).collect::<Vec<_>>().join(" ");
                    if text.contains("SPL") || text.contains("logo") || (name == "CGACT" && (text.contains("64=") || text.contains("65="))) {
                        println!("tick {tick} frame {frame} {name} {text}");
                    }
                }
            }
        }
    }
}
