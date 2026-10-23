//! wgpu 后端端到端:合成命令表 → GfxState → CompositorFrame → GPU → RGBA8。
//!
//! 与软后端测试分开:这里验证 GPU 路径、readback 行对齐、纹理缓存与
//! “计数与软后端一致”。夹具不依赖真实游戏数据。

use std::sync::atomic::{AtomicUsize, Ordering};

use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder, Rgba, RgbaImage};
use yu_backend_soft::{SoftRenderer, TextureStore};
use yu_backend_wgpu::device::code;
use yu_backend_wgpu::{DeviceOptions, ScaleFilter, WgpuRenderOutput, WgpuRenderer};
use yu_format::yscm::{Arg as FmtArg, Command as FmtCommand, CommandTable};
use yu_gfx::{Clear, GfxConfig, GfxState, ImageFormat, ParamTable, Viewport};
use yu_rt::event::{ArgValue, Domain, VmEvent};
use yu_rt::tables::YscmTable;
use yu_rt::value::Value;
use yu_vfs::{Codec, MemSource};

static SKIPPED: AtomicUsize = AtomicUsize::new(0);

fn command(name: &str, params: &[&str]) -> FmtCommand {
    FmtCommand {
        name: name.into(),
        argc: params.len() as u8,
        args: params
            .iter()
            .map(|param| FmtArg {
                name: (*param).into(),
                flag: if matches!(*param, "ID" | "ID2" | "FILE" | "TID" | "NAME" | "SETSTR") {
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
                    "ID", "FILE", "E", "A", "X", "Y", "Z", "SX", "SY", "MODE", "T", "TA", "TID",
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
                    "REVERSE",
                    "HORIZONTAL",
                    "TEXT",
                    "SETSTR",
                    "BNO",
                ],
            ),
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

fn png_bytes_for(width: u32, height: u32, color: Rgba<u8>) -> Vec<u8> {
    let image = RgbaImage::from_pixel(width, height, color);
    let mut out = Vec::new();
    PngEncoder::new(&mut out)
        .write_image(image.as_raw(), width, height, ExtendedColorType::Rgba8)
        .expect("encode fixture");
    out
}

fn store(entries: &[(&[u8], Vec<u8>)]) -> TextureStore {
    let mut source = MemSource::new(Codec::Cp932);
    for (path, bytes) in entries {
        source.insert(path, bytes.clone()).expect("insert");
    }
    TextureStore::new(Box::new(source), Codec::Cp932, vec![ImageFormat::Png])
}

fn renderer_or_skip(store: TextureStore) -> Option<WgpuRenderer> {
    match WgpuRenderer::new(store) {
        Ok(renderer) => Some(renderer),
        Err(diag) if diag.code == code::ADAPTER_UNAVAILABLE => {
            SKIPPED.fetch_add(1, Ordering::Relaxed);
            println!(
                "SKIP yu-backend-wgpu: 无适配器 [{}] {}",
                diag.code, diag.detail
            );
            None
        }
        Err(diag) => panic!("GPU 初始化失败 [{}]: {}", diag.code, diag.detail),
    }
}

fn pixel(output: &WgpuRenderOutput, x: u32, y: u32) -> [u8; 4] {
    let i = ((y as usize) * (output.surface.width as usize) + x as usize) * 4;
    let rgba = &output.surface.rgba;
    [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
}

fn assert_close(actual: [u8; 4], expected: [u8; 4], tolerance: u8, what: &str) {
    for channel in 0..4 {
        let diff = actual[channel].abs_diff(expected[channel]);
        assert!(
            diff <= tolerance,
            "{what}: channel {channel} actual={:?} expected={:?} (tol {tolerance})",
            actual,
            expected
        );
    }
}

#[test]
fn device_initializes_or_skips() {
    let Some(renderer) = renderer_or_skip(store(&[])) else {
        return;
    };
    let adapter = renderer.adapter();
    assert!(!adapter.name.is_empty());
    assert!(matches!(
        adapter.backend.as_str(),
        "vulkan" | "dx12" | "gl" | "metal" | "noop" | "browser_webgpu"
    ));
    println!(
        "adapter: {} [{}] {} (skipped renderers so far: {})",
        adapter.name,
        adapter.backend,
        adapter.device_type,
        SKIPPED.load(Ordering::Relaxed)
    );
}

#[test]
fn missing_adapter_is_explicit_error() {
    let options = DeviceOptions {
        adapter_name: Some("__yu-backend-wgpu-no-such-adapter__".to_string()),
        ..DeviceOptions::default()
    };
    let err = match WgpuRenderer::new_with_options(store(&[]), &options) {
        Ok(_) => panic!("bogus adapter name must fail"),
        Err(diag) => diag,
    };
    assert_eq!(err.code, code::ADAPTER_UNAVAILABLE);
    assert!(
        err.detail.contains("__yu-backend-wgpu-no-such-adapter__"),
        "detail should name the filter: {}",
        err.detail
    );
}

#[test]
fn clear_only_frame_matches_explicit_clear() {
    let mut gfx = state(Viewport::new(3, 2), [10, 20, 30, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        VmEvent::FrameEnd { frame: 1 },
    ]);
    let frame = gfx.frame();
    let Some(mut renderer) = renderer_or_skip(store(&[])) else {
        return;
    };
    let out = renderer.render_frame(&frame).expect("render");
    assert_eq!((out.surface.width, out.surface.height), (3, 2));
    for y in 0..2 {
        for x in 0..3 {
            assert_eq!(pixel(&out, x, y), [10, 20, 30, 255], "pixel {x},{y}");
        }
    }
    assert_eq!(out.report.ops_total, 0);
    assert_eq!(out.stats.draw_calls, 0);
}

#[test]
fn asset_layer_composites_with_padded_rows() {
    let table = table();
    let mut gfx = state(Viewport::new(3, 2), [255, 255, 255, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"A".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"red".to_vec())),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);
    let frame = gfx.frame();
    let Some(mut renderer) = renderer_or_skip(store(&[(
        b"red.png",
        png_bytes_for(2, 2, Rgba([255, 0, 0, 255])),
    )])) else {
        return;
    };
    let out = renderer.render_frame(&frame).expect("render");

    assert_eq!((out.surface.width, out.surface.height), (3, 2));
    assert_close(pixel(&out, 0, 0), [255, 0, 0, 255], 1, "red 0,0");
    assert_close(pixel(&out, 1, 1), [255, 0, 0, 255], 1, "red 1,1");
    assert_close(
        pixel(&out, 2, 1),
        [255, 255, 255, 255],
        1,
        "uncovered clear",
    );
    // 主组直绘:只有 asset 图层 op 与 1 次 draw(DIV-GFX-008/041)。
    assert_eq!(out.report.ops_drawn, 1);
    assert_eq!(out.stats.draw_calls, 1);
    assert_eq!(out.stats.readback_bytes, 3 * 2 * 4);
}

#[test]
fn repeated_asset_uploads_once_and_cache_survives_frames() {
    let table = table();
    let mut gfx = state(Viewport::new(4, 4), [0, 0, 0, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"A".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"red".to_vec())),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"B".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"red".to_vec())),
                arg(&table, "CG", "X", Value::Int(2)),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);
    let frame = gfx.frame();
    let Some(mut renderer) = renderer_or_skip(store(&[(
        b"red.png",
        png_bytes_for(2, 2, Rgba([255, 0, 0, 255])),
    )])) else {
        return;
    };
    let first = renderer.render_frame(&frame).expect("render");
    // 同一资产被两层引用:只上传一次;draw = 2 个直绘图层。
    assert_eq!(first.stats.texture_uploads, 1);
    assert_eq!(first.stats.upload_bytes, 2 * 2 * 4);
    assert_eq!(first.report.ops_drawn, 2);
    assert_eq!(first.stats.draw_calls, 2);

    let second = renderer.render_frame(&frame).expect("render again");
    assert_eq!(second.stats.texture_uploads, 0, "second frame must reuse");
    assert_eq!(second.stats.total_texture_uploads, 1);
    assert_eq!(second.surface.rgba, first.surface.rgba);
    assert_eq!(second.stats.pipelines, 1, "only MODE 0 pipeline");
}

#[test]
fn unsupported_blend_counts_match_soft_backend() {
    let table = table();
    let mut gfx = state(Viewport::new(4, 4), [255, 255, 255, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"M".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"red".to_vec())),
                arg(&table, "CG", "MODE", Value::Int(4)),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);
    let frame = gfx.frame();

    let Some(mut gpu) = renderer_or_skip(store(&[(
        b"red.png",
        png_bytes_for(1, 1, Rgba([255, 0, 0, 255])),
    )])) else {
        return;
    };
    let gpu_out = gpu.render_frame(&frame).expect("render");
    let mut soft = SoftRenderer::new(store(&[(
        b"red.png",
        png_bytes_for(1, 1, Rgba([255, 0, 0, 255])),
    )]));
    let soft_out = soft.render_frame(&frame);

    assert_eq!(gpu_out.report.passes, soft_out.report.passes);
    assert_eq!(gpu_out.report.ops_total, soft_out.report.ops_total);
    assert_eq!(gpu_out.report.ops_drawn, soft_out.report.ops_drawn);
    assert_eq!(
        gpu_out.report.skipped_ops_total,
        soft_out.report.skipped_ops_total
    );
    assert_eq!(gpu_out.report.skipped, soft_out.report.skipped);
    assert_eq!(gpu_out.report.layers, soft_out.report.layers);
    assert_eq!(
        gpu_out.report.visible_layers,
        soft_out.report.visible_layers
    );
    assert_eq!(
        gpu_out.report.offscreen_passes,
        soft_out.report.offscreen_passes
    );
    // 不做跨后端逐像素门禁(任务书 §0/§3):这里只对账计数与跳过原因。
}

#[test]
fn group_alpha_is_applied_once() {
    let table = table();
    let mut gfx = state(Viewport::new(4, 4), [255, 255, 255, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b":/W0/G/".to_vec())),
                arg(&table, "CG", "A", Value::Int(128)),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b":/W0/G/L".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"red".to_vec())),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);
    let frame = gfx.frame();
    let Some(mut renderer) = renderer_or_skip(store(&[(
        b"red.png",
        png_bytes_for(4, 4, Rgba([255, 0, 0, 255])),
    )])) else {
        return;
    };
    let out = renderer.render_frame(&frame).expect("render");
    // 主组 G 的离屏内容是不透明红 (255,0,0);G 以 A=128 直接合成到白屏
    // (DIV-GFX-008/041 主组直绘:主窗口组没有自己的 RT,不存在“再乘一次
    // 组 alpha”):
    //   R = 255·a + 255·(1−a) = 255(a=128/255)
    //   G/B = 0·a + 255·(1−a) = 127
    // 屏幕是 opaque 窗口,最终读回 alpha 归一为 255(见 render_frame)。
    assert_close(pixel(&out, 0, 0), [255, 127, 127, 255], 2, "group alpha");
    assert_close(pixel(&out, 3, 3), [255, 127, 127, 255], 2, "group alpha");
}

/// Phase B 验收(EVID-J-02..05):RECTPAINT/2/3 在 soft 与 wgpu 上逐像素一致。
///
/// 同一帧:不透明红(RECTPAINT)、绿+alpha 0x80(RECTPAINT + RECTPAINT2);
/// 16 个像素中一半是半透明。填充是精确的 u8 运算,两后端应逐字节一致。
#[test]
fn rectpaint_family_matches_soft_backend() {
    let table = table();
    let mut gfx = state(Viewport::new(8, 4), [255, 255, 255, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
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

    let Some(mut gpu) = renderer_or_skip(store(&[])) else {
        return;
    };
    let gpu_out = gpu.render_frame(&frame).expect("gpu render");
    let mut soft = SoftRenderer::new(store(&[]));
    let soft_out = soft.render_frame(&frame);

    let mut diff_pixels = 0usize;
    let mut max_diff = 0u8;
    for (gpu_px, soft_px) in gpu_out
        .surface
        .rgba
        .chunks_exact(4)
        .zip(soft_out.surface.rgba.chunks_exact(4))
    {
        let d = (0..4)
            .map(|i| gpu_px[i].abs_diff(soft_px[i]))
            .max()
            .unwrap();
        if d > 0 {
            diff_pixels += 1;
        }
        max_diff = max_diff.max(d);
    }
    println!(
        "rectpaint parity: diff_pixels={diff_pixels}/{} max_channel_diff={max_diff}",
        gpu_out.surface.rgba.len() / 4
    );
    assert_eq!(max_diff, 0, "RECTPAINT 像素应逐字节一致");
    assert_eq!(
        gpu_out.report, soft_out.report,
        "双后端 RenderReport 必须逐字段一致"
    );
}

/// Phase A 验收(EVID-J-01):同一半透明帧,soft 与 wgpu 逐通道差 ≤1。
///
/// 组 A=128 的 4×4 红层覆盖整屏(内部合成半透明);最终屏幕是 opaque,
/// 读回 alpha 归一 255。控制断言改为检查帧里确实有 A<256 的组,避免拿
/// 全不透明帧做 0 差冒烟。
#[test]
fn semi_transparent_pixels_match_soft_within_one() {
    let table = table();
    let mut gfx = state(Viewport::new(4, 4), [255, 255, 255, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b":/W0/G/".to_vec())),
                arg(&table, "CG", "A", Value::Int(128)),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b":/W0/G/L".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"red".to_vec())),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);
    let frame = gfx.frame();
    let fixture = [(
        b"red.png".as_slice(),
        png_bytes_for(4, 4, Rgba([255, 0, 0, 255])),
    )];
    let Some(mut gpu) = renderer_or_skip(store(&fixture)) else {
        return;
    };
    let gpu_out = gpu.render_frame(&frame).expect("gpu render");
    let mut soft = SoftRenderer::new(store(&fixture));
    let soft_out = soft.render_frame(&frame);

    let mut diff_pixels = 0usize;
    let mut max_diff = 0u8;
    for (gpu_px, soft_px) in gpu_out
        .surface
        .rgba
        .chunks_exact(4)
        .zip(soft_out.surface.rgba.chunks_exact(4))
    {
        let d = (0..4)
            .map(|i| gpu_px[i].abs_diff(soft_px[i]))
            .max()
            .unwrap();
        if d > 0 {
            diff_pixels += 1;
        }
        max_diff = max_diff.max(d);
    }
    println!(
        "semi-transparent parity: diff_pixels={diff_pixels}/{} max_channel_diff={max_diff}",
        gpu_out.surface.rgba.len() / 4
    );
    assert!(
        frame
            .layers
            .iter()
            .any(|layer| layer.is_group() && layer.alpha < 256),
        "controle: 必须有半透明组,否则测试无意义"
    );
    assert!(
        max_diff <= 1,
        "soft/wgpu 半透明像素逐通道差 {max_diff} > 1 (diff_pixels={diff_pixels})"
    );
    assert_eq!(
        gpu_out.report, soft_out.report,
        "双后端 RenderReport 必须逐字段一致"
    );
}

/// `_DXMODE` 双路径:psC(mask R)与固定管线 G-mask 产生不同覆盖度。
#[test]
fn dx_mode_selects_engine_psc_transition() {
    let build = |dx_mode: u32| {
        let table = table();
        let mut config = GfxConfig::new(Viewport::new(4, 4));
        config.clear = Clear::explicit([0, 0, 0, 255]);
        config.image_probe_order = vec![ImageFormat::Png];
        config.dx_mode = dx_mode;
        // 主窗口直接指向图像层,避免 W0 组离屏对 alpha 再做一次合成
        // (DIV-GFX-008/041 的已知契约差异);本测试只验证转场模型。
        config.main_layer_set = yu_gfx::LayerId::new(b":/W0/A".to_vec());
        let mut gfx = GfxState::new(table.clone(), config);
        gfx.consume_events(&[
            VmEvent::FrameStart { frame: 1 },
            call(
                1,
                "CG",
                vec![
                    arg(&table, "CG", "ID", Value::Str(b"A".to_vec())),
                    arg(&table, "CG", "FILE", Value::Str(b"from".to_vec())),
                    arg(&table, "CG", "Z", Value::Int(10)),
                    arg(&table, "CG", "T", Value::Int(128)),
                    arg(&table, "CG", "TA", Value::Int(256)),
                    arg(&table, "CG", "TID", Value::Str(b"B".to_vec())),
                ],
            ),
            call(
                1,
                "CG",
                vec![
                    arg(&table, "CG", "ID", Value::Str(b"B".to_vec())),
                    arg(&table, "CG", "FILE", Value::Str(b"mask".to_vec())),
                    arg(&table, "CG", "E", Value::Int(0)),
                ],
            ),
            VmEvent::FrameEnd { frame: 1 },
        ]);
        gfx.frame()
    };
    let assets = [
        (
            b"from.png".as_slice(),
            png_bytes_for(4, 4, Rgba([200, 100, 50, 255])),
        ),
        (
            b"mask.png".as_slice(),
            png_bytes_for(4, 4, Rgba([127, 0, 0, 255])),
        ),
    ];
    let Some(mut wgpu) = renderer_or_skip(store(&assets)) else {
        return;
    };
    let mut soft = SoftRenderer::new(store(&assets));

    let frame1 = build(1);
    let gpu1 = wgpu.render_frame(&frame1).expect("render dx1");
    let soft1 = soft.render_frame(&frame1);
    for (gpu_px, soft_px) in gpu1
        .surface
        .rgba
        .chunks_exact(4)
        .zip(soft1.surface.rgba.chunks_exact(4))
    {
        for i in 0..4 {
            assert!(
                gpu_px[i].abs_diff(soft_px[i]) <= 2,
                "dx1 soft/wgpu mismatch: {gpu_px:?} vs {soft_px:?}"
            );
        }
    }
    // psC 公式(TA=256,A=256,mask.r=127/255):
    // fade=128/(256/(256/256+128))/256 ≈ 0.25195;
    // coverage=(0.25195-0.5*127/255)*256 ≈ 0.751。
    // 源色 = from * 0.751 => [150,75,38]。
    let center = pixel(&gpu1, 2, 2);
    assert!(
        (145..=155).contains(&center[0])
            && (70..=80).contains(&center[1])
            && (33..=43).contains(&center[2]),
        "psC center {center:?} not in expected band"
    );

    let frame0 = build(0);
    let gpu0 = wgpu.render_frame(&frame0).expect("render dx0");
    // 固定管线 G 通道 = 0,fade<1 => 覆盖度 0,图层不可见。
    assert_eq!(pixel(&gpu0, 2, 2), [0, 0, 0, 255]);
}

/// MODE 3/5/6/7/9:soft 与 wgpu 的 blend 因子/两阶段归约逐像素一致。
/// 主窗口直接指向图像层,避免 W0 组离屏的二次 alpha 合成(DIV-GFX-008/041)。
#[test]
fn fixed_pipeline_modes_3_5_6_7_9_match_soft() {
    let expected: [(u32, [u8; 3]); 5] = [
        (3, [31, 24, 16]),
        (5, [209, 136, 114]),
        (6, [200, 100, 80]),
        (7, [40, 60, 50]),
        (9, [177, 113, 99]),
    ];
    let assets = [(
        b"src.png".as_slice(),
        png_bytes_for(2, 2, Rgba([200, 100, 50, 255])),
    )];
    let Some(mut wgpu) = renderer_or_skip(store(&assets)) else {
        return;
    };
    let mut soft = SoftRenderer::new(store(&assets));
    for (mode, rgb) in expected {
        let table = table();
        let mut config = GfxConfig::new(Viewport::new(2, 2));
        config.clear = Clear::explicit([40, 60, 80, 255]);
        config.image_probe_order = vec![ImageFormat::Png];
        config.main_layer_set = yu_gfx::LayerId::new(b":/W0/A".to_vec());
        let mut gfx = GfxState::new(table.clone(), config);
        gfx.consume_events(&[
            VmEvent::FrameStart { frame: 1 },
            call(
                1,
                "CG",
                vec![
                    arg(&table, "CG", "ID", Value::Str(b"A".to_vec())),
                    arg(&table, "CG", "FILE", Value::Str(b"src".to_vec())),
                    arg(&table, "CG", "A", Value::Int(256)),
                    arg(&table, "CG", "MODE", Value::Int(i64::from(mode))),
                ],
            ),
            VmEvent::FrameEnd { frame: 1 },
        ]);
        let frame = gfx.frame();
        let gpu = wgpu.render_frame(&frame).expect("render mode");
        let soft_out = soft.render_frame(&frame);
        for (gpu_px, soft_px) in gpu
            .surface
            .rgba
            .chunks_exact(4)
            .zip(soft_out.surface.rgba.chunks_exact(4))
        {
            for i in 0..4 {
                assert!(
                    gpu_px[i].abs_diff(soft_px[i]) <= 1,
                    "MODE {mode}: {gpu_px:?} vs {soft_px:?}"
                );
            }
        }
        let center = pixel(&gpu, 1, 1);
        assert_eq!(&center[..3], &rgb[..], "MODE {mode} center {center:?}");
    }
}

// ---- 引擎显示缩放 effect Pass2/Pass3 -------------------------------------

/// 确定性 RGB 图案(alpha 全 255,避免层 alpha 干扰滤镜对拍)。
fn pattern_png(width: u32, height: u32) -> Vec<u8> {
    let image = RgbaImage::from_fn(width, height, |x, y| {
        Rgba([
            ((x * 37 + y * 11) % 251) as u8,
            ((x * 13 + y * 47) % 241) as u8,
            ((x * 71 + y * 29) % 239) as u8,
            255,
        ])
    });
    let mut out = Vec::new();
    PngEncoder::new(&mut out)
        .write_image(image.as_raw(), width, height, ExtendedColorType::Rgba8)
        .expect("encode pattern");
    out
}

fn fullscreen_pattern_frame(
    table: &ParamTable,
    width: u32,
    height: u32,
) -> yu_gfx::CompositorFrame {
    let mut gfx = state(Viewport::new(width, height), [0, 0, 0, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(table, "CG", "ID", Value::Str(b"A".to_vec())),
                arg(table, "CG", "FILE", Value::Str(b"pat".to_vec())),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);
    gfx.frame()
}

fn ref_texel(src: &[u8], width: u32, height: u32, x: i64, y: i64) -> [f32; 4] {
    let x = x.clamp(0, i64::from(width) - 1) as u32;
    let y = y.clamp(0, i64::from(height) - 1) as u32;
    let i = ((y as usize) * (width as usize) + x as usize) * 4;
    [
        f32::from(src[i]) / 255.0,
        f32::from(src[i + 1]) / 255.0,
        f32::from(src[i + 2]) / 255.0,
        f32::from(src[i + 3]) / 255.0,
    ]
}

fn ref_mix(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    let mut out = [0.0; 4];
    for i in 0..4 {
        out[i] = a[i] * (1.0 - t) + b[i] * t;
    }
    out
}

fn ref_encode(color: [f32; 4]) -> [u8; 4] {
    let mut out = [0u8; 4];
    for i in 0..4 {
        out[i] = (color[i].clamp(0.0, 1.0) * 255.0).round() as u8;
    }
    out
}

/// psA 的三次权重(与 ps_2_0 常量 c2..c5 同式)。
fn cubic_weights(f: f32) -> [f32; 4] {
    let f2 = f * f;
    let f3 = f2 * f;
    [
        -f3 + 2.0 * f2 - f,
        f3 - 2.0 * f2 + 1.0,
        -f3 + f2 + f,
        f3 - f2,
    ]
}

/// 与 WGSL 同式的 CPU 参考:输出像素中心减半 texel → point tap → u8。
fn reference_scale(
    src: &[u8],
    width: u32,
    height: u32,
    output_width: u32,
    output_height: u32,
    filter: ScaleFilter,
) -> Vec<u8> {
    let mut out = vec![0u8; (output_width as usize) * (output_height as usize) * 4];
    for y in 0..output_height {
        for x in 0..output_width {
            let qx = (f64::from(x) + 0.5) * f64::from(width) / f64::from(output_width) - 0.5;
            let qy = (f64::from(y) + 0.5) * f64::from(height) / f64::from(output_height) - 0.5;
            let fx = (qx - qx.floor()) as f32;
            let fy = (qy - qy.floor()) as f32;
            let ix = qx.floor() as i64;
            let iy = qy.floor() as i64;
            let color = match filter {
                ScaleFilter::Sharp => {
                    let wx = ((fx - 0.5) * 1.25 + 0.5).clamp(0.0, 1.0);
                    let wy = ((fy - 0.5) * 1.25 + 0.5).clamp(0.0, 1.0);
                    let top = ref_mix(
                        ref_texel(src, width, height, ix, iy),
                        ref_texel(src, width, height, ix + 1, iy),
                        wx,
                    );
                    let bottom = ref_mix(
                        ref_texel(src, width, height, ix, iy + 1),
                        ref_texel(src, width, height, ix + 1, iy + 1),
                        wx,
                    );
                    ref_mix(top, bottom, wy)
                }
                ScaleFilter::Bicubic => {
                    let wx = cubic_weights(fx);
                    let wy = cubic_weights(fy);
                    let mut sum = [0.0f32; 4];
                    for (r, wy_r) in wy.iter().enumerate() {
                        for (c, wx_c) in wx.iter().enumerate() {
                            let tap =
                                ref_texel(src, width, height, ix - 1 + c as i64, iy - 1 + r as i64);
                            let weight = wx_c * wy_r;
                            for ch in 0..4 {
                                sum[ch] += tap[ch] * weight;
                            }
                        }
                    }
                    sum
                }
            };
            let encoded = ref_encode(color);
            let i = ((y as usize) * (output_width as usize) + x as usize) * 4;
            out[i..i + 4].copy_from_slice(&encoded);
        }
    }
    out
}

fn assert_scale_matches_reference(filter: ScaleFilter, output_width: u32, output_height: u32) {
    let table = table();
    let (width, height) = (8u32, 6u32);
    let frame = fullscreen_pattern_frame(&table, width, height);
    let assets = [(b"pat.png".as_slice(), pattern_png(width, height))];
    let Some(mut renderer) = renderer_or_skip(store(&assets)) else {
        return;
    };
    // 屏幕纹理 = 1:1 合成结果,作为滤镜参考的输入。
    let screen = renderer.render_frame(&frame).expect("screen render");
    let expected = reference_scale(
        &screen.surface.rgba,
        width,
        height,
        output_width,
        output_height,
        filter,
    );
    let out = renderer
        .render_frame_scaled(&frame, Viewport::new(output_width, output_height), filter)
        .expect("scaled render");
    assert_eq!(
        (out.surface.width, out.surface.height),
        (output_width, output_height)
    );
    assert_eq!(out.surface.rgba.len(), expected.len());
    for (index, (actual, want)) in out
        .surface
        .rgba
        .chunks_exact(4)
        .zip(expected.chunks_exact(4))
        .enumerate()
    {
        for channel in 0..4 {
            assert!(
                actual[channel].abs_diff(want[channel]) <= 1,
                "{filter:?} {output_width}x{output_height} pixel {index} \
                 actual={actual:?} expected={want:?}"
            );
        }
    }
}

#[test]
fn display_scale_1to1_reproduces_composited_screen() {
    // 8×6(2 的幂)与 7×5(非 2 的幂;覆盖 base*W 的 f32 舍入)。
    for (width, height) in [(8u32, 6u32), (7, 5)] {
        let table = table();
        let frame = fullscreen_pattern_frame(&table, width, height);
        let assets = [(b"pat.png".as_slice(), pattern_png(width, height))];
        let Some(mut renderer) = renderer_or_skip(store(&assets)) else {
            return;
        };
        let base = renderer.render_frame(&frame).expect("base render");
        assert_eq!(base.stats.draw_calls, 1);
        for filter in [ScaleFilter::Sharp, ScaleFilter::Bicubic] {
            let scaled = renderer
                .render_frame_scaled(&frame, Viewport::new(width, height), filter)
                .expect("scaled render");
            for (actual, want) in scaled
                .surface
                .rgba
                .chunks_exact(4)
                .zip(base.surface.rgba.chunks_exact(4))
            {
                for channel in 0..4 {
                    assert!(
                        actual[channel].abs_diff(want[channel]) <= 1,
                        "{width}x{height} {filter:?} 1:1 actual={actual:?} expected={want:?}"
                    );
                }
            }
            // 合成 1 次 draw + 缩放 1 次内部 draw。
            assert_eq!(scaled.stats.draw_calls, 2, "{filter:?}");
        }
    }
}

#[test]
fn display_scale_psb_matches_reference_upscale_and_downscale() {
    assert_scale_matches_reference(ScaleFilter::Sharp, 16, 12);
    assert_scale_matches_reference(ScaleFilter::Sharp, 5, 4);
}

#[test]
fn display_scale_psa_matches_reference_upscale_and_downscale() {
    assert_scale_matches_reference(ScaleFilter::Bicubic, 16, 12);
    assert_scale_matches_reference(ScaleFilter::Bicubic, 5, 4);
}

/// `DIV-GFX-052`:同层先 `RECTPAINT3` 再 `REVERSE`,两端必须逐像素一致。
///
/// 证据:脚本对 `SPLTRBUF` 先涂色再反色(`es_Sprite.yst:4233/6106` 的
/// `CGACT[REVERSE=1]`);引擎按命令顺序立即作用在同一纹理上(EVID-T-15)。
#[test]
fn reverse_after_rectpaint_matches_soft() {
    let table = table();
    let mut gfx = state(Viewport::new(2, 2), [0, 0, 0, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"T".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"".to_vec())),
                arg(&table, "CG", "SX", Value::Int(2)),
                arg(&table, "CG", "SY", Value::Int(2)),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", Value::Str(b"T".to_vec())),
                arg(&table, "CGACT", "RECTPAINT3", Value::Int(1)),
                arg(&table, "CGACT", "SET", Value::Int(0xFF102030)),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", Value::Str(b"T".to_vec())),
                arg(&table, "CGACT", "REVERSE", Value::Int(1)),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);
    let frame = gfx.frame();

    let Some(mut gpu) = renderer_or_skip(store(&[])) else {
        return;
    };
    let gpu_out = gpu.render_frame(&frame).expect("render");
    let mut soft = SoftRenderer::new(store(&[]));
    let soft_out = soft.render_frame(&frame);

    assert_eq!(gpu_out.report.ops_total, soft_out.report.ops_total);
    assert_eq!(gpu_out.report.ops_drawn, soft_out.report.ops_drawn);
    assert_eq!(gpu_out.report.skipped, soft_out.report.skipped);
    let soft_surface = soft_out_surface(&soft_out);
    assert_eq!(pixel(&gpu_out, 0, 0), pixel_of_surface(&soft_surface, 0, 0));
    // 0x10,0x20,0x30 反色 = 0xEF,0xDF,0xCF;alpha 保持 0xFF。
    assert_eq!(pixel(&gpu_out, 0, 0), [0xEF, 0xDF, 0xCF, 0xFF]);
}

fn soft_out_surface(out: &yu_backend_soft::RenderOutput) -> yu_backend_soft::Surface {
    (*out.surface).clone()
}

fn pixel_of_surface(surface: &yu_backend_soft::Surface, x: u32, y: u32) -> [u8; 4] {
    surface.pixel(x, y).expect("pixel inside surface")
}

#[test]
fn text_layout_frames_match_soft_exactly() {
    let Some(mut gpu) = renderer_or_skip(store(&[])) else {
        return;
    };
    let mut soft = SoftRenderer::new(store(&[]));
    if !soft.text_composer().available() {
        SKIPPED.fetch_add(1, Ordering::Relaxed);
        println!("SKIP text_layout_frames_match_soft_exactly: no system font");
        return;
    }
    let mut gfx = state(Viewport::new(64, 32), [0, 0, 0, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        VmEvent::Text {
            frame: 1,
            id: String::new(),
            bytes: b"A\x82\xB1B".to_vec(),
        },
    ]);
    // 权威 `_TEXT_FLAG`:半角/全角/半角(SX=SY=20 默认)。
    gfx.sync_text_state(&yu_rt::text::TextSnapshot {
        bytes: b"A\x82\xB1B".to_vec(),
        flags: b"121".to_vec(),
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
    });
    let frame = gfx.frame();
    assert_eq!(frame.text_layout.as_ref().map_or(0, |l| l.glyphs.len()), 3);

    let soft_out = soft.render_frame(&frame);
    let gpu_out = gpu.render_frame(&frame).expect("render");
    assert_eq!(soft_out.report.text_glyphs, 3);
    assert_eq!(gpu_out.report.text_glyphs, 3);
    assert_eq!(soft_out.report.text_draws, gpu_out.report.text_draws);
    assert_eq!(soft_out.report.text_diags, gpu_out.report.text_diags);
    assert_eq!(
        soft_out.surface.rgba, gpu_out.surface.rgba,
        "text frame must be pixel-identical across backends"
    );
}

#[test]
fn cgact_text_layer_frames_match_soft_exactly() {
    let table = table();
    let Some(mut gpu) = renderer_or_skip(store(&[(
        b"dark.png",
        png_bytes_for(32, 32, Rgba([0, 0, 0, 255])),
    )])) else {
        return;
    };
    let mut soft = SoftRenderer::new(store(&[(
        b"dark.png",
        png_bytes_for(32, 32, Rgba([0, 0, 0, 255])),
    )]));
    if !soft.text_composer().available() {
        SKIPPED.fetch_add(1, Ordering::Relaxed);
        println!("SKIP cgact_text_layer_frames_match_soft_exactly: no system font");
        return;
    }
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

    let soft_out = soft.render_frame(&frame);
    let gpu_out = gpu.render_frame(&frame).expect("render");
    assert_eq!(soft_out.report.text_draws, 1);
    assert_eq!(gpu_out.report.text_draws, 1);
    assert_eq!(soft_out.report.text_glyphs, 1);
    assert_eq!(gpu_out.report.text_glyphs, 1);
    assert_eq!(soft_out.report.text_diags, gpu_out.report.text_diags);
    assert_eq!(
        soft_out.surface.rgba, gpu_out.surface.rgba,
        "CGACT[TEXT] layer frame must be pixel-identical across backends"
    );
}
