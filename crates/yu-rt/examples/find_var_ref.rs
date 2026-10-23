//! 扫描全部脚本对某个变量 id 的引用/取值(按空间字节),用于定位写入点。
//!
//! 用法:`find_var_ref <root> <var-id> [space-hex=40]`
//! 输出:`sid=.. line=.. kind=REF|VAL|REF76 space=0x.. off=..`。
use yu_rt::value::{VarRef, VarSpace};
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource};

fn main() {
    let root = std::env::args().nth(1).expect("root");
    let id: u16 = std::env::args()
        .nth(2)
        .expect("var id")
        .parse()
        .expect("decimal var id");
    let want_space = std::env::args()
        .nth(3)
        .map(|v| u8::from_str_radix(v.trim_start_matches("0x"), 16).expect("space hex"));
    let src = DirSource::open(format!("{root}/data"), Codec::Cp932).unwrap();
    let aux = DirSource::open(&root, Codec::Cp932).unwrap();
    let cfg = BootConfig {
        entry_label: "SYSTEM_START".into(),
        ..BootConfig::default()
    };
    let mut vm = Vm::boot_with_aux(Box::new(src), Some(Box::new(aux)), cfg).unwrap();
    let dat = DirSource::open(format!("{root}/data"), Codec::Cp932).unwrap();
    let lo = id as u8;
    let hi = (id >> 8) as u8;
    let mut total = 0usize;
    for sid in 0..200u32 {
        let sf = match vm.tables_mut().load_script(&dat, sid) {
            Ok(sf) => sf.clone(),
            Err(_) => continue,
        };
        let pool = sf.pool();
        for space in [0x40u8, 0x24, 0x60, 0x23] {
            if want_space.is_some_and(|s| s != space) {
                continue;
            }
            for (op, name) in [(0x56u8, "REF"), (0x48, "VAL"), (0x76, "REF76")] {
                let pat = [op, 3, 0, space, lo, hi];
                let mut at = 0usize;
                while at + pat.len() <= pool.len() {
                    let Some(i) = pool[at..]
                        .windows(pat.len())
                        .position(|w| w == pat.as_slice())
                    else {
                        break;
                    };
                    let off = at + i;
                    let line = group_line_for_offset(&sf, off);
                    println!("sid={sid} line={line} kind={name} space=0x{space:02x} off={off}");
                    let from = off.saturating_sub(24);
                    let to = (off + 72).min(pool.len());
                    println!("    ctx[{from}..{to}] = {:02x?}", &pool[from..to]);
                    total += 1;
                    at = off + pat.len();
                }
            }
        }
    }
    println!("# total={total}");
    // 启动后的槽位事实(不 tick):初值/维数/前几个元素。
    let var = VarRef::new(VarSpace::At, id);
    if let Some(slot) = vm.store().slot(var) {
        println!("# var @{id} ty={:?} dims={:?}", slot.ty, slot.dims);
        for (index, value) in slot.data.iter().enumerate().take(8) {
            println!("#   [{index}] = {}", value.to_event_repr());
        }
    } else {
        println!("# var @{id} 未声明");
    }
}

fn group_line_for_offset(sf: &yu_rt::tables::ScriptFile, off: usize) -> u32 {
    let mut best: Option<(usize, usize)> = None;
    for gi in 0..sf.groups.len() {
        for w in sf.group_windows(gi) {
            let start = w.offset as usize;
            let end = start + w.length as usize;
            if off >= start && off < end && best.is_none_or(|(b, _)| start < b) {
                best = Some((start, gi));
            }
        }
    }
    best.and_then(|(_, gi)| sf.source_position(gi)).unwrap_or(0)
}
