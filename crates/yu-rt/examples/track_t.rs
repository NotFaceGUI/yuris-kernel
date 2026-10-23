//! 追踪 $T 数组的所有非空写入(LET 目标)。
use yu_rt::input::InputTimeline;
use yu_rt::value::{VarRef, VarSpace};
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource};
fn main() {
    let root = std::env::args().nth(1).unwrap();
    let t_id: u16 = u16::from_str_radix(
        std::env::args()
            .nth(2)
            .as_deref()
            .unwrap_or("0x50c")
            .trim_start_matches("0x"),
        16,
    )
    .unwrap();
    let src = DirSource::open(format!("{root}/data"), Codec::Cp932).unwrap();
    let aux = DirSource::open(&root, Codec::Cp932).unwrap();
    let cfg = BootConfig {
        entry_label: "SYSTEM_START".into(),
        ..BootConfig::default()
    };
    let mut vm = Vm::boot_with_aux(Box::new(src), Some(Box::new(aux)), cfg).unwrap();
    println!(
        "before: dims={:?}",
        vm.store().shape(VarRef::new(VarSpace::Dollar, t_id))
    );
    vm.tick_at(0, &InputTimeline::new());
    let r = VarRef::new(VarSpace::Dollar, t_id);
    match vm.store().slot(r) {
        Some(s) => {
            let nz: Vec<(usize, String)> = s
                .data
                .iter()
                .enumerate()
                .filter(|(_, v)| matches!(v, yu_rt::value::Value::Str(b) if !b.is_empty()))
                .map(|(i, v)| {
                    (
                        i,
                        match v {
                            yu_rt::value::Value::Str(b) => String::from_utf8_lossy(b).into_owned(),
                            _ => String::new(),
                        },
                    )
                })
                .take(12)
                .collect();
            println!(
                "after dims={:?} nonempty={} first: {:?}",
                s.dims,
                s.data
                    .iter()
                    .filter(|v| matches!(v, yu_rt::value::Value::Str(b) if !b.is_empty()))
                    .count(),
                nz
            );
        }
        None => println!("no slot"),
    }
    let writes: Vec<_> = vm
        .events()
        .iter()
        .filter_map(|e| match e {
            yu_rt::VmEvent::Command {
                name, script, pc, ..
            } if name == "LET" || name == "VARACT" || name == "STR" => {
                Some(format!("{name} s{script} pc{pc}"))
            }
            _ => None,
        })
        .take(10)
        .collect();
    println!("writes sample: {writes:?}");
}
