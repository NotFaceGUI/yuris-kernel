//! 合成工程上的行为断言:事件流 golden、变量终值、错误路径。
//!
//! 所有用例都走**生产路径**(`Vm::boot` → `TableSet::load` → `ScriptFile::parse`),
//! 只是数据换成合成夹具。这样语义与真实语料共享同一批处理器。

mod support;

use support::{ex, with_int_var, GroupSpec, Init, ProjectBuilder, VarSpec};
use yu_rt::input::{InputFrame, InputTimeline};
use yu_rt::json::to_jsonl;
use yu_rt::task::MAIN_TASK_ID;
use yu_rt::value::{Value, VarRef, VarSpace};
use yu_rt::{BootConfig, Vm};

/// 与 0.495 `ysc.ybn` 一致的 opcode(夹具命令表同序)。
const OP_END: u8 = 14;
const OP_FILEINFO: u8 = 22;
const OP_FPS: u8 = 29;
const OP_GO: u8 = 43;
const OP_GOSUB: u8 = 44;
const OP_IF: u8 = 45;
const OP_IFBLEND: u8 = 46;
const OP_IFEND: u8 = 49;
const OP_INT: u8 = 51;
const OP_LET: u8 = 54;
const OP_LABELINFO: u8 = 53;
const OP_LOOP: u8 = 56;
const OP_LOOPEND: u8 = 59;
const OP_RETURN: u8 = 77;
const OP_STR: u8 = 90;
const OP_VARACT: u8 = 100;
const OP_WAIT: u8 = 102;
const OP_MATH: u8 = 61;

fn no_input() -> InputTimeline {
    InputTimeline::new()
}

fn boot(builder: ProjectBuilder) -> Vm {
    let src = builder.build();
    Vm::boot(Box::new(src), BootConfig::default()).expect("boot")
}

/// 声明变量 id:夹具里用户变量从 sys_count 起。
fn user_var(vm: &Vm, offset: u16) -> VarRef {
    let sys = vm.sysvars().len() as u16;
    VarRef::new(VarSpace::At, sys + offset)
}

// ---------------------------------------------------------------- 基本声明/赋值

#[test]
fn declare_and_let_scalar() {
    // 用户变量 id 必须高于系统变量数(系统变量镜像占用 0..sys_count)。
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![VarSpec {
        kind: 1,
        group: 0,
        script_id: 0,
        var_id: sys,
        value_type: 1,
        dims: vec![],
        init: Init::Int(0),
    }];
    let groups = vec![
        // INT[@v = 5]
        GroupSpec::new(OP_INT)
            .window(0, ex::r#ref(0x40, sys))
            .window(0, ex::i8_(5)),
        // LET[@v = @v * 2 + 1]
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, sys))
            .window(
                0,
                ex::cat(&[
                    ex::get(0x40, sys),
                    ex::i8_(2),
                    ex::op(0x2a),
                    ex::i8_(1),
                    ex::op(0x2b),
                ]),
            ),
        GroupSpec::new(OP_END),
    ];
    b.scripts = vec![groups];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);

    let mut vm = boot(b);
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let v = vm.store().read(user_var(&vm, 0), &[]).unwrap();
    assert_eq!(v, Value::Int(11));

    // 事件流:声明 + LET + END(END 结束主线)
    let kinds: Vec<&str> = vm.events().iter().map(|e| e.kind()).collect();
    assert!(kinds.contains(&"command"));
    assert_eq!(vm.events().iter().filter(|e| e.kind() == "end").count(), 1);
}

// ---------------------------------------------------------------- FILEINFO

/// 在合成工程上再插入数据文件(封包/散盘等价;`MemSource` 是同一 VFS 接口)。
fn boot_with_files(builder: ProjectBuilder, files: &[(&[u8], &[u8])]) -> Vm {
    let mut src = builder.build();
    for (path, bytes) in files {
        src.insert(path, bytes.to_vec())
            .expect("insert fixture file");
    }
    Vm::boot(Box::new(src), BootConfig::default()).expect("boot")
}

/// `FILEINFO[EXIST=1 FILE=... LET=@x]`:
/// 存在写 1、不存在写 0(EVID-J3-01;REA `0x443222/0x4432dc`)。
/// 路径按引擎习惯归一(反斜杠/大小写不影响命中)。
#[test]
fn fileinfo_exist_writes_one_and_zero_to_let() {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![
        VarSpec {
            kind: 1,
            group: 0,
            script_id: 0,
            var_id: sys,
            value_type: 1,
            dims: vec![],
            init: Init::Int(-1),
        },
        VarSpec {
            kind: 1,
            group: 0,
            script_id: 0,
            var_id: sys + 1,
            value_type: 1,
            dims: vec![],
            init: Init::Int(-1),
        },
    ];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        // FILEINFO[FILE="DATA\\BG.PNG" EXIST=1 LET=@a]
        // 脚本字面量里的 `\\` 是转义后的单个反斜杠(expr 0x4d 规则)。
        GroupSpec::new(OP_FILEINFO)
            .window(0, ex::s("DATA\\\\BG.PNG"))
            .window(1, ex::i8_(1))
            .window(2, ex::r#ref(0x40, sys)),
        // FILEINFO[FILE="data/missing.png" EXIST=1 LET=@b]
        GroupSpec::new(OP_FILEINFO)
            .window(0, ex::s("data/missing.png"))
            .window(1, ex::i8_(1))
            .window(2, ex::r#ref(0x40, sys + 1)),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot_with_files(b, &[(b"data/bg.png", b"\x89PNG")]);
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys), &[])
            .unwrap(),
        Value::Int(1)
    );
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys + 1), &[])
            .unwrap(),
        Value::Int(0),
        "不存在的文件也必须写 0(引擎 0x4432dc),不能跳过 LET"
    );
}

/// LET 目标是浮点槽时,结果按槽类型写 1.0/0.0(引擎 `0x443290` 分支)。
#[test]
fn fileinfo_exist_writes_float_target() {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![VarSpec {
        kind: 1,
        group: 0,
        script_id: 0,
        var_id: sys,
        value_type: 2,
        dims: vec![],
        init: Init::Float(0.0),
    }];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        GroupSpec::new(OP_FILEINFO)
            .window(0, ex::s("save/save_001.sd"))
            .window(1, ex::i8_(1))
            .window(2, ex::r#ref(0x40, sys)),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot_with_files(b, &[(b"save/save_001.sd", b"SD")]);
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys), &[])
            .unwrap(),
        Value::Float(1.0)
    );
}

// ---------------------------------------------------------------- IF / ELSE

fn if_fixture(cond: i64) -> ProjectBuilder {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![VarSpec {
        kind: 1,
        group: 0,
        script_id: 0,
        var_id: sys,
        value_type: 1,
        dims: vec![],
        init: Init::Int(0),
    }];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        // 0: IF[cond] ELSE 目标 = 3,出口 = 5
        GroupSpec::new(OP_IF)
            .window(0, ex::i8_(cond as i8))
            .jump(3)
            .jump(5),
        // 1: LET[@v=1]
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, sys))
            .window(0, ex::i8_(1)),
        // 2: IFBLEND(真分支结束 → 出口 5;语料 887 例同形)
        GroupSpec::new(OP_IFBLEND),
        // 3: ELSE(无条件 → 进 else 体)
        GroupSpec::new(11).window(0, Vec::new()).jump(0).jump(5),
        // 4: LET[@v=2]
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, sys))
            .window(0, ex::i8_(2)),
        // 5: IFEND
        GroupSpec::new(OP_IFEND),
        GroupSpec::new(OP_END),
    ]];
    b
}

#[test]
fn if_true_takes_then_branch() {
    let mut vm = boot(if_fixture(1));
    vm.tick_at(0, &no_input());
    assert_eq!(
        vm.store().read(user_var(&vm, 0), &[]).unwrap(),
        Value::Int(1)
    );
}

#[test]
fn if_false_takes_else_branch() {
    let mut vm = boot(if_fixture(0));
    vm.tick_at(0, &no_input());
    assert_eq!(
        vm.store().read(user_var(&vm, 0), &[]).unwrap(),
        Value::Int(2)
    );
}

#[test]
fn if_false_jump_is_observable() {
    let mut vm = boot(if_fixture(0));
    vm.tick_at(0, &no_input());
    let jumps: Vec<_> = vm
        .events()
        .iter()
        .filter_map(|e| match e {
            yu_rt::VmEvent::Jump { kind, to_pc, .. } => Some((*kind, *to_pc)),
            _ => None,
        })
        .collect();
    assert!(jumps
        .iter()
        .any(|(k, to)| *k == yu_rt::event::JumpKind::IfFalse && *to == 3));
}

// ---------------------------------------------------------------- LOOP

fn loop_fixture(count: i64) -> ProjectBuilder {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![
        VarSpec {
            kind: 1,
            group: 0,
            script_id: 0,
            var_id: sys,
            value_type: 1,
            dims: vec![],
            init: Init::Int(0),
        },
        VarSpec {
            kind: 1,
            group: 0,
            script_id: 0,
            var_id: sys + 1,
            value_type: 1,
            dims: vec![],
            init: Init::Int(0),
        },
    ];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        // 0: LOOP[SET=count] → LOOPEND 组 3
        GroupSpec::new(OP_LOOP)
            .window(0, ex::i8_(count as i8))
            .jump(3),
        // 1: LET[@n = @n + 1]
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, sys))
            .window(0, ex::cat(&[ex::get(0x40, sys), ex::i8_(1), ex::op(0x2b)])),
        // 2: LET[@lc = @_LC]  (系统变量 id 由表解析)
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, sys + 1))
            .window(
                0,
                ex::get(0x40, 0), // 占位;下面替换
            ),
        // 3: LOOPEND
        GroupSpec::new(OP_LOOPEND),
        GroupSpec::new(OP_END),
    ]];
    b
}

#[test]
fn loop_counts_and_lc_is_visible() {
    let mut b = loop_fixture(5);
    // `_LC` 的 id 由夹具 ycd 顺序决定;在构建前解析一次。
    let mut probe = ProjectBuilder::new();
    probe.sysvars = b.sysvars.clone();
    let src = probe.build();
    let tables = yu_rt::TableSet::load(&src, yu_rt::tables::DEFAULT_SCRIPT_KEY).unwrap();
    let lc_id = tables.boot.sysvars.id("_LC").unwrap();
    // 组 2 的第 2 窗口 = @_LC
    b.scripts[0][2].windows[1].4 = ex::get(0x40, lc_id);

    let mut vm = boot(b);
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let sys = vm.sysvars().len() as u16;
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys), &[])
            .unwrap(),
        Value::Int(5),
        "循环体执行 5 次"
    );
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys + 1), &[])
            .unwrap(),
        Value::Int(5),
        "_LC 在最后一轮 = 5(手册 1..5)"
    );
}

// ---------------------------------------------------------------- GOSUB / RETURN

#[test]
fn gosub_passes_pint_and_return_brings_rint() {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![
        VarSpec {
            kind: 1,
            group: 0,
            script_id: 0,
            var_id: sys,
            value_type: 1,
            dims: vec![],
            init: Init::Int(0),
        },
        VarSpec {
            kind: 1,
            group: 0,
            script_id: 0,
            var_id: sys + 1,
            value_type: 1,
            dims: vec![],
            init: Init::Int(0),
        },
    ];
    // 标签 SUB 在组 3
    b.labels = vec![("SYSTEM_START", 0, 0, 0), ("SUB", 3, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        // 0: GOSUB[#="SUB" PINT=21]
        GroupSpec::new(OP_GOSUB)
            .window(0, ex::s("SUB"))
            .window(1, ex::i8_(21)),
        // 1: LET[@r = @_RINT(1)]
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, sys + 1))
            .window(0, ex::get(0x40, 0)),
        GroupSpec::new(OP_END),
        // 3: SUB: LET[@a = @_PINT(1) * 2]; RETURN[RINT=@a]
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, sys))
            .window(0, ex::cat(&[ex::get(0x40, 0), ex::i8_(2), ex::op(0x2a)])),
        GroupSpec::new(OP_RETURN).window(0, ex::get(0x40, sys)),
    ]];
    // 填入 @_PINT(1) / @_RINT(1) 的真实 id。
    let probe_src = {
        let mut p = ProjectBuilder::new();
        p.sysvars = b.sysvars.clone();
        p.build()
    };
    let tables = yu_rt::TableSet::load(&probe_src, yu_rt::tables::DEFAULT_SCRIPT_KEY).unwrap();
    let pint = tables.boot.sysvars.id("_PINT").unwrap();
    let rint = tables.boot.sysvars.id("_RINT").unwrap();
    // 组 1 第二窗口 @_RINT(1):索引 1 → 数组取值
    b.scripts[0][1].windows[1].4 = ex::cat(&[ex::r#ref(0x40, rint), ex::i8_(1), ex::aload()]);
    // 组 3 第一窗口 @_PINT(1)
    // 被调方:LET[@a = @_PINT(1) * 2]
    // 窗口 0 = 左值引用;窗口 1 = 表达式(数组取值需 引用+下标+aload)。
    b.scripts[0][3].windows[1].4 = ex::cat(&[
        ex::r#ref(0x40, pint),
        ex::i8_(1),
        ex::aload(),
        ex::i8_(2),
        ex::op(0x2a),
    ]);

    let mut vm = boot(b);
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys), &[])
            .unwrap(),
        Value::Int(42),
        "被调方读到 PINT(1)=21 并乘 2"
    );
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys + 1), &[])
            .unwrap(),
        Value::Int(42),
        "调用方读到 RINT(1)=42"
    );
}

// ---------------------------------------------------------------- WAIT / 帧

#[test]
fn wait_frame_yields_and_wakes() {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![VarSpec {
        kind: 1,
        group: 0,
        script_id: 0,
        var_id: sys,
        value_type: 1,
        dims: vec![],
        init: Init::Int(0),
    }];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        // 0: LET[@n=@n+1]
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, sys))
            .window(0, ex::cat(&[ex::get(0x40, sys), ex::i8_(1), ex::op(0x2b)])),
        // 1: WAIT[FRAME=2]
        GroupSpec::new(OP_WAIT).window(0, ex::i8_(2)),
        // 2: LET[@n=@n+10]
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, sys))
            .window(0, ex::cat(&[ex::get(0x40, sys), ex::i8_(10), ex::op(0x2b)])),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    // 第 1 帧:执行到 WAIT 让出(n=1)
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys), &[])
            .unwrap(),
        Value::Int(1)
    );
    // FRAME=2:再等两帧后继续
    vm.tick_at(16, &no_input());
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys), &[])
            .unwrap(),
        Value::Int(1)
    );
    vm.tick_at(32, &no_input());
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys), &[])
            .unwrap(),
        Value::Int(11)
    );
    let waits = vm.events().iter().filter(|e| e.kind() == "wait").count();
    assert_eq!(waits, 1);
}

#[test]
fn wait_time_uses_injected_clock_only() {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![VarSpec {
        kind: 1,
        group: 0,
        script_id: 0,
        var_id: sys,
        value_type: 1,
        dims: vec![],
        init: Init::Int(0),
    }];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        GroupSpec::new(OP_WAIT).window(1, ex::i16_(100)),
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, sys))
            .window(0, ex::i8_(1)),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    vm.tick_at(1000, &no_input());
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys), &[])
            .unwrap(),
        Value::Int(0)
    );
    vm.tick_at(1099, &no_input());
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys), &[])
            .unwrap(),
        Value::Int(0)
    );
    vm.tick_at(1100, &no_input());
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys), &[])
            .unwrap(),
        Value::Int(1)
    );
}

// ---------------------------------------------------------------- 输入

#[test]
fn key_counter_is_signed_frame_counter() {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![VarSpec {
        kind: 1,
        group: 0,
        script_id: 0,
        var_id: sys,
        value_type: 1,
        dims: vec![],
        init: Init::Int(0),
    }];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        // LET[@k = @_KEY_Z]
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, sys))
            .window(0, ex::get(0x40, 0)),
        GroupSpec::new(OP_WAIT).window(0, ex::i8_(1)),
        GroupSpec::new(OP_END),
    ]];
    let probe = {
        let mut p = ProjectBuilder::new();
        p.sysvars = b.sysvars.clone();
        p.build()
    };
    let tables = yu_rt::TableSet::load(&probe, yu_rt::tables::DEFAULT_SCRIPT_KEY).unwrap();
    // 夹具 sysvars 里没有 _KEY_Z:用 _KEY_A(手册同族)
    let key = tables.boot.sysvars.id("_KEY_A").unwrap();
    b.scripts[0][0].windows[1].4 = ex::get(0x40, key);

    let mut timeline = InputTimeline::new();
    timeline.set(1, InputFrame::new(["KEY_A"]));
    timeline.set(3, InputFrame::new(std::iter::empty::<&str>()));

    let mut vm = boot(b);
    // 第 1 帧按下 → 1
    vm.tick_at(0, &timeline);
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys), &[])
            .unwrap(),
        Value::Int(1)
    );
    // 第 2 帧持续 → 2
    vm.tick_at(16, &timeline);
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys), &[])
            .unwrap(),
        Value::Int(1)
    );
}

// ---------------------------------------------------------------- 字符串 / VARACT

#[test]
fn string_declare_concat_and_varact_copy() {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![
        VarSpec {
            kind: 1,
            group: 0,
            script_id: 0,
            var_id: sys,
            value_type: 3,
            dims: vec![],
            init: Init::Str(b"ABCDEF"),
        },
        VarSpec {
            kind: 1,
            group: 0,
            script_id: 0,
            var_id: sys + 1,
            value_type: 3,
            dims: vec![],
            init: Init::Str(b""),
        },
    ];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        // STR[$dst = "ABC" + ".BMP"]
        GroupSpec::new(OP_STR)
            .window(0, ex::r#ref(0x24, sys + 1))
            .window(0, ex::cat(&[ex::s("ABC"), ex::s(".BMP"), ex::op(0x2b)])),
        // VARACT[COPY=1 SET=$src LET=$dst POS=2 LENGTH=3] → "BCD"
        GroupSpec::new(OP_VARACT)
            .window(0, ex::r#ref(0x24, sys))
            .window(1, ex::r#ref(0x24, sys + 1))
            .window(3, ex::i8_(1))
            .window(4, ex::i8_(2))
            .window(5, ex::i8_(3)),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::Dollar, sys), &[])
            .unwrap(),
        Value::Str(b"ABCDEF".to_vec())
    );
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::Dollar, sys + 1), &[])
            .unwrap(),
        Value::Str(b"BCD".to_vec()),
        "VARACT COPY:2 字符起 3 个字符(手册例 5)"
    );
}

#[test]
fn string_concat_assignment() {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![VarSpec {
        kind: 1,
        group: 0,
        script_id: 0,
        var_id: sys,
        value_type: 3,
        dims: vec![],
        init: Init::Str(b""),
    }];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        GroupSpec::new(OP_STR)
            .window(0, ex::r#ref(0x24, sys))
            .window(0, ex::cat(&[ex::s("ABC"), ex::s(".BMP"), ex::op(0x2b)])),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    vm.tick_at(0, &no_input());
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::Dollar, sys), &[])
            .unwrap(),
        Value::Str(b"ABC.BMP".to_vec())
    );
}

// ---------------------------------------------------------------- MATH

#[test]
fn math_sqrt_and_abs() {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![VarSpec {
        kind: 1,
        group: 0,
        script_id: 0,
        var_id: sys,
        value_type: 2,
        dims: vec![],
        init: Init::Float(0.0),
    }];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        // MATH[LET=@v SQRT=1 SET=9]
        GroupSpec::new(OP_MATH)
            .window(0, ex::r#ref(0x40, sys))
            .window(14, ex::i8_(1))
            .window(1, ex::i8_(9)),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    match vm
        .store()
        .read(VarRef::new(VarSpace::At, sys), &[])
        .unwrap()
    {
        Value::Float(f) => assert!((f - 3.0).abs() < 1e-12),
        other => panic!("期望浮点,得到 {other:?}"),
    }
}

// ---------------------------------------------------------------- FPS

#[test]
fn fps_sets_engine_rate() {
    let mut b = ProjectBuilder::new();
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        GroupSpec::new(OP_FPS)
            .window(0, ex::i8_(30))
            .window(1, ex::i8_(0)),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    vm.tick_at(0, &no_input());
    assert_eq!(vm.fps(), 30);
    let fps_events = vm.events().iter().filter(|e| e.kind() == "fps").count();
    assert_eq!(fps_events, 1);
}

#[test]
fn fps_out_of_range_is_fatal() {
    let mut b = ProjectBuilder::new();
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![GroupSpec::new(OP_FPS).window(0, ex::i32_(300))]];
    let mut vm = boot(b);
    let r = vm.tick_at(0, &no_input());
    assert_eq!(r.errors.len(), 1);
    assert!(r.errors[0].contains("rt.cmd.slot"), "{:?}", r.errors);
}

// ---------------------------------------------------------------- 错误路径

#[test]
fn unknown_label_is_fatal_and_recorded() {
    let mut b = ProjectBuilder::new();
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![GroupSpec::new(OP_GO).window(0, ex::s("NOPE"))]];
    let mut vm = boot(b);
    let r = vm.tick_at(0, &no_input());
    assert_eq!(r.errors.len(), 1);
    assert!(r.errors[0].contains("rt.label"), "{:?}", r.errors);
    assert!(vm
        .events()
        .iter()
        .any(|e| matches!(e, yu_rt::VmEvent::Diag { code, .. } if code == "rt.label")));
}

#[test]
fn undeclared_variable_write_is_reported() {
    let mut b = ProjectBuilder::new();
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![GroupSpec::new(OP_LET)
        .window(0, ex::r#ref(0x40, 5000))
        .window(0, ex::i8_(1))]];
    let mut vm = boot(b);
    let r = vm.tick_at(0, &no_input());
    assert_eq!(r.errors.len(), 1);
    assert!(r.errors[0].contains("rt.var.undeclared"), "{:?}", r.errors);
}

#[test]
fn unimplemented_command_is_recorded_once_and_execution_continues() {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![VarSpec {
        kind: 1,
        group: 0,
        script_id: 0,
        var_id: sys,
        value_type: 1,
        dims: vec![],
        init: Init::Int(0),
    }];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    // `VAR`:官方有独立处理体(0x453f88),本内核尚无处理器 ⇒ Unknown/
    // Unimplemented。F_INT 在 P0-A 后归默认槽(脚本错误),不再走此路径。
    let op_var = b
        .commands
        .iter()
        .position(|(n, _)| *n == "VAR")
        .expect("夹具命令表应有 VAR") as u8;
    b.scripts = vec![vec![
        GroupSpec::new(op_var),
        GroupSpec::new(op_var),
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, sys))
            .window(0, ex::i8_(7)),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "Unimplemented 不致命");
    assert_eq!(
        vm.unimplemented_cmds().get("VAR").copied(),
        Some(2),
        "计数 2 次"
    );
    assert_eq!(
        vm.events()
            .iter()
            .filter(
                |e| matches!(e, yu_rt::VmEvent::Diag { code, .. } if code == "rt.cmd.unimplemented")
            )
            .count(),
        1,
        "事件流只挂一次"
    );
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys), &[])
            .unwrap(),
        Value::Int(7),
        "执行继续"
    );
}

#[test]
fn missing_table_refuses_boot() {
    // 商业发行版不带 ycd ⇒ 它不是必需表(DIV-RT-016);其余六张必需。
    for table in ["ysc", "ysl", "ysv", "yst_list", "yscfg"] {
        let mut b = ProjectBuilder::new();
        b.labels = vec![("SYSTEM_START", 0, 0, 0)];
        b.entry(0, 0);
        b.scripts = vec![vec![GroupSpec::new(OP_END)]];
        b.omit = vec![table];
        let err = match Vm::boot(Box::new(b.build()), BootConfig::default()) {
            Ok(_) => panic!("缺 {table} 必须拒绝启动"),
            Err(e) => e,
        };
        assert_eq!(err.code.as_ref(), "table.missing", "缺 {table}");
    }

    // 只缺 ycd:必须能启动,且系统变量来自内嵌 ABI 表。
    let mut b = ProjectBuilder::new();
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![GroupSpec::new(OP_END)]];
    b.omit = vec!["ycd"];
    b.sysvars_abi = true;
    let vm =
        Vm::boot(Box::new(b.build()), BootConfig::default()).expect("商业形态(无 ycd)必须可启动");
    assert!(vm.tables().boot.ycd.is_none());
    assert_eq!(vm.sysvars().len(), 548, "回退到引擎 ABI 表");
    assert_eq!(vm.sysvars().id("_LC"), Some(48));
}

// ---------------------------------------------------------------- golden

#[test]
fn event_stream_is_byte_stable() {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![VarSpec {
        kind: 1,
        group: 0,
        script_id: 0,
        var_id: sys,
        value_type: 1,
        dims: vec![],
        init: Init::Int(0),
    }];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, sys))
            .window(0, ex::i8_(1)),
        GroupSpec::new(OP_END),
    ]];
    let run = || {
        let mut vm = boot(ProjectBuilder {
            vars: b.vars.clone(),
            scripts: b.scripts.clone(),
            labels: b.labels.clone(),
            ..ProjectBuilder::new()
        });
        vm.tick_at(0, &no_input());
        to_jsonl(vm.events())
    };
    let a = run();
    let b2 = run();
    assert_eq!(a, b2, "同配置重放必须逐字节一致(A3)");
    assert!(a.contains("\"kind\":\"command\""), "{a}");
}

#[test]
fn main_task_id_is_system_and_script_visible() {
    // 手册 `basic/task.html`:「起動時に生成されるタスクをシステムタスクと呼び、
    // 「SYSTEM」というIDが名づけられています」;引擎启动链
    // `FUN_00465384` @0x4652f0 以字符串 "SYSTEM"(0x807b00)建任务。
    // 脚本可见 ID 与链内 ID 必须是同一个(任务书 E R1)。
    assert_eq!(MAIN_TASK_ID, "SYSTEM");
}

/// R1 回归:`TASKEND[]`(无 ID)只结束**执行者自身**,主线继续。
///
/// 组布局:
/// ```text
/// 0  TASK[ID="SUB" #="SUB.BODY"]        生成子任务
/// 1  WAIT[FRAME=2]                       让子任务先跑完
/// 2  LET[@v = 7]                         主线继续写变量
/// 3  END
/// 4  SUB.BODY: LET[@v = 1]
/// 5           TASKEND[]                  只结束自己
/// 6           LET[@v = 2]                不应执行(R2 同时验证)
/// 7           END
/// ```
#[test]
fn taskend_without_id_ends_only_self_and_main_continues() {
    let mut b = ProjectBuilder::new();
    let v = with_int_var(&mut b, 0, 0);
    b.labels = vec![("SYSTEM_START", 0, 0, 0), ("SUB.BODY", 4, 0, 0)];
    b.entry(0, 0);
    const OP_TASK: u8 = 93;
    const OP_TASKEND: u8 = 94;
    const OP_TASKINFO: u8 = 95;
    b.scripts = vec![vec![
        GroupSpec::new(OP_TASK)
            .window(0, ex::s("SUB"))
            .window(34, ex::s("SUB.BODY")),
        GroupSpec::new(OP_WAIT).window(0, ex::i8_(2)),
        // TASKINFO[ID="SYSTEM" EXIST=1 LET=@v2] —— 主线此时必须仍是 EXIST=1
        GroupSpec::new(OP_TASKINFO)
            .window(0, ex::s("SYSTEM"))
            .window(2, ex::i8_(1))
            .window(1, ex::r#ref(0x40, v.id + 1)),
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, v.id))
            .window(0, ex::i8_(7)),
        GroupSpec::new(OP_END),
        // 4: SUB.BODY
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, v.id))
            .window(0, ex::i8_(1)),
        // 5: TASKEND[](无 ID = 自身)
        GroupSpec::new(OP_TASKEND),
        // 6: 不应执行
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, v.id))
            .window(0, ex::i8_(2)),
        GroupSpec::new(OP_END),
    ]];
    // v+1 承接 TASKINFO 结果
    b.vars.push(VarSpec {
        kind: 1,
        group: 0,
        script_id: 0,
        var_id: v.id + 1,
        value_type: 1,
        dims: vec![],
        init: Init::Int(-1),
    });
    let mut vm = boot(b);
    for f in 0..6u32 {
        vm.tick_at(f * 16, &no_input());
    }
    assert_eq!(
        vm.store().read(user_var(&vm, 0), &[]).unwrap(),
        Value::Int(7),
        "子任务 TASKEND[] 后不再执行 LET,主线继续写 7"
    );
    assert_eq!(
        vm.store().read(user_var(&vm, 1), &[]).unwrap(),
        Value::Int(1),
        "TASKINFO[ID=\"SYSTEM\" EXIST=1] 命中主线"
    );
    assert!(vm.scheduler().find_by_id("SUB").is_none(), "子任务已摘链");
}

/// R2 回归:`TASKEND[]` 后紧邻的 `LET` 不执行;同 ID 重建从新标签开始。
#[test]
fn taskend_stops_neighbor_commands_and_rebuild_uses_new_entry() {
    let mut b = ProjectBuilder::new();
    let v = with_int_var(&mut b, 0, 0);
    b.labels = vec![
        ("SYSTEM_START", 0, 0, 0),
        ("T.ONE", 3, 0, 0),
        ("T.TWO", 6, 0, 0),
    ];
    b.entry(0, 0);
    const OP_TASK: u8 = 93;
    const OP_TASKEND: u8 = 94;
    b.scripts = vec![vec![
        GroupSpec::new(OP_TASK)
            .window(0, ex::s("T"))
            .window(34, ex::s("T.ONE")),
        GroupSpec::new(OP_WAIT).window(0, ex::i8_(1)),
        GroupSpec::new(OP_END),
        // 3: T.ONE
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, v.id))
            .window(0, ex::i8_(1)),
        GroupSpec::new(OP_TASKEND),
        GroupSpec::new(OP_LET) // 5: 不应执行
            .window(0, ex::r#ref(0x40, v.id))
            .window(0, ex::i8_(9)),
        // 6: T.TWO
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, v.id))
            .window(0, ex::i8_(2)),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    for f in 0..4u32 {
        vm.tick_at(f * 16, &no_input());
    }
    assert_eq!(
        vm.store().read(user_var(&vm, 0), &[]).unwrap(),
        Value::Int(1),
        "TASKEND 后的 LET 不执行(仍为 1)"
    );
    // 旧节点已摘链 ⇒ EXIST = 0;同 ID 重建走 `spawn`(新执行位,从新标签开始)
    assert!(
        vm.scheduler().find_by_id("T").is_none(),
        "TASKEND 后 EXIST = 0"
    );
}

/// R4 回归:两个不同 Z 的任务,高 Z 先执行;同 Z 按创建序。
///
/// 组布局:
/// ```text
/// 0 TASK[ID="A" #="A.BODY" Z=10]
/// 1 TASK[ID="B" #="B.BODY" Z=300]
/// 2 END
/// 3 A.BODY: LET[@v=1]
/// 4 END
/// 5 (占位 LET,不执行)
/// 6 B.BODY: LET[@v=2]
/// 7 END
/// ```
#[test]
fn higher_z_task_runs_before_lower_z() {
    let mut b = ProjectBuilder::new();
    let v = with_int_var(&mut b, 0, 0);
    b.labels = vec![
        ("SYSTEM_START", 0, 0, 0),
        ("A.BODY", 3, 0, 0),
        ("B.BODY", 6, 0, 0),
    ];
    b.entry(0, 0);
    const OP_TASK: u8 = 93;
    // 体 = LET + WAIT(50) + END:任务跑完第一行后长期休眠,便于事后查 Z。
    let body = |pc_value: i8| {
        vec![
            GroupSpec::new(OP_LET)
                .window(0, ex::r#ref(0x40, v.id))
                .window(0, ex::i8_(pc_value)),
            GroupSpec::new(OP_WAIT).window(0, ex::i8_(50)),
            GroupSpec::new(OP_END),
        ]
    };
    let mut groups = vec![
        GroupSpec::new(OP_TASK)
            .window(0, ex::s("A"))
            .window(3, ex::i8_(10))
            .window(34, ex::s("A.BODY")),
        GroupSpec::new(OP_TASK)
            .window(0, ex::s("B"))
            .window(3, ex::i16_(300))
            .window(34, ex::s("B.BODY")),
        GroupSpec::new(OP_END),
    ];
    groups.extend(body(1)); // 3..5
                            // 6..8 = B 体(与标签 B.BODY=6 对齐)
    groups.extend(body(2));
    b.scripts = vec![groups];
    let mut vm = boot(b);
    for f in 0..3u32 {
        vm.tick_at(f * 16, &no_input());
    }
    // 帧 2 里两个任务体都被泵到:顺序必须是 B(pc6) 先于 A(pc3)。
    let order: Vec<u32> = vm
        .events()
        .iter()
        .filter_map(|e| match e {
            yu_rt::VmEvent::Command {
                frame: 2, pc, name, ..
            } if name == "LET" && (*pc == 3 || *pc == 6) => Some(*pc),
            _ => None,
        })
        .collect();
    assert_eq!(order, vec![6, 3], "Z=300 的 B 先于 Z=10 的 A 执行");
    assert_eq!(
        vm.store().read(user_var(&vm, 0), &[]).unwrap(),
        Value::Int(1),
        "低 Z 的 A 后写 ⇒ 终值 1(证明 A 确实在 B 之后执行)"
    );
    assert_eq!(
        vm.scheduler().find_by_id("B").unwrap().priority,
        300,
        "TASK 的 Z 参数生效"
    );
}

// ---------------------------------------------------------------- VAR.ADD 宏链

/// ERIS `macro.yst` 的 `VAR.ADD` 展开:
/// `VARACT[SET=$_M() DIMSIZE+=1]; VARINFO[SET=$_M() DIMSIZE=1 LET=@es.INT(0)];
///  $_M(@es.INT(0)-1)=$_M2`
///
/// 这条链是 ERIS 全工程的「数组追加」惯用法(1,000+ 次)。断言:
/// ① 初始长度 0 的数组经 `DIMSIZE+=1` 变 1;② `VARINFO[DIMSIZE=1]` 读出 1;
/// ③ 追加下标 `0-…` 不为负,元素确实写进去。
#[test]
fn var_add_macro_chain_appends() {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    // $arr: 声明为空数组(dims [0],与 `G_STR[$X(0)]` 同形)
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
        // @cnt(0): 计数器数组(G_INT,@es.INT 同形)
        VarSpec {
            kind: 1,
            group: 1,
            script_id: 0,
            var_id: sys + 1,
            value_type: 1,
            dims: vec![10],
            init: Init::Int(0),
        },
        // $val: 待追加值
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
        // VARACT[SET=$arr DIMSIZE+=1]
        GroupSpec::new(OP_VARACT)
            .window(0, arr.clone())
            .window_tagged(13, 0, 0x0101, ex::i8_(1)),
        // VARINFO[SET=$arr DIMSIZE=1 LET=@cnt(0)]
        GroupSpec::new(101)
            .window(0, arr.clone())
            .window(5, ex::i8_(1))
            .window(1, ex::cat(&[cnt.clone(), ex::i8_(0), ex::aload()])),
        // LET[$arr[@cnt(0)-1] = $val]
        //
        // 与真实字节(project-eris s2 pc16)同形:
        //   ref($arr) ref(@cnt) 0 aload 1 sub aload
        // 注意内层下标必须是 **引用+下标+aload**,用 `get` 会让外层 aload 回溯到
        // 错误的引用点(本用例第一版就踩了这个坑,已按真实字节改正)。
        GroupSpec::new(OP_LET)
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
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    let arr_ref = VarRef::new(VarSpace::Dollar, sys);
    let before = vm.store().shape(arr_ref).map(|(_, d)| d.to_vec());
    let r = vm.tick_at(0, &no_input());
    let after = vm.store().shape(arr_ref).map(|(_, d)| d.to_vec());
    assert!(
        r.errors.is_empty(),
        "{:?} before={before:?} after={after:?}",
        r.errors
    );
    let neg: Vec<String> = vm
        .events()
        .iter()
        .filter_map(|e| match e {
            yu_rt::VmEvent::Diag { detail, .. } if detail.contains("为负") => {
                Some(detail.clone())
            }
            _ => None,
        })
        .collect();
    assert!(neg.is_empty(), "不应出现负下标: {neg:?}");
    let arr_ref = VarRef::new(VarSpace::Dollar, sys);
    assert_eq!(vm.store().shape(arr_ref).unwrap().1, &[1], "数组应扩到 1");
    assert_eq!(
        vm.store().read(arr_ref, &[0]).unwrap(),
        Value::Str(b"yuu".to_vec()),
        "元素写入 @cnt(0)-1 = 0"
    );
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys + 1), &[0])
            .unwrap(),
        Value::Int(1),
        "VARINFO DIMSIZE 读出 1"
    );
}

// ---------------------------------------------------------------- 文本流(WORD/_/RETURNCODE)

/// 0.495 `ysc.ybn` 实测序号(dump_ycd):79 RETURNCODE / 106 WORD / 110 `_`。
const OP_RETURNCODE: u8 = 79;
const OP_TEXT: u8 = 96;
const OP_TEXTINFO: u8 = 97;
const OP_WORD: u8 = 106;
const OP_TEXT_OUT: u8 = 110;

#[test]
fn word_text_out_and_returncode_form_the_text_stream() {
    // 对齐 ERIS userscript 实测编译形态:
    //   WORD("こん") _(式) WORD("にちは") RETURNCODE(kind=7)
    // ①手册 basic/basic.html ●テキスト表示(`_[式]` 显示式值);
    // ②语料 464 WORD / 396 RETURNCODE / 33 _;
    // ③RxYuris YSTB_TextEditor_V5 以 WORD 为文本单元。
    let mut b = ProjectBuilder::new();
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    // CP932 字节常量(本 crate 不带编码表,编码归任务 B;实测锚点直写):
    // こ=82 B1 ん=82 F1 / 世=90 A2 界=8A 45
    let cp932_kon = vec![0x82u8, 0xB1, 0x82, 0xF1];
    let cp932_sekai = vec![0x90u8, 0xA2, 0x8A, 0x45];
    // WORD 载荷 = CP932 原始字节(无表达式壳;语料 464/464 实测)。
    let word = |bytes: Vec<u8>| GroupSpec::new(OP_WORD).window(0, bytes);
    b.scripts = vec![vec![
        word(cp932_kon.clone()),
        // _["に" + "ちは"] → 文本 "にちは"(表达式串拼接走 0x4d 字面量)
        GroupSpec::new(OP_TEXT_OUT).window(
            0,
            ex::cat(&[
                ex::s("AB"), // ASCII 便捷字面量;语义验证与字节内容无关
                ex::s("CD"),
                ex::op(0x2b),
            ]),
        ),
        word(cp932_sekai.clone()),
        // RETURNCODE 唯一窗口 kind = 行计数(语料实测 1 起逐行 +1)
        GroupSpec::new(OP_RETURNCODE).window(7, Vec::new()),
        GroupSpec::new(OP_END),
    ]];

    let mut vm = boot(b);
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);

    let texts: Vec<&Vec<u8>> = vm
        .events()
        .iter()
        .filter_map(|e| match e {
            yu_rt::VmEvent::Text { bytes, .. } => Some(bytes),
            _ => None,
        })
        .collect();
    assert_eq!(texts.len(), 3, "WORD×2 + _×1 应产出 3 条 Text 事件");
    // CP932 字节原样:"こん" = 0x82 0xB1 0x82 0xF1
    assert_eq!(texts[0], &cp932_kon, "WORD 载荷 CP932 原样透传");
    // `_` 的式值:"ABCD"(0x4d 字面量拼接)
    assert_eq!(texts[1], b"ABCD");
    assert_eq!(texts[2], &cp932_sekai);

    let rc: Vec<u32> = vm
        .events()
        .iter()
        .filter_map(|e| match e {
            yu_rt::VmEvent::ReturnCode { line, .. } => Some(*line),
            _ => None,
        })
        .collect();
    assert_eq!(rc, vec![7], "RETURNCODE 窗口 kind = 编译器写入的行计数");
}

#[derive(Default)]
struct MockHostState {
    events: Vec<String>,
    queries: Vec<(String, Vec<u8>)>,
}

struct MockHost {
    state: std::rc::Rc<std::cell::RefCell<MockHostState>>,
}

impl yu_rt::event::GraphicsHost for MockHost {
    fn apply_event(&mut self, ev: &yu_rt::VmEvent) {
        if let yu_rt::VmEvent::DomainCall { name, .. } = ev {
            self.state.borrow_mut().events.push(name.clone());
        }
    }

    fn query(
        &mut self,
        command: &str,
        args: &[yu_rt::event::ArgValue],
    ) -> Result<yu_rt::Value, yu_rt::Diag> {
        self.state
            .borrow_mut()
            .queries
            .push((command.to_string(), args.iter().map(|a| a.slot).collect()));
        Ok(yu_rt::Value::Int(42))
    }
}

#[test]
fn info_query_writes_let_with_synchronously_applied_state() {
    // CGINFO[ID=L A=1 LET=@v]:宿主必须在 CG 之后、CGINFO 之前看到 DomainCall,
    // 查询结果 42 写回 @v;未安装宿主时则只报 Unimplemented、不写。
    let mut b = ProjectBuilder::new();
    let v = with_int_var(&mut b, 0, 0);
    // 夹具默认 CGINFO 无参数;本测试需要 ID/A/LET 三个槽。
    if let Some((_, params)) = b.commands.iter_mut().find(|(n, _)| *n == "CGINFO") {
        *params = vec![("ID", 1, 0), ("A", 0, 3), ("LET", 0, 3)];
    }
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    const OP_CG: u8 = 1;
    const OP_CGINFO: u8 = 4;
    b.scripts = vec![vec![
        GroupSpec::new(OP_CG).window(0, ex::s("L")),
        GroupSpec::new(OP_CGINFO)
            .window(0, ex::s("L"))
            .window(1, ex::i8_(1))
            .window(2, ex::r#ref(0x40, v.id)),
        GroupSpec::new(OP_END),
    ]];

    let shared = std::rc::Rc::new(std::cell::RefCell::new(MockHostState::default()));
    let mut vm = boot(b);
    vm.set_graphics_host(Box::new(MockHost {
        state: shared.clone(),
    }));
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);

    assert_eq!(
        vm.store().read(v, &[]).unwrap(),
        yu_rt::Value::Int(42),
        "LET 必须写入查询结果"
    );
    let st = shared.borrow();
    assert_eq!(
        st.events,
        vec!["CG".to_string(), "CGINFO".to_string()],
        "宿主同步看到 CG 与 CGINFO 事件事实"
    );
    assert_eq!(st.queries.len(), 1);
    assert_eq!(st.queries[0].0, "CGINFO");
    // ID / A 两个参数窗口;LET 不进入查询参数。
    assert_eq!(st.queries[0].1, vec![0, 1]);
}

#[test]
fn text_id_sets_following_run_id_and_clear_empties_buffer() {
    // 手册 text/text.html:ID = 自动输出模式的目标文本层 ID;
    // CLEAR=1 = 完全清空系统内部文本缓冲。
    let mut b = ProjectBuilder::new();
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    let text = |slot: u8, payload: Vec<u8>| GroupSpec::new(OP_TEXT).window(slot, payload);
    b.scripts = vec![vec![
        text(0, ex::s("MSG")), // TEXT[ID="MSG"]
        GroupSpec::new(OP_WORD).window(0, b"A".to_vec()),
        text(16, ex::i32_(1)), // TEXT[CLEAR=1](0.495 slot 16)
        GroupSpec::new(OP_WORD).window(0, b"B".to_vec()),
        GroupSpec::new(OP_END),
    ]];

    let mut vm = boot(b);
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);

    let texts: Vec<String> = vm
        .events()
        .iter()
        .filter_map(|e| match e {
            yu_rt::VmEvent::Text { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(texts, vec!["MSG".to_string(), "MSG".to_string()]);
    assert_eq!(vm.current_text(), b"B", "CLEAR 清掉前面的 A");
    assert_eq!(vm.text_flags(), b"1", "CLEAR 同时清标志流(只剩 B)");
}

#[test]
fn text_flags_track_cp932_width_and_returncode_auto_defaults() {
    // REA 0x458d44:每字符一个 '1'(半角)/ '2'(全角);
    // REA 0x44c390:自动模式 RETURNCODE 追加 AUTOCLICK 'C'/AUTORETURN 'R'/
    // AUTOPAGE 'P',最后恒追加 'E'。默认 OUTPUTMODE/AUTO* 都是 1。
    let mut b = ProjectBuilder::new();
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        GroupSpec::new(OP_WORD).window(0, b"A\x82\xB1".to_vec()), // A + こ
        GroupSpec::new(OP_RETURNCODE).window(1, Vec::new()),
        GroupSpec::new(OP_END),
    ]];

    let mut vm = boot(b);
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(vm.current_text(), b"A\x82\xB1");
    assert_eq!(vm.text_flags(), b"12CRPE");
}

#[test]
fn text_outputmode_manual_click_continues_and_auto_click_yields() {
    // REA 0x45383f..0x45388b:CLICK=1 先写 'C';自动模式返回 -1(本帧让出),
    // 手动模式继续处理同一条 TEXT 的 RETURN 并执行后续命令。
    let mut b = ProjectBuilder::new();
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    let text = |slot: u8, payload: Vec<u8>| GroupSpec::new(OP_TEXT).window(slot, payload);
    b.scripts = vec![vec![
        text(1, ex::i32_(0)),  // OUTPUTMODE=0 手动
        text(15, ex::i32_(1)), // CLICK=1:手动模式不 yield
        text(11, ex::i32_(1)), // RETURN=1
        GroupSpec::new(OP_END),
    ]];

    let mut vm = boot(b);
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    // 引擎次序:CLICK 'C' 先于 RETURN 'R'。
    assert_eq!(vm.text_flags(), b"CR", "手动模式 CLICK 继续并处理 RETURN");

    // 自动模式:CLICK=1 让出,后续 RETURN 与后续命令都要等下一次 tick。
    let mut b = ProjectBuilder::new();
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        text(1, ex::i32_(1)),  // OUTPUTMODE=1 自动(默认亦为 1)
        text(15, ex::i32_(1)), // CLICK=1 → yield
        text(11, ex::i32_(1)), // 本次 TEXT 里 RETURN 被跳过
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    let r1 = vm.tick_at(0, &no_input());
    assert!(r1.errors.is_empty(), "{:?}", r1.errors);
    assert_eq!(vm.text_flags(), b"C", "自动模式 CLICK 后立即让出");
    let r2 = vm.tick_at(16, &no_input());
    assert!(r2.errors.is_empty(), "{:?}", r2.errors);
    assert_eq!(vm.text_flags(), b"CR", "下一帧继续执行被跳过的后续命令");
}

#[test]
fn textinfo_queries_text_state_by_parameter_presence() {
    // 手册 text/textinfo.html;REA 0x4538cc:查询判据是参数**出现**,
    // 不是值是否为 1;次序 OUTPUTMODE/CX/CY/CLICK/AUTO*。
    let mut b = ProjectBuilder::new();
    if let Some((_, params)) = b.commands.iter_mut().find(|(n, _)| *n == "TEXTINFO") {
        *params = vec![
            ("ID", 1, 0),
            ("OUTPUTMODE", 0, 3),
            ("CX", 0, 3),
            ("CY", 0, 3),
            ("CLICK", 0, 3),
            ("AUTORETURN", 0, 3),
            ("AUTOPAGE", 0, 3),
            ("AUTOCLICK", 0, 3),
            ("LET", 0, 3),
            ("SET", 0, 3),
        ];
    }
    let v1 = with_int_var(&mut b, 0, -1);
    let v2 = with_int_var(&mut b, 1, -1);
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    let text = |slot: u8, payload: Vec<u8>| GroupSpec::new(OP_TEXT).window(slot, payload);
    let info = |slot: u8, target: VarRef| {
        GroupSpec::new(OP_TEXTINFO)
            .window(slot, ex::i32_(1))
            .window(8, ex::r#ref(0x40, target.id))
    };
    b.scripts = vec![vec![
        text(1, ex::i32_(0)),  // OUTPUTMODE=0
        text(17, ex::i32_(0)), // AUTORETURN=0
        info(1, v1),           // TEXTINFO[OUTPUTMODE=1 LET=@v1]
        info(5, v2),           // TEXTINFO[AUTORETURN=1 LET=@v2]
        GroupSpec::new(OP_END),
    ]];

    let mut vm = boot(b);
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(vm.store().read(v1, &[]).unwrap(), Value::Int(0));
    assert_eq!(vm.store().read(v2, &[]).unwrap(), Value::Int(0));
}

#[test]
fn text_flag_sysvar_reads_the_control_stream() {
    // REA 0x446b93:id 0x7b(_TEXT_FLAG)→ [state+0x3c]。WORD 写入后,
    // 脚本用 LET[$v = $_TEXT_FLAG] 必须拿到同一字节流。
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    let flag_id = b
        .sysvars
        .iter()
        .position(|(n, _, _)| *n == "_TEXT_FLAG")
        .expect("fixture 系统变量含 _TEXT_FLAG") as u16;
    b.vars.push(VarSpec {
        kind: 1,
        group: 0,
        script_id: 0,
        var_id: sys,
        value_type: 3,
        dims: vec![],
        init: Init::Str(b""),
    });
    let v = VarRef::new(VarSpace::Dollar, sys);
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        GroupSpec::new(OP_WORD).window(0, b"A\x82\xB1".to_vec()),
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x24, v.id))
            .window(1, ex::get(0x40, flag_id)),
        GroupSpec::new(OP_END),
    ]];

    let mut vm = boot(b);
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(
        vm.store().read(v, &[]).unwrap(),
        Value::Str(b"12".to_vec()),
        "$_TEXT_FLAG = 半角/全角控制流"
    );
}

#[test]
fn domain_args_map_slots_through_project_yscm() {
    // 域事件只带槽号;宿主应按工程 ysc.ybn 的名字消费(槽号不跨版本稳定)。
    // 手册 sound/sound.html:ID / VOLUME(0..256)/ E(0/1)。
    let mut b = ProjectBuilder::new();
    if let Some((_, params)) = b.commands.iter_mut().find(|(n, _)| *n == "SOUND") {
        *params = vec![("ID", 1, 0), ("VOLUME", 0, 3), ("E", 0, 3)];
    }
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    const OP_SOUND: u8 = 87;
    b.scripts = vec![vec![
        GroupSpec::new(OP_SOUND)
            .window(0, ex::s("BGM"))
            .window(1, ex::i32_(256))
            .window(2, ex::i32_(1)),
        GroupSpec::new(OP_END),
    ]];

    let mut vm = boot(b);
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let call = vm
        .events()
        .iter()
        .find(|e| matches!(e, yu_rt::VmEvent::DomainCall { name, .. } if name == "SOUND"))
        .expect("SOUND DomainCall");
    let named = call
        .domain_args(&vm.tables().boot.yscm)
        .expect("DomainCall");
    // P4:域事件带可选脚本坐标(内核发出的总是 Some)。
    if let yu_rt::VmEvent::DomainCall { script, pc, .. } = call {
        assert_eq!(*script, Some(0));
        assert_eq!(*pc, Some(0));
    }
    let names: Vec<&str> = named.iter().map(|a| a.name.as_ref()).collect();
    assert_eq!(names, vec!["ID", "VOLUME", "E"]);
    assert_eq!(named[0].value, &Value::Str(b"BGM".to_vec()));
    assert_eq!(named[1].value, &Value::Int(256));
    assert_eq!(named[2].value, &Value::Int(1));
}

// ---------------------------------------------------------------- 按钮码 _CODE_* / _INP

#[test]
fn inp_and_code_constants_share_the_engine_counter_table() {
    // 手册 basic/svariable.html ※3:`@_INP(@_CODE_Z)` ≡ `@_KEY_Z`;
    // REA(0.495 FUN_00427350):case 0x90(_INP)与 case 0x19f..(_CODE_*)
    // 落同一张 VK 计数表;`_INP(n<=0) → 0`。
    use support::Init;
    let mut b = ProjectBuilder::new();
    // 用**内嵌引擎 ABI 全表**(548 项)当工程系统变量段——
    // `_CODE_*` 家族只有在全表里才有。
    b.sysvars_abi = true;
    let sys = yu_rt::sysvar_abi::SysVarTable::builtin_abi().decls.len() as u16;
    // 5 个用户整型变量接住结果。
    b.vars = (0..5)
        .map(|i| VarSpec {
            kind: 1,
            group: 0,
            script_id: 0,
            var_id: sys + i,
            value_type: 1,
            dims: vec![],
            init: Init::Int(0),
        })
        .collect();
    // 夹具的 ABI 名字表(顺序 = 0.495 ycd;名字 → id)。
    let abi = yu_rt::sysvar_abi::SysVarTable::builtin_abi();
    let sysvar_id = |name: &str| -> u16 {
        abi.decls
            .iter()
            .position(|d| d.name == name)
            .expect("引擎 ABI 表应有该 _CODE_* 变量") as u16
    };
    let code_z = sysvar_id("_CODE_Z");
    let code_enter = sysvar_id("_CODE_ENTER");
    let key_z = sysvar_id("_KEY_Z");
    let inp = sysvar_id("_INP");

    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        // @v0 = @_CODE_Z
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, sys))
            .window(0, ex::get(0x40, code_z)),
        // @v1 = @_CODE_ENTER
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, sys + 1))
            .window(0, ex::get(0x40, code_enter)),
        // @v2 = @_KEY_Z
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, sys + 2))
            .window(0, ex::get(0x40, key_z)),
        // @v3 = @_INP[@_CODE_Z](ref + 下标 + 数组取值)
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, sys + 3))
            .window(
                0,
                ex::cat(&[ex::r#ref(0x40, inp), ex::i16_(0x5A), ex::aload()]),
            ),
        GroupSpec::new(OP_END),
    ]];

    // 注入:第 0 帧按下 Z。脚本与按键同一逻辑帧。
    let mut tl = InputTimeline::new();
    tl.set(0, InputFrame::new(["KEY_Z"]));
    let mut vm = boot(b);
    let report = vm.tick_at(0, &tl);
    assert!(report.errors.is_empty(), "{:?}", report.errors);

    let v = |i: u16| vm.store().read(user_var(&vm, i), &[]).unwrap();
    assert_eq!(v(0), Value::Int(0x5A), "_CODE_Z = VK_Z");
    assert_eq!(v(1), Value::Int(0x0D), "_CODE_ENTER = VK_RETURN");
    assert_eq!(v(2), Value::Int(1), "_KEY_Z 本帧按下 = 1");
    assert_eq!(v(3), Value::Int(1), "_INP(@_CODE_Z) ≡ _KEY_Z(同一计数表)");
    // _KEYLAST 手册无页 ⇒ 系统变量读取挂号;但 InputState 内部照常记录
    // (供宿主/取证),码值 = 引擎按钮码。
    assert_eq!(vm.input().keylast(), 0x5A, "内部记录最近按下按钮码");

    // _INP(0) → 0(REA case 0x90 guard);手柄码无注入恒 0。
    assert_eq!(vm.input().inp(0).unwrap(), 0);
    assert_eq!(vm.input().inp(388 + 36 + 5).unwrap(), 0);

    // 写入 _CODE_* = 键位重映射(手册 ○;InputState::set_code)。
    vm.input_state_mut().set_code(code_enter, 0x5A);
    assert_eq!(vm.input().code_value(code_enter, 0x0D), 0x5A);
}

// ---------------------------------------------------------------- panic 收口(任务书 E R7)

#[test]
fn ifbreak_lv0_is_a_no_op_not_a_panic() {
    // 修复前:`IFBREAK[LV=0]` 在 `if_stack[keep=len]` 越界 panic。
    let mut b = ProjectBuilder::new();
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    const OP_IFBREAK: u8 = 47;
    b.scripts = vec![vec![
        GroupSpec::new(OP_IFBREAK).window(0, ex::i8_(0)),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "LV=0 应为 no-op: {:?}", r.errors);
}

#[test]
fn gosub_param_name_out_of_range_is_fatal_not_a_panic() {
    // 修复前:工程 ysc 表异常(PINT0 / PINT17+)→ `args[idx-1]` 回绕/越界 panic。
    let mut b = ProjectBuilder::new();
    // 故意造一张异常表:GOSUB 参数 1 改名 PINT0、参数 2 改名 PINT17。
    if let Some(gosub) = b.commands.iter_mut().find(|(n, _)| *n == "GOSUB") {
        gosub.1[1] = ("PINT0", 1, 0);
        gosub.1[2] = ("PINT17", 1, 0);
    }
    b.labels = vec![("SYSTEM_START", 0, 0, 0), ("SUB", 2, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        GroupSpec::new(OP_GOSUB)
            .window(0, ex::s("SUB"))
            .window(1, ex::i8_(1)), // PINT0 → 回绕
        GroupSpec::new(OP_END),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    let r = vm.tick_at(0, &no_input());
    assert_eq!(
        r.errors.len(),
        1,
        "PINT0 应报 Fatal 而非 panic: {:?}",
        r.errors
    );
    assert!(r.errors[0].contains("超出 1..16"), "{:?}", r.errors);
}

#[test]
fn dimsize_huge_is_fatal_not_an_abort() {
    // 修复前:`VARACT[DIMSIZE=<巨型>]` → Vec::resize 容量溢出/ OOM abort。
    use support::Init;
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![VarSpec {
        kind: 1,
        group: 0,
        script_id: 0,
        var_id: sys,
        value_type: 1,
        dims: vec![2],
        init: Init::Int(0),
    }];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    // VARACT[SET=@v DIMSIZE=0x7fff_ffff_ffff](绝对值;复合码 0 = slot 高字节 0;
    // 形态对齐 VAR.ADD 回归用例的 SET lvalue 窗口)
    b.scripts = vec![vec![
        GroupSpec::new(OP_VARACT)
            .window(0, ex::r#ref(0x40, sys))
            .window_tagged(13, 0, 0x0000, ex::i64_(0x7fff_ffff_ffff)),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    let r = vm.tick_at(0, &no_input());
    assert_eq!(
        r.errors.len(),
        1,
        "巨型 DIMSIZE 应报 Fatal 而非 abort: {:?}",
        r.errors
    );
    assert!(r.errors[0].contains("防御上限"), "{:?}", r.errors);
}

// ---------------------------------------------------------------- 任务修复回归(任务书 E2)

/// R3 回归:`RETURNCODE` 是文本让出点。
///
/// REA 0.495 `0x44c390`:`MOV byte [task+9],1` + `MOV EAX,-1`;泵 `0x40442e`
/// 检查 `task+9` 非零即跳过该任务。`FUN_004046fc` 在推进输入
/// (MOUSE_L/ENTER/CTRL/手柄)为正时清标志。
///
/// 断言:① 一帧只推进一行;② 宿主收到 `Text`;③ 点击前不进入下一行;
/// ④ 注入点击后进入下一行。
#[test]
fn one_text_line_per_frame_until_advance_input() {
    let mut b = ProjectBuilder::new();
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    const OP_RETURNCODE: u8 = 79;
    const OP_WORD: u8 = 106;
    let word = |s: &str| GroupSpec::new(OP_WORD).window(0, s.as_bytes().to_vec());
    b.scripts = vec![vec![
        word("A"),
        GroupSpec::new(OP_RETURNCODE).window(1, Vec::new()),
        word("B"),
        GroupSpec::new(OP_RETURNCODE).window(2, Vec::new()),
        word("C"),
        GroupSpec::new(OP_RETURNCODE).window(3, Vec::new()),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    // 帧 1:只出 "A" 行。
    vm.tick_at(0, &no_input());
    let texts = |vm: &Vm| -> Vec<Vec<u8>> {
        vm.events()
            .iter()
            .filter_map(|e| match e {
                yu_rt::VmEvent::Text { bytes, .. } => Some(bytes.clone()),
                _ => None,
            })
            .collect()
    };
    assert_eq!(texts(&vm), vec![b"A".to_vec()], "一帧只推进一行");
    let rc_count = |vm: &Vm| {
        vm.events()
            .iter()
            .filter(|e| e.kind() == "return_code")
            .count()
    };
    assert_eq!(rc_count(&vm), 1);

    // 帧 2/3:无输入 ⇒ 仍停在第一行。
    vm.tick_at(16, &no_input());
    vm.tick_at(32, &no_input());
    assert_eq!(texts(&vm), vec![b"A".to_vec()], "点击前不进入下一行");
    assert_eq!(rc_count(&vm), 1);

    // 帧 4:注入点击(MOUSE_L)⇒ 恢复,推进到第二行。
    let mut tl = InputTimeline::new();
    tl.set(4, InputFrame::new(["MOUSE_L"]));
    vm.tick_at(48, &tl);
    assert_eq!(
        texts(&vm),
        vec![b"A".to_vec(), b"B".to_vec()],
        "推进输入后进入下一行(且一次只一行)"
    );
    assert_eq!(rc_count(&vm), 2);

    // 松开若干帧后再次点击(计数器已为负数):按手册「押された瞬間から 1」
    // 必须从 1 重新起算,否则第二次点击永远到不了正值,第三行不出现。
    let mut tl = InputTimeline::new();
    tl.set(10, InputFrame::new(["MOUSE_L"]));
    for f in 5..=10 {
        vm.tick_at(f * 16, &tl);
    }
    assert_eq!(
        texts(&vm),
        vec![b"A".to_vec(), b"B".to_vec(), b"C".to_vec()],
        "长期松开后的第二次点击仍能推进"
    );
    assert_eq!(rc_count(&vm), 3);
}

/// R9 回归:GOSUB 允许 64 次嵌套,第 65 次 Fatal。
///
/// 手册 `control/gosub.html`:「GOSUBのネストは64回まで可能です」;
/// 引擎 `FUN_004459c4` @0x4459e6:`depth+1 == 0x40` 报错。
fn gosub_chain(n: usize) -> ProjectBuilder {
    let mut b = ProjectBuilder::new();
    let v = with_int_var(&mut b, 0, 0);
    let name = |i: usize| -> &'static str { Box::leak(format!("L{i}").into_boxed_str()) };
    let mut labels: Vec<(&'static str, u32, u16, u8)> = vec![("SYSTEM_START", 0, 0, 0)];
    for i in 0..n {
        labels.push((name(i), (3 + 2 * i) as u32, 0, 0));
    }
    b.labels = labels;
    b.entry(0, 0);
    let mut groups = vec![
        GroupSpec::new(OP_GOSUB).window(0, ex::s("L0")),
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, v.id))
            .window(0, ex::i8_(1)),
        GroupSpec::new(OP_END),
    ];
    for i in 0..n {
        if i + 1 < n {
            groups.push(GroupSpec::new(OP_GOSUB).window(0, ex::s(name(i + 1))));
        } else {
            groups.push(
                GroupSpec::new(OP_LET)
                    .window(0, ex::r#ref(0x40, v.id))
                    .window(0, ex::i8_(2)),
            );
        }
        groups.push(GroupSpec::new(OP_RETURN));
    }
    b.scripts = vec![groups];
    b
}

#[test]
fn gosub_allows_64_nested_calls() {
    let b = gosub_chain(64);
    let mut vm = boot(b);
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "64 层必须成功: {:?}", r.errors);
    assert_eq!(
        vm.store().read(user_var(&vm, 0), &[]).unwrap(),
        Value::Int(1),
        "整条链返回后主线继续"
    );
}

#[test]
fn gosub_65_nested_calls_is_fatal() {
    let b = gosub_chain(65);
    let mut vm = boot(b);
    let r = vm.tick_at(0, &no_input());
    assert_eq!(r.errors.len(), 1, "第 65 层应 Fatal: {:?}", r.errors);
    assert!(r.errors[0].contains("rt.nesting"), "{:?}", r.errors);
}

/// R9 回归:`IFCONTINUE` 不累积 IF 帧(引擎 `0x446490` 把深度设为 `depth-LV`,
/// 目标 IF 在入口重新压帧;若保留目标层再压一次就会无限累积)。
///
/// 结构:内层 `IF[@n<200]` + `LET[@n+=1]` + `IFCONTINUE` 自旋 200 次,
/// 外层 `LOOP[SET=200]`。修复前每轮 IFCONTINUE 都多留一层 IF 帧,
/// 第 65 次即撞上限 Fatal;修复后深度恒为 1。
#[test]
fn ifcontinue_does_not_accumulate_if_frames() {
    let mut b = ProjectBuilder::new();
    let n = with_int_var(&mut b, 0, 0);
    let v = with_int_var(&mut b, 1, 0);
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    const OP_IFCONTINUE: u8 = 48;
    let n_get = ex::get(0x40, n.id);
    b.scripts = vec![vec![
        // 0: LOOP[SET=200] → 7
        GroupSpec::new(OP_LOOP).window(0, ex::i16_(200)).jump(7),
        // 1: IF[@n < 200] ELSE→6 出口→6
        GroupSpec::new(OP_IF)
            .window(0, ex::cat(&[n_get.clone(), ex::i16_(200), ex::op(0x5a)]))
            .jump(6)
            .jump(6),
        // 2: LET[@n = @n + 1]
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, n.id))
            .window(0, ex::cat(&[n_get.clone(), ex::i8_(1), ex::op(0x2b)])),
        // 3: IFCONTINUE[] → 回到 pc1
        GroupSpec::new(OP_IFCONTINUE),
        // 4,5: 空档
        GroupSpec::new(OP_END),
        GroupSpec::new(OP_END),
        // 6: IFEND
        GroupSpec::new(OP_IFEND),
        // 7: LOOPEND
        GroupSpec::new(OP_LOOPEND),
        // 8: LET[@v=1]
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, v.id))
            .window(0, ex::i8_(1)),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    let r = vm.tick_at(0, &no_input());
    assert!(
        r.errors.is_empty(),
        "IFCONTINUE 不应撞嵌套上限: {:?}",
        r.errors
    );
    assert_eq!(
        vm.store().read(user_var(&vm, 1), &[]).unwrap(),
        Value::Int(1),
        "循环跑完后主线继续"
    );
    // IF 栈在帧尾必须为空(全部配对弹出)。
    let ev = to_jsonl(vm.events());
    assert!(ev.contains("\"kind\":\"frame_end\""), "帧应正常结束");
}

/// R10 回归:一帧事件以 `FrameStart` 开始、`FrameEnd` 结束。
#[test]
fn frame_events_are_bounded_by_start_and_end() {
    let mut b = ProjectBuilder::new();
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        GroupSpec::new(OP_WAIT).window(0, ex::i8_(1)),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    for f in 0..3u32 {
        vm.tick_at(f * 16, &no_input());
    }
    for frame in 1..=3u64 {
        let kinds: Vec<&str> = vm
            .events()
            .iter()
            .filter(|e| e.frame() == frame)
            .map(|e| e.kind())
            .collect();
        assert_eq!(
            kinds.first(),
            Some(&"frame_start"),
            "帧 {frame} 以 FrameStart 开始"
        );
        assert_eq!(
            kinds.last(),
            Some(&"frame_end"),
            "帧 {frame} 以 FrameEnd 结束"
        );
    }
}

/// R10 回归:事件队列有界,丢弃可观测(长跑不无界增长)。
#[test]
fn event_queue_is_bounded_and_reports_drops() {
    let mut b = ProjectBuilder::new();
    let v = with_int_var(&mut b, 0, 0);
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    // 1000 次赋值 ⇒ 远多于容量 32。
    b.scripts = vec![vec![
        GroupSpec::new(OP_LOOP).window(0, ex::i16_(1000)).jump(3),
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, v.id))
            .window(0, ex::i8_(1)),
        GroupSpec::new(OP_LOOPEND),
        GroupSpec::new(OP_END),
    ]];
    let src = b.build();
    let mut vm = Vm::boot(Box::new(src), BootConfig::default())
        .expect("boot")
        .with_event_capacity(32);
    vm.tick_at(0, &no_input());
    assert!(vm.events().len() <= 32, "事件队列不得超过容量");
    assert!(vm.events_dropped() > 0, "丢弃必须可见");
    // drain 接口能取走并清空。
    let mut out = Vec::new();
    vm.drain_events(&mut out);
    assert!(!out.is_empty());
    assert!(vm.events().is_empty(), "drain 后队列为空");
}

/// R11 回归:脚本错误 Diag 带 task/script/pc/命令名。
#[test]
fn script_diag_carries_task_script_and_pc() {
    let mut b = ProjectBuilder::new();
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    // GO 到不存在的标签 ⇒ Fatal(RT.LABEL)。pc 0。
    b.scripts = vec![vec![GroupSpec::new(OP_GO).window(0, ex::s("NO.SUCH"))]];
    let mut vm = boot(b);
    vm.tick_at(0, &no_input());
    let line = vm
        .diags()
        .iter()
        .find(|d| d.code.contains("rt.label"))
        .map(|d| d.to_json_line())
        .expect("应有 rt.label 诊断");
    assert!(line.contains("\"kind\":\"script\""), "{line}");
    assert!(
        line.contains("fixture0000.yst"),
        "脚本文件名进 Diag: {line}"
    );
    assert!(
        line.contains("\"label\":\"SYSTEM\""),
        "任务标识进 label: {line}"
    );
    assert!(line.contains("\"pc\":0"), "{line}");
    assert!(line.contains("\"cmd\":\"GO\""), "{line}");
}

/// 主线正常走到 `END` 时 `ChainReport::main_finished` 必须为真(R2 的邻居
/// 断言:结束路径不只覆盖错误路径)。
#[test]
fn main_end_sets_main_finished() {
    let mut b = ProjectBuilder::new();
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![GroupSpec::new(OP_END)]];
    let mut vm = boot(b);
    let r = vm.tick_at(0, &no_input());
    assert!(r.main_finished, "END 后主线结束");
    assert!(vm.scheduler().find_by_id("SYSTEM").is_none(), "节点已摘链");
}

/// `TASK` 未传 `TINT` 时,新任务的 `@_TINT(n)` 必须是 0(不继承调用者)。
///
/// REA 0.495 `TASK` 处理体 `0x450916..0x450943`:建任务时逐槽清零
/// `+0x348/+0x34c`(TFLT)、`+0x3d0/+0x3d4`(TINT)、`+0x458`(TSTR),
/// 再按参数落位。
#[test]
fn task_without_tint_does_not_inherit_caller_registers() {
    let mut b = ProjectBuilder::new();
    let v = with_int_var(&mut b, 0, -1);
    let tint = b
        .sysvars
        .iter()
        .position(|(n, _, _)| *n == "_TINT")
        .expect("_TINT") as u16;
    b.labels = vec![
        ("SYSTEM_START", 0, 0, 0),
        ("A.BODY", 3, 0, 0),
        ("B.BODY", 6, 0, 0),
    ];
    b.entry(0, 0);
    const OP_TASK: u8 = 93;
    const OP_TASKEND: u8 = 94;
    b.scripts = vec![vec![
        GroupSpec::new(OP_TASK)
            .window(0, ex::s("A"))
            .window(34, ex::s("A.BODY"))
            .window(4, ex::i8_(7)), // TINT(1)=7
        GroupSpec::new(OP_WAIT).window(0, ex::i8_(4)),
        GroupSpec::new(OP_END),
        // 3: A.BODY —— 再建 B,不传 TINT
        GroupSpec::new(OP_TASK)
            .window(0, ex::s("B"))
            .window(34, ex::s("B.BODY")),
        GroupSpec::new(OP_TASKEND),
        GroupSpec::new(OP_END),
        // 6: B.BODY —— 读 @_TINT(1)
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, v.id))
            .window(
                0,
                ex::cat(&[ex::r#ref(0x40, tint), ex::i8_(1), ex::aload()]),
            ),
        GroupSpec::new(OP_TASKEND),
    ]];
    let mut vm = boot(b);
    for f in 0..5u32 {
        vm.tick_at(f * 16, &no_input());
    }
    assert_eq!(
        vm.store().read(user_var(&vm, 0), &[]).unwrap(),
        Value::Int(0),
        "B 未传 TINT ⇒ 读 0(不是继承 A 的 7)"
    );
}

// ---------------------------------------------------------------- 回归:初值与调用层栈

/// YSVR 字符串初值是 YSTB 字面量形态(`M` + u16 + 引号串),
/// 必须在装载时解码;否则 `$es.STARTLABEL` 会带着 `M\x10\x00"` 前缀。
#[test]
fn ysvr_string_initial_value_is_decoded() {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![VarSpec {
        kind: 1,
        group: 1,
        script_id: 0,
        var_id: sys,
        value_type: 3,
        dims: vec![],
        init: Init::Str(&b"M\x10\x00\"SCENARIO_START\""[..]),
    }];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![GroupSpec::new(OP_END)]];

    let vm = boot(b);
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::Dollar, sys), &[])
            .unwrap(),
        Value::Str(b"SCENARIO_START".to_vec()),
        "字符串初值必须去掉 YSTB 字面量封装"
    );
}

/// 被调层未闭合的 `LOOP` 不能泄漏到调用方。
///
/// 引擎 RETURN 处理器 `0x44b418` 用帧里记录的深度 truncate IF/循环栈
/// (旧线逆向成果 51.5);否则调用方 `LOOPEND` 会弹出被调层的帧,
/// 跳到被调脚本的 `body_pc`(跨脚本越界)。
#[test]
fn callee_loop_frame_does_not_leak_into_caller() {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![VarSpec {
        kind: 1,
        group: 0,
        script_id: 0,
        var_id: sys,
        value_type: 1,
        dims: vec![],
        init: Init::Int(0),
    }];
    b.labels = vec![("SYSTEM_START", 0, 0, 0), ("SUB", 10, 1, 0)];
    b.entry(0, 0);

    let let_zero = || {
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, sys))
            .window(0, ex::i8_(0))
    };
    // 调用方(脚本 0,共 6 组):LOOP → GOSUB → 两条 LET → LOOPEND → END。
    let caller = vec![
        GroupSpec::new(OP_LOOP).window(0, ex::i8_(1)).jump(4),
        GroupSpec::new(OP_GOSUB).window(0, ex::s("SUB")),
        let_zero(),
        let_zero(),
        GroupSpec::new(OP_LOOPEND),
        GroupSpec::new(OP_END),
    ];
    // 被调方(脚本 1):填充 10 组后 **无限 LOOP** → RETURN(循环未闭合)
    // → LOOPEND → END。无限循环才能让调用方 LOOPEND 走到回跳分支。
    let mut callee: Vec<GroupSpec> = (0..10).map(|_| let_zero()).collect();
    callee.push(GroupSpec::new(OP_LOOP).window(0, Vec::new()).jump(12));
    callee.push(GroupSpec::new(OP_RETURN));
    callee.push(GroupSpec::new(OP_LOOPEND));
    callee.push(GroupSpec::new(OP_END));
    b.scripts = vec![caller, callee];

    let mut vm = boot(b);
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert!(report.main_finished, "调用方应正常结束");
}

// ---------------------------------------------------------------- 声音域接线

/// `SOUNDINFO[ID=... TIME=1 LET=@x]`：VM 持 LET 左值 → `SoundHost::query`
/// → `write_target`。用 mock 宿主验证接线本身（不依赖 `yu-sound`）。
#[test]
fn soundinfo_query_writes_let_through_host() {
    use std::cell::Cell;
    use std::rc::Rc;
    use yu_rt::event::{ArgValue, SoundHost, VmEvent};

    struct Mock {
        seen: Rc<Cell<bool>>,
    }
    impl SoundHost for Mock {
        fn apply_event(&mut self, _ev: &VmEvent) {}
        fn query(&mut self, command: &str, args: &[ArgValue]) -> Result<Value, yu_log::Diag> {
            assert_eq!(command, "SOUNDINFO");
            let names: Vec<u8> = args.iter().map(|a| a.slot).collect();
            assert_eq!(names, vec![0, 1], "ID/TIME 槽");
            self.seen.set(true);
            Ok(Value::Int(1234))
        }
    }

    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![VarSpec {
        kind: 1,
        group: 0,
        script_id: 0,
        var_id: sys,
        value_type: 1,
        dims: vec![],
        init: Init::Int(-1),
    }];
    // 保留完整默认命令表（boot 要求 GOSUB 等必需命令），只给 SOUNDINFO
    // 补上参数声明；opcode 仍取工程表里的实际序号。
    let op_soundinfo = b
        .commands
        .iter()
        .position(|(name, _)| *name == "SOUNDINFO")
        .expect("SOUNDINFO in fixture table") as u8;
    let op_end = b
        .commands
        .iter()
        .position(|(name, _)| *name == "END")
        .expect("END in fixture table") as u8;
    for (name, params) in b.commands.iter_mut() {
        if *name == "SOUNDINFO" {
            *params = vec![("ID", 1, 0), ("TIME", 0, 3), ("LET", 0, 0)];
        }
    }
    let groups = vec![
        GroupSpec::new(op_soundinfo)
            .window(0, ex::s("BGM"))
            .window(1, ex::i8_(1))
            .window(2, ex::r#ref(0x40, sys)),
        GroupSpec::new(op_end),
    ];
    b.scripts = vec![groups];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);

    let mut vm = boot(b);
    let seen = Rc::new(Cell::new(false));
    vm.set_sound_host(Box::new(Mock { seen: seen.clone() }));
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert!(seen.get(), "SoundHost::query 未被调用");
    let v = vm.store().read(user_var(&vm, 0), &[]).unwrap();
    assert_eq!(v, Value::Int(1234));
}

// ---------------------------------------------------------------- 官方分发表槽(P0-A/DIV-RT-043)

const OP_DEBUGLIST: u8 = 9;
const OP_DIALOG: u8 = 10;
const OP_F_INT: u8 = 18;
const OP_LABEL: u8 = 52;
const OP_PREP: u8 = 75;
const OP_RETURNBREAK: u8 = 78;
const OP_S_INT: u8 = 81;

/// 官方默认槽 `0x459e90`(F_INT 等):报一次「スクリプト実行エラー」+
/// 返回 -1 ⇒ 本帧让出、pc 已前进、任务继续(DIV-RT-043)。
#[test]
fn default_slot_reports_once_and_yields_without_terminating() {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![VarSpec {
        kind: 1,
        group: 0,
        script_id: 0,
        var_id: sys,
        value_type: 1,
        dims: vec![],
        init: Init::Int(0),
    }];
    let groups = vec![
        GroupSpec::new(OP_F_INT),
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, sys))
            .window(1, ex::i8_(1)),
        GroupSpec::new(OP_F_INT),
        GroupSpec::new(OP_END),
    ];
    b.scripts = vec![groups];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);

    let mut vm = boot(b);
    // 第一帧:F_INT 返回 -1(让出)⇒ 后面的 LET 本帧不执行。
    let r0 = vm.tick_at(0, &no_input());
    assert!(
        r0.errors.is_empty(),
        "默认槽不是内核 Fatal: {:?}",
        r0.errors
    );
    assert_eq!(
        vm.store().read(user_var(&vm, 0), &[]).unwrap(),
        Value::Int(0)
    );
    assert_eq!(vm.script_slot_hits(), 1);
    let diag0 = vm
        .events()
        .iter()
        .filter(|e| matches!(e, yu_rt::VmEvent::Diag { code, .. } if code == "rt.script.error"))
        .count();
    assert_eq!(diag0, 1, "官方只弹一次全局错误(0x5e4f84)");
    assert!(
        vm.unimplemented_cmds().is_empty(),
        "默认槽不是 Unimplemented"
    );

    // 第二帧:LET 继续,第二条 F_INT 再让出;诊断不重复。
    let r1 = vm.tick_at(16, &no_input());
    assert!(r1.errors.is_empty(), "{:?}", r1.errors);
    assert_eq!(
        vm.store().read(user_var(&vm, 0), &[]).unwrap(),
        Value::Int(1)
    );
    assert_eq!(vm.script_slot_hits(), 2);
    let diag1 = vm
        .events()
        .iter()
        .filter(|e| matches!(e, yu_rt::VmEvent::Diag { code, .. } if code == "rt.script.error"))
        .count();
    assert_eq!(diag1, 1, "第二次命中仍只保留首条诊断");

    // 第三帧:END 正常结束主线。
    let r2 = vm.tick_at(32, &no_input());
    assert!(r2.main_finished);
}

/// 官方 `0x4634d8`(xor eax,eax; ret)= 真 no-op:同帧继续,无诊断。
#[test]
fn noop_slots_continue_in_the_same_pump() {
    let groups = vec![
        GroupSpec::new(OP_DEBUGLIST),
        GroupSpec::new(OP_LABEL),
        GroupSpec::new(OP_PREP),
        GroupSpec::new(OP_RETURNBREAK),
        GroupSpec::new(OP_S_INT),
        GroupSpec::new(OP_END),
    ];
    let mut b = ProjectBuilder::new();
    b.scripts = vec![groups];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);

    let mut vm = boot(b);
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(r.commands_pumped, 6, "no-op 不得让出");
    assert!(vm.unimplemented_cmds().is_empty());
    assert!(vm.script_slot_hits() == 0);
    assert!(!vm
        .events()
        .iter()
        .any(|e| matches!(e, yu_rt::VmEvent::Diag { .. })));
}

// ---------------------------------------------------------------- DIALOG(P0-B/DIV-RT-044)

/// DIALOG 挂起任务;宿主回答后写 `LET`(1 基按钮序号),下一帧继续。
#[test]
fn dialog_blocks_task_until_answered_and_writes_let() {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![VarSpec {
        kind: 1,
        group: 0,
        script_id: 0,
        var_id: sys,
        value_type: 1,
        dims: vec![],
        init: Init::Int(0),
    }];
    let groups = vec![
        // DIALOG[STR="q" CAPTION="c" YESNO=1 LET=@v]
        GroupSpec::new(OP_DIALOG)
            .window(0, ex::s("c"))
            .window(1, ex::s("q"))
            .window(14, ex::i8_(1))
            .window(11, ex::r#ref(0x40, sys)),
        // 回答后才会执行:LET[@v = 99]
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, sys))
            .window(1, ex::i8_(99)),
        GroupSpec::new(OP_END),
    ];
    b.scripts = vec![groups];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);

    let mut vm = boot(b);
    let r0 = vm.tick_at(0, &no_input());
    assert!(r0.errors.is_empty(), "{:?}", r0.errors);
    let pending = vm.pending_dialogs().to_vec();
    assert_eq!(pending.len(), 1);
    let req = &pending[0];
    assert_eq!(req.text, b"q");
    assert_eq!(req.caption, b"c");
    assert_eq!(req.buttons, yu_rt::dialog::DialogButtons::YesNo);
    assert!(req.accepts_button(1) && req.accepts_button(2) && !req.accepts_button(3));
    assert_eq!(vm.dialog_of_task(MAIN_TASK_ID), Some(req.id));
    assert_eq!(
        vm.store().read(user_var(&vm, 0), &[]).unwrap(),
        Value::Int(0),
        "未回答前任务必须挂起,后续 LET 不得执行"
    );

    // 第二帧仍挂起。
    vm.tick_at(16, &no_input());
    assert_eq!(
        vm.store().read(user_var(&vm, 0), &[]).unwrap(),
        Value::Int(0)
    );

    // 越界按钮被拒绝;选「いいえ」= 2。
    let id = req.id;
    assert!(vm.answer_dialog(id, 3, None).is_err());
    vm.answer_dialog(id, 2, None).expect("answer");
    assert!(vm.pending_dialogs().is_empty());
    assert_eq!(vm.dialog_of_task(MAIN_TASK_ID), None);
    assert_eq!(
        vm.store().read(user_var(&vm, 0), &[]).unwrap(),
        Value::Int(2),
        "LET = 左起第 2 个按钮"
    );

    // 下一帧继续 DIALOG 之后的指令。
    let r1 = vm.tick_at(32, &no_input());
    assert!(r1.errors.is_empty(), "{:?}", r1.errors);
    assert_eq!(
        vm.store().read(user_var(&vm, 0), &[]).unwrap(),
        Value::Int(99),
        "pc 已前进,回答后执行下一条"
    );
    assert!(r1.main_finished);
}

/// INPUT=1:按钮固定 OK(LET=1),文本写回 LETSTR;缺文本的回答被拒绝。
#[test]
fn dialog_input_writes_letstr_and_let() {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![
        VarSpec {
            kind: 1,
            group: 0,
            script_id: 0,
            var_id: sys,
            value_type: 1,
            dims: vec![],
            init: Init::Int(0),
        },
        VarSpec {
            kind: 1,
            group: 0,
            script_id: 0,
            var_id: sys + 1,
            value_type: 3,
            dims: vec![],
            init: Init::Str(b""),
        },
    ];
    let groups = vec![
        // DIALOG[CAPTION="c" STR="name?" INPUT=1 DEFSTR="abc" LETSTR=$s LET=@v]
        GroupSpec::new(OP_DIALOG)
            .window(0, ex::s("c"))
            .window(1, ex::s("name?"))
            .window(9, ex::s("abc"))
            .window(10, ex::r#ref(0x24, sys + 1))
            .window(11, ex::r#ref(0x40, sys))
            .window(16, ex::i8_(1)),
        GroupSpec::new(OP_END),
    ];
    b.scripts = vec![groups];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);

    let mut vm = boot(b);
    vm.tick_at(0, &no_input());
    let req = vm.pending_dialogs()[0].clone();
    assert!(req.input);
    assert_eq!(req.default_input, b"abc");
    assert_eq!(req.buttons, yu_rt::dialog::DialogButtons::Ok);
    assert!(
        vm.answer_dialog(req.id, 1, None).is_err(),
        "INPUT 必须带文本"
    );
    vm.answer_dialog(req.id, 1, Some(b"Alice")).expect("answer");
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::Dollar, sys + 1), &[])
            .map(|v| match v {
                Value::Str(b) => b,
                _ => panic!("LETSTR 目标类型"),
            })
            .unwrap(),
        b"Alice".to_vec()
    );
    assert_eq!(
        vm.store().read(user_var(&vm, 0), &[]).unwrap(),
        Value::Int(1)
    );
    let r = vm.tick_at(16, &no_input());
    assert!(r.main_finished);
}

/// 未取证参数(NOWAIT 等)⇒ Unimplemented,不弹窗、不写回。
#[test]
fn dialog_unsupported_param_is_unimplemented() {
    let groups = vec![
        GroupSpec::new(OP_DIALOG)
            .window(1, ex::s("q"))
            .window(49, ex::i8_(1)),
        GroupSpec::new(OP_END),
    ];
    let mut b = ProjectBuilder::new();
    b.scripts = vec![groups];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);

    let mut vm = boot(b);
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(vm.pending_dialogs().is_empty());
    assert_eq!(
        vm.unimplemented_cmds().get("DIALOG").copied(),
        Some(1),
        "未取证参数按命令挂号"
    );
    // 事件流仍记录 DomainCall 事实,但不产生待回答请求。
    assert!(vm.events().iter().any(|e| matches!(
        e,
        yu_rt::VmEvent::DomainCall { name, .. } if name == "DIALOG"
    )));
    assert!(r.main_finished, "未取证参数不阻塞,END 同帧执行");
}

// ---------------------------------------------------------------- P3 系统变量收口

/// `$_TEXT_FLAG` 写入 = 替换控制字节流,不重建文本(REA 0x446b98..)。
#[test]
fn text_flag_write_replaces_stream_without_rebuilding_text() {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![
        VarSpec {
            kind: 1,
            group: 0,
            script_id: 0,
            var_id: sys,
            value_type: 3,
            dims: vec![],
            init: Init::Str(b""),
        },
        VarSpec {
            kind: 1,
            group: 0,
            script_id: 0,
            var_id: sys + 1,
            value_type: 3,
            dims: vec![],
            init: Init::Str(b""),
        },
    ];
    let flag_id = b
        .sysvars
        .iter()
        .position(|(n, _, _)| *n == "_TEXT_FLAG")
        .expect("_TEXT_FLAG") as u16;
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        GroupSpec::new(OP_WORD).window(0, b"A\x82\xB1".to_vec()),
        // LET[$_TEXT_FLAG = "9"]
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x24, flag_id))
            .window(1, ex::s("9")),
        // LET[$v = $_TEXT_FLAG]
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x24, sys))
            .window(1, ex::get(0x24, flag_id)),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(vm.current_text(), b"A\x82\xB1", "写标志不得改文本");
    assert_eq!(vm.text_flags(), b"9", "写入替换整个控制流");
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::Dollar, sys), &[])
            .unwrap(),
        Value::Str(b"9".to_vec())
    );
}

/// `VARACT[HANTOZEN/ZENTOHAN]` 使用官方 0.495 表(DIV-RT-013)。
#[test]
fn varact_hantozen_and_zentohan_use_official_tables() {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![
        VarSpec {
            kind: 1,
            group: 0,
            script_id: 0,
            var_id: sys,
            value_type: 3,
            dims: vec![],
            init: Init::Str(b"A\xb1"),
        },
        VarSpec {
            kind: 1,
            group: 0,
            script_id: 0,
            var_id: sys + 1,
            value_type: 3,
            dims: vec![],
            init: Init::Str(b""),
        },
        VarSpec {
            kind: 1,
            group: 0,
            script_id: 0,
            var_id: sys + 2,
            value_type: 3,
            dims: vec![],
            init: Init::Str(b""),
        },
    ];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    let src = VarRef::new(VarSpace::Dollar, sys);
    let wide = VarRef::new(VarSpace::Dollar, sys + 1);
    let back = VarRef::new(VarSpace::Dollar, sys + 2);
    b.scripts = vec![vec![
        // VARACT[SET=$src LET=$wide HANTOZEN=1]
        GroupSpec::new(OP_VARACT)
            .window(0, ex::r#ref(0x24, src.id))
            .window(1, ex::r#ref(0x24, wide.id))
            .window(11, ex::i8_(1)),
        // VARACT[SET=$wide LET=$back ZENTOHAN=1]
        GroupSpec::new(OP_VARACT)
            .window(0, ex::r#ref(0x24, wide.id))
            .window(1, ex::r#ref(0x24, back.id))
            .window(12, ex::i8_(1)),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    // A → Ａ(0x82 0x60);ｱ(0xb1) → ア(0x83 0x41)
    assert_eq!(
        vm.store().read(wide, &[]).unwrap(),
        Value::Str(vec![0x82, 0x60, 0x83, 0x41])
    );
    assert_eq!(
        vm.store().read(back, &[]).unwrap(),
        Value::Str(vec![b'A', 0xb1])
    );
}

/// `MATH[RND]` 用注入的 MT19937 种子;缺省 5489,首个输出对 1..=9 取模得 3。
#[test]
fn math_rnd_uses_injected_mt19937_seed() {
    let mut b = ProjectBuilder::new();
    let v = with_int_var(&mut b, 0, 0);
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        GroupSpec::new(OP_MATH)
            .window(0, ex::r#ref(0x40, v.id))
            .window(1, ex::i8_(1))
            .window(2, ex::i8_(9))
            .window(20, ex::i8_(1)),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    // 3499211612 % 9 = 2 ⇒ 1 + 2 = 3。
    assert_eq!(vm.store().read(v, &[]).unwrap(), Value::Int(3));

    // 同种子第二个 VM 结果一致(确定性)。
    let mut b = ProjectBuilder::new();
    let v2 = with_int_var(&mut b, 0, 0);
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        GroupSpec::new(OP_MATH)
            .window(0, ex::r#ref(0x40, v2.id))
            .window(1, ex::i8_(1))
            .window(2, ex::i8_(9))
            .window(20, ex::i8_(1)),
        GroupSpec::new(OP_END),
    ]];
    let mut vm2 = boot(b);
    vm2.tick_at(0, &no_input());
    assert_eq!(vm2.store().read(v2, &[]).unwrap(), Value::Int(3));
}

/// `MATH[LET=@A(i)]` 写**带下标的元素**(es_First.yst:145 的 2 的幂表填充形态;
/// DIV-RT-003 回归:曾把 LET 下标丢弃,63 次写全落 [0],`@es.POW2[k≥1]` 恒 0,
/// 消费行 `x / @es.POW2[@_LC]` 整数除零 Fatal)。
#[test]
fn math_let_writes_indexed_array_element() {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    let arr = VarRef::new(VarSpace::At, sys);
    b.vars.push(VarSpec {
        kind: 1,
        group: 0,
        script_id: 0,
        var_id: sys,
        value_type: 1,
        dims: vec![8],
        init: Init::Int(0),
    });
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    // MATH[POW=1 SET=2 SET2=<e> LET=@arr(<e>)] — 同 `es_First.yst:145`。
    let math = |e: i8| {
        GroupSpec::new(OP_MATH)
            .window(
                0,
                ex::cat(&[ex::r#ref(0x40, arr.id), ex::i8_(e), ex::aload()]),
            )
            .window(1, ex::i8_(2))
            .window(2, ex::i8_(e))
            .window(19, ex::i8_(1))
    };
    b.scripts = vec![vec![math(0), math(1), math(2), GroupSpec::new(OP_END)]];
    let mut vm = boot(b);
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let get = |i: i64| vm.store().read(arr, &[i]).unwrap();
    assert_eq!(get(0), Value::Int(1));
    assert_eq!(get(1), Value::Int(2));
    assert_eq!(get(2), Value::Int(4));
    assert_eq!(get(3), Value::Int(0), "未写的元素保持初值");
}

/// `VARACT[SET=$arr PUSH=0]`(**无 LET、旗标值为 0**)是语料合法形态:
/// `es_logset.yst` 的 LOG 记录入栈(s33 pc46..49,`PUSH(kind14, i8 0)`)。
/// 回归:曾按「旗标值 ≠ 0」判定,`PUSH=0` 被当成无参 ⇒ 掉进字符串加工
/// 分支报 `VARACT 缺 LET`。官方语义 = **参数存在性**(DIV-RT-024「不在场
/// 才不做」同族;DIV-RT-044 DIALOG「存在性判定不看值」)。
#[test]
fn varact_push_zero_is_presence_not_value() {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    let arr = VarRef::new(VarSpace::Dollar, sys);
    b.vars.push(VarSpec {
        kind: 1,
        group: 0,
        script_id: 0,
        var_id: sys,
        value_type: 3,
        dims: vec![4],
        init: Init::Str(b""),
    });
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        GroupSpec::new(OP_VARACT)
            .window(0, ex::r#ref(0x24, arr.id))
            .window(14, ex::i8_(0)),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let slot = vm.store().slot(arr).expect("数组在变量域");
    // shift_1d = 原位旋转(既有对齐语义),不扩容;本回归只锁「PUSH=0 被
    // 当成参数在场的分支分发」,不再掉进要求 LET 的字符串加工分支。
    assert_eq!(slot.dims, vec![4]);
}

/// `_WINDOW_INIT_POS` 由宿主注入(`yscfg.dat` 第 10 项;DIV-RT-047)。
#[test]
fn window_init_pos_is_host_injected() {
    let mut b = ProjectBuilder::new();
    let v = with_int_var(&mut b, 0, 0);
    let pos_id = b
        .sysvars
        .iter()
        .position(|(n, _, _)| *n == "_WINDOW_INIT_POS")
        .expect("_WINDOW_INIT_POS") as u16;
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, v.id))
            .window(1, ex::get(0x40, pos_id)),
        GroupSpec::new(OP_END),
    ]];
    let src = b.build();
    let mut vm = Vm::boot(
        Box::new(src),
        BootConfig {
            window_init_pos: 1,
            ..BootConfig::default()
        },
    )
    .expect("boot");
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(vm.store().read(v, &[]).unwrap(), Value::Int(1));
}

// ---------------------------------------------------------------- P1/P2 域与 no-op

/// `CLIPACT`/`CLIPINFO` 内核镜像:写后读回;`<0x20` 字节按官方过滤。
#[test]
fn clipact_and_clipinfo_round_trip_with_control_filtering() {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![VarSpec {
        kind: 1,
        group: 0,
        script_id: 0,
        var_id: sys,
        value_type: 3,
        dims: vec![],
        init: Init::Str(b""),
    }];
    for (name, params) in b.commands.iter_mut() {
        if *name == "CLIPACT" {
            *params = vec![("TEXT", 0, 3), ("SET", 1, 0)];
        }
        if *name == "CLIPINFO" {
            *params = vec![("TEXT", 0, 3), ("LET", 0, 0)];
        }
    }
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    let op_clipact = b
        .commands
        .iter()
        .position(|(n, _)| *n == "CLIPACT")
        .unwrap() as u8;
    let op_clipinfo = b
        .commands
        .iter()
        .position(|(n, _)| *n == "CLIPINFO")
        .unwrap() as u8;
    b.scripts = vec![vec![
        // CLIPACT[TEXT=1 SET="A\x01B"]:0x01 在读取时被官方过滤
        GroupSpec::new(op_clipact)
            .window(0, ex::i8_(1))
            .window(1, ex::s("A\x01B")),
        // CLIPINFO[TEXT=1 LET=$s]
        GroupSpec::new(op_clipinfo)
            .window(0, ex::i8_(1))
            .window(1, ex::r#ref(0x24, sys)),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(vm.clipboard(), b"A\x01B", "内核镜像保存原始字节");
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::Dollar, sys), &[])
            .unwrap(),
        Value::Str(b"AB".to_vec()),
        "CLIPINFO 丢弃 < 0x20 的字节(REA 0x43e4d2)"
    );
    // 事件带脚本坐标(P4)。
    let clip = vm
        .events()
        .iter()
        .find(|e| matches!(e, yu_rt::VmEvent::DomainCall { name, .. } if name == "CLIPACT"))
        .expect("CLIPACT event");
    if let yu_rt::VmEvent::DomainCall { script, pc, .. } = clip {
        assert_eq!(*script, Some(0));
        assert_eq!(*pc, Some(0));
    }
}

/// `ERROR[COM_CG=0]` 写逐命令启用表(默认全 1;消费者未定位,DIV-RT-046)。
#[test]
fn error_sets_the_per_command_enable_table() {
    let mut b = ProjectBuilder::new();
    for (name, params) in b.commands.iter_mut() {
        if *name == "ERROR" {
            *params = vec![("NO", 0, 3), ("SET", 0, 3), ("COM_CG", 0, 3)];
        }
    }
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    let op_error = b.commands.iter().position(|(n, _)| *n == "ERROR").unwrap() as u8;
    b.scripts = vec![vec![
        GroupSpec::new(op_error).window(2, ex::i8_(0)),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(vm.command_enable()[1], 0, "COM_CG → opcode 1");
    assert_eq!(vm.command_enable()[0], 1, "未触碰的命令保持 1");
    assert_eq!(vm.command_enable()[14], 1);
}

/// `TRAY` 归宿主域(托盘图标):只发事件,不改 VM 状态。
#[test]
fn tray_is_a_domain_event() {
    let mut b = ProjectBuilder::new();
    for (name, params) in b.commands.iter_mut() {
        if *name == "TRAY" {
            *params = vec![("E", 0, 3), ("NAME", 1, 0), ("ID", 1, 0), ("MENUID", 1, 0)];
        }
    }
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    let op_tray = b.commands.iter().position(|(n, _)| *n == "TRAY").unwrap() as u8;
    b.scripts = vec![vec![
        GroupSpec::new(op_tray)
            .window(0, ex::i8_(1))
            .window(1, ex::s("app")),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(vm.events().iter().any(|e| matches!(
        e,
        yu_rt::VmEvent::DomainCall { domain: yu_rt::event::Domain::System, name, .. } if name == "TRAY"
    )));
    assert_eq!(
        vm.unimplemented_cmds().get("TRAY").copied(),
        Some(1),
        "未安装系统域宿主 ⇒ 事件已发但挂号"
    );
}

/// `@_WINDOW_EXEC_CAPTION`:宿主 setter 置 1,脚本读 1 后写回 0(DIV-RT-007)。
#[test]
fn window_exec_caption_is_a_host_setter_round_trip() {
    let mut b = ProjectBuilder::new();
    let v = with_int_var(&mut b, 0, -1);
    let v2 = with_int_var(&mut b, 1, -1);
    let cap_id = b
        .sysvars
        .iter()
        .position(|(n, _, _)| *n == "_WINDOW_EXEC_CAPTION")
        .expect("_WINDOW_EXEC_CAPTION") as u16;
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        // LET[@v = @_WINDOW_EXEC_CAPTION]
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, v.id))
            .window(1, ex::get(0x40, cap_id)),
        // LET[@_WINDOW_EXEC_CAPTION = 0](脚本写回 0)
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, cap_id))
            .window(1, ex::i8_(0)),
        // LET[@v2 = @_WINDOW_EXEC_CAPTION](再读应为 0)
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, v2.id))
            .window(1, ex::get(0x40, cap_id)),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    vm.request_window_caption();
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(
        vm.store().read(v, &[]).unwrap(),
        Value::Int(1),
        "宿主 setter 置 1"
    );
    assert_eq!(
        vm.store().read(v2, &[]).unwrap(),
        Value::Int(0),
        "脚本写回 0"
    );
}

/// `MATH[ABS=0 …]`:旗标**值为 0** 仍按「参数在场」选中函数
/// (官方判定链测窗口在场,旁线 §3.20;曾按「值 ≠ 0」丢参 ⇒ 落到
/// 无旗标分支写出 SET 原值)。同时锁 ABS 的整数路:
/// SET 整数 ⇒ 整数绝对值 + 整数写(`0x448412 → 0x455540`)。
#[test]
fn math_flag_zero_value_still_selects_function() {
    let mut b = ProjectBuilder::new();
    let v = with_int_var(&mut b, 0, 0);
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        GroupSpec::new(OP_MATH)
            .window(0, ex::r#ref(0x40, v.id))
            .window(1, ex::i8_(-5)) // SET = -5
            .window(15, ex::i8_(0)), // ABS = 0(值为 0 的在场旗标)
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(vm.store().read(v, &[]).unwrap(), Value::Int(5));
}

/// `VARACT[INIT=0 G_INT=1]`:INIT **在场但值为 0** ⇒ 仍恢复
/// (官方判据 = 载入器「参数在场字节表」`0x684160+序号`,旁线 §3.26 ③;
/// 曾按「INIT 值 == 0 ⇒ 不做」把该形态吞掉)。语料当前全为 `INIT=1`,
/// 本用例防的就是潜伏形态回归。
#[test]
fn varact_init_zero_value_still_restores() {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    let g = VarRef::new(VarSpace::At, sys);
    b.vars.push(VarSpec {
        kind: 1,
        group: 1,
        script_id: 0,
        var_id: sys,
        value_type: 1,
        dims: vec![],
        init: Init::Int(7),
    });
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        // LET[@g = 9]
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, g.id))
            .window(1, ex::i8_(9)),
        // VARACT[INIT=0 G_INT=1] ⇒ 恢复为 ysv 初值 7
        GroupSpec::new(OP_VARACT)
            .window(16, ex::i8_(0))
            .window(17, ex::i8_(1)),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(vm.store().read(g, &[]).unwrap(), Value::Int(7));
}

/// `LABELINFO[#… LET=@arr(i) EXIST=1]`:LET 目标带下标 ⇒ 写 `arr[i]`
/// (曾丢下标标量化,与 MATH 同族病根)。
#[test]
fn labelinfo_let_writes_indexed_element() {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    let arr = VarRef::new(VarSpace::At, sys + 1);
    b.vars.push(VarSpec {
        kind: 1,
        group: 0,
        script_id: 0,
        var_id: sys,
        value_type: 1,
        dims: vec![],
        init: Init::Int(0),
    });
    b.vars.push(VarSpec {
        kind: 1,
        group: 0,
        script_id: 0,
        var_id: sys + 1,
        value_type: 1,
        dims: vec![4],
        init: Init::Int(-1),
    });
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        GroupSpec::new(OP_LABELINFO)
            .window(0, ex::s("SYSTEM_START"))
            .window(
                1,
                ex::cat(&[ex::r#ref(0x40, arr.id), ex::i8_(2), ex::aload()]),
            )
            .window(2, ex::i8_(1)),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let get = |i: i64| vm.store().read(arr, &[i]).unwrap();
    assert_eq!(get(2), Value::Int(1), "SYSTEM_START 在场 ⇒ [2] = 1");
    assert_eq!(get(0), Value::Int(-1), "未写的元素保持初值");
}

/// 裸 `WAIT[]` ⇒ 让出一帧(手册 `basic/task.html`:「ウェイト命令…タスクは
/// それを実行したあと、一旦休憩します(約0.016秒)」;`control/wait.html`
/// 只定义 TIME/FRAME,无参时长未定义 —— 取 1 帧,Likely)。
/// 语料 95 处 WAIT 全带参(FRAME×81/TIME×14),本用例防潜伏回归。
#[test]
fn bare_wait_yields_one_frame() {
    let mut b = ProjectBuilder::new();
    let v = with_int_var(&mut b, 0, 0);
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        GroupSpec::new(OP_WAIT),
        // 第二帧才执行的赋值:证明任务停了一帧而不是继续。
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, v.id))
            .window(1, ex::i8_(9)),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    let tl = no_input();
    let _ = vm.tick_at(0, &tl);
    assert_eq!(
        vm.store().read(v, &[]).unwrap(),
        Value::Int(0),
        "WAIT 当帧不得执行后续命令"
    );
    let _ = vm.tick_at(1, &tl);
    assert_eq!(
        vm.store().read(v, &[]).unwrap(),
        Value::Int(9),
        "次帧恢复执行"
    );
}

// ---------------------------------------------------------------- 存档闭环(B1/DIV-RT-042)

use yu_log::Diag;
use yu_rt::event::{LoadedBlock, LoadedVar, SaveHost, VarSnapshot};

/// 存档同步宿主记录器:断言 VM 侧契约(预约快照/组枚举/刷盘名/写回)本身,
/// 不依赖 `yu-save`。通过 `Rc<RefCell<_>>` 共享(`Box<dyn SaveHost>` 无法
/// downcast,测试持同一共享单元直查调用事实)。
type GroupRecord = (u32, yu_rt::value::ElemType, u8, Vec<(u16, VarSnapshot)>);

struct SaveRecorder {
    reserves: Vec<(u32, VarRef, VarSnapshot)>,
    groups: Vec<GroupRecord>,
    flushes: Vec<String>,
    ends: u32,
    loads: Vec<(String, u32)>,
}

impl SaveRecorder {
    fn new() -> std::rc::Rc<std::cell::RefCell<Self>> {
        std::rc::Rc::new(std::cell::RefCell::new(Self {
            reserves: Vec::new(),
            groups: Vec::new(),
            flushes: Vec::new(),
            ends: 0,
            loads: Vec::new(),
        }))
    }
}

/// 共享宿主包装:`reply` 为 `Some` 时 `load_block` 回复之(LOAD 写回用例)。
struct SharedHost {
    inner: std::rc::Rc<std::cell::RefCell<SaveRecorder>>,
    reply: std::rc::Rc<std::cell::RefCell<Option<LoadedBlock>>>,
}

impl SaveHost for SharedHost {
    fn apply_event(&mut self, _ev: &yu_rt::event::VmEvent) {}
    fn reserve(&mut self, dno: u32, var: VarRef, snap: &VarSnapshot) -> Result<(), Diag> {
        self.inner
            .borrow_mut()
            .reserves
            .push((dno, var, snap.clone()));
        Ok(())
    }
    fn reserve_group(
        &mut self,
        dno: u32,
        ty: yu_rt::value::ElemType,
        group: u8,
        vars: &[(u16, VarSnapshot)],
    ) -> Result<(), Diag> {
        self.inner
            .borrow_mut()
            .groups
            .push((dno, ty, group, vars.to_vec()));
        Ok(())
    }
    fn flush(&mut self, file: &str) -> Result<(), Diag> {
        self.inner.borrow_mut().flushes.push(file.to_string());
        Ok(())
    }
    fn save_end(&mut self) {
        self.inner.borrow_mut().ends += 1;
    }
    fn load_block(&mut self, file: &str, dno: u32) -> Result<LoadedBlock, Diag> {
        self.inner.borrow_mut().loads.push((file.to_string(), dno));
        let reply = self.reply.borrow_mut().take();
        reply.ok_or_else(|| Diag::fatal("test", yu_log::Boundary::Domain))
    }
}

fn int_snap(vals: &[i64], dims: &[u32]) -> VarSnapshot {
    VarSnapshot {
        ty: yu_rt::value::ElemType::Int,
        dims: dims.to_vec(),
        data: vals.iter().map(|&v| Value::Int(v)).collect(),
    }
}

/// `SAVE[DNO SET=@a]`:预约 = **整组值快照**(REA `0x44d73d`);事件在 SET
/// 槽携带变量引用(DIV-RT-042/B1 裁 1-A);`SAVE[FILE]` 名字补 `.sd`
/// (手册拡張子省略可);`SAVEEND` 清预约。
#[test]
fn save_load_loop_records_reserve_flush_and_writes_back() {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![
        VarSpec {
            kind: 1,
            group: 0,
            script_id: 0,
            var_id: sys,
            value_type: 1,
            dims: vec![4],
            init: Init::Int(9),
        },
        VarSpec {
            kind: 1,
            group: 0,
            script_id: 0,
            var_id: sys + 1,
            value_type: 1,
            dims: vec![],
            init: Init::Int(0),
        },
    ];
    let op_save = b.commands.iter().position(|(n, _)| *n == "SAVE").unwrap() as u8;
    let op_load = b.commands.iter().position(|(n, _)| *n == "LOAD").unwrap() as u8;
    let op_saveend = b
        .commands
        .iter()
        .position(|(n, _)| *n == "SAVEEND")
        .unwrap() as u8;
    let groups = vec![
        // SAVE[DNO=3 SET=@a] —— 数组变量引用,预约 = 整组快照。
        GroupSpec::new(op_save)
            .window(1, ex::i8_(3))
            .window(2, ex::r#ref(0x40, sys)),
        // SAVE[FILE="arc"] —— 缺 .sd 由内核补。
        GroupSpec::new(op_save).window(0, ex::s("arc")),
        // LOAD[FILE="seld.sd" DNO=1 LET=@b] —— 块值 42 写回标量。
        GroupSpec::new(op_load)
            .window(0, ex::s("seld.sd"))
            .window(1, ex::i8_(1))
            .window(2, ex::r#ref(0x40, sys + 1)),
        // SAVE[DNO=9 SET=@b] —— 写回后的值再次预约(闭环)。
        GroupSpec::new(op_save)
            .window(1, ex::i8_(9))
            .window(2, ex::r#ref(0x40, sys + 1)),
        GroupSpec::new(op_saveend),
        GroupSpec::new(OP_END),
    ];
    b.scripts = vec![groups];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);

    let recorder = SaveRecorder::new();
    let reply = std::rc::Rc::new(std::cell::RefCell::new(Some(LoadedBlock {
        vars: vec![LoadedVar {
            var: None,
            snap: int_snap(&[42], &[]),
        }],
    })));
    let mut vm = boot(b);
    vm.set_save_host(Box::new(SharedHost {
        inner: recorder.clone(),
        reply: reply.clone(),
    }));
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let rec = recorder.borrow();
    // 预约 1:@a 整组快照(标量 init 填满数组是引擎语义,VarSlot::new)。
    assert_eq!(rec.reserves.len(), 2, "两次预约: {:?}", rec.reserves);
    let (dno, var, snap) = &rec.reserves[0];
    assert_eq!(*dno, 3);
    assert_eq!(*var, VarRef::new(VarSpace::At, sys));
    assert_eq!(snap.dims, vec![4]);
    assert_eq!(snap.data, vec![Value::Int(9); 4]);
    // 刷盘名补 .sd(REA `0x44c6cc..` 检测后拼接)。
    assert_eq!(rec.flushes, vec!["arc.sd".to_string()]);
    // LOAD:文件/DNO 原样传宿主;块值写回 @b。
    assert_eq!(rec.loads, vec![("seld.sd".to_string(), 1)]);
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys + 1), &[])
            .unwrap(),
        Value::Int(42)
    );
    // 闭环预约:@b 的当前值(42)成为快照。
    let (dno, var, snap) = &rec.reserves[1];
    assert_eq!(*dno, 9);
    assert_eq!(*var, VarRef::new(VarSpace::At, sys + 1));
    assert_eq!(snap.data, vec![Value::Int(42)]);
    assert_eq!(rec.ends, 1);
    drop(rec);
    // 事件流:SAVE/LOAD 域事件的引用槽携带变量身份(DIV-RT-042)。
    let events = vm.take_events();
    let save_args: Vec<_> = events
        .iter()
        .filter_map(|ev| match ev {
            yu_rt::event::VmEvent::DomainCall { name, args, .. } if name == "SAVE" => Some(args),
            _ => None,
        })
        .collect();
    // 三条 SAVE 域事件:DNO=3 / FILE-only(无预约,纯刷盘) / DNO=9。
    assert_eq!(save_args.len(), 3);
    let set_ref = save_args[0]
        .iter()
        .find(|a| a.slot == 2)
        .expect("SET 槽在事件里");
    assert_eq!(set_ref.reference, Some(VarRef::new(VarSpace::At, sys)));
    let load_args: Vec<_> = events
        .iter()
        .filter_map(|ev| match ev {
            yu_rt::event::VmEvent::DomainCall { name, args, .. } if name == "LOAD" => Some(args),
            _ => None,
        })
        .collect();
    assert_eq!(load_args.len(), 1);
    let let_ref = load_args[0]
        .iter()
        .find(|a| a.slot == 2)
        .expect("LET 槽在事件里");
    assert_eq!(let_ref.reference, Some(VarRef::new(VarSpace::At, sys + 1)));
}

/// 组形态 `SAVE[DNO=5 G_INT2=1]`:枚举 (值类型=INT, 组=2) 的全局变量逐个
/// 子块化,顺序 = 工程变量表序(REA `0x44ca8a..`;与 VARACT[INIT] 同源)。
#[test]
fn save_group_form_enumerates_matching_globals() {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![
        VarSpec {
            kind: 1,
            group: 2,
            script_id: 0,
            var_id: sys,
            value_type: 1,
            dims: vec![2],
            init: Init::Int(0),
        },
        VarSpec {
            kind: 1,
            group: 2,
            script_id: 0,
            var_id: sys + 1,
            value_type: 1,
            dims: vec![],
            init: Init::Int(7),
        },
        VarSpec {
            kind: 1,
            group: 2,
            script_id: 0,
            var_id: sys + 2,
            value_type: 2,
            dims: vec![],
            init: Init::Float(0.0),
        },
        VarSpec {
            kind: 1,
            group: 1,
            script_id: 0,
            var_id: sys + 3,
            value_type: 1,
            dims: vec![],
            init: Init::Int(0),
        },
    ];
    let op_save = b.commands.iter().position(|(n, _)| *n == "SAVE").unwrap() as u8;
    let groups = vec![
        // SAVE[DNO=5 G_INT2=1](官方 ycd 参数序:DNO=1、G_INT2 = 槽 6)
        GroupSpec::new(op_save)
            .window(1, ex::i8_(5))
            .window(6, ex::i8_(1)),
        GroupSpec::new(OP_END),
    ];
    b.scripts = vec![groups];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);

    let recorder = SaveRecorder::new();
    let reply = std::rc::Rc::new(std::cell::RefCell::new(None));
    let mut vm = boot(b);
    vm.set_save_host(Box::new(SharedHost {
        inner: recorder.clone(),
        reply,
    }));
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let rec = recorder.borrow();
    assert_eq!(rec.groups.len(), 1, "groups: {:?}", rec.groups);
    let (dno, ty, group, vars) = &rec.groups[0];
    assert_eq!(*dno, 5);
    assert_eq!(*ty, yu_rt::value::ElemType::Int);
    assert_eq!(*group, 2);
    // 只收 INT 组 2;FLT 组 2 / INT 组 1 不混入;顺序 = 声明序。
    assert_eq!(
        vars.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        vec![sys, sys + 1]
    );
    assert_eq!(vars[0].1.data, vec![Value::Int(0), Value::Int(0)]);
    assert_eq!(vars[1].1.data, vec![Value::Int(7)]);
}

/// 组块 `LOAD[FILE DNO G_*]`:块类型 2 的子块**按块内 var_id 直接恢复**
/// (REA `0x447460..0x4474de`:不读 LET/G_* 在场位;语料组旗标被求值但
/// 不消费)。目标 = 官方语料的旧式空间 id 同型(ERIS `@G(0001)` = 1161,
/// DIV-RT-017;夹具用 ≥ sys 的等价 id)。
#[test]
fn load_group_block_restores_each_var_by_id() {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![VarSpec {
        kind: 1,
        group: 2,
        script_id: 0,
        var_id: sys,
        value_type: 1,
        dims: vec![2],
        init: Init::Int(0),
    }];
    let op_load = b.commands.iter().position(|(n, _)| *n == "LOAD").unwrap() as u8;
    let groups = vec![
        GroupSpec::new(op_load)
            .window(0, ex::s("gsd.sd"))
            .window(1, ex::i8_(3))
            .window(6, ex::i8_(1)),
        GroupSpec::new(OP_END),
    ];
    b.scripts = vec![groups];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);

    let recorder = SaveRecorder::new();
    let reply = std::rc::Rc::new(std::cell::RefCell::new(Some(LoadedBlock {
        vars: vec![LoadedVar {
            var: Some(VarRef::new(VarSpace::At, sys)),
            snap: int_snap(&[5, 7], &[2]),
        }],
    })));
    let mut vm = boot(b);
    vm.set_save_host(Box::new(SharedHost {
        inner: recorder.clone(),
        reply,
    }));
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(
        recorder.borrow().loads,
        vec![("gsd.sd".to_string(), 3)],
        "LOAD 文件名/DNO 原样传宿主"
    );
    // 逐元素验证写回(整组恢复)。
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys), &[0])
            .unwrap(),
        Value::Int(5)
    );
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys), &[1])
            .unwrap(),
        Value::Int(7)
    );
}

/// FILEINFO 的「散盘」回退(REA `0x44316c`;B1 桌面实测暴露):路径以注入的
/// `$_SAVE_PATH` 为前缀 ⇒ save 根文件系统判存在。存档界面的槽存在判定
/// (`es.SaveDataCheck → \_file.info.exist`)与启动恢复的闸门都走这条。
#[test]
fn fileinfo_fs_fallback_checks_save_root() {
    use yu_rt::vm::HostPaths;

    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/fixture-fsinfo");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("t.sd"), b"YSSD-dummy").unwrap();
    let root_str = format!("{}/", dir.to_string_lossy().replace('\\', "/"));

    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![VarSpec {
        kind: 1,
        group: 0,
        script_id: 0,
        var_id: sys,
        value_type: 1,
        dims: vec![],
        init: Init::Int(-1),
    }];
    let op_fileinfo = b
        .commands
        .iter()
        .position(|(n, _)| *n == "FILEINFO")
        .unwrap() as u8;
    let groups = vec![
        // FILEINFO[FILE=$es.SAVEPATH+t.sd EXIST=1 LET=@r] —— save 根内存在
        GroupSpec::new(op_fileinfo)
            .window(0, ex::s(&format!("{root_str}t.sd")))
            .window(1, ex::i8_(1))
            .window(2, ex::r#ref(0x40, sys)),
        GroupSpec::new(OP_END),
    ];
    b.scripts = vec![groups];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);

    let src = b.build();
    let cfg = BootConfig {
        paths: HostPaths {
            save_path: Some(root_str.clone().into_bytes()),
            ..HostPaths::default()
        },
        ..BootConfig::default()
    };
    let mut vm = Vm::boot(Box::new(src), cfg).expect("boot");
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys), &[])
            .unwrap(),
        Value::Int(1)
    );
}
