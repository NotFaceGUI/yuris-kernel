//! 商业发行版端到端:从 `data.ypf` 单包启动并跑 N 帧。
//!
//! 用法:`cargo run -p yu-rt --example run_game -- <game_root> [frames]`
use yu_rt::input::InputTimeline;
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource, YpfSource};
fn main() {
    let root = std::env::args().nth(1).expect("game root (含 data.ypf)");
    let frames: u32 = std::env::args().nth(2).and_then(|v| v.parse().ok()).unwrap_or(5);
    let pack = YpfSource::open(format!("{root}/data.ypf"), Codec::Cp932).expect("data.ypf");
    // 商业包无 system/YSCom/YSCom.ycd;aux 指向同一目录(找不到就走内嵌 ABI 表)。
    let aux = DirSource::open(&root, Codec::Cp932).expect("game root");
    let cfg = BootConfig {
        entry_label: "SYSTEM_START".into(),
        fps: 60,
        screen_color: 32,
        dx_mode: 0,
        media_readonly: false,
        os_sound: true,
        paths: yu_rt::vm::HostPaths {
            system_path: Some(format!("{root}/").into_bytes()),
            save_path: Some(format!("{root}/save/").into_bytes()),
            appdata_path: Some(format!("{root}/").into_bytes()),
            personal_path: Some(format!("{root}/").into_bytes()),
            project_path: Some(format!("{root}/").into_bytes()),
        },
        ..BootConfig::default()
    };
    let mut vm = Vm::boot_with_aux(Box::new(pack), Some(Box::new(aux)), cfg).expect("boot");
    println!("== provenance ==");
    for l in vm.table_provenance() { println!("  {l}"); }
    let tl = InputTimeline::new();
    let mut total = 0u64;
    for f in 0..frames {
        let r = vm.tick_at(f * 16, &tl);
        total += r.commands_pumped;
        println!("frame {f}: cmds={} errors={} main_finished={}", r.commands_pumped, r.errors.len(), r.main_finished);
        for e in &r.errors { println!("   ERR {e}"); }
        if r.main_finished { break; }
    }
    println!("== total {total} ==");
    println!("== unimplemented ==");
    let mut rows: Vec<_> = vm.unimplemented_cmds().iter().collect();
    rows.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
    for (name, n) in rows.iter().take(20) { println!("  {name}: {n}"); }
}
