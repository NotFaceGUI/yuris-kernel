//! `YMV` 影片图层(路线图 B14 / DIV-GFX-073)。
//!
//! 证据锚点(0.495 `yu-ris.exe`,sha256 `4f5eefa3…`):
//! - `YMV` `0x458ee0`:`FILE` 存在时解析容器并挂到图层(`0x459094`),
//!   `FRAME` 存在时换帧(`0x45934c`,帧号钳位);
//! - `YMVINFO` `0x459b98`:`NUM/SX/SY/FPS` 直接读影片结构;层/影片缺失
//!   时向 `LET` 写 **0**;
//! - `YMVEND` `0x459a64`:释放影片结构,图层本身保留。
//!
//! 元数据由 VM 解析容器头后经 `GraphicsHost::note_movie` 推入(测试里直接
//! 调用 `GfxState::set_movie_meta` 模拟)。

mod support;

use support::{arg, call, frame_end, frame_start, i, s, state, table};
use yu_gfx::{MovieInfo, Op, PassTarget, TextureRef};
use yu_rt::event::Domain;
use yu_rt::value::Value;

/// 影片域事件(`Domain::Movie`;测试 support 的 `call` 固定 Graphics)。
fn movie_call(frame: u64, name: &str, args: Vec<yu_rt::event::ArgValue>) -> yu_rt::event::VmEvent {
    yu_rt::event::VmEvent::DomainCall {
        frame,
        domain: Domain::Movie,
        name: name.to_string(),
        args,
        script: None,
        pc: None,
    }
}

fn info() -> MovieInfo {
    MovieInfo {
        frames: 20,
        width: 800,
        height: 600,
        fps: 24,
        has_alpha: true,
    }
}

/// 建一个空的影片层(`CG[ID FILE=""]`,与 `es_Sprite.yst` 的
/// `CG[ID=… E=0 A=0 X=0 Y=0 SX=1 SY=1 Z=1 FILE=""]` 同型),再挂影片。
fn movie_layer(meta: bool) -> (yu_gfx::GfxState, yu_gfx::ParamTable) {
    let table = table();
    let mut gfx = state();
    if meta {
        gfx.set_movie_meta(b"pac/sample2", info());
    }
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("YMV0001")),
                arg(&table, "CG", "FILE", s("")),
            ],
        ),
        movie_call(
            1,
            "YMV",
            vec![
                arg(&table, "YMV", "ID", s("YMV0001")),
                arg(&table, "YMV", "FILE", s("pac/sample2")),
            ],
        ),
        frame_end(1),
    ]);
    (gfx, table)
}

#[test]
fn ymv_attaches_movie_and_reports_metadata() {
    let (gfx, table) = movie_layer(true);
    let layer = gfx.layer(&support::id(":/W0/YMV0001")).expect("layer");
    let movie = layer.movie.as_ref().expect("movie attached");
    assert_eq!(movie.asset, b"pac/sample2".to_vec());
    assert_eq!(movie.frame, 0);
    assert_eq!(movie.info.frames, 20);
    // 首帧即生效:纹理引用直接指向影片第 0 帧。
    assert_eq!(
        layer.texture,
        TextureRef::Movie {
            asset: b"pac/sample2".to_vec(),
            frame: 0
        }
    );
    // 官方 `[层+0x86] = (alpha标志 == 1)`。
    assert!(layer.alpha_aware);

    // `YMVINFO`:与官方同源的四字段。
    let ask = |flag: &str| {
        gfx.query_info(
            "YMVINFO",
            &[
                arg(&table, "YMVINFO", "ID", s("YMV0001")),
                arg(&table, "YMVINFO", flag, i(1)),
            ],
        )
        .expect("query")
    };
    assert_eq!(ask("NUM"), Value::Int(20));
    assert_eq!(ask("SX"), Value::Int(800));
    assert_eq!(ask("SY"), Value::Int(600));
    assert_eq!(ask("FPS"), Value::Int(24));
}

#[test]
fn ymv_frame_clamps_and_switches() {
    let (mut gfx, table) = movie_layer(true);
    let send = |gfx: &mut yu_gfx::GfxState, frame: i64| {
        gfx.consume_events(&[
            frame_start(2),
            movie_call(
                2,
                "YMV",
                vec![
                    arg(&table, "YMV", "ID", s("YMV0001")),
                    arg(&table, "YMV", "FRAME", i(frame)),
                ],
            ),
            frame_end(2),
        ]);
        gfx.layer(&support::id(":/W0/YMV0001"))
            .expect("layer")
            .movie
            .as_ref()
            .expect("movie")
            .frame
    };
    assert_eq!(send(&mut gfx, 7), 7);
    // 官方 `0x45934c`:负 → 0;超尾 → 帧数-1。
    assert_eq!(send(&mut gfx, -5), 0);
    assert_eq!(send(&mut gfx, 999), 19);
    // 纹理引用与影片状态同步。
    assert_eq!(
        gfx.layer(&support::id(":/W0/YMV0001")).unwrap().texture,
        TextureRef::Movie {
            asset: b"pac/sample2".to_vec(),
            frame: 19
        }
    );
}

#[test]
fn ymv_without_metadata_leaves_layer_untouched() {
    let (gfx, _table) = movie_layer(false);
    let layer = gfx.layer(&support::id(":/W0/YMV0001")).expect("layer");
    assert!(layer.movie.is_none());
    // `CG[FILE=""]` 的 1×1 黑图保持不变(官方装载失败时图层原状)。
    assert_eq!(layer.texture, TextureRef::SolidBlack1x1);
    let codes: Vec<String> = gfx
        .diagnostics()
        .iter()
        .map(|d| d.code.clone())
        .collect();
    assert!(
        codes.iter().any(|c| c == "gfx.texture.missing"),
        "应留下资产缺失诊断,实际 {codes:?}"
    );
}

#[test]
fn ymv_requires_existing_layer() {
    let table = table();
    let mut gfx = state();
    gfx.set_movie_meta(b"pac/sample2", info());
    gfx.consume_events(&[
        frame_start(1),
        movie_call(
            1,
            "YMV",
            vec![
                arg(&table, "YMV", "ID", s("NOPE")),
                arg(&table, "YMV", "FILE", s("pac/sample2")),
            ],
        ),
        frame_end(1),
    ]);
    assert!(gfx.layer(&support::id(":/W0/NOPE")).is_none());
    let codes: Vec<String> = gfx
        .diagnostics()
        .iter()
        .map(|d| d.code.clone())
        .collect();
    assert!(
        codes.iter().any(|c| c == "gfx.layer.missing"),
        "应留下图层缺失诊断,实际 {codes:?}"
    );
}

#[test]
fn ymvinfo_zero_when_movie_missing_or_layer_gone() {
    let table = table();
    let mut gfx = state();
    // 图层存在但没有影片 ⇒ 0(官方 `0x459d63`)。
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("PLAIN")),
                arg(&table, "CG", "FILE", s("")),
            ],
        ),
        frame_end(1),
    ]);
    let num = gfx
        .query_info(
            "YMVINFO",
            &[
                arg(&table, "YMVINFO", "ID", s("PLAIN")),
                arg(&table, "YMVINFO", "NUM", i(1)),
            ],
        )
        .expect("query");
    assert_eq!(num, Value::Int(0));
    // 层不存在 ⇒ 同样 0(官方 `0x459de2`)。
    let num = gfx
        .query_info(
            "YMVINFO",
            &[
                arg(&table, "YMVINFO", "ID", s("GONE")),
                arg(&table, "YMVINFO", "NUM", i(1)),
            ],
        )
        .expect("query");
    assert_eq!(num, Value::Int(0));
    // 缺失查询字段 ⇒ 不猜(Unimplemented)。
    assert!(gfx
        .query_info("YMVINFO", &[arg(&table, "YMVINFO", "ID", s("PLAIN"))])
        .is_err());
}

/// 官方 `0x459b98` 对四个查询字段是**顺序 `if` + 各自 `ret`**:同时置位时
/// 槽号最小的那个生效(不是报错、也不是全写)。语料只用单字段,但语义要
/// 对齐;内核曾按「多字段未取证 ⇒ Unimplemented」处理。
#[test]
fn ymvinfo_multiple_flags_first_in_slot_order_wins() {
    let (gfx, table) = movie_layer(true);
    let num = gfx
        .query_info(
            "YMVINFO",
            &[
                arg(&table, "YMVINFO", "ID", s("YMV0001")),
                arg(&table, "YMVINFO", "NUM", i(1)),
                arg(&table, "YMVINFO", "SX", i(1)),
            ],
        )
        .expect("多字段按首个置位字段回答");
    assert_eq!(num, Value::Int(20), "NUM(槽 3)先于 SX(槽 4)");
    let sx_first = gfx
        .query_info(
            "YMVINFO",
            &[
                arg(&table, "YMVINFO", "ID", s("YMV0001")),
                arg(&table, "YMVINFO", "SX", i(1)),
                arg(&table, "YMVINFO", "FPS", i(1)),
            ],
        )
        .expect("query");
    assert_eq!(sx_first, Value::Int(800));
}

#[test]
fn ymvend_releases_movie_but_keeps_layer() {
    let (mut gfx, table) = movie_layer(true);
    gfx.consume_events(&[
        frame_start(3),
        movie_call(
            3,
            "YMVEND",
            vec![arg(&table, "YMVEND", "ID", s("YMV0001"))],
        ),
        frame_end(3),
    ]);
    let layer = gfx.layer(&support::id(":/W0/YMV0001")).expect("layer kept");
    assert!(layer.movie.is_none());
    // 官方释放 D3D 表面 ⇒ 该层不再有图(属性保留)。
    assert_eq!(layer.texture, TextureRef::None);
    // 官方只释放 `[层+0xa0]` 的影片结构,**不改** `[层+0x86]`(alpha 位保留)。
    assert!(layer.alpha_aware);
    let num = gfx
        .query_info(
            "YMVINFO",
            &[
                arg(&table, "YMVINFO", "ID", s("YMV0001")),
                arg(&table, "YMVINFO", "NUM", i(1)),
            ],
        )
        .expect("query");
    assert_eq!(num, Value::Int(0));
}

#[test]
fn ymv_command_is_routed_from_movie_domain() {
    // 域归属仍是 Movie(事件契约不变),但 gfx 必须消费它 —— 否则
    // `YMV[FILE]` 不会挂上影片。手工构造 Movie 域事件(测试 helper 的
    // `call` 固定为 Graphics)。
    let table = table();
    let mut gfx = state();
    gfx.set_movie_meta(b"pac/sample2", info());
    gfx.consume_events(&[
        frame_start(9),
        call(
            9,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("YMV0001")),
                arg(&table, "CG", "FILE", s("")),
            ],
        ),
        yu_rt::event::VmEvent::DomainCall {
            frame: 9,
            domain: Domain::Movie,
            name: "YMV".to_string(),
            args: vec![
                arg(&table, "YMV", "ID", s("YMV0001")),
                arg(&table, "YMV", "FILE", s("pac/sample2")),
            ],
            script: None,
            pc: None,
        },
        frame_end(9),
    ]);
    assert!(gfx
        .layer(&support::id(":/W0/YMV0001"))
        .expect("layer")
        .movie
        .is_some());
}

/// `MOVIE` 全屏影片:overlay 注入后整帧只含影片层(其他图层被遮住),
/// 清 overlay 后恢复正常投影(手册 `movie/movie.html` 例 3)。
#[test]
fn movie_overlay_replaces_all_layers() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("BG")),
                arg(&table, "CG", "FILE", s("")),
            ],
        ),
        frame_end(1),
    ]);
    assert!(gfx.frame().layers.iter().any(|l| l.id.display() == ":/W0/BG"));

    gfx.set_movie_overlay(Some(yu_gfx::MovieOverlay {
        asset: b":movie/current".to_vec(),
        rect: yu_gfx::RectI {
            x: 0,
            y: 0,
            width: 400,
            height: 300,
        },
    }));
    let f = gfx.frame();
    assert_eq!(f.layers.len(), 1, "影片模式只投影影片层");
    assert_eq!(f.layers[0].id.display(), ":/MOVIE");
    assert_eq!(f.passes.len(), 1);
    assert!(f.diagnostics.is_empty(), "{:?}", f.diagnostics);
    assert!(matches!(
        gfx.movie_overlay(),
        Some(o) if o.asset == b":movie/current".to_vec()
    ));

    gfx.set_movie_overlay(None);
    assert!(gfx.frame().layers.iter().any(|l| l.id.display() == ":/W0/BG"));
}

/// 影片模式帧不投影脚本图层/COPY pass,后端会按该帧回收按钮快照;
/// `baked_copies` 若继续把 COPY 当「已冻结」而跳过重发,`MOVIEEND` 后
/// 底部按钮条/UI 快照就永久只剩黑底。回归:退出影片模式必须重发 COPY。
#[test]
fn movie_overlay_exit_replays_baked_copy_snapshots() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("SRC")),
                arg(&table, "CG", "FILE", s("src")),
                arg(&table, "CG", "Z", i(1)),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("DST")),
                arg(&table, "CG", "FILE", s("")),
                arg(&table, "CG", "Z", i(2)),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("SRC")),
                arg(&table, "CGACT", "ID2", s("DST")),
                arg(&table, "CGACT", "COPY2", i(1)),
            ],
        ),
        frame_end(1),
    ]);
    let has_copy = |f: &yu_gfx::CompositorFrame| {
        f.passes.iter().any(|p| {
            matches!(&p.target, PassTarget::Texture(id) if id.display() == ":/W0/DST")
                && p.ops.iter().any(|op| matches!(op, Op::Copy(_)))
        })
    };
    assert!(has_copy(&gfx.frame()), "初始帧应发射 COPY");
    // 模拟后端成功执行(引擎 tick 末 `commit_baked`),COPY 进入冻结集。
    gfx.commit_baked(&[]);

    gfx.set_movie_overlay(Some(yu_gfx::MovieOverlay {
        asset: b":movie/current".to_vec(),
        rect: yu_gfx::RectI {
            x: 0,
            y: 0,
            width: 400,
            height: 300,
        },
    }));
    assert!(!has_copy(&gfx.frame()), "影片帧不投影 COPY pass");

    gfx.set_movie_overlay(None);
    let f = gfx.frame();
    assert!(
        has_copy(&f),
        "MOVIEEND 后必须重发 COPY,否则后端已回收的快照不会重建"
    );
    assert!(
        f.layers.iter().any(|l| l.id.display() == ":/W0/DST"
            && matches!(l.texture, TextureRef::Snapshot(_))),
        "目标层仍引用快照"
    );
}
