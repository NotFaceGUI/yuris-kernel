//! 检查 TDDEF 定义表加载情况。
use yu_rt::input::InputTimeline;
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource};
fn main() {
    let root = std::env::args().nth(1).unwrap();
    let src = DirSource::open(format!("{root}/data"), Codec::Cp932).unwrap();
    let aux = DirSource::open(&root, Codec::Cp932).unwrap();
    let cfg = BootConfig { entry_label: "SYSTEM_START".into(), ..BootConfig::default() };
    let mut vm = Vm::boot_with_aux(Box::new(src), Some(Box::new(aux)), cfg).unwrap();
    vm.tick_at(0, &InputTimeline::new());
    let mut set_calls = 0;
    let mut search_calls: Vec<String> = Vec::new();
    for e in vm.events() {
        if let yu_rt::VmEvent::Command { name, script, pc, args, .. } = e {
            if name == "GOSUB" {
                if let Some(a) = args.first() {
                    if let yu_rt::value::Value::Str(b) = &a.value {
                        let s = String::from_utf8_lossy(b);
                        if s == "es.TDDEF.SET" { set_calls += 1; }
                        if s == "es.TDDEF.SEARCH" {
                            search_calls.push(format!("s{script} pc{pc} pstr={:?}", args.get(1).map(|x| x.value.to_event_repr())));
                        }
                    }
                }
            }
        }
    }
    println!("TDDEF.SET calls = {set_calls}");
    println!("TDDEF.SEARCH calls = {}", search_calls.len());
    for s in search_calls.iter().take(5) { println!("  {s}"); }
    // TDD counter variables: find by name in ERIS? print ids with small ints changed
    println!("--- sample tasks ---");
    for n in vm.scheduler().nodes() { println!("  {} s{} pc{} fin={}", n.id, n.script_id, n.pc, n.finished); }
}
