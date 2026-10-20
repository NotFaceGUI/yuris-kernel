//! 合成命令表 + 事件构造器(不依赖真实语料)。
//!
//! 槽位顺序逐字取自 0.495 ERIS 的 `ysc.ybn`(`dump_yscm` 输出),
//! 但测试只通过**参数名**构造事件,避免在断言里写死槽号。

#![allow(dead_code)]

use yu_format::yscm::{Arg as FmtArg, Command as FmtCommand, CommandTable};
use yu_gfx::{GfxConfig, GfxState, LayerId, ParamTable, Viewport};
use yu_rt::event::{ArgValue, Domain, VmEvent};
use yu_rt::tables::YscmTable;
use yu_rt::value::Value;

fn cmd(name: &str, params: &[&str]) -> FmtCommand {
    FmtCommand {
        name: name.into(),
        argc: params.len() as u8,
        args: params
            .iter()
            .map(|p| FmtArg {
                name: (*p).into(),
                flag: 0,
                kind: 0,
            })
            .collect(),
    }
}

fn string_cmd(name: &str, params: &[&str]) -> FmtCommand {
    FmtCommand {
        name: name.into(),
        argc: params.len() as u8,
        args: params
            .iter()
            .map(|p| FmtArg {
                name: (*p).into(),
                flag: if matches!(
                    *p,
                    "ID" | "ID2" | "ID3" | "ID4" | "FILE" | "MID" | "TID" | "NAME" | "SETSTR"
                ) {
                    1
                } else {
                    0
                },
                kind: 0,
            })
            .collect(),
    }
}

/// 与 ERIS 0.495 同序的图形域命令表。
pub fn table() -> ParamTable {
    build_table(command_list())
}

/// 缺一条命令的变体(用于 fail-fast 回归)。
pub fn table_without(name: &str) -> ParamTable {
    let mut commands = command_list();
    commands.retain(|c| c.name.as_ref() != name);
    build_table(commands)
}

fn build_table(commands: Vec<FmtCommand>) -> ParamTable {
    let fmt = CommandTable {
        version: 495,
        commands,
        messages: Vec::new(),
        key_table: Box::new([0u8; 256]),
    };
    ParamTable::new(YscmTable::from_format(fmt).expect("synthetic yscm"))
}

fn command_list() -> Vec<FmtCommand> {
    vec![
        string_cmd(
            "CG",
            &[
                "ID", "IDNO", "E", "A", "X", "Y", "Z", "SX", "SY", "TSX", "TSY", "MIPMAP", "TEX",
                "DXBUF", "DXDRAW", "FX", "FY", "FBX", "FBY", "FRX", "FRY", "FRZ", "FCX", "FCY",
                "FEX", "FEY", "FEZ", "FSD", "FMD", "FQU", "FID", "F", "REMALLOC", "FILE", "RID",
                "MID", "MFILE", "T", "TID", "TIDNO", "TA", "MODE", "CASH", "LINT", "LINT2",
            ],
        ),
        string_cmd(
            "CGACT",
            &[
                "ID",
                "ID2",
                "ID3",
                "ID4",
                "IDNO",
                "IDNO2",
                "IDNO3",
                "IDNO4",
                "Z2",
                "SET",
                "SET2",
                "SETSTR",
                "X",
                "Y",
                "X2",
                "Y2",
                "SX",
                "SY",
                "FX",
                "FY",
                "FX2",
                "FY2",
                "FSX",
                "FSY",
                "SIZE",
                "SIZE2",
                "SIZE3",
                "SIZE4",
                "SIZE5",
                "ROTATION",
                "ROTATION2",
                "SAIDO",
                "MEIDO",
                "SEPIA",
                "MERGE",
                "MERGE2",
                "TRANSITION",
                "TRANSITION2",
                "XOR",
                "AND",
                "TRIM",
                "RESIZE",
                "AKARUSA",
                "MONO",
                "VERTICAL",
                "HORIZONTAL",
                "MOSAIC",
                "BLUR",
                "BLUR2",
                "BLUR3",
                "BLUR4",
                "BLUR5",
                "BLUR6",
                "DIFFUSION",
                "RASTER",
                "RASTER2",
                "RASTER3",
                "REVERSE",
                "RECTPAINT",
                "RECTPAINT2",
                "RECTPAINT3",
                "WHIRLPOOL",
                "BNO",
                "TEXT",
                "COPY",
                "COPY2",
                "SWAP",
                "SWAP2",
                "TEXCOPY",
                "TEXSSCOPY",
            ],
        ),
        string_cmd("CGEND", &["ID", "IDNO"]),
        string_cmd(
            "WINDOW",
            &[
                "NO",
                "CAPTION",
                "A",
                "E",
                "X",
                "Y",
                "Z",
                "SX",
                "SY",
                "ACTIVE",
                "FULLSCREEN",
                "FULLSCREENBUTTON",
                "MINIMIZE",
                "CLOSEMODE",
                "CGID",
                "MENUID",
                "REGIONID",
                "DRAGID",
            ],
        ),
        string_cmd(
            "WINDOWINFO",
            &[
                "NO",
                "LET",
                "EXIST",
                "E",
                "ACTIVE",
                "CAPTION",
                "X",
                "Y",
                "SX",
                "SY",
                "FULLSCREEN",
                "FULLSCREENBUTTON",
                "MINIMIZE",
                "CLOSEMODE",
            ],
        ),
        string_cmd("WINDOWEND", &["NO"]),
        string_cmd(
            "FONT",
            &[
                "BNO",
                "NO",
                "NAME",
                "ANTIALIASING",
                "COLOR",
                "COLOR2",
                "GRADATION",
                "ROTATION",
                "DRAWMODE",
                "SX",
                "SY",
                "BOLD",
            ],
        ),
        string_cmd(
            "FONTINFO",
            &[
                "NO",
                "NAME",
                "ANTIALIASING",
                "COLOR",
                "COLOR2",
                "GRADATION",
                "ROTATION",
                "DRAWMODE",
                "SX",
                "SY",
                "BOLD",
                "NUM",
                "PITCH",
                "LANG",
                "X",
                "Y",
                "SET",
                "SETSTR",
                "LET",
            ],
        ),
        string_cmd(
            "CGINFO",
            &[
                "ID", "IDNO", "EXIST", "E", "A", "X", "Y", "Z", "MFILE", "T", "TID", "TA", "MODE",
                "SX", "SY", "FX", "FY", "FBX", "FBY", "FEX", "FEY", "FEZ", "TEX", "ONMOUSE",
                "ONMOUSE2", "ONMOUSE3", "COLOR", "TRIM", "LINT", "LINT2", "LET", "SET", "SET2",
            ],
        ),
        cmd("TEXT", &["ID", "OUTPUTMODE"]),
    ]
}

pub fn state() -> GfxState {
    GfxState::new(table(), GfxConfig::new(Viewport::new(800, 600)))
}

pub fn slot(table: &ParamTable, cmd: &str, name: &str) -> u8 {
    table
        .params(cmd)
        .expect("command")
        .iter()
        .position(|p| p.name == name)
        .map(|i| u8::try_from(i).expect("slot fits"))
        .expect("param")
}

pub fn arg(table: &ParamTable, cmd: &str, name: &str, value: Value) -> ArgValue {
    ArgValue::new(slot(table, cmd, name), value)
}

pub fn s(v: &str) -> Value {
    Value::Str(v.as_bytes().to_vec())
}

pub fn i(v: i64) -> Value {
    Value::Int(v)
}

pub fn f(v: f64) -> Value {
    Value::Float(v)
}

pub fn call(frame: u64, name: &str, args: Vec<ArgValue>) -> VmEvent {
    VmEvent::DomainCall {
        frame,
        domain: Domain::Graphics,
        name: name.to_string(),
        args,
    }
}

pub fn window_call(frame: u64, name: &str, args: Vec<ArgValue>) -> VmEvent {
    VmEvent::DomainCall {
        frame,
        domain: Domain::Window,
        name: name.to_string(),
        args,
    }
}

pub fn frame_start(frame: u64) -> VmEvent {
    VmEvent::FrameStart { frame }
}

pub fn frame_end(frame: u64) -> VmEvent {
    VmEvent::FrameEnd { frame }
}

pub fn id(v: &str) -> LayerId {
    LayerId::new(v.as_bytes().to_vec())
}
