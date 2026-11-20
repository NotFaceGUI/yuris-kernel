//! 点击帧回归：桌面 streaming 路径无 CPU 读回时，`COPY2 ID=:/W0/`
//! 必须读到**上一呈现帧**（屏幕表面），不得退回清屏色。
//!
//! 复现链（<game-b> v494 s51 pc161..177）：
//! 1. 帧 1 呈现标题；
//! 2. 帧 2 `CG SSSSS E=0 FILE=""` → `CGACT COPY2 ID=:/W0/ ID2=SSSSS`
//!    （屏幕契约与上一帧相同 ⇒ `capture_screen_snapshot` 返回 `None`）；
//! 3. 桌面 wgpu 走 `render_frame_streaming`，没有 CPU 后缓冲。
//!
//! 期望：快照 = 上一帧标题像素。由 `WgpuRenderer::screen_surface`
//! （每次呈现前 GPU→GPU 拷贝）满足；本文件
//! 三项用例全部为绿（soft oracle + wgpu + 25 帧淡出）。

use std::sync::atomic::{AtomicUsize, Ordering};

use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder, Rgba};
use yu_backend_soft::{SoftRenderer, TextureStore};
use yu_backend_wgpu::device::code;
use yu_backend_wgpu::WgpuRenderer;
use yu_format::yscm::{Arg as FmtArg, Command as FmtCommand, CommandTable};
use yu_gfx::{Clear, GfxConfig, GfxState, ImageFormat, ParamTable, Viewport};
use yu_rt::event::{ArgValue, Domain, VmEvent};
use yu_rt::tables::YscmTable;
use yu_rt::value::Value;
use yu_vfs::{Codec, MemSource};

static SKIPPED: AtomicUsize = AtomicUsize::new(0);

const TITLE: [u8; 4] = [10, 200, 30, 255];

fn command(name: &str, params: &[&str]) -> FmtCommand {
    FmtCommand {
        name: name.into(),
        argc: params.len() as u8,
        args: params
            .iter()
            .map(|param| FmtArg {
                name: (*param).into(),
                flag: if matches!(*param, "ID" | "ID2" | "FILE" | "SETSTR") {
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
                &["ID", "FILE", "E", "A", "X", "Y", "Z", "SX", "SY"],
            ),
            command(
                "CGACT",
                &["ID", "ID2", "COPY2", "COPY", "SET", "X", "Y", "X2", "Y2"],
            ),
            command("CGEND", &["ID"]),
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
    let image = image::RgbaImage::from_pixel(width, height, color);
    let mut out = Vec::new();
    PngEncoder::new(&mut out)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            ExtendedColorType::Rgba8,
        )
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

fn max_channel_diff(a: &yu_backend_soft::Surface, b: &yu_backend_soft::Surface) -> u8 {
    a.rgba
        .chunks_exact(4)
        .zip(b.rgba.chunks_exact(4))
        .map(|(x, y)| (0..4).map(|i| x[i].abs_diff(y[i])).max().unwrap())
        .max()
        .unwrap_or(0)
}

fn frame_of(gfx: &mut GfxState, frame: u64, events: Vec<VmEvent>) -> yu_gfx::CompositorFrame {
    let mut all = vec![VmEvent::FrameStart { frame }];
    all.extend(events);
    all.push(VmEvent::FrameEnd { frame });
    gfx.consume_events(&all);
    gfx.frame()
}

/// 夹具:标题帧 → 点击帧(契约未变的 `COPY2 :/W0/`)。
///
/// 返回 `(frame1, frame2, copy_id)`。前提断言放在这里,任何用例都要先满足
/// 「点击帧没有命令时刻快照」这一 bug 触发条件。
fn click_frame_fixture() -> (
    yu_gfx::CompositorFrame,
    yu_gfx::CompositorFrame,
    yu_gfx::CopyId,
) {
    let table = table();
    let viewport = Viewport::new(8, 8);
    let mut gfx = state(viewport, [0, 0, 0, 255]);

    // 帧 1:标题铺满屏幕(主窗口根组 :/W0 由 ensure_parents 建立)。
    let f1 = frame_of(
        &mut gfx,
        1,
        vec![call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"TITLE".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"title".to_vec())),
                arg(&table, "CG", "Z", Value::Int(1)),
                arg(&table, "CG", "SX", Value::Int(8)),
                arg(&table, "CG", "SY", Value::Int(8)),
            ],
        )],
    );

    // 帧 2:与 s51 pc161..162 同形 —— 先建 E=0 的空目标层,再 COPY2 :/W0/。
    // 命令时刻屏幕契约与帧 1 相同 ⇒ capture_screen_snapshot 必须返回 None。
    let f2 = frame_of(
        &mut gfx,
        2,
        vec![
            call(
                2,
                "CG",
                vec![
                    arg(&table, "CG", "ID", Value::Str(b"SSSSS".to_vec())),
                    arg(&table, "CG", "FILE", Value::Str(Vec::new())),
                    arg(&table, "CG", "E", Value::Int(0)),
                    arg(&table, "CG", "A", Value::Int(256)),
                    arg(&table, "CG", "Z", Value::Int(2)),
                    arg(&table, "CG", "SX", Value::Int(8)),
                    arg(&table, "CG", "SY", Value::Int(8)),
                ],
            ),
            call(
                2,
                "CGACT",
                vec![
                    arg(&table, "CGACT", "COPY2", Value::Int(1)),
                    arg(&table, "CGACT", "ID", Value::Str(b":/W0/".to_vec())),
                    arg(&table, "CGACT", "ID2", Value::Str(b"SSSSS".to_vec())),
                ],
            ),
            call(
                2,
                "CG",
                vec![
                    arg(&table, "CG", "ID", Value::Str(b"SSSSS".to_vec())),
                    arg(&table, "CG", "E", Value::Int(1)),
                    arg(&table, "CG", "A", Value::Int(8)),
                ],
            ),
        ],
    );

    // 前提:点击帧的 W0 复制没有命令时刻快照(契约未变)。
    assert!(
        f2.screen_snapshots.is_empty(),
        "夹具失效:点击帧应命中契约未变捷径(screen_snapshot=None)"
    );
    let copy_id = gfx
        .copies()
        .last()
        .expect("click frame must record a copy")
        .id;
    (f1, f2, copy_id)
}

/// soft oracle:同一帧序列必须得到标题像素(P0 起即绿,证明夹具正确)。
#[test]
fn soft_main_window_copy_reads_last_presented_screen() {
    let (f1, f2, copy_id) = click_frame_fixture();
    let mut soft = SoftRenderer::new(store(&[(
        b"title.png",
        png_bytes_for(8, 8, Rgba(TITLE)),
    )]));
    soft.render_frame(&f1);
    soft.render_frame(&f2);
    let snap = soft
        .snapshot_surface(copy_id)
        .expect("soft 必须物化同一 CopyId");
    assert_eq!(
        snap.pixel(1, 1),
        Some(TITLE),
        "soft oracle:W0 复制 = 上一呈现帧"
    );
}

/// wgpu 回归:streaming、无 CPU 读回时 W0 复制必须读
/// `screen_surface`(上次呈现帧),不得退回清屏色。
#[test]
fn wgpu_main_window_copy_reads_last_presented_screen_without_cpu_readback() {
    let (f1, f2, copy_id) = click_frame_fixture();
    let Some(mut gpu) = renderer_or_skip(store(&[(
        b"title.png",
        png_bytes_for(8, 8, Rgba(TITLE)),
    )])) else {
        return;
    };
    gpu.render_frame_streaming(&f1)
        .expect("frame 1 streaming render");
    gpu.render_frame_streaming(&f2)
        .expect("frame 2 streaming render");
    let snap = gpu
        .read_snapshot(copy_id)
        .expect("点击帧的 W0 复制必须物化出快照");
    assert_eq!(
        snap.pixel(1, 1),
        Some(TITLE),
        "COPY2 ID=:/W0/ 必须复制上一呈现帧(标题),不得是清屏色黑"
    );
}

/// P2 任务书验收 1：点击帧之后 CGS 幕布 25 帧淡出，逐帧内容变化，
/// 首帧是上一帧标题而不是黑。
///
/// 合成场景与 <game-b> `s51 pc161..177` 同形：
/// 标题帧 → `CG CGS E=0` → `COPY2 :/W0/ CGS` → `CG CGS E=1 A=256`
/// → 隐藏标题（让幕布淡出到黑）→ 每帧 `CG CGS A=256-10k`。
#[test]
fn main_window_copy_fades_old_screen_over_25_frames() {
    let table = table();
    let viewport = Viewport::new(8, 8);
    let mut gfx = state(viewport, [0, 0, 0, 255]);

    // 帧 1：标题。
    let f1 = frame_of(
        &mut gfx,
        1,
        vec![call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"TITLE".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"title".to_vec())),
                arg(&table, "CG", "Z", Value::Int(1)),
                arg(&table, "CG", "SX", Value::Int(8)),
                arg(&table, "CG", "SY", Value::Int(8)),
            ],
        )],
    );

    // 帧 2：拍旧屏 → CGS 可见 A=256；随后隐藏标题，幕布下面只剩清屏色。
    let f2 = frame_of(
        &mut gfx,
        2,
        vec![
            call(
                2,
                "CG",
                vec![
                    arg(&table, "CG", "ID", Value::Str(b"CGS".to_vec())),
                    arg(&table, "CG", "FILE", Value::Str(Vec::new())),
                    arg(&table, "CG", "E", Value::Int(0)),
                    arg(&table, "CG", "A", Value::Int(256)),
                    arg(&table, "CG", "Z", Value::Int(2)),
                    arg(&table, "CG", "SX", Value::Int(8)),
                    arg(&table, "CG", "SY", Value::Int(8)),
                ],
            ),
            call(
                2,
                "CGACT",
                vec![
                    arg(&table, "CGACT", "COPY2", Value::Int(1)),
                    arg(&table, "CGACT", "ID", Value::Str(b":/W0/".to_vec())),
                    arg(&table, "CGACT", "ID2", Value::Str(b"CGS".to_vec())),
                ],
            ),
            call(
                2,
                "CG",
                vec![
                    arg(&table, "CG", "ID", Value::Str(b"CGS".to_vec())),
                    arg(&table, "CG", "E", Value::Int(1)),
                ],
            ),
            call(
                2,
                "CG",
                vec![
                    arg(&table, "CG", "ID", Value::Str(b"TITLE".to_vec())),
                    arg(&table, "CG", "E", Value::Int(0)),
                ],
            ),
        ],
    );

    let Some(mut gpu) = renderer_or_skip(store(&[(
        b"title.png",
        png_bytes_for(8, 8, Rgba(TITLE)),
    )])) else {
        return;
    };
    let mut soft = SoftRenderer::new(store(&[(
        b"title.png",
        png_bytes_for(8, 8, Rgba(TITLE)),
    )]));
    // 帧 1/2 用精确路径，保证 screen_surface 与 golden 同步。
    let out1 = gpu.render_frame(&f1).expect("frame 1 render");
    let soft1 = soft.render_frame(&f1);
    assert_eq!(
        max_channel_diff(&out1.surface, &soft1.surface),
        0,
        "帧 1 soft/wgpu 必须逐像素一致"
    );
    let out2 = gpu.render_frame(&f2).expect("frame 2 render");
    let soft2 = soft.render_frame(&f2);
    assert_eq!(
        max_channel_diff(&out2.surface, &soft2.surface),
        0,
        "点击帧 soft/wgpu 必须逐像素一致"
    );
    assert_eq!(
        out2.surface.pixel(1, 1),
        Some(TITLE),
        "点击帧必须显示上一帧标题，不得瞬间黑"
    );

    let mut prev = TITLE[0];
    for k in 1..=25 {
        let a = (256 - k * 10).max(0);
        let frame = frame_of(
            &mut gfx,
            2 + k as u64,
            vec![call(
                2 + k as u64,
                "CG",
                vec![
                    arg(&table, "CG", "ID", Value::Str(b"CGS".to_vec())),
                    arg(&table, "CG", "A", Value::Int(a)),
                ],
            )],
        );
        let out = gpu.render_frame(&frame).expect("fade render");
        let soft_out = soft.render_frame(&frame);
        assert_eq!(
            max_channel_diff(&out.surface, &soft_out.surface),
            0,
            "淡出第 {k} 帧 soft/wgpu 必须逐像素一致"
        );
        let px = out.surface.pixel(1, 1).expect("pixel");
        assert!(
            px[0] <= prev,
            "淡出第 {k} 帧红通道必须单调不增(prev={prev}, now={})",
            px[0]
        );
        prev = px[0];
    }
    let last_frame = frame_of(
        &mut gfx,
        28,
        vec![call(
            28,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"CGS".to_vec())),
                arg(&table, "CG", "A", Value::Int(0)),
            ],
        )],
    );
    let last = gpu.render_frame(&last_frame).expect("final render");
    let soft_last = soft.render_frame(&last_frame);
    assert_eq!(
        max_channel_diff(&last.surface, &soft_last.surface),
        0,
        "淡出末帧 soft/wgpu 必须逐像素一致"
    );
    assert_eq!(
        last.surface.pixel(1, 1),
        Some([0, 0, 0, 255]),
        "25 帧后幕布必须淡到黑"
    );
}
