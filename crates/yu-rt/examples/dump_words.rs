//! 逐组倾倒一个脚本的命令序列(名字+窗口+载荷预览),对齐源码行。
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
    let sid: u32 = std::env::args()
        .nth(1)
        .and_then(|v| v.parse().ok())
        .unwrap_or(130);
    let from: usize = std::env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let to: usize = std::env::args()
        .nth(3)
        .and_then(|v| v.parse().ok())
        .unwrap_or(18);
    let src = DirSource::open(format!("{root}/data"), Codec::Cp932).unwrap();
    let aux = DirSource::open(&root, Codec::Cp932).unwrap();
    let cfg = BootConfig {
        entry_label: "SYSTEM_START".into(),
        ..BootConfig::default()
    };
    let mut vm = Vm::boot_with_aux(Box::new(src), Some(Box::new(aux)), cfg).unwrap();
    let dat = DirSource::open(format!("{root}/data"), Codec::Cp932).unwrap();
    let sf = vm.tables_mut().load_script(&dat, sid).unwrap().clone();

    for gi in from..to.min(sf.groups.len()) {
        let g = &sf.groups[gi];
        let name = vm
            .tables()
            .boot
            .yscm
            .command(g.opcode)
            .map(|c| c.name.clone())
            .unwrap_or(format!("?{}", g.opcode));
        let line = sf.source_position(gi).unwrap_or(0);
        println!("--- [{gi}] {name} param={} line={line}", g.param);
        for (wi, w) in sf.group_windows(gi).iter().enumerate() {
            let pool = sf.pool();
            let off = w.offset as usize;
            let len = w.length as usize;
            let payload = pool.get(off..off + len).unwrap_or(&[]);
            let preview = if len <= 48 {
                format!("{payload:02x?}")
            } else {
                format!("{:02x?}…({len}B)", &payload[..40])
            };
            // 可打印时顺带给文本
            let text: String = if payload.iter().all(|&b| b == 0 || (0x20..0x7f).contains(&b)) {
                String::from_utf8_lossy(payload).into_owned()
            } else {
                String::new()
            };
            println!(
                "    w{wi} kind={} flag={} slot={} len={} off={} | {preview} {text}",
                w.kind, w.flag, w.slot, w.length, w.offset
            );
        }
    }
}
