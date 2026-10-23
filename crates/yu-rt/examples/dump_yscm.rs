//! 倾倒 ysc.ybn(YSCM 运行时命令表)的命令名+参数名。
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource, YpfSource};

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
    // 数据入口唯一约定:data.ypf 优先,散盘 data/ 为备(AGENTS §2)。
    let ypf = std::path::PathBuf::from(&root).join("data.ypf");
    let src: Box<dyn yu_vfs::DataSource> = if ypf.is_file() {
        Box::new(YpfSource::open(&ypf, Codec::Cp932).unwrap())
    } else {
        Box::new(DirSource::open(format!("{root}/data"), Codec::Cp932).unwrap())
    };
    let aux = DirSource::open(root, Codec::Cp932).unwrap();
    let cfg = BootConfig {
        entry_label: "SYSTEM_START".into(),
        ..BootConfig::default()
    };
    let vm = Vm::boot_with_aux(src, Some(Box::new(aux)), cfg).unwrap();
    let filter = std::env::args().nth(1).unwrap_or_default();
    let yscm = &vm.tables().boot.yscm;
    for (i, c) in yscm.commands.iter().enumerate() {
        if !filter.is_empty() && !c.name.to_uppercase().contains(&filter.to_uppercase()) {
            continue;
        }
        let params: Vec<String> = c
            .params
            .iter()
            .enumerate()
            .map(|(k, p)| format!("{k}:{}", p.name))
            .collect();
        println!("[{i}] {} ({})", c.name, params.join(" "));
    }
}
