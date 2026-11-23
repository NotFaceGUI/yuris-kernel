//! v555 `TEXT[FILE]` + 运行时翻译:行内台词/裸文本在写 LET 前重写。
//!
//! 合成工程走真实 `TEXT[FILE]` 路径(精确 555 独立运行面),验证:
//! - `\LC("…")` 宏只替换引号内文本;
//! - 裸文本行整行替换;
//! - 其余行原样保留;
//! - 命中统计与文件/行号坐标一致。

#[path = "support/mod.rs"]
mod support;

use support::{ex, GroupSpec, Init, ProjectBuilder, VarSpec};
use yu_format::scenario::UnitKind;
use yu_rt::input::InputTimeline;
use yu_rt::translation::{ScenarioUnit, TranslationTable};
use yu_rt::value::{Value, VarRef, VarSpace};
use yu_rt::{BootConfig, Vm};

const OP_END: u8 = 14;
const OP_WORD: u8 = 106;

fn boot(b: ProjectBuilder) -> Vm {
    let mut src = b.build();
    src.insert(b"scn\\a.txt", b"\\LC(\"AA\")\r\nplain text\r\n".to_vec())
        .expect("insert scenario text");
    let mut table = TranslationTable::default();
    table.insert_scenario(
        "scn/a.txt",
        1,
        vec![ScenarioUnit {
            seq: 0,
            kind: UnitKind::Macro,
            name: b"LC".to_vec(),
            bytes: b"BB".to_vec(),
        }],
    );
    table.insert_scenario(
        "scn/a.txt",
        2,
        vec![ScenarioUnit {
            seq: 0,
            kind: UnitKind::Text,
            name: Vec::new(),
            bytes: b"CC".to_vec(),
        }],
    );
    let mut vm = Vm::boot(
        Box::new(src),
        BootConfig {
            translation: Some(table),
            ..BootConfig::default()
        },
    )
    .expect("boot");
    vm.install_other_runtime();
    let report = vm.tick_at(0, &InputTimeline::new());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    vm
}

#[test]
fn text_file_rewrites_macro_and_bare_lines() {
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
        GroupSpec::new(OP_WORD).window(0, b"OLD".to_vec()),
        GroupSpec::new(op_text)
            .window(0, ex::sb(b"scn/a.txt"))
            .window(1, ex::r#ref(0x24, sys))
            .window(2, ex::i32_(1)),
        GroupSpec::new(OP_END),
    ]];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    let vm = boot(b);

    let arr = VarRef::new(VarSpace::Dollar, sys);
    assert_eq!(
        vm.store().read(arr, &[0]).unwrap(),
        Value::Str(b"\\LC(\"BB\")\r".to_vec()),
        "第 1 行台词宏只替换引号内文本"
    );
    assert_eq!(
        vm.store().read(arr, &[1]).unwrap(),
        Value::Str(b"CC\r".to_vec()),
        "第 2 行裸文本整行替换"
    );
    let stats = vm.translation_stats().expect("翻译统计");
    assert_eq!(stats.scenario_line_hits, 2);
    assert_eq!(stats.scenario_units, 2);
    assert_eq!(stats.scenario_rewrite_errors, 0);
}

#[test]
fn text_file_without_translation_keeps_bytes() {
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
            .window(1, ex::r#ref(0x24, sys))
            .window(2, ex::i32_(0)),
        GroupSpec::new(OP_END),
    ]];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    let mut src = b.build();
    src.insert(b"scn\\a.txt", b"\\LC(\"AA\")\r\nplain text\r\n".to_vec())
        .expect("insert scenario text");
    let mut vm = Vm::boot(Box::new(src), BootConfig::default()).expect("boot");
    vm.install_other_runtime();
    let _ = vm.tick_at(0, &InputTimeline::new());
    let arr = VarRef::new(VarSpace::Dollar, sys);
    assert_eq!(
        vm.store().read(arr, &[0]).unwrap(),
        Value::Str(b"\\LC(\"AA\")\r".to_vec())
    );
    assert_eq!(
        vm.store().read(arr, &[1]).unwrap(),
        Value::Str(b"plain text\r".to_vec())
    );
}
