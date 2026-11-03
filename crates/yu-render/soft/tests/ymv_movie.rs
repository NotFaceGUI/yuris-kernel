//! `YMV` 影片图层 · 软后端端到端(真实 SDK 样例)。
//!
//! 走完整链路:`CG[ID]` 建层 → `YMV[ID FILE]` 挂片(元数据由 VM 侧解析后
//! 注入,测试直接调 `set_movie_meta`)→ `CompositorFrame` → 软后端按
//! `(资产, 帧)` 解出 RGBA → 屏幕像素。
//!
//! 样例来自官方 SDK(`YURIS_SAMPLE_SDK0495/data/pac/*.ymv`);
//! `YURIS_REQUIRE_SAMPLES=1` 时缺样例直接失败(禁止空跑通过)。

use std::path::PathBuf;

use yu_backend_soft::{SoftRenderer, TextureStore};
use yu_format::yscm::{Arg as FmtArg, Command as FmtCommand, CommandTable};
use yu_gfx::{Clear, GfxConfig, GfxState, ImageFormat, MovieInfo, ParamTable, Viewport};
use yu_rt::event::{ArgValue, Domain, VmEvent};
use yu_rt::tables::YscmTable;
use yu_rt::value::Value;
use yu_vfs::{Codec, DirSource};

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
            command("CG", &["ID", "FILE", "E", "A", "X", "Y", "Z", "SX", "SY", "REMALLOC"]),
            command(
                "CGACT",
                &[
                    "ID", "ID2", "SIZE", "SIZE2", "FX", "FY", "FX2", "FY2", "FSX", "FSY", "X", "Y",
                    "X2", "Y2", "SX", "SY",
                ],
            ),
            command("YMV", &["ID", "IDNO", "FILE", "FRAME"]),
            command("YMVEND", &["ID", "IDNO"]),
            command("YMVINFO", &["ID", "IDNO", "LET", "NUM", "SX", "SY", "FPS"]),
            command("CGEND", &["ID"]),
        ],
        messages: Vec::new(),
        key_table: Box::new([0u8; 256]),
    };
    ParamTable::new(YscmTable::from_format(fmt).expect("synthetic yscm"))
}

fn state() -> GfxState {
    let mut config = GfxConfig::new(Viewport::new(400, 300));
    config.clear = Clear::explicit([0, 0, 0, 255]);
    config.image_probe_order = vec![ImageFormat::Png, ImageFormat::Jpg];
    GfxState::new(table(), config)
}

fn arg(table: &ParamTable, cmd: &str, name: &str, value: Value) -> ArgValue {
    let slot = table
        .params(cmd)
        .expect("command")
        .iter()
        .position(|p| p.name == name)
        .expect("param");
    ArgValue::new(slot as u8, value)
}

fn call(frame: u64, domain: Domain, name: &str, args: Vec<ArgValue>) -> VmEvent {
    VmEvent::DomainCall {
        frame,
        domain,
        name: name.to_string(),
        args,
        script: None,
        pc: None,
    }
}

fn s(text: &str) -> Value {
    Value::Str(text.as_bytes().to_vec())
}

fn i(v: i64) -> Value {
    Value::Int(v)
}

fn f(v: f64) -> Value {
    Value::Float(v)
}

/// SDK 的**数据根**(资产 key 相对它:`pac/sample2` → `data/pac/sample2.ymv`)。
fn sdk_data() -> Option<PathBuf> {
    let root = std::env::var("YURIS_SAMPLE_SDK0495").ok()?;
    let dir = PathBuf::from(root).join("data");
    dir.join("pac/sample2.ymv").is_file().then_some(dir)
}

/// 精灵框架的真实 blit 链(`es_Sprite.yst:8666`):
/// `CGACT[ID=$src ID2=$dst SIZE2=1 FX=… FY=… FX2=… FY2=… FSX=… FSY=…]`
/// 把影片层缩放拷进显示层。这是「影片必须在屏幕上出现」的必经路径 ——
/// 曾因 gfx 的图像加工**源层白名单**漏掉 `TextureRef::Movie` 而整条被判
/// `gfx.imageop.copy_source.unsupported`(影片永不显示 + 每帧刷诊断)。
#[test]
fn ymv_scaled_blit_reaches_display_layer() {
    let Some(dir) = sdk_data() else {
        if std::env::var("YURIS_REQUIRE_SAMPLES").ok().as_deref() == Some("1") {
            panic!("YURIS_REQUIRE_SAMPLES=1 但 YURIS_SAMPLE_SDK0495/data/pac/sample2.ymv 缺失");
        }
        return;
    };
    let table = table();
    let mut gfx = state();
    gfx.set_movie_meta(
        b"pac/sample2",
        MovieInfo {
            frames: 306,
            width: 400,
            height: 300,
            fps: 30,
            has_alpha: true,
        },
    );
    // 影片层 `MV` + 显示层 `DISP`(框架里 `$dst` 由 `CG[ID REMALLOC=1]` 建)。
    let blit = |gfx: &mut GfxState, frame_no: u64, frame_idx: i64| {
        gfx.consume_events(&[
            VmEvent::FrameStart { frame: frame_no },
            call(
                frame_no,
                Domain::Movie,
                "YMV",
                vec![
                    arg(&table, "YMV", "ID", s("MV")),
                    arg(&table, "YMV", "FRAME", i(frame_idx)),
                ],
            ),
            call(
                frame_no,
                Domain::Graphics,
                "CGACT",
                vec![
                    arg(&table, "CGACT", "ID", s("MV")),
                    arg(&table, "CGACT", "ID2", s("DISP")),
                    arg(&table, "CGACT", "SIZE2", i(1)),
                    // 区域与尺寸都是 1/100 定点(框架 `@X*100`)。
                    arg(&table, "CGACT", "FX", f(0.0)),
                    arg(&table, "CGACT", "FY", f(0.0)),
                    arg(&table, "CGACT", "FX2", f(40000.0)),
                    arg(&table, "CGACT", "FY2", f(30000.0)),
                    arg(&table, "CGACT", "FSX", f(40000.0)),
                    arg(&table, "CGACT", "FSY", f(30000.0)),
                ],
            ),
            VmEvent::FrameEnd { frame: frame_no },
        ]);
    };
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            Domain::Graphics,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("MV")),
                arg(&table, "CG", "FILE", s("")),
            ],
        ),
        call(
            1,
            Domain::Graphics,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("DISP")),
                arg(&table, "CG", "FILE", s("")),
                arg(&table, "CG", "SX", i(400)),
                arg(&table, "CG", "SY", i(300)),
            ],
        ),
        call(
            1,
            Domain::Movie,
            "YMV",
            vec![
                arg(&table, "YMV", "ID", s("MV")),
                arg(&table, "YMV", "FILE", s("pac/sample2")),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);
    blit(&mut gfx, 2, 0);

    // 图像加工必须**可执行**(旧实现:源层是 Movie ⇒ copy_source.unsupported)。
    let bad: Vec<String> = gfx
        .diagnostics()
        .iter()
        .filter(|d| d.code.starts_with("gfx.imageop"))
        .map(|d| format!("{}: {}", d.code, d.detail))
        .collect();
    assert!(bad.is_empty(), "SIZE2 从影片层取源被判未实现:{bad:?}");

    let mut backend = SoftRenderer::new(TextureStore::new(
        Box::new(DirSource::open(&dir, Codec::Cp932).expect("sdk data dir")),
        Codec::Cp932,
        vec![ImageFormat::Png, ImageFormat::Jpg],
    ));
    let out = backend.render_frame(&gfx.frame());
    assert_eq!(
        out.report.skipped_ops_total, 0,
        "渲染仍有跳过: {:?}",
        out.report.skipped
    );
    assert!(out.report.ops_drawn > 0, "显示层没有画出任何 op");
    let first = out.surface.clone();
    let distinct: std::collections::BTreeSet<[u8; 4]> = (0..first.height)
        .flat_map(|y| (0..first.width).map(move |x| (x, y)))
        .filter_map(|(x, y)| first.pixel(x, y))
        .collect();
    assert!(
        distinct.len() > 64,
        "显示层像素过少({}) —— 影片没有被拷进 DISP",
        distinct.len()
    );

    blit(&mut gfx, 3, 150);
    let out2 = backend.render_frame(&gfx.frame());
    let mut diff = 0usize;
    for y in 0..first.height {
        for x in 0..first.width {
            if first.pixel(x, y) != out2.surface.pixel(x, y) {
                diff += 1;
            }
        }
    }
    assert!(diff > 1000, "换帧后显示层未变化({diff} 像素)");
}

/// `YMV[FRAME]` 的两帧必须解出**不同**像素,且不是全黑/全同色。
#[test]
fn ymv_frames_render_distinct_pixels() {
    let Some(dir) = sdk_data() else {
        if std::env::var("YURIS_REQUIRE_SAMPLES").ok().as_deref() == Some("1") {
            panic!("YURIS_REQUIRE_SAMPLES=1 但 YURIS_SAMPLE_SDK0495/data/pac/sample2.ymv 缺失");
        }
        return;
    };
    let table = table();
    let mut gfx = state();
    // 容器头:400×300 / 30fps / 306 帧(官方样例实测)。
    gfx.set_movie_meta(
        b"pac/sample2",
        MovieInfo {
            frames: 306,
            width: 400,
            height: 300,
            fps: 30,
            has_alpha: true,
        },
    );
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            Domain::Graphics,
            "CG",
            vec![arg(&table, "CG", "ID", s("MV")), arg(&table, "CG", "FILE", s(""))],
        ),
        call(
            1,
            Domain::Movie,
            "YMV",
            vec![
                arg(&table, "YMV", "ID", s("MV")),
                arg(&table, "YMV", "FILE", s("pac/sample2")),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);

    let mut backend = SoftRenderer::new(TextureStore::new(
        Box::new(DirSource::open(&dir, Codec::Cp932).expect("sdk data dir")),
        Codec::Cp932,
        vec![ImageFormat::Png, ImageFormat::Jpg],
    ));
    let out = backend.render_frame(&gfx.frame());
    assert!(out.report.skipped_ops_total == 0, "{:?}", out.report);
    let first = out.surface.clone();
    assert_eq!((first.width, first.height), (400, 300));
    // 画面不是纯色(水族箱样例:有珊瑚/鱼的颜色分布)。
    let distinct: std::collections::BTreeSet<[u8; 4]> = (0..first.height)
        .flat_map(|y| (0..first.width).map(move |x| (x, y)))
        .filter_map(|(x, y)| first.pixel(x, y))
        .collect();
    assert!(distinct.len() > 64, "帧 0 颜色过少:{}", distinct.len());

    // 换到第 150 帧:同一层,像素必须变化。
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 2 },
        call(
            2,
            Domain::Movie,
            "YMV",
            vec![arg(&table, "YMV", "ID", s("MV")), arg(&table, "YMV", "FRAME", i(150))],
        ),
        VmEvent::FrameEnd { frame: 2 },
    ]);
    let out2 = backend.render_frame(&gfx.frame());
    assert!(out2.report.skipped_ops_total == 0, "{:?}", out2.report);
    let mut diff = 0usize;
    for y in 0..first.height {
        for x in 0..first.width {
            if first.pixel(x, y) != out2.surface.pixel(x, y) {
                diff += 1;
            }
        }
    }
    assert!(
        diff > 1000,
        "第 150 帧与第 0 帧差异过小({diff} 像素),影片换帧未生效"
    );
}
