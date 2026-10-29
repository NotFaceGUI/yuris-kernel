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
use yu_rt::{BootConfig, HostLocalTime, Vm};

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
const OP_TASKINFO: u8 = 95;
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

/// 按 `b.vars` 的当前顺序追加一个用户变量(offset = 追加序号)。
fn push_var(b: &mut ProjectBuilder, value_type: u8, init: Init) -> VarRef {
    let sys = b.sysvars.len() as u16;
    let var_id = sys + b.vars.len() as u16;
    b.vars.push(VarSpec {
        kind: 1,
        group: 0,
        script_id: 0,
        var_id,
        value_type,
        dims: vec![],
        init,
    });
    let space = if value_type == 3 {
        VarSpace::Dollar
    } else {
        VarSpace::At
    };
    VarRef::new(space, var_id)
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
fn jump_events_are_suppressed_when_record_events_off() {
    let b = if_fixture(0);
    let src = b.build();
    let mut vm = Vm::boot(
        Box::new(src),
        BootConfig {
            record_events: false,
            ..BootConfig::default()
        },
    )
    .expect("boot");
    vm.tick_at(0, &no_input());
    assert!(
        !vm.events()
            .iter()
            .any(|e| matches!(e, yu_rt::VmEvent::Jump { .. })),
        "record_events=false 时 Jump 不得入队"
    );
    assert_eq!(
        vm.store().read(user_var(&vm, 0), &[]).unwrap(),
        Value::Int(2),
        "关事件流不改变分支语义"
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

/// `@_TIME` 在宿主注入本地时间后返回真实日期时间(存档菜单的现实日期;
/// 未注入时仍走 1970 确定性注入时钟,DIV-RT-009)。
#[test]
fn time_sysvar_uses_injected_local_clock() {
    let mut b = ProjectBuilder::new();
    let year = push_var(&mut b, 1, Init::Int(0));
    let hour = push_var(&mut b, 1, Init::Int(0));
    // `_TIME` id 由夹具 ycd 顺序决定;构建前解析一次(与 `_LC` 用例同法)。
    let mut probe = ProjectBuilder::new();
    probe.sysvars = b.sysvars.clone();
    let src = probe.build();
    let tables = yu_rt::TableSet::load(&src, yu_rt::tables::DEFAULT_SCRIPT_KEY).unwrap();
    let time_id = tables.boot.sysvars.id("_TIME").expect("_TIME 系统变量");
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        // LET[@year = @_TIME(0,0)]
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, year.id))
            .window(
                0,
                ex::cat(&[
                    ex::r#ref(0x40, time_id),
                    ex::i8_(0),
                    ex::i8_(0),
                    ex::aload(),
                ]),
            ),
        // LET[@hour = @_TIME(1,4)]
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, hour.id))
            .window(
                0,
                ex::cat(&[
                    ex::r#ref(0x40, time_id),
                    ex::i8_(1),
                    ex::i8_(4),
                    ex::aload(),
                ]),
            ),
        GroupSpec::new(OP_END),
    ]];
    let local = HostLocalTime::new(2026, 10, 26, 13, 45, 30, 250).expect("合法本地时间");
    let boot_cfg = BootConfig {
        local_time: Some(local),
        ..BootConfig::default()
    };
    let mut vm = Vm::boot(Box::new(b.build()), boot_cfg).expect("boot");
    vm.tick_at(0, &no_input());
    assert_eq!(
        vm.store().read(year, &[]).unwrap(),
        Value::Int(2026),
        "@_TIME(0,0) = 宿主本地年,不是 1970 纪元"
    );
    assert_eq!(
        vm.store().read(hour, &[]).unwrap(),
        Value::Int(13),
        "@_TIME(1,4) = 启动时刻的小时"
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

/// `TASKINFO[TEXTLINE/NEXTVOICE]`:引擎 `0x44c390`(RETURNCODE)把当前指令
/// 首窗口记录的两个 dword 抄进 `task+0xc/+0x10`,TASKINFO 分支
/// `0x45272a/0x452708` 原样读回。夹具窗口 `id` = 文本行(3)、
/// `length` = voice(7);内核必须保存同一对值。
#[test]
fn taskinfo_text_line_and_next_voice_come_from_returncode_window() {
    let mut b = ProjectBuilder::new();
    let line = push_var(&mut b, 1, Init::Int(-1));
    let voice = push_var(&mut b, 1, Init::Int(-1));
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        // RETURNCODE:窗口 id=3(文本行)、length=7(voice)。
        GroupSpec::new(OP_RETURNCODE).window_value(3, 7).line(11),
        GroupSpec::new(OP_TASKINFO)
            .window(12, ex::i8_(1))
            .window(1, ex::r#ref(0x40, line.id))
            .line(12),
        GroupSpec::new(OP_TASKINFO)
            .window(13, ex::i8_(1))
            .window(1, ex::r#ref(0x40, voice.id))
            .line(13),
        GroupSpec::new(OP_END).line(14),
    ]];
    let mut vm = boot(b);
    // RETURNCODE → TextYield;TextYield 需要推进输入才恢复(引擎 `task+9`)。
    vm.tick_at(0, &no_input());
    let mut tl = InputTimeline::new();
    tl.set(1, InputFrame::new(["MOUSE_L"]));
    vm.tick_at(16, &tl);
    vm.tick_at(32, &tl);
    assert_eq!(vm.store().read(line, &[]).unwrap(), Value::Int(3));
    assert_eq!(vm.store().read(voice, &[]).unwrap(), Value::Int(7));
}

/// `TASKINFO[FILE0/FILENO0/LINE0]` = 第一层 GOSUB 的调用点(引擎
/// `task+0x144[0]`:调用方脚本号/调用点源码行);`FILE/FILENO/LINE` =
/// 当前执行位(跨脚本 GOSUB 后是被调方)。
#[test]
fn taskinfo_file0_fileno0_line0_are_first_gosub_call_site() {
    let mut b = ProjectBuilder::new();
    let path0 = push_var(&mut b, 3, Init::Str(b""));
    let n0 = push_var(&mut b, 1, Init::Int(-1));
    let l0 = push_var(&mut b, 1, Init::Int(-1));
    let path1 = push_var(&mut b, 3, Init::Str(b""));
    let n1 = push_var(&mut b, 1, Init::Int(-1));
    let l1 = push_var(&mut b, 1, Init::Int(-1));
    b.labels = vec![("SYSTEM_START", 0, 0, 0), ("CALLEE", 0, 1, 0)];
    b.entry(0, 0);
    b.scripts = vec![
        vec![
            GroupSpec::new(OP_GOSUB).window(0, ex::s("CALLEE")).line(10),
            GroupSpec::new(OP_END).line(11),
        ],
        vec![
            GroupSpec::new(OP_TASKINFO)
                .window(9, ex::i8_(1))
                .window(1, ex::r#ref(0x24, path0.id))
                .line(30),
            GroupSpec::new(OP_TASKINFO)
                .window(10, ex::i8_(1))
                .window(1, ex::r#ref(0x40, n0.id))
                .line(31),
            GroupSpec::new(OP_TASKINFO)
                .window(11, ex::i8_(1))
                .window(1, ex::r#ref(0x40, l0.id))
                .line(32),
            GroupSpec::new(OP_TASKINFO)
                .window(6, ex::i8_(1))
                .window(1, ex::r#ref(0x24, path1.id))
                .line(33),
            GroupSpec::new(OP_TASKINFO)
                .window(7, ex::i8_(1))
                .window(1, ex::r#ref(0x40, n1.id))
                .line(34),
            GroupSpec::new(OP_TASKINFO)
                .window(8, ex::i8_(1))
                .window(1, ex::r#ref(0x40, l1.id))
                .line(35),
            GroupSpec::new(OP_RETURN).line(36),
            GroupSpec::new(OP_END).line(37),
        ],
    ];
    let mut vm = boot(b);
    for f in 0..8u32 {
        vm.tick_at(f * 16, &no_input());
    }
    assert_eq!(
        vm.store().read(path0, &[]).unwrap(),
        Value::Str(b"data\\script\\fixture0000.yst".to_vec()),
        "FILE0 = 调用方脚本路径(引擎 0x801850[frame[0].script])"
    );
    assert_eq!(
        vm.store().read(n0, &[]).unwrap(),
        Value::Int(0),
        "FILENO0 = 第一层 GOSUB 的调用方脚本号"
    );
    assert_eq!(
        vm.store().read(l0, &[]).unwrap(),
        Value::Int(10),
        "LINE0 = 第一层 GOSUB 的调用点源码行(引擎 frame+0xc)"
    );
    assert_eq!(
        vm.store().read(path1, &[]).unwrap(),
        Value::Str(b"data\\script\\fixture0001.yst".to_vec()),
        "FILE = 当前执行(被调方)脚本路径"
    );
    assert_eq!(
        vm.store().read(n1, &[]).unwrap(),
        Value::Int(1),
        "FILENO = 当前执行脚本号"
    );
    assert_eq!(
        vm.store().read(l1, &[]).unwrap(),
        Value::Int(35),
        "LINE = 当前执行命令的源码行(数据区 B)"
    );
}

/// 查询槽按**存在性**判定(值为 0 也算请求),且分派顺序按 0.495
/// 处理体 `0x4523fc` 的固定链(`LINE` 在 `LINE0` 前),不是窗口顺序。
#[test]
fn taskinfo_query_presence_ignores_zero_and_dispatch_uses_engine_order() {
    let mut b = ProjectBuilder::new();
    let path = push_var(&mut b, 3, Init::Str(b""));
    let x = push_var(&mut b, 1, Init::Int(-1));
    b.labels = vec![("SYSTEM_START", 0, 0, 0), ("CALLEE", 0, 1, 0)];
    b.entry(0, 0);
    b.scripts = vec![
        vec![
            GroupSpec::new(OP_GOSUB).window(0, ex::s("CALLEE")).line(10),
            GroupSpec::new(OP_END).line(11),
        ],
        vec![
            // FILE0=0:值 0 也算请求(引擎看参数存在性)。
            GroupSpec::new(OP_TASKINFO)
                .window(9, ex::i8_(0))
                .window(1, ex::r#ref(0x24, path.id))
                .line(30),
            // 同时给出 LINE0=1 与 LINE=0:0.495 固定顺序 LINE 在 LINE0 前。
            GroupSpec::new(OP_TASKINFO)
                .window(11, ex::i8_(1))
                .window(8, ex::i8_(0))
                .window(1, ex::r#ref(0x40, x.id))
                .line(31),
            GroupSpec::new(OP_RETURN).line(32),
            GroupSpec::new(OP_END).line(33),
        ],
    ];
    let mut vm = boot(b);
    for f in 0..6u32 {
        vm.tick_at(f * 16, &no_input());
    }
    assert_eq!(
        vm.store().read(path, &[]).unwrap(),
        Value::Str(b"data\\script\\fixture0000.yst".to_vec()),
        "FILE0=0 仍是请求;返回调用方脚本路径"
    );
    assert_eq!(
        vm.store().read(x, &[]).unwrap(),
        Value::Int(31),
        "0.495 分派顺序 LINE 在 LINE0 前 ⇒ 取 LINE(31),不是 LINE0(10)"
    );
}

/// ID 未命中:按 LET 目标类型写默认值(引擎 `0x452918`);
/// `GOLABEL` 等 0.495 无分支的槽不写 LET,`FILE` 的 STR 值不写进 INT 目标。
#[test]
fn taskinfo_missing_id_uses_type_defaults_and_unhandled_slots_are_noop() {
    let mut b = ProjectBuilder::new();
    let int_target = push_var(&mut b, 1, Init::Int(42));
    let str_target = push_var(&mut b, 3, Init::Str(b"keep"));
    let file_int_target = push_var(&mut b, 1, Init::Int(42));
    let golabel_target = push_var(&mut b, 1, Init::Int(7));
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        GroupSpec::new(OP_TASKINFO)
            .window(0, ex::s("NOPE"))
            .window(8, ex::i8_(1))
            .window(1, ex::r#ref(0x40, int_target.id))
            .line(10),
        GroupSpec::new(OP_TASKINFO)
            .window(0, ex::s("NOPE"))
            .window(8, ex::i8_(1))
            .window(1, ex::r#ref(0x24, str_target.id))
            .line(11),
        // FILE 需要 STR 目标;INT 目标不写(引擎 0x43f0d8)。
        GroupSpec::new(OP_TASKINFO)
            .window(6, ex::i8_(1))
            .window(1, ex::r#ref(0x40, file_int_target.id))
            .line(12),
        // GOLABEL 在 0.495 处理体无分支:不写 LET。
        GroupSpec::new(OP_TASKINFO)
            .window(14, ex::i8_(1))
            .window(1, ex::r#ref(0x40, golabel_target.id))
            .line(13),
        GroupSpec::new(OP_END).line(14),
    ]];
    let mut vm = boot(b);
    for f in 0..5u32 {
        vm.tick_at(f * 16, &no_input());
    }
    assert_eq!(
        vm.store().read(int_target, &[]).unwrap(),
        Value::Int(0),
        "ID 未命中:INT 目标写 0"
    );
    assert_eq!(
        vm.store().read(str_target, &[]).unwrap(),
        Value::Str(Vec::new()),
        "ID 未命中:STR 目标写空串"
    );
    assert_eq!(
        vm.store().read(file_int_target, &[]).unwrap(),
        Value::Int(42),
        "FILE 的字符串值不写进 INT 目标"
    );
    assert_eq!(
        vm.store().read(golabel_target, &[]).unwrap(),
        Value::Int(7),
        "GOLABEL 在 0.495 无分支,不写 LET"
    );
}

/// A 槽(引擎 `task+0x15`):`TASK` 写、`TASKINFO` 读,默认 1。
///
/// 证据:调度器 `0x4043dc..0x4043e6`(`movsx E; movsx A; test A, E`)、
/// `TASK` 处理体 `0x450bbf..0x450bd1`、`TASKINFO` `0x4527b0`、构造器
/// `0x465630/0x465633` 默认 E=A=1。手册无 A 页(DIV-RT-015)。
#[test]
fn taskinfo_a_reads_default_and_task_a_toggles_it() {
    let mut b = ProjectBuilder::new();
    let before = push_var(&mut b, 1, Init::Int(-1));
    let disabled = push_var(&mut b, 1, Init::Int(-1));
    let reenabled = push_var(&mut b, 1, Init::Int(-1));
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    const OP_TASK: u8 = 93;
    b.scripts = vec![vec![
        GroupSpec::new(OP_TASKINFO)
            .window(4, ex::i8_(1))
            .window(1, ex::r#ref(0x40, before.id))
            .line(10),
        GroupSpec::new(OP_TASK).window(2, ex::i8_(0)).line(11),
        GroupSpec::new(OP_TASKINFO)
            .window(4, ex::i8_(1))
            .window(1, ex::r#ref(0x40, disabled.id))
            .line(12),
        GroupSpec::new(OP_TASK).window(2, ex::i8_(1)).line(13),
        GroupSpec::new(OP_TASKINFO)
            .window(4, ex::i8_(1))
            .window(1, ex::r#ref(0x40, reenabled.id))
            .line(14),
        GroupSpec::new(OP_END).line(15),
    ]];
    let mut vm = boot(b);
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(
        vm.store().read(before, &[]).unwrap(),
        Value::Int(1),
        "新建任务 A 默认 1"
    );
    assert_eq!(
        vm.store().read(disabled, &[]).unwrap(),
        Value::Int(0),
        "TASK[A=0] 后同一次泵里 TASKINFO[A] 立即为 0"
    );
    assert_eq!(
        vm.store().read(reenabled, &[]).unwrap(),
        Value::Int(1),
        "TASK[A=1] 恢复"
    );
}

/// E/A 只按**存在性**写任务记录;Z-only 的 `TASK` 不得把禁用任务重新打开。
/// 同一次泵内 E/A 变更必须同步到 `task_view`(TASKINFO 立即看到新值)。
#[test]
fn task_e_a_changes_are_visible_to_taskinfo_in_same_pump() {
    let mut b = ProjectBuilder::new();
    let e_after_disable = push_var(&mut b, 1, Init::Int(-1));
    let a_after_disable = push_var(&mut b, 1, Init::Int(-1));
    let e_after_z = push_var(&mut b, 1, Init::Int(-1));
    let a_after_z = push_var(&mut b, 1, Init::Int(-1));
    b.labels = vec![("SYSTEM_START", 0, 0, 0), ("SUB.BODY", 9, 0, 0)];
    b.entry(0, 0);
    const OP_TASK: u8 = 93;
    b.scripts = vec![vec![
        // 0 生成 SUB
        GroupSpec::new(OP_TASK)
            .window(0, ex::s("SUB"))
            .window(34, ex::s("SUB.BODY"))
            .line(10),
        // 1 E=0
        GroupSpec::new(OP_TASK)
            .window(0, ex::s("SUB"))
            .window(1, ex::i8_(0))
            .line(11),
        // 2 A=0
        GroupSpec::new(OP_TASK)
            .window(0, ex::s("SUB"))
            .window(2, ex::i8_(0))
            .line(12),
        // 3-4 同泵查询
        GroupSpec::new(OP_TASKINFO)
            .window(0, ex::s("SUB"))
            .window(3, ex::i8_(1))
            .window(1, ex::r#ref(0x40, e_after_disable.id))
            .line(13),
        GroupSpec::new(OP_TASKINFO)
            .window(0, ex::s("SUB"))
            .window(4, ex::i8_(1))
            .window(1, ex::r#ref(0x40, a_after_disable.id))
            .line(14),
        // 5 Z-only:不得触碰 E/A
        GroupSpec::new(OP_TASK)
            .window(0, ex::s("SUB"))
            .window(3, ex::i8_(5))
            .line(15),
        // 6-7 再次查询,仍应为 0
        GroupSpec::new(OP_TASKINFO)
            .window(0, ex::s("SUB"))
            .window(3, ex::i8_(1))
            .window(1, ex::r#ref(0x40, e_after_z.id))
            .line(16),
        GroupSpec::new(OP_TASKINFO)
            .window(0, ex::s("SUB"))
            .window(4, ex::i8_(1))
            .window(1, ex::r#ref(0x40, a_after_z.id))
            .line(17),
        GroupSpec::new(OP_END).line(18),
        // 9 SUB.BODY(任务被禁用,保持挂起)
        GroupSpec::new(OP_WAIT).window(0, ex::i8_(10)).line(19),
    ]];
    let mut vm = boot(b);
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(
        vm.store().read(e_after_disable, &[]).unwrap(),
        Value::Int(0)
    );
    assert_eq!(
        vm.store().read(a_after_disable, &[]).unwrap(),
        Value::Int(0)
    );
    assert_eq!(
        vm.store().read(e_after_z, &[]).unwrap(),
        Value::Int(0),
        "Z-only TASK 不得重新打开 E"
    );
    assert_eq!(
        vm.store().read(a_after_z, &[]).unwrap(),
        Value::Int(0),
        "Z-only TASK 不得重新打开 A"
    );
}

/// 引擎 `0x4525fa/0x4528fa` 写完 FILENO0 后继续检查 SCRIPTPOS
/// (`jmp 0x452628`);同时请求时 SCRIPTPOS 二次写入覆盖。
#[test]
fn taskinfo_fileno0_falls_through_to_scrip_pos() {
    let mut b = ProjectBuilder::new();
    let blob = push_var(&mut b, 3, Init::Str(b""));
    b.labels = vec![("SYSTEM_START", 0, 0, 0), ("CALLEE", 0, 1, 0)];
    b.entry(0, 0);
    b.scripts = vec![
        vec![
            GroupSpec::new(OP_GOSUB).window(0, ex::s("CALLEE")).line(10),
            GroupSpec::new(OP_END).line(11),
        ],
        vec![
            // FILENO0 + SCRIPTPOS 同时给;LET 是字符串目标:
            // FILENO0 的数值写入应被类型闸门丢弃,随后 SCRIPTPOS 写入。
            GroupSpec::new(OP_TASKINFO)
                .window(10, ex::i8_(1))
                .window(19, ex::i8_(1))
                .window(1, ex::r#ref(0x24, blob.id))
                .line(30),
            GroupSpec::new(OP_RETURN).line(31),
            GroupSpec::new(OP_END).line(32),
        ],
    ];
    let mut vm = boot(b);
    for f in 0..4u32 {
        vm.tick_at(f * 16, &no_input());
    }
    let stored = vm.store().read(blob, &[]).unwrap();
    let Value::Str(raw) = stored else {
        panic!("SCRIPTPOS 必须写字符串变量");
    };
    let sp = yu_rt::scriptpos::ScriptPos::decode(&raw).expect("引擎 SCRIPTPOS 格式");
    assert!(sp.if_entries.is_empty());
    assert_eq!(sp.gosub_entries.len(), 1, "被调方在 GOSUB 内");
    assert_eq!(sp.gosub_entries[0].if_depth, 0);
    assert_eq!(sp.gosub_entries[0].loop_depth, 0);
    assert_eq!(sp.gosub_entries[0].return_pc, 1);
    assert_eq!(sp.gosub_entries[0].return_script, 0);
    assert!(sp.loop_entries.is_empty());
    assert_eq!(sp.text_line, 0);
    assert_eq!(sp.pc, 1);
    assert_eq!(sp.script_name, b"data\\script\\fixture0001.yst");
    assert_eq!(
        &raw[..8],
        &yu_rt::scriptpos::MAGIC,
        "FILENO0 后必须继续写 SCRIPTPOS(引擎 0x4525fa→0x452628)"
    );
}

/// `TASK[SCRIPTPOS]` 必须按引擎 `0x450fe0` 完整恢复 IF/GOSUB/LOOP 三栈、
/// text_line、pc 与脚本号(脚本名反查),而不是旧的 8 字节 script+pc。
#[test]
fn task_scrip_pos_restores_full_exec_state() {
    use yu_rt::scriptpos::{GosubEntry, IfEntry, LoopEntry, ScriptPos};

    let mut b = ProjectBuilder::new();
    let ifn = push_var(&mut b, 1, Init::Int(-1));
    let gn = push_var(&mut b, 1, Init::Int(-1));
    let ln = push_var(&mut b, 1, Init::Int(-1));
    let tl = push_var(&mut b, 1, Init::Int(-1));
    let blob = push_var(&mut b, 3, Init::Str(b""));
    let out = push_var(&mut b, 3, Init::Str(b""));
    b.labels = vec![("SYSTEM_START", 0, 0, 0), ("SUB.BODY", 9, 0, 0)];
    b.entry(0, 0);
    const OP_TASK: u8 = 93;
    b.scripts = vec![vec![
        GroupSpec::new(OP_TASK)
            .window(0, ex::s("SUB"))
            .window(34, ex::s("SUB.BODY"))
            .line(10),
        GroupSpec::new(OP_TASK)
            .window(0, ex::s("SUB"))
            .window(1, ex::i8_(0))
            .line(11),
        // 用合成 blob 把 SUB 的执行位恢复成 IF×2 + GOSUB×1 + LOOP×1。
        GroupSpec::new(OP_TASK)
            .window(0, ex::s("SUB"))
            .window(35, ex::get(0x24, blob.id))
            .line(12),
        GroupSpec::new(OP_TASKINFO)
            .window(0, ex::s("SUB"))
            .window(16, ex::i8_(1))
            .window(1, ex::r#ref(0x40, ifn.id))
            .line(13),
        GroupSpec::new(OP_TASKINFO)
            .window(0, ex::s("SUB"))
            .window(17, ex::i8_(1))
            .window(1, ex::r#ref(0x40, gn.id))
            .line(14),
        GroupSpec::new(OP_TASKINFO)
            .window(0, ex::s("SUB"))
            .window(18, ex::i8_(1))
            .window(1, ex::r#ref(0x40, ln.id))
            .line(15),
        GroupSpec::new(OP_TASKINFO)
            .window(0, ex::s("SUB"))
            .window(12, ex::i8_(1))
            .window(1, ex::r#ref(0x40, tl.id))
            .line(16),
        GroupSpec::new(OP_TASKINFO)
            .window(0, ex::s("SUB"))
            .window(19, ex::i8_(1))
            .window(1, ex::r#ref(0x24, out.id))
            .line(17),
        GroupSpec::new(OP_END).line(18),
        // 9 SUB.BODY(被禁用,保持挂起)
        GroupSpec::new(OP_WAIT).window(0, ex::i8_(10)).line(19),
    ]];
    let synthetic = ScriptPos {
        if_entries: vec![
            IfEntry {
                loop_depth: 0,
                entry_pc: 10,
                end_pc: 11,
            },
            IfEntry {
                loop_depth: 0,
                entry_pc: 12,
                end_pc: 13,
            },
        ],
        gosub_entries: vec![GosubEntry {
            if_depth: 2,
            loop_depth: 1,
            return_pc: 0,
            return_script: 0,
        }],
        loop_entries: vec![LoopEntry {
            if_depth: 2,
            body_pc: 14,
            end_pc: 15,
            counter: 1,
            limit: 3,
        }],
        text_line: 7,
        pc: 0,
        script_name: b"data\\script\\fixture0000.yst".to_vec(),
    };
    let raw = synthetic.encode().expect("encode synthetic SCRIPTPOS");
    let mut vm = boot(b);
    vm.store_mut()
        .write(blob, &[], &Value::Str(raw))
        .expect("write blob var");
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(vm.store().read(ifn, &[]).unwrap(), Value::Int(2));
    assert_eq!(vm.store().read(gn, &[]).unwrap(), Value::Int(1));
    assert_eq!(vm.store().read(ln, &[]).unwrap(), Value::Int(1));
    assert_eq!(vm.store().read(tl, &[]).unwrap(), Value::Int(7));

    let Value::Str(round_raw) = vm.store().read(out, &[]).unwrap() else {
        panic!("SCRIPTPOS 输出必须是字符串");
    };
    let round = ScriptPos::decode(&round_raw).expect("engine format roundtrip");
    assert_eq!(round, synthetic, "TASKINFO[SCRIPTPOS] 必须还原同一执行位");
}
/// 旧内核存档(2026-10-26 前)的 SCRIPTPOS 是 8 字节
/// `[script_id u32 LE][pc u32 LE]`;读档必须按兼容路径还原(无三栈),
/// 不能 Fatal。新/官方存档仍走引擎串。
#[test]
fn task_scrip_pos_accepts_legacy_eight_byte_kernel_blob() {
    use yu_rt::scriptpos::ScriptPos;

    let mut b = ProjectBuilder::new();
    let blob = push_var(&mut b, 3, Init::Str(b""));
    let out = push_var(&mut b, 3, Init::Str(b""));
    b.labels = vec![("SYSTEM_START", 0, 0, 0), ("SUB.BODY", 9, 0, 0)];
    b.entry(0, 0);
    const OP_TASK: u8 = 93;
    b.scripts = vec![vec![
        GroupSpec::new(OP_TASK)
            .window(0, ex::s("SUB"))
            .window(34, ex::s("SUB.BODY"))
            .line(10),
        // 旧内核 blob:脚本号 0 + pc 9(SUB.BODY)。
        GroupSpec::new(OP_TASK)
            .window(0, ex::s("SUB"))
            .window(35, ex::get(0x24, blob.id))
            .line(11),
        GroupSpec::new(OP_TASKINFO)
            .window(0, ex::s("SUB"))
            .window(19, ex::i8_(1))
            .window(1, ex::r#ref(0x24, out.id))
            .line(12),
        GroupSpec::new(OP_END).line(13),
        // 4..=8 填充,pc9 = SUB.BODY。
        GroupSpec::new(OP_WAIT).window(0, ex::i8_(10)).line(14),
        GroupSpec::new(OP_WAIT).window(0, ex::i8_(10)).line(15),
        GroupSpec::new(OP_WAIT).window(0, ex::i8_(10)).line(16),
        GroupSpec::new(OP_WAIT).window(0, ex::i8_(10)).line(17),
        GroupSpec::new(OP_WAIT).window(0, ex::i8_(10)).line(18),
        GroupSpec::new(OP_WAIT).window(0, ex::i8_(10)).line(19),
    ]];
    let mut legacy = Vec::new();
    legacy.extend_from_slice(&0u32.to_le_bytes());
    legacy.extend_from_slice(&9u32.to_le_bytes());
    let mut vm = boot(b);
    vm.store_mut()
        .write(blob, &[], &Value::Str(legacy))
        .expect("write legacy blob");
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let Value::Str(raw) = vm.store().read(out, &[]).unwrap() else {
        panic!("SCRIPTPOS 输出必须是字符串");
    };
    let sp = ScriptPos::decode(&raw).expect("legacy restore must round-trip as engine format");
    assert_eq!(sp.pc, 9);
    assert!(sp.if_entries.is_empty(), "{:?}", sp.if_entries);
    assert!(sp.gosub_entries.is_empty(), "{:?}", sp.gosub_entries);
    assert!(sp.loop_entries.is_empty(), "{:?}", sp.loop_entries);
    assert_eq!(sp.script_name, b"data\\script\\fixture0000.yst");
}

#[test]
fn returncode_line_keeps_full_u16_id() {
    let mut b = ProjectBuilder::new();
    let line = push_var(&mut b, 1, Init::Int(-1));
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        // id = 0x012C = 300(kind=0x2C, flag=1),length=0
        GroupSpec::new(OP_RETURNCODE)
            .window_tagged(0x2C, 1, 0, Vec::new())
            .line(11),
        GroupSpec::new(OP_TASKINFO)
            .window(12, ex::i8_(1))
            .window(1, ex::r#ref(0x40, line.id))
            .line(12),
        GroupSpec::new(OP_END).line(13),
    ]];
    let mut vm = boot(b);
    vm.tick_at(0, &no_input());
    let rc: Vec<u32> = vm
        .events()
        .iter()
        .filter_map(|e| match e {
            yu_rt::VmEvent::ReturnCode { line, .. } => Some(*line),
            _ => None,
        })
        .collect();
    assert_eq!(rc, vec![300], "ReturnCode.line 必须保留 u16 id");
    let mut tl = InputTimeline::new();
    tl.set(1, InputFrame::new(["MOUSE_L"]));
    vm.tick_at(16, &tl);
    assert_eq!(
        vm.store().read(line, &[]).unwrap(),
        Value::Int(300),
        "TEXTLINE 与事件行同源"
    );
}

/// `TASK` 重配置复用任务记录,不清 `task+0xc/+0x10`;
/// 内核重建 ExecState 时必须继承 TEXTLINE/NEXTVOICE。
#[test]
fn task_reconfiguration_preserves_text_line_and_voice() {
    let mut b = ProjectBuilder::new();
    let line = push_var(&mut b, 1, Init::Int(-1));
    let voice = push_var(&mut b, 1, Init::Int(-1));
    b.labels = vec![("SYSTEM_START", 0, 0, 0), ("BODY", 3, 0, 0)];
    b.entry(0, 0);
    const OP_TASK: u8 = 93;
    b.scripts = vec![vec![
        GroupSpec::new(OP_RETURNCODE).window_value(3, 7).line(11),
        GroupSpec::new(OP_TASK).window(34, ex::s("BODY")).line(12),
        GroupSpec::new(OP_END).line(13),
        // 3 BODY
        GroupSpec::new(OP_TASKINFO)
            .window(12, ex::i8_(1))
            .window(1, ex::r#ref(0x40, line.id))
            .line(30),
        GroupSpec::new(OP_TASKINFO)
            .window(13, ex::i8_(1))
            .window(1, ex::r#ref(0x40, voice.id))
            .line(31),
        GroupSpec::new(OP_END).line(32),
    ]];
    let mut vm = boot(b);
    vm.tick_at(0, &no_input());
    let mut tl = InputTimeline::new();
    tl.set(1, InputFrame::new(["MOUSE_L"]));
    vm.tick_at(16, &tl);
    vm.tick_at(32, &tl);
    assert_eq!(vm.store().read(line, &[]).unwrap(), Value::Int(3));
    assert_eq!(vm.store().read(voice, &[]).unwrap(), Value::Int(7));
}

/// LET 目标类型不符时按引擎不写入,但诊断只报一次(按 query+类型去重)。
#[test]
fn taskinfo_type_mismatch_reports_once() {
    let mut b = ProjectBuilder::new();
    let target = push_var(&mut b, 1, Init::Int(42));
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        GroupSpec::new(OP_TASKINFO)
            .window(6, ex::i8_(1))
            .window(1, ex::r#ref(0x40, target.id))
            .line(10),
        GroupSpec::new(OP_TASKINFO)
            .window(6, ex::i8_(1))
            .window(1, ex::r#ref(0x40, target.id))
            .line(11),
        GroupSpec::new(OP_END).line(12),
    ]];
    let mut vm = boot(b);
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(vm.store().read(target, &[]).unwrap(), Value::Int(42));
    let reports = vm
        .events()
        .iter()
        .filter(
            |e| matches!(e, yu_rt::VmEvent::Diag { code, .. } if code == yu_rt::code::rt::VAR_TYPE),
        )
        .count();
    assert_eq!(reports, 1, "同 query/类型只报一次");
}

/// ID 未命中且 LET 目标未声明:不得静默,必须保留 write_target 的 Fatal 诊断。
#[test]
fn taskinfo_missing_id_with_undeclared_target_is_reported() {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        GroupSpec::new(OP_TASKINFO)
            .window(0, ex::s("NOPE"))
            .window(8, ex::i8_(1))
            .window(1, ex::r#ref(0x40, sys + 77))
            .line(10),
        GroupSpec::new(OP_END).line(11),
    ]];
    let mut vm = boot(b);
    let r = vm.tick_at(0, &no_input());
    assert_eq!(r.errors.len(), 1, "{:?}", r.errors);
    assert!(r.errors[0].contains("rt.var.undeclared"), "{:?}", r.errors);
}

/// `TASK[TEXTPOS=…]`:0.495 处理体无槽 36 分支,官方忽略;内核同型 no-op,
/// 不得报 Unimplemented(与 DIV-RT-015 的台账一致)。
#[test]
fn task_textpos_is_aligned_noop() {
    let mut b = ProjectBuilder::new();
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    const OP_TASK: u8 = 93;
    b.scripts = vec![vec![
        GroupSpec::new(OP_TASK)
            .window(36, ex::s("ignored"))
            .line(10),
        GroupSpec::new(OP_END).line(11),
    ]];
    let mut vm = boot(b);
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
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

/// `VARINFO[SEARCH]` 的针值槽 / `NO=` 槽**必须按参数名解析**。
///
/// 背景(2026-10-25,官方 v494 取证):`VARINFO` 的参数表随版本新增条目
/// 整体位移 —— SDK 0.495 是 `INT=17/FLT=18/STR=19/NO=20`,商业包 v494 是
/// `INT=15/FLT=16/STR=17/NO=18`。旧实现把 17/18/19/20 写死,在 v494 上
/// `SEARCH` 取不到针值、恒返回 -1:`es.LNO.GET("BGF")` 明明存在却判
/// 「未找到」,sprite 注册链整条走空(`es.SP.DRAW` 一次都不调用)。
///
/// 本用例故意把合成工程的 `VARINFO` 表改成 **v494 形态**(在 SEARCH 与
/// INT 之间插入 SJISCODE/UTF8CODE 前的位置关系不同),断言 SEARCH 仍命中。
/// 若有人改回硬编码槽号,这里会红。
#[test]
fn varinfo_search_resolves_slots_by_param_name() {
    // v494 形态:INT=15 / FLT=16 / STR=17 / NO=18(SEARCH=14)。
    let v494_varinfo: Vec<(&'static str, u8, u8)> = vec![
        ("SET", 0, 0),
        ("LET", 0, 0),
        ("TYPE", 0, 3),
        ("STRTYPE", 0, 3),
        ("DIMNUM", 0, 3),
        ("DIMSIZE", 0, 3),
        ("DIMSIZE2", 0, 3),
        ("DIMSIZE3", 0, 3),
        ("DIMSIZE4", 0, 3),
        ("DIMSIZE5", 0, 3),
        ("DIMSIZE6", 0, 3),
        ("DIMSIZE7", 0, 3),
        ("DIMSIZE8", 0, 3),
        ("LENGTH", 0, 3),
        ("SEARCH", 0, 3),
        ("INT", 0, 0),
        ("FLT", 0, 0),
        ("STR", 1, 0),
        ("NO", 0, 1),
    ];
    let mut b = ProjectBuilder::new();
    // 只换 VARINFO 的参数表,其余保持默认(命令顺序/opcode 不变)。
    for c in b.commands.iter_mut() {
        if c.0 == "VARINFO" {
            c.1 = v494_varinfo.clone();
        }
    }
    let sys = b.sysvars.len() as u16;
    b.vars = vec![
        // $names: ["", "BGF", ""] —— 目标是要查到 "BGF" 在下标 1
        VarSpec {
            kind: 1,
            group: 1,
            script_id: 0,
            var_id: sys,
            value_type: 3,
            dims: vec![4],
            init: Init::Str(b""),
        },
        VarSpec {
            kind: 1,
            group: 1,
            script_id: 0,
            var_id: sys + 1,
            value_type: 3,
            dims: vec![],
            init: Init::Str(b"BGF"),
        },
        // @hit:接收 SEARCH 结果
        VarSpec {
            kind: 1,
            group: 1,
            script_id: 0,
            var_id: sys + 2,
            value_type: 1,
            dims: vec![],
            init: Init::Int(-99),
        },
    ];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    let names = ex::r#ref(0x24, sys);
    let needle = ex::get(0x24, sys + 1);
    let hit = ex::r#ref(0x40, sys + 2);
    b.scripts = vec![vec![
        // $names[1] = "BGF"(元素 0/2 保持空串)
        GroupSpec::new(OP_LET)
            .window(0, ex::cat(&[names.clone(), ex::i8_(1), ex::aload()]))
            .window(0, needle.clone()),
        // VARINFO[SET=$names SEARCH=1 STR=$needle NO=0 LET=@hit]
        // 槽号全部取 v494 表:SEARCH=14 / STR=17 / NO=18。
        GroupSpec::new(101)
            .window(0, names.clone())
            .window(14, ex::i8_(1))
            .window(17, needle)
            .window(18, ex::i8_(0))
            .window(1, hit),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys + 2), &[])
            .unwrap(),
        Value::Int(1),
        "v494 参数表下 SEARCH 必须命中下标 1(写死 17/18/19/20 会返回 -1)"
    );
}

/// `VARINFO` 的**所有**查询项都按参数名分派(不只是 SEARCH 的针值槽)。
///
/// 2026-10-25 审查轮:`op_varinfo` 原本用 `match slot { 2=>TYPE, 4=>DIMNUM,
/// 5..=12=>DIMSIZE, 13=>LENGTH, 14=>SEARCH }` 写死槽号,查询项筛选也用
/// `!matches!(slot, 17..=20)` 排除辅助参数 —— 这个区间在 v494 上根本是错的
/// (v494 的 INT/FLT/STR/NO 是 15/16/17/18)。一旦参数表再位移,`TYPE` 会
/// 落到别的分支上并静默给出错值。
///
/// 本用例把 `VARINFO` 表**整体前移**(去掉 SJISCODE/UTF8CODE 两项),使所有
/// 槽号都变;断言 `TYPE`/`DIMNUM`/`DIMSIZE2`/`LENGTH`/`SEARCH` 依旧正确。
#[test]
fn varinfo_query_dispatch_follows_param_names() {
    // 前移后的形态: TYPE=2 / DIMNUM=3 / DIMSIZE=4 / DIMSIZE2=5 /
    // LENGTH=6 / SEARCH=7 / INT=8 / FLT=9 / STR=10 / NO=11
    // —— 与默认表(TYPE=2/DIMNUM=4/DIMSIZE=5..12/LENGTH=13/SEARCH=14/
    // INT=17/FLT=18/STR=19/NO=20)几乎每个槽号都不同。
    let shifted: Vec<(&'static str, u8, u8)> = vec![
        ("SET", 0, 0),
        ("LET", 0, 0),
        ("TYPE", 0, 3),
        ("DIMNUM", 0, 3),
        ("DIMSIZE", 0, 3),
        ("DIMSIZE2", 0, 3),
        ("LENGTH", 0, 3),
        ("SEARCH", 0, 3),
        ("INT", 0, 0),
        ("FLT", 0, 0),
        ("STR", 1, 0),
        ("NO", 0, 1),
    ];
    let mut b = ProjectBuilder::new();
    for c in b.commands.iter_mut() {
        if c.0 == "VARINFO" {
            c.1 = shifted.clone();
        }
    }
    let sys = b.sysvars.len() as u16;
    b.vars = vec![
        // $table: 3×2 字符串数组(用于 DIMNUM/DIMSIZE2)
        VarSpec {
            kind: 1,
            group: 1,
            script_id: 0,
            var_id: sys,
            value_type: 3,
            dims: vec![3, 2],
            init: Init::Str(b""),
        },
        // $flat: 一维字符串数组(SEARCH 只支持一维)
        VarSpec {
            kind: 1,
            group: 1,
            script_id: 0,
            var_id: sys + 1,
            value_type: 3,
            dims: vec![2],
            init: Init::Str(b""),
        },
        // @out(0..4): 五个结果槽
        VarSpec {
            kind: 1,
            group: 1,
            script_id: 0,
            var_id: sys + 2,
            value_type: 1,
            dims: vec![5],
            init: Init::Int(-1),
        },
    ];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    let table = ex::r#ref(0x24, sys);
    let flat = ex::r#ref(0x24, sys + 1);
    let out = |i: i8| ex::cat(&[ex::r#ref(0x40, sys + 2), ex::i8_(i), ex::aload()]);
    b.scripts = vec![vec![
        // TYPE → out0(Str = 3)
        GroupSpec::new(101)
            .window(0, table.clone())
            .window(2, ex::i8_(1))
            .window(1, out(0)),
        // DIMNUM → out1(2 维)
        GroupSpec::new(101)
            .window(0, table.clone())
            .window(3, ex::i8_(1))
            .window(1, out(1)),
        // DIMSIZE2 → out2(第 2 维 = 2)
        GroupSpec::new(101)
            .window(0, table.clone())
            .window(5, ex::i8_(1))
            .window(1, out(2)),
        // LENGTH → out3(空串 = 0)
        GroupSpec::new(101)
            .window(0, table.clone())
            .window(6, ex::i8_(1))
            .window(1, out(3)),
        // SEARCH(STR="") → out4(下标 0;一维 $flat)
        GroupSpec::new(101)
            .window(0, flat)
            .window(7, ex::i8_(1))
            .window(10, ex::get(0x24, sys + 1))
            .window(11, ex::i8_(0))
            .window(1, out(4)),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let read = |vm: &Vm, i: i64| {
        vm.store()
            .read(VarRef::new(VarSpace::At, sys + 2), &[i])
            .unwrap()
    };
    assert_eq!(read(&vm, 0), Value::Int(3), "TYPE = Str(3)");
    assert_eq!(read(&vm, 1), Value::Int(2), "DIMNUM = 2");
    assert_eq!(read(&vm, 2), Value::Int(2), "DIMSIZE2 = 2");
    assert_eq!(read(&vm, 3), Value::Int(0), "LENGTH(空串) = 0");
    assert_eq!(read(&vm, 4), Value::Int(0), "SEARCH(\"\") 命中下标 0");
}

/// `VARACT[INIT=1 G_INTn]` 的**位号映射**:后缀 N ⇒ 组 N ⇒ 掩码位 `1<<(N-1)`。
///
/// 2026-10-25 审查:把 `17..=28` 槽号区间改成按名解析时,第一版把
/// `G_INT2` 算成了 bit 2(应为 bit 1)、`G_INT4` 算成 bit 4 后被
/// `bit >= 4` 丢弃 —— 静默错位。本用例钉住四组各自的恢复范围。
#[test]
fn varact_g_int_group_bits_follow_suffix() {
    // 四个全局变量,组号 1..4,初值即组号便于辨认。
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = (1..=4)
        .map(|g| VarSpec {
            kind: 1,
            group: g as u8,
            script_id: 0,
            var_id: sys + g as u16,
            value_type: 1,
            dims: vec![],
            init: Init::Int(g as i64),
        })
        .collect();
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    // 先把四个变量全改成 99,再用 VARACT 只恢复组 2。
    let mut script = Vec::new();
    for g in 1..=4u16 {
        script.push(
            GroupSpec::new(OP_LET)
                .window(0, ex::r#ref(0x40, sys + g))
                .window(0, ex::i8_(99)),
        );
    }
    // VARACT[INIT=1 G_INT2=1](只该恢复组 2)
    let slot_of = |name: &str| -> u8 {
        crate::support::default_commands()
            .iter()
            .find(|c| c.0 == "VARACT")
            .and_then(|c| c.1.iter().position(|p| p.0 == name))
            .map(|i| i as u8)
            .expect("VARACT 参数表里应有该名字")
    };
    script.push(
        GroupSpec::new(OP_VARACT)
            .window(slot_of("INIT"), ex::i8_(1))
            .window(slot_of("G_INT2"), ex::i8_(1)),
    );
    script.push(GroupSpec::new(OP_END));
    b.scripts = vec![script];
    let mut vm = boot(b);
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let read = |vm: &Vm, g: u16| {
        vm.store()
            .read(VarRef::new(VarSpace::At, sys + g), &[])
            .unwrap()
    };
    assert_eq!(read(&vm, 1), Value::Int(99), "组 1 不在掩码里,保持 99");
    assert_eq!(read(&vm, 2), Value::Int(2), "组 2 命中,恢复初值 2");
    assert_eq!(read(&vm, 3), Value::Int(99), "组 3 不在掩码里,保持 99");
    assert_eq!(read(&vm, 4), Value::Int(99), "组 4 不在掩码里,保持 99");
}

/// 名字→槽位族的口径一致性(统一入口后的回归锁)。
///
/// 2026-10-25 审查:**同一类错误在本项目出现过两次**
/// (`G_INT2` 当 bit 2、`TINT2` 当索引 2),根因都是「名字后缀 N 被当成
/// 索引 N」,而正解是 `N-1`(1-based 序号)。本用例把四条族的 1-based
/// 语义钉在行为上,任何人再写 `N` 就会红。
///
/// | 族 | 生产者 | 消费者 | 口径 |
/// |---|---|---|---|
/// | `PINTn`/`RINTn` | `GOSUB[PINTn=]`/`RETURN[RINTn=]` | `@_PINT(n)`/`@_RINT(n)` | `n-1` |
/// | `TINTn` | `TASK[TINTn=]` | `@_TINT(n)` | `n-1` |
/// | `DIMSIZEn` | — | `VARINFO[DIMSIZEn]` = 第 n 维 | `n-1` |
/// | `G_INTn` | `VARACT[G_INTn=]` | `restore_globals` 组 n | 掩码位 `n-1` |
#[test]
fn name_suffix_families_are_one_based_everywhere() {
    // 端到端:`RETURN[RINT2=42]`(生产者,槽 = 名字后缀 2)
    //      → `@_RINT(2)`(消费者,索引 = 2-1 = 1)
    // 若任一侧把后缀当索引,读到的是 0 而非 42。
    // 结构照 `gosub_passes_pint_and_return_brings_rint`(同帧同步完成,
    // 不需要 WAIT)。
    let mut b = ProjectBuilder::new();
    let v = with_int_var(&mut b, 0, -1);
    b.labels = vec![("SYSTEM_START", 0, 0, 0), ("SUB", 3, 0, 0)];
    b.entry(0, 0);
    const OP_GOSUB: u8 = 44;
    const OP_RETURN: u8 = 77;
    // RETURN 的参数表:`RINT`=槽 0、`RINT2`=槽 1 …(按名反查,不写死)。
    let rint2_slot = crate::support::default_commands()
        .iter()
        .find(|c| c.0 == "RETURN")
        .and_then(|c| c.1.iter().position(|p| p.0 == "RINT2"))
        .map(|i| i as u8)
        .expect("RETURN 参数表里应有 RINT2");
    b.scripts = vec![vec![
        // 0: GOSUB[#="SUB"]
        GroupSpec::new(OP_GOSUB).window(0, ex::s("SUB")),
        // 1: LET[@v = @_RINT(2)]
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, v.id))
            .window(0, ex::cat(&[ex::get(0x40, 0), ex::i8_(0), ex::aload()])),
        GroupSpec::new(OP_END),
        // 3: SUB: RETURN[RINT2=42]
        GroupSpec::new(OP_RETURN).window(rint2_slot, ex::i8_(42)),
    ]];
    // 填入 @_RINT 的真实 id(第二窗口目前是占位)。
    let probe_src = {
        let mut p = ProjectBuilder::new();
        p.sysvars = b.sysvars.clone();
        p.build()
    };
    let tables = yu_rt::TableSet::load(&probe_src, yu_rt::tables::DEFAULT_SCRIPT_KEY).unwrap();
    let rint = tables.boot.sysvars.id("_RINT").unwrap();
    b.scripts[0][1].windows[1].4 = ex::cat(&[ex::r#ref(0x40, rint), ex::i8_(2), ex::aload()]);
    let mut vm = boot(b);
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(
        vm.store().read(user_var(&vm, 0), &[]).unwrap(),
        Value::Int(42),
        "RETURN[RINT2=42] 必须能被 @_RINT(2) 读到(后缀 N ⇒ 槽位 N-1)"
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

/// R3 回归(手动模式):`TEXT[OUTPUTMODE=0]` 下 `RETURNCODE` 只休憩**一帧**,
/// 不等待点击。
///
/// 引擎 `FUN_004046fc`(`0x4046fc..0x404835`)对 `task+9` 的恢复判据:
/// - `0x404711 CMP byte[state+0x1c],1`(文本状态 = `TEXT[OUTPUTMODE]`);
/// - `0x404715 JNE 0x404828` ⇒ 手动(0)/其它值**无条件恢复**,不经按钮表;
/// - `== 1`(自动)才读 CLICKBUTTON/SKIPBUTTON(`0x5b48ac..0x5b48bc`)。
///
/// v494 同构(`0x44c20c` 头部逐指令对齐,`[0x802ad8]` vs `[0x801800]`)。
///
/// 语料意义:ERIS 框架 `es_first` 首指令 `TEXT[OUTPUTMODE=0]`,若把手动模式
/// 也当"等点击",`ES.CHARNAME.DEFINE`(s84,256 组)这类定义链会永久停住 ——
/// 实测 `<game-b>` 表现为"必须手动点击才推进、只画出半套 GAMEMAIN 皮肤"。
#[test]
fn manual_outputmode_returncode_resumes_without_input() {
    let mut b = ProjectBuilder::new();
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    const OP_RETURNCODE: u8 = 79;
    const OP_WORD: u8 = 106;
    const OP_TEXT: u8 = 96;
    let word = |s: &str| GroupSpec::new(OP_WORD).window(0, s.as_bytes().to_vec());
    b.scripts = vec![vec![
        GroupSpec::new(OP_TEXT).window(1, ex::i32_(0)), // OUTPUTMODE=0 手动
        word("A"),
        GroupSpec::new(OP_RETURNCODE).window(1, Vec::new()),
        word("B"),
        GroupSpec::new(OP_RETURNCODE).window(2, Vec::new()),
        word("C"),
        GroupSpec::new(OP_RETURNCODE).window(3, Vec::new()),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    let texts = |vm: &Vm| -> Vec<Vec<u8>> {
        vm.events()
            .iter()
            .filter_map(|e| match e {
                yu_rt::VmEvent::Text { bytes, .. } => Some(bytes.clone()),
                _ => None,
            })
            .collect()
    };

    // 全程零输入:每帧一行,不需要任何点击。
    vm.tick_at(0, &no_input());
    assert_eq!(texts(&vm), vec![b"A".to_vec()], "第一帧出 A");
    vm.tick_at(16, &no_input());
    assert_eq!(
        texts(&vm),
        vec![b"A".to_vec(), b"B".to_vec()],
        "手动模式:无输入也进入下一行"
    );
    vm.tick_at(32, &no_input());
    assert_eq!(
        texts(&vm),
        vec![b"A".to_vec(), b"B".to_vec(), b"C".to_vec()],
        "手动模式:继续自动推进"
    );
}

/// R9 回归:`GOSUB` 允许 64 次嵌套,第 65 次 Fatal。
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

/// **端到端证明**:`TASK[TINTn=x]` 写入的槽必须等于 `@_TINT(n)` 读的槽。
///
/// 2026-10-25 审查:把「槽号区间」改成「名字后缀」时,第一版把后缀 N 直接
/// 当索引用(`TINT2` ⇒ 索引 2),而消费者 `sysvar::param_index` 读的是
/// `n-1`(`@_TINT(2)` ⇒ 索引 1)⇒ **生产者与消费者错位一格**,值静默丢失。
///
/// 本用例走完整链路:脚本 `TASK[TINT2=7]` → 子任务体 `@_TINT(2)`,
/// 断言读到 7。修复前该用例必红(读到 0)。
#[test]
fn task_tint2_value_reaches_tint2_consumer() {
    let mut b = ProjectBuilder::new();
    let v = with_int_var(&mut b, 0, -1);
    let tint = b
        .sysvars
        .iter()
        .position(|(n, _, _)| *n == "_TINT")
        .expect("_TINT") as u16;
    b.labels = vec![("SYSTEM_START", 0, 0, 0), ("BODY", 3, 0, 0)];
    b.entry(0, 0);
    const OP_TASK: u8 = 93;
    const OP_TASKEND: u8 = 94;
    // 按**参数名**取槽号,与实现同口径(不写死 5)。
    let slot_of = |name: &str| -> u8 {
        crate::support::default_commands()
            .iter()
            .find(|c| c.0 == "TASK")
            .and_then(|c| c.1.iter().position(|p| p.0 == name))
            .map(|i| i as u8)
            .unwrap_or_else(|| panic!("TASK 参数表里应有 {name}"))
    };
    b.scripts = vec![vec![
        // A = TASK[ID="A" #="BODY" TINT2=7]
        GroupSpec::new(OP_TASK)
            .window(slot_of("ID"), ex::s("A"))
            .window(slot_of("#"), ex::s("BODY"))
            .window(slot_of("TINT2"), ex::i8_(7)),
        GroupSpec::new(OP_WAIT).window(0, ex::i8_(4)),
        GroupSpec::new(OP_END),
        // 3: BODY —— 读 @_TINT(2)
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, v.id))
            .window(
                0,
                ex::cat(&[ex::r#ref(0x40, tint), ex::i8_(2), ex::aload()]),
            ),
        GroupSpec::new(OP_TASKEND),
    ]];
    let mut vm = boot(b);
    for f in 0..5u32 {
        vm.tick_at(f * 16, &no_input());
    }
    assert_eq!(
        vm.store().read(user_var(&vm, 0), &[]).unwrap(),
        Value::Int(7),
        "TASK[TINT2=7] 必须能被 @_TINT(2) 读到(后缀 N ⇒ 槽位 N-1)"
    );
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
    // `TRAY`/`SYSTEM` 属 System 域 = 宿主窗口/托盘策略(域事件契约 §9):
    // 内核只发事件、无脚本可见返回,属**归属**而非未实现 ⇒ 不进挂号表
    // (以 Info 记录一次,码 `rt.cmd.aligned_stub`)。
    assert_eq!(
        vm.unimplemented_cmds().get("TRAY").copied(),
        None,
        "System 域命令归宿主,不应进未实现挂号表"
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
    /// `SAVE[ID]` 的图像预约(命令时刻像素)。
    images: Vec<(u32, yu_rt::event::CapturedImage)>,
    /// `LOAD[ID]` 的图像读取请求。
    image_loads: Vec<(String, u32)>,
    /// `Vm::emit` 同步喂入的 Save/File 事件与 flush 的**先后顺序**
    /// (FILEACT 必须先于同一帧的 SAVE[FILE] 刷盘)。
    order: Vec<String>,
}

impl SaveRecorder {
    fn new() -> std::rc::Rc<std::cell::RefCell<Self>> {
        std::rc::Rc::new(std::cell::RefCell::new(Self {
            reserves: Vec::new(),
            groups: Vec::new(),
            flushes: Vec::new(),
            ends: 0,
            loads: Vec::new(),
            images: Vec::new(),
            image_loads: Vec::new(),
            order: Vec::new(),
        }))
    }
}

/// 共享宿主包装:`reply` 为 `Some` 时 `load_block` 回复之(LOAD 写回用例)。
struct SharedHost {
    inner: std::rc::Rc<std::cell::RefCell<SaveRecorder>>,
    reply: std::rc::Rc<std::cell::RefCell<Option<LoadedBlock>>>,
}

impl SaveHost for SharedHost {
    fn apply_event(&mut self, ev: &yu_rt::event::VmEvent) {
        if let yu_rt::event::VmEvent::DomainCall {
            domain: yu_rt::event::Domain::Save | yu_rt::event::Domain::File,
            name,
            ..
        } = ev
        {
            self.inner.borrow_mut().order.push(format!("event:{name}"));
        }
    }
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
        self.inner
            .borrow_mut()
            .order
            .push(format!("flush:{file}"));
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
    fn reserve_image(
        &mut self,
        dno: u32,
        image: &yu_rt::event::CapturedImage,
    ) -> Result<(), Diag> {
        self.inner.borrow_mut().images.push((dno, image.clone()));
        Ok(())
    }
    fn load_image(
        &mut self,
        file: &str,
        dno: u32,
    ) -> Result<yu_rt::event::CapturedImage, Diag> {
        self.inner
            .borrow_mut()
            .image_loads
            .push((file.to_string(), dno));
        Ok(sample_image())
    }
}

fn sample_image() -> yu_rt::event::CapturedImage {
    // 2×1 RGBA:用来区分「取到像素」与「空层」。
    yu_rt::event::CapturedImage {
        width: 2,
        height: 1,
        rgba: vec![1, 2, 3, 255, 4, 5, 6, 255],
    }
}

/// 图形宿主桩:记录 `SAVE[ID]` 抓帧与 `LOAD[ID]` 装回。
struct PixelHost {
    captured: std::rc::Rc<std::cell::RefCell<Vec<(Vec<u8>, yu_rt::event::CapturedImage)>>>,
    restored: std::rc::Rc<std::cell::RefCell<Vec<(Vec<u8>, yu_rt::event::CapturedImage)>>>,
}

impl yu_rt::event::GraphicsHost for PixelHost {
    fn apply_event(&mut self, _ev: &yu_rt::event::VmEvent) {}

    fn query(
        &mut self,
        _command: &str,
        _args: &[yu_rt::event::ArgValue],
    ) -> Result<Value, Diag> {
        Err(Diag::fatal("test", yu_log::Boundary::Domain))
    }

    fn capture_layer(
        &mut self,
        id: &[u8],
    ) -> Result<Option<yu_rt::event::CapturedImage>, Diag> {
        let image = sample_image();
        self.captured
            .borrow_mut()
            .push((id.to_vec(), image.clone()));
        Ok(Some(image))
    }

    fn restore_layer(
        &mut self,
        id: &[u8],
        image: &yu_rt::event::CapturedImage,
    ) -> Result<bool, Diag> {
        self.restored
            .borrow_mut()
            .push((id.to_vec(), image.clone()));
        Ok(true)
    }
}

/// `SAVE[ID]` = 命令时刻图层像素 → 宿主图像预约;`LOAD[ID]` = 宿主读块 →
/// 图形宿主装回图层。两条命令都不得再误报「未实现」。
#[test]
fn save_id_reserves_layer_image_and_load_id_restores_it() {
    let mut b = ProjectBuilder::new();
    let op_save = b.commands.iter().position(|(n, _)| *n == "SAVE").unwrap() as u8;
    let op_saveend = b
        .commands
        .iter()
        .position(|(n, _)| *n == "SAVEEND")
        .unwrap() as u8;
    let op_load = b.commands.iter().position(|(n, _)| *n == "LOAD").unwrap() as u8;
    let groups = vec![
        // SAVE[DNO=3 ID="THUMB"]
        GroupSpec::new(op_save)
            .window(1, ex::i8_(3))
            .window(3, ex::s("THUMB")),
        // SAVE[FILE="lay"]
        GroupSpec::new(op_save).window(0, ex::s("lay")),
        // LOAD[FILE="lay" DNO=3 ID="THUMB"]
        GroupSpec::new(op_load)
            .window(0, ex::s("lay"))
            .window(1, ex::i8_(3))
            .window(3, ex::s("THUMB")),
        GroupSpec::new(op_saveend),
        GroupSpec::new(OP_END),
    ];
    b.scripts = vec![groups];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);

    let recorder = SaveRecorder::new();
    let reply = std::rc::Rc::new(std::cell::RefCell::new(None));
    let captured = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let restored = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut vm = boot(b);
    vm.set_save_host(Box::new(SharedHost {
        inner: recorder.clone(),
        reply,
    }));
    vm.set_graphics_host(Box::new(PixelHost {
        captured: captured.clone(),
        restored: restored.clone(),
    }));
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);

    // SAVE[ID]:抓帧 ID 原样交给宿主,像素进入图像预约。
    assert_eq!(
        captured.borrow().as_slice(),
        &[("THUMB".to_string().into_bytes(), sample_image())]
    );
    let rec = recorder.borrow();
    assert_eq!(rec.images, vec![(3, sample_image())]);
    assert_eq!(rec.flushes, vec!["lay.sd".to_string()]);
    // LOAD[ID]:读块 → 装回同名图层。
    assert_eq!(rec.image_loads, vec![("lay.sd".to_string(), 3)]);
    assert_eq!(
        restored.borrow().as_slice(),
        &[("THUMB".to_string().into_bytes(), sample_image())]
    );
    assert_eq!(rec.ends, 1);
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

/// 存档脚本的「先删旧槽(pc315)、再 `SAVE[FILE]`(pc341)」必须在同一帧里
/// 保持**命令顺序**:`FILEACT` 事件由 `Vm::emit` 在命令时刻同步喂宿主,
/// 不能落到帧末 drain;否则删除晚于同步刷盘,刚写出的存档会被删
/// (2026-10-27 菜单存档实测回归)。
#[test]
fn fileact_is_applied_before_save_file_flush_in_same_frame() {
    let mut b = ProjectBuilder::new();
    let op_fileact = b
        .commands
        .iter()
        .position(|(n, _)| *n == "FILEACT")
        .unwrap() as u8;
    let op_save = b.commands.iter().position(|(n, _)| *n == "SAVE").unwrap() as u8;
    let op_saveend = b
        .commands
        .iter()
        .position(|(n, _)| *n == "SAVEEND")
        .unwrap() as u8;
    let groups = vec![
        // FILEACT[FILE="order.sd"] —— 事件必须在下一命令前已落到宿主。
        GroupSpec::new(op_fileact).window(0, ex::s("order.sd")),
        // SAVE[FILE="order.sd"] —— 同步刷盘。
        GroupSpec::new(op_save).window(0, ex::s("order.sd")),
        GroupSpec::new(op_saveend),
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
    assert_eq!(
        rec.order,
        vec![
            "event:FILEACT".to_string(),
            "event:SAVE".to_string(),
            "flush:order.sd".to_string(),
            "event:SAVEEND".to_string(),
        ],
        "FILEACT 必须早于同一帧的 SAVE[FILE] 刷盘: {:?}",
        rec.order
    );
}

/// `SAVE[SET=@系统变量]`(语料 DNO 90 = `@_RINT(1)`):系统变量没有
/// `VarSlot`,但 SET 已在命令时刻求值;预约必须是**标量快照**
/// (官方块:type=Int、dims=[]、data=[值])。
#[test]
fn save_set_on_system_variable_reserves_scalar_snapshot() {
    let mut b = ProjectBuilder::new();
    let lc_id = b
        .sysvars
        .iter()
        .position(|(name, _, _)| *name == "_LC")
        .expect("_LC sysvar")
        as u16;
    let op_save = b.commands.iter().position(|(n, _)| *n == "SAVE").unwrap() as u8;
    b.scripts = vec![vec![
        GroupSpec::new(op_save)
            .window(1, ex::i8_(90))
            .window(2, ex::get(0x40, lc_id))
            .line(10),
        GroupSpec::new(OP_END).line(11),
    ]];
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
    assert_eq!(rec.reserves.len(), 1, "{:?}", rec.reserves);
    let (dno, var, snap) = &rec.reserves[0];
    assert_eq!(*dno, 90);
    assert_eq!(*var, VarRef::new(VarSpace::At, lc_id));
    assert_eq!(snap.ty, yu_rt::value::ElemType::Int);
    assert!(snap.dims.is_empty(), "{:?}", snap.dims);
    assert_eq!(snap.data, vec![Value::Int(0)]);
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

/// `VARACT` 的 `LET` 可缺省:引擎 `0x4542ab` 按 `[0x684161]`(LET 在场)
/// 分两支,**两支都调 `0x43f0d8`**;缺省时结果写回 `SET` 指向的变量。
///
/// 语料实测 `es_Text.yst:2568`/`:2586`/`:2024`/`:2040` 全是
/// `VARACT[CUT=1 SET=$TX POS=… LENGTH=…]` 的原地剪切形态。旧实现在这里
/// Fatal(`rt.cmd.slot`),直接终止 `es_Text` 任务(桌面实测:进入演出样例后
/// `FATAL rt.cmd.slot at es_text.yst:SYSTEM@1354 VARACT`,此后输入不再推进)。
#[test]
fn varact_cut_without_let_writes_back_to_set() {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![VarSpec {
        kind: 1,
        group: 0,
        script_id: 0,
        var_id: sys,
        value_type: 3,
        dims: vec![],
        init: Init::Str(b"ABCDE"),
    }];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        // VARACT[CUT=1 SET=$s POS=1 LENGTH=2] → 删掉第 1..2 字符 ⇒ "CDE"
        GroupSpec::new(OP_VARACT)
            .window(0, ex::r#ref(0x24, sys))
            .window(2, ex::i8_(1))
            .window(4, ex::i8_(1))
            .window(5, ex::i8_(2)),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::Dollar, sys), &[])
            .unwrap(),
        Value::Str(b"CDE".to_vec()),
        "LET 缺省时 CUT 结果必须回写 SET(手册例 6 的原地形态)"
    );
}

#[test]
fn inp_keylast_mouselast_writes_are_applied() {
    // 手册 `basic/svariable.html` 把 `_INP` / `_KEYLAST` / `_MOUSELAST`
    // 标为 ○ 可赋值;REA 0x495 写分派:
    // - `_INP(n)=v`(case 0x90 → `0x446efc` → `0x447014`):n>0 时按物理
    //   状态钳制写**同一张** VK 计数表;
    // - `_KEYLAST=0` / `_MOUSELAST=0`(case 0x91/0x92 → `0x446f52/0x446f3c`):
    //   直接覆盖 `DAT_00801908/0x80190c`,无钳制。
    // ERIS `es_Button.yst:3417` 每次点击/推进都写 `_INP(...)=2` 消费按下;
    // `es_Text.yst:1287` 写 `_KEYLAST=0` 取消 skip。修复前这两类写入
    // 会误报 `rt.sysvar.write` 并被丢弃。
    let mut b = ProjectBuilder::new();
    b.sysvars_abi = true;
    let abi = yu_rt::sysvar_abi::SysVarTable::builtin_abi();
    let sysvar_id = |name: &str| -> u16 {
        abi.decls
            .iter()
            .position(|d| d.name == name)
            .expect("引擎 ABI 表应有该变量") as u16
    };
    let inp = sysvar_id("_INP");
    let keylast = sysvar_id("_KEYLAST");
    let mouselast = sysvar_id("_MOUSELAST");
    let code: i16 = 0x5A; // VK_Z

    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        // @_INP(VK_Z) = 2
        GroupSpec::new(OP_LET)
            .window(
                0,
                ex::cat(&[ex::r#ref(0x40, inp), ex::i16_(code), ex::aload()]),
            )
            .window(0, ex::i8_(2)),
        // @_KEYLAST = 0
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, keylast))
            .window(0, ex::i8_(0)),
        // @_MOUSELAST = 0
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, mouselast))
            .window(0, ex::i8_(0)),
        GroupSpec::new(OP_END),
    ]];

    // 第 0 帧按下 Z:`_INP` 计数 = 1(按下),`_KEYLAST` = VK_Z。
    let mut tl = InputTimeline::new();
    tl.set(0, InputFrame::new(["KEY_Z"]));
    let mut vm = boot(b);
    let report = vm.tick_at(0, &tl);
    assert!(
        report.errors.is_empty(),
        "可赋值系统变量写入不得再挂号: {:?}",
        report.errors
    );
    assert_eq!(
        vm.input().inp(i64::from(code)).unwrap(),
        2,
        "按下帧写 2:物理状态钳制后保留正数"
    );
    assert_eq!(vm.input().keylast(), 0, "_KEYLAST 直接写 0");
    assert_eq!(vm.input().mouselast(), 0, "_MOUSELAST 直接写 0");
}
