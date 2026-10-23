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
                &["ID", "FILE", "E", "A", "X", "Y", "Z", "SX", "SY", "MODE"],
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
                    "RECTPAINT",
                    "RECTPAINT2",
                    "RECTPAINT3",
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

fn texture_store(entries: &[(&[u8], Vec<u8>)]) -> TextureStore {
    let mut source = MemSource::new(Codec::Cp932);
    for (path, bytes) in entries {
        source.insert(path, bytes.clone()).expect("insert");
    }
    TextureStore::new(Box::new(source), Codec::Cp932, vec![ImageFormat::Png])
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
fn text_without_flags_is_counted_but_not_rasterized() {
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
fn synced_text_flags_rasterize_glyphs() {
    let mut gfx = state(Viewport::new(32, 32), [0, 0, 0, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        VmEvent::Text {
            frame: 1,
            id: String::new(),
            bytes: b"A".to_vec(),
        },
    ]);
    // 权威 `_TEXT_FLAG`(VM `[state+0x3c]`):'2' = 全角单元。
    let mut snapshot = yu_rt::text::TextSnapshot {
        // 全角「こ」(CP932 0x82B1)配 '2';对齐必须成功。
        bytes: b"\x82\xB1".to_vec(),
        flags: b"2".to_vec(),
        output_mode: 1,
        wait_ms: 0,
        autoreturn: 1,
        autopage: 1,
        autoclick: 1,
        click: false,
        clear_pending: false,
        cursor_x: 0,
        cursor_y: 0,
        click_buttons: [1, 13, 0x3ff, 0x3ff],
        skip_buttons: [17, 0x3ff, 0x3ff, 0x3ff],
    };
    gfx.sync_text_state(&snapshot);
    let frame = gfx.frame();
    assert!(frame.text_layout.is_some(), "layout comes from _TEXT_FLAG");

    let mut backend = SoftRenderer::new(texture_store(&[]));
    if !backend.text_composer().available() {
        println!("SKIP synced_text_flags_rasterize_glyphs: no system font");
        return;
    }
    let out = backend.render_frame(&frame);
    assert_eq!(out.report.skipped_text, 0, "synced run is not skipped");
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

    // 清空同步(CLEAR)后文本消失。
    snapshot.bytes.clear();
    snapshot.flags.clear();
    gfx.consume_events(&[VmEvent::FrameStart { frame: 2 }]);
    gfx.sync_text_state(&snapshot);
    let cleared = backend.render_frame(&gfx.frame());
    assert_eq!(cleared.report.text_glyphs, 0);
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
