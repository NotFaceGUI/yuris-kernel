//! 系统域:SYSTEMINFO/INPUT/CLIP/REGINFO/DIALOG/LABELINFO。
//!
//! 由 `tests/fixture.rs` 拆分而来;测试名与断言不变。

use super::common::*;

/// v555 `_STR(slot<1000, index)`:官方 `FUN_004213bc` 的字符串系统变量
/// case(如 `_ARGV`/`_SYSTEM_PATH`)必须被 `_STR` 读到,而不是返回空串。
#[test]
#[ignore = "v555 other 模块: _STR 动态访问器已从共享 sysvar 移出,用例随 other/sysvar 迁入"]
fn str_dynamic_accessor_reads_system_string_vars_below_1000() {
    let mut b = ProjectBuilder::new();
    b.scripts = vec![vec![GroupSpec::new(OP_END)]];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    let cfg = yu_rt::BootConfig {
        argv: vec![b"alpha".to_vec()],
        paths: yu_rt::vm::HostPaths {
            system_path: Some(b"C:\\sys".to_vec()),
            ..Default::default()
        },
        ..Default::default()
    };
    let vm = yu_rt::Vm::boot(Box::new(b.build()), cfg).expect("boot");
    let st = yu_rt::vm::ExecState::new(0, 0);

    let str_id = vm.sysvars().id("_STR").expect("_STR id");
    let argv_id = vm.sysvars().id("_ARGV").expect("_ARGV id");

    // `_STR(_ARGV, 0)` = `_ARGV[0]`。
    let argv = yu_rt::sysvar::read(
        &vm,
        &st,
        VarRef::new(VarSpace::At, str_id),
        &[i64::from(argv_id), 0],
    )
    .expect("_STR(_ARGV,0)");
    assert_eq!(argv, Value::Str(b"alpha".to_vec()));

    // `_STR(_SYSTEM_PATH, 0)` = 宿主注入路径(标量字符串变量)。
    let sys_path_id = vm.sysvars().id("_SYSTEM_PATH").expect("_SYSTEM_PATH id");
    let path = yu_rt::sysvar::read(
        &vm,
        &st,
        VarRef::new(VarSpace::At, str_id),
        &[i64::from(sys_path_id), 0],
    )
    .expect("_STR(_SYSTEM_PATH)");
    assert_eq!(path, Value::Str(b"C:\\sys".to_vec()));
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
    // 合成一个不属于任何官方分类的未知命令:`CSV`/`CSVINFO` 已实现,
    // 不能再拿它们验证「Unimplemented 只挂一次」。追加到表尾不影响既有序号。
    b.commands.push(("ZZTESTUNKNOWN", vec![]));
    let op_unknown = (b.commands.len() - 1) as u8;
    b.scripts = vec![vec![
        GroupSpec::new(op_unknown),
        GroupSpec::new(op_unknown),
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, sys))
            .window(0, ex::i8_(7)),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "Unimplemented 不致命");
    assert_eq!(
        vm.unimplemented_cmds().get("ZZTESTUNKNOWN").copied(),
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
    use crate::support::Init;
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

/// `REGINFO`(`0x44bd30`):宿主读取 + 官方写回口径。
/// - 字符串目标成功 ⇒ 写值;
/// - INT/FLT 目标成功 ⇒ **保持原值**(官方成功分支只判 `type==3`);
/// - 键/值不存在 ⇒ 按类型写 0 / 0.0 / 空串;
/// - 根前缀不匹配 ⇒ 不写 LET;
/// - `KEY` 的 `/` 折成 `\` 后再交给宿主。
#[test]
fn reginfo_reads_host_registry_and_matches_official_write_rules() {
    use std::cell::RefCell;
    use std::rc::Rc;
    use yu_rt::event::{RegistryHost, RegistryValue};

    struct FakeRegistry {
        seen: Rc<RefCell<Vec<(String, Vec<u8>, Option<Vec<u8>>)>>>,
    }
    impl RegistryHost for FakeRegistry {
        fn read_value(
            &mut self,
            hive: &str,
            subkey: &[u8],
            name: Option<&[u8]>,
        ) -> Option<RegistryValue> {
            self.seen.borrow_mut().push((
                hive.to_string(),
                subkey.to_vec(),
                name.map(|n| n.to_vec()),
            ));
            if name == Some(b"missing".as_slice()) {
                None
            } else {
                Some(RegistryValue::String(b"hello".to_vec()))
            }
        }
    }

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
            init: Init::Int(42),
        },
        VarSpec {
            kind: 1,
            group: 0,
            script_id: 0,
            var_id: sys + 1,
            value_type: 1,
            dims: vec![],
            init: Init::Int(7),
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
        VarSpec {
            kind: 1,
            group: 0,
            script_id: 0,
            var_id: sys + 3,
            value_type: 1,
            dims: vec![],
            init: Init::Int(9),
        },
    ];
    let op_reginfo = b
        .commands
        .iter()
        .position(|(n, _)| *n == "REGINFO")
        .expect("夹具命令表应有 REGINFO") as u8;
    let reg = |key: &str, name: &str, letv: Vec<u8>| {
        GroupSpec::new(op_reginfo)
            .window(0, ex::s(key))
            .window(1, ex::s(name))
            .window(2, letv)
    };
    b.scripts = vec![vec![
        // 字符串目标:成功写 "hello"(KEY 用 `/`,宿主应收到 `\`)。
        reg(
            "HKEY_CURRENT_USER/Software/X",
            "s",
            ex::r#ref(0x24, sys + 2),
        ),
        // INT 目标:成功不写,保持 42。
        reg("HKEY_CURRENT_USER/Software/X", "i", ex::r#ref(0x40, sys)),
        // 键/值不存在:INT 目标写 0。
        reg(
            "HKEY_CURRENT_USER/Software/X",
            "missing",
            ex::r#ref(0x40, sys + 3),
        ),
        // 根前缀不匹配:不写,保持 7。
        reg("NOPE/Software/X", "s", ex::r#ref(0x40, sys + 1)),
        GroupSpec::new(OP_END),
    ]];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);

    let mut vm = boot(b);
    let seen = Rc::new(RefCell::new(Vec::new()));
    vm.set_registry_host(Box::new(FakeRegistry { seen: seen.clone() }));
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(
        vm.unimplemented_cmds().is_empty(),
        "REGINFO 不得再挂号: {:?} events={:?}",
        vm.unimplemented_cmds(),
        vm.events()
            .iter()
            .filter_map(|e| match e {
                yu_rt::VmEvent::Diag { code, detail, .. } => Some((code.clone(), detail.clone())),
                _ => None,
            })
            .collect::<Vec<_>>()
    );
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::Dollar, sys + 2), &[])
            .unwrap(),
        Value::Str(b"hello".to_vec())
    );
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys), &[])
            .unwrap(),
        Value::Int(42),
        "INT 目标成功分支官方不写"
    );
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys + 3), &[])
            .unwrap(),
        Value::Int(0),
        "缺失值 ⇒ 0"
    );
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys + 1), &[])
            .unwrap(),
        Value::Int(7),
        "根前缀不匹配 ⇒ 不写"
    );
    let seen = seen.borrow();
    assert_eq!(seen.len(), 3, "只在前缀匹配的 3 条命令上调用宿主");
    assert_eq!(seen[0].0, "HKEY_CURRENT_USER");
    assert_eq!(seen[0].1, b"Software\\X", "`/` 必须折成 `\\`");
    assert_eq!(seen[0].2.as_deref(), Some(b"s".as_slice()));
}

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

/// 扩展参数对齐 no-op:官方 0.495 `0x43fc74` + 过程 `0x4403d0` 对
/// `STR2..8/A*/E*/CHECK*/FILEOPEN/FILEEXT/FOLDER/CHECKBOX/SLIDER/NOWAIT`
/// **零读取**(0.479 `0x439cd8` 同构),也不求值表达式。
///
/// 夹具故意给 `CHECKBOX` 一个**未声明变量引用**:若内核还按旧逻辑求值,
/// 会先报 `rt.var.undeclared`。正确行为是跳过求值、接受命令、只记名字。
#[test]
fn dialog_extended_params_are_ignored_without_eval() {
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
        GroupSpec::new(OP_DIALOG)
            .window(1, ex::s("q"))
            .window(2, ex::s("line2"))
            .window(23, ex::r#ref(0x40, sys + 77)) // CHECKBOX:未声明,求值即错
            .window(22, ex::s("dir"))
            .window(49, ex::i8_(1))
            .window(24, ex::i8_(50))
            .window(11, ex::r#ref(0x40, sys)),
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
    assert!(
        !vm.events().iter().any(
            |e| matches!(e, yu_rt::VmEvent::Diag { code, .. } if code == "rt.cmd.unimplemented")
        ),
        "扩展参数不得再报 Unimplemented"
    );
    let pending = vm.pending_dialogs().to_vec();
    assert_eq!(pending.len(), 1);
    assert_eq!(
        pending[0].ignored_params,
        vec!["STR2", "CHECKBOX", "FOLDER", "NOWAIT", "SLIDER"],
        "官方不读的参数按窗口顺序记录名字"
    );
    assert_eq!(
        vm.store().read(user_var(&vm, 0), &[]).unwrap(),
        Value::Int(0),
        "未回答前不写 LET"
    );

    let id = pending[0].id;
    vm.answer_dialog(id, 1, None).expect("answer");
    assert_eq!(
        vm.store().read(user_var(&vm, 0), &[]).unwrap(),
        Value::Int(1),
        "扩展参数不改变基础 LET 写回"
    );
    let r1 = vm.tick_at(16, &no_input());
    assert!(r1.errors.is_empty(), "{:?}", r1.errors);
    assert_eq!(
        vm.store().read(user_var(&vm, 0), &[]).unwrap(),
        Value::Int(99),
        "回答后正常执行下一条"
    );
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

/// 官方不读取的参数(`NOWAIT` 等)对齐 no-op:命令照常弹基础框,
/// 不挂号、不改变写回(REA 0.495 `0x43fc74`;0.479 `0x439cd8`)。
#[test]
fn dialog_unsupported_param_is_aligned_noop() {
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
    let pending = vm.pending_dialogs().to_vec();
    assert_eq!(pending.len(), 1, "基础 DIALOG 正常挂起");
    assert_eq!(pending[0].ignored_params, vec!["NOWAIT"]);
    assert_eq!(
        vm.unimplemented_cmds().get("DIALOG").copied().unwrap_or(0),
        0,
        "官方不读的参数不再挂号"
    );
    // 事件流仍记录 DomainCall 事实;参数不进入 event_args(未求值)。
    assert!(vm.events().iter().any(|e| matches!(
        e,
        yu_rt::VmEvent::DomainCall { name, .. } if name == "DIALOG"
    )));
    assert!(!r.main_finished, "DIALOG 挂起时任务不推进到下一条");
    vm.answer_dialog(pending[0].id, 1, None).expect("answer");
    let r = vm.tick_at(16, &no_input());
    assert!(r.main_finished, "回答后执行 END");
}

// ---------------------------------------------------------------- P3 系统变量收口

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

/// `@_SAVE_PATH_TYPE` / `@_RET`:手册无页,但官方 getter 有确定取值。
///
/// - `_SAVE_PATH_TYPE`:宿主从游戏根 `yssfs.dat` 偏移 0x54 读入(官方 0.495
///   `FUN_00460c80`,只接受 0..=3),getter 读 `[0x801324]`;
/// - `_RET`:getter case 49 的块恒 `MOV EBP,0x3E7`,即 999,不读任何全局。
#[test]
fn save_path_type_and_ret_are_injected_not_unimplemented() {
    let mut b = ProjectBuilder::new();
    let v = with_int_var(&mut b, 0, -1);
    let v2 = with_int_var(&mut b, 1, -1);
    let spt = b
        .sysvars
        .iter()
        .position(|(n, _, _)| *n == "_SAVE_PATH_TYPE")
        .expect("_SAVE_PATH_TYPE") as u16;
    let ret = b
        .sysvars
        .iter()
        .position(|(n, _, _)| *n == "_RET")
        .expect("_RET") as u16;
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, v.id))
            .window(1, ex::get(0x40, spt)),
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, v2.id))
            .window(1, ex::get(0x40, ret)),
        GroupSpec::new(OP_END),
    ]];
    let src = b.build();
    let cfg = BootConfig {
        save_path_type: 3,
        ..BootConfig::default()
    };
    let mut vm = Vm::boot(Box::new(src), cfg).expect("boot");
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(
        vm.store().read(v, &[]).unwrap(),
        Value::Int(3),
        "宿主注入的存档路径类型原样读回"
    );
    assert_eq!(
        vm.store().read(v2, &[]).unwrap(),
        Value::Int(999),
        "官方 getter 恒返回 999"
    );
}

/// `@_WINDOW_EXEC_MIN`:手册无页,但官方是真实状态变量 —— 宿主 setter 置 1,
/// 脚本读 1 后写回 0(`fixed(no-manual)`)。
///
/// 证据(0.495 `yu-ris.exe`,sha256 `4f5eefa3…`):
/// - getter `FUN_00427350` 跳表 case 118 → `MOVSX EBP,byte ptr [0x80191c]`;
/// - 写侧 `0x446fa4` → `MOV byte ptr [0x80191c],DL`;
/// - 窗口过程 `FUN_0045acdc` 在 `WM_SYSCOMMAND & 0xFFF0 == SC_MINIMIZE`
///   写 `(char)槽号 + 1`(同分支族 MAX=`0x801918`/CLOSE=`0x801920`)。
#[test]
fn window_exec_min_is_a_host_setter_round_trip() {
    let mut b = ProjectBuilder::new();
    let v = with_int_var(&mut b, 0, -1);
    let v2 = with_int_var(&mut b, 1, -1);
    let min_id = b
        .sysvars
        .iter()
        .position(|(n, _, _)| *n == "_WINDOW_EXEC_MIN")
        .expect("_WINDOW_EXEC_MIN") as u16;
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        // LET[@v = @_WINDOW_EXEC_MIN]
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, v.id))
            .window(1, ex::get(0x40, min_id)),
        // LET[@_WINDOW_EXEC_MIN = 0](脚本写回 0)
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, min_id))
            .window(1, ex::i8_(0)),
        // LET[@v2 = @_WINDOW_EXEC_MIN](再读应为 0)
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, v2.id))
            .window(1, ex::get(0x40, min_id)),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    vm.request_window_minimize();
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

