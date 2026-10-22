//! 真实 ERIS 语料上的整循环回归（VM-only，不渲染、不出设备）。
//!
//! 6000 帧 × 16ms = 96s 模拟时钟，跨过 `bgm/bgm01.ogg` 的 90.8s END 边界，
//! 观察 LOOPCOUNT/TIME。缺样本显式 SKIP。
//!
//! ```bash
//! YURIS_SAMPLE_ERIS_ROOT=<project-eris> \
//!   cargo run -p yu-cli --example sound_loop_census -- 6000
//! ```

use std::sync::{Arc, Mutex};

use yu_cli::project::Project;
use yu_log::Diag;
use yu_rt::input::InputTimeline;
use yu_rt::{BootConfig, Vm};
use yu_sound::{SoundHost, SoundLoader, SoundState};
use yu_vfs::{Codec, DataSource, LookupKey};

struct Loader(Box<dyn DataSource>);

impl SoundLoader for Loader {
    fn load(&self, name: &[u8]) -> Result<Option<Vec<u8>>, Diag> {
        let key = LookupKey::new(name, Codec::Cp932).map_err(|d| {
            yu_sound::error::recoverable(yu_sound::error::code::FILE_READ, format!("path: {d}"))
        })?;
        if !self.0.exists(&key) {
            return Ok(None);
        }
        self.0.open(&key).map(Some).map_err(|d| {
            yu_sound::error::recoverable(yu_sound::error::code::FILE_READ, format!("read: {d}"))
        })
    }
}

fn main() {
    let Ok(root) = std::env::var("YURIS_SAMPLE_ERIS_ROOT") else {
        eprintln!("SKIP sound_loop_census: YURIS_SAMPLE_ERIS_ROOT not set");
        return;
    };
    let frames: u32 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(6000);
    let project = Project::discover(std::path::Path::new(&root), None).expect("project");
    let src = project.open_source().expect("source");
    let aux = project.open_aux().expect("aux");
    let cfg = BootConfig {
        record_events: false,
        ..BootConfig::default()
    };
    let mut vm = Vm::boot_with_aux(src, Some(aux), cfg).expect("boot");
    let sound = Arc::new(Mutex::new(SoundState::new(Box::new(Loader(
        project.open_source().expect("sound source"),
    )))));
    let yscm = vm.tables().boot.yscm.clone();
    vm.set_sound_host(Box::new(SoundHost::new(sound.clone(), yscm)));

    let timeline = InputTimeline::new();
    let mut last_log = 0u32;
    for f in 0..frames {
        let now = f.saturating_mul(16);
        vm.tick_at(now, &timeline);
        if now.saturating_sub(last_log) >= 5000 {
            last_log = now;
            let s = sound.lock().unwrap();
            if let Some(l) = s.layer(b"BGM/B1") {
                println!(
                    "t={}ms state={:?} time={} total={} loop={} count={}",
                    now,
                    l.state(),
                    l.position_ms(),
                    l.total_ms(),
                    l.loop_setting(),
                    l.loop_count()
                );
            }
        }
    }
    {
        let s = sound.lock().unwrap();
        if let Some(l) = s.layer(b"BGM/B1") {
            println!(
                "FINAL t={}ms state={:?} time={} total={} loop={} count={}",
                frames.saturating_mul(16),
                l.state(),
                l.position_ms(),
                l.total_ms(),
                l.loop_setting(),
                l.loop_count()
            );
        } else {
            println!("BGM/B1 layer not found");
        }
    }
    let d = sound.lock().unwrap().take_diags();
    println!("snd_diags={}", d.len());
    for x in d.iter().take(10) {
        println!("diag {} {}", x.code, x.detail);
    }
}
