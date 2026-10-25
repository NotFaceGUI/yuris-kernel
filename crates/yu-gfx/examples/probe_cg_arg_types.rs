//! 取证探针:`CG` 命令的 `X/Y/SX/SY` 实参类型分布(整数/浮点/字符串)。
//!
//! 为什么需要:gfx 的 `apply_layer_params` 用 `as_int()` 读 `X/Y/SX/SY`
//! (`fract()==0` 守卫),而 `CGACT` 侧用 `find_px`/`find_f64` 接受浮点。
//! `es_Sprite.yst` 的 `es.CGACT_SIZE` 结尾是
//! `CG[ID=$dst X=@LEFT_X Y=@LEFT_Y]`,而 `@LEFT_X/@LEFT_Y` 是 **FLT**
//! (可带小数)。若这里被 `as_int()` 判成类型错误,图层位置就静默丢弃。
//! 本探针统计真实语料里 `CG` 的 `X/Y/SX/SY` 到底有没有浮点形态。
//!
//! 用法:`cargo run -p yu-gfx --example probe_cg_arg_types -- <game_root> [frames]
//!       [entry_label] [click_frame,x,y,interval]`
use std::collections::BTreeMap;

use yu_gfx::{GfxConfig, GfxState, Viewport};
use yu_rt::event::VmEvent;
use yu_rt::input::{InputFrame, InputTimeline};
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource, YpfSource};

fn main() {
    let root = std::env::args().nth(1).expect("game root");
    let frames: u32 = std::env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(600);
    let entry = std::env::args()
        .nth(3)
        .unwrap_or_else(|| "SYSTEM_START".to_string());
    let click: Option<(u32, i64, i64, u32)> = std::env::args().nth(4).and_then(|v| {
        let mut p = v.split(',');
        Some((
            p.next()?.parse().ok()?,
            p.next()?.parse().ok()?,
            p.next()?.parse().ok()?,
            p.next().and_then(|s| s.parse().ok()).unwrap_or(0),
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
    let mut gfx = GfxState::from_tables(vm.tables(), GfxConfig::new(viewport)).expect("gfx");
    let table = yu_gfx::ParamTable::from_tables(vm.tables());

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
    let mut stats: BTreeMap<(String, String, &'static str), u32> = BTreeMap::new();
    let mut samples: BTreeMap<(String, String, &'static str), String> = BTreeMap::new();
    for f in 0..frames {
        vm.tick_at(f * 16, &timeline);
        events.clear();
        vm.drain_events(&mut events);
        for ev in &events {
            let VmEvent::DomainCall { domain, name, args, .. } = ev else {
                continue;
            };
            if !matches!(domain, yu_rt::event::Domain::Graphics) {
                continue;
            }
            if name != "CG" && name != "CGACT" {
                continue;
            }
            for r in table.resolve(name, args) {
                let Some(pname) = r.name else { continue };
                if !matches!(pname, "X" | "Y" | "X2" | "Y2" | "SX" | "SY") {
                    continue;
                }
                let kind = match &r.value {
                    yu_rt::Value::Int(_) => "int",
                    yu_rt::Value::Float(fv) => {
                        if fv.fract() == 0.0 {
                            "float(whole)"
                        } else {
                            "float(frac)"
                        }
                    }
                    yu_rt::Value::Str(_) => "str",
                };
                let key = (name.clone(), pname.to_string(), kind);
                *stats.entry(key.clone()).or_default() += 1;
                samples.entry(key).or_insert_with(|| match &r.value {
                    yu_rt::Value::Int(i) => i.to_string(),
                    yu_rt::Value::Float(v) => format!("{v}"),
                    yu_rt::Value::Str(b) => format!("{b:?}"),
                });
            }
        }
        gfx.consume_events(&events);
    }

    println!("== CG/CGACT 的 X/Y/X2/Y2/SX/SY 实参类型分布(frames={frames})==");
    for ((cmd, param, kind), n) in &stats {
        let sample = samples
            .get(&(cmd.clone(), param.clone(), kind))
            .cloned()
            .unwrap_or_default();
        println!("  {cmd:<6} {param:<4} {kind:<13} x{n:<6} 例={sample}");
    }
    let frac = stats
        .iter()
        .filter(|((_, _, k), _)| *k == "float(frac)")
        .map(|(_, n)| *n)
        .sum::<u32>();
    println!("\n非整数(带小数)实参合计:{frac}");
}
