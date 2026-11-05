//! 过渡覆盖率 oracle 测试:用**官方参考帧**里读出的 α 表钉住实现。
//!
//! # 真值来源
//!
//! 官方 0.495 微场景 `j2-micro-trans8`(`target/tmp/j2-micro-trans8-ref/frame_0001.png`)
//! 把参数排成网格:行 = TA{2,64,128,192},列 = T{0,64,128,192,255},
//! 每格两个 mask 层(RECTPAINT `SET=0x000000` 与 `SET=0x808080`),
//! 格内先用 `RECTPAINT` 涂成绿色再 `CG[TID=mask T=.. TA=..]`。
//! 参考帧上 α = 绿色通道 / 255(底色纯黑)。
//!
//! # 这张表为什么可信,以及它对应哪条路径
//!
//! 表里 `mask=0, T=255` 的四点精确等于 `(256 - TA/2)/256`
//! (0.996 / 0.875 / 0.749 / 0.624,误差 ≤0.004),不是随手拟合能凑出来的。
//!
//! 官方固定管线(`_DXMODE=0`)在“有配对且 `TA>=1`”时走 effect **Pass1
//! (psC)**(`FUN_00414ba4` `0x414dfa` BeginPass(1)、`0x416c8a` 设 uniform):
//!
//! ```text
//! oC0.a = tex1.a * saturate((YS_FadeVal - 0.5*tex2.r) * YS_FadeVal2) * YS_AlphaVal
//! ```
//!
//! 且遮罩按 **/256** 归一化、使用精灵系统反色后的缓冲。这两点合起来对整张
//! 40 格表逐点成立(≤2/255),也解释了 `mask=0,T=255`:
//! `fade(256)=0.5+1/TA`,反色后的 `mask=0` 是 1.0,故
//! `sat((0.5+1/TA - 0.5)*TA)=1` 附近,单块几何边界上的取值就是
//! `1-TA/512`。

use yu_backend_soft::{SoftRenderer, TextureStore};
use yu_format::yscm::{Arg as FmtArg, Command as FmtCommand, CommandTable};
use yu_gfx::{Clear, GfxConfig, GfxState, ImageFormat, ParamTable, Viewport};
use yu_rt::event::{ArgValue, Domain, VmEvent};
use yu_rt::tables::YscmTable;
use yu_rt::value::Value;
use yu_vfs::{Codec, MemSource};

fn command(name: &str, params: &[&str]) -> FmtCommand {
    FmtCommand {
        name: name.into(),
        argc: params.len() as u8,
        args: params
            .iter()
            .map(|param| FmtArg {
                name: (*param).into(),
                flag: if matches!(*param, "ID" | "ID2" | "FILE" | "NAME" | "SETSTR") {
                    1
                } else {
                    0
                },
                kind: 0,
            })
            .collect(),
    }
}

fn table() -> ParamTable {
    let fmt = CommandTable {
        version: 495,
        commands: vec![
            command(
                "CG",
                &[
                    "ID", "FILE", "E", "A", "X", "Y", "Z", "SX", "SY", "MODE", "TID", "T", "TA",
                ],
            ),
            command(
                "CGACT",
                &[
                    "ID",
                    "ID2",
                    "RECTPAINT",
                    "RECTPAINT2",
                    "RECTPAINT3",
                    "REVERSE",
                    "SET",
                    "X",
                    "Y",
                    "X2",
                    "Y2",
                ],
            ),
        ],
        messages: Vec::new(),
        key_table: Box::new([0u8; 256]),
    };
    ParamTable::new(YscmTable::from_format(fmt).expect("synthetic yscm"))
}

fn slot(table: &ParamTable, command: &str, name: &str) -> u8 {
    table
        .params(command)
        .expect("command exists")
        .iter()
        .position(|param| param.name == name)
        .map(|index| u8::try_from(index).expect("slot fits"))
        .expect("param exists")
}

fn arg(table: &ParamTable, command: &str, name: &str, value: Value) -> ArgValue {
    ArgValue::new(slot(table, command, name), value)
}

fn call(frame: u64, name: &str, args: Vec<ArgValue>) -> VmEvent {
    VmEvent::DomainCall {
        frame,
        domain: Domain::Graphics,
        name: name.to_string(),
        args,
        script: None,
        pc: None,
    }
}

/// 官方 `j2-micro-trans8` 参考帧读出的 α:行 = TA,列 = T,值 = (mask0, mask128)。
const OFFICIAL: &[(i64, [f64; 5], [f64; 5])] = &[
    (
        2,
        [0.0, 0.0, 0.0, 0.502, 0.996],
        [0.0, 0.0, 0.502, 1.0, 1.0],
    ),
    (64, [0.0, 0.0, 0.0, 0.0, 0.875], [0.0, 0.0, 0.498, 1.0, 1.0]),
    (
        128,
        [0.0, 0.0, 0.0, 0.0, 0.749],
        [0.0, 0.0, 0.498, 1.0, 1.0],
    ),
    (
        192,
        [0.0, 0.0, 0.0, 0.0, 0.624],
        [0.0, 0.0, 0.498, 1.0, 1.0],
    ),
];
const TVALS: [i64; 5] = [0, 64, 128, 192, 255];

fn render_alpha(ta: i64, t: i64, mask_set: i64) -> f64 {
    let table = table();
    let mut config = GfxConfig::new(Viewport::new(24, 24));
    config.clear = Clear::explicit([0, 0, 0, 255]);
    config.image_probe_order = vec![ImageFormat::Png];
    let mut gfx = GfxState::new(table.clone(), config);

    let mut events = vec![VmEvent::FrameStart { frame: 1 }];
    // 底:BG(RECTPAINT 黑)
    events.push(call(
        1,
        "CG",
        vec![
            arg(&table, "CG", "ID", Value::Str(b"BG".to_vec())),
            arg(&table, "CG", "FILE", Value::Str(Vec::new())),
            arg(&table, "CG", "SX", Value::Int(24)),
            arg(&table, "CG", "SY", Value::Int(24)),
        ],
    ));
    events.push(call(
        1,
        "CGACT",
        vec![
            arg(&table, "CGACT", "ID", Value::Str(b"BG".to_vec())),
            arg(&table, "CGACT", "RECTPAINT", Value::Int(1)),
            arg(&table, "CGACT", "SET", Value::Int(0x000000)),
        ],
    ));
    // mask 层(均匀色)
    events.push(call(
        1,
        "CG",
        vec![
            arg(&table, "CG", "ID", Value::Str(b"M".to_vec())),
            arg(&table, "CG", "FILE", Value::Str(Vec::new())),
            arg(&table, "CG", "SX", Value::Int(8)),
            arg(&table, "CG", "SY", Value::Int(8)),
        ],
    ));
    events.push(call(
        1,
        "CGACT",
        vec![
            arg(&table, "CGACT", "ID", Value::Str(b"M".to_vec())),
            arg(&table, "CGACT", "RECTPAINT", Value::Int(1)),
            arg(&table, "CGACT", "SET", Value::Int(mask_set)),
        ],
    ));
    // 官方精灵系统的过渡缓冲在进入转场前被 `CGACT[REVERSE=1]` 反色
    // (`es_Sprite.yst` LOADTR);micro 参考帧测到的就是反色后的缓冲。
    events.push(call(
        1,
        "CGACT",
        vec![
            arg(&table, "CGACT", "ID", Value::Str(b"M".to_vec())),
            arg(&table, "CGACT", "REVERSE", Value::Int(1)),
        ],
    ));
    // 测试层:绿色,再挂过渡
    events.push(call(
        1,
        "CG",
        vec![
            arg(&table, "CG", "ID", Value::Str(b"T".to_vec())),
            arg(&table, "CG", "FILE", Value::Str(Vec::new())),
            arg(&table, "CG", "X", Value::Int(8)),
            arg(&table, "CG", "Y", Value::Int(8)),
            arg(&table, "CG", "SX", Value::Int(8)),
            arg(&table, "CG", "SY", Value::Int(8)),
        ],
    ));
    events.push(call(
        1,
        "CGACT",
        vec![
            arg(&table, "CGACT", "ID", Value::Str(b"T".to_vec())),
            arg(&table, "CGACT", "RECTPAINT", Value::Int(1)),
            arg(&table, "CGACT", "SET", Value::Int(0x00FF00)),
        ],
    ));
    let mut tr = vec![
        arg(&table, "CG", "ID", Value::Str(b"T".to_vec())),
        arg(&table, "CG", "TID", Value::Str(b"M".to_vec())),
        arg(&table, "CG", "T", Value::Int(t)),
    ];
    if ta >= 1 {
        tr.push(arg(&table, "CG", "TA", Value::Int(ta)));
    }
    events.push(call(1, "CG", tr));
    events.push(VmEvent::FrameEnd { frame: 1 });
    gfx.consume_events(&events);

    let mut store = TextureStore::new(
        Box::new(MemSource::new(Codec::Cp932)),
        Codec::Cp932,
        vec![ImageFormat::Png],
    );
    let _ = &mut store;
    let mut renderer = SoftRenderer::new(store);
    let out = renderer.render_frame(&gfx.frame());
    // 采样测试层中心(8+4, 8+4)
    let i = ((12 * out.surface.width as usize) + 12) * 4;
    f64::from(out.surface.rgba[i + 1]) / 255.0
}

/// 官方 `j2-micro-trans6`(TA=128)的 **9 级 mask 扫描**:T∈{64,128,192,256} ×
/// mask∈{0,32,…,255}。实测(参考帧逐格):见 `T6` 表。
///
/// 这 36 点同时被两种带宽拟合:「窄带阈值」(现实现,`1/(30+70f)` ≈0.02)
/// 与「宽带」(w=1/6)都能逐点落进 8/255 —— 因为采样只落在连通域边界附近。
/// 判别证据来自开幕:菱形格 `ysr023` 是平滑梯度掩膜,宽带会把它糊成灰阶,
/// 故只能取窄带(DIV-GFX-065 §10)。本测试用整条量化链防回归。
///
/// **注意**:下方 40 格表(TA=2/64/192)**仍不通过**:`TA→带宽` 映射未定,
/// 见 DIV-GFX-065 §11.1。
#[test]
fn transition_alpha_matches_official_mask_sweep() {
    // (T, mask, 官方 α) —— `j2-micro-trans6-ref/frame_0001.png` 逐格读取。
    const T6: &[(i64, i64, f64)] = &[
        (64, 0, 0.0),
        (64, 32, 0.0),
        (64, 64, 0.0),
        (64, 96, 0.0),
        (64, 128, 0.0),
        (64, 160, 0.0),
        (64, 192, 0.247),
        (64, 224, 1.0),
        (64, 255, 1.0),
        (128, 0, 0.0),
        (128, 32, 0.0),
        (128, 64, 0.0),
        (128, 96, 0.0),
        (128, 128, 0.498),
        (128, 160, 1.0),
        (128, 192, 1.0),
        (128, 224, 1.0),
        (128, 255, 1.0),
        (192, 0, 0.0),
        (192, 32, 0.0),
        (192, 64, 0.749),
        (192, 96, 1.0),
        (192, 128, 1.0),
        (192, 160, 1.0),
        (192, 192, 1.0),
        (192, 224, 1.0),
        (192, 255, 1.0),
        (256, 0, 1.0),
        (256, 32, 1.0),
        (256, 64, 1.0),
        (256, 96, 1.0),
        (256, 128, 1.0),
        (256, 160, 1.0),
        (256, 192, 1.0),
        (256, 224, 1.0),
        (256, 255, 1.0),
    ];
    let mut failures = Vec::new();
    for &(t, mask, expected) in T6 {
        let got = render_alpha(128, t, mask << 16 | mask << 8 | mask);
        if (got - expected).abs() > 8.0 / 255.0 {
            failures.push(format!(
                "T={t} mask={mask}: 期望 {expected:.3} 实得 {got:.3}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "与官方 mask 扫描偏差超过 8/255:
{}",
        failures.join(
            "
"
        )
    );
}

/// TA 维度(官方 `j2-micro-trans8`:TA{2,64,192} × T × mask{0,128})。
///
/// 官方固定管线路径实际走 effect Pass1(psC):
/// `α = sat((FadeVal - 0.5*mask/256) * TA)`,遮罩是精灵系统反色后的缓冲。
/// 该式对整张 40 格表逐点成立(≤2/255),包括此前无法解释的
/// `mask=0,T=255 → 1-TA/512` —— 它是 `fade(256)=0.5+1/TA` 在那个
/// 单块的几何边界上的取值,不是额外的覆盖率公式。
#[test]
fn transition_alpha_matches_official_grid() {
    let mut failures = Vec::new();
    for &(ta, row0, row128) in OFFICIAL {
        for (ci, &t) in TVALS.iter().enumerate() {
            for (mask_set, expected) in [(0x000000, row0[ci]), (0x808080, row128[ci])] {
                let got = render_alpha(ta, t, mask_set);
                if (got - expected).abs() > 2.0 / 255.0 {
                    failures.push(format!(
                        "TA={ta} T={t} mask={mask_set:#08x}: 期望 {expected:.3} 实得 {got:.3}"
                    ));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "过渡覆盖率与官方参考帧偏差超过 2/255:\n{}",
        failures.join("\n")
    );
}
