//! 取证探针:包装 `GfxHost`,把每次 `CGINFO/CGEND` 请求与结果打出来。
//!
//! 用法:`cargo run -p yu-backend-soft --example probe_host_query -- <root> <from> <to>`
//!
//! 用途(DIV-GFX-046 定案):官方 v494 trace 证明它在同一帧里
//! `CGEND SPLBUF1..100` → `CG SPLBUF1`(重建) → `CG SPL1` → **`es.SP.DRAW`**,
//! 而内核在 `es.SP.DRAW` 前就分叉了。分叉判据只依赖 `CGINFO[EXIST]` 的返回,
//! 所以这里把每一次 `CGINFO` 的提问与答案原样记录。
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use yu_gfx::{GfxConfig, GfxHost, GfxState, Viewport};
use yu_rt::event::{ArgValue, GraphicsHost, VmEvent};
use yu_rt::input::InputTimeline;
use yu_rt::value::Value;
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource, YpfSource};

fn cp932(b: &[u8]) -> String {
    yu_format::Codec::Cp932
        .decode(b, b"probe")
        .unwrap_or_else(|_| format!("<{:02x?}>", b))
}

/// 透传 + 记录。
struct LoggingHost {
    inner: GfxHost,
    on: bool,
    only_exist: bool,
}

impl GraphicsHost for LoggingHost {
    fn apply_event(&mut self, ev: &VmEvent) {
        self.inner.apply_event(ev);
    }

    fn query(&mut self, command: &str, args: &[ArgValue]) -> Result<Value, yu_log::Diag> {
        let result = self.inner.query(command, args);
        if self.on && command == "CGINFO" {
            let want: Vec<String> = args
                .iter()
                .map(|a| match &a.value {
                    Value::Int(i) => format!("{}#{i}", a.slot),
                    Value::Float(x) => format!("{}#{x}", a.slot),
                    Value::Str(b) => format!("{}=\"{}\"", a.slot, cp932(b)),
                })
                .collect();
            let flags: Vec<u8> = args
                .iter()
                .filter(|a| !matches!(a.value, Value::Str(_)))
                .map(|a| a.slot)
                .collect();
            let is_exist = flags.contains(&2); // CGINFO 槽 2 = EXIST(本作 ysc)
            if !self.only_exist || is_exist {
                match &result {
                    Ok(v) => println!("    CGINFO [{}] => {:?}", want.join(" "), v),
                    Err(d) => println!("    CGINFO [{}] => ERR {}", want.join(" "), d.code),
                }
            }
        }
        result
    }

    fn note_asset_size(&mut self, key: &[u8], width: u32, height: u32) {
        self.inner.note_asset_size(key, width, height);
    }

    fn set_pointer(&mut self, x: i64, y: i64) {
        self.inner.set_pointer(x, y);
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let root = args.next().expect("用法: <root> <from> <to>");
    let from: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    let to: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(1);
    let only_exist = std::env::var_os("PROBE_ALL").is_none();

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
    vm.set_graphics_host(Box::new(LoggingHost {
        inner: GfxHost::new(state.clone()),
        on: true,
        only_exist,
    }));

    let tl = InputTimeline::new();
    for f in 0..to {
        if f >= from {
            println!("== f{} ==", f + 1);
        }
        vm.tick_at(f * 16, &tl);
        let _ = vm.take_events();
    }
}
