//! 倾倒语料里某个数值变量(全局/脚本局部)的元素(取证用;DIV-RT-003)。
//!
//! 用法:`dump_var_int <var-id> [ticks]`
//! boot 后立即打印一次,再跑 `ticks` 逻辑帧后打印一次(缺省 0)。
use yu_rt::input::InputTimeline;
use yu_rt::value::{VarRef, VarSpace};
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource};

fn eris_root() -> String {
    std::env::var("YURIS_SAMPLE_ERIS_ROOT")
        .or_else(|_| {
            std::env::var("YURIS_SAMPLE_ERIS_YSBIN").map(|p| {
                std::path::Path::new(&p)
                    .ancestors()
                    .nth(2)
                    .unwrap()
                    .display()
                    .to_string()
            })
        })
        .expect("set YURIS_SAMPLE_ERIS_ROOT or YURIS_SAMPLE_ERIS_YSBIN")
}

fn dump(vm: &Vm, id: u16, tag: &str) {
    let var = VarRef::new(VarSpace::At, id);
    let Some(slot) = vm.store().slot(var) else {
        println!("{tag}: id={id} 不在变量域(或未声明)");
        return;
    };
    let dims = slot.dims.clone();
    let n: usize = dims.iter().product::<usize>().max(1);
    let mut vals = Vec::new();
    for i in 0..n.min(70) {
        match vm.store().read(var, &[i as i64]) {
            Ok(v) => vals.push(format!("{}", v.as_int().unwrap_or(0))),
            Err(d) => vals.push(format!("<{}>", d.code)),
        }
    }
    println!(
        "{tag}: id={id} dims={dims:?} 前{}项 = [{}]",
        vals.len(),
        vals.join(", ")
    );
}

fn main() {
    let root = eris_root();
    let id: u16 = std::env::args()
        .nth(1)
        .expect("var-id")
        .parse()
        .expect("u16");
    let ticks: u64 = std::env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let src = DirSource::open(format!("{root}/save/data"), Codec::Cp932).unwrap();
    let aux = DirSource::open(&root, Codec::Cp932).unwrap();
    let cfg = BootConfig {
        entry_label: "SYSTEM_START".into(),
        ..BootConfig::default()
    };
    let mut vm = Vm::boot_with_aux(Box::new(src), Some(Box::new(aux)), cfg).unwrap();
    dump(&vm, id, "boot 后");
    for f in 0..ticks {
        let r = vm.tick_at(f as u32, &InputTimeline::new());
        for e in &r.errors {
            println!("frame {f} ERR: {e}");
        }
    }
    if ticks > 0 {
        dump(&vm, id, &format!("tick {ticks} 后"));
    }
}
