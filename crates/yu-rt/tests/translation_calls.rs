//! `GOSUB` 字符串参数的运行时翻译(角色名/选项入口)。
//!
//! 合成 v555 工程:`GOSUB("SUB", PSTR1="HELLO")` 命中调用点 `(s0,pc0,arg0)`
//! 后,被调方通过 `$_PSTR(1)` 读到的必须是译文。

#[path = "support/mod.rs"]
mod support;

use support::{ex, GroupSpec, Init, ProjectBuilder, VarSpec};
use yu_rt::input::InputTimeline;
use yu_rt::translation::TranslationTable;
use yu_rt::value::{Value, VarRef, VarSpace};
use yu_rt::{BootConfig, Vm};

const OP_END: u8 = 14;
const OP_GOSUB: u8 = 44;
const OP_LET: u8 = 54;
const OP_RETURN: u8 = 77;

#[test]
fn gosub_string_arg_is_replaced_at_call_site() {
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
        // 3: LET[$out[0] = $_PSTR(1)]; RETURN
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x24, sys))
            .window(0, ex::cat(&[ex::r#ref(0x24, 0), ex::i8_(1), ex::aload()])),
        GroupSpec::new(OP_RETURN),
    ]];
    // 系统变量 `_PSTR` 的实际 id 由工程 ABI 决定(夹具表序号可能不同)。
    let probe_src = {
        let mut p = ProjectBuilder::new();
        p.sysvars = b.sysvars.clone();
        p.build()
    };
    let tables = yu_rt::TableSet::load(&probe_src, yu_rt::tables::DEFAULT_SCRIPT_KEY).unwrap();
    let pstr = tables.boot.sysvars.id("_PSTR").expect("_PSTR in ABI");
    b.scripts[0][3].windows[1].4 = ex::cat(&[ex::r#ref(0x24, pstr), ex::i8_(1), ex::aload()]);

    let mut table = TranslationTable::default();
    table.insert_call(0, 0, 0, b"WORLD".to_vec());
    let mut vm = Vm::boot(
        Box::new(b.build()),
        BootConfig {
            translation: Some(table),
            ..BootConfig::default()
        },
    )
    .expect("boot");
    vm.install_other_runtime();
    let report = vm.tick_at(0, &InputTimeline::new());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::Dollar, sys), &[0])
            .unwrap(),
        Value::Str(b"WORLD".to_vec()),
        "被调方读到的 PSTR1 必须是译文"
    );
    let stats = vm.translation_stats().expect("翻译统计");
    assert_eq!(stats.call_hits, 1);
    assert_eq!(stats.call_misses, 0);
}

#[test]
fn gosub_string_literal_is_replaced_by_window_segment() {
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
        // 0: GOSUB[#="SUB" PSTR1="HELLO"];窗口 1 的段 0 是 HELLO。
        GroupSpec::new(OP_GOSUB)
            .window(0, ex::s("SUB"))
            .window(33, ex::sb(b"HELLO")),
        GroupSpec::new(OP_END),
        GroupSpec::new(OP_END),
        // 3: LET[$out[0] = $_PSTR(1)]; RETURN
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x24, sys))
            .window(0, ex::cat(&[ex::r#ref(0x24, 0), ex::i8_(1), ex::aload()])),
        GroupSpec::new(OP_RETURN),
    ]];
    let probe_src = {
        let mut p = ProjectBuilder::new();
        p.sysvars = b.sysvars.clone();
        p.build()
    };
    let tables = yu_rt::TableSet::load(&probe_src, yu_rt::tables::DEFAULT_SCRIPT_KEY).unwrap();
    let pstr = tables.boot.sysvars.id("_PSTR").expect("_PSTR in ABI");
    b.scripts[0][3].windows[1].4 = ex::cat(&[ex::r#ref(0x24, pstr), ex::i8_(1), ex::aload()]);

    let mut table = TranslationTable::default();
    table.insert_literal(0, 0, 1, 0, b"WORLD".to_vec());
    let mut vm = Vm::boot(
        Box::new(b.build()),
        BootConfig {
            translation: Some(table),
            ..BootConfig::default()
        },
    )
    .expect("boot");
    vm.install_other_runtime();
    let report = vm.tick_at(0, &InputTimeline::new());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::Dollar, sys), &[0])
            .unwrap(),
        Value::Str(b"WORLD".to_vec()),
        "文本常量窗口 1 段 0 必须在求值前替换"
    );
    let stats = vm.translation_stats().expect("翻译统计");
    assert_eq!(stats.literal_hits, 1);
    assert_eq!(stats.literal_misses, 0);
}
