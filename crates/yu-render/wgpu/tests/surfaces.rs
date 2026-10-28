//! 图层常驻表面(W1/W2/W3)的行为门禁。
//!
//! 这些用例不依赖具体像素容差之外的实现细节:只验证「同一操作只执行一次」
//! 「表面跨帧复用」「快照别名/区域拷贝」「采样修饰不物化」等契约。

use std::sync::atomic::{AtomicUsize, Ordering};

use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder, Rgba, RgbaImage};
use yu_backend_soft::{SoftRenderer, TextureStore};
use yu_backend_wgpu::device::code;
use yu_backend_wgpu::WgpuRenderer;
use yu_format::yscm::{Arg as FmtArg, Command as FmtCommand, CommandTable};
use yu_gfx::{Clear, CompositorFrame, GfxConfig, GfxState, ImageFormat, ParamTable, Viewport};
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
                    "ID3",
                    "TRANSITION",
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
                    "VERTICAL",
                    "TEXT",
                    "SETSTR",
                    "BNO",
                    "MERGE",
                    "MERGE2",
                    "SIZE",
                    "SIZE2",
                    "SX",
                    "SY",
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

fn pixel_of(surface: &yu_backend_soft::Surface, x: u32, y: u32) -> [u8; 4] {
    let i = ((y as usize) * (surface.width as usize) + x as usize) * 4;
    let rgba = &surface.rgba;
    [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
}

fn max_channel_diff(a: &yu_backend_soft::Surface, b: &yu_backend_soft::Surface) -> u8 {
    a.rgba
        .chunks_exact(4)
        .zip(b.rgba.chunks_exact(4))
        .map(|(x, y)| (0..4).map(|i| x[i].abs_diff(y[i])).max().unwrap())
        .max()
        .unwrap_or(0)
}

/// 同一帧重复渲染:`CGACT` 只作用一次 —— 第二帧零表面写入、零上传。
#[test]
fn repeated_frame_reuses_resident_surface() {
    let table = table();
    let Some(mut gpu) = renderer_or_skip(store(&[(
        b"base.png",
        png_bytes_for(8, 8, Rgba([20, 40, 60, 255])),
    )])) else {
        return;
    };
    let mut gfx = state(Viewport::new(8, 8), [0, 0, 0, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"L".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"base".to_vec())),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", Value::Str(b"L".to_vec())),
                arg(&table, "CGACT", "RECTPAINT3", Value::Int(1)),
                arg(&table, "CGACT", "SET", Value::Int(0xFF_3366_99u32 as i64)),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);
    let frame = gfx.frame();
    let first = gpu.render_frame(&frame).expect("first");
    // 第一帧:一次填充 + 一次屏幕 blit。
    assert_eq!(gpu.render_phases().surface_passes, 1, "第一帧应执行一次填充");
    let first_surface = first.surface.clone();

    // 同一 frame 再渲染一次:操作前缀相同 ⇒ 表面写入 0、上传 0。
    let second = gpu.render_frame(&frame).expect("second");
    assert_eq!(
        second.stats.texture_uploads, 0,
        "常驻表面复用不应再上传纹理"
    );
    let phases = gpu.render_phases();
    assert!(
        phases.reused_layers >= 1,
        "同前缀操作必须复用(实际 reused_layers={})",
        phases.reused_layers
    );
    assert_eq!(second.surface.rgba, first_surface.rgba, "复用的表面像素不变");
}

/// `SIZE`(无区域)是纯采样修饰:不产生表面写入,像素随层声明尺寸拉伸。
#[test]
fn size_without_region_is_sampling_only() {
    let table = table();
    let entries = [(b"p.png".as_slice(), png_bytes_for(2, 2, Rgba([200, 100, 50, 255])))];
    let Some(mut gpu) = renderer_or_skip(store(&entries)) else {
        return;
    };
    let mut soft = SoftRenderer::new(store(&entries));
    let mut gfx = state(Viewport::new(8, 8), [0, 0, 0, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"S".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"p".to_vec())),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", Value::Str(b"S".to_vec())),
                arg(&table, "CGACT", "ID2", Value::Str(b"D".to_vec())),
                arg(&table, "CGACT", "SIZE2", Value::Int(1)),
                arg(&table, "CGACT", "SX", Value::Int(8)),
                arg(&table, "CGACT", "SY", Value::Int(8)),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);
    let frame = gfx.frame();
    let gpu_out = gpu.render_frame(&frame).expect("render");
    let soft_out = soft.render_frame(&frame);
    let phases = gpu.render_phases();
    assert_eq!(phases.surface_passes, 0, "SIZE 不应产生表面写入");
    assert_eq!(phases.materialized, 0, "SIZE 不应物化采样窗");
    assert_eq!(
        max_channel_diff(&gpu_out.surface, &soft_out.surface),
        0,
        "SIZE 的拉伸结果必须与 soft 一致"
    );
}

/// 整幅 COPY 走别名(零拷贝);区域 COPY 走真实区域复制。
#[test]
fn full_copy_aliases_and_region_copy_blits() {
    let table = table();
    let entries = [(b"a.png".as_slice(), png_bytes_for(4, 4, Rgba([10, 20, 30, 255])))];
    let Some(mut gpu) = renderer_or_skip(store(&entries)) else {
        return;
    };
    let mut gfx = state(Viewport::new(4, 4), [0, 0, 0, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"A".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"a".to_vec())),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", Value::Str(b"A".to_vec())),
                arg(&table, "CGACT", "ID2", Value::Str(b"SNAP".to_vec())),
                arg(&table, "CGACT", "COPY2", Value::Int(1)),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);
    let frame = gfx.frame();
    let out = gpu.render_frame(&frame).expect("render");
    let phases = gpu.render_phases();
    assert_eq!(phases.alias_hits, 1, "整幅 COPY 必须别名(零拷贝)");
    assert_eq!(phases.blits, 0, "别名路径不应有纹理复制");
    assert_eq!(pixel_of(&out.surface, 0, 0), [10, 20, 30, 255]);

    // 区域 COPY:真实区域复制(blits >= 1),像素与 soft 一致。
    // 新的 `GfxState` = 新的 `CopyId` 序列 ⇒ 渲染器也要重建(引擎 restart
    // 走 `WgpuRenderer::reset`,用例里直接用新渲染器)。
    let Some(mut gpu) = renderer_or_skip(store(&entries)) else {
        return;
    };
    let mut gfx = state(Viewport::new(4, 4), [0, 0, 0, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"A".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"a".to_vec())),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", Value::Str(b"A".to_vec())),
                arg(&table, "CGACT", "ID2", Value::Str(b"CROP".to_vec())),
                arg(&table, "CGACT", "COPY2", Value::Int(1)),
                arg(&table, "CGACT", "X", Value::Int(1)),
                arg(&table, "CGACT", "Y", Value::Int(1)),
                arg(&table, "CGACT", "X2", Value::Int(2)),
                arg(&table, "CGACT", "Y2", Value::Int(2)),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);
    let frame = gfx.frame();
    let out = gpu.render_frame(&frame).expect("render");
    let phases = gpu.render_phases();
    assert!(phases.blits >= 1, "区域 COPY 必须做区域复制");
    let mut soft = SoftRenderer::new(store(&entries));
    let soft_out = soft.render_frame(&frame);
    assert_eq!(
        max_channel_diff(&out.surface, &soft_out.surface),
        0,
        "区域 COPY 的落点必须与 soft 一致"
    );
}

/// `MERGE2` 在 GPU 上执行并与 soft 逐像素一致(alpha 参与混合)。
#[test]
fn merge2_matches_soft() {
    let table = table();
    let entries = [
        (b"low.png".as_slice(), png_bytes_for(8, 8, Rgba([40, 120, 200, 128]))),
        (b"up.png".as_slice(), png_bytes_for(8, 8, Rgba([220, 30, 20, 160]))),
    ];
    let Some(mut gpu) = renderer_or_skip(store(&entries)) else {
        return;
    };
    let mut soft = SoftRenderer::new(store(&entries));
    let mut gfx = state(Viewport::new(8, 8), [0, 0, 0, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"LOW".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"low".to_vec())),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"UP".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"up".to_vec())),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", Value::Str(b"UP".to_vec())),
                arg(&table, "CGACT", "ID2", Value::Str(b"LOW".to_vec())),
                arg(&table, "CGACT", "MERGE2", Value::Int(1)),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);
    let frame = gfx.frame();
    let out = gpu.render_frame(&frame).expect("render");
    assert!(
        gpu.render_phases().surface_passes >= 1,
        "MERGE2 必须走 GPU 表面写入"
    );
    let soft_out = soft.render_frame(&frame);
    assert_eq!(
        max_channel_diff(&out.surface, &soft_out.surface),
        0,
        "MERGE2 的 alpha 公式必须与 soft 逐字节一致"
    );
}

/// `REVERSE`/`HORIZONTAL`/`VERTICAL` 三个原地变换与 soft 逐像素一致。
#[test]
fn in_place_transforms_match_soft() {
    let table = table();
    let entries = [(
        b"img.png".as_slice(),
        png_bytes_for(4, 4, Rgba([32, 64, 96, 200])),
    )];
    for (name, param) in [
        ("REVERSE", "REVERSE"),
        ("HORIZONTAL", "HORIZONTAL"),
        ("VERTICAL", "VERTICAL"),
    ] {
        let Some(mut gpu) = renderer_or_skip(store(&entries)) else {
            return;
        };
        let mut soft = SoftRenderer::new(store(&entries));
        let mut gfx = state(Viewport::new(4, 4), [0, 0, 0, 255]);
        gfx.consume_events(&[
            VmEvent::FrameStart { frame: 1 },
            call(
                1,
                "CG",
                vec![
                    arg(&table, "CG", "ID", Value::Str(b"I".to_vec())),
                    arg(&table, "CG", "FILE", Value::Str(b"img".to_vec())),
                ],
            ),
            call(
                1,
                "CGACT",
                vec![
                    arg(&table, "CGACT", "ID", Value::Str(b"I".to_vec())),
                    arg(&table, "CGACT", param, Value::Int(1)),
                ],
            ),
            VmEvent::FrameEnd { frame: 1 },
        ]);
        let frame = gfx.frame();
        let out = gpu.render_frame(&frame).expect("render");
        let soft_out = soft.render_frame(&frame);
        assert_eq!(
            max_channel_diff(&out.surface, &soft_out.surface),
            0,
            "{name} 必须与 soft 逐字节一致"
        );
    }
}

/// 表面表随图层删除剪枝:退出时不留死层表面。
#[test]
fn surfaces_are_pruned_when_layers_die() {
    let table = table();
    let entries = [(b"a.png".as_slice(), png_bytes_for(4, 4, Rgba([1, 2, 3, 255])))];
    let Some(mut gpu) = renderer_or_skip(store(&entries)) else {
        return;
    };
    let mut gfx = state(Viewport::new(4, 4), [0, 0, 0, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"A".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"a".to_vec())),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", Value::Str(b"A".to_vec())),
                arg(&table, "CGACT", "RECTPAINT3", Value::Int(1)),
                arg(&table, "CGACT", "SET", Value::Int(0xFF0000)),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);
    let frame = gfx.frame();
    let _ = gpu.render_frame(&frame).expect("render");
    assert!(gpu.surface_stats().live_surfaces >= 1);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 2 },
        call(
            2,
            "CGEND",
            vec![arg(&table, "CG", "ID", Value::Str(b"A".to_vec()))],
        ),
        VmEvent::FrameEnd { frame: 2 },
    ]);
    let frame = gfx.frame();
    let _ = gpu.render_frame(&frame).expect("render");
    assert_eq!(
        gpu.surface_stats().live_surfaces,
        0,
        "图层删除后表面必须剪枝"
    );
}

/// 确定性:同一帧序列在两个渲染器上逐帧输出一致(前缀缓存不改变像素)。
#[test]
fn resident_surface_sequence_is_deterministic() {
    let table = table();
    let entries = [(b"a.png".as_slice(), png_bytes_for(8, 8, Rgba([90, 40, 10, 200])))];
    let mut gfx = state(Viewport::new(8, 8), [0, 0, 0, 255]);
    let mut frames = Vec::new();
    for frame in 1..=6u64 {
        gfx.consume_events(&[
            VmEvent::FrameStart { frame },
            call(
                frame,
                "CG",
                vec![
                    arg(&table, "CG", "ID", Value::Str(b"A".to_vec())),
                    arg(&table, "CG", "FILE", Value::Str(b"a".to_vec())),
                ],
            ),
            call(
                frame,
                "CG",
                vec![
                    arg(&table, "CG", "ID", Value::Str(b"A".to_vec())),
                    arg(&table, "CG", "X", Value::Int(frame as i64)),
                ],
            ),
            // 每帧追加一条新的填充:链增长,表面必须按命令序推进。
            call(
                frame,
                "CGACT",
                vec![
                    arg(&table, "CGACT", "ID", Value::Str(b"A".to_vec())),
                    arg(&table, "CGACT", "RECTPAINT3", Value::Int(1)),
                    arg(&table, "CGACT", "SET", Value::Int(0xFF000000 | frame as i64)),
                    arg(&table, "CGACT", "X", Value::Int(0)),
                    arg(&table, "CGACT", "Y", Value::Int(0)),
                    arg(&table, "CGACT", "X2", Value::Int(0)),
                    arg(&table, "CGACT", "Y2", Value::Int(0)),
                ],
            ),
            VmEvent::FrameEnd { frame },
        ]);
        frames.push(gfx.frame());
    }
    let Some(mut a) = renderer_or_skip(store(&entries)) else {
        return;
    };
    let Some(mut b) = renderer_or_skip(store(&entries)) else {
        return;
    };
    for frame in &frames {
        let out_a = a.render_frame(frame).expect("render a");
        let out_b = b.render_frame(frame).expect("render b");
        assert_eq!(out_a.surface.rgba, out_b.surface.rgba, "同序列必须逐帧一致");
    }
}

/// 文本层跨帧稳定:同一帧重复渲染不得重复叠加字形(前缀复用 + 文本缓存),
/// 内容变化时只重烤一次。
#[test]
fn text_layer_is_stable_across_frames() {
    let table = table();
    let Some(mut gpu) = renderer_or_skip(store(&[(
        b"dark.png",
        png_bytes_for(32, 16, Rgba([0, 0, 0, 255])),
    )])) else {
        return;
    };
    let mut soft = SoftRenderer::new(store(&[(
        b"dark.png",
        png_bytes_for(32, 16, Rgba([0, 0, 0, 255])),
    )]));
    if !soft.text_composer().available() {
        SKIPPED.fetch_add(1, Ordering::Relaxed);
        println!("SKIP text_layer_is_stable_across_frames: no system font");
        return;
    }
    let mut gfx = state(Viewport::new(32, 16), [0, 0, 0, 255]);
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
    let first = gpu.render_frame(&frame).expect("first");
    let soft_first = soft.render_frame(&frame);
    assert_eq!(
        max_channel_diff(&first.surface, &soft_first.surface),
        0,
        "文本层首帧必须与 soft 一致"
    );
    // 同帧再渲染:文本不得二次叠加。
    let second = gpu.render_frame(&frame).expect("second");
    assert_eq!(
        second.surface.rgba, first.surface.rgba,
        "重复渲染不得重复叠字(前缀 + 文本缓存)"
    );
    // 文本内容变化(新绘制项):只重烤一次,画面跟 soft 一致。
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 2 },
        call(
            2,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", Value::Str(b"TX".to_vec())),
                arg(&table, "CGACT", "TEXT", Value::Int(1)),
                arg(&table, "CGACT", "SET", Value::Int(0)),
                arg(&table, "CGACT", "SETSTR", Value::Str(b"B".to_vec())),
                arg(&table, "CGACT", "X", Value::Int(0)),
                arg(&table, "CGACT", "Y", Value::Int(0)),
            ],
        ),
        VmEvent::FrameEnd { frame: 2 },
    ]);
    let frame2 = gfx.frame();
    let third = gpu.render_frame(&frame2).expect("third");
    let soft_third = soft.render_frame(&frame2);
    assert_eq!(
        max_channel_diff(&third.surface, &soft_third.surface),
        0,
        "文本内容变化后必须与 soft 一致(不得残留旧字形)"
    );
}

/// 60 帧多场景序列:换差分(COPY)/过渡(TRANSITION)/MERGE/缩放/文本,
/// wgpu 与 soft **逐帧**逐像素一致;同时验证前缀复用不改变像素。
///
/// 脚本形态对齐 `es_SPExec.yst`:每帧对中间层重发 `CG[FILE]`(引擎重建
/// 层表面、清空该层的加工历史),再走 `COPY → MERGE`;目标层也每帧重建。
#[test]
fn sixty_frame_scene_matches_soft() {
    let table = table();
    let entries = [
        (b"bg.png".as_slice(), png_bytes_for(16, 16, Rgba([10, 60, 120, 255]))),
        (b"a.png".as_slice(), png_bytes_for(8, 8, Rgba([220, 40, 30, 200]))),
        (b"b.png".as_slice(), png_bytes_for(8, 8, Rgba([30, 200, 80, 160]))),
    ];
    let Some(mut gpu) = renderer_or_skip(store(&entries)) else {
        return;
    };
    let mut soft = SoftRenderer::new(store(&entries));
    let mut gfx = state(Viewport::new(16, 16), [0, 0, 0, 255]);
    for frame in 1..=60u64 {
        let cg = |id: &[u8], file: &[u8]| {
            call(
                frame,
                "CG",
                vec![
                    arg(&table, "CG", "ID", Value::Str(id.to_vec())),
                    arg(&table, "CG", "FILE", Value::Str(file.to_vec())),
                ],
            )
        };
        let mut events = vec![VmEvent::FrameStart { frame }];
        // 每帧重建目标/中间层(清空该层的加工历史,与语料同型)。
        events.push(cg(b"BG", b"bg"));
        events.push(cg(b"FRAME", b""));
        events.push(cg(b"MIX", b""));
        events.push(cg(b"SCALED", b""));
        events.push(cg(b"FROM", b"a"));
        events.push(cg(b"TO", b"b"));
        // 差分层:两张图交替 COPY 成快照(眨眼/换表情同型)。
        let diff = if frame % 2 == 0 { b"a" } else { b"b" };
        events.push(cg(b"SRC", diff));
        events.push(call(
            frame,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", Value::Str(b"SRC".to_vec())),
                arg(&table, "CGACT", "ID2", Value::Str(b"FRAME".to_vec())),
                arg(&table, "CGACT", "COPY2", Value::Int(1)),
            ],
        ));
        events.push(call(
            frame,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", Value::Str(b"FRAME".to_vec())),
                arg(&table, "CGACT", "ID2", Value::Str(b"BG".to_vec())),
                arg(&table, "CGACT", "MERGE", Value::Int(1)),
            ],
        ));
        // 过渡层:两源按 SET 逐步推进(动画引擎同型)。
        events.push(call(
            frame,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", Value::Str(b"FROM".to_vec())),
                arg(&table, "CGACT", "ID2", Value::Str(b"TO".to_vec())),
                arg(&table, "CGACT", "ID3", Value::Str(b"MIX".to_vec())),
                arg(&table, "CGACT", "TRANSITION", Value::Int(1)),
                arg(&table, "CGACT", "SET", Value::Int((frame * 4) as i64)),
            ],
        ));
        // 缩放(无区域:纯采样)。
        events.push(call(
            frame,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", Value::Str(b"MIX".to_vec())),
                arg(&table, "CGACT", "ID2", Value::Str(b"SCALED".to_vec())),
                arg(&table, "CGACT", "SIZE2", Value::Int(1)),
                arg(&table, "CGACT", "SX", Value::Int(8)),
                arg(&table, "CGACT", "SY", Value::Int(8)),
            ],
        ));
        events.push(VmEvent::FrameEnd { frame });
        gfx.consume_events(&events);
        let frame_data: CompositorFrame = gfx.frame();
        let gpu_out = gpu.render_frame(&frame_data).expect("gpu render");
        let soft_out = soft.render_frame(&frame_data);
        let diff = max_channel_diff(&gpu_out.surface, &soft_out.surface);
        assert_eq!(diff, 0, "frame {frame}: wgpu 与 soft 必须逐像素一致");
        assert_eq!(
            gpu_out.report.ops_total, soft_out.report.ops_total,
            "frame {frame}: ops_total 必须一致"
        );
        assert_eq!(
            gpu_out.report.ops_drawn, soft_out.report.ops_drawn,
            "frame {frame}: ops_drawn 必须一致"
        );
        assert_eq!(
            gpu_out.report.skipped, soft_out.report.skipped,
            "frame {frame}: 跳过原因必须一致"
        );
    }
}

/// 采样修饰后的 COPY:源层先 `SIZE2`(带区域)再整幅 `COPY2` —— 复制必须
/// 物化后按**逻辑网格**拷贝,不得把源纹理按放大后的尺寸越界读。
#[test]
fn copy_from_scaled_layer_materializes_before_blit() {
    let table = table();
    let entries = [(
        b"img.png".as_slice(),
        png_bytes_for(4, 4, Rgba([200, 60, 20, 255])),
    )];
    let Some(mut gpu) = renderer_or_skip(store(&entries)) else {
        return;
    };
    let mut soft = SoftRenderer::new(store(&entries));
    let mut gfx = state(Viewport::new(16, 16), [0, 0, 0, 255]);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"S".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"img".to_vec())),
            ],
        ),
        // 源区域 (1,1)-(2,2) 放大到 8×8:逻辑网格 8×8,纹理只有 4×4。
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", Value::Str(b"S".to_vec())),
                arg(&table, "CGACT", "ID2", Value::Str(b"BIG".to_vec())),
                arg(&table, "CGACT", "SIZE2", Value::Int(1)),
                arg(&table, "CGACT", "SX", Value::Int(8)),
                arg(&table, "CGACT", "SY", Value::Int(8)),
                arg(&table, "CGACT", "X", Value::Int(1)),
                arg(&table, "CGACT", "Y", Value::Int(1)),
                arg(&table, "CGACT", "X2", Value::Int(2)),
                arg(&table, "CGACT", "Y2", Value::Int(2)),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", Value::Str(b"BIG".to_vec())),
                arg(&table, "CGACT", "ID2", Value::Str(b"SNAP".to_vec())),
                arg(&table, "CGACT", "COPY2", Value::Int(1)),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);
    let frame = gfx.frame();
    let out = gpu.render_frame(&frame).expect("render");
    let soft_out = soft.render_frame(&frame);
    assert_eq!(
        out.report.skipped, soft_out.report.skipped,
        "采样源 COPY 不得产生新的跳过项"
    );
    assert_eq!(
        max_channel_diff(&out.surface, &soft_out.surface),
        0,
        "采样源 COPY 的落点必须与 soft 一致"
    );
}
