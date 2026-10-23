//! 倾倒 ysv.ybn 变量记录(按 var_id 过滤;不过滤打印前 N 条)。
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

fn main() {
    let root = eris_root();
    let want: Vec<u16> = std::env::args()
        .skip(1)
        .filter_map(|v| v.parse().ok())
        .collect();
    let src = DirSource::open(format!("{root}/data"), Codec::Cp932).unwrap();
    let aux = DirSource::open(root, Codec::Cp932).unwrap();
    let cfg = BootConfig {
        entry_label: "SYSTEM_START".into(),
        ..BootConfig::default()
    };
    let vm = Vm::boot_with_aux(Box::new(src), Some(Box::new(aux)), cfg).unwrap();
    for r in &vm.tables().boot.vars.records {
        if !want.is_empty() && !want.contains(&r.var_id) {
            continue;
        }
        println!(
            "id={} kind={} group={} script={} type={} dims={:?} init={}",
            r.var_id,
            r.kind,
            r.group,
            r.script_id,
            r.value_type,
            r.dims,
            match &r.init {
                yu_rt::tables::VarInit::Str(s) => format!("str {}B {s:02x?}", s.len()),
                yu_rt::tables::VarInit::Int(v) => format!("int {v}"),
                yu_rt::tables::VarInit::Float(v) => format!("flt {v}"),
                yu_rt::tables::VarInit::None => "none".into(),
            }
        );
    }
}
