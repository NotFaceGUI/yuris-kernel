//! 影片帧成本的量化探针(`YMV` / B14)。
//!
//! 只回答一个问题:**换一帧到底花在哪**。分别计时:
//! 1. 容器载入 + 全平面解混淆(官方 `0x459094`,每片一次);
//! 2. 单帧物化(切段 + 两张 JPEG 解码 + RGBA 合成;官方 `0x45934c`);
//! 3. 同帧重复取(缓存命中);
//! 4. 软后端整帧渲染(含 `CGACT[SIZE2]` 缩放 blit)。
//!
//! 用法:
//! ```text
//! YURIS_SAMPLE_SDK0495=<sdk>/システム cargo run -p yu-backend-soft \
//!     --example probe_ymv_timing -- [debug|release 由构建模式决定]
//! ```
//! 结论口径:debug 构建下 `zune-jpeg` 解码会慢 5–10 倍,评估播放帧率必须用
//! `--release`;本探针两种模式都能跑,便于对照。

use std::time::Instant;

use yu_backend_soft::{SoftRenderer, TextureStore};
use yu_format::yscm::{Arg as FmtArg, Command as FmtCommand, CommandTable};
use yu_gfx::{Clear, GfxConfig, GfxState, ImageFormat, MovieInfo, ParamTable, Viewport};
use yu_rt::event::{ArgValue, Domain, VmEvent};
use yu_rt::tables::YscmTable;
use yu_rt::value::Value;
use yu_vfs::{Codec, DataSource, DirSource};

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
            command("CG", &["ID", "FILE", "SX", "SY", "X", "Y"]),
            command("YMV", &["ID", "IDNO", "FILE", "FRAME"]),
            command(
                "CGACT",
                &[
                    "ID", "ID2", "SIZE", "SIZE2", "FX", "FY", "FX2", "FY2", "FSX", "FSY",
                ],
            ),
        ],
        messages: Vec::new(),
        key_table: Box::new([0u8; 256]),
    };
    ParamTable::new(YscmTable::from_format(fmt).expect("synthetic yscm"))
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

fn main() {
    let Ok(root) = std::env::var("YURIS_SAMPLE_SDK0495") else {
        eprintln!("需要 YURIS_SAMPLE_SDK0495=<sdk>/システム");
        std::process::exit(2);
    };
    let data = format!("{root}/data");
    let bytes = std::fs::read(format!("{data}/pac/sample2.ymv")).expect("sample2.ymv");
    println!(
        "构建模式 = {}",
        if cfg!(debug_assertions) { "debug" } else { "release" }
    );
    println!("影片 = sample2.ymv({} 字节)", bytes.len());

    // 1. 容器载入 + 解混淆(官方 0x459094,每片一次)。
    let t = Instant::now();
    let container = yu_format::ymv::DecryptedYmv::parse(&bytes, b"pac/sample2").expect("parse");
    let load_us = t.elapsed().as_micros() as u64;
    let info = container.info();
    println!(
        "容器载入+解混淆: {load_us}us(≈{:.2}ms,{} 帧 {}x{})",
        load_us as f64 / 1000.0,
        info.frames,
        info.width,
        info.height
    );

    // 1b. 对照:每帧"回包重读"的成本(旧实现的每帧固定开销)。
    {
        let source = DirSource::open(&data, Codec::Cp932).expect("sdk data");
        let lookup = yu_vfs::LookupKey::new(b"pac/sample2.ymv", Codec::Cp932).expect("key");
        let t = Instant::now();
        let mut total = 0usize;
        for _ in 0..5 {
            total += source.open(&lookup).expect("open").len();
        }
        println!(
            "对照·每帧回包重读: {:.2}ms/次({} 字节)",
            t.elapsed().as_micros() as f64 / 5.0 / 1000.0,
            total / 5
        );
    }

    // 2/3. 单帧物化(缓存未命中)与同帧命中。
    let mut store = TextureStore::new(
        Box::new(DirSource::open(&data, Codec::Cp932).expect("sdk data")),
        Codec::Cp932,
        vec![ImageFormat::Png, ImageFormat::Jpg],
    );
    let frames = [0u32, 1, 60, 150, 300];
    let mut total_us = 0u64;
    for &f in &frames {
        let t = Instant::now();
        let tex = store.resolve_movie(b"pac/sample2", f).expect("frame");
        let us = t.elapsed().as_micros() as u64;
        total_us += us;
        println!(
            "  帧 {f:>3} 冷解码: {us:>6}us(≈{:>5.2}ms,{}x{})",
            us as f64 / 1000.0,
            tex.width,
            tex.height
        );
    }
    println!(
        "  冷解码均值: {:.2}ms/帧",
        total_us as f64 / frames.len() as f64 / 1000.0
    );
    let t = Instant::now();
    for _ in 0..100 {
        let _ = store.resolve_movie(b"pac/sample2", 300).expect("hit");
    }
    println!(
        "  同帧命中: {:.1}us/次",
        t.elapsed().as_micros() as f64 / 100.0
    );

    // 4. 软后端整帧渲染(影片层 + 框架式 SIZE2 blit)。
    let table = table();
    let mut config = GfxConfig::new(Viewport::new(400, 300));
    config.clear = Clear::explicit([0, 0, 0, 255]);
    config.image_probe_order = vec![ImageFormat::Png, ImageFormat::Jpg];
    let mut gfx = GfxState::new(table.clone(), config);
    gfx.set_movie_meta(
        b"pac/sample2",
        MovieInfo {
            frames: info.frames,
            width: info.width,
            height: info.height,
            fps: info.fps,
            has_alpha: info.has_alpha,
        },
    );
    gfx.consume_events(&[
        VmEvent::FrameStart { frame: 1 },
        call(
            1,
            Domain::Graphics,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"MV".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(Vec::new())),
            ],
        ),
        call(
            1,
            Domain::Graphics,
            "CG",
            vec![
                arg(&table, "CG", "ID", Value::Str(b"DISP".to_vec())),
                arg(&table, "CG", "FILE", Value::Str(Vec::new())),
                arg(&table, "CG", "SX", Value::Int(400)),
                arg(&table, "CG", "SY", Value::Int(300)),
            ],
        ),
        call(
            1,
            Domain::Movie,
            "YMV",
            vec![
                arg(&table, "YMV", "ID", Value::Str(b"MV".to_vec())),
                arg(&table, "YMV", "FILE", Value::Str(b"pac/sample2".to_vec())),
            ],
        ),
        VmEvent::FrameEnd { frame: 1 },
    ]);
    let mut backend = SoftRenderer::new(TextureStore::new(
        Box::new(DirSource::open(&data, Codec::Cp932).expect("sdk data")),
        Codec::Cp932,
        vec![ImageFormat::Png, ImageFormat::Jpg],
    ));
    let n = 20u32;
    let mut render_us = 0u64;
    for i in 0..n {
        let frame_no = 2 + u64::from(i);
        let idx = i * 15;
        gfx.consume_events(&[
            VmEvent::FrameStart { frame: frame_no },
            call(
                frame_no,
                Domain::Movie,
                "YMV",
                vec![
                    arg(&table, "YMV", "ID", Value::Str(b"MV".to_vec())),
                    arg(&table, "YMV", "FRAME", Value::Int(i64::from(idx))),
                ],
            ),
            call(
                frame_no,
                Domain::Graphics,
                "CGACT",
                vec![
                    arg(&table, "CGACT", "ID", Value::Str(b"MV".to_vec())),
                    arg(&table, "CGACT", "ID2", Value::Str(b"DISP".to_vec())),
                    arg(&table, "CGACT", "SIZE2", Value::Int(1)),
                    arg(&table, "CGACT", "FX", Value::Float(0.0)),
                    arg(&table, "CGACT", "FY", Value::Float(0.0)),
                    arg(&table, "CGACT", "FX2", Value::Float(40000.0)),
                    arg(&table, "CGACT", "FY2", Value::Float(30000.0)),
                    arg(&table, "CGACT", "FSX", Value::Float(40000.0)),
                    arg(&table, "CGACT", "FSY", Value::Float(30000.0)),
                ],
            ),
            VmEvent::FrameEnd { frame: frame_no },
        ]);
        let t = Instant::now();
        let out = backend.render_frame(&gfx.frame());
        render_us += t.elapsed().as_micros() as u64;
        assert_eq!(out.report.skipped_ops_total, 0, "{:?}", out.report.skipped);
    }
    println!(
        "软后端整帧渲染(含换帧冷解码): {:.2}ms/帧",
        render_us as f64 / f64::from(n) / 1000.0
    );
}
