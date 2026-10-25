//! 取证探针:跟踪指定图层 ID 前缀在 gfx 侧的完整生命周期
//! (nodes 是否存在 / texture 种类 / copy_log 是否登记 / frame() 是否过滤)。
//!
//! 用法:
//!   `cargo run -p yu-gfx --example probe_copy_chain -- <game_root> <frames> <前缀>`
use yu_gfx::{GfxConfig, GfxState, Viewport};
use yu_rt::input::InputTimeline;
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource, YpfSource};

fn main() {
    let root = std::env::args().nth(1).expect("game root");
    let frames: u32 = std::env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(90);
    let prefix = std::env::args().nth(3).unwrap_or_else(|| "span".into());

    let ypf = std::path::PathBuf::from(&root).join("data.ypf");
    let src: Box<dyn yu_vfs::DataSource> = Box::new(
        YpfSource::open(&ypf, Codec::Cp932).expect("data.ypf"),
    );
    let aux = DirSource::open(&root, Codec::Cp932).expect("game root");
    let cfg = BootConfig {
        entry_label: "SYSTEM_START".to_string(),
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

    let timeline = InputTimeline::new();
    let mut events = Vec::new();
    for f in 0..frames {
        vm.tick_at(f * 16, &timeline);
        events.clear();
        vm.drain_events(&mut events);
        gfx.consume_events(&events);

        // 每帧打印匹配前缀的 copy 与图层。
        let mut lines: Vec<String> = Vec::new();
        for c in gfx.copies() {
            let d = c.dest.display();
            let s = match &c.source {
                yu_gfx::op::CopySource::Layer(l) => l.display(),
                yu_gfx::op::CopySource::Group(g) => format!("group:{}", g.display()),
            };
            if d.contains(&prefix) || s.contains(&prefix) {
                lines.push(format!(
                    "   COPY {:?} {} <- {} status={:?} srctex={:?}",
                    c.kind, d, s, c.status, c.source_texture
                ));
            }
        }
        let fr = gfx.frame();
        for l in &fr.layers {
            let id = l.id.display();
            if id.contains(&prefix) {
                lines.push(format!(
                    "   NODE {} kind={:?} tex={:?} a={} vis={} z={}",
                    id, l.kind, l.texture, l.alpha, l.visible, l.z
                ));
            }
        }
        if !lines.is_empty() {
            println!("== f{f} ==");
            for l in lines.iter().take(40) {
                println!("{l}");
            }
        }
    }
}
