//! 软件后端端到端:合成命令表 → GfxState → CompositorFrame → RGBA8/PNG。
//!
//! 夹具不依赖真实游戏数据:命令表只保留本测试用到的参数;纹理用
//! `MemSource` + 内存 PNG。

use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder, Rgba, RgbaImage};
use yu_backend_soft::{frame_has_drawable_output, png, SoftRenderer, TextureStore};
use yu_format::yscm::{Arg as FmtArg, Command as FmtCommand, CommandTable};
use yu_gfx::{
    Clear, GfxConfig, GfxState, ImageFormat, ParamTable, Viewport,
};
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
                flag: if matches!(*param, "ID" | "ID2" | "FILE") {
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
                &["ID", "FILE", "E", "A", "X", "Y", "Z", "SX", "SY", "MODE"],
            ),
            command("CGACT", &[
                "ID",
                "ID2",
                "COPY2",
                "SET",
                "X",
                "Y",
                "X2",
                "Y2",
                "RECTPAINT",
                "RECTPAINT2",
                "RECTPAINT3",
            ]),
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

fn texture_store(entries: &[(&[u8], Vec<u8>)]) -> TextureStore {
    let mut source = MemSource::new(Codec::Cp932);
    for (path, bytes) in entries {
        source.insert(path, bytes.clone()).expect("insert");
    }
    TextureStore::new(
        Box::new(source),
        Codec::Cp932,
        vec![ImageFormat::Png],
    )
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

    assert_eq!(out.surface.pixel(0, 0), Some([0, 0, 0, 255]));
    assert_eq!(out.surface.pixel(3, 3), Some([0, 0, 0, 255]));
    assert_eq!(out.surface.pixel(4, 0), Some([255, 255, 255, 255]));
    // 主组直绘:只有那一层 op(组不再单独计数)。
    assert_eq!(out.report.ops_drawn, 1);
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
fn text_runs_are_counted_but_not_rasterized() {
    let table = table();
    let mut gfx = state(Viewport::new(8, 8), [0, 0, 0, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        VmEvent::Text {
            frame: 1,
            id: "T".to_string(),
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
    let mut backend = SoftRenderer::new(texture_store(&[(
        b"img.png",
        {
            let image = RgbaImage::from_pixel(4, 4, Rgba([10, 20, 30, 255]));
            let mut out = Vec::new();
            PngEncoder::new(&mut out)
                .write_image(image.as_raw(), 4, 4, ExtendedColorType::Rgba8)
                .expect("encode fixture");
            out
        },
    )]));
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
