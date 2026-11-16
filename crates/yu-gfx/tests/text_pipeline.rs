//! P2 文本链路:跨帧持久 / `_TEXT_FLAG` 排版 / FONT 字段 / `CGACT[TEXT]`。
//!
//! 证据见 `crates/yu-gfx/src/text.rs` 与 `docs/divergences/gfx-text.md`。

mod support;

use support::{arg, call, frame_start, i, s, state, table};
use yu_gfx::Op;
use yu_rt::text::{TextFrame, TextGlyphDraw, TextStep};

fn glyph(bytes: &[u8], x: i64, y: i64, full: bool) -> TextStep {
    TextStep::Glyph(TextGlyphDraw {
        codec: yu_rt::Codec::Cp932,
        bytes: bytes.iter().copied().collect(),
        full,
        x,
        y,
        width: 20,
        height: 20,
        style: yu_rt::text::TextGlyphStyle {
            fg: 0x00FF_FFFF,
            bg: 0x00_FF00,
            antialias: true,
            gradation: 2,
            bold: true,
        },
        draw_mode: 0,
        bno: 1,
    })
}

fn text_frame(layer: &[u8], page: u64, first: usize, steps: Vec<TextStep>) -> TextFrame {
    TextFrame {
        layer_id: layer.to_vec(),
        output_mode: 1,
        page_clear_seq: page,
        cursor_x: 0,
        cursor_y: 0,
        click: false,
        bytes: Vec::new(),
        flags: Vec::new(),
        font: Default::default(),
        total_steps: first + steps.len(),
        first_step: first,
        steps,
    }
}

fn cg_text_layer(gfx: &mut yu_gfx::GfxState, t: &yu_gfx::ParamTable, id: &str, w: i64, h: i64) {
    gfx.on_event(&call(
        0,
        "CG",
        vec![
            arg(t, "CG", "ID", s(id)),
            arg(t, "CG", "FILE", s("")),
            arg(t, "CG", "SX", i(w)),
            arg(t, "CG", "SY", i(h)),
        ],
    ));
}

#[test]
fn font_change_reprojects_committed_layout_style() {
    let mut gfx = state();
    let t = table();
    gfx.on_event(&frame_start(0));
    cg_text_layer(&mut gfx, &t, "MSG", 64, 32);
    gfx.apply_text_frame(&text_frame(
        b"MSG",
        0,
        0,
        vec![glyph(b"A", 0, 0, false)],
    ));
    assert!(
        gfx.frame()
            .text_layout
            .expect("committed layout")
            .style
            .name
            .is_none()
    );
    let set_name = |gfx: &mut yu_gfx::GfxState, name: &str| {
        gfx.on_event(&call(
            0,
            "FONT",
            vec![arg(&t, "FONT", "NAME", s(name))],
        ));
    };
    // 行级样式参与布局投影:换字体后旧缓存必须失效。
    set_name(&mut gfx, "Alpha");
    assert_eq!(
        gfx.frame()
            .text_layout
            .expect("after Alpha")
            .style
            .name
            .as_deref(),
        Some(b"Alpha".as_slice())
    );
    set_name(&mut gfx, "Beta");
    assert_eq!(
        gfx.frame()
            .text_layout
            .expect("after Beta")
            .style
            .name
            .as_deref(),
        Some(b"Beta".as_slice())
    );
}

#[test]
fn committed_glyphs_target_text_layer() {
    let mut gfx = state();
    let t = table();
    gfx.on_event(&frame_start(0));
    cg_text_layer(&mut gfx, &t, "MSG", 64, 32);
    let mut snap = text_frame(
        b"MSG",
        0,
        0,
        vec![glyph(b"A", 0, 0, false), glyph(b"\x82\xa0", 10, 0, true)],
    );
    snap.bytes = b"A\x82\xa0".to_vec();
    snap.flags = b"12".to_vec();
    gfx.apply_text_frame(&snap);

    let out = gfx.frame();
    let layout = out.text_layout.as_ref().expect("committed layout");
    assert_eq!(
        layout.target,
        Some(yu_gfx::path::normalize(b"MSG", None, false).path),
        "自动文本只画进 TEXT[ID] 命中层"
    );
    assert_eq!(layout.glyphs.len(), 2);
    assert!(!layout.glyphs[0].full);
    assert!(layout.glyphs[1].full);
    assert_eq!(layout.glyphs[1].x, 10);
    // 逐字符样式/尺寸快照必须原样透出。
    assert_eq!(layout.glyphs[0].width, 20);
    assert_eq!(layout.glyphs[0].style.fg, 0x00FF_FFFF);
    assert_eq!(layout.glyphs[0].style.bg, 0x00_FF00);
    assert_eq!(layout.glyphs[0].style.gradation, 2);
    assert!(layout.glyphs[0].style.bold);
    // 待显示缓冲只做诊断镜像。
    assert_eq!(out.text.len(), 1);
    assert_eq!(out.text[0].bytes, b"A\x82\xa0");
    assert_eq!(out.text[0].flags.as_deref(), Some(&b"12"[..]));
}

#[test]
fn page_clear_drops_surface_but_text_clear_does_not() {
    let mut gfx = state();
    let t = table();
    gfx.on_event(&frame_start(0));
    cg_text_layer(&mut gfx, &t, "MSG", 64, 32);
    gfx.apply_text_frame(&text_frame(b"MSG", 0, 0, vec![glyph(b"A", 0, 0, false)]));
    assert_eq!(gfx.frame().text_layout.expect("A").glyphs.len(), 1);

    // 'P' 换页:清表面,只留 PageClear。
    gfx.apply_text_frame(&text_frame(b"MSG", 1, 0, vec![TextStep::PageClear]));
    assert!(gfx.frame().text_layout.is_none(), "'P' 清已提交字形");

    gfx.apply_text_frame(&text_frame(b"MSG", 1, 1, vec![glyph(b"B", 0, 0, false)]));
    assert_eq!(gfx.frame().text_layout.expect("B").glyphs.len(), 1);

    // TEXT[CLEAR]:同页、无新步骤、空待显示缓冲 —— 已提交像素必须保留。
    let mut clear = text_frame(b"MSG", 1, 2, Vec::new());
    clear.bytes.clear();
    clear.flags.clear();
    gfx.apply_text_frame(&clear);
    assert_eq!(
        gfx.frame().text_layout.expect("after CLEAR").glyphs.len(),
        1,
        "TEXT[CLEAR] 只清待显示缓冲,不清表面"
    );
}

#[test]
fn missing_target_layer_is_not_drawn() {
    let mut gfx = state();
    gfx.on_event(&frame_start(0));
    gfx.apply_text_frame(&text_frame(
        b"NOPE",
        0,
        0,
        vec![glyph(b"A", 0, 0, false)],
    ));
    assert!(gfx.frame().text_layout.is_none(), "无目标层 = 不绘制");
}

#[test]
fn text_layer_surface_uses_layer_pixels() {
    let mut gfx = state();
    let t = table();
    gfx.on_event(&frame_start(0));
    cg_text_layer(&mut gfx, &t, "MSG", 64, 32);
    assert_eq!(gfx.text_layer_surface(b"MSG"), Some((64, 32)));
    assert_eq!(gfx.text_layer_surface(b"NOPE"), None);
    assert_eq!(gfx.text_layer_surface(b""), None);
}

#[test]
fn switching_text_layer_replays_page_from_zero() {
    let mut gfx = state();
    let t = table();
    gfx.on_event(&frame_start(0));
    cg_text_layer(&mut gfx, &t, "A", 64, 32);
    cg_text_layer(&mut gfx, &t, "B", 64, 32);
    gfx.apply_text_frame(&text_frame(b"A", 0, 0, vec![glyph(b"A", 0, 0, false)]));
    assert_eq!(
        gfx.frame().text_layout.expect("A").target,
        Some(yu_gfx::path::normalize(b"A", None, false).path)
    );
    // 层切换:engine 从 0 重放新层本页日志。
    gfx.apply_text_frame(&text_frame(
        b"B",
        0,
        0,
        vec![glyph(b"B", 0, 0, false), glyph(b"C", 10, 0, false)],
    ));
    let layout = gfx.frame().text_layout.expect("B");
    assert_eq!(
        layout.target,
        Some(yu_gfx::path::normalize(b"B", None, false).path)
    );
    assert_eq!(layout.glyphs.len(), 2, "不混入旧层字形");
}

#[test]
fn font_fields_are_stored_clamped_and_read_back() {
    let mut gfx = state();
    let t = table();
    gfx.on_event(&frame_start(0));
    gfx.on_event(&call(
        0,
        "FONT",
        vec![
            arg(&t, "FONT", "GRADATION", i(3)),
            arg(&t, "FONT", "ROTATION", i(5)),
            arg(&t, "FONT", "COLOR", i(0x12_3456)),
            arg(&t, "FONT", "COLOR2", i(0x00_BCDE)),
            arg(&t, "FONT", "SX", i(-7)),
            arg(&t, "FONT", "SY", i(999)),
        ],
    ));
    let (_, _, style, _) = gfx.text_state();
    assert_eq!(style.gradation, 3);
    assert_eq!(style.rotation, 5);
    assert_eq!(style.color, 0x12_3456);
    assert_eq!(style.color2, Some(0x00_BCDE));
    assert_eq!(style.size_x, 0, "SX clamp 0..=256");
    assert_eq!(style.size_y, 256, "SY clamp 0..=256");

    // FONTINFO 读回(单字段查询)。
    let q = |name: &str| {
        gfx.query_info(
            "FONTINFO",
            &[
                arg(&t, "FONTINFO", name, i(1)),
                arg(&t, "FONTINFO", "LET", i(0)),
            ],
        )
        .expect("query")
    };
    assert_eq!(q("GRADATION"), yu_rt::Value::Int(3));
    assert_eq!(q("ROTATION"), yu_rt::Value::Int(5));
    assert_eq!(q("COLOR2"), yu_rt::Value::Int(0x00_BCDE));
}

#[test]
fn font_defaults_match_engine_values() {
    let gfx = state();
    let t = table();
    // FUN_00464fac: SX=SY=20, COLOR=COLOR2=0xffffff, GRADATION=1, ROTATION=0.
    let q = |name: &str| {
        gfx.query_info(
            "FONTINFO",
            &[
                arg(&t, "FONTINFO", name, i(1)),
                arg(&t, "FONTINFO", "LET", i(0)),
            ],
        )
        .expect("query")
    };
    assert_eq!(q("SX"), yu_rt::Value::Int(20));
    assert_eq!(q("SY"), yu_rt::Value::Int(20));
    assert_eq!(q("COLOR2"), yu_rt::Value::Int(0xff_ffff));
    assert_eq!(q("GRADATION"), yu_rt::Value::Int(1));
    assert_eq!(q("ROTATION"), yu_rt::Value::Int(0));
}

#[test]
fn cgact_text_becomes_a_layer_draw() {
    let mut gfx = state();
    let t = table();
    gfx.on_event(&frame_start(0));
    gfx.on_event(&call(
        0,
        "CG",
        vec![
            arg(&t, "CG", "ID", s("TX")),
            arg(&t, "CG", "FILE", s("")),
            arg(&t, "CG", "Z", i(10)),
        ],
    ));
    gfx.on_event(&call(
        0,
        "FONT",
        vec![
            arg(&t, "FONT", "SX", i(12)),
            arg(&t, "FONT", "SY", i(12)),
            arg(&t, "FONT", "COLOR", i(0x00_FF00)),
        ],
    ));
    gfx.on_event(&call(
        0,
        "CGACT",
        vec![
            arg(&t, "CGACT", "ID", s("TX")),
            arg(&t, "CGACT", "TEXT", i(1)),
            arg(&t, "CGACT", "SET", i(1)),
            arg(&t, "CGACT", "SETSTR", s("A")),
            arg(&t, "CGACT", "X", i(2)),
            arg(&t, "CGACT", "Y", i(3)),
        ],
    ));
    let frame = gfx.frame();
    assert_eq!(frame.text_draws.len(), 1);
    let draw = &frame.text_draws[0];
    assert_eq!(draw.target.display(), ":/W0/TX");
    assert_eq!(draw.bytes, b"A");
    assert_eq!((draw.x, draw.y), (2, 3));
    assert_eq!(draw.style.size_x, 12);
    assert_eq!(draw.style.color, 0x00_FF00);
    // REA FUN_004056b8:SET=1 → 3×3 邻域 9 次合成(偏移,非同位置叠加)
    assert_eq!(draw.composite_passes(), 9);
    let (offsets, n) = draw.composite_offsets();
    assert_eq!(n, 9);
    assert!(offsets[..9].contains(&(-1, -1)));
    assert!(offsets[..9].contains(&(1, 1)));

    // 跨帧持久(DIV-GFX-058:官方写层表面,画一次即常驻)。
    gfx.on_event(&frame_start(1));
    assert_eq!(gfx.frame().text_draws.len(), 1);

    // 同层同参重发 = 精确去重(对话逐字路径每帧重发,可见结果不变)。
    gfx.on_event(&call(
        0,
        "CGACT",
        vec![
            arg(&t, "CGACT", "ID", s("TX")),
            arg(&t, "CGACT", "TEXT", i(1)),
            arg(&t, "CGACT", "SET", i(1)),
            arg(&t, "CGACT", "SETSTR", s("A")),
            arg(&t, "CGACT", "X", i(2)),
            arg(&t, "CGACT", "Y", i(3)),
        ],
    ));
    assert_eq!(gfx.frame().text_draws.len(), 1, "重复命令去重");

    // 层删除清除(CGEND)。
    gfx.on_event(&call(0, "CGEND", vec![arg(&t, "CGEND", "ID", s("TX"))]));
    assert!(gfx.frame().text_draws.is_empty());
}

#[test]
fn cgact_text_does_not_enter_image_ops() {
    let mut gfx = state();
    let t = table();
    gfx.on_event(&frame_start(0));
    gfx.on_event(&call(
        0,
        "CG",
        vec![arg(&t, "CG", "ID", s("TX")), arg(&t, "CG", "FILE", s(""))],
    ));
    gfx.on_event(&call(
        0,
        "CGACT",
        vec![
            arg(&t, "CGACT", "ID", s("TX")),
            arg(&t, "CGACT", "TEXT", i(1)),
            arg(&t, "CGACT", "SETSTR", s("A")),
        ],
    ));
    let frame = gfx.frame();
    let image_ops = frame
        .passes
        .iter()
        .flat_map(|p| &p.ops)
        .filter(|op| matches!(op, Op::Image(_)))
        .count();
    assert_eq!(image_ops, 0, "TEXT 不是像素加工 op,由 text_draws 承载");
}

#[test]
fn cgact_text_missing_layer_is_reported() {
    let mut gfx = state();
    let t = table();
    gfx.on_event(&frame_start(0));
    gfx.on_event(&call(
        0,
        "CGACT",
        vec![
            arg(&t, "CGACT", "ID", s("NOPE")),
            arg(&t, "CGACT", "TEXT", i(1)),
            arg(&t, "CGACT", "SETSTR", s("A")),
        ],
    ));
    assert!(gfx.frame().text_draws.is_empty());
    assert!(gfx
        .diagnostics()
        .iter()
        .any(|d| d.code == "gfx.layer.missing"));
}

/// `SETSTR` 空串/缺省 = 官方空串短路(不绘制),**不是**参数缺失。
///
/// 证据:手册 `cg/cgact.html`「TEXT=レイヤに文字列を表示する」;引擎
/// `FUN_004056b8` 文本合成对空串直接返回。ERIS `es_Text` 逐字推进
/// (`\BT.FONT.DRAW(...,$txone)`)在空字/无名字时每行都会走到这里
/// (0.486/0.494 商业包实测),按 Recoverable 报会在演出期刷屏。
#[test]
fn cgact_text_empty_setstr_is_a_verbose_noop() {
    let mut gfx = state();
    let t = table();
    gfx.on_event(&frame_start(0));
    gfx.on_event(&call(
        0,
        "CG",
        vec![
            arg(&t, "CG", "ID", s("TX")),
            arg(&t, "CG", "FILE", s("")),
        ],
    ));
    for (pc, setstr) in [(1u64, Some("")), (2, None)] {
        let mut args = vec![
            arg(&t, "CGACT", "ID", s("TX")),
            arg(&t, "CGACT", "TEXT", i(1)),
        ];
        if let Some(v) = setstr {
            args.push(arg(&t, "CGACT", "SETSTR", s(v)));
        }
        gfx.on_event(&call(pc, "CGACT", args));
    }
    assert!(
        gfx.frame().text_draws.is_empty(),
        "空串/缺省不得产出绘制项"
    );
    let diags = gfx.diagnostics();
    assert!(
        diags.iter().all(|d| d.severity != "Recoverable"),
        "空串是正常路径,不得报 Recoverable: {diags:?}"
    );
    let empty_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.code == "gfx.text.empty")
        .collect();
    assert_eq!(empty_diags.len(), 2, "空串与缺省各自一条 Verbose: {diags:?}");
    assert!(
        empty_diags.iter().all(|d| d.severity == "Verbose"),
        "空串只留痕,不到 Recoverable: {empty_diags:?}"
    );
}

/// 对话影子的真实形态(商业包 `s30 pc32..59`):脚本用 `FONT[BNO=0/1]` 配两套
/// 样式(影/正文),再各发一条 `CGACT[TEXT SET=n]`。
///
/// 关键点:两条绘制项必须带**不同颜色**且各自带 SET —— 若 `BNO` 被当成字体
/// 序号(旧实现),两套样式会合并成一个,两条绘制项同色 ⇒ 影子消失。
#[test]
fn two_style_slots_produce_shadow_and_body_draws() {
    let mut gfx = state();
    let t = table();
    gfx.on_event(&frame_start(0));
    gfx.on_event(&call(
        0,
        "CG",
        vec![
            arg(&t, "CG", "ID", s("TX")),
            arg(&t, "CG", "FILE", s("")),
        ],
    ));
    // 槽 0 = 影子(黑),槽 1 = 正文(青)。
    gfx.on_event(&call(
        0,
        "FONT",
        vec![
            arg(&t, "FONT", "BNO", i(0)),
            arg(&t, "FONT", "COLOR", i(0x00_0000)),
            arg(&t, "FONT", "SX", i(24)),
            arg(&t, "FONT", "SY", i(24)),
        ],
    ));
    gfx.on_event(&call(
        0,
        "FONT",
        vec![
            arg(&t, "FONT", "BNO", i(1)),
            arg(&t, "FONT", "COLOR", i(0x00_FFFF)),
            arg(&t, "FONT", "SX", i(24)),
            arg(&t, "FONT", "SY", i(24)),
        ],
    ));
    // 影子:9 遍 3×3(SET=3);正文:1 遍。
    for (bno, set) in [(0, 3), (1, 0)] {
        gfx.on_event(&call(
            0,
            "CGACT",
            vec![
                arg(&t, "CGACT", "ID", s("TX")),
                arg(&t, "CGACT", "TEXT", i(1)),
                arg(&t, "CGACT", "BNO", i(bno)),
                arg(&t, "CGACT", "SET", i(set)),
                arg(&t, "CGACT", "SETSTR", s("A")),
            ],
        ));
    }
    let frame = gfx.frame();
    assert_eq!(frame.text_draws.len(), 2, "影 + 正文两条绘制项");
    let shadow = &frame.text_draws[0];
    let body = &frame.text_draws[1];
    assert_eq!(shadow.style.color, 0x00_0000, "影子用槽 0 的颜色");
    assert_eq!(body.style.color, 0x00_FFFF, "正文用槽 1 的颜色");
    assert_eq!(shadow.composite_passes(), 9, "SET=3 → 9 遍");
    assert_eq!(body.composite_passes(), 1, "SET=0 → 1 遍");
}

#[test]
fn gaiji_chain_lowers_to_text_image_glyph() {
    // DIV-GFX-076 方案 E:`CG[图]→SIZE2→MEIDO→MERGE2 到文本层` 必须降级成
    // 文本图像字形,而不是层表面 MERGE2(后者会清掉前缀字)。
    let mut gfx = state();
    let t = table();
    gfx.on_event(&frame_start(0));
    for (id, file) in [("TX", "dark"), ("IMG.SRC", "glyph")] {
        gfx.on_event(&call(
            0,
            "CG",
            vec![arg(&t, "CG", "ID", s(id)), arg(&t, "CG", "FILE", s(file))],
        ));
    }
    gfx.on_event(&call(
        0,
        "CG",
        vec![arg(&t, "CG", "ID", s("IMG.DST")), arg(&t, "CG", "FILE", s(""))],
    ));
    gfx.on_event(&call(
        0,
        "CGACT",
        vec![
            arg(&t, "CGACT", "ID", s("IMG.SRC")),
            arg(&t, "CGACT", "ID2", s("IMG.DST")),
            arg(&t, "CGACT", "SIZE2", i(1)),
            arg(&t, "CGACT", "SX", i(22)),
            arg(&t, "CGACT", "SY", i(22)),
        ],
    ));
    gfx.on_event(&call(
        0,
        "CGACT",
        vec![
            arg(&t, "CGACT", "ID", s("IMG.DST")),
            arg(&t, "CGACT", "MEIDO", i(1)),
            arg(&t, "CGACT", "SET", i(0x00_FF0000)),
        ],
    ));
    gfx.on_event(&call(
        0,
        "CGACT",
        vec![
            arg(&t, "CGACT", "ID", s("IMG.DST")),
            arg(&t, "CGACT", "ID2", s("TX")),
            arg(&t, "CGACT", "MERGE2", i(1)),
            arg(&t, "CGACT", "X", i(40)),
            arg(&t, "CGACT", "Y", i(10)),
        ],
    ));

    let frame = gfx.frame();
    let image = frame
        .text_draws
        .iter()
        .find(|d| d.image.is_some())
        .expect("外字链应降级为图像字形");
    assert_eq!(image.target.display(), ":/W0/TX");
    assert_eq!((image.x, image.y), (40, 10));
    let img = image.image.as_ref().unwrap();
    assert_eq!(img.asset, b"glyph");
    assert_eq!((img.width, img.height), (22, 22));
    assert_eq!(img.tint, Some(0x00_FF0000));
    assert!(
        !frame.passes.iter().any(|p| p.ops.iter().any(|op| matches!(
            op,
            Op::Image(i) if i.action == yu_gfx::ImageAction::Merge2
        ))),
        "降级后不得再记录 MERGE2 图像加工"
    );
}

#[test]
fn dir_shaped_file_creates_blank_layer_and_keeps_text() {
    // DIV-GFX-084:脚本运行时会把 `FILE` 拼成**目录形键**(以分隔符结尾),
    // 例如 ERIS 本篇正文层 `ES.GAMEMAIN.TIP.MESW.TX` 实际拿到
    // `cgsys/../system/window/`。这种键不可能解码成图片,但官方把
    // "`FILE` 装载失败"当作"该层没有图片":层对象与 `SX/SY` 表面照旧
    // 存在,`CGACT[TEXT]` 依旧写进这块面。
    //
    // 旧实现把该键原样留成 `Asset`,后端 `layer_base` 解析失败 ⇒ 整层
    // 文本被丢弃(实机表现为对话正文与按钮文字全灭,换字体无效)。
    let mut gfx = state();
    let t = table();
    gfx.on_event(&frame_start(0));
    gfx.on_event(&call(
        0,
        "CG",
        vec![
            arg(&t, "CG", "ID", s("MESW.TX")),
            arg(
                &t,
                "CG",
                "FILE",
                yu_rt::value::Value::Str(b"cgsys/../system/window/".to_vec()),
            ),
            arg(&t, "CG", "SX", i(771)),
            arg(&t, "CG", "SY", i(95)),
        ],
    ));

    let id = yu_gfx::path::normalize(b"MESW.TX", None, false).path;
    let layer = gfx.layer(&id).expect("目录形 FILE 仍必须建层");
    assert!(
        matches!(layer.texture, yu_gfx::TextureRef::SolidBlack1x1),
        "目录形 FILE 应退化为「有 FILE、无图片」的空白层(与 CG[FILE=\"\"] 同族),实际 {:?}",
        layer.texture
    );
    assert_eq!(
        layer.image_size().map(|s| (s.width, s.height)),
        Some((771, 95)),
        "SX/SY 必须保留为表面尺寸,否则文本无处可写"
    );
    // **画布必须透明**:这一层是 `CGACT[TEXT]` 的写字面,不是黑幕。
    // 与 `CG[FILE=""]`(不透明黑,当黑幕/遮挡板)靠 `fill` 区分;
    // 若这里退化成不透明黑,正文下方会压出一块实色(DIV-GFX-084)。
    assert_eq!(
        layer.fill,
        Some([0, 0, 0, 0]),
        "目录形 FILE 的空白画布必须全透明,不能是不透明黑"
    );

    // 文本绘制项必须能登记到该层(后端据此合成)。
    gfx.on_event(&call(
        0,
        "CGACT",
        vec![
            arg(&t, "CGACT", "ID", s("MESW.TX")),
            arg(&t, "CGACT", "TEXT", i(1)),
            arg(&t, "CGACT", "SET", i(0)),
            arg(&t, "CGACT", "SETSTR", s("あ")),
            arg(&t, "CGACT", "X", i(4)),
            arg(&t, "CGACT", "Y", i(4)),
        ],
    ));
    let frame = gfx.frame();
    let draw = frame
        .text_draws
        .iter()
        .find(|d| d.target == id)
        .expect("CGACT[TEXT] 必须落到目录形 FILE 的层上");
    // `SETSTR` 按原样字节保留(不断言具体编码字面量,避免把测试绑死在
    // 某一种源码编码上)。
    assert!(!draw.bytes.is_empty(), "SETSTR 字节必须保留");
}

#[test]
fn normal_file_key_is_not_degraded() {
    // 反向守卫:非目录形的键必须原样保留为 `Asset`,不能被上面的退化
    // 逻辑吞掉(否则所有图片层都会变成空白)。
    let mut gfx = state();
    let t = table();
    gfx.on_event(&frame_start(0));
    gfx.on_event(&call(
        0,
        "CG",
        vec![
            arg(&t, "CG", "ID", s("BG")),
            arg(&t, "CG", "FILE", s("cg/bg/black")),
        ],
    ));
    let id = yu_gfx::path::normalize(b"BG", None, false).path;
    let layer = gfx.layer(&id).expect("层存在");
    assert!(
        matches!(&layer.texture, yu_gfx::TextureRef::Asset(k) if k == b"cg/bg/black"),
        "普通资产键不得退化,实际 {:?}",
        layer.texture
    );
}

/// 官方 `'P'` 是 zero_fill 整个层表面:同层的 `CGACT[TEXT]` 像素也必须清。
#[test]
fn page_clear_also_drops_cgact_text_on_same_layer() {
    let mut gfx = state();
    let t = table();
    gfx.on_event(&frame_start(0));
    cg_text_layer(&mut gfx, &t, "MSG", 64, 32);
    gfx.on_event(&call(
        0,
        "CGACT",
        vec![
            arg(&t, "CGACT", "ID", s("MSG")),
            arg(&t, "CGACT", "TEXT", i(1)),
            arg(&t, "CGACT", "SETSTR", s("A")),
        ],
    ));
    assert_eq!(gfx.frame().text_draws.len(), 1);
    gfx.apply_text_frame(&text_frame(
        b"MSG",
        0,
        0,
        vec![glyph(b"B", 0, 0, false)],
    ));
    assert_eq!(gfx.frame().text_draws.len(), 1);
    gfx.apply_text_frame(&text_frame(b"MSG", 1, 0, vec![TextStep::PageClear]));
    let frame = gfx.frame();
    assert!(frame.text_layout.is_none());
    assert!(
        frame.text_draws.is_empty(),
        "'P' 必须清同层 CGACT 文本(官方 zero_fill 整个层表面)"
    );
}
