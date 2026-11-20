//! 变量域:LET/DECLARE/VARINFO/VARACT/MATH/TASK/TASKINFO。
//!
//! 由 `tests/fixture.rs` 拆分而来;测试名与断言不变。

use super::common::*;

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

/// `ysv` 串初值 = **YSTB 参数数据**,装载时要过窗口求值器。
///
/// 证据(0.481 商业引擎 `FUN_00456e50`,调用点 `0x456f61`):
/// `uVar3 = *puVar11; (*DAT_008097e0)(pvVar6, puVar11 + 1, uVar3)` 之后
/// 才 `FUN_00465654(slot, buf, len)` 写变量 —— `0x4d` 字面量与
/// `$arr[i]` 表达式在字节层是同一种东西,必须求值。
///
/// 只认字面量的实现会把**字节码本身**当字符串存进变量,随后被脚本拼进
/// 资源路径(实测商业包:路径前缀出现 `56 03 00 24 77 04 …`,VFS 报
/// `NUL byte in lookup key`)。
#[test]
fn ysv_str_init_expression_is_evaluated_not_stored_raw() {
    fn leak(bytes: Vec<u8>) -> &'static [u8] {
        Box::leak(bytes.into_boxed_slice())
    }
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    // 表达式窗口 `$A[0]`(0x56 引用 + 0x42 下标 + 0x29 取值)。
    let expr = || leak(ex::cat(&[ex::r#ref(0x24, sys), ex::i8_(0), ex::aload()]));
    b.vars = vec![
        // 先声明:`$A` 是 2 元素串数组,每个元素初值 "cg/"。
        VarSpec {
            kind: 1,
            group: 0,
            script_id: 0,
            var_id: sys,
            value_type: 3,
            dims: vec![2],
            init: Init::Str(b"cg/"),
        },
        // 全局串:初值不是字面量,是表达式 `$A[0]`。
        VarSpec {
            kind: 1,
            group: 0,
            script_id: 0,
            var_id: sys + 1,
            value_type: 3,
            dims: vec![],
            init: Init::Str(expr()),
        },
        // 脚本 0 的局部串:同一表达式,在脚本装载时求值。
        VarSpec {
            kind: 2,
            group: 0,
            script_id: 0,
            var_id: sys + 2,
            value_type: 3,
            dims: vec![],
            init: Init::Str(expr()),
        },
    ];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![GroupSpec::new(OP_END)]];
    let mut vm = boot(b);
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);

    let read = |vm: &Vm, id: u16| {
        vm.store()
            .read(VarRef::new(VarSpace::Dollar, id), &[])
            .unwrap()
    };
    assert_eq!(
        read(&vm, sys + 1),
        Value::Str(b"cg/".to_vec()),
        "全局表达式初值要按求值结果落地"
    );
    assert_eq!(
        read(&vm, sys + 2),
        Value::Str(b"cg/".to_vec()),
        "局部表达式初值要按求值结果落地(不是字节码)"
    );
    // 数组元素初值保持字面量语义(与旧口径一致)。
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::Dollar, sys), &[1])
            .unwrap(),
        Value::Str(b"cg/".to_vec())
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
/// 2  TASKINFO[ID="SYSTEM" EXIST=1 …]     主线仍可见
/// 3  LET[@v = 7]                         主线继续写变量
/// 4  END
/// 5  SUB.BODY: LET[@v = 1]
/// 6           TASKEND[]                  只结束自己
/// 7           LET[@v = 2]                不应执行(R2 同时验证)
/// 8           END
/// ```
#[test]
fn taskend_without_id_ends_only_self_and_main_continues() {
    let mut b = ProjectBuilder::new();
    let v = with_int_var(&mut b, 0, 0);
    // 组 4 是主线的 END;子任务体从组 5 开始(TASKINFO 插在主 LET 之前)。
    b.labels = vec![("SYSTEM_START", 0, 0, 0), ("SUB.BODY", 5, 0, 0)];
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
        // 5: SUB.BODY
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, v.id))
            .window(0, ex::i8_(1)),
        // 6: TASKEND[](无 ID = 自身)
        GroupSpec::new(OP_TASKEND),
        // 7: 不应执行
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
    // 合成样本无 gfx 层;按真实路径注入目标层表面尺寸(否则官方语义下
    // `state+0x00 == 0` 不推进文本,任务会停在 RETURNCODE)。
    vm.set_text_window(Some((800, 600)));
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
        tail_byte: 0,
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
    vm.set_text_window(Some((800, 600)));
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
    // 点击与首次布局同帧:官方 `FUN_004046fc` 在 post-buttons 只消费 'C',
    // 不再重跑布局;R/P/E 在下一帧处理完后才恢复任务。
    vm.tick_at(32, &no_input());
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
    vm.set_text_window(Some((800, 600)));
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
        // 主线保持存活(R1 的 END 是全程序退出,不能再用它“停主线”)。
        GroupSpec::new(OP_WAIT).window(0, ex::i8_(100)),
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
/// 2 WAIT[FRAME=100]          主线保持存活(END 是全程序退出)
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
        // 主线保持存活:END 是全程序退出(REA 0x441334),不能再用它“停主线”。
        GroupSpec::new(OP_WAIT).window(0, ex::i8_(100)),
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

/// `VARINFO[STRTYPE]`:REA 0.495 handler `0x456050` 分支 `0x4563b7` ——
/// 首字符的 CP932 字节宽度:单字节 = 1,lead byte = 2;空串报错。
/// `ystdlib.yst:489` `_strtype` 的「1=半角/2=全角」注释与之一致。
#[test]
fn varinfo_strtype_reports_first_char_width() {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![
        // $zen = "あ"(CP932 82 A0,双字节)
        VarSpec {
            kind: 1,
            group: 1,
            script_id: 0,
            var_id: sys,
            value_type: 3,
            dims: vec![],
            init: Init::Str(b"\x82\xa0"),
        },
        // $han = "｡"(CP932 A1,半角片假名,单字节)
        VarSpec {
            kind: 1,
            group: 1,
            script_id: 0,
            var_id: sys + 1,
            value_type: 3,
            dims: vec![],
            init: Init::Str(b"\xa1"),
        },
        VarSpec {
            kind: 1,
            group: 1,
            script_id: 0,
            var_id: sys + 2,
            value_type: 1,
            dims: vec![4],
            init: Init::Int(0),
        },
        VarSpec {
            kind: 1,
            group: 1,
            script_id: 0,
            var_id: sys + 3,
            value_type: 1,
            dims: vec![4],
            init: Init::Int(0),
        },
        // FLT 目标:官方按 LET 当前类型写 2.0
        VarSpec {
            kind: 1,
            group: 1,
            script_id: 0,
            var_id: sys + 4,
            value_type: 2,
            dims: vec![],
            init: Init::Float(0.0),
        },
    ];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    let zen = ex::r#ref(0x24, sys);
    let han = ex::r#ref(0x24, sys + 1);
    let r_zen = ex::r#ref(0x40, sys + 2);
    let r_han = ex::r#ref(0x40, sys + 3);
    let r_flt = ex::r#ref(0x40, sys + 4);
    b.scripts = vec![vec![
        GroupSpec::new(101)
            .window(0, zen.clone())
            .window(3, ex::i8_(1))
            .window(1, ex::cat(&[r_zen, ex::i8_(0), ex::aload()])),
        GroupSpec::new(101)
            .window(0, han)
            .window(3, ex::i8_(1))
            .window(1, ex::cat(&[r_han, ex::i8_(0), ex::aload()])),
        GroupSpec::new(101)
            .window(0, zen)
            .window(3, ex::i8_(1))
            .window(1, r_flt),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys + 2), &[0])
            .unwrap(),
        Value::Int(2),
        "全角首字符 → 2"
    );
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys + 3), &[0])
            .unwrap(),
        Value::Int(1),
        "半角首字符 → 1"
    );
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys + 4), &[])
            .unwrap(),
        Value::Float(2.0),
        "FLT 目标按官方写 2.0"
    );
}

/// `SJISCODE`/`UTF8CODE` 槽在官方 0.495 处理体里零引用 ⇒ 无操作。
/// 只给它们时不得报 Unimplemented,也不得改 LET。
#[test]
fn varinfo_inert_sjiscode_is_noop() {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![
        VarSpec {
            kind: 1,
            group: 1,
            script_id: 0,
            var_id: sys,
            value_type: 3,
            dims: vec![],
            init: Init::Str(b"yuu"),
        },
        VarSpec {
            kind: 1,
            group: 1,
            script_id: 0,
            var_id: sys + 1,
            value_type: 1,
            dims: vec![],
            init: Init::Int(7),
        },
    ];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        GroupSpec::new(101)
            .window(0, ex::r#ref(0x24, sys))
            .window(15, ex::i8_(1))
            .window(1, ex::r#ref(0x40, sys + 1)),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys + 1), &[])
            .unwrap(),
        Value::Int(7),
        "惰性槽不得改写 LET"
    );
    let stub = vm.events().iter().any(|e| {
        matches!(
            e,
            yu_rt::VmEvent::Diag { code, .. } if code.to_string() == "rt.cmd.aligned_stub"
        )
    });
    assert!(stub, "应留一条 aligned-stub 取证");
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

#[test]
fn dimsize_huge_is_fatal_not_an_abort() {
    // 修复前:`VARACT[DIMSIZE=<巨型>]` → Vec::resize 容量溢出/ OOM abort。
    use crate::support::Init;
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

