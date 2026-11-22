//! 独立运行面(`other`)回归:三组 GOSUB 帧的顶层 RETURN 返回值。
//!
//! 555 启动链 `es.ERIS` 用 `GOSUB es._label.info.exist` + `IF @_RINT(1)`
//! 判标签存在;任务内第一次 GOSUB 的 RETURN 必须写回任务基帧,否则调用方
//! 恒读 0(ES.FIRST.LOOP 永远不被调用)。

use super::common::*;

#[test]
fn other_runtime_top_level_return_survives_to_caller() {
    let mut b = ProjectBuilder::new();
    b.ysc_version = 555;
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
    b.scripts[0][1].windows[1].4 = ex::cat(&[ex::r#ref(0x40, rint), ex::i8_(1), ex::aload()]);
    b.scripts[0][3].windows[1].4 = ex::cat(&[
        ex::r#ref(0x40, pint),
        ex::i8_(1),
        ex::aload(),
        ex::i8_(2),
        ex::op(0x2a),
    ]);

    let mut vm = boot(b);
    // 与 yu-engine 装配一致:精确 555 才安装独立运行面。
    vm.install_other_runtime();
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
        "任务基帧承接 RETURN RINT(1),调用方读回 42"
    );
}

/// v555 宏链直接以 `$_PSTR(n)`(= `$0x37[n]`)读字符串实参。
///
/// 病栋 2 的标题脚本 `es.BT.CG.SET`(s9 pc0)不调用 `_PSTR()` 访问器,
/// 而是把 `$0x37[1..6]` 直接拷进 BT 表;若 GOSUB 帧没有让系统变量通路
/// 读到 PSTR,标题背景/按钮名会全部变空。
#[test]
fn other_gosub_pstr_is_visible_through_system_var_slot() {
    let mut b = ProjectBuilder::new();
    b.ysc_version = 555;
    let sys = b.sysvars.len() as u16;
    b.vars = vec![VarSpec {
        kind: 1,
        group: 0,
        script_id: 0,
        var_id: sys,
        value_type: 3,
        dims: vec![1],
        init: Init::Str(b""),
    }];
    b.labels = vec![("SYSTEM_START", 0, 0, 0), ("SUB", 3, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        // 0: GOSUB[#="SUB" PSTR1="HELLO"]
        GroupSpec::new(OP_GOSUB)
            .window(0, ex::s("SUB"))
            .window(33, ex::sb(b"HELLO")),
        GroupSpec::new(OP_END),
        GroupSpec::new(OP_END),
        // 3: SUB: LET[$out[0] = $_PSTR(1)]; RETURN
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x24, sys))
            .window(0, ex::cat(&[ex::r#ref(0x24, 0), ex::i8_(1), ex::aload()])),
        GroupSpec::new(OP_RETURN),
    ]];
    // 系统变量 id 来自工程 ABI(真实 555 工程 `_PSTR`=0x37;夹具表序号可能不同)。
    let probe_src = {
        let mut p = ProjectBuilder::new();
        p.sysvars = b.sysvars.clone();
        p.build()
    };
    let tables = yu_rt::TableSet::load(&probe_src, yu_rt::tables::DEFAULT_SCRIPT_KEY).unwrap();
    let pstr = tables.boot.sysvars.id("_PSTR").expect("_PSTR in ABI");
    b.scripts[0][3].windows[1].4 = ex::cat(&[ex::r#ref(0x24, pstr), ex::i8_(1), ex::aload()]);
    let mut vm = boot(b);
    vm.install_other_runtime();
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::Dollar, sys), &[0])
            .unwrap(),
        Value::Str(b"HELLO".to_vec()),
        "被调方必须能通过 $_PSTR(1)=$0x37[1] 读到 GOSUB 的 PSTR1"
    );
}

/// v555 PINT 实参按**当前调用帧**读,嵌套 GOSUB 返回后仍是外层实参。
///
/// 证据:`es_select2.yst` s36 pc114/115 先嵌套 `GOSUB es.BT.N1`,
/// pc117–120 再从 `$_PINT(1..4)` 取外层 `\CSEL` 传入的
/// 1/-1/350/300 填 `$0x49b[93][1..4]`。若读全局“最后一次实参”,
/// 会被 N1 的 PINT2=0 覆盖成 0,CSEL 按钮参数全错。
#[test]
fn other_pint_survives_nested_gosub() {
    let mut b = ProjectBuilder::new();
    b.ysc_version = 555;
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
    b.labels = vec![
        ("SYSTEM_START", 0, 0, 0),
        ("SUB1", 3, 0, 0),
        ("SUB2", 6, 0, 0),
    ];
    b.entry(0, 0);
    b.scripts = vec![vec![
        // 0: GOSUB[#="SUB1" PINT1=1 PINT2=-1]
        GroupSpec::new(OP_GOSUB)
            .window(0, ex::s("SUB1"))
            .window(1, ex::i8_(1))
            .window(2, ex::i8_(-1)),
        GroupSpec::new(OP_END),
        GroupSpec::new(OP_END),
        // 3: SUB1: GOSUB[#="SUB2"]; LET[@out=@_PINT(2)]
        GroupSpec::new(OP_GOSUB).window(0, ex::s("SUB2")),
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, sys))
            .window(0, ex::cat(&[ex::r#ref(0x40, 0), ex::i8_(2), ex::aload()])),
        GroupSpec::new(OP_RETURN),
        // 6: SUB2: RETURN
        GroupSpec::new(OP_RETURN),
    ]];
    let probe_src = {
        let mut p = ProjectBuilder::new();
        p.sysvars = b.sysvars.clone();
        p.build()
    };
    let tables = yu_rt::TableSet::load(&probe_src, yu_rt::tables::DEFAULT_SCRIPT_KEY).unwrap();
    let pint = tables.boot.sysvars.id("_PINT").expect("_PINT in ABI");
    b.scripts[0][4].windows[1].4 = ex::cat(&[ex::r#ref(0x40, pint), ex::i8_(2), ex::aload()]);
    let mut vm = boot(b);
    vm.install_other_runtime();
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys), &[])
            .unwrap(),
        Value::Int(-1),
        "嵌套无参 GOSUB 返回后,当前帧 PINT2 仍是外层 -1"
    );
}

/// v555 GOSUB 的整数参数数组引用:ERIS 旧式空间(`#`/`` ` ``)传槽号,
/// `@` 普通数值数组仍按元素值。
///
/// 证据:`es_scenario_parser.yst` s61 pc212 以 PINT2=`#0x0b5f[0]`、
/// PINT3=`` `0x0b60[0] `` 调 `es.ScenarioMacroExec`;被调方
/// `es.ScenarioMacro.SetParam` 用 `_STR(_PINT(2), i)` /
/// `_INT(_PINT(3), i)` 读参数数组。若数值数组也按值,`_PINT(3)` 会变成
/// 元素值 1,`_INT(1+i)` 全 0,DrawBG 把 logo_a/bg_black 当缺参清空。
/// 同一解析器 pc93 的 `com_no.pos.SET` 用 `@` 数组元素,必须仍传元素值。
#[test]
fn other_gosub_legacy_array_args_pass_slot_id_not_element() {
    let mut b = ProjectBuilder::new();
    b.ysc_version = 555;
    let sys = b.sysvars.len() as u16;
    let int_arr = sys + 3;
    let str_arr = sys + 4;
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
        VarSpec {
            kind: 1,
            group: 0,
            script_id: 0,
            var_id: sys + 2,
            value_type: 1,
            dims: vec![],
            init: Init::Int(0),
        },
        VarSpec {
            kind: 1,
            group: 0,
            script_id: 0,
            var_id: int_arr,
            value_type: 1,
            dims: vec![2],
            init: Init::Int(0),
        },
        VarSpec {
            kind: 1,
            group: 0,
            script_id: 0,
            var_id: str_arr,
            value_type: 3,
            dims: vec![2],
            init: Init::Str(b""),
        },
    ];
    b.labels = vec![("SYSTEM_START", 0, 0, 0), ("SUB", 4, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        // 0: @arr[0] = 7
        GroupSpec::new(OP_LET)
            .window(
                0,
                ex::cat(&[ex::r#ref(0x40, int_arr), ex::i8_(0), ex::aload()]),
            )
            .window(1, ex::i8_(7)),
        // 1: $arr[0] = "HELLO"
        GroupSpec::new(OP_LET)
            .window(
                0,
                ex::cat(&[ex::r#ref(0x24, str_arr), ex::i8_(0), ex::aload()]),
            )
            .window(1, ex::sb(b"HELLO")),
        // 2: GOSUB[#="SUB" PINT1=`int[0] PINT2=#str[0] PINT3=@int[0]]
        GroupSpec::new(OP_GOSUB)
            .window(0, ex::s("SUB"))
            .window(
                1,
                ex::cat(&[ex::r#ref(0x60, int_arr), ex::i8_(0), ex::aload()]),
            )
            .window(
                2,
                ex::cat(&[ex::r#ref(0x23, str_arr), ex::i8_(0), ex::aload()]),
            )
            .window(
                3,
                ex::cat(&[ex::r#ref(0x40, int_arr), ex::i8_(0), ex::aload()]),
            ),
        GroupSpec::new(OP_END),
        // 4..6: @out = @_PINT(n)
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, sys))
            .window(0, ex::r#ref(0x40, 0)),
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, sys + 1))
            .window(0, ex::r#ref(0x40, 0)),
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, sys + 2))
            .window(0, ex::r#ref(0x40, 0)),
        // 7: RETURN
        GroupSpec::new(OP_RETURN),
    ]];
    let probe_src = {
        let mut p = ProjectBuilder::new();
        p.sysvars = b.sysvars.clone();
        p.build()
    };
    let tables = yu_rt::TableSet::load(&probe_src, yu_rt::tables::DEFAULT_SCRIPT_KEY).unwrap();
    let pint = tables.boot.sysvars.id("_PINT").expect("_PINT in ABI");
    for (row, n) in [(4usize, 1i8), (5, 2), (6, 3)] {
        b.scripts[0][row].windows[1].4 = ex::cat(&[ex::r#ref(0x40, pint), ex::i8_(n), ex::aload()]);
    }
    let mut vm = boot(b);
    vm.install_other_runtime();
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let read = |id| vm.store().read(VarRef::new(VarSpace::At, id), &[]).unwrap();
    assert_eq!(
        read(sys),
        Value::Int(i64::from(int_arr)),
        "`int[0]`(旧式 0x60)以槽号 2912 一类的 id 传入"
    );
    assert_eq!(
        read(sys + 1),
        Value::Int(i64::from(str_arr)),
        "`#str[0]`(旧式 0x23)以槽号传入"
    );
    assert_eq!(
        read(sys + 2),
        Value::Int(7),
        "`@int[0]` 普通数值数组仍按元素值传入"
    );
}

/// v555 `TEXT[FILE, LET, CLEAR]`:读整份场景文本,按 CRLF/CR/LF 切行,
/// 每行末尾加 `0x0D` 写入字符串数组;末尾两个空元素(REA <v555-4> `0x452a0c`)。
#[test]
fn text_555_file_fills_let_string_array() {
    let mut b = ProjectBuilder::new();
    b.ysc_version = 555;
    // 555 参数表:TEXT = FILE/LET/CLEAR(0.495 的 TEXT 没有 FILE)。
    let text = &mut b
        .commands
        .iter_mut()
        .find(|(n, _)| *n == "TEXT")
        .expect("fixture TEXT")
        .1;
    *text = vec![("FILE", 1, 0), ("LET", 0, 0), ("CLEAR", 0, 3)];
    let op_text = b.commands.iter().position(|(n, _)| *n == "TEXT").unwrap() as u8;
    let sys = b.sysvars.len() as u16;
    b.vars = vec![VarSpec {
        kind: 1,
        group: 0,
        script_id: 0,
        var_id: sys,
        value_type: 3,
        dims: vec![8],
        init: Init::Str(b""),
    }];
    b.scripts = vec![vec![
        GroupSpec::new(op_text)
            .window(0, ex::sb(b"scn/a.txt"))
            .window(1, ex::r#ref(0x24, sys)),
        GroupSpec::new(OP_END),
    ]];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    let mut src = b.build();
    // 官方打开前把 `/` 换成 `\`;夹具里直接用规范化后的 key。
    src.insert(b"scn\\a.txt", b"#a\r\n#b\r\n".to_vec())
        .expect("insert scenario text");
    let mut vm = Vm::boot(Box::new(src), BootConfig::default()).expect("boot");
    // 555 TEXT[FILE] 由独立运行面拦截(引擎按精确 555 安装)。
    vm.install_other_runtime();
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let arr = VarRef::new(VarSpace::Dollar, sys);
    let want: &[&[u8]] = &[b"#a\r", b"#b\r", b"\r", b"", b""];
    for (i, line) in want.iter().enumerate() {
        assert_eq!(
            vm.store().read(arr, &[i as i64]).unwrap(),
            Value::Str(line.to_vec()),
            "line[{i}]"
        );
    }
}

/// v555 `TEXT[FILE]` 打不开时官方返回 -1,调用方继续执行下一条命令
/// (样本脚本用 base → `sc_cn_` → `sc_tw_` 三级回退)。内核必须让出一次
/// 后推进 pc,不能停在同一条 TEXT 上。
#[test]
fn text_555_missing_file_yields_then_advances_to_next_command() {
    let mut b = ProjectBuilder::new();
    b.ysc_version = 555;
    let text = &mut b
        .commands
        .iter_mut()
        .find(|(n, _)| *n == "TEXT")
        .expect("fixture TEXT")
        .1;
    *text = vec![("FILE", 1, 0), ("LET", 0, 0), ("CLEAR", 0, 3)];
    let op_text = b.commands.iter().position(|(n, _)| *n == "TEXT").unwrap() as u8;
    let sys = b.sysvars.len() as u16;
    b.vars = vec![
        VarSpec {
            kind: 1,
            group: 0,
            script_id: 0,
            var_id: sys,
            value_type: 3,
            dims: vec![8],
            init: Init::Str(b""),
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
    b.scripts = vec![vec![
        GroupSpec::new(op_text)
            .window(0, ex::sb(b"scn/missing.txt"))
            .window(1, ex::r#ref(0x24, sys)),
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, sys + 1))
            .window(1, ex::i8_(7)),
        GroupSpec::new(OP_END),
    ]];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    let mut vm = boot(b);
    vm.install_other_runtime();
    let first = vm.tick_at(0, &no_input());
    assert!(first.errors.is_empty(), "{:?}", first.errors);
    vm.tick_at(16, &no_input());
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys + 1), &[])
            .unwrap(),
        Value::Int(7),
        "TEXT 失败后必须推进到下一条 LET"
    );
}

/// v555 `TEXTINFO` 参数表为 0;官方 `0x452d54` 只装载参数后 `return 0`。
/// 空签名调用不得报「缺 LET」。
#[test]
fn textinfo_empty_signature_is_a_silent_noop() {
    let mut b = ProjectBuilder::new();
    b.ysc_version = 555;
    b.scripts = vec![vec![GroupSpec::new(OP_TEXTINFO), GroupSpec::new(OP_END)]];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    let mut vm = boot(b);
    vm.install_other_runtime();
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let diags = format!("{:?}", vm.diags());
    assert!(
        !diags.contains("TEXTINFO"),
        "空签名 TEXTINFO 不应产生诊断: {diags}"
    );
}
