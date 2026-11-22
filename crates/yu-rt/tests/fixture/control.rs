//! 控制流/任务让出:IF/LOOP/GOSUB/WAIT/FPS/END。
//!
//! 由 `tests/fixture.rs` 拆分而来;测试名与断言不变。

use super::common::*;

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
fn empty_go_label_is_noop_not_fatal() {
    // v474 样本的 `GO[#=$es.STARTLABEL]` 在 boot 期变量为空;官方能启动,
    // 故空目标必须是 no-op,不能按“标签不存在”Fatal。
    let mut b = ProjectBuilder::new();
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        GroupSpec::new(OP_GO).window(0, ex::s("")),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(!vm
        .events()
        .iter()
        .any(|e| matches!(e, yu_rt::VmEvent::Diag { code, .. } if code == "rt.label")));
}

/// `END` 是**全程序退出**,不是只结束执行者(手册 `control/end.html`;
/// REA 0.495 `0x441334` 置 `[0x80186c]`,主循环 `0x403bfc` 停帧泵)。
///
/// 组布局:
/// ```text
/// 0  TASK[ID="SUB" #="SUB.BODY"]
/// 1  WAIT[FRAME=5]         子任务先跑
/// 2  LET[@v = 7]           不应执行(程序已退)
/// 3  END
/// 4  SUB.BODY: END[RESTART=0]
/// 5           LET[@v = 2]  不应执行
/// ```
#[test]
fn end_in_child_task_stops_program() {
    let mut b = ProjectBuilder::new();
    let v = with_int_var(&mut b, 0, 0);
    b.labels = vec![("SYSTEM_START", 0, 0, 0), ("SUB.BODY", 4, 0, 0)];
    b.entry(0, 0);
    const OP_TASK: u8 = 93;
    b.scripts = vec![vec![
        GroupSpec::new(OP_TASK)
            .window(0, ex::s("SUB"))
            .window(34, ex::s("SUB.BODY")),
        GroupSpec::new(OP_WAIT).window(0, ex::i8_(5)),
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, v.id))
            .window(0, ex::i8_(7)),
        GroupSpec::new(OP_END),
        // 4: SUB.BODY
        GroupSpec::new(OP_END).window(0, ex::i8_(0)),
        // 5: 不应执行
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, v.id))
            .window(0, ex::i8_(2)),
    ]];
    let mut vm = boot(b);
    let mut chain = None;
    for f in 0..8u32 {
        let r = vm.tick_at(f * 16, &no_input());
        if r.program_exit.is_some() {
            chain = Some(r);
            break;
        }
    }
    let chain = chain.expect("子任务 END 必须触发全程序退出");
    assert_eq!(
        chain.program_exit,
        Some(yu_rt::task::ProgramExit { restart: false }),
        "子任务 END[RESTART=0] 触发全程序退出"
    );
    assert!(chain.main_finished, "全程序退出同时满足宿主 exit 判据");
    assert_eq!(
        vm.store().read(user_var(&vm, 0), &[]).unwrap(),
        Value::Int(0),
        "END 后主任务/子任务后续 LET 都不再执行"
    );
    assert!(vm.scheduler().find_by_id("SUB").is_none(), "子任务已终止");
    assert!(
        vm.scheduler().find_by_id(MAIN_TASK_ID).is_none(),
        "主任务也随全程序退出终止"
    );
    // 退出后的 tick 不得再泵任何命令(官方主循环不再跑帧泵)。
    let after = vm.tick_at(8 * 16, &no_input());
    assert_eq!(after.commands_pumped, 0, "退出后不再执行命令");
    assert_eq!(after.logical_frames, 1, "退出后 flow 仍可 tick,但只有空帧");
}

/// 官方 `mov [0x80152c], al` 只保存 `RESTART` 的**低 8 位**(REA 0x441358);
/// 消费侧按字节判零。`256` 不得被当成 restart。
#[test]
fn end_restart_uses_low_byte() {
    for (value, expected) in [(0i32, false), (1, true), (256, false), (257, true)] {
        let mut b = ProjectBuilder::new();
        b.labels = vec![("SYSTEM_START", 0, 0, 0)];
        b.entry(0, 0);
        b.scripts = vec![vec![GroupSpec::new(OP_END).window(0, ex::i32_(value))]];
        let mut vm = boot(b);
        let chain = vm.tick_at(0, &no_input());
        assert_eq!(
            chain.program_exit,
            Some(yu_rt::task::ProgramExit { restart: expected }),
            "RESTART={value} 的低 8 位语义"
        );
        assert!(chain.main_finished);
    }
}

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


/// 手册 `control/gosub.html`:同编号的 `PINT1` / `PFLT1` / `PSTR1` 是
/// **三组独立参数**(分别落 `@_PINT(1)`/`@_PFLT(1)`/`$_PSTR(1)`),不是同一
/// 槽位的三种类型。v555 剧本链 `GOSUB[#=… PSTR="CSEL" PINT=1 …]` 依赖这一点;
/// 早期单一 `args[16]` 会让 PINT 覆盖 PSTR,命令名永远读到整数。
#[test]
#[ignore = "v555 other 模块: 共享 GosubFrame 已回退,三组用例随 other/gosub 迁入"]
fn gosub_keeps_pint_and_pstr_groups_apart() {
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
    b.labels = vec![("SYSTEM_START", 0, 0, 0), ("SUB", 3, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        // 0: GOSUB[#="SUB" PINT=21 PSTR="CMD"]
        GroupSpec::new(OP_GOSUB)
            .window(0, ex::s("SUB"))
            .window(1, ex::i8_(21))
            .window(33, ex::s("CMD")),
        GroupSpec::new(OP_END),
        GroupSpec::new(OP_END),
        // 3: SUB: LET[@a = @_PINT(1)]; LET[$b = $_PSTR(1)]; RETURN
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, sys))
            .window(1, ex::cat(&[ex::r#ref(0x40, 0), ex::i8_(1), ex::aload()])),
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x24, sys + 1))
            .window(1, ex::cat(&[ex::r#ref(0x24, 0), ex::i8_(1), ex::aload()])),
        GroupSpec::new(OP_RETURN),
    ]];
    let probe_src = {
        let mut p = ProjectBuilder::new();
        p.sysvars = b.sysvars.clone();
        p.build()
    };
    let tables = yu_rt::TableSet::load(&probe_src, yu_rt::tables::DEFAULT_SCRIPT_KEY).unwrap();
    let pint = tables.boot.sysvars.id("_PINT").unwrap();
    let pstr = tables.boot.sysvars.id("_PSTR").unwrap();
    // 被调方:@_PINT(1) / $_PSTR(1) 的数组取值(引用 + 下标 + aload)。
    b.scripts[0][3].windows[1].4 = ex::cat(&[
        ex::r#ref(0x40, pint),
        ex::i8_(1),
        ex::aload(),
    ]);
    b.scripts[0][4].windows[1].4 = ex::cat(&[
        ex::r#ref(0x24, pstr),
        ex::i8_(1),
        ex::aload(),
    ]);

    let mut vm = boot(b);
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys), &[])
            .unwrap(),
        Value::Int(21),
        "@_PINT(1) 保持 21"
    );
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::Dollar, sys + 1), &[])
            .unwrap(),
        Value::Str(b"CMD".to_vec()),
        "$_PSTR(1) 保持 \"CMD\"(不被同编号 PINT 覆盖)"
    );
}
