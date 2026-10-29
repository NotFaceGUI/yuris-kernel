//! 晚一帧 `SAVE[ID]` 的端到端回归:帧 N 的 `COPY/COPY2` 产生快照,
//! 帧 N+1 的 `SAVE[ID]` 才去取像素。
//!
//! wgpu 引擎在 `SAVE` 命令时刻通过 `SnapshotSource` 读**上一帧已冻结**的
//! 快照(官方读取端 `0x4477c0` 同帧直接读层表面);这里验证:
//! - 渲染后的 `observe` 不主动回读(eager=false);
//! - `capture` 按需读一次并缓存;
//! - 端口拿不到像素时显式计数并登记 `wanted`,不静默返回空图。

use std::cell::RefCell;
use std::rc::Rc;

use yu_backend_soft::{png, Surface, TextureStore};
use yu_engine::capture::GfxCapture;
use yu_format::yscm::{Arg as FmtArg, Command as FmtCommand, CommandTable};
use yu_gfx::{CopyId, GfxConfig, GfxState, LayerCapture, Op, ParamTable, Viewport};
use yu_rt::event::{ArgValue, Domain, VmEvent};
use yu_rt::tables::YscmTable;
use yu_rt::value::Value;
use yu_vfs::{Codec, MemSource, SharedMem};

fn command(name: &str, params: &[&str]) -> FmtCommand {
    FmtCommand {
        name: name.into(),
        argc: params.len() as u8,
        args: params
            .iter()
            .map(|p| FmtArg {
                name: (*p).into(),
                flag: u8::from(matches!(*p, "ID" | "ID2" | "FILE")),
                kind: 0,
            })
            .collect(),
    }
}

fn table() -> ParamTable {
    let fmt = CommandTable {
        version: 495,
        commands: vec![
            command("CG", &["ID", "IDNO", "FILE", "Z", "A", "SX", "SY"]),
            command("CGACT", &["ID", "ID2", "COPY", "COPY2"]),
        ],
        messages: Vec::new(),
        key_table: Box::new([0u8; 256]),
    };
    ParamTable::new(YscmTable::from_format(fmt).expect("synthetic yscm"))
}

fn slot(table: &ParamTable, cmd: &str, name: &str) -> u8 {
    table
        .params(cmd)
        .expect("command")
        .iter()
        .position(|p| p.name == name)
        .map(|i| u8::try_from(i).expect("slot"))
        .expect("param")
}

fn arg(table: &ParamTable, cmd: &str, name: &str, value: Value) -> ArgValue {
    ArgValue::new(slot(table, cmd, name), value)
}

fn call(frame: u64, name: &str, args: Vec<ArgValue>) -> VmEvent {
    VmEvent::DomainCall {
        frame,
        domain: Domain::Graphics,
        name: name.into(),
        args,
        script: None,
        pc: None,
    }
}

/// 帧 N:建源层 + `CGACT[COPY2]` 生成目标快照。
fn state_with_copy() -> (GfxState, CopyId) {
    let table = table();
    let mut config = GfxConfig::new(Viewport::new(800, 600));
    config.image_probe_order = vec![yu_gfx::ImageFormat::Png];
    let mut gfx = GfxState::new(table.clone(), config);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"SRC".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"src".to_vec())),
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
    let copy_id = frame
        .passes
        .iter()
        .flat_map(|pass| pass.ops.iter())
        .find_map(|op| match op {
            Op::Copy(copy) => Some(copy.id),
            _ => None,
        })
        .expect("COPY2 快照");
    (gfx, copy_id)
}

struct FakeSource {
    id: CopyId,
    calls: Rc<RefCell<u32>>,
}

impl yu_render_core::SnapshotSource for FakeSource {
    fn read(&mut self, id: CopyId) -> Option<Surface> {
        *self.calls.borrow_mut() += 1;
        (id == self.id).then(|| Surface::new(2, 2, [1, 2, 3, 255]))
    }
}

fn capture_with(source_id: CopyId) -> (GfxCapture, Rc<RefCell<u32>>) {
    let store = TextureStore::new(
        Box::new(MemSource::new(Codec::Cp932)),
        Codec::Cp932,
        Vec::new(),
    );
    let mut capture = GfxCapture::new(store, SharedMem::new(Codec::Cp932));
    let calls = Rc::new(RefCell::new(0));
    capture.set_source(Box::new(FakeSource {
        id: source_id,
        calls: Rc::clone(&calls),
    }));
    capture.set_eager(false);
    (capture, calls)
}

#[test]
fn next_frame_save_reads_frozen_snapshot_once() {
    let (gfx, copy_id) = state_with_copy();
    let frame = gfx.frame();
    let (mut capture, calls) = capture_with(copy_id);

    // 渲染后:wgpu 路径不主动回读。
    assert!(capture.observe(&frame).is_empty());
    assert_eq!(*calls.borrow(), 0, "没有 SAVE 不得回读快照");

    // 下一帧 `SAVE[ID]`:按需读一次,像素正确。
    let image = capture
        .capture(&gfx, b"DST")
        .expect("capture")
        .expect("snapshot pixels");
    assert_eq!((image.width, image.height), (2, 2));
    assert_eq!(image.rgba, Surface::new(2, 2, [1, 2, 3, 255]).rgba);
    assert_eq!(*calls.borrow(), 1);
    assert_eq!(capture.missing_pixels, 0);

    // 再次 SAVE 命中镜像缓存。
    capture.capture(&gfx, b"DST").expect("capture").expect("cached");
    assert_eq!(*calls.borrow(), 1, "同一快照只读一次");
}

#[test]
fn unavailable_snapshot_is_counted_and_queued() {
    let (gfx, _copy_id) = state_with_copy();
    let frame = gfx.frame();
    let (mut capture, calls) = capture_with(CopyId(999));

    assert!(capture
        .capture(&gfx, b"DST")
        .expect("capture")
        .is_none());
    assert_eq!(capture.missing_pixels, 1, "缺像素必须显式计数");
    assert_eq!(*calls.borrow(), 1);

    // 仍未物化 ⇒ 渲染后登记给宿主补齐。
    let wanted = capture.observe(&frame);
    assert_eq!(wanted.len(), 1, "缺像素的快照进入 wanted");
    assert!(capture.capture(&gfx, b"DST").expect("capture").is_none());
    assert_eq!(capture.missing_pixels, 2);
}

/// 官方存档脚本同 tick `CG[FILE=""] → COPY2 → SAVE[ID]`
/// (`es_saveload` pc312/pc313/pc319)。渲染还没发生,必须按命令时刻
/// 软回放拿到快照,不能退化成 1×1 占位。
#[test]
fn same_tick_copy_then_save_uses_source_chain() {
    let table = table();
    let png = png::png_bytes(&Surface::new(8, 8, [20, 40, 60, 255])).expect("fixture png");
    let mut source = MemSource::new(Codec::Cp932);
    source.insert(b"base.png", png).expect("insert asset");
    let store = TextureStore::new(Box::new(source), Codec::Cp932, vec![yu_gfx::ImageFormat::Png]);
    let mut capture = GfxCapture::new(store, SharedMem::new(Codec::Cp932));

    let mut config = GfxConfig::new(Viewport::new(800, 600));
    config.image_probe_order = vec![yu_gfx::ImageFormat::Png];
    let mut gfx = GfxState::new(table.clone(), config);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        // pc312 形态:先建 1×1 占位层。
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"SLTEMP2".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(Vec::new())),
                arg(&table, "CG", "SX", Value::Int(1)),
                arg(&table, "CG", "SY", Value::Int(1)),
            ],
        ),
        // 被拷贝的真实内容。
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"SRC".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(b"base".to_vec())),
            ],
        ),
        // pc313 形态:同 tick 落快照。
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", Value::Str(b"SRC".to_vec())),
                arg(&table, "CGACT", "ID2", Value::Str(b"SLTEMP2".to_vec())),
                arg(&table, "CGACT", "COPY2", Value::Int(1)),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);

    // 关键:没有任何渲染/observe,直接 SAVE。
    let image = capture
        .capture(&gfx, b"SLTEMP2")
        .expect("capture")
        .expect("命令时刻裁剪必须产出快照");
    assert_eq!((image.width, image.height), (8, 8));
    assert_eq!(&image.rgba[..4], [20, 40, 60, 255]);
    assert_eq!(capture.snapshot_replays, 0, "资产源走裁剪快路径");
    assert_eq!(capture.missing_pixels, 0);

    // 同一快照只物化一次。
    let again = capture
        .capture(&gfx, b"SLTEMP2")
        .expect("capture")
        .expect("cached");
    assert_eq!(again.rgba, image.rgba);
    assert_eq!(capture.snapshot_replays, 0);
}

/// 源是组/后缓冲(命令时刻无法从资产链裁剪)时,才整帧软回放。
#[test]
fn same_tick_group_copy_then_save_replays_last_screen() {
    let table = table();
    let store = TextureStore::new(
        Box::new(MemSource::new(Codec::Cp932)),
        Codec::Cp932,
        Vec::new(),
    );
    let mut capture = GfxCapture::new(store, SharedMem::new(Codec::Cp932));
    capture.set_last_screen(Some(std::sync::Arc::new(Surface::new(
        4,
        4,
        [9, 8, 7, 255],
    ))));

    let mut config = GfxConfig::new(Viewport::new(800, 600));
    config.image_probe_order = vec![yu_gfx::ImageFormat::Png];
    let mut gfx = GfxState::new(table.clone(), config);
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"THUMB".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(Vec::new())),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", Value::Str(b":/W0/".to_vec())),
                arg(&table, "CGACT", "ID2", Value::Str(b"THUMB".to_vec())),
                arg(&table, "CGACT", "COPY2", Value::Int(1)),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);

    let image = capture
        .capture(&gfx, b"THUMB")
        .expect("capture")
        .expect("整帧回放必须产出快照");
    assert_eq!((image.width, image.height), (4, 4));
    assert_eq!(&image.rgba[..4], [9, 8, 7, 255]);
    assert_eq!(capture.snapshot_replays, 1);
}
