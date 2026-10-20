//! `CGINFO/FONTINFO/WINDOWINFO` 同步查询(供 VM 的 `LET` 写回)。
//!
//! 只测手册明确且状态足以回答的字段;需要资产元数据/字体列表/鼠标/像素的
//! 字段必须返回 Unimplemented,不能猜 0。

mod support;

use support::*;
use yu_rt::value::Value;

fn q(gfx: &yu_gfx::GfxState, command: &str, args: Vec<yu_rt::event::ArgValue>) -> Value {
    gfx.query_info(command, &args).expect("query")
}

fn q_err(gfx: &yu_gfx::GfxState, command: &str, args: Vec<yu_rt::event::ArgValue>) -> yu_log::Diag {
    gfx.query_info(command, &args)
        .expect_err("must be unimplemented")
}

#[test]
fn cginfo_answers_documented_attributes() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("L")),
                arg(&table, "CG", "FILE", s("l")),
                arg(&table, "CG", "A", i(128)),
                arg(&table, "CG", "X", i(10)),
                arg(&table, "CG", "Y", i(20)),
                arg(&table, "CG", "Z", i(7)),
                arg(&table, "CG", "MODE", i(2)),
                arg(&table, "CG", "E", i(0)),
                arg(&table, "CG", "T", i(100)),
                arg(&table, "CG", "TA", i(64)),
                arg(&table, "CG", "TID", s("SRC")),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("SRC")),
                arg(&table, "CG", "FILE", s("src")),
            ],
        ),
        frame_end(1),
    ]);
    let info = |flag: &str| {
        q(
            &gfx,
            "CGINFO",
            vec![
                arg(&table, "CGINFO", "ID", s("L")),
                arg(&table, "CGINFO", flag, i(1)),
            ],
        )
    };
    assert_eq!(info("EXIST"), Value::Int(1));
    assert_eq!(info("E"), Value::Int(0));
    assert_eq!(info("A"), Value::Int(128));
    assert_eq!(info("X"), Value::Int(10));
    assert_eq!(info("Y"), Value::Int(20));
    assert_eq!(info("Z"), Value::Int(7));
    assert_eq!(info("MODE"), Value::Int(2));
    assert_eq!(info("T"), Value::Int(100));
    assert_eq!(info("TA"), Value::Int(64));
    // CGINFO[TID] 返回脚本原样字符串(引擎 +0x4c 保存原文,不是规范化路径)。
    assert_eq!(info("TID"), Value::Str(b"SRC".to_vec()));
    // 资源图尺寸需要资产元数据:必须显式 Unimplemented。
    let err = q_err(
        &gfx,
        "CGINFO",
        vec![
            arg(&table, "CGINFO", "ID", s("L")),
            arg(&table, "CGINFO", "SX", i(1)),
        ],
    );
    assert_eq!(err.code, "gfx.info.asset_size");
}

#[test]
fn cginfo_group_and_solid_black_sizes_are_known() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("G/")),
                arg(&table, "CG", "SX", i(400)),
                arg(&table, "CG", "SY", i(300)),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("BLACK")),
                arg(&table, "CG", "FILE", s("")),
                arg(&table, "CG", "SX", i(800)),
                arg(&table, "CG", "SY", i(600)),
            ],
        ),
        frame_end(1),
    ]);
    let size = |id: &str, flag: &str| {
        q(
            &gfx,
            "CGINFO",
            vec![
                arg(&table, "CGINFO", "ID", s(id)),
                arg(&table, "CGINFO", flag, i(1)),
            ],
        )
    };
    assert_eq!(size("G/", "SX"), Value::Int(400));
    assert_eq!(size("G/", "SY"), Value::Int(300));
    assert_eq!(size("BLACK", "SX"), Value::Int(800));
    assert_eq!(size("BLACK", "SY"), Value::Int(600));
}

#[test]
fn cginfo_missing_layer_only_answers_exist() {
    let table = table();
    let gfx = state();
    assert_eq!(
        q(
            &gfx,
            "CGINFO",
            vec![
                arg(&table, "CGINFO", "ID", s("NOPE")),
                arg(&table, "CGINFO", "EXIST", i(1)),
            ],
        ),
        Value::Int(0)
    );
    let err = q_err(
        &gfx,
        "CGINFO",
        vec![
            arg(&table, "CGINFO", "ID", s("NOPE")),
            arg(&table, "CGINFO", "A", i(1)),
        ],
    );
    assert_eq!(err.code, "gfx.info.missing");
}

#[test]
fn cginfo_multiple_fields_are_rejected_not_guessed() {
    let table = table();
    let gfx = state();
    let err = q_err(
        &gfx,
        "CGINFO",
        vec![
            arg(&table, "CGINFO", "ID", s("X")),
            arg(&table, "CGINFO", "A", i(1)),
            arg(&table, "CGINFO", "E", i(1)),
        ],
    );
    assert_eq!(err.code, "gfx.info.multi");
}

#[test]
fn fontinfo_answers_current_style_fields() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "FONT",
            vec![
                arg(&table, "FONT", "NAME", s("TestFont")),
                arg(&table, "FONT", "COLOR", i(0x00ff_ff00)),
                arg(&table, "FONT", "COLOR2", i(0x0000_00ff)),
                arg(&table, "FONT", "SX", i(22)),
                arg(&table, "FONT", "SY", i(24)),
                arg(&table, "FONT", "BOLD", i(1)),
                arg(&table, "FONT", "ANTIALIASING", i(0)),
                arg(&table, "FONT", "DRAWMODE", i(1)),
            ],
        ),
        frame_end(1),
    ]);
    let info = |flag: &str| q(&gfx, "FONTINFO", vec![arg(&table, "FONTINFO", flag, i(1))]);
    assert_eq!(info("COLOR"), Value::Int(0x00ff_ff00));
    assert_eq!(info("COLOR2"), Value::Int(0x0000_00ff));
    assert_eq!(info("SX"), Value::Int(22));
    assert_eq!(info("SY"), Value::Int(24));
    assert_eq!(info("BOLD"), Value::Int(1));
    assert_eq!(info("ANTIALIASING"), Value::Int(0));
    assert_eq!(info("DRAWMODE"), Value::Int(1));
    assert_eq!(info("NAME"), Value::Str(b"TestFont".to_vec()));

    let err = q_err(
        &gfx,
        "FONTINFO",
        vec![
            arg(&table, "FONTINFO", "NAME", i(1)),
            arg(&table, "FONTINFO", "NO", i(3)),
        ],
    );
    assert_eq!(err.code, "gfx.info.unsupported");
}

#[test]
fn fontinfo_unset_fields_are_not_fabricated() {
    let table = table();
    let gfx = state();
    for flag in ["COLOR2", "NAME"] {
        let err = q_err(&gfx, "FONTINFO", vec![arg(&table, "FONTINFO", flag, i(1))]);
        assert_eq!(err.code, "gfx.info.unsupported", "{flag}");
    }
}

#[test]
fn windowinfo_answers_main_geometry() {
    let table = table();
    let mut gfx = state();
    let info = |gfx: &yu_gfx::GfxState, flag: &str, no: i64| {
        q(
            gfx,
            "WINDOWINFO",
            vec![
                arg(&table, "WINDOWINFO", "NO", i(no)),
                arg(&table, "WINDOWINFO", flag, i(1)),
            ],
        )
    };
    assert_eq!(info(&gfx, "EXIST", 0), Value::Int(1));
    assert_eq!(info(&gfx, "SX", 0), Value::Int(800));
    assert_eq!(info(&gfx, "SY", 0), Value::Int(600));
    assert_eq!(info(&gfx, "E", 0), Value::Int(1));

    gfx.consume_events(&[
        frame_start(1),
        window_call(
            1,
            "WINDOW",
            vec![
                arg(&table, "WINDOW", "NO", i(0)),
                arg(&table, "WINDOW", "E", i(0)),
                arg(&table, "WINDOW", "X", i(11)),
                arg(&table, "WINDOW", "Y", i(22)),
                arg(&table, "WINDOW", "SX", i(640)),
                arg(&table, "WINDOW", "SY", i(480)),
            ],
        ),
        frame_end(1),
    ]);
    assert_eq!(info(&gfx, "E", 0), Value::Int(0));
    assert_eq!(info(&gfx, "X", 0), Value::Int(11));
    assert_eq!(info(&gfx, "Y", 0), Value::Int(22));
    assert_eq!(info(&gfx, "SX", 0), Value::Int(640));
    assert_eq!(info(&gfx, "SY", 0), Value::Int(480));
    assert_eq!(info(&gfx, "EXIST", 1), Value::Int(0));

    let err = q_err(
        &gfx,
        "WINDOWINFO",
        vec![
            arg(&table, "WINDOWINFO", "NO", i(0)),
            arg(&table, "WINDOWINFO", "CAPTION", i(1)),
        ],
    );
    assert_eq!(err.code, "gfx.info.unsupported");
}
