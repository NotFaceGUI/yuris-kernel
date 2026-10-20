//! wgpu 后端端到端:合成命令表 → GfxState → CompositorFrame → GPU → RGBA8。
//!
//! 与软后端测试分开:这里验证 GPU 路径、readback 行对齐、纹理缓存与
//! “计数与软后端一致”。夹具不依赖真实游戏数据。

use std::sync::atomic::{AtomicUsize, Ordering};

use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder, Rgba, RgbaImage};
use yu_backend_soft::{SoftRenderer, TextureStore};
use yu_backend_wgpu::device::code;
use yu_backend_wgpu::{DeviceOptions, WgpuRenderOutput, WgpuRenderer};
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
            command("CGACT", &["ID", "ID2", "COPY2"]),
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
    // group op + asset layer op。
    assert_eq!(out.report.ops_drawn, 2);
    assert_eq!(out.stats.draw_calls, 2);
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
    // 同一资产被两层引用:只上传一次;draw = group + 2 layers。
    assert_eq!(first.stats.texture_uploads, 1);
    assert_eq!(first.stats.upload_bytes, 2 * 2 * 4);
    assert_eq!(first.report.ops_drawn, 3);
    assert_eq!(first.stats.draw_calls, 3);

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
                arg(&table, "CG", "MODE", Value::Int(3)),
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
    // gfx 每帧都会把主组 W0 再包一层离屏。D3D9 同因子 alpha(源项 a²):
    // 子层在 G 的透明离屏里先成为 (255,0,0,255);G 以 A=128 合成进透明的
    // W0 离屏,得到 (128,0,0,64);W0 再以 A=256 合成到白屏:
    //   R = 128/255·64/255 + 255/255·(1−64/255) ≈ 223
    //   G/B = 255/255·(1−64/255) = 191
    //   A = (64/255)² + 1·(1−64/255) ≈ 207
    // 组 A 只在“组 → 父目标”合成时作用一次,没有下推到子层。
    assert_close(pixel(&out, 0, 0), [223, 191, 191, 207], 2, "group alpha");
    assert_close(pixel(&out, 3, 3), [223, 191, 191, 207], 2, "group alpha");
}
