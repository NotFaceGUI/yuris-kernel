//! P2 文本链路:跨帧持久 / `_TEXT_FLAG` 排版 / FONT 字段 / `CGACT[TEXT]`。
//!
//! 证据见 `crates/yu-gfx/src/text.rs` 与 `docs/divergences/gfx-text.md`。

mod support;

use support::{arg, call, frame_start, i, s, state, table};
use yu_gfx::Op;
use yu_rt::text::TextSnapshot;

fn snapshot(bytes: &[u8], flags: &[u8]) -> TextSnapshot {
    TextSnapshot {
        layer_id: Vec::new(),
        bytes: bytes.to_vec(),
        flags: flags.to_vec(),
        output_mode: 1,
        wait_ms: 0,
        autoreturn: 1,
        autopage: 1,
        autoclick: 1,
        click: false,
        clear_pending: false,
        clear_seq: 0,
        reveal_flags: 0,
        reveal_bytes: 0,
        reveal_chars: 0,
        skip_mode: false,
        cursor_x: 0,
        cursor_y: 0,
        click_buttons: [1, 13, 0x3ff, 0x3ff],
        skip_buttons: [17, 0x3ff, 0x3ff, 0x3ff],
    }
}

#[test]
fn text_buffer_persists_across_frames() {
    let mut gfx = state();
    gfx.on_event(&frame_start(0));
    gfx.on_event(&yu_rt::VmEvent::Text {
        frame: 0,
        id: Vec::new(),
        bytes: b"A".to_vec(),
    });
    assert_eq!(gfx.frame().text.len(), 1);

    // DIV-GFX-034:下一帧不再清空。
    gfx.on_event(&frame_start(1));
    let frame = gfx.frame();
    assert_eq!(frame.text.len(), 1);
    assert_eq!(frame.text[0].bytes, b"A");
    // 事件路径没有 `_TEXT_FLAG` ⇒ 不产出排版(禁止猜宽度)。
    assert!(frame.text[0].flags.is_none());
    assert!(frame.text_layout.is_none());
}

#[test]
fn synced_flags_drive_text_flag_layout() {
    let mut gfx = state();
    gfx.on_event(&frame_start(0));
    // "AこB":A 半角,こ 全角,B 半角;SX=SY=20(引擎默认)。
    gfx.sync_text_state(&snapshot(b"A\x82\xB1B", b"121"));
    let frame = gfx.frame();
    assert_eq!(frame.text.len(), 1);
    assert_eq!(frame.text[0].flags.as_deref(), Some(&b"121"[..]));
    let layout = frame.text_layout.as_ref().expect("layout from _TEXT_FLAG");
    assert_eq!(layout.chars, 3);
    assert_eq!(layout.glyphs.len(), 3);
    assert_eq!((layout.glyphs[0].x, layout.glyphs[0].y), (0, 0));
    assert_eq!(layout.glyphs[1].x, 10, "半角 = SX/2");
    assert_eq!(layout.glyphs[2].x, 30, "全角 = SX");
    assert!(layout.glyphs[1].full);
}

#[test]
fn reveal_is_the_only_render_truncation_input() {
    let mut gfx = state();
    gfx.on_event(&frame_start(0));
    gfx.sync_text_state(&snapshot(b"A\x82\xB1B", b"121"));
    gfx.set_text_reveal(Some(2));
    let layout = gfx.frame().text_layout.expect("layout");
    assert_eq!(layout.reveal, Some(2), "后端按 reveal 截断,不自建计时器");
    gfx.set_text_reveal(None);
    assert_eq!(gfx.frame().text_layout.expect("layout").reveal, None);
}

#[test]
fn clear_sync_clears_cross_frame_text() {
    let mut gfx = state();
    gfx.on_event(&frame_start(0));
    gfx.sync_text_state(&snapshot(b"A", b"2"));
    assert_eq!(gfx.frame().text.len(), 1);

    // 空 bytes **不是** CLEAR:官方 CLEAR 是显式状态迁移(REA 0x453894 →
    // 0x4638e4),而 `FUN_004638e4` 不触碰图层表面。没有 clear_seq 变化时
    // 显示层保留(任务书 `task-text-auto-pipeline.md` C1)。
    gfx.on_event(&frame_start(1));
    gfx.sync_text_state(&snapshot(b"", b""));
    assert_eq!(gfx.frame().text.len(), 1, "空缓冲不隐式清层");

    // `TEXT[CLEAR]` 经 `TextSnapshot::clear_seq` 显式传播。
    let mut cleared = snapshot(b"", b"");
    cleared.clear_seq = 1;
    gfx.sync_text_state(&cleared);
    let frame = gfx.frame();
    assert!(frame.text.is_empty());
    assert!(frame.text_layout.is_none());
}

#[test]
fn non_ascii_text_layer_id_targets_exact_layer() {
    // 合成样本(ERIS 141 脚本里 TEXT[ID] = 0 次)。
    // 关键:层 ID 是**不透明字节**,不是 CP932/UTF-8 字符串。
    // - 0x82 0xA0 = CP932「あ」(真实语料编码样本);
    // - 0xFF 0xFE 不是合法 CP932 前缀,必须原样透传(证明没有解码)。
    // REA 0x4141e4 先做路径规范化,0x414559 再按字节哈希+比较查层;
    // gfx 的 LayerId 也是字节,故目标层必须精确命中。
    for raw in [b"\x82\xa0".as_slice(), b"\xff\xfe".as_slice()] {
        let mut gfx = state();
        let t = table();
        gfx.on_event(&frame_start(0));
        gfx.on_event(&call(
            0,
            "CG",
            vec![
                arg(&t, "CG", "ID", yu_rt::value::Value::Str(raw.to_vec())),
                arg(&t, "CG", "FILE", yu_rt::value::Value::Str(Vec::new())),
            ],
        ));
        let mut snap = snapshot(b"A", b"1");
        snap.layer_id = raw.to_vec();
        gfx.sync_text_state(&snap);
        let layout = gfx.frame().text_layout.expect("layout");
        assert_eq!(
            layout.target,
            Some(yu_gfx::path::normalize(raw, None, false).path),
            "层 ID 必须按字节规范化后精确命中"
        );
    }
}

#[test]
fn text_layout_tracks_current_layer_and_keeps_reveal() {
    // 引擎只有一个当前文本层(`[state+0x00]`);旧 `text_layout_now` 用
    // BTreeMap 的 `find_map` 取第一个非空层,层 ID 切换后会画错层。
    // 本用例让 A/B 都非空,B 后同步 ⇒ 必须选 B(排序上 A 在前)。
    let mut gfx = state();
    let t = table();
    gfx.on_event(&frame_start(0));
    for id in [b"A".as_slice(), b"B".as_slice()] {
        gfx.on_event(&call(
            0,
            "CG",
            vec![
                arg(&t, "CG", "ID", yu_rt::value::Value::Str(id.to_vec())),
                arg(&t, "CG", "FILE", yu_rt::value::Value::Str(Vec::new())),
            ],
        ));
    }
    let mut a = snapshot(b"A", b"1");
    a.layer_id = b"A".to_vec();
    gfx.sync_text_state(&a);
    assert_eq!(
        gfx.frame().text_layout.expect("A").target,
        Some(yu_gfx::path::normalize(b"A", None, false).path)
    );

    let mut b = snapshot(b"B", b"1");
    b.layer_id = b"B".to_vec();
    gfx.sync_text_state(&b);
    gfx.set_text_reveal(Some(1));
    let layout = gfx.frame().text_layout.expect("B");
    assert_eq!(
        layout.target,
        Some(yu_gfx::path::normalize(b"B", None, false).path),
        "只排当前层,不取 map 里的第一个非空层"
    );
    assert_eq!(layout.reveal, Some(1), "reveal 由宿主显式设置");

    // 同一层再同步(clear_seq 未变)不得重置 reveal。
    gfx.sync_text_state(&b);
    assert_eq!(gfx.frame().text_layout.expect("B2").reveal, Some(1));

    // CLEAR 代数变化才清,并重置 reveal。
    let mut cleared = snapshot(b"", b"");
    cleared.layer_id = b"B".to_vec();
    cleared.clear_seq = 1;
    gfx.sync_text_state(&cleared);
    assert!(gfx.frame().text_layout.is_none());
}

#[test]
fn text_flag_mismatch_is_not_layouted() {
    let mut gfx = state();
    gfx.on_event(&frame_start(0));
    // 全角标志配半角字节:对齐失败,不猜。
    gfx.sync_text_state(&snapshot(b"A", b"2"));
    let frame = gfx.frame();
    assert!(frame.text_layout.is_none());
    assert!(gfx
        .diagnostics()
        .iter()
        .any(|d| d.code == "gfx.text.unsupported"));
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
