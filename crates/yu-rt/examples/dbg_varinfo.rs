//! 逐步复现 VAR.ADD 宏链,在 WAIT 处分帧观察。
use yu_rt::input::InputTimeline;
use yu_rt::value::{VarRef, VarSpace};
use yu_rt::{BootConfig, Vm};
use yu_vfs::MemSource;
#[path = "../tests/support/mod.rs"]
mod support;
use support::{ex, GroupSpec, Init, ProjectBuilder, VarSpec};

fn main() {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![
        VarSpec {
            kind: 1,
            group: 1,
            script_id: 0,
            var_id: sys,
            value_type: 3,
            dims: vec![0],
            init: Init::Str(b""),
        },
        VarSpec {
            kind: 1,
            group: 1,
            script_id: 0,
            var_id: sys + 1,
            value_type: 1,
            dims: vec![10],
            init: Init::Int(0),
        },
        VarSpec {
            kind: 1,
            group: 1,
            script_id: 0,
            var_id: sys + 2,
            value_type: 3,
            dims: vec![],
            init: Init::Str(b"yuu"),
        },
    ];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    let arr = ex::r#ref(0x24, sys);
    let cnt = ex::r#ref(0x40, sys + 1);
    b.scripts = vec![vec![
        GroupSpec::new(100)
            .window(0, arr.clone())
            .window_tagged(13, 0, 0x0101, ex::i8_(1)),
        GroupSpec::new(102).window(0, ex::i8_(1)), // WAIT[FRAME=1]
        GroupSpec::new(101)
            .window(0, arr.clone())
            .window(5, ex::i8_(1))
            .window(1, ex::cat(&[cnt.clone(), ex::i8_(0), ex::aload()])),
        GroupSpec::new(102).window(0, ex::i8_(1)),
        GroupSpec::new(54)
            .window(
                0,
                ex::cat(&[
                    ex::r#ref(0x24, sys),
                    ex::r#ref(0x40, sys + 1),
                    ex::i8_(0),
                    ex::aload(),
                    ex::i8_(1),
                    ex::op(0x2d),
                    ex::aload(),
                ]),
            )
            .window(0, ex::get(0x24, sys + 2)),
        GroupSpec::new(14),
    ]];
    let src: MemSource = b.build();
    let mut vm = Vm::boot(Box::new(src), BootConfig::default()).unwrap();
    let tl = InputTimeline::new();
    let arr_ref = VarRef::new(VarSpace::Dollar, sys);
    let cnt_ref = VarRef::new(VarSpace::At, sys + 1);
    for f in 0..5 {
        let r = vm.tick_at(f * 16, &tl);
        println!(
            "frame {f}: cmds={} errors={:?}",
            r.commands_pumped, r.errors
        );
        println!(
            "   $arr dims={:?}",
            vm.store().shape(arr_ref).map(|(_, d)| d.to_vec())
        );
        println!(
            "   @cnt dims={:?} [0]={:?}",
            vm.store().shape(cnt_ref).map(|(_, d)| d.to_vec()),
            vm.store().read(cnt_ref, &[0])
        );
        println!("   $arr[0]={:?}", vm.store().read(arr_ref, &[0]));
        if r.main_finished {
            break;
        }
    }
}
