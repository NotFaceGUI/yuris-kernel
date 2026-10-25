//! 取证探针:**挂 GfxHost** 的变量导出(逐帧/指定帧)。
//!
//! 用法:`cargo run -p yu-backend-soft --example probe_host_vars -- <root> <from> <to> <id>...`
//!
//! 为什么不能直接看 `Vm` 裸跑:`CGINFO/WINDOWINFO/FONTINFO` 在没有宿主时
//! 不写回 `LET`,脚本读到 0 就走错分支 —— 裸跑得到的变量值和命令序都不可信。
use std::cell::RefCell;
use std::rc::Rc;
use std::path::PathBuf;

use yu_gfx::{GfxConfig, GfxHost, GfxState, Viewport};
use yu_rt::input::InputTimeline;
use yu_rt::value::Value;
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource, YpfSource};

fn cp932(b: &[u8]) -> String {
    yu_format::Codec::Cp932
        .decode(b, b"probe")
        .unwrap_or_else(|_| format!("<{:02x?}>", b))
}

fn main() {
    let mut args = std::env::args().skip(1);
    let root = args.next().expect("用法: <root> <from> <to> <id>...");
    let from: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    let to: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(1);
    let ids: Vec<u16> = args.filter_map(|v| v.parse().ok()).collect();

    let root_path = PathBuf::from(&root);
    let ypf = root_path.join("data.ypf");
    let src: Box<dyn yu_vfs::DataSource> = if ypf.is_file() {
        Box::new(YpfSource::open(&ypf, Codec::Cp932).unwrap())
    } else {
        Box::new(DirSource::open(root_path.join("data"), Codec::Cp932).unwrap())
    };
    let aux = DirSource::open(&root_path, Codec::Cp932).unwrap();
    let cfg = BootConfig {
        entry_label: "SYSTEM_START".into(),
        fps: 60,
        screen_color: 32,
        dx_mode: 0,
        paths: yu_rt::vm::HostPaths {
            system_path: Some(format!("{root}/").into_bytes()),
            save_path: Some(format!("{root}/save/").into_bytes()),
            appdata_path: Some(format!("{root}/").into_bytes()),
            personal_path: Some(format!("{root}/").into_bytes()),
            project_path: Some(format!("{root}/").into_bytes()),
        },
        ..BootConfig::default()
    };
    let mut vm = Vm::boot_with_aux(src, Some(Box::new(aux)), cfg).unwrap();
    let viewport = Viewport::new(
        vm.tables().boot.config.width,
        vm.tables().boot.config.height,
    );
    let state = Rc::new(RefCell::new(
        GfxState::from_tables(vm.tables(), GfxConfig::new(viewport)).unwrap(),
    ));
    vm.set_graphics_host(Box::new(GfxHost::new(state.clone())));

    let tl = InputTimeline::new();
    for f in 0..to {
        vm.tick_at(f * 16, &tl);
        let _ = vm.take_events();
        if f + 1 < from {
            continue;
        }
        println!("== f{} ==", f + 1);
        for (r, slot) in vm.store().iter() {
            if !ids.contains(&r.id) {
                continue;
            }
            let mut shown = Vec::new();
            for (i, v) in slot.data.iter().enumerate() {
                let s = match v {
                    Value::Int(x) => format!("{x}"),
                    Value::Float(x) => format!("{x}"),
                    Value::Str(b) => format!("{:?}", cp932(b)),
                };
                let default = matches!(v, Value::Int(0)) || matches!(v, Value::Float(x) if *x == 0.0);
                if default && slot.data.len() > 12 {
                    continue;
                }
                shown.push(format!("[{i}]={s}"));
            }
            println!(
                "  {}{} ty={:?} len={} {}",
                r.space.prefix() as char,
                r.id,
                slot.ty,
                slot.data.len(),
                if shown.is_empty() { "(all zero)".to_string() } else { shown.join(" ") }
            );
        }
    }
}
