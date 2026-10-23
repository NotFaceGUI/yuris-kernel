//! 交叉验证:同一个 ERIS 工程,分别用**散盘**与**官方 data.ypf 包**启动,
//! 比对前 N 帧的命令数、事件流摘要与变量终值。A1(数据源不可见)的运行核级证明。
use yu_rt::input::InputTimeline;
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource, YpfSource};

fn cfg() -> BootConfig {
    BootConfig {
        entry_label: "SYSTEM_START".into(),
        fps: 60,
        screen_color: 32,
        dx_mode: 0,
        media_readonly: false,
        os_sound: true,
        ..BootConfig::default()
    }
}

fn run(vm: &mut Vm, frames: u32) -> (Vec<u64>, u64, usize) {
    let tl = InputTimeline::new();
    let mut counts = Vec::new();
    let mut total = 0u64;
    let mut errs = 0usize;
    for f in 0..frames {
        let r = vm.tick_at(f * 16, &tl);
        total += r.commands_pumped;
        errs += r.errors.len();
        counts.push(r.commands_pumped);
        if r.main_finished {
            break;
        }
    }
    (counts, total, errs)
}

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let frames: u32 = std::env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(6);

    // A: 散盘
    let src = DirSource::open(format!("{root}/data"), Codec::Cp932).unwrap();
    let aux = DirSource::open(&root, Codec::Cp932).unwrap();
    let mut a = Vm::boot_with_aux(Box::new(src), Some(Box::new(aux)), cfg()).unwrap();
    let (ca, ta, ea) = run(&mut a, frames);

    // B: 官方 data.ypf(发布形态;ysc/ysl/ysv 等全部从包内读)
    let pack = YpfSource::open(format!("{root}/data.ypf"), Codec::Cp932).unwrap();
    let auxb = DirSource::open(&root, Codec::Cp932).unwrap();
    let mut b = Vm::boot_with_aux(Box::new(pack), Some(Box::new(auxb)), cfg()).unwrap();
    let (cb, tb, eb) = run(&mut b, frames);

    println!("loose: counts={ca:?} total={ta} errors={ea}");
    println!("ypf  : counts={cb:?} total={tb} errors={eb}");
    assert_eq!(ca, cb, "逐帧命令数必须一致");
    assert_eq!(ea, eb, "错误数必须一致");
    // 变量终值:比较全部变量的确定摘要
    let digest = |vm: &Vm| {
        let mut h = String::new();
        for (r, slot) in vm.store().iter() {
            h.push_str(&format!("{}={:?};", r.display(), slot.data));
        }
        yu_rt::sysvar_abi::sha256_hex(h.as_bytes())
    };
    let da = digest(&a);
    let db = digest(&b);
    println!("var digest loose={da}\nvar digest ypf  ={db}");
    assert_eq!(da, db, "变量域终值必须一致(A1)");
    println!("A1 OK: 散盘与官方 data.ypf 逐帧命令数/错误数/变量终值一致");
}
