//! 取证探针(临时):boot 后读 SCRIPTPOS 变量,按两代线格式解码并打印。
//!
//! 用法:`scriptpos_probe <project-root> <ticks> <slot>`

use std::path::Path;

use yu_engine::project::Project;
use yu_rt::input::InputTimeline;
use yu_rt::scriptpos::{ScriptPos, WIRE_GOSUB_U16, WIRE_GOSUB_U8};
use yu_rt::value::{Value, VarRef, VarSpace};
use yu_rt::{BootConfig, Vm};

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let ticks: u32 = std::env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(240);
    let slot: u16 = std::env::args()
        .nth(3)
        .map(|s| {
            s.strip_prefix("0x")
                .map(|h| u16::from_str_radix(h, 16).expect("hex"))
                .unwrap_or_else(|| s.parse().expect("slot"))
        })
        .unwrap_or(0x050C);
    let project = Project::discover(Path::new(&root), None).expect("project");
    let key = project.resolve_script_key(None).expect("script key");
    let src = project.open_source().expect("source");
    let aux = project.open_aux().expect("aux");
    let mut vm = Vm::boot_with_aux(
        src,
        Some(aux),
        BootConfig {
            entry_label: "SYSTEM_START".into(),
            script_key: key.bytes,
            ..BootConfig::default()
        },
    )
    .expect("boot");
    for f in 0..ticks {
        let _ = vm.tick_at(f, &InputTimeline::new());
    }
    let var = VarRef {
        space: VarSpace::Dollar,
        id: slot,
    };
    let Value::Str(bytes) = vm.store().read(var, &[]).expect("read slot") else {
        panic!("slot {slot} 不是字符串");
    };
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    println!("slot={slot} len={} hex={hex}", bytes.len());
    for (name, wire) in [("u16+tail", WIRE_GOSUB_U16), ("u8", WIRE_GOSUB_U8)] {
        match ScriptPos::decode_for_wire(&bytes, wire) {
            Ok((sp, used)) => println!(
                "{name}: OK used={used:?} pc={} line={} if={} gosub={} loop={} name={:?}",
                sp.pc,
                sp.text_line,
                sp.if_entries.len(),
                sp.gosub_entries.len(),
                sp.loop_entries.len(),
                String::from_utf8_lossy(&sp.script_name)
            ),
            Err(e) => println!("{name}: ERR {e}"),
        }
    }
}
