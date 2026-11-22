//! 文件/存档域:FILEINFO/CSV/SAVE/LOAD/FILEACT。
//!
//! 由 `tests/fixture.rs` 拆分而来;测试名与断言不变。

use super::common::*;

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
            .window(4, ex::i8_(1))
            .window(7, ex::r#ref(0x40, sys)),
        // FILEINFO[FILE="data/missing.png" EXIST=1 LET=@b]
        GroupSpec::new(OP_FILEINFO)
            .window(0, ex::s("data/missing.png"))
            .window(4, ex::i8_(1))
            .window(7, ex::r#ref(0x40, sys + 1)),
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
            .window(4, ex::i8_(1))
            .window(7, ex::r#ref(0x40, sys)),
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

/// `FILEINFO[FOLDER=...]`(手册例 2;REA `0x4433d6`):
/// - `EXIST` 只查散盘目录(`FILE_ATTRIBUTE_DIRECTORY`),包内文件夹不可查;
/// - `SIZE` 对 FOLDER 写 0(官方只支持 FILE);
/// - `RELATION` 对 FOLDER 写目标类型默认值;
/// - `..` 被拒绝。
#[test]
fn fileinfo_folder_checks_disk_directory() {
    let root = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("fileinfo-folder");
    std::fs::create_dir_all(root.join("cg")).expect("fixture dir");
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    let mut vars = Vec::new();
    for offset in 0..3 {
        vars.push(VarSpec {
            kind: 1,
            group: 0,
            script_id: 0,
            var_id: sys + offset,
            value_type: 1,
            dims: vec![],
            init: Init::Int(-1),
        });
    }
    vars.push(VarSpec {
        kind: 1,
        group: 0,
        script_id: 0,
        var_id: sys + 3,
        value_type: 3,
        dims: vec![],
        init: Init::Str(b"old"),
    });
    b.vars = vars;
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    let folder = |path: &str, query: u8, letv: Vec<u8>| {
        GroupSpec::new(OP_FILEINFO)
            .window(1, ex::s(path))
            .window(query, ex::i8_(1))
            .window(7, letv)
    };
    b.scripts = vec![vec![
        folder("cg", 4, ex::r#ref(0x40, sys)),
        folder("missing", 4, ex::r#ref(0x40, sys + 1)),
        folder("cg", 5, ex::r#ref(0x40, sys + 2)),
        folder("cg", 6, ex::r#ref(0x24, sys + 3)),
        folder("../escape", 4, ex::r#ref(0x40, sys + 1)),
        GroupSpec::new(OP_END),
    ]];
    let src = b.build();
    let mut cfg = BootConfig::default();
    let mut system = root.to_string_lossy().replace('\\', "/").into_bytes();
    system.push(b'/');
    cfg.paths.system_path = Some(system);
    let mut vm = Vm::boot(Box::new(src), cfg).expect("boot");
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
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
        "不存在 / `..` 都写 0"
    );
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys + 2), &[])
            .unwrap(),
        Value::Int(0),
        "FOLDER+SIZE 官方写 0"
    );
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::Dollar, sys + 3), &[])
            .unwrap(),
        Value::Str(Vec::new()),
        "FOLDER+RELATION 写空串"
    );
    assert!(
        vm.unimplemented_cmds().is_empty(),
        "FOLDER 不得再挂号: {:?}",
        vm.unimplemented_cmds()
    );
}

// ---------------------------------------------------------------- IF / ELSE
// ------------------------------------------------------------------- YMV

#[test]
fn csv_parses_cells_and_csvinfo_counts_cells() {
    // v474 语义(REA 0x43577c/0x435cfc):
    // - CSVINFO 统计“单元格数”(逗号 + 有内容的行终止),不是文件字节数;
    // - CSV 把单元格按出现顺序写进 LET 字符串数组,文件尾自动补 CRLF;
    // - 单元格保留 CP932 原始字节,行首空格/Tab 跳过。
    // v474 表在 CSV 前多一个槽(0.495 的 CSV=7 是空桩);解析器只在 v474
    // 形状(CSV=8)下选择。
    let mut b = with_v474_extra_slot(ProjectBuilder::new());
    let op_end = opcode_of(&b, "END");
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
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    let op_csv = b
        .commands
        .iter()
        .position(|(n, _)| *n == "CSV")
        .expect("夹具命令表应有 CSV") as u8;
    let op_csvinfo = b
        .commands
        .iter()
        .position(|(n, _)| *n == "CSVINFO")
        .expect("夹具命令表应有 CSVINFO") as u8;
    b.scripts = vec![vec![
        GroupSpec::new(op_csvinfo)
            .window(0, ex::sb(b"data/test.csv"))
            .window(1, ex::i8_(1))
            .window(2, ex::r#ref(0x40, sys + 1)),
        GroupSpec::new(op_csv)
            .window(0, ex::sb(b"data/test.csv"))
            .window(1, ex::r#ref(0x24, sys)),
        GroupSpec::new(op_end),
    ]];
    let file = b"a,b\r\n\x83\x74\x83\x48,c\r\n".to_vec();
    let mut src = b.build();
    src.insert(b"data/test.csv", file).unwrap();
    let mut vm = Vm::boot(Box::new(src), BootConfig::default()).expect("boot");
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let arr = VarRef::new(VarSpace::Dollar, sys);
    // 文件尾 CRLF 让 CSV 再产生一个空单元格(官方 len+2 追加的直接结果)。
    let want: &[&[u8]] = &[b"a", b"b", b"\x83\x74\x83\x48", b"c", b""];
    for (i, cell) in want.iter().enumerate() {
        assert_eq!(
            vm.store().read(arr, &[i as i64]).unwrap(),
            Value::Str(cell.to_vec()),
            "cell[{i}]"
        );
    }
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys + 1), &[])
            .unwrap(),
        Value::Int(4),
        "CSVINFO:第 1 行 2 格 + 第 2 行 2 格 = 4"
    );
}

#[test]
fn csvinfo_does_not_append_crlf() {
    // 官方 CSVINFO 不补 CRLF:文件末行没有终止符时,最后一格不计入
    // (CSV 仍会补 CRLF 并写出该格)。
    let mut b = with_v474_extra_slot(ProjectBuilder::new());
    let op_end = opcode_of(&b, "END");
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
    let op_csvinfo = b
        .commands
        .iter()
        .position(|(n, _)| *n == "CSVINFO")
        .expect("夹具命令表应有 CSVINFO") as u8;
    b.scripts = vec![vec![
        GroupSpec::new(op_csvinfo)
            .window(0, ex::sb(b"data/noeol.csv"))
            .window(1, ex::i8_(1))
            .window(2, ex::r#ref(0x40, sys)),
        GroupSpec::new(op_end),
    ]];
    let mut src = b.build();
    src.insert(b"data/noeol.csv", b"a,b\r\nc,d".to_vec())
        .unwrap();
    let mut vm = Vm::boot(Box::new(src), BootConfig::default()).expect("boot");
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    // 2 个逗号 + 第 1 行终止 = 3;末行 d 没有终止符,不计入。
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys), &[])
            .unwrap(),
        Value::Int(3)
    );
}

#[test]
fn csv_missing_file_yields_without_fatal() {
    let mut b = with_v474_extra_slot(ProjectBuilder::new());
    let op_end = opcode_of(&b, "END");
    let op_let = opcode_of(&b, "LET");
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
    let op_csv = b
        .commands
        .iter()
        .position(|(n, _)| *n == "CSV")
        .expect("夹具命令表应有 CSV") as u8;
    b.scripts = vec![vec![
        GroupSpec::new(op_csv)
            .window(0, ex::sb(b"data/missing.csv"))
            .window(1, ex::r#ref(0x40, sys)),
        GroupSpec::new(op_let)
            .window(0, ex::r#ref(0x40, sys))
            .window(0, ex::i8_(7)),
        GroupSpec::new(op_end),
    ]];
    let mut vm = boot(b);
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "缺文件不致命: {:?}", r.errors);
    assert!(
        vm.events().iter().any(
            |e| matches!(e, yu_rt::VmEvent::Diag { code, .. } if code == "rt.csv")
        ),
        "应挂 rt.csv 诊断"
    );
    // 官方返回 -1 ⇒ 本帧让出;下一帧继续执行后续命令。
    let r = vm.tick_at(1, &no_input());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys), &[])
            .unwrap(),
        Value::Int(7),
        "让出后继续"
    );
}

#[test]
fn csv_at_0495_slot_is_noop() {
    // 0.495 表序号 7 的 CSV 是空桩(REA 0x43e640:只求值参数后 `ret 0`):
    // 不打开文件、不写 LET。解析器只在 v474 形状(CSV=8)下选择。
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![VarSpec {
        kind: 1,
        group: 0,
        script_id: 0,
        var_id: sys,
        value_type: 3,
        dims: vec![],
        init: Init::Str(b"keep"),
    }];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    let op_csv = b
        .commands
        .iter()
        .position(|(n, _)| *n == "CSV")
        .expect("夹具命令表应有 CSV") as u8;
    b.scripts = vec![vec![
        GroupSpec::new(op_csv)
            .window(0, ex::sb(b"data/absent.csv"))
            .window(1, ex::r#ref(0x40, sys)),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::Dollar, sys), &[])
            .unwrap(),
        Value::Str(b"keep".to_vec()),
        "0.495 CSV 空桩不得写 LET / 不得打开文件"
    );
}

/// v555 的 CSV 仍是序号 7 且无 LINE,但处理体是真实解析器
/// (`0x43c3a0`,1148B;<v555-1> 变体 1682B)。不能再套 0.495 的空桩判据。
#[test]
fn csv_at_555_slot_is_a_real_parser() {
    let mut b = ProjectBuilder::new();
    b.ysc_version = 555;
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
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    let op_csv = b
        .commands
        .iter()
        .position(|(n, _)| *n == "CSV")
        .expect("夹具命令表应有 CSV") as u8;
    b.scripts = vec![vec![
        GroupSpec::new(op_csv)
            .window(0, ex::sb(b"data/test.csv"))
            .window(1, ex::r#ref(0x24, sys)),
        GroupSpec::new(OP_END),
    ]];
    let mut src = b.build();
    src.insert(b"data/test.csv", b"a,b\r\nc,d\r\n".to_vec())
        .unwrap();
    let mut vm = Vm::boot(Box::new(src), BootConfig::default()).expect("boot");
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let arr = VarRef::new(VarSpace::Dollar, sys);
    let want: &[&[u8]] = &[b"a", b"b", b"c", b"d", b""];
    for (i, cell) in want.iter().enumerate() {
        assert_eq!(
            vm.store().read(arr, &[i as i64]).unwrap(),
            Value::Str(cell.to_vec()),
            "cell[{i}]"
        );
    }
}

/// 0.466 表:CSV 序号仍是 7,但参数表带 `LINE` ⇒ 真实解析器 + 行过滤。
/// 证据:0.466 `0x431970`(`mov eax,7; call` 后按 `[0x650201]` LINE 过滤)。
#[test]
fn csv_0466_with_line_param_reads_and_filters_rows() {
    let mut b = ProjectBuilder::new();
    b.commands
        .iter_mut()
        .find(|c| c.0 == "CSV")
        .expect("夹具命令表应有 CSV")
        .1
        .insert(1, ("LINE", 0, 0));
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
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    let op_csv = opcode_of(&b, "CSV");
    b.scripts = vec![vec![
        GroupSpec::new(op_csv)
            .window(0, ex::sb(b"data/test.csv"))
            .window(1, ex::i8_(2))
            .window(2, ex::r#ref(0x24, sys)),
        GroupSpec::new(OP_END),
    ]];
    let mut src = b.build();
    src.insert(b"data/test.csv", b"a,b\r\nc,d\r\n".to_vec())
        .unwrap();
    let mut vm = Vm::boot(Box::new(src), BootConfig::default()).expect("boot");
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let arr = VarRef::new(VarSpace::Dollar, sys);
    assert_eq!(vm.store().read(arr, &[0]).unwrap(), Value::Str(b"c".to_vec()));
    assert_eq!(vm.store().read(arr, &[1]).unwrap(), Value::Str(b"d".to_vec()));
    assert_eq!(
        vm.store().read(arr, &[2]).unwrap(),
        Value::Str(Vec::new()),
        "只写第 2 行,索引从 0 重计"
    );
}

/// 0.495 表:CSVINFO[DATA=1] 把单元格写入 LET 字符串数组
/// (REA `0x43e660` 的 DATA 分支 → `0x43f0d8`)。
#[test]
fn csvinfo_data_writes_cells_into_let_array() {
    let mut b = ProjectBuilder::new();
    // 默认 [FILE,SIZE,LET] → [FILE,SIZE,DATA,LET]
    b.commands
        .iter_mut()
        .find(|c| c.0 == "CSVINFO")
        .expect("夹具命令表应有 CSVINFO")
        .1
        .insert(2, ("DATA", 0, 0));
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
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    let op_csvinfo = opcode_of(&b, "CSVINFO");
    b.scripts = vec![vec![
        GroupSpec::new(op_csvinfo)
            .window(0, ex::sb(b"data/test.csv"))
            .window(2, ex::i8_(1))
            .window(3, ex::r#ref(0x24, sys)),
        GroupSpec::new(OP_END),
    ]];
    let mut src = b.build();
    src.insert(b"data/test.csv", b"a,b\r\nc,d\r\n".to_vec())
        .unwrap();
    let mut vm = Vm::boot(Box::new(src), BootConfig::default()).expect("boot");
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let arr = VarRef::new(VarSpace::Dollar, sys);
    let want: &[&[u8]] = &[b"a", b"b", b"c", b"d", b""];
    for (i, cell) in want.iter().enumerate() {
        assert_eq!(
            vm.store().read(arr, &[i as i64]).unwrap(),
            Value::Str(cell.to_vec()),
            "cell[{i}]"
        );
    }
}

/// 0.466 表:CSVINFO 的 `LINE` 既过滤 SIZE 计数,也可单独返回行数。
/// 证据:0x466 `0x432050`;`SIZE` 计数含 EOF 最后一格(`0x43225a`),
/// 纯 `LINE` 写当前行号 `ebp`(`0x4322f2`)。
#[test]
fn csvinfo_0466_line_filters_count_and_returns_line_count() {
    let mut b = ProjectBuilder::new();
    // 默认 [FILE,SIZE,LET] → [FILE,LINE,SIZE,LET]
    b.commands
        .iter_mut()
        .find(|c| c.0 == "CSVINFO")
        .expect("夹具命令表应有 CSVINFO")
        .1
        .insert(1, ("LINE", 0, 0));
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
    let op_csvinfo = opcode_of(&b, "CSVINFO");
    b.scripts = vec![vec![
        // SIZE=1 LINE=2 → 第 2 行 2 格
        GroupSpec::new(op_csvinfo)
            .window(0, ex::sb(b"data/test.csv"))
            .window(1, ex::i8_(2))
            .window(2, ex::i8_(1))
            .window(3, ex::r#ref(0x40, sys)),
        // LINE=1(无 SIZE)→ 行数
        GroupSpec::new(op_csvinfo)
            .window(0, ex::sb(b"data/test.csv"))
            .window(1, ex::i8_(1))
            .window(3, ex::r#ref(0x40, sys + 1)),
        GroupSpec::new(OP_END),
    ]];
    let mut src = b.build();
    src.insert(b"data/test.csv", b"a,b\r\nc,d\r\n".to_vec())
        .unwrap();
    let mut vm = Vm::boot(Box::new(src), BootConfig::default()).expect("boot");
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys), &[])
            .unwrap(),
        Value::Int(2),
        "SIZE=1 LINE=2 → c,d"
    );
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys + 1), &[])
            .unwrap(),
        Value::Int(3),
        "LINE=1 无 SIZE → 行数 = CR/LF + 1"
    );
}

/// `SAVEINFO`(`0x44e1c8`)与 `VAR`(`0x453f88`)是官方 **no-op**:
/// 守卫 `FUN_00429fd1` 后 `xor eax,eax; pop ecx; ret`,不读参数、不写 LET、
/// 不发域事件。REA 原始字节:`44e1e1 33 c0 59 c3`、`453fa1 33 c0 59 c3`。
#[test]
fn saveinfo_and_var_are_official_noops() {
    let groups = vec![
        // SAVEINFO 带 FILE/LET 参数:若被求值会触碰未声明变量并挂号。
        GroupSpec::new(OP_SAVEINFO)
            .window(0, ex::s("save_info.csv"))
            .window(3, ex::r#ref(0x40, 0x7fff)),
        GroupSpec::new(OP_VAR),
        GroupSpec::new(OP_END),
    ];
    let mut b = ProjectBuilder::new();
    b.scripts = vec![groups];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);

    let mut vm = boot(b);
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(r.commands_pumped, 3, "no-op 不得让出");
    assert!(
        vm.unimplemented_cmds().is_empty(),
        "SAVEINFO/VAR 不得再挂号: {:?}",
        vm.unimplemented_cmds()
    );
    assert!(
        !vm.events()
            .iter()
            .any(|e| matches!(e, yu_rt::VmEvent::DomainCall { .. })),
        "官方处理体不发域事件"
    );
    assert!(
        !vm.events()
            .iter()
            .any(|e| matches!(e, yu_rt::VmEvent::Diag { .. })),
        "不读参数 ⇒ 不产生参数诊断"
    );
}

// ---------------------------------------------------------------- DIALOG(P0-B/DIV-RT-044)
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
    /// v555 `SAVE[MEM]` 的槽号与 `LOAD[MEM]` 的 (槽号, DNO)。
    mem_saves: Vec<u32>,
    mem_loads: Vec<(u32, u32)>,
    mem_image_loads: Vec<(u32, u32)>,
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
            mem_saves: Vec::new(),
            mem_loads: Vec::new(),
            mem_image_loads: Vec::new(),
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
        self.inner.borrow_mut().order.push(format!("flush:{file}"));
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
    fn save_mem(&mut self, slot: u32) -> Result<(), Diag> {
        self.inner.borrow_mut().mem_saves.push(slot);
        Ok(())
    }
    fn load_block_mem(&mut self, slot: u32, dno: u32) -> Result<LoadedBlock, Diag> {
        self.inner.borrow_mut().mem_loads.push((slot, dno));
        let reply = self.reply.borrow_mut().take();
        reply.ok_or_else(|| Diag::fatal("test", yu_log::Boundary::Domain))
    }
    fn load_image_mem(
        &mut self,
        slot: u32,
        dno: u32,
    ) -> Result<yu_rt::event::CapturedImage, Diag> {
        self.inner.borrow_mut().mem_image_loads.push((slot, dno));
        Ok(sample_image())
    }
    fn reserve_image(&mut self, dno: u32, image: &yu_rt::event::CapturedImage) -> Result<(), Diag> {
        self.inner.borrow_mut().images.push((dno, image.clone()));
        Ok(())
    }
    fn load_image(&mut self, file: &str, dno: u32) -> Result<yu_rt::event::CapturedImage, Diag> {
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

    fn query(&mut self, _command: &str, _args: &[yu_rt::event::ArgValue]) -> Result<Value, Diag> {
        Err(Diag::fatal("test", yu_log::Boundary::Domain))
    }

    fn capture_layer(&mut self, id: &[u8]) -> Result<Option<yu_rt::event::CapturedImage>, Diag> {
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

/// `SAVE[ID IDNO]` / `LOAD[ID IDNO]`:层名按官方规则拼成 `ID.N`。
///
/// REA 0.495 `0x44c4aa..0x44c51c` / `0x4470d1..0x447145`:追加 `.`
/// (`0x808354`)后 `_ltoa(…,10)`;再交给 `0x4141e4` 路径规范化。
#[test]
fn save_load_idno_appends_dotted_suffix() {
    let mut b = ProjectBuilder::new();
    let op_save = b.commands.iter().position(|(n, _)| *n == "SAVE").unwrap() as u8;
    let op_load = b.commands.iter().position(|(n, _)| *n == "LOAD").unwrap() as u8;
    let op_saveend = b
        .commands
        .iter()
        .position(|(n, _)| *n == "SAVEEND")
        .unwrap() as u8;
    b.scripts = vec![vec![
        // SAVE[DNO=3 ID="THUMB" IDNO=3] → THUMB.3
        GroupSpec::new(op_save)
            .window(1, ex::i8_(3))
            .window(3, ex::s("THUMB"))
            .window(4, ex::i8_(3)),
        // LOAD[FILE="lay" DNO=3 ID="THUMB" IDNO=3] → THUMB.3
        GroupSpec::new(op_load)
            .window(0, ex::s("lay"))
            .window(1, ex::i8_(3))
            .window(3, ex::s("THUMB"))
            .window(4, ex::i8_(3)),
        GroupSpec::new(op_saveend),
        GroupSpec::new(OP_END),
    ]];
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

    let want = b"THUMB.3".to_vec();
    assert_eq!(captured.borrow().as_slice(), &[(want.clone(), sample_image())]);
    assert_eq!(restored.borrow().as_slice(), &[(want, sample_image())]);
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

/// `SAVE[DNO SET=@x[i][j]]`:块内容 = 该下标处的**标量元素**,不是整组。
///
/// 0.479 官方 save00001 DNO=2 实证:脚本目标 `@0x45d[0x8d][1]` 存成
/// Int 标量 14,而变量本身是 Int[256,128]。旧实现忽略 `lv.indices`、
/// 快照整组 → 块变 Int[256,128],官方 QLOAD 的严格维数比对失败
/// (`FUN_00440490` 0x440c22..),qsave 官方读不了。
#[test]
fn save_set_on_array_element_reserves_scalar_snapshot() {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![VarSpec {
        kind: 1,
        group: 0,
        script_id: 0,
        var_id: sys,
        value_type: 1,
        dims: vec![3, 4],
        init: Init::Int(5),
    }];
    let op_save = b.commands.iter().position(|(n, _)| *n == "SAVE").unwrap() as u8;
    let op_saveend = b
        .commands
        .iter()
        .position(|(n, _)| *n == "SAVEEND")
        .unwrap() as u8;
    // `@x[1][2]` 的真实字节形:ref, 1, 0x2c(组分隔), 2, 0x29(取值)。
    let elem = ex::cat(&[
        ex::r#ref(0x40, sys),
        ex::i8_(1),
        ex::op(0x2c),
        ex::i8_(2),
        ex::aload(),
    ]);
    let groups = vec![
        GroupSpec::new(op_save)
            .window(1, ex::i8_(2))
            .window(2, elem),
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
    assert_eq!(rec.reserves.len(), 1, "{:?}", rec.reserves);
    let (dno, var, snap) = &rec.reserves[0];
    assert_eq!(*dno, 2);
    assert_eq!(*var, VarRef::new(VarSpace::At, sys));
    assert!(
        snap.dims.is_empty(),
        "数组元素必须写标量块,实际 dims={:?}",
        snap.dims
    );
    assert_eq!(snap.data, vec![Value::Int(5)]);
}

/// `LOAD[LET=@x[i][j]]`:标量块只写该元素,其余元素保持原值。
/// 与 `save_set_on_array_element_reserves_scalar_snapshot` 成对;旧实现
/// 把整块写回整个变量(元素数不符 → 恢复被拒),qload 丢变量。
#[test]
fn load_into_array_element_writes_only_that_element() {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![VarSpec {
        kind: 1,
        group: 0,
        script_id: 0,
        var_id: sys,
        value_type: 1,
        dims: vec![3, 4],
        init: Init::Int(5),
    }];
    let op_load = b.commands.iter().position(|(n, _)| *n == "LOAD").unwrap() as u8;
    let elem = ex::cat(&[
        ex::r#ref(0x40, sys),
        ex::i8_(1),
        ex::op(0x2c),
        ex::i8_(2),
        ex::aload(),
    ]);
    let groups = vec![
        GroupSpec::new(op_load)
            .window(0, ex::s("elem.sd"))
            .window(1, ex::i8_(1))
            .window(2, elem),
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
        reply,
    }));
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let var = VarRef::new(VarSpace::At, sys);
    assert_eq!(vm.store().read(var, &[1, 2]).unwrap(), Value::Int(42));
    assert_eq!(vm.store().read(var, &[0, 0]).unwrap(), Value::Int(5));
    assert_eq!(vm.store().read(var, &[2, 3]).unwrap(), Value::Int(5));
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
        .expect("_LC sysvar") as u16;
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
            .window(4, ex::i8_(1))
            .window(7, ex::r#ref(0x40, sys)),
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


/// v555 `SAVE[MEM]` / `LOAD[MEM]`:MEM 是内存槽号,优先于 FILE,走宿主的
/// 内存槽契约而不是文件刷盘/读盘(REA <v555-4> `0x44b874`/`0x4447c8`)。
#[test]
fn save_and_load_mem_use_memory_slots_not_files() {
    let mut b = ProjectBuilder::new();
    // 555 参数表:在 FILE 后插入 MEM(后面的 DNO/SET/LET/ID/IDNO 顺移)。
    for name in ["SAVE", "LOAD"] {
        let params = &mut b
            .commands
            .iter_mut()
            .find(|(n, _)| *n == name)
            .expect("fixture command")
            .1;
        params.insert(1, ("MEM", 3, 0));
    }
    let op_int = b.commands.iter().position(|(n, _)| *n == "INT").unwrap() as u8;
    let op_save = b.commands.iter().position(|(n, _)| *n == "SAVE").unwrap() as u8;
    let op_load = b.commands.iter().position(|(n, _)| *n == "LOAD").unwrap() as u8;
    let op_saveend = b
        .commands
        .iter()
        .position(|(n, _)| *n == "SAVEEND")
        .unwrap() as u8;
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
    b.scripts = vec![vec![
        // INT[@x = 42]
        GroupSpec::new(op_int)
            .window(0, ex::r#ref(0x40, sys))
            .window(0, ex::i8_(42)),
        // SAVE[DNO=5 SET=@x] → 预约(走文件域的预约表)
        GroupSpec::new(op_save)
            .window(2, ex::i8_(5))
            .window(3, ex::r#ref(0x40, sys)),
        // SAVE[MEM=90001] → 写内存槽,不刷文件
        GroupSpec::new(op_save).window(1, ex::i32_(90_001)),
        GroupSpec::new(op_saveend),
        // INT[@x = 0]
        GroupSpec::new(op_int)
            .window(0, ex::r#ref(0x40, sys))
            .window(0, ex::i8_(0)),
        // LOAD[MEM=90001 DNO=5 LET=@x]
        GroupSpec::new(op_load)
            .window(1, ex::i32_(90_001))
            .window(2, ex::i8_(5))
            .window(3, ex::r#ref(0x40, sys)),
        GroupSpec::new(OP_END),
    ]];
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
        reply,
    }));
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);

    let rec = recorder.borrow();
    assert_eq!(rec.mem_saves, vec![90_001], "SAVE[MEM] 走内存槽");
    assert_eq!(
        rec.mem_loads,
        vec![(90_001, 5)],
        "LOAD[MEM] 走内存槽且 DNO 原样传给宿主"
    );
    assert!(rec.flushes.is_empty(), "MEM 分支不得退化成文件刷盘");
    assert!(rec.loads.is_empty(), "MEM 分支不得退化成文件读盘");
    drop(rec);
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys), &[])
            .unwrap(),
        Value::Int(42),
        "LOAD[MEM] 的块类型 0 写回 LET 目标"
    );
}
