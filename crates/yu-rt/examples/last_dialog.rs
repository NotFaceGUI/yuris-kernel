//! 打印最后一次 DIALOG 的完整参数(解码 CP932)。
use yu_rt::input::InputTimeline;
use yu_rt::value::Value;
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource};
fn main() {
    let root = std::env::args().nth(1).unwrap();
    let src = DirSource::open(format!("{root}/data"), Codec::Cp932).unwrap();
    let aux = DirSource::open(&root, Codec::Cp932).unwrap();
    let cfg = BootConfig { entry_label: "SYSTEM_START".into(), ..BootConfig::default() };
    let mut vm = Vm::boot_with_aux(Box::new(src), Some(Box::new(aux)), cfg).unwrap();
    vm.tick_at(0, &InputTimeline::new());
    for e in vm.events() {
        if let yu_rt::VmEvent::Command { name, script, pc, args, .. } = e {
            if name == "DIALOG" || name == "ERROR" {
                println!("{name} s{script} pc{pc}");
                for a in args {
                    let raw = vm.events().len();
                    let _ = raw;
                    match &a.value {
                        Value::Str(b) => {
                            let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
                            println!("  slot{} str(hex)={h} lossy={:?}", a.slot, String::from_utf8_lossy(b));
                        }
                        v => println!("  slot{} {:?}", a.slot, v),
                    }
                }
            }
        }
    }
}
