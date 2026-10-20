//! 打印前若干负下标诊断的上下文事件序列。
use yu_rt::input::InputTimeline;
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource};
fn main() {
    let root = std::env::args().nth(1).unwrap();
    let limit: usize = std::env::args().nth(2).and_then(|v| v.parse().ok()).unwrap_or(3);
    let src = DirSource::open(format!("{root}/data"), Codec::Cp932).unwrap();
    let aux = DirSource::open(&root, Codec::Cp932).unwrap();
    let cfg = BootConfig { entry_label: "SYSTEM_START".into(), ..BootConfig::default() };
    let mut vm = Vm::boot_with_aux(Box::new(src), Some(Box::new(aux)), cfg).unwrap();
    vm.tick_at(0, &InputTimeline::new());
    let evs = vm.events();
    let mut shown = 0;
    for (i, e) in evs.iter().enumerate() {
        if let yu_rt::VmEvent::Diag { detail, .. } = e {
            if detail.contains("为负") {
                shown += 1;
                println!("--- hit {shown} at event {i} ---");
                let start = i.saturating_sub(6);
                for x in &evs[start..=i] { println!("{}", yu_rt::json::event_line(x)); }
                if shown >= limit { break; }
            }
        }
    }
}
