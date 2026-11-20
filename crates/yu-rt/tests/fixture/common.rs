//! 共享夹具:工程构建、boot、OP 常量与跨域小工具。
//!
//! 由 `tests/fixture.rs` 拆分而来;测试名与断言不变。

pub(crate) use crate::support::{ex, with_int_var, with_v474_extra_slot, GroupSpec, Init, ProjectBuilder, VarSpec};
pub(crate) use yu_rt::input::{InputFrame, InputTimeline};
pub(crate) use yu_rt::json::to_jsonl;
pub(crate) use yu_rt::task::MAIN_TASK_ID;
pub(crate) use yu_rt::value::{Value, VarRef, VarSpace};
pub(crate) use yu_rt::{BootConfig, HostLocalTime, Vm};

/// 与 0.495 `ysc.ybn` 一致的 opcode(夹具命令表同序)。
pub(crate) const OP_END: u8 = 14;
pub(crate) const OP_FILEINFO: u8 = 22;
pub(crate) const OP_FPS: u8 = 29;
pub(crate) const OP_GO: u8 = 43;
pub(crate) const OP_GOSUB: u8 = 44;
pub(crate) const OP_IF: u8 = 45;
pub(crate) const OP_IFBLEND: u8 = 46;
pub(crate) const OP_IFEND: u8 = 49;
pub(crate) const OP_INT: u8 = 51;
pub(crate) const OP_LET: u8 = 54;
pub(crate) const OP_LABELINFO: u8 = 53;
pub(crate) const OP_LOOP: u8 = 56;
pub(crate) const OP_LOOPEND: u8 = 59;
pub(crate) const OP_RETURN: u8 = 77;
pub(crate) const OP_STR: u8 = 90;
pub(crate) const OP_TASKINFO: u8 = 95;
pub(crate) const OP_VARACT: u8 = 100;
pub(crate) const OP_WAIT: u8 = 102;
pub(crate) const OP_MATH: u8 = 61;
pub(crate) const OP_SAVEINFO: u8 = 86;
pub(crate) const OP_VAR: u8 = 99;

pub(crate) fn no_input() -> InputTimeline {
    InputTimeline::new()
}

pub(crate) fn boot(builder: ProjectBuilder) -> Vm {
    let src = builder.build();
    Vm::boot(Box::new(src), BootConfig::default()).expect("boot")
}

/// `SemanticProfile` 从工程 `ysc.ybn` 头派生一次,不是宿主猜的引擎版本。
/// 夹具默认头版本 = 495 ⇒ `SJISCODE` 保持 0.495 的惰性形态。
#[test]
pub(crate) fn boot_derives_semantic_profile_from_project_tables() {
    let mut b = ProjectBuilder::new();
    b.scripts = vec![vec![GroupSpec::new(OP_END)]];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    let vm = boot(b);
    let profile = vm.semantics();
    assert_eq!(profile.ysc_version, 495);
    assert_eq!(
        profile.varinfo_sjis,
        yu_rt::semantics::SjisCodeMode::Inert,
        "0.495 的 SJISCODE 是惰性参数"
    );
}

/// 声明变量 id:夹具里用户变量从 sys_count 起。
pub(crate) fn user_var(vm: &Vm, offset: u16) -> VarRef {
    let sys = vm.sysvars().len() as u16;
    VarRef::new(VarSpace::At, sys + offset)
}

/// 按 `b.vars` 的当前顺序追加一个用户变量(offset = 追加序号)。
pub(crate) fn push_var(b: &mut ProjectBuilder, value_type: u8, init: Init) -> VarRef {
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

/// 在合成工程上再插入数据文件(封包/散盘等价;`MemSource` 是同一 VFS 接口)。
pub(crate) fn boot_with_files(builder: ProjectBuilder, files: &[(&[u8], &[u8])]) -> Vm {
    let mut src = builder.build();
    for (path, bytes) in files {
        src.insert(path, bytes.to_vec())
            .expect("insert fixture file");
    }
    Vm::boot(Box::new(src), BootConfig::default()).expect("boot")
}

pub(crate) fn opcode_of(b: &ProjectBuilder, name: &str) -> u8 {
    b.commands
        .iter()
        .position(|c| c.0 == name)
        .expect("fixture command present") as u8
}

#[test]
pub(crate) fn missing_table_refuses_boot() {
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
pub(crate) fn event_stream_is_byte_stable() {
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

/// 0.495 `ysc.ybn` 实测序号(dump_ycd):79 RETURNCODE / 106 WORD / 110 `_`。
pub(crate) const OP_RETURNCODE: u8 = 79;
pub(crate) const OP_TEXT: u8 = 96;
pub(crate) const OP_TEXTINFO: u8 = 97;
pub(crate) const OP_WORD: u8 = 106;
pub(crate) const OP_TEXT_OUT: u8 = 110;

/// R10 回归:一帧事件以 `FrameStart` 开始、`FrameEnd` 结束。
#[test]
pub(crate) fn frame_events_are_bounded_by_start_and_end() {
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
pub(crate) fn event_queue_is_bounded_and_reports_drops() {
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
pub(crate) fn script_diag_carries_task_script_and_pc() {
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

/// YSVR 字符串初值是 YSTB 字面量形态(`M` + u16 + 引号串),
/// 必须在装载时解码;否则 `$es.STARTLABEL` 会带着 `M\x10\x00"` 前缀。
#[test]
pub(crate) fn ysvr_string_initial_value_is_decoded() {
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

pub(crate) const OP_DEBUGLIST: u8 = 9;
pub(crate) const OP_DIALOG: u8 = 10;
pub(crate) const OP_F_INT: u8 = 18;
pub(crate) const OP_LABEL: u8 = 52;
pub(crate) const OP_PREP: u8 = 75;
pub(crate) const OP_RETURNBREAK: u8 = 78;
pub(crate) const OP_S_INT: u8 = 81;

pub(crate) use yu_log::Diag;
pub(crate) use yu_rt::event::{LoadedBlock, LoadedVar, SaveHost, VarSnapshot};

