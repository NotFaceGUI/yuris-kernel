//! `plan()` 阶段耗时随**层数**的缩放(人工量化,`--ignored` 运行)。
//!
//! 背景:ERIS 打开「環境設定」时单帧建到 **1500 层**(`menu-spike.log`
//! `layers=1500 passes=282 plan=9.8ms`),`plan` 成为 render 尖峰的最大项。
//! 其中 `plan()` 开头的逐层 `base_token` 会为每一层在 `frame.layers` 里
//! 线性搜同一个 ID —— 若不修,就是 `O(层数²)` 的 `LayerId` 比较。
//!
//! 这个基准把层数做成自变量,直接给出耗时曲线;它**不做断言**(墙钟在
//! CI 上不稳),只打印,供人工判断是否仍是平方关系:
//!
//! ```text
//! cargo test --release -p yu-backend-wgpu --test plan_scaling -- --ignored --nocapture
//! ```
//!
//! 判读方法:层数 4 倍时,`plan` 若涨约 4 倍 = 线性(已修好);
//! 涨约 16 倍 = 仍是平方(有回归)。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use yu_backend_wgpu::device::code;
use yu_backend_wgpu::WgpuRenderer;
use yu_format::yscm::{Arg as FmtArg, Command as FmtCommand, CommandTable};
use yu_gfx::{GfxConfig, GfxState, ImageFormat, ParamTable, Viewport};
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
                &["ID", "FILE", "E", "A", "X", "Y", "Z", "SX", "SY"],
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

fn arg(table: &ParamTable, cmd: &str, name: &str, value: Value) -> ArgValue {
    ArgValue::new(slot(table, cmd, name), value)
}

fn s(v: &str) -> Value {
    Value::Str(v.as_bytes().to_vec())
}

fn i(v: i64) -> Value {
    Value::Int(v)
}

fn state(table: &ParamTable, viewport: Viewport) -> GfxState {
    let mut config = GfxConfig::new(viewport);
    config.image_probe_order = vec![ImageFormat::Png];
    GfxState::new(table.clone(), config)
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

/// 建一个含 `layers` 个**可见图像层**的帧(全部挂在主组下)。
///
/// 层 ID 用等长的长名字,还原真实语料里的 `:/W0/BT.OFFES.…` 形态 ——
/// `base_token` 的逐层查找成本正比于 `LayerId` 的字节长度。
fn frame_with_layers(table: &ParamTable, layers: usize, frame_no: u64) -> GfxState {
    let viewport = Viewport::new(800, 600);
    let mut gfx = state(table, viewport);
    let mut events = vec![VmEvent::FrameStart { frame: 1 }];
    for n in 0..layers {
        // 40 字节量级的名字,和真实语料相当。
        let id = format!(":/W0/BT.OFFES.CONFIG.BTN.SLOT{n:0>6}");
        events.push(call(
            1,
            "CG",
            vec![
                arg(table, "CG", "ID", s(&id)),
                arg(table, "CG", "FILE", s("")),
                arg(table, "CG", "SX", i(16)),
                arg(table, "CG", "SY", i(16)),
                arg(table, "CG", "Z", i(n as i64)),
            ],
        ));
    }
    events.push(VmEvent::FrameEnd { frame: 1 });
    gfx.consume_events(&events);
    let frame = gfx.frame();
    assert_eq!(frame.frame, 1);
    // 断言真的建起来了,否则这个基准什么都没测。
    assert!(
        frame.layers.len() >= layers,
        "期望至少 {layers} 层,实际 {}",
        frame.layers.len()
    );
    let _ = frame_no;
    gfx
}

fn renderer_or_skip() -> Option<WgpuRenderer> {
    let source = MemSource::new(Codec::Cp932);
    let store = yu_backend_soft::TextureStore::new(
        Box::new(source),
        Codec::Cp932,
        vec![ImageFormat::Png],
    );
    match WgpuRenderer::new(store) {
        Ok(renderer) => Some(renderer),
        Err(diag) if diag.code == code::ADAPTER_UNAVAILABLE => {
            SKIPPED.fetch_add(1, Ordering::Relaxed);
            println!("SKIP yu-backend-wgpu: 无适配器 [{}] {}", diag.code, diag.detail);
            None
        }
        Err(diag) => panic!("GPU 初始化失败 [{}]: {}", diag.code, diag.detail),
    }
}

#[test]
#[ignore = "人工量化用;release 档跑才有意义"]
fn plan_cost_scaling_with_layer_count() {
    let Some(mut renderer) = renderer_or_skip() else {
        return;
    };
    let table = table();
    println!("{:>8} {:>10} {:>12} {:>12}", "layers", "plan_min", "plan_us/层", "ratio");
    let mut prev: Option<(usize, f64)> = None;
    for layers in [250usize, 500, 1000, 2000] {
        let mut gfx = frame_with_layers(&table, layers, 1);
        // 冷启动一次(建管线/上传),不计入。
        let warm = gfx.frame();
        let _ = renderer.render_frame(&warm).expect("warm render");

        let mut best = f64::MAX;
        for r in 0..7 {
            let frame_no = 100 + r;
            gfx.consume_events(&[
                VmEvent::FrameStart { frame: frame_no },
                VmEvent::FrameEnd { frame: frame_no },
            ]);
            let frame = gfx.frame();
            let started = Instant::now();
            let _out = renderer.render_frame(&frame).expect("render");
            // 用 plan 阶段而不是 total:后者含读回与提交抖动。
            best = best.min(renderer.render_phases().plan_us as f64);
            let _ = started;
        }
        let per_layer = best / layers as f64;
        let ratio = prev.map_or(f64::NAN, |(pl, pu)| {
            let layer_ratio = layers as f64 / pl as f64;
            (best / pu) / layer_ratio
        });
        println!(
            "{layers:>8} {best:>10.0} {per_layer:>12.2} {ratio:>12.2}",
            layers = layers,
            best = best,
            per_layer = per_layer,
            ratio = ratio
        );
        prev = Some((layers, best));
    }
    println!(
        "判读:ratio ≈ 1.0 ⇒ 线性(已修);ratio ≈ 层数倍数 ⇒ 平方(回归)"
    );
}
