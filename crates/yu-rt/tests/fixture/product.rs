//! v555 `PRODUCT`:COM=29 模式位、COM=21 VFS 枚举与 `_DROPFILE*`。

use super::common::*;

/// v555 `PRODUCT[COM=29, LET=@x]`:把模式位(BSS 初值 0)写入 LET。
/// 其余 Win32 工具分支不得把 LET 猜成非零值。
#[test]
#[ignore = "v555 other 模块: PRODUCT 路由已按精确 555 门控,用例随 other/handlers/product 迁入"]
fn product_com29_writes_mode_bit_zero_to_let() {
    let mut b = ProjectBuilder::new();
    b.commands
        .push(("PRODUCT", vec![("COM", 1, 0), ("LET", 0, 0)]));
    let op_product = b
        .commands
        .iter()
        .position(|(n, _)| *n == "PRODUCT")
        .unwrap() as u8;
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
    b.scripts = vec![vec![
        GroupSpec::new(op_product)
            .window(0, ex::i8_(29))
            .window(1, ex::r#ref(0x40, sys)),
        GroupSpec::new(OP_END),
    ]];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    let mut vm = boot(b);
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys), &[])
            .unwrap(),
        Value::Int(0),
        "COM=29 = DAT_00872410 初值 0"
    );
}

/// v555 `PRODUCT[COM=21, PSTR="scn/"]`:按 VFS 目录枚举文件,填充
/// `_DROPFILE`/`_DROPFILE_NUM`(555 场景列表的数据来源)。
#[test]
#[ignore = "v555 other 模块: PRODUCT 路由已按精确 555 门控,用例随 other/handlers/product 迁入"]
fn product_com21_enumerates_vfs_directory_into_dropfiles() {
    let mut b = ProjectBuilder::new();
    b.commands
        .push(("PRODUCT", vec![("COM", 1, 0), ("PSTR", 1, 0)]));
    let op_product = b.commands.iter().position(|(n, _)| *n == "PRODUCT").unwrap() as u8;
    b.scripts = vec![vec![
        GroupSpec::new(op_product)
            .window(0, ex::i8_(21))
            .window(1, ex::sb(b"scn/")),
        GroupSpec::new(OP_END),
    ]];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    let mut vm = boot_with_files(
        b,
        &[
            (b"scn/a.txt", b"a"),
            (b"scn/b.txt", b"b"),
            (b"scn/sub/c.txt", b"c"),
        ],
    );
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(vm.drop_files_len(), 2, "只枚举直接子文件,不递归");
    assert_eq!(vm.drop_file(0), Some(b"a.txt".as_slice()));
    assert_eq!(vm.drop_file(1), Some(b"b.txt".as_slice()));
}

/// v555 `PRODUCT[COM=12]`:官方 `FUN_0045a70c` 复制文件。内核映射成
/// `FILEACT[COPY]` 域事件(save_host 在命令点同步消费),不写 LET。
#[test]
#[ignore = "v555 other 模块: PRODUCT 路由已按精确 555 门控,用例随 other/handlers/product 迁入"]
fn product_com12_emits_fileact_copy_event() {
    use yu_rt::event::{Domain, VmEvent};

    let mut b = ProjectBuilder::new();
    b.commands
        .push(("PRODUCT", vec![("COM", 1, 0), ("PSTR", 1, 0), ("PSTR2", 1, 0)]));
    // 夹具默认 FILEACT 只有 FILE;COM=12 需要 FILE/FILE2/COPY 三槽。
    let fileact = b
        .commands
        .iter()
        .position(|(n, _)| *n == "FILEACT")
        .expect("fixture FILEACT");
    b.commands[fileact].1 = vec![("FILE", 1, 0), ("FILE2", 1, 0), ("COPY", 0, 0)];
    let op_product = (b.commands.len() - 1) as u8;
    b.scripts = vec![vec![
        GroupSpec::new(op_product)
            .window(0, ex::i8_(12))
            .window(1, ex::sb(b"src/a.bin"))
            .window(2, ex::sb(b"dst/b.bin")),
        GroupSpec::new(OP_END),
    ]];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    let mut vm = boot(b);
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let mut events = Vec::new();
    vm.drain_events(&mut events);
    let ev = events
        .iter()
        .find(|e| {
            matches!(
                e,
                VmEvent::DomainCall {
                    domain: Domain::File,
                    name,
                    ..
                } if name == "FILEACT"
            )
        })
        .expect("COM=12 应发出 FILEACT 域事件");
    let args: Vec<(String, Value)> = ev
        .domain_args(&vm.tables().boot.yscm)
        .expect("FILEACT domain args")
        .into_iter()
        .map(|a| (a.name.into_owned(), a.value.clone()))
        .collect();
    let get = |name: &str| {
        args.iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| panic!("missing {name}: {args:?}"))
    };
    assert_eq!(get("FILE"), Value::Str(b"src\\a.bin".to_vec()));
    assert_eq!(get("FILE2"), Value::Str(b"dst\\b.bin".to_vec()));
    assert_eq!(get("COPY"), Value::Int(1));
}

/// v555 `PRODUCT[COM=34]`:官方 `0x45c1d0` = GetDiskFreeSpaceExA,结果写 LET。
/// 宿主注入的剩余空间必须原样落到脚本变量;未注入才写 0。
#[test]
#[ignore = "v555 other 模块: PRODUCT 路由已按精确 555 门控,用例随 other/handlers/product 迁入"]
fn product_com34_writes_host_disk_free_bytes() {
    let mut b = ProjectBuilder::new();
    b.commands
        .push(("PRODUCT", vec![("COM", 1, 0), ("LET", 0, 0)]));
    let op_product = (b.commands.len() - 1) as u8;
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
    b.scripts = vec![vec![
        GroupSpec::new(op_product)
            .window(0, ex::i8_(34))
            .window(1, ex::r#ref(0x40, sys)),
        GroupSpec::new(OP_END),
    ]];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    let cfg = yu_rt::BootConfig {
        disk_free_bytes: Some(123_456_789),
        ..Default::default()
    };
    let mut vm = yu_rt::Vm::boot(Box::new(b.build()), cfg).expect("boot");
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(
        vm.store().read(VarRef::new(VarSpace::At, sys), &[]).unwrap(),
        Value::Int(123_456_789)
    );
}

/// v555 `PRODUCT[COM=23]`:官方 `FUN_0045bcf8` 遍历源字符串数组,把每行
/// 长度写入 PINT 目标数组,非空行数写入 LET。旧实现只写 `last_text_lines`。
#[test]
#[ignore = "v555 other 模块: PRODUCT 已迁入 other/handlers,用例待随 other 测试安装运行面"]
fn product_com23_writes_line_lengths_and_count() {
    let mut b = ProjectBuilder::new();
    b.ysc_version = 555;
    b.commands.push((
        "PRODUCT",
        vec![
            ("COM", 1, 0),
            ("LET", 0, 0),
            ("PINT", 1, 0),
            ("PINT2", 1, 0),
        ],
    ));
    let op_product = (b.commands.len() - 1) as u8;
    let sys = b.sysvars.len() as u16;
    b.vars = vec![
        // 源字符串数组
        VarSpec {
            kind: 1,
            group: 0,
            script_id: 0,
            var_id: sys,
            value_type: 3,
            dims: vec![4],
            init: Init::Str(b""),
        },
        // 目标整数数组
        VarSpec {
            kind: 1,
            group: 0,
            script_id: 0,
            var_id: sys + 1,
            value_type: 1,
            dims: vec![4],
            init: Init::Int(0),
        },
        // LET 计数
        VarSpec {
            kind: 1,
            group: 0,
            script_id: 0,
            var_id: sys + 2,
            value_type: 1,
            dims: vec![],
            init: Init::Int(-1),
        },
    ];
    b.scripts = vec![vec![
        GroupSpec::new(op_product)
            .window(0, ex::i8_(23))
            .window(1, ex::r#ref(0x40, sys + 2))
            .window(2, ex::i32_(i32::from(sys + 1)))
            .window(3, ex::i32_(i32::from(sys))),
        GroupSpec::new(OP_END),
    ]];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    let mut vm = boot(b);
    let src = VarRef::new(VarSpace::Dollar, sys);
    for (i, s) in [b"abc".as_slice(), b"de", b"f"].iter().enumerate() {
        vm.store_mut()
            .write(src, &[i as i64], &Value::Str(s.to_vec()))
            .unwrap();
    }
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let dst = VarRef::new(VarSpace::At, sys + 1);
    for (i, n) in [3i64, 2, 1].iter().enumerate() {
        assert_eq!(
            vm.store().read(dst, &[i as i64]).unwrap(),
            Value::Int(*n),
            "第 {i} 行长度"
        );
    }
    assert_eq!(
        vm.store().read(VarRef::new(VarSpace::At, sys + 2), &[]).unwrap(),
        Value::Int(3),
        "非空行数"
    );
}
