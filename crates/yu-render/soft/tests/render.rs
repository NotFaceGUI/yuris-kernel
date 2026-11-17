//! 软件后端端到端:合成命令表 → GfxState → CompositorFrame → RGBA8/PNG。
//!
//! 夹具不依赖真实游戏数据:命令表只保留本测试用到的参数;纹理用
//! `MemSource` + 内存 PNG。

use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder, Rgba, RgbaImage};
use yu_backend_soft::{frame_has_drawable_output, png, SoftRenderer, TextureStore};
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
                    "ID", "FILE", "E", "A", "X", "Y", "Z", "SX", "SY", "MODE", "FBX", "FBY", "MID",
                ],
            ),
            command("CGEND", &["ID"]),
            command(
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
            command(
                "CGACT",
                &[
                    "ID",
                    "ID2",
                    "COPY2",
                    "SET",
                    "X",
                    "Y",
                    "X2",
                    "Y2",
                    "SX",
                    "SY",
                    "SIZE",
                    "RECTPAINT",
                    "RECTPAINT2",
                    "RECTPAINT3",
                    "MERGE",
                    "MERGE2",
                    "TEXT",
                    "SETSTR",
                    "BNO",
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

fn state(viewport: Viewport, clear: [u8; 4]) -> GfxState {
    let mut config = GfxConfig::new(viewport);
    config.clear = Clear::explicit(clear);
    config.image_probe_order = vec![ImageFormat::Png];
    GfxState::new(table(), config)
}

fn png_bytes_for(color: Rgba<u8>) -> Vec<u8> {
    let image = RgbaImage::from_pixel(2, 2, color);
    let mut out = Vec::new();
    PngEncoder::new(&mut out)
        .write_image(image.as_raw(), 2, 2, ExtendedColorType::Rgba8)
        .expect("encode fixture");
    out
}

fn png_strip_red_blue() -> Vec<u8> {
    let mut image = RgbaImage::new(2, 1);
    image.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
    image.put_pixel(1, 0, Rgba([0, 0, 255, 255]));
    let mut out = Vec::new();
    PngEncoder::new(&mut out)
        .write_image(image.as_raw(), 2, 1, ExtendedColorType::Rgba8)
        .expect("encode fixture");
    out
}

fn texture_store(entries: &[(&[u8], Vec<u8>)]) -> TextureStore {
    let mut source = MemSource::new(Codec::Cp932);
    for (path, bytes) in entries {
        source.insert(path, bytes.clone()).expect("insert");
    }
    TextureStore::new(Box::new(source), Codec::Cp932, vec![ImageFormat::Png])
}

/// `CG[FILE=资产 SX/SY]`:官方 `+0x78/+0x7c` 是绘制目标矩形,纹理按矩形
/// 缩放(DIV-GFX-022)。旧内核会把该层标 `gfx.image.size.unsupported` 整层
/// 跳过;本用例锁住“1×1 红图 → 4×4 红块”的可见结果。
#[test]
fn asset_with_explicit_size_is_scaled_not_skipped() {
    let table = table();
    let mut gfx = state(Viewport::new(8, 8), [255, 255, 255, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"A".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"red".to_vec())),
                arg(&table, "CG", "X", Value::Int(0)),
                arg(&table, "CG", "Y", Value::Int(0)),
                arg(&table, "CG", "SX", Value::Int(4)),
                arg(&table, "CG", "SY", Value::Int(4)),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);
    let frame = gfx.frame();
    let store = texture_store(&[(b"red.png", png_bytes_for(Rgba([255, 0, 0, 255])))]);
    let mut backend = SoftRenderer::new(store);
    let out = backend.render_frame(&frame);
    assert_eq!(out.report.skipped_ops_total, 0, "不得整层跳过");
    for y in 0..4 {
        for x in 0..4 {
            assert_eq!(
                out.surface.pixel(x, y),
                Some([255, 0, 0, 255]),
                "1×1 红图应按 SX/SY 铺满 4×4,差在 ({x},{y})"
            );
        }
    }
    assert_eq!(out.surface.pixel(4, 4), Some([255, 255, 255, 255]));
}

#[test]
fn asset_layer_composites_to_screen() {
    let table = table();
    let mut gfx = state(Viewport::new(8, 8), [255, 255, 255, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"A".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"red".to_vec())),
                arg(&table, "CG", "X", Value::Int(1)),
                arg(&table, "CG", "Y", Value::Int(1)),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);
    let frame = gfx.frame();
    assert!(frame_has_drawable_output(&frame));

    let store = texture_store(&[(b"red.png", png_bytes_for(Rgba([255, 0, 0, 255])))]);
    let mut backend = SoftRenderer::new(store);
    let out = backend.render_frame(&frame);

    assert_eq!((out.surface.width, out.surface.height), (8, 8));
    assert_eq!(out.surface.pixel(1, 1), Some([255, 0, 0, 255]));
    assert_eq!(out.surface.pixel(2, 2), Some([255, 0, 0, 255]));
    assert_eq!(out.surface.pixel(0, 0), Some([255, 255, 255, 255]));
    // 主组直绘(DIV-GFX-008/041):只有图层 op,组不单独计数。
    assert_eq!(out.report.ops_drawn, 1, "asset layer drawn directly");
    assert_eq!(out.report.skipped_ops_total, 0);
    assert_eq!(
        out.report.ops_total,
        out.report.ops_drawn + out.report.skipped_ops_total as usize,
        "ops_total = drawn + skipped"
    );
    assert!(out.report.missing_assets.is_empty());
    assert_eq!(out.report.skipped_text, 0);

    // 确定性:同一帧再次渲染,PNG 逐字节一致。
    let first_png = png::png_bytes(&out.surface).expect("encode");
    let again = backend.render_frame(&frame);
    let second_png = png::png_bytes(&again.surface).expect("encode");
    assert_eq!(first_png, second_png);
    let decoded = image::load_from_memory(&first_png).expect("decode png");
    assert_eq!(decoded.width(), 8);
    assert_eq!(decoded.height(), 8);
}

/// 0.495 `FUN_00414ba4`:普通图层宽度 = `(FBX/10000) * 自然宽`
/// (同 y/FBY)。2×2 图缩到 50% 后应只覆盖 1×1。
#[test]
fn affine_scale_scales_layer_quad() {
    let table = table();
    let mut gfx = state(Viewport::new(8, 8), [255, 255, 255, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"S".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"red".to_vec())),
                arg(&table, "CG", "X", Value::Int(2)),
                arg(&table, "CG", "Y", Value::Int(2)),
                arg(&table, "CG", "FBX", Value::Int(5000)),
                arg(&table, "CG", "FBY", Value::Int(5000)),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);
    let frame = gfx.frame();
    let mut backend = SoftRenderer::new(texture_store(&[(
        b"red.png",
        png_bytes_for(Rgba([255, 0, 0, 255])),
    )]));
    let out = backend.render_frame(&frame);

    assert_eq!(out.surface.pixel(2, 2), Some([255, 0, 0, 255]));
    assert_eq!(out.surface.pixel(3, 3), Some([255, 255, 255, 255]));
    assert_eq!(out.surface.pixel(1, 1), Some([255, 255, 255, 255]));
    assert_eq!(out.report.ops_drawn, 1);
    assert_eq!(out.report.skipped_ops_total, 0);
}

/// 负 `FBX` 是镜像:2×1(左红右蓝)以 `X=2, FBX=-10000` 画到 [0,2),
/// 左像素应取源右(蓝),右像素取源左(红)。
#[test]
fn negative_affine_scale_mirrors_layer() {
    let table = table();
    let mut gfx = state(Viewport::new(4, 1), [255, 255, 255, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"M".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"strip".to_vec())),
                arg(&table, "CG", "X", Value::Int(2)),
                arg(&table, "CG", "Y", Value::Int(0)),
                arg(&table, "CG", "FBX", Value::Int(-10000)),
                arg(&table, "CG", "FBY", Value::Int(10000)),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);
    let frame = gfx.frame();
    let mut backend = SoftRenderer::new(texture_store(&[(b"strip.png", png_strip_red_blue())]));
    let out = backend.render_frame(&frame);
    assert_eq!(out.surface.pixel(0, 0), Some([0, 0, 255, 255]));
    assert_eq!(out.surface.pixel(1, 0), Some([255, 0, 0, 255]));
    assert_eq!(out.surface.pixel(2, 0), Some([255, 255, 255, 255]));
}

#[test]
fn solid_black_1x1_is_stretched_to_sx_sy() {
    let table = table();
    let mut gfx = state(Viewport::new(8, 8), [255, 255, 255, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"B".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(Vec::new())),
                arg(&table, "CG", "SX", Value::Int(4)),
                arg(&table, "CG", "SY", Value::Int(4)),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);
    let frame = gfx.frame();
    let mut backend = SoftRenderer::new(texture_store(&[]));
    let out = backend.render_frame(&frame);

    // `CG[FILE=""]` 是**不透明黑**的 1×1 表面,按 `SX/SY` 铺开即黑幕/
    // 遮挡板(手册「1x1 の黒画像」)。这里同时钉住"不能透明":改成 alpha=0
    // 会让黑幕提前消失(实测读档淡出缺失)。
    assert_eq!(out.surface.pixel(0, 0), Some([0, 0, 0, 255]));
    assert_eq!(out.surface.pixel(3, 3), Some([0, 0, 0, 255]));
    // 层矩形之外的像素不受影响,仍是画布底色。
    assert_eq!(out.surface.pixel(4, 0), Some([255, 255, 255, 255]));
    // 主组直绘:只有那一层 op(组不再单独计数)。
    assert_eq!(out.report.ops_drawn, 1);
}

#[test]
fn cgact_size_from_fileless_blank_layer_is_transparent() {
    let table = table();
    let mut gfx = state(Viewport::new(8, 8), [255, 255, 255, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        // 空白源:BUF.LNO_FB.1 同型(`CG[FILE=""]`,无 fill),脚本里 E=0。
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"BUF".to_vec())),
                arg(&table, "CG", "E", Value::Int(0)),
                arg(&table, "CG", "FILE", Value::Str(Vec::new())),
                arg(&table, "CG", "SX", Value::Int(1)),
                arg(&table, "CG", "SY", Value::Int(1)),
            ],
        ),
        // 可见层:空白面 + 目标尺寸,再由 SIZE 从 BUF 取像素。
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"FADE".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(Vec::new())),
                arg(&table, "CG", "SX", Value::Int(8)),
                arg(&table, "CG", "SY", Value::Int(8)),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", Value::Str(b"BUF".to_vec())),
                arg(&table, "CGACT", "ID2", Value::Str(b"FADE".to_vec())),
                arg(&table, "CGACT", "SIZE", Value::Int(1)),
                arg(&table, "CGACT", "SX", Value::Int(8)),
                arg(&table, "CGACT", "SY", Value::Int(8)),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);
    let frame = gfx.frame();
    let mut backend = SoftRenderer::new(texture_store(&[]));
    let out = backend.render_frame(&frame);

    // 官方空串分支把 1×1 新缓冲四字节全写 0(REA `0x40c5f8`),`CGACT[SIZE]`
    // 读的是这份像素 ⇒ 空白源缩放后必须**透明**。若按直接绘制口径给黑,
    // 这里会把空白缓冲放大成不透明黑板(转场首帧单帧闪黑,2026-11-15 实测)。
    assert_eq!(out.surface.pixel(0, 0), Some([255, 255, 255, 255]));
    assert_eq!(out.surface.pixel(4, 4), Some([255, 255, 255, 255]));
    assert_eq!(out.surface.pixel(7, 7), Some([255, 255, 255, 255]));
}

#[test]
fn missing_asset_is_reported_and_layer_skipped() {
    let table = table();
    let mut gfx = state(Viewport::new(8, 8), [255, 255, 255, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"C".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"nope".to_vec())),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);
    let frame = gfx.frame();
    let mut backend = SoftRenderer::new(texture_store(&[]));
    let out = backend.render_frame(&frame);

    assert_eq!(out.surface.pixel(0, 0), Some([255, 255, 255, 255]));
    // DIV-GFX-008/041(主组直绘):主窗口组不占一个 group op,子层直接
    // 合成到屏幕;缺失资产的那一层被跳过,因此本帧没有可绘制 op。
    assert_eq!(out.report.ops_drawn, 0, "main group is drawn directly");
    assert_eq!(out.report.skipped_ops_total, 1);
    assert!(out
        .report
        .skipped
        .iter()
        .any(|skip| skip.kind == "layer" && skip.reason == "asset.missing"));
    assert_eq!(out.report.missing_assets.len(), 1);
    assert_eq!(out.report.missing_assets[0].key, b"nope");
    assert_eq!(out.report.missing_assets[0].code, "asset.missing");
}

#[test]
fn text_without_flags_is_counted_but_not_rasterized() {
    let table = table();
    let mut gfx = state(Viewport::new(8, 8), [0, 0, 0, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        VmEvent::Text {
            frame: 1,
            id: b"T".to_vec(),
            bytes: b"hello".to_vec(),
        },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"B".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(Vec::new())),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);
    let frame = gfx.frame();
    assert_eq!(frame.text.len(), 1, "fixture produced one TextRun");

    let mut backend = SoftRenderer::new(texture_store(&[]));
    let out = backend.render_frame(&frame);
    assert_eq!(out.report.skipped_text, 1);
    // 文本不参与像素:唯一可见内容仍是 1×1 黑图。
    assert_eq!(out.surface.pixel(0, 0), Some([0, 0, 0, 255]));
}

#[test]
fn committed_text_glyphs_rasterize_into_layer() {
    let table = table();
    let mut gfx = state(Viewport::new(64, 32), [0, 0, 0, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"TX".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(Vec::new())),
                arg(&table, "CG", "SX", Value::Int(64)),
                arg(&table, "CG", "SY", Value::Int(32)),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);
    let glyph = yu_rt::text::TextGlyphDraw {
        codec: yu_rt::Codec::Cp932,
        // 全角「こ」(CP932 0x82B1)。
        bytes: b"\x82\xB1".iter().copied().collect(),
        full: true,
        x: 0,
        y: 0,
        width: 32,
        height: 32,
        style: yu_rt::text::TextGlyphStyle {
            fg: 0x00FF_FFFF,
            bg: 0x00FF_FFFF,
            antialias: true,
            gradation: 0,
            bold: false,
        },
        draw_mode: 0,
        bno: 1,
    };
    let mut frame_data = yu_rt::text::TextFrame {
        layer_id: b"TX".to_vec(),
        output_mode: 1,
        page_clear_seq: 0,
        cursor_x: 32,
        cursor_y: 0,
        click: false,
        bytes: b"\x82\xB1".to_vec(),
        flags: b"2".to_vec(),
        font: Default::default(),
        first_step: 0,
        total_steps: 1,
        steps: vec![yu_rt::text::TextStep::Glyph(glyph)],
    };
    gfx.apply_text_frame(&frame_data);
    let frame = gfx.frame();
    assert!(
        frame.text_layout.is_some(),
        "layout comes from committed glyphs"
    );

    let mut backend = SoftRenderer::new(texture_store(&[]));
    if !backend.text_composer().available() {
        println!("SKIP committed_text_glyphs_rasterize_into_layer: no system font");
        return;
    }
    let out = backend.render_frame(&frame);
    assert_eq!(out.report.skipped_text, 0);
    assert_eq!(out.report.text_glyphs, 1);
    let ink = out
        .surface
        .rgba
        .chunks_exact(4)
        .filter(|px| px[0] > 0 || px[1] > 0 || px[2] > 0)
        .count();
    assert!(ink > 0, "glyph must be visible (white on black)");

    // 同一帧两次渲染逐字节一致(确定性)。
    let again = backend.render_frame(&frame);
    assert_eq!(
        png::png_bytes(&out.surface).expect("encode"),
        png::png_bytes(&again.surface).expect("encode")
    );

    // `TEXT[CLEAR]`:同页、无新步骤、空待显示缓冲 ⇒ 已提交像素保留。
    let mut clear = frame_data.clone();
    clear.first_step = 1;
    clear.total_steps = 1;
    clear.steps.clear();
    clear.bytes.clear();
    clear.flags.clear();
    gfx.apply_text_frame(&clear);
    assert_eq!(
        backend.render_frame(&gfx.frame()).report.text_glyphs,
        1,
        "TEXT[CLEAR] 不清表面"
    );

    // `'P'` 换页:清表面。
    frame_data.page_clear_seq = 1;
    frame_data.first_step = 0;
    frame_data.total_steps = 1;
    frame_data.steps = vec![yu_rt::text::TextStep::PageClear];
    gfx.apply_text_frame(&frame_data);
    assert_eq!(
        backend.render_frame(&gfx.frame()).report.text_glyphs,
        0,
        "'P' 清页后无字形"
    );
}

#[test]
fn cgact_text_draws_into_layer() {
    let table = table();
    let mut gfx = state(Viewport::new(32, 32), [255, 255, 255, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"TX".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"dark".to_vec())),
            ],
        ),
        call(
            1,
            "FONT",
            vec![
                arg(&table, "FONT", "SX", Value::Int(16)),
                arg(&table, "FONT", "SY", Value::Int(16)),
                arg(&table, "FONT", "COLOR", Value::Int(0x00_FF00)),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", Value::Str(b"TX".to_vec())),
                arg(&table, "CGACT", "TEXT", Value::Int(1)),
                arg(&table, "CGACT", "SET", Value::Int(0)),
                arg(&table, "CGACT", "SETSTR", Value::Str(b"A".to_vec())),
                arg(&table, "CGACT", "X", Value::Int(0)),
                arg(&table, "CGACT", "Y", Value::Int(0)),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);
    let frame = gfx.frame();
    assert_eq!(frame.text_draws.len(), 1);

    let mut backend = SoftRenderer::new(texture_store(&[(
        b"dark.png",
        png_bytes_for_wh(32, 32, Rgba([0, 0, 0, 255])),
    )]));
    if !backend.text_composer().available() {
        println!("SKIP cgact_text_draws_into_layer: no system font");
        return;
    }
    let out = backend.render_frame(&frame);
    assert_eq!(out.report.text_draws, 1);
    assert_eq!(out.report.text_glyphs, 1);
    // `text.font_init` 是首刷必发的一次性信息诊断(P6 字体预热;非慢机也发
    // `0ms`),不是错误;其余诊断(缺字/字体不可用)仍然失败。
    let unexpected: Vec<&String> = out
        .report
        .text_diags
        .iter()
        .filter(|d| !d.starts_with("text.font_init: "))
        .collect();
    assert!(unexpected.is_empty(), "{:?}", out.report.text_diags);
    // 绿色字画进黑色图层:出现绿色通道占优的像素。
    let green = out
        .surface
        .rgba
        .chunks_exact(4)
        .filter(|px| px[1] > 0 && px[1] >= px[0] && px[1] >= px[2])
        .count();
    assert!(green > 0, "text draw must reach the layer pixels");
}

#[test]
fn rectpaint_family_executes_and_matches_evidence_semantics() {
    let table = table();
    let mut gfx = state(Viewport::new(8, 4), [255, 255, 255, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        // R: RECTPAINT -> opaque red (alpha forced 255).
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"R".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(Vec::new())),
                arg(&table, "CG", "SX", Value::Int(4)),
                arg(&table, "CG", "SY", Value::Int(4)),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", Value::Str(b"R".to_vec())),
                arg(&table, "CGACT", "RECTPAINT", Value::Int(1)),
                arg(&table, "CGACT", "SET", Value::Int(0x00FF0000)),
            ],
        ),
        // G: RECTPAINT green then RECTPAINT2 alpha 0x80.
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"G".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(Vec::new())),
                arg(&table, "CG", "X", Value::Int(4)),
                arg(&table, "CG", "SX", Value::Int(4)),
                arg(&table, "CG", "SY", Value::Int(4)),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", Value::Str(b"G".to_vec())),
                arg(&table, "CGACT", "RECTPAINT", Value::Int(1)),
                arg(&table, "CGACT", "SET", Value::Int(0x0000FF00)),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", Value::Str(b"G".to_vec())),
                arg(&table, "CGACT", "RECTPAINT2", Value::Int(1)),
                arg(&table, "CGACT", "SET", Value::Int(0x80)),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);
    let frame = gfx.frame();
    assert_eq!(
        frame
            .passes
            .iter()
            .filter(|p| matches!(p.target, yu_gfx::PassTarget::Texture(_)))
            .count(),
        3,
        "three executable image passes"
    );
    assert!(frame.passes.iter().all(|p| p
        .ops
        .iter()
        .all(|op| op.status() == yu_gfx::OpStatus::Executable)));

    let mut backend = SoftRenderer::new(texture_store(&[]));
    let out = backend.render_frame(&frame);
    // R: W0 离屏里不透明红 -> 屏幕红。
    assert_eq!(out.surface.pixel(0, 0), Some([255, 0, 0, 255]));
    // G: A=128 的绿**直接**合成到白屏(主组直绘,不经过组 RT 的 alpha):
    //   a = 128/255;R = 255·(1−a) = 127;G = 255·a + 255·(1−a) = 255。
    // 屏幕是 opaque 窗口,读回 alpha 归一 255。
    assert_eq!(out.surface.pixel(4, 0), Some([127, 255, 127, 255]));
    assert_eq!(out.report.ops_drawn, 3 + 2, "3 image ops + 2 direct layers");
    assert_eq!(out.report.skipped_ops_total, 0);
}

#[test]
fn rectpaint_region_fills_only_requested_texture_area() {
    let table = table();
    let mut gfx = state(Viewport::new(4, 4), [255, 255, 255, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"IMG".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"img".to_vec())),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", Value::Str(b"IMG".to_vec())),
                arg(&table, "CGACT", "RECTPAINT3", Value::Int(1)),
                arg(&table, "CGACT", "SET", Value::Int(0xFF00FF00)),
                arg(&table, "CGACT", "X", Value::Int(1)),
                arg(&table, "CGACT", "Y", Value::Int(1)),
                arg(&table, "CGACT", "X2", Value::Int(2)),
                arg(&table, "CGACT", "Y2", Value::Int(2)),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);
    let frame = gfx.frame();
    let mut backend = SoftRenderer::new(texture_store(&[(b"img.png", {
        let image = RgbaImage::from_pixel(4, 4, Rgba([10, 20, 30, 255]));
        let mut out = Vec::new();
        PngEncoder::new(&mut out)
            .write_image(image.as_raw(), 4, 4, ExtendedColorType::Rgba8)
            .expect("encode fixture");
        out
    })]));
    let out = backend.render_frame(&frame);
    assert_eq!(out.surface.pixel(0, 0), Some([10, 20, 30, 255]));
    assert_eq!(out.surface.pixel(1, 1), Some([0, 255, 0, 255]));
    assert_eq!(out.surface.pixel(2, 2), Some([0, 255, 0, 255]));
    assert_eq!(out.surface.pixel(3, 3), Some([10, 20, 30, 255]));
}

#[test]
fn copy_texture_pass_executes_and_draws_snapshot() {
    let table = table();
    let mut gfx = state(Viewport::new(8, 8), [255, 255, 255, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"SRC".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"red".to_vec())),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", Value::Str(b"SRC".to_vec())),
                arg(&table, "CGACT", "ID2", Value::Str(b"DST".to_vec())),
                arg(&table, "CGACT", "COPY2", Value::Int(1)),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);
    let frame = gfx.frame();
    assert!(frame
        .passes
        .iter()
        .any(|pass| matches!(pass.target, yu_gfx::PassTarget::Texture(_))));

    let mut backend = SoftRenderer::new(texture_store(&[(
        b"red.png",
        png_bytes_for(Rgba([0, 0, 255, 255])),
    )]));
    let out = backend.render_frame(&frame);
    // EVID-T-11/T-12:COPY2 可执行,DST 从快照纹理绘制。
    assert!(
        !out.report
            .skipped
            .iter()
            .any(|skip| skip.kind == "copy" || skip.kind == "layer"),
        "COPY2 不应再被跳过: {:?}",
        out.report.skipped
    );
    assert_eq!(out.surface.pixel(0, 0), Some([0, 0, 255, 255]));
}

/// `DIV-GFX-051`:主组 `COPY ID=:/W0/` 在**首帧**(没有上一帧屏幕)必须
/// 复制设备初始化的后缓冲(= `frame.clear`),而不是报 `offscreen_missing`。
///
/// 证据:D3D9 设备创建后后缓冲即存在,内容为初始化清屏色(DIV-GFX-010);
/// 主组已直绘、没有离屏 RT(EVID-J3-07),所以首帧唯一可用的源就是它。
#[test]
fn main_group_copy_on_first_frame_uses_device_clear() {
    let table = table();
    let mut gfx = state(Viewport::new(4, 4), [10, 20, 30, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"CGS".to_vec())),
                arg(&table, "CG", "E", Value::Int(1)),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", Value::Str(b":/W0/".to_vec())),
                arg(&table, "CGACT", "ID2", Value::Str(b"CGS".to_vec())),
                arg(&table, "CGACT", "COPY2", Value::Int(1)),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);
    let frame = gfx.frame();
    assert!(
        frame
            .passes
            .iter()
            .any(|p| matches!(p.target, yu_gfx::PassTarget::Texture(_))),
        "主组 COPY 应输出纹理 pass"
    );

    // 首帧:渲染器没有 last_screen。
    let mut backend = SoftRenderer::new(texture_store(&[]));
    let out = backend.render_frame(&frame);
    assert!(
        !out.report.skipped.iter().any(|s| s.kind == "copy"),
        "首帧主组 COPY 不应被跳过: {:?}",
        out.report.skipped
    );
    // 快照内容 = 设备清屏色。
    assert_eq!(out.surface.pixel(0, 0), Some([10, 20, 30, 255]));
}

/// `DIV-GFX-053` + `DIV-GFX-054`:源层在同一帧被 `CGEND` 删除,且区域只给
/// `X/X2`(无 `Y/Y2`)。官方在命令时刻已完成复制(切片是整高),所以
/// 快照必须按命令时刻的源纹理 + 整高区域生成。
#[test]
fn copy_slices_full_height_after_source_layer_is_gone() {
    let table = table();
    let mut gfx = state(Viewport::new(8, 4), [0, 0, 0, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"TMP".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"red".to_vec())),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"DST".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"".to_vec())),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", Value::Str(b"TMP".to_vec())),
                arg(&table, "CGACT", "ID2", Value::Str(b"DST".to_vec())),
                arg(&table, "CGACT", "COPY2", Value::Int(1)),
                arg(&table, "CGACT", "X", Value::Int(0)),
                arg(&table, "CGACT", "X2", Value::Int(1)),
            ],
        ),
        // 源层在本帧稍后被删(ERIS 按钮快照的真实序列)。
        call(
            1,
            "CGEND",
            vec![arg(&table, "CGEND", "ID", Value::Str(b"TMP".to_vec()))],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);
    let frame = gfx.frame();
    let mut backend = SoftRenderer::new(texture_store(&[(
        b"red.png",
        png_bytes_for_wh(4, 4, Rgba([255, 0, 0, 255])),
    )]));
    let out = backend.render_frame(&frame);
    assert!(
        !out.report.skipped.iter().any(|s| s.kind == "copy"),
        "源层删除后 COPY 应仍可重放: {:?}",
        out.report.skipped
    );
}

/// `DIV-HOST-001`:渲染器 `reset()` 必须清掉快照;否则重启后新 `CopyId`
/// (从 0 重新分配)会命中旧快照并冻结早退。
#[test]
fn reset_drops_snapshots_so_new_copy_ids_do_not_alias() {
    let table = table();
    let mut backend = SoftRenderer::new(texture_store(&[
        (b"red.png", png_bytes_for_wh(2, 2, Rgba([255, 0, 0, 255]))),
        (b"blue.png", png_bytes_for_wh(2, 2, Rgba([0, 0, 255, 255]))),
    ]));
    let build = |color_file: &[u8], frame_no: u64| {
        let mut gfx = state(Viewport::new(4, 4), [0, 0, 0, 255]);
        gfx.consume_events(&[
            VmEvent::FrameStart { frame: frame_no },
            call(
                frame_no,
                "CG",
                vec![
                    arg(&table, "CG", "ID", Value::Str(b"SRC".to_vec())),
                    arg(&table, "CG", "FILE", Value::Str(color_file.to_vec())),
                ],
            ),
            call(
                frame_no,
                "CG",
                vec![
                    arg(&table, "CG", "ID", Value::Str(b"DST".to_vec())),
                    arg(&table, "CG", "FILE", Value::Str(b"".to_vec())),
                ],
            ),
            call(
                frame_no,
                "CGACT",
                vec![
                    arg(&table, "CGACT", "ID", Value::Str(b"SRC".to_vec())),
                    arg(&table, "CGACT", "ID2", Value::Str(b"DST".to_vec())),
                    arg(&table, "CGACT", "COPY2", Value::Int(1)),
                ],
            ),
            VmEvent::FrameEnd { frame: frame_no },
        ]);
        gfx.frame()
    };

    let first = build(b"red", 1);
    let out = backend.render_frame(&first);
    assert_eq!(out.surface.pixel(0, 0), Some([255, 0, 0, 255]));

    // “重启”:GfxState 重建 ⇒ CopyId 从 0 重新分配。
    backend.reset();
    backend.textures_mut().begin_frame();

    let second = build(b"blue", 1);
    let out = backend.render_frame(&second);
    assert_eq!(
        out.surface.pixel(0, 0),
        Some([0, 0, 255, 255]),
        "reset 后同 id 的快照必须重新捕获(不能冻结在旧像素)"
    );
}

fn png_bytes_for_wh(width: u32, height: u32, color: Rgba<u8>) -> Vec<u8> {
    let image = RgbaImage::from_pixel(width, height, color);
    let mut out = Vec::new();
    PngEncoder::new(&mut out)
        .write_image(image.as_raw(), width, height, ExtendedColorType::Rgba8)
        .expect("encode fixture");
    out
}

// ---------------------------------------------------------------- DIV-GFX-058
// 按钮快照与文本的时序微场景(官方:CGACT[TEXT] 写层表面,COPY 天然携带)。

fn green_pixel_count(surface: &yu_backend_soft::Surface) -> usize {
    surface
        .rgba
        .chunks_exact(4)
        .filter(|px| px[1] > 0 && px[1] >= px[0] && px[1] >= px[2])
        .count()
}

fn text_draw_event(table: &ParamTable, id: &[u8], x: i64) -> VmEvent {
    call(
        1,
        "CGACT",
        vec![
            arg(table, "CGACT", "ID", Value::Str(id.to_vec())),
            arg(table, "CGACT", "TEXT", Value::Int(1)),
            arg(table, "CGACT", "SET", Value::Int(0)),
            arg(table, "CGACT", "SETSTR", Value::Str(b"A".to_vec())),
            arg(table, "CGACT", "X", Value::Int(x)),
            arg(table, "CGACT", "Y", Value::Int(0)),
        ],
    )
}

fn copy2_event(table: &ParamTable, src: &[u8], dst: &[u8]) -> VmEvent {
    call(
        1,
        "CGACT",
        vec![
            arg(table, "CGACT", "ID", Value::Str(src.to_vec())),
            arg(table, "CGACT", "ID2", Value::Str(dst.to_vec())),
            arg(table, "CGACT", "COPY2", Value::Int(1)),
        ],
    )
}

/// 微场景 1(先文本后 COPY):快照携带命令时刻已画的文本,且跨帧保留。
#[test]
fn snapshot_after_text_carries_frozen_glyphs() {
    let table = table();
    let mut gfx = state(Viewport::new(16, 16), [255, 255, 255, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"SRC".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"dark".to_vec())),
                arg(&table, "CG", "E", Value::Int(0)),
                arg(&table, "CG", "SX", Value::Int(16)),
                arg(&table, "CG", "SY", Value::Int(16)),
            ],
        ),
        call(
            1,
            "FONT",
            vec![
                arg(&table, "FONT", "SX", Value::Int(16)),
                arg(&table, "FONT", "SY", Value::Int(16)),
                arg(&table, "FONT", "COLOR", Value::Int(0x00_FF00)),
            ],
        ),
        text_draw_event(&table, b"SRC", 0),
        copy2_event(&table, b"SRC", b"DST"),
        VmEvent::FrameEnd { frame: 1 },
    ]);

    let mut backend = SoftRenderer::new(texture_store(&[(
        b"dark.png",
        png_bytes_for_wh(16, 16, Rgba([0, 0, 0, 255])),
    )]));
    if !backend.text_composer().available() {
        println!("SKIP snapshot_after_text_carries_frozen_glyphs: no system font");
        return;
    }
    let out = backend.render_frame(&gfx.frame());
    assert!(
        green_pixel_count(&out.surface) > 0,
        "快照必须携带命令时刻已画的文本(源层不可见 ⇒ 绿字只能来自快照面)"
    );

    // 跨帧:快照冻结(EVID-T-12),不随后续帧变化;帧 2 无命令重放同帧。
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 2 },
        VmEvent::FrameEnd { frame: 2 },
    ]);
    let out2 = backend.render_frame(&gfx.frame());
    assert!(green_pixel_count(&out2.surface) > 0, "快照跨帧保留文本");
}

/// 微场景 2(先 COPY 后文本):命令之后才画的文本不得进入快照
/// (DIV-GFX-053 同门:命令时刻冻结)。
#[test]
fn text_after_copy_does_not_enter_snapshot() {
    let table = table();
    let mut gfx = state(Viewport::new(16, 16), [255, 255, 255, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"SRC".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"dark".to_vec())),
                arg(&table, "CG", "E", Value::Int(0)),
                arg(&table, "CG", "SX", Value::Int(16)),
                arg(&table, "CG", "SY", Value::Int(16)),
            ],
        ),
        call(
            1,
            "FONT",
            vec![
                arg(&table, "FONT", "SX", Value::Int(16)),
                arg(&table, "FONT", "SY", Value::Int(16)),
                arg(&table, "FONT", "COLOR", Value::Int(0x00_FF00)),
            ],
        ),
        copy2_event(&table, b"SRC", b"DST"),
        text_draw_event(&table, b"SRC", 0),
        VmEvent::FrameEnd { frame: 1 },
    ]);

    let mut backend = SoftRenderer::new(texture_store(&[(
        b"dark.png",
        png_bytes_for_wh(16, 16, Rgba([0, 0, 0, 255])),
    )]));
    if !backend.text_composer().available() {
        println!("SKIP text_after_copy_does_not_enter_snapshot: no system font");
        return;
    }
    let out = backend.render_frame(&gfx.frame());
    assert_eq!(
        green_pixel_count(&out.surface),
        0,
        "COPY 之后才画的文本不得进入快照;源层不可见 ⇒ 屏幕不应出现绿字"
    );

    // 跨帧:文本持久(DIV-GFX-058)但快照仍冻结在命令时刻。
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 2 },
        VmEvent::FrameEnd { frame: 2 },
    ]);
    let out2 = backend.render_frame(&gfx.frame());
    assert_eq!(green_pixel_count(&out2.surface), 0);
    assert_eq!(gfx.frame().text_draws.len(), 1, "源层文本跨帧保留");
}

/// 微场景 3(S4 按钮标签形状):文本直接画在快照精灵层上,画一次、
/// 跨帧常驻(官方烘焙语义;P8 修复的直接验收路径)。
#[test]
fn text_on_snapshot_layer_persists_across_frames() {
    let table = table();
    let mut gfx = state(Viewport::new(16, 16), [255, 255, 255, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"BTN".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"dark".to_vec())),
                arg(&table, "CG", "SX", Value::Int(16)),
                arg(&table, "CG", "SY", Value::Int(16)),
            ],
        ),
        copy2_event(&table, b"BTN", b"LBL"),
        call(
            1,
            "FONT",
            vec![
                arg(&table, "FONT", "SX", Value::Int(16)),
                arg(&table, "FONT", "SY", Value::Int(16)),
                arg(&table, "FONT", "COLOR", Value::Int(0x00_FF00)),
            ],
        ),
        text_draw_event(&table, b"LBL", 0),
        VmEvent::FrameEnd { frame: 1 },
    ]);

    let mut backend = SoftRenderer::new(texture_store(&[(
        b"dark.png",
        png_bytes_for_wh(16, 16, Rgba([0, 0, 0, 255])),
    )]));
    if !backend.text_composer().available() {
        println!("SKIP text_on_snapshot_layer_persists_across_frames: no system font");
        return;
    }
    let out = backend.render_frame(&gfx.frame());
    assert!(
        green_pixel_count(&out.surface) > 0,
        "快照精灵层上的标签文本必须显示(P8 修复前 layer_base 拒绝 Snapshot 源)"
    );

    // 帧 2(无任何命令):官方标签画一次即常驻。
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 2 },
        VmEvent::FrameEnd { frame: 2 },
    ]);
    let out2 = backend.render_frame(&gfx.frame());
    assert!(
        green_pixel_count(&out2.surface) > 0,
        "标签文本跨帧常驻(不再逐帧清空)"
    );
}

#[test]
fn screen_snapshot_carries_image_processed_layer() {
    let table = table();
    let mut backend = SoftRenderer::new(texture_store(&[(
        b"blue.png",
        png_bytes_for_wh(4, 4, Rgba([0, 0, 255, 255])),
    )]));
    let mut gfx = state(Viewport::new(4, 4), [0, 0, 0, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"SRC".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"blue".to_vec())),
                arg(&table, "CG", "Z", Value::Int(1)),
                arg(&table, "CG", "A", Value::Int(256)),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", Value::Str(b"SRC".to_vec())),
                arg(&table, "CGACT", "RECTPAINT", Value::Int(1)),
                arg(&table, "CGACT", "SET", Value::Int(0xff0000)),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", Value::Str(b":/W0/".to_vec())),
                arg(&table, "CGACT", "ID2", Value::Str(b"SNAP".to_vec())),
                arg(&table, "CGACT", "COPY2", Value::Int(1)),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"SNAP".to_vec())),
                arg(&table, "CG", "Z", Value::Int(10)),
                arg(&table, "CG", "A", Value::Int(256)),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);
    let out = backend.render_frame(&gfx.frame());
    assert_eq!(
        out.surface.pixel(0, 0),
        Some([255, 0, 0, 255]),
        "COPY2 :/W0/ 必须携带命令时刻已经 RECTPAINT 的层像素"
    );
}

/// DIV-GFX-072 §1-8:源层在同帧被 `CGEND` 删除,但 COPY2 已经在命令时刻
/// 读过它。回放 COPY 时必须取**加工后**像素(RECTPAINT 绿),不能退回基图红。
#[test]
fn copy_after_processing_survives_source_cg_end() {
    let table = table();
    let backend_textures =
        texture_store(&[(b"red.png", png_bytes_for_wh(4, 4, Rgba([255, 0, 0, 255])))]);
    let mut gfx = state(Viewport::new(4, 4), [0, 0, 0, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"SRC".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"red".to_vec())),
                arg(&table, "CG", "Z", Value::Int(1)),
                arg(&table, "CG", "A", Value::Int(256)),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", Value::Str(b"SRC".to_vec())),
                arg(&table, "CGACT", "RECTPAINT", Value::Int(1)),
                arg(&table, "CGACT", "SET", Value::Int(0x00ff00)),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", Value::Str(b"SRC".to_vec())),
                arg(&table, "CGACT", "ID2", Value::Str(b"SNAP".to_vec())),
                arg(&table, "CGACT", "COPY2", Value::Int(1)),
            ],
        ),
        call(
            1,
            "CGEND",
            vec![arg(&table, "CGEND", "ID", Value::Str(b"SRC".to_vec()))],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"SNAP".to_vec())),
                arg(&table, "CG", "Z", Value::Int(10)),
                arg(&table, "CG", "A", Value::Int(256)),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);
    let frame = gfx.frame();
    // 判别点:源层的 RECTPAINT 加工 pass 必须在帧内被发射(否则 COPY 只能
    // 退回命令时刻的红色基图)。
    assert!(
        frame.passes.iter().any(|p| {
            matches!(&p.target, yu_gfx::PassTarget::Texture(id)
                if id.display() == ":/W0/SRC")
                && p.ops.iter().any(|op| matches!(op, yu_gfx::Op::Image(_)))
        }),
        "已删源层的加工链必须为本帧 COPY 重放"
    );
    let mut backend = SoftRenderer::new(backend_textures);
    let out = backend.render_frame(&frame);
    assert_eq!(
        out.surface.pixel(0, 0),
        Some([0, 255, 0, 255]),
        "源层已 CGEND 时 COPY 仍必须取命令时刻的加工后像素(绿),不是基图(红)"
    );
}

/// `CG[MID]`:REA `0x42a862..0x42a88c` 要求两层宽高相等,逐像素
/// `target.a = mask.r`(RGB 不动)。mask 左上 R=0 ⇒ 该像素透明;
/// 其余 R=255 ⇒ 保持红色不透明。
#[test]
fn mid_uses_mask_red_channel_as_layer_alpha() {
    let table = table();
    let mut mask = RgbaImage::new(2, 2);
    mask.put_pixel(0, 0, Rgba([0, 9, 9, 255]));
    for y in 0..2 {
        for x in 0..2 {
            if (x, y) != (0, 0) {
                mask.put_pixel(x, y, Rgba([255, 9, 9, 255]));
            }
        }
    }
    let mut mask_png = Vec::new();
    PngEncoder::new(&mut mask_png)
        .write_image(mask.as_raw(), 2, 2, ExtendedColorType::Rgba8)
        .expect("encode mask");
    let store = texture_store(&[
        (b"red.png", png_bytes_for_wh(2, 2, Rgba([255, 0, 0, 255]))),
        (b"mask.png", mask_png),
    ]);
    let mut gfx = state(Viewport::new(2, 2), [0, 0, 0, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"L".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"red".to_vec())),
                arg(&table, "CG", "SX", Value::Int(2)),
                arg(&table, "CG", "SY", Value::Int(2)),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"M".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"mask".to_vec())),
                arg(&table, "CG", "E", Value::Int(0)),
                arg(&table, "CG", "SX", Value::Int(2)),
                arg(&table, "CG", "SY", Value::Int(2)),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"L".to_vec())),
                arg(&table, "CG", "MID", Value::Str(b"M".to_vec())),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);
    let frame = gfx.frame();
    let mask_op = frame
        .passes
        .iter()
        .flat_map(|p| p.ops.iter())
        .find_map(|op| match op {
            yu_gfx::Op::Image(i) if i.action.name() == "mask_alpha" => Some(i),
            _ => None,
        })
        .expect("MID 必须记录 mask_alpha 合成 op");
    assert_eq!(mask_op.status, yu_gfx::OpStatus::Executable);
    let mut backend = SoftRenderer::new(store);
    let out = backend.render_frame(&frame);
    assert_eq!(out.report.skipped_ops_total, 0);
    assert_eq!(
        out.surface.pixel(0, 0),
        Some([0, 0, 0, 255]),
        "mask R=0 ⇒ 层 alpha 0,只剩不透明黑底"
    );
    assert_eq!(out.surface.pixel(1, 0), Some([255, 0, 0, 255]));
    assert_eq!(out.surface.pixel(1, 1), Some([255, 0, 0, 255]));
}

/// EVID-J-08:子组 RT 的 alpha 不参与父级合成。组内 `K A=128` 的绿色先与
/// 黑底混合成 `(0,128,0)`,组再按自身 A(256)合成;不会在父级再乘一次 128。
#[test]
fn subgroup_rt_alpha_is_opaque_in_parent_composite() {
    let table = table();
    let store = texture_store(&[(b"green.png", png_bytes_for_wh(4, 4, Rgba([0, 255, 0, 255])))]);
    let mut gfx = state(Viewport::new(4, 4), [32, 64, 96, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"G/".to_vec())),
                arg(&table, "CG", "SX", Value::Int(4)),
                arg(&table, "CG", "SY", Value::Int(4)),
                arg(&table, "CG", "A", Value::Int(256)),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"G/K".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"green".to_vec())),
                arg(&table, "CG", "A", Value::Int(128)),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);
    let frame = gfx.frame();
    let mut backend = SoftRenderer::new(store);
    let out = backend.render_frame(&frame);
    assert_eq!(
        out.surface.pixel(1, 1),
        Some([0, 128, 0, 255]),
        "子组 RT alpha 必须按不透明处理(EVID-J-08)"
    );
}

/// `CG[FILE=""] SX/SY` 的 1×1 黑层在引擎里是 `SX×SY` 的黑色表面;
/// `CGACT[TEXT]` 的字符坐标相对这块表面。若文本基面仍按 1×1 算,
/// x=24 等字符全部落到图外 —— 字体预览(SMTX/FONTL1)整块空白。
#[test]
fn cgact_text_on_fileless_solid_layer_uses_explicit_surface_size() {
    let table = table();
    let mut gfx = state(Viewport::new(48, 24), [255, 255, 255, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"TX".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(Vec::new())),
                arg(&table, "CG", "SX", Value::Int(48)),
                arg(&table, "CG", "SY", Value::Int(24)),
            ],
        ),
        call(
            1,
            "FONT",
            vec![
                arg(&table, "FONT", "SX", Value::Int(16)),
                arg(&table, "FONT", "SY", Value::Int(16)),
                arg(&table, "FONT", "COLOR", Value::Int(0x00_FF00)),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", Value::Str(b"TX".to_vec())),
                arg(&table, "CGACT", "TEXT", Value::Int(1)),
                arg(&table, "CGACT", "SET", Value::Int(0)),
                arg(&table, "CGACT", "SETSTR", Value::Str(b"A".to_vec())),
                arg(&table, "CGACT", "X", Value::Int(24)),
                arg(&table, "CGACT", "Y", Value::Int(4)),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);
    let frame = gfx.frame();
    assert_eq!(frame.text_draws.len(), 1);

    let mut backend = SoftRenderer::new(texture_store(&[]));
    if !backend.text_composer().available() {
        println!(
            "SKIP cgact_text_on_fileless_solid_layer_uses_explicit_surface_size: no system font"
        );
        return;
    }
    let out = backend.render_frame(&frame);
    assert_eq!(out.report.text_draws, 1);
    assert_eq!(out.report.text_glyphs, 1);
    let green_right = out
        .surface
        .rgba
        .chunks_exact(4)
        .enumerate()
        .filter(|(i, px)| {
            // 只统计 x>=24 的像素:1×1 基面时这里必为纯黑。
            (i % 48) >= 24 && px[1] > 0 && px[1] >= px[0] && px[1] >= px[2]
        })
        .count();
    assert!(
        green_right > 0,
        "text at x=24 must land on the 48x24 solid layer"
    );
}

/// `COPY2` 局部区域:快照表面保持**源表面尺寸**,区域外清空。
///
/// 证据 EVID-T-11/`j2-micro-copy`;反例是 2026-11-05 商業包 環境設定:
/// 18×18 状态标被 `SIZE` 按 50×18 裁剪时横向拉伸,盖住背景图里的 on/off。
#[test]
fn partial_copy_keeps_source_surface_size() {
    let table = table();
    let mut gfx = state(Viewport::new(64, 24), [255, 255, 255, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"SRC".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"sheet".to_vec())),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"DST".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(Vec::new())),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", Value::Str(b"SRC".to_vec())),
                arg(&table, "CGACT", "ID2", Value::Str(b"DST".to_vec())),
                arg(&table, "CGACT", "COPY2", Value::Int(1)),
                arg(&table, "CGACT", "X", Value::Int(0)),
                arg(&table, "CGACT", "X2", Value::Int(17)),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);
    let frame = gfx.frame();
    let copy_id = frame
        .passes
        .iter()
        .flat_map(|pass| pass.ops.iter())
        .find_map(|op| match op {
            yu_gfx::Op::Copy(copy) => Some(copy.id),
            _ => None,
        })
        .expect("COPY2 pass");

    let mut sheet = RgbaImage::new(54, 18);
    for y in 0..18 {
        for x in 0..18 {
            sheet.put_pixel(x, y, Rgba([255, 0, 0, 255]));
        }
    }
    let mut png = Vec::new();
    PngEncoder::new(&mut png)
        .write_image(sheet.as_raw(), 54, 18, ExtendedColorType::Rgba8)
        .expect("encode fixture");

    let mut backend = SoftRenderer::new(texture_store(&[(b"sheet.png", png)]));
    let out = backend.render_frame(&frame);
    assert_eq!(out.report.skipped_ops_total, 0, "COPY2 应执行");
    let snapshot = backend
        .snapshot_surface(copy_id)
        .expect("partial copy snapshot");
    assert_eq!((snapshot.width, snapshot.height), (54, 18));
    assert_eq!(snapshot.pixel(0, 0), Some([255, 0, 0, 255]));
    assert_eq!(
        snapshot.pixel(30, 0),
        Some([0, 0, 0, 0]),
        "区域外必须清空为透明"
    );
}

/// `CG[FILE=""]` + `RECTPAINT3 SET=0` 后是**透明**表面(`Layer.fill`);
/// 文本仍按 `SX/SY` 的图层尺寸落位并可见。这是環境設定字体预览的真实
/// 链路:旧实现要么画成整块黑幕(忽略表面填充),要么用 1×1 基面把字裁掉。
#[test]
fn cgact_text_on_painted_transparent_solid_layer_is_visible() {
    let table = table();
    let mut gfx = state(Viewport::new(48, 24), [255, 255, 255, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"TX".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(Vec::new())),
                arg(&table, "CG", "SX", Value::Int(48)),
                arg(&table, "CG", "SY", Value::Int(24)),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", Value::Str(b"TX".to_vec())),
                arg(&table, "CGACT", "RECTPAINT3", Value::Int(1)),
                arg(&table, "CGACT", "SET", Value::Int(0)),
            ],
        ),
        call(
            1,
            "FONT",
            vec![
                arg(&table, "FONT", "SX", Value::Int(16)),
                arg(&table, "FONT", "SY", Value::Int(16)),
                arg(&table, "FONT", "COLOR", Value::Int(0x00_FF00)),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", Value::Str(b"TX".to_vec())),
                arg(&table, "CGACT", "TEXT", Value::Int(1)),
                arg(&table, "CGACT", "SET", Value::Int(0)),
                arg(&table, "CGACT", "SETSTR", Value::Str(b"A".to_vec())),
                arg(&table, "CGACT", "X", Value::Int(24)),
                arg(&table, "CGACT", "Y", Value::Int(4)),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);
    let frame = gfx.frame();
    assert_eq!(
        frame
            .layers
            .iter()
            .find(|layer| layer.id.display() == ":/W0/TX")
            .and_then(|layer| layer.fill),
        Some([0, 0, 0, 0])
    );
    let mut backend = SoftRenderer::new(texture_store(&[]));
    if !backend.text_composer().available() {
        println!("SKIP cgact_text_on_painted_transparent_solid_layer_is_visible: no system font");
        return;
    }
    let out = backend.render_frame(&frame);
    assert_eq!(out.report.text_draws, 1);
    assert_eq!(out.report.text_glyphs, 1);
    let green = out
        .surface
        .rgba
        .chunks_exact(4)
        .enumerate()
        .filter(|(i, px)| (i % 48) >= 24 && px[1] > 0 && px[1] >= px[0] && px[1] >= px[2])
        .count();
    assert!(green > 0, "text must be visible on the transparent layer");
    assert_eq!(
        out.surface.pixel(0, 0),
        Some([255, 255, 255, 255]),
        "RECTPAINT3 SET=0 后不得残留黑幕"
    );
}

/// 游戏内字体切换:同一层/同一文本,`FONT[NAME]` 变化必须改变像素;
/// 切回原字体必须逐字节还原(合成缓存不能留下旧字体)。
#[test]
fn font_switch_changes_pixels_and_restores_on_switch_back() {
    let table = table();
    let mut backend = SoftRenderer::new(texture_store(&[]));
    if !backend.text_composer().available() {
        println!("SKIP font_switch_changes_pixels_and_restores_on_switch_back: no system font");
        return;
    }
    let families = backend.text_composer().font_families();
    if families.len() < 2 {
        println!("SKIP font_switch_changes_pixels_and_restores_on_switch_back: <2 families");
        return;
    }
    let mut gfx = state(Viewport::new(64, 32), [0, 0, 0, 255]);
    let render = |backend: &mut SoftRenderer,
                  gfx: &mut yu_gfx::GfxState,
                  name: &[u8],
                  frame: u64,
                  recreate: bool| {
        let mut events = vec![VmEvent::FrameStart { frame }];
        if recreate {
            // 字体选择器重画时先清层:旧字体像素与绘制项都必须消失。
            events.push(call(
                frame,
                "CGEND",
                vec![arg(&table, "CGEND", "ID", Value::Str(b"TX".to_vec()))],
            ));
        }
        events.push(call(
            frame,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"TX".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(Vec::new())),
                arg(&table, "CG", "SX", Value::Int(64)),
                arg(&table, "CG", "SY", Value::Int(32)),
            ],
        ));
        events.push(call(
            frame,
            "FONT",
            vec![
                arg(&table, "FONT", "NAME", Value::Str(name.to_vec())),
                arg(&table, "FONT", "SX", Value::Int(24)),
                arg(&table, "FONT", "SY", Value::Int(24)),
                arg(&table, "FONT", "COLOR", Value::Int(0x00FF_FFFF)),
            ],
        ));
        events.push(call(
            frame,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", Value::Str(b"TX".to_vec())),
                arg(&table, "CGACT", "TEXT", Value::Int(1)),
                arg(&table, "CGACT", "SET", Value::Int(0)),
                // 'A':两个字体都应有该字形,形状差异可见。
                arg(&table, "CGACT", "SETSTR", Value::Str(b"A".to_vec())),
                arg(&table, "CGACT", "X", Value::Int(4)),
                arg(&table, "CGACT", "Y", Value::Int(4)),
            ],
        ));
        events.push(VmEvent::FrameEnd { frame });
        gfx.consume_events(&events);
        backend.render_frame(&gfx.frame()).surface
    };
    let first = render(&mut backend, &mut gfx, &families[0], 1, false);
    let other = render(&mut backend, &mut gfx, &families[1], 2, true);
    let again = render(&mut backend, &mut gfx, &families[0], 3, true);
    assert_eq!(
        first.rgba, again.rgba,
        "切回原字体必须逐字节还原(缓存不得残留)"
    );
    if first.rgba == other.rgba {
        println!(
            "SKIP font_switch_changes_pixels_and_restores_on_switch_back: first two families render identically"
        );
        return;
    }
    let ink = |rgba: &[u8]| rgba.chunks_exact(4).any(|px| px[..3] != [0, 0, 0]);
    assert!(ink(&first.rgba), "第一个字体必须真的画出字形");
    assert!(ink(&other.rgba), "第二个字体必须真的画出字形");

    // 引擎 restart = 新 GfxState(修订号从 0 重计)+ 渲染器 reset。
    // reset 必须清合成/基面缓存,否则同 (layer, revision) 会复用旧字体纹理。
    backend.reset();
    let mut restarted = state(Viewport::new(64, 32), [0, 0, 0, 255]);
    let cross_session = render(&mut backend, &mut restarted, &families[1], 1, false);
    assert_eq!(
        cross_session.rgba, other.rgba,
        "reset 后同修订号必须按新字体重算,不得命中旧会话纹理"
    );
}

/// 逐字符样式必须真的应用到像素:同一行两个字形不同前景色。
///
/// 这条用例防的是“逐字样式只进了模型、合成器仍用整段样式”的静默回归:
/// 若合成器忽略 `glyph.style`，两字都会用 `TextFrame.font` 的默认白色，
/// 红/蓝通道断言失败。
#[test]
fn per_glyph_foreground_colors_reach_pixels() {
    let table = table();
    let mut gfx = state(Viewport::new(96, 32), [0, 0, 0, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"TX".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(Vec::new())),
                arg(&table, "CG", "SX", Value::Int(96)),
                arg(&table, "CG", "SY", Value::Int(32)),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);
    let glyph = |ch: u8, x: i64, fg: u32| {
        yu_rt::text::TextStep::Glyph(yu_rt::text::TextGlyphDraw {
            codec: yu_rt::Codec::Cp932,
            bytes: [ch].into_iter().collect(),
            full: false,
            x,
            y: 0,
            width: 20,
            height: 20,
            style: yu_rt::text::TextGlyphStyle {
                fg,
                bg: 0x000000,
                antialias: true,
                gradation: 0,
                bold: false,
            },
            draw_mode: 0,
            bno: 1,
        })
    };
    let frame_data = yu_rt::text::TextFrame {
        layer_id: b"TX".to_vec(),
        output_mode: 1,
        page_clear_seq: 0,
        cursor_x: 40,
        cursor_y: 0,
        click: false,
        bytes: b"AB".to_vec(),
        flags: b"11".to_vec(),
        // 默认白色:如果逐字 fg 没生效,像素会是白色而不是红/蓝。
        font: Default::default(),
        first_step: 0,
        total_steps: 2,
        steps: vec![glyph(b'A', 0, 0x00FF_0000), glyph(b'B', 20, 0x0000_00FF)],
    };
    gfx.apply_text_frame(&frame_data);
    let mut backend = SoftRenderer::new(texture_store(&[]));
    if !backend.text_composer().available() {
        println!("SKIP per_glyph_foreground_colors_reach_pixels: no system font");
        return;
    }
    let out = backend.render_frame(&gfx.frame());
    let mut max_red_left = 0u8;
    let mut max_blue_right = 0u8;
    let mut max_green_any = 0u8;
    for y in 0..out.surface.height {
        for x in 0..out.surface.width {
            let px = out.surface.pixel(x, y).expect("pixel");
            if x < 20 {
                max_red_left = max_red_left.max(px[0]);
            } else if (20..40).contains(&x) {
                max_blue_right = max_blue_right.max(px[2]);
            }
            max_green_any = max_green_any.max(px[1]);
        }
    }
    assert!(max_red_left > 0, "第一个字形必须用红色前景(逐字 fg 生效)");
    assert!(max_blue_right > 0, "第二个字形必须用蓝色前景(逐字 fg 生效)");
    // 红色字形不应出现蓝色像素,蓝色字形不应出现红色像素。
    assert_eq!(max_green_any, 0, "红/蓝前景不应混入绿色/白色");
}

/// CG 鉴赏预览:`RECTPAINT3 白底 → MERGE CG → COPY2 → CGEND` 同一 tick。
///
/// 官方 `CGACT` 命令体内立即执行;`CGEND` 删除白底不会撤销已经合成的像素。
/// 延迟回放若因目标层消失而丢掉 `MERGE`/丢失白底的 `SX/SY`,快照会退化成
/// 1×1 白,整屏白色(`es_cgmode.yst` pc533..536)。
#[test]
fn merge_into_target_deleted_same_tick_keeps_copy2_pixels() {
    let table = table();
    let mut gfx = state(Viewport::new(16, 16), [255, 255, 255, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"CG".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"green".to_vec())),
                arg(&table, "CG", "E", Value::Int(0)),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"BUF".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(Vec::new())),
                arg(&table, "CG", "SX", Value::Int(16)),
                arg(&table, "CG", "SY", Value::Int(16)),
                arg(&table, "CG", "A", Value::Int(0)),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"DST".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(Vec::new())),
                arg(&table, "CG", "SX", Value::Int(16)),
                arg(&table, "CG", "SY", Value::Int(16)),
                arg(&table, "CG", "E", Value::Int(1)),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", Value::Str(b"BUF".to_vec())),
                arg(&table, "CGACT", "RECTPAINT3", Value::Int(1)),
                arg(&table, "CGACT", "SET", Value::Int(0xFFFF_FFFF)),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", Value::Str(b"CG".to_vec())),
                arg(&table, "CGACT", "ID2", Value::Str(b"BUF".to_vec())),
                arg(&table, "CGACT", "MERGE", Value::Int(1)),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", Value::Str(b"BUF".to_vec())),
                arg(&table, "CGACT", "ID2", Value::Str(b"DST".to_vec())),
                arg(&table, "CGACT", "COPY2", Value::Int(1)),
            ],
        ),
        call(
            1,
            "CGEND",
            vec![arg(&table, "CGEND", "ID", Value::Str(b"BUF".to_vec()))],
        ),
        call(
            1,
            "CGEND",
            vec![arg(&table, "CGEND", "ID", Value::Str(b"CG".to_vec()))],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);

    let mut backend = SoftRenderer::new(texture_store(&[(
        b"green.png",
        png_bytes_for_wh(16, 16, Rgba([0, 255, 0, 255])),
    )]));
    let out = backend.render_frame(&gfx.frame());
    let px = out.surface.pixel(8, 8).expect("centre pixel");
    assert_eq!(
        px,
        [0, 255, 0, 255],
        "白底被 CGEND 后,MERGE 的 CG 像素仍须进入 COPY2 快照"
    );
}

/// 活目标层被 `CG[FILE]` 换图后,历史图像加工必须作用在**当前**纹理上。
///
/// hover/缩略图槽位复用:旧 `MERGE2` 记录若用命令时刻的旧基面重放,
/// 会把上一张图写回当前槽位。只有目标层已删除时才允许用 `target_base`。
#[test]
fn live_target_replay_uses_current_cg_file_not_recorded_base() {
    let table = table();
    let mut gfx = state(Viewport::new(4, 4), [0, 0, 0, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"BUF".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"red".to_vec())),
                arg(&table, "CG", "SX", Value::Int(4)),
                arg(&table, "CG", "SY", Value::Int(4)),
                arg(&table, "CG", "E", Value::Int(1)),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"UP".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"clear".to_vec())),
                arg(&table, "CG", "E", Value::Int(0)),
            ],
        ),
        // 记录一次历史加工:upper 全透明 ⇒ 下层像素应保持。
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", Value::Str(b"UP".to_vec())),
                arg(&table, "CGACT", "ID2", Value::Str(b"BUF".to_vec())),
                arg(&table, "CGACT", "MERGE2", Value::Int(1)),
            ],
        ),
        // 同 tick 换图:hover 槽位切到下一张。
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"BUF".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"blue".to_vec())),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);

    let mut backend = SoftRenderer::new(texture_store(&[
        (
            b"red.png",
            png_bytes_for_wh(4, 4, Rgba([255, 0, 0, 255])),
        ),
        (
            b"blue.png",
            png_bytes_for_wh(4, 4, Rgba([0, 0, 255, 255])),
        ),
        (
            b"clear.png",
            png_bytes_for_wh(4, 4, Rgba([0, 0, 0, 0])),
        ),
    ]));
    let out = backend.render_frame(&gfx.frame());
    let px = out.surface.pixel(2, 2).expect("centre pixel");
    assert_eq!(
        px,
        [0, 0, 255, 255],
        "活目标层必须用换图后的 blue 基面,不能用记录里的 red"
    );
}

/// CG 鉴赏 hover/缩略图槽位复用:`_TMP_OF`/`_TMP_OV` 每张图都同名重建。
/// 第二张图不得复用第一张图的 `COPY2/MERGE2` 中间结果。
#[test]
fn gallery_reused_thumbnail_slots_do_not_leak_previous_image() {
    let table = table();
    let mut gfx = state(Viewport::new(4, 4), [0, 0, 0, 255]);
    let mut events = vec![VmEvent::FrameStart { frame: 1 }];
    for (btn, btn2) in [("BTN", "BTN2")] {
        events.push(call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(btn.as_bytes().to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"base".to_vec())),
                arg(&table, "CG", "SX", Value::Int(4)),
                arg(&table, "CG", "SY", Value::Int(4)),
                arg(&table, "CG", "E", Value::Int(1)),
            ],
        ));
        events.push(call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(btn2.as_bytes().to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"base".to_vec())),
                arg(&table, "CG", "SX", Value::Int(4)),
                arg(&table, "CG", "SY", Value::Int(4)),
                arg(&table, "CG", "E", Value::Int(1)),
            ],
        ));
    }
    for image in [b"red".as_slice(), b"blue".as_slice()] {
        events.push(call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"_TMP_OF".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(image.to_vec())),
                arg(&table, "CG", "E", Value::Int(0)),
            ],
        ));
        events.push(call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"_TMP_OV".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(Vec::new())),
                arg(&table, "CG", "E", Value::Int(0)),
            ],
        ));
        events.push(copy2_event(&table, b"_TMP_OF", b"_TMP_OV"));
        for (src, dst) in [
            (b"_TMP_OF".as_slice(), b"BTN".as_slice()),
            (b"_TMP_OV".as_slice(), b"BTN2".as_slice()),
        ] {
            events.push(call(
                1,
                "CGACT",
                vec![
                    arg(&table, "CGACT", "ID", Value::Str(src.to_vec())),
                    arg(&table, "CGACT", "ID2", Value::Str(dst.to_vec())),
                    arg(&table, "CGACT", "MERGE2", Value::Int(1)),
                ],
            ));
        }
        events.push(call(
            1,
            "CGEND",
            vec![arg(&table, "CGEND", "ID", Value::Str(b"_TMP_OF".to_vec()))],
        ));
        events.push(call(
            1,
            "CGEND",
            vec![arg(&table, "CGEND", "ID", Value::Str(b"_TMP_OV".to_vec()))],
        ));
    }
    events.push(VmEvent::FrameEnd { frame: 1 });
    gfx.consume_events(&events);

    let mut backend = SoftRenderer::new(texture_store(&[
        (
            b"base.png",
            png_bytes_for_wh(4, 4, Rgba([0, 0, 0, 255])),
        ),
        (
            b"red.png",
            png_bytes_for_wh(4, 4, Rgba([255, 0, 0, 255])),
        ),
        (
            b"blue.png",
            png_bytes_for_wh(4, 4, Rgba([0, 0, 255, 255])),
        ),
    ]));
    let out = backend.render_frame(&gfx.frame());
    for (x, y, label) in [(1, 1, "BTN"), (3, 1, "BTN2")] {
        let px = out.surface.pixel(x, y).expect("button pixel");
        assert!(
            px[2] > 200 && px[0] < 8,
            "{label} 必须显示第二张图 blue,实际 {px:?}(上一张 red 泄漏)"
        );
    }
}
