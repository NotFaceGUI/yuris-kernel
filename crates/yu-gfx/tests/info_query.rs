//! `CGINFO/FONTINFO/WINDOWINFO` 同步查询(供 VM 的 `LET` 写回)。
//!
//! 只测手册明确且状态足以回答的字段;需要资产元数据/字体列表/鼠标/像素的
//! 字段必须返回 Unimplemented,不能猜 0。

mod support;

use support::*;
use yu_rt::value::Value;

fn q(gfx: &yu_gfx::GfxState, command: &str, args: Vec<yu_rt::event::ArgValue>) -> Value {
    gfx.query_info(command, &args).expect("query")
}

fn q_err(gfx: &yu_gfx::GfxState, command: &str, args: Vec<yu_rt::event::ArgValue>) -> yu_log::Diag {
    gfx.query_info(command, &args)
        .expect_err("must be unimplemented")
}

#[test]
fn cginfo_answers_documented_attributes() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("L")),
                arg(&table, "CG", "FILE", s("l")),
                arg(&table, "CG", "A", i(128)),
                arg(&table, "CG", "X", i(10)),
                arg(&table, "CG", "Y", i(20)),
                arg(&table, "CG", "Z", i(7)),
                arg(&table, "CG", "MODE", i(2)),
                arg(&table, "CG", "E", i(0)),
                arg(&table, "CG", "T", i(100)),
                arg(&table, "CG", "TA", i(64)),
                arg(&table, "CG", "TID", s("SRC")),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("SRC")),
                arg(&table, "CG", "FILE", s("src")),
            ],
        ),
        // 仿射字段:FX/FY 是 X/Y 的百分位口径;FBX/FBY/FEX/FEY/FEZ 原值。
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("L2")),
                arg(&table, "CG", "FILE", s("l2")),
                arg(&table, "CG", "FX", i(1234)),
                arg(&table, "CG", "FY", i(5678)),
                arg(&table, "CG", "FBX", i(5000)),
                arg(&table, "CG", "FBY", i(6000)),
                arg(&table, "CG", "FEX", i(100)),
                arg(&table, "CG", "FEY", i(200)),
                arg(&table, "CG", "FEZ", i(300)),
            ],
        ),
        frame_end(1),
    ]);
    let info = |flag: &str| {
        q(
            &gfx,
            "CGINFO",
            vec![
                arg(&table, "CGINFO", "ID", s("L")),
                arg(&table, "CGINFO", flag, i(1)),
            ],
        )
    };
    assert_eq!(info("EXIST"), Value::Int(1));
    assert_eq!(info("E"), Value::Int(0));
    assert_eq!(info("A"), Value::Int(128));
    assert_eq!(info("X"), Value::Int(10));
    assert_eq!(info("Y"), Value::Int(20));
    assert_eq!(info("Z"), Value::Int(7));
    assert_eq!(info("MODE"), Value::Int(2));
    assert_eq!(info("T"), Value::Int(100));
    assert_eq!(info("TA"), Value::Int(64));
    // CGINFO[TID] 返回脚本原样字符串(引擎 +0x4c 保存原文,不是规范化路径)。
    assert_eq!(info("TID"), Value::Str(b"SRC".to_vec()));
    let info2 = |flag: &str| {
        q(
            &gfx,
            "CGINFO",
            vec![
                arg(&table, "CGINFO", "ID", s("L2")),
                arg(&table, "CGINFO", flag, i(1)),
            ],
        )
    };
    assert_eq!(info2("FX"), Value::Int(1234), "FX = X*100");
    assert_eq!(info2("FY"), Value::Int(5678), "FY = Y*100");
    assert_eq!(info2("FBX"), Value::Int(5000));
    assert_eq!(info2("FBY"), Value::Int(6000));
    assert_eq!(info2("FEX"), Value::Int(100));
    assert_eq!(info2("FEY"), Value::Int(200));
    assert_eq!(info2("FEZ"), Value::Int(300));
    // 资源图尺寸需要资产元数据:必须显式 Unimplemented。
    let err = q_err(
        &gfx,
        "CGINFO",
        vec![
            arg(&table, "CGINFO", "ID", s("L")),
            arg(&table, "CGINFO", "SX", i(1)),
        ],
    );
    assert_eq!(err.code, "gfx.info.asset_size");
}

#[test]
fn cginfo_group_and_solid_black_sizes_are_known() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("G/")),
                arg(&table, "CG", "SX", i(400)),
                arg(&table, "CG", "SY", i(300)),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("BLACK")),
                arg(&table, "CG", "FILE", s("")),
                arg(&table, "CG", "SX", i(800)),
                arg(&table, "CG", "SY", i(600)),
            ],
        ),
        frame_end(1),
    ]);
    let size = |id: &str, flag: &str| {
        q(
            &gfx,
            "CGINFO",
            vec![
                arg(&table, "CGINFO", "ID", s(id)),
                arg(&table, "CGINFO", flag, i(1)),
            ],
        )
    };
    assert_eq!(size("G/", "SX"), Value::Int(400));
    assert_eq!(size("G/", "SY"), Value::Int(300));
    assert_eq!(size("BLACK", "SX"), Value::Int(800));
    assert_eq!(size("BLACK", "SY"), Value::Int(600));
}

#[test]
fn cginfo_missing_layer_only_answers_exist() {
    let table = table();
    let gfx = state();
    assert_eq!(
        q(
            &gfx,
            "CGINFO",
            vec![
                arg(&table, "CGINFO", "ID", s("NOPE")),
                arg(&table, "CGINFO", "EXIST", i(1)),
            ],
        ),
        Value::Int(0)
    );
    let err = q_err(
        &gfx,
        "CGINFO",
        vec![
            arg(&table, "CGINFO", "ID", s("NOPE")),
            arg(&table, "CGINFO", "A", i(1)),
        ],
    );
    assert_eq!(err.code, "gfx.info.missing");
}

/// 官方 CGINFO 是顺序 if/else 链(`0x43d758..0x43dddf`):多字段同时置位
/// 时按固定优先级回答第一个,不是报错。
#[test]
fn cginfo_multiple_fields_use_official_priority() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("X")),
                arg(&table, "CG", "FILE", s("x")),
                arg(&table, "CG", "A", i(123)),
                arg(&table, "CG", "E", i(1)),
            ],
        ),
        frame_end(1),
    ]);
    assert_eq!(
        q(
            &gfx,
            "CGINFO",
            vec![
                arg(&table, "CGINFO", "ID", s("X")),
                arg(&table, "CGINFO", "A", i(1)),
                arg(&table, "CGINFO", "E", i(1)),
            ],
        ),
        Value::Int(1),
        "E 在 A 之前(官方链顺序)"
    );
    assert_eq!(
        q(
            &gfx,
            "CGINFO",
            vec![
                arg(&table, "CGINFO", "ID", s("X")),
                arg(&table, "CGINFO", "A", i(1)),
            ],
        ),
        Value::Int(123)
    );
}

/// `CGINFO[COLOR]` = **指定坐标**的像素色,打包与 `ONMOUSE` 相同
/// (`r | g<<8 | b<<16`),坐标取 `SET/SET2`(缺省 0,0),越界 `-1`。
///
/// 语料依据(`es_button.yst` s0 pc2020-2026):`CG[ID]` 建层 →
/// `CGINFO[SX]/[SY]/[COLOR SET=0 SET2=0]` → `IF @SX==1 && @SY==1 &&
/// @color == 0x00808080` → `CGACT[RECTPAINT3]`:1×1 的 50% 灰占位图
/// 会被重绘。引擎侧同体:像素采样器按全局 `[0x68417d]` 在鼠标坐标与
/// 显式 `SET/SET2` 之间切换,越界返回 -1。
#[test]
fn cginfo_color_samples_pixel_at_set_coordinates() {
    struct Grey1x1;
    impl yu_gfx::TextureSampler for Grey1x1 {
        fn size(&mut self, _t: &yu_gfx::TextureRef) -> Option<yu_gfx::SizeU32> {
            Some(yu_gfx::SizeU32 {
                width: 1,
                height: 1,
            })
        }
        fn sample(&mut self, _t: &yu_gfx::TextureRef, x: i64, y: i64) -> Option<[u8; 4]> {
            if x == 0 && y == 0 {
                Some([128, 128, 128, 255])
            } else {
                None
            }
        }
    }

    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("BT")),
                arg(&table, "CG", "FILE", s("bt")),
            ],
        ),
        frame_end(1),
    ]);
    let color = |set: i64, set2: i64| {
        let mut sampler = Grey1x1;
        gfx.query_info_with(
            "CGINFO",
            &[
                arg(&table, "CGINFO", "ID", s("BT")),
                arg(&table, "CGINFO", "COLOR", i(1)),
                arg(&table, "CGINFO", "SET", i(set)),
                arg(&table, "CGINFO", "SET2", i(set2)),
            ],
            Some(&mut sampler),
        )
        .expect("CGINFO[COLOR]")
    };
    assert_eq!(
        color(0, 0),
        Value::Int(0x0080_8080),
        "1×1 灰 = r|g<<8|b<<16"
    );
    assert_eq!(color(5, 5), Value::Int(-1), "越界返回 -1");
}

#[test]
fn fontinfo_answers_current_style_fields() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "FONT",
            vec![
                arg(&table, "FONT", "NAME", s("TestFont")),
                arg(&table, "FONT", "COLOR", i(0x00ff_ff00)),
                arg(&table, "FONT", "COLOR2", i(0x0000_00ff)),
                arg(&table, "FONT", "SX", i(22)),
                arg(&table, "FONT", "SY", i(24)),
                arg(&table, "FONT", "BOLD", i(1)),
                arg(&table, "FONT", "ANTIALIASING", i(0)),
                arg(&table, "FONT", "DRAWMODE", i(1)),
            ],
        ),
        frame_end(1),
    ]);
    let info = |flag: &str| q(&gfx, "FONTINFO", vec![arg(&table, "FONTINFO", flag, i(1))]);
    assert_eq!(info("COLOR"), Value::Int(0x00ff_ff00));
    assert_eq!(info("COLOR2"), Value::Int(0x0000_00ff));
    assert_eq!(info("SX"), Value::Int(22));
    assert_eq!(info("SY"), Value::Int(24));
    assert_eq!(info("BOLD"), Value::Int(1));
    assert_eq!(info("ANTIALIASING"), Value::Int(0));
    assert_eq!(info("DRAWMODE"), Value::Int(1));
    assert_eq!(info("NAME"), Value::Str(b"TestFont".to_vec()));
    // `PITCH` 官方处理体恒写 0(REA `0x444e52` xor eax/edx)。
    assert_eq!(info("PITCH"), Value::Int(0));

    // `NAME NO=n` 走字体清单(1 基);清单未注入/越界 = 空串。引擎按
    // EnumFontFamiliesExA 枚举失败即空,不报错 —— 见 `query_fontinfo` 的
    // `NAME` 分支注释。
    let listed = q(
        &gfx,
        "FONTINFO",
        vec![
            arg(&table, "FONTINFO", "NAME", i(1)),
            arg(&table, "FONTINFO", "NO", i(3)),
        ],
    );
    assert_eq!(listed, Value::Str(Vec::new()));
}

#[test]
fn fontinfo_unset_fields_are_not_fabricated() {
    let table = table();
    let gfx = state();
    // 引擎默认已取证(FUN_00464fac):COLOR2=0xffffff、GRADATION=1、ROTATION=0。
    for (flag, expected) in [("COLOR2", 0xff_ffff), ("GRADATION", 1), ("ROTATION", 0)] {
        assert_eq!(
            q(&gfx, "FONTINFO", vec![arg(&table, "FONTINFO", flag, i(1))]),
            Value::Int(expected),
            "{flag}"
        );
    }
    // `NUM`/`LANG` 在清单未注入时显式应答 0(`query_fontinfo` 注释:脚本
    // `es_Font.yst:166` 以 LANG 非 0 过滤,清单空 ⇒ 走「无字体可选」路径,
    // 与引擎在无字体环境的行为一致)。`PITCH` 官方处理体恒 0(REA
    // `0x444e52`);`NAME`(需系统字体列表)未设置时仍未取证,必须报错。
    for flag in ["NUM", "LANG", "PITCH"] {
        assert_eq!(
            q(&gfx, "FONTINFO", vec![arg(&table, "FONTINFO", flag, i(1))]),
            Value::Int(0),
            "{flag}"
        );
    }
    for flag in ["NAME"] {
        let err = q_err(&gfx, "FONTINFO", vec![arg(&table, "FONTINFO", flag, i(1))]);
        assert_eq!(err.code, "gfx.info.unsupported", "{flag}");
    }
}

#[test]
fn fontinfo_lists_catalog_and_rewrites_engine_default_name() {
    let table = table();
    let mut gfx = state();
    gfx.set_font_families(vec![b"HostFont".to_vec(), b"SystemFont".to_vec()]);
    // 引擎默认 family 名单由宿主按 CP932 注入;这里用 ASCII 等价名测试
    // 「请求引擎默认名 ⇒ 回报宿主默认名」的分支。
    gfx.set_font_selection_meta(Some(b"HostFont".to_vec()), vec![b"MS Gothic".to_vec()]);
    let info = |gfx: &yu_gfx::GfxState, flag: &str, no: Option<i64>| {
        let mut args = vec![arg(&table, "FONTINFO", flag, i(1))];
        if let Some(no) = no {
            args.push(arg(&table, "FONTINFO", "NO", i(no)));
        }
        q(gfx, "FONTINFO", args)
    };
    assert_eq!(info(&gfx, "NUM", None), Value::Int(2));
    assert_eq!(info(&gfx, "LANG", Some(1)), Value::Int(0));
    assert_eq!(
        info(&gfx, "NAME", Some(1)),
        Value::Str(b"HostFont".to_vec())
    );
    assert_eq!(
        info(&gfx, "NAME", Some(2)),
        Value::Str(b"SystemFont".to_vec())
    );
    assert_eq!(info(&gfx, "NAME", Some(3)), Value::Str(Vec::new()));

    let set_name = |gfx: &mut yu_gfx::GfxState, name: &str| {
        gfx.consume_events(&[
            frame_start(1),
            call(1, "FONT", vec![arg(&table, "FONT", "NAME", s(name))]),
            frame_end(1),
        ]);
    };
    // 脚本请求引擎默认名:实际渲染宿主默认字体,显示名也回报它。
    set_name(&mut gfx, "MS Gothic");
    assert_eq!(info(&gfx, "NAME", None), Value::Str(b"HostFont".to_vec()));
    // 游戏内明确选择其它字体:原样回报,不被默认名覆盖。
    set_name(&mut gfx, "SystemFont");
    assert_eq!(
        info(&gfx, "NAME", None),
        Value::Str(b"SystemFont".to_vec())
    );
}

#[test]
fn windowinfo_answers_main_geometry() {
    let table = table();
    let mut gfx = state();
    let info = |gfx: &yu_gfx::GfxState, flag: &str, no: i64| {
        q(
            gfx,
            "WINDOWINFO",
            vec![
                arg(&table, "WINDOWINFO", "NO", i(no)),
                arg(&table, "WINDOWINFO", flag, i(1)),
            ],
        )
    };
    assert_eq!(info(&gfx, "EXIST", 0), Value::Int(1));
    assert_eq!(info(&gfx, "SX", 0), Value::Int(800));
    assert_eq!(info(&gfx, "SY", 0), Value::Int(600));
    assert_eq!(info(&gfx, "E", 0), Value::Int(1));

    gfx.consume_events(&[
        frame_start(1),
        window_call(
            1,
            "WINDOW",
            vec![
                arg(&table, "WINDOW", "NO", i(0)),
                arg(&table, "WINDOW", "E", i(0)),
                arg(&table, "WINDOW", "X", i(11)),
                arg(&table, "WINDOW", "Y", i(22)),
                arg(&table, "WINDOW", "SX", i(640)),
                arg(&table, "WINDOW", "SY", i(480)),
            ],
        ),
        frame_end(1),
    ]);
    assert_eq!(info(&gfx, "E", 0), Value::Int(0));
    assert_eq!(info(&gfx, "X", 0), Value::Int(11));
    assert_eq!(info(&gfx, "Y", 0), Value::Int(22));
    assert_eq!(info(&gfx, "SX", 0), Value::Int(640));
    assert_eq!(info(&gfx, "SY", 0), Value::Int(480));
    assert_eq!(info(&gfx, "EXIST", 1), Value::Int(0));

    // `CAPTION` 已按 REA 0x458b59 实现(`[wnd+0x14]`,未设置 = -1)。
    let caption = q(
        &gfx,
        "WINDOWINFO",
        vec![
            arg(&table, "WINDOWINFO", "NO", i(0)),
            arg(&table, "WINDOWINFO", "CAPTION", i(1)),
        ],
    );
    assert_eq!(caption, Value::Int(-1));
}

/// `WINDOW[MINMODE]`/`WINDOW[MAXMODE]`:工程 ysc.ybn 里与 `WINDOWINFO`
/// 的同名字段成对(WINDOW 槽 16/17、WINDOWINFO 槽 18/19),官方创建窗口时
/// 落到 `WS_MINIMIZEBOX`/`WS_MAXIMIZEBOX`。内核存值 + 读回,不挂未实现。
/// 语料用法:`es_first.yst:169` `WINDOW[NO=0 MINMODE=0]`。
#[test]
fn window_min_max_mode_round_trip_through_windowinfo() {
    let table = table();
    let mut gfx = state();
    let info = |gfx: &yu_gfx::GfxState, flag: &str| {
        q(
            gfx,
            "WINDOWINFO",
            vec![
                arg(&table, "WINDOWINFO", "NO", i(0)),
                arg(&table, "WINDOWINFO", flag, i(1)),
            ],
        )
    };
    // 未设置 = 创建样式默认(主窗口两个按钮位都在)。
    assert_eq!(info(&gfx, "MINMODE"), Value::Int(1));
    assert_eq!(info(&gfx, "MAXMODE"), Value::Int(1));

    gfx.consume_events(&[
        frame_start(1),
        window_call(
            1,
            "WINDOW",
            vec![
                arg(&table, "WINDOW", "NO", i(0)),
                arg(&table, "WINDOW", "MINMODE", i(0)),
                arg(&table, "WINDOW", "MAXMODE", i(0)),
            ],
        ),
        frame_end(1),
    ]);
    assert_eq!(info(&gfx, "MINMODE"), Value::Int(0));
    assert_eq!(info(&gfx, "MAXMODE"), Value::Int(0));
    // 归类后不得再产生“窗口参数未分类”。
    assert!(
        !gfx.diagnostics()
            .iter()
            .any(|d| d.detail.contains("窗口参数未分类")),
        "{:?}",
        gfx.diagnostics()
    );
}

/// `WINDOW[MINIMIZE]`/`WINDOW[CLOSEMODE]`/`WINDOW[ACTIVE]` —— 手册
/// `window/window.html` 有页:`MINIMIZE` 是 1=最小化/0=通常化的**切换**,
/// `CLOSEMODE` 是 0..2 的关闭按钮行为,`ACTIVE=0` 手册明示"也不能取消
/// 激活"⇒ 无副作用。三者都不得再报"窗口参数未分类"。
#[test]
fn window_minimize_close_mode_follow_manual() {
    let table = table();
    let mut gfx = state();
    let info = |gfx: &yu_gfx::GfxState, flag: &str| {
        q(
            gfx,
            "WINDOWINFO",
            vec![
                arg(&table, "WINDOWINFO", "NO", i(0)),
                arg(&table, "WINDOWINFO", flag, i(1)),
            ],
        )
    };
    assert_eq!(info(&gfx, "MINIMIZE"), Value::Int(0));
    assert_eq!(info(&gfx, "CLOSEMODE"), Value::Int(0));

    let call = |gfx: &mut yu_gfx::GfxState, params: Vec<yu_rt::event::ArgValue>| {
        let mut args = vec![arg(&table, "WINDOW", "NO", i(0))];
        args.extend(params);
        gfx.consume_events(&[frame_start(1), window_call(1, "WINDOW", args), frame_end(1)]);
    };
    call(
        &mut gfx,
        vec![
            arg(&table, "WINDOW", "MINIMIZE", i(1)),
            arg(&table, "WINDOW", "CLOSEMODE", i(2)),
            arg(&table, "WINDOW", "ACTIVE", i(0)),
        ],
    );
    assert_eq!(info(&gfx, "MINIMIZE"), Value::Int(1));
    assert_eq!(info(&gfx, "CLOSEMODE"), Value::Int(2));

    call(&mut gfx, vec![arg(&table, "WINDOW", "MINIMIZE", i(0))]);
    assert_eq!(info(&gfx, "MINIMIZE"), Value::Int(0));

    assert!(
        !gfx.diagnostics()
            .iter()
            .any(|d| d.detail.contains("窗口参数未分类")),
        "{:?}",
        gfx.diagnostics()
    );
}

/// `WINDOW[WSX/WSY]` 是 `WINDOWINFO[WSX/WSY]` 的写侧(读侧 = `GetWindowRect`,
/// REA `0x458a2a/0x458a4f`);成对给出时覆盖含边框整体尺寸。
#[test]
fn window_wsx_wsy_sets_outer_size_read_back_by_windowinfo() {
    let table = table();
    let mut gfx = state();
    let info = |gfx: &yu_gfx::GfxState, flag: &str| {
        q(
            gfx,
            "WINDOWINFO",
            vec![
                arg(&table, "WINDOWINFO", "NO", i(0)),
                arg(&table, "WINDOWINFO", flag, i(1)),
            ],
        )
    };
    // 未设置 = 客户区同值(无边框假设)。
    assert_eq!(info(&gfx, "WSX"), Value::Int(800));
    assert_eq!(info(&gfx, "WSY"), Value::Int(600));

    gfx.consume_events(&[
        frame_start(1),
        window_call(
            1,
            "WINDOW",
            vec![
                arg(&table, "WINDOW", "NO", i(0)),
                arg(&table, "WINDOW", "WSX", i(816)),
                arg(&table, "WINDOW", "WSY", i(639)),
            ],
        ),
        frame_end(1),
    ]);
    assert_eq!(info(&gfx, "WSX"), Value::Int(816));
    assert_eq!(info(&gfx, "WSY"), Value::Int(639));
    // 客户区尺寸不受影响。
    assert_eq!(info(&gfx, "SX"), Value::Int(800));
    assert_eq!(info(&gfx, "SY"), Value::Int(600));
}

/// 主窗口组 `COPY2 :/W0/` 的快照尺寸 = 窗口/后缓冲尺寸;脚本会立刻用
/// `CGINFO[SX/SY]` 做布局,缺失会让整层按 0 处理。
#[test]
fn main_group_copy_snapshot_reports_viewport_size() {
    let table = table();
    let mut gfx = state();
    let viewport = gfx.config().viewport;
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s(":/W0/")),
                arg(&table, "CGACT", "ID2", s("CGS100")),
                arg(&table, "CGACT", "COPY2", i(1)),
            ],
        ),
        frame_end(1),
    ]);
    let info = |flag: &str| {
        q(
            &gfx,
            "CGINFO",
            vec![
                arg(&table, "CGINFO", "ID", s("CGS100")),
                arg(&table, "CGINFO", flag, i(1)),
            ],
        )
    };
    assert_eq!(info("SX"), Value::Int(i64::from(viewport.width)));
    assert_eq!(info("SY"), Value::Int(i64::from(viewport.height)));
}

/// 复用 `CG[FILE=""]` 层(读档黑幕 `CGS<n>` 的实际形态)再 `COPY2`:
/// COPY 替换目标表面与矩形(官方 `FUN_0042f809` 无条件写
/// `+0x78/+0x7c = 区域尺寸`,`+0x90` 换成区域尺寸新表面),所以 COPY 前
/// 遗留的 `FILE=""` 1×1 矩形必须失效;内核快照表面保持源尺寸
/// (DIV-GFX-077 残留),绘制尺寸应回到快照自然尺寸。`CGINFO[SX/SY]`
/// 仍按复制区域回答。
///
/// 回归:菜单打开时建过 `CGS100`,读档时再次 `CG CGS100 FILE="" A=0`
/// 把绘制尺寸写成 1×1(`dc545ae`),若 COPY 不清掉它,整幅冻结屏幕会缩
/// 成 1 个像素 ⇒ 读档淡出直接黑屏(2026-11-19 <game-b> 实测)。
#[test]
fn copy2_after_reused_fileless_layer_draws_snapshot_at_natural_size() {
    let table = table();
    let mut gfx = state();
    let viewport = gfx.config().viewport;
    // 菜单打开:首次建立黑幕层。
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("CGS100")),
                arg(&table, "CG", "FILE", s("")),
                arg(&table, "CG", "A", i(0)),
            ],
        ),
        frame_end(1),
    ]);
    let f = gfx.frame();
    let first = f
        .layers
        .iter()
        .find(|l| l.id.display() == ":/W0/CGS100")
        .expect("CGS100");
    assert_eq!(first.image_size(), None, "首建空壳不写绘制尺寸");

    // 读档:复用同一层,再 COPY2 主窗口。
    gfx.consume_events(&[
        frame_start(2),
        call(
            2,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("CGS100")),
                arg(&table, "CG", "FILE", s("")),
                arg(&table, "CG", "A", i(0)),
            ],
        ),
        call(
            2,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s(":/W0/")),
                arg(&table, "CGACT", "ID2", s("CGS100")),
                arg(&table, "CGACT", "COPY2", i(1)),
            ],
        ),
        frame_end(2),
    ]);
    let f = gfx.frame();
    let layer = f
        .layers
        .iter()
        .find(|l| l.id.display() == ":/W0/CGS100")
        .expect("CGS100");
    assert!(matches!(
        layer.texture,
        yu_gfx::TextureRef::Snapshot(_)
    ));
    assert_eq!(
        layer.image_size(),
        None,
        "COPY 后绘制尺寸必须回到快照自然尺寸,不能保留 FILE=\"\" 的 1×1"
    );
    let info = |flag: &str| {
        q(
            &gfx,
            "CGINFO",
            vec![
                arg(&table, "CGINFO", "ID", s("CGS100")),
                arg(&table, "CGINFO", flag, i(1)),
            ],
        )
    };
    assert_eq!(info("SX"), Value::Int(i64::from(viewport.width)));
    assert_eq!(info("SY"), Value::Int(i64::from(viewport.height)));
}

/// `COPY2` 局部切片:目标层矩形 = **复制块**尺寸,不是源合图尺寸。
///
/// ERIS 按钮把多状态合图切成 OFF/OVER/ON/NA 四块后,立刻用
/// `CGINFO[ID=$off SX/SY]` 填 `@BT.SX/SY`(`es_Button.yst:2657`),
/// `es.BT.MOUSECHECK` 再用它做命中矩形。返回合图宽会让命中矩形覆盖四个
/// 按钮:置灰按钮被 `@BT.N` 跳过时,相邻按钮的矩形接住 hover,显示成
/// “不能点的按钮却高亮了旁边的按钮”(2026-11-11 <game-b> 实测)。
#[test]
fn partial_copy2_reports_copied_block_size() {
    let table = table();
    let mut gfx = state();
    gfx.set_asset_size(
        b"bt4",
        yu_gfx::SizeU32 {
            width: 264,
            height: 18,
        },
    );
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("BT.OFF.TMP")),
                arg(&table, "CG", "FILE", s("bt4")),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("BT.OFF.VOICE")),
                arg(&table, "CG", "FILE", s("")),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("BT.OFF.TMP")),
                arg(&table, "CGACT", "ID2", s("BT.OFF.VOICE")),
                arg(&table, "CGACT", "COPY2", i(1)),
                arg(&table, "CGACT", "X", i(66)),
                arg(&table, "CGACT", "X2", i(131)),
            ],
        ),
        frame_end(1),
    ]);
    let info = |flag: &str| {
        q(
            &gfx,
            "CGINFO",
            vec![
                arg(&table, "CGINFO", "ID", s("BT.OFF.VOICE")),
                arg(&table, "CGINFO", flag, i(1)),
            ],
        )
    };
    assert_eq!(info("SX"), Value::Int(66), "按钮宽 = 切片宽,不是合图宽 264");
    assert_eq!(info("SY"), Value::Int(18));
}

/// 0×0 源(资产装载失败时 VM 回填 0×0)+ 局部 `COPY2`:不能因区域 clamp
/// 的 `min > max` panic;层矩形保持官方 0(没有可解析的区域)。
#[test]
fn partial_copy2_zero_source_size_keeps_zero_rect() {
    let table = table();
    let mut gfx = state();
    gfx.set_asset_size(
        b"missing",
        yu_gfx::SizeU32 {
            width: 0,
            height: 0,
        },
    );
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("BT.OFF.TMP")),
                arg(&table, "CG", "FILE", s("missing")),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("BT.OFF.VOICE")),
                arg(&table, "CG", "FILE", s("")),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("BT.OFF.TMP")),
                arg(&table, "CGACT", "ID2", s("BT.OFF.VOICE")),
                arg(&table, "CGACT", "COPY2", i(1)),
                arg(&table, "CGACT", "X", i(0)),
                arg(&table, "CGACT", "X2", i(65)),
            ],
        ),
        frame_end(1),
    ]);
    let info = |flag: &str| {
        q(
            &gfx,
            "CGINFO",
            vec![
                arg(&table, "CGINFO", "ID", s("BT.OFF.VOICE")),
                arg(&table, "CGINFO", flag, i(1)),
            ],
        )
    };
    assert_eq!(info("SX"), Value::Int(0));
    assert_eq!(info("SY"), Value::Int(0));
}

/// `COPY2` 时资产头尚未注入:资产尺寸晚到后回填快照 `SX/SY`。
///
/// <0.474 样本> 实跑 `es_sprite.yst@6519/6520` 曾在 COPY 后立刻查快照尺寸;
/// 旧实现只记 `snapshot_files`、不记尺寸,查询一直报
/// 「快照纹理尺寸未记录」,脚本的 `LET` 留在 0。
#[test]
fn late_asset_size_backfills_snapshot_size() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("BUF")),
                arg(&table, "CG", "FILE", s("late")),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("OUT")),
                arg(&table, "CG", "FILE", s("")),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("BUF")),
                arg(&table, "CGACT", "ID2", s("OUT")),
                arg(&table, "CGACT", "COPY2", i(1)),
            ],
        ),
        frame_end(1),
    ]);
    // COPY 之后资产尺寸才到(VM 的 `CGINFO` 回退路径解析图像头后注入)。
    gfx.set_asset_size(
        b"late",
        yu_gfx::SizeU32 {
            width: 264,
            height: 18,
        },
    );
    let info = |flag: &str| {
        q(
            &gfx,
            "CGINFO",
            vec![
                arg(&table, "CGINFO", "ID", s("OUT")),
                arg(&table, "CGINFO", flag, i(1)),
            ],
        )
    };
    assert_eq!(info("SX"), Value::Int(264));
    assert_eq!(info("SY"), Value::Int(18));
}

/// 同上的局部切片:晚到资产尺寸也要按复制区域换算显示矩形(含端点)。
#[test]
fn late_asset_size_backfills_partial_snapshot_block() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("BUF")),
                arg(&table, "CG", "FILE", s("late")),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("OUT")),
                arg(&table, "CG", "FILE", s("")),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("BUF")),
                arg(&table, "CGACT", "ID2", s("OUT")),
                arg(&table, "CGACT", "COPY2", i(1)),
                arg(&table, "CGACT", "X", i(66)),
                arg(&table, "CGACT", "X2", i(131)),
            ],
        ),
        frame_end(1),
    ]);
    gfx.set_asset_size(
        b"late",
        yu_gfx::SizeU32 {
            width: 264,
            height: 18,
        },
    );
    let info = |flag: &str| {
        q(
            &gfx,
            "CGINFO",
            vec![
                arg(&table, "CGINFO", "ID", s("OUT")),
                arg(&table, "CGINFO", flag, i(1)),
            ],
        )
    };
    assert_eq!(info("SX"), Value::Int(66));
    assert_eq!(info("SY"), Value::Int(18));
}

/// 0×0 尺寸(装载失败)不能把快照尺寸永久钉死;后续真实尺寸必须能回填。
#[test]
fn late_zero_asset_size_does_not_poison_snapshot() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("BUF")),
                arg(&table, "CG", "FILE", s("late")),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("OUT")),
                arg(&table, "CG", "FILE", s("")),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("BUF")),
                arg(&table, "CGACT", "ID2", s("OUT")),
                arg(&table, "CGACT", "COPY2", i(1)),
            ],
        ),
        frame_end(1),
    ]);
    let info = |gfx: &yu_gfx::GfxState, flag: &str| {
        q(
            gfx,
            "CGINFO",
            vec![
                arg(&table, "CGINFO", "ID", s("OUT")),
                arg(&table, "CGINFO", flag, i(1)),
            ],
        )
    };
    gfx.set_asset_size(
        b"late",
        yu_gfx::SizeU32 {
            width: 0,
            height: 0,
        },
    );
    assert_eq!(info(&gfx, "SX"), Value::Int(0));
    gfx.set_asset_size(
        b"late",
        yu_gfx::SizeU32 {
            width: 264,
            height: 18,
        },
    );
    assert_eq!(info(&gfx, "SX"), Value::Int(264));
    assert_eq!(info(&gfx, "SY"), Value::Int(18));
}

/// `COPY2` 自另一个尺寸尚未回填的快照:派生快照没有资产键,必须沿源链
/// 解析尺寸。
#[test]
fn snapshot_of_late_snapshot_resolves_size_through_source_chain() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("BUF")),
                arg(&table, "CG", "FILE", s("late")),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("A")),
                arg(&table, "CG", "FILE", s("")),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("B")),
                arg(&table, "CG", "FILE", s("")),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("BUF")),
                arg(&table, "CGACT", "ID2", s("A")),
                arg(&table, "CGACT", "COPY2", i(1)),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("A")),
                arg(&table, "CGACT", "ID2", s("B")),
                arg(&table, "CGACT", "COPY2", i(1)),
            ],
        ),
        frame_end(1),
    ]);
    gfx.set_asset_size(
        b"late",
        yu_gfx::SizeU32 {
            width: 264,
            height: 18,
        },
    );
    let info = |gfx: &yu_gfx::GfxState, flag: &str| {
        q(
            gfx,
            "CGINFO",
            vec![
                arg(&table, "CGINFO", "ID", s("B")),
                arg(&table, "CGINFO", flag, i(1)),
            ],
        )
    };
    assert_eq!(info(&gfx, "SX"), Value::Int(264));
    assert_eq!(info(&gfx, "SY"), Value::Int(18));
}

/// 非主窗口的**子组**快照:组未声明尺寸时,后端按视口尺寸组合
/// (`group_surface_size` soft/wgpu 同式);快照尺寸必须同口径。
#[test]
fn subgroup_copy_snapshot_reports_viewport_size() {
    let table = table();
    let mut gfx = state();
    let viewport = gfx.config().viewport;
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("G/")),
                arg(&table, "CG", "A", i(256)),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("G/X")),
                arg(&table, "CG", "FILE", s("x")),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("G/")),
                arg(&table, "CGACT", "ID2", s("SHOT")),
                arg(&table, "CGACT", "COPY2", i(1)),
            ],
        ),
        frame_end(1),
    ]);
    let info = |flag: &str| {
        q(
            &gfx,
            "CGINFO",
            vec![
                arg(&table, "CGINFO", "ID", s("SHOT")),
                arg(&table, "CGINFO", flag, i(1)),
            ],
        )
    };
    assert_eq!(info("SX"), Value::Int(i64::from(viewport.width)));
    assert_eq!(info("SY"), Value::Int(i64::from(viewport.height)));
}

/// `CGINFO[ONMOUSE*]` 对局部 `COPY2` 快照:显示矩形 = 区域尺寸,像素采样
/// 回落到源资产时要加回区域原点(复制块写在快照 `(0,0)`)。
#[test]
fn partial_copy2_onmouse_samples_at_region_origin() {
    struct OriginProbe;
    impl yu_gfx::TextureSampler for OriginProbe {
        fn size(&mut self, _t: &yu_gfx::TextureRef) -> Option<yu_gfx::SizeU32> {
            Some(yu_gfx::SizeU32 {
                width: 264,
                height: 18,
            })
        }
        fn sample(&mut self, _t: &yu_gfx::TextureRef, x: i64, y: i64) -> Option<[u8; 4]> {
            // 只有源合图第二块 (66..131) 的 (76,5) 是不透明。
            Some([0, 0, 0, u8::from(x == 76 && y == 5) * 255])
        }
    }

    let table = table();
    let mut gfx = state();
    gfx.set_asset_size(
        b"bt4",
        yu_gfx::SizeU32 {
            width: 264,
            height: 18,
        },
    );
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("BT.OFF.TMP")),
                arg(&table, "CG", "FILE", s("bt4")),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("BT.OVER.VOICE")),
                arg(&table, "CG", "FILE", s("")),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("BT.OFF.TMP")),
                arg(&table, "CGACT", "ID2", s("BT.OVER.VOICE")),
                arg(&table, "CGACT", "COPY2", i(1)),
                arg(&table, "CGACT", "X", i(66)),
                arg(&table, "CGACT", "X2", i(131)),
            ],
        ),
        frame_end(1),
    ]);
    gfx.set_pointer(10, 5);
    let mut sampler = OriginProbe;
    let out = gfx
        .query_info_with(
            "CGINFO",
            &[
                arg(&table, "CGINFO", "ID", s("BT.OVER.VOICE")),
                arg(&table, "CGINFO", "ONMOUSE2", i(1)),
            ],
            Some(&mut sampler),
        )
        .expect("CGINFO[ONMOUSE2]");
    assert_eq!(out, Value::Int(255), "层内 (10,5) 应采样源资产 (66+10,5)");
}

/// 任务 01 回归 #1/#2:目标层在 COPY 前已有显式 `SX/SY` 时,`COPY2` 的
/// 区域矩形必须覆盖它;之后显式 `CG[SX/SY]` 再覆盖 COPY 矩形。
///
/// 官方 `FUN_0042f809` 在复制动作后无条件写目标 `+0x78/+0x7c`(0x4305f4/
/// 0x4305fe),`CGINFO`/`ONMOUSE*` 直接读该矩形。旧实现查询先读
/// `LayerKind::Image.size` ⇒ 返回 COPY 前的 800×600。
#[test]
fn copy2_rect_overrides_stale_explicit_size_and_yields_to_new_cg_size() {
    struct Probe;
    impl yu_gfx::TextureSampler for Probe {
        fn size(&mut self, _t: &yu_gfx::TextureRef) -> Option<yu_gfx::SizeU32> {
            Some(yu_gfx::SizeU32 {
                width: 800,
                height: 600,
            })
        }
        fn sample(&mut self, _t: &yu_gfx::TextureRef, _x: i64, _y: i64) -> Option<[u8; 4]> {
            Some([0, 0, 0, 255])
        }
    }

    let table = table();
    let mut gfx = state();
    gfx.set_asset_size(
        b"big",
        yu_gfx::SizeU32 {
            width: 800,
            height: 600,
        },
    );
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("SRC")),
                arg(&table, "CG", "FILE", s("big")),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("DST")),
                // 任务书原场景:目标已有**资产图 + 显式 800×600**。
                arg(&table, "CG", "FILE", s("big")),
                arg(&table, "CG", "SX", i(800)),
                arg(&table, "CG", "SY", i(600)),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("SRC")),
                arg(&table, "CGACT", "ID2", s("DST")),
                arg(&table, "CGACT", "COPY2", i(1)),
                arg(&table, "CGACT", "X", i(10)),
                arg(&table, "CGACT", "Y", i(20)),
                arg(&table, "CGACT", "X2", i(109)),
                arg(&table, "CGACT", "Y2", i(69)),
            ],
        ),
        frame_end(1),
    ]);
    let info = |gfx: &yu_gfx::GfxState, flag: &str| {
        q(
            gfx,
            "CGINFO",
            vec![
                arg(&table, "CGINFO", "ID", s("DST")),
                arg(&table, "CGINFO", flag, i(1)),
            ],
        )
    };
    assert_eq!(
        info(&gfx, "SX"),
        Value::Int(100),
        "COPY 后矩形 = 区域宽(含端点)"
    );
    assert_eq!(
        info(&gfx, "SY"),
        Value::Int(50),
        "COPY 后矩形 = 区域高(含端点)"
    );

    // 绘制尺寸:COPY 后回到快照自然尺寸。官方 `FUN_0042f809` 释放目标旧
    // `+0x90` 表面、挂上按区域尺寸新分配的表面并写 `+0x78/+0x7c = 区域
    // 尺寸` ⇒ COPY 前的显式 800×600 不再代表目标层。内核快照表面按
    // DIV-GFX-077 保持源尺寸(区域块在 (0,0),其余透明),自然尺寸绘制
    // 才与官方“区域尺寸表面 1:1”逐像素一致;写区域尺寸会把源合图压扁
    // (DIV-GFX-077 的取舍),所以这里清成 None 而不是 100×50。
    let frame = gfx.frame();
    let dst = frame
        .layers
        .iter()
        .find(|l| l.id.as_bytes().ends_with(b"DST"))
        .expect("DST layer in frame");
    assert_eq!(
        dst.image_size(),
        None,
        "COPY 后绘制尺寸 = 快照自然尺寸(官方替换目标表面与矩形)"
    );

    // 命中矩形 = 100×50:层在 (0,0),(99,49) 命中,(100,50) 越界。
    gfx.set_pointer(99, 49);
    let mut probe = Probe;
    let hit = gfx
        .query_info_with(
            "CGINFO",
            &[
                arg(&table, "CGINFO", "ID", s("DST")),
                arg(&table, "CGINFO", "ONMOUSE2", i(1)),
            ],
            Some(&mut probe),
        )
        .expect("CGINFO[ONMOUSE2] hit");
    assert_eq!(hit, Value::Int(255));
    gfx.set_pointer(100, 50);
    let miss = gfx
        .query_info_with(
            "CGINFO",
            &[
                arg(&table, "CGINFO", "ID", s("DST")),
                arg(&table, "CGINFO", "ONMOUSE2", i(1)),
            ],
            Some(&mut probe),
        )
        .expect("CGINFO[ONMOUSE2] miss");
    assert_eq!(miss, Value::Int(-1), "(100,50) 已在 COPY 矩形外");

    // COPY 后的显式尺寸重新覆盖矩形(官方 `CG[SX/SY]` 写 +0x78/+0x7c)。
    gfx.consume_events(&[
        frame_start(2),
        call(
            2,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("DST")),
                arg(&table, "CG", "SX", i(200)),
                arg(&table, "CG", "SY", i(150)),
            ],
        ),
        frame_end(2),
    ]);
    assert_eq!(info(&gfx, "SX"), Value::Int(200));
    assert_eq!(info(&gfx, "SY"), Value::Int(150));
    gfx.set_pointer(199, 149);
    let hit = gfx
        .query_info_with(
            "CGINFO",
            &[
                arg(&table, "CGINFO", "ID", s("DST")),
                arg(&table, "CGINFO", "ONMOUSE2", i(1)),
            ],
            Some(&mut probe),
        )
        .expect("CGINFO[ONMOUSE2] 200x150 hit");
    assert_eq!(hit, Value::Int(255));
    gfx.set_pointer(200, 150);
    let miss = gfx
        .query_info_with(
            "CGINFO",
            &[
                arg(&table, "CGINFO", "ID", s("DST")),
                arg(&table, "CGINFO", "ONMOUSE2", i(1)),
            ],
            Some(&mut probe),
        )
        .expect("CGINFO[ONMOUSE2] 200x150 miss");
    assert_eq!(miss, Value::Int(-1));
}

/// 任务 01 回归(单轴 `CG SX`):缺省轴取 COPY 区域矩形,不是 COPY 前的
/// 旧 `Image.size`(官方 `CG[SX]` 只写 `+0x78`,`+0x7c` 保持 COPY 后的 50)。
#[test]
fn single_axis_cg_size_after_copy_keeps_other_axis_from_copy_rect() {
    let table = table();
    let mut gfx = state();
    gfx.set_asset_size(
        b"big5",
        yu_gfx::SizeU32 {
            width: 800,
            height: 600,
        },
    );
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("SRC")),
                arg(&table, "CG", "FILE", s("big5")),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("DST")),
                arg(&table, "CG", "FILE", s("")),
                arg(&table, "CG", "SX", i(800)),
                arg(&table, "CG", "SY", i(600)),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("SRC")),
                arg(&table, "CGACT", "ID2", s("DST")),
                arg(&table, "CGACT", "COPY2", i(1)),
                arg(&table, "CGACT", "X", i(0)),
                arg(&table, "CGACT", "Y", i(0)),
                arg(&table, "CGACT", "X2", i(99)),
                arg(&table, "CGACT", "Y2", i(49)),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("DST")),
                arg(&table, "CG", "SX", i(200)),
            ],
        ),
        frame_end(1),
    ]);
    let info = |flag: &str| {
        q(
            &gfx,
            "CGINFO",
            vec![
                arg(&table, "CGINFO", "ID", s("DST")),
                arg(&table, "CGINFO", flag, i(1)),
            ],
        )
    };
    assert_eq!(info("SX"), Value::Int(200));
    assert_eq!(
        info("SY"),
        Value::Int(50),
        "缺省轴取 COPY 矩形 50,不是旧 Image.size 600"
    );
}

/// 回归(<game-b> 底部按钮实测):COPY2 后的**纯属性 `CG`**(没有
/// `SX/SY`,例如 `CG[ID=$off E=0 X=… Y=… MODE=…]`)不得把 COPY 区域
/// 矩形写进 `LayerKind::Image.size`。
///
/// 该字段是绘制缩放尺寸;写进区域尺寸会把源尺寸快照压扁,按钮状态图
/// 出现错位(2026-11-17 桌面实机回归)。显示/命中矩形只走
/// `display_rect`,与绘制尺寸分离。
#[test]
fn pure_attribute_cg_after_copy_keeps_draw_size_untouched() {
    let table = table();
    let mut gfx = state();
    gfx.set_asset_size(
        b"bt4",
        yu_gfx::SizeU32 {
            width: 264,
            height: 18,
        },
    );
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("SRC")),
                arg(&table, "CG", "FILE", s("bt4")),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("DST")),
                arg(&table, "CG", "FILE", s("")),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("SRC")),
                arg(&table, "CGACT", "ID2", s("DST")),
                arg(&table, "CGACT", "COPY2", i(1)),
                arg(&table, "CGACT", "X", i(66)),
                arg(&table, "CGACT", "X2", i(131)),
            ],
        ),
        // 按钮脚本建完状态层后的纯属性更新:无 SX/SY。
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("DST")),
                arg(&table, "CG", "E", i(0)),
                arg(&table, "CG", "X", i(10)),
                arg(&table, "CG", "Y", i(20)),
                arg(&table, "CG", "MODE", i(0)),
            ],
        ),
        frame_end(1),
    ]);
    let frame = gfx.frame();
    let dst = frame
        .layers
        .iter()
        .find(|l| l.id.as_bytes().ends_with(b"DST"))
        .expect("DST layer in frame");
    assert_eq!(
        dst.image_size(),
        None,
        "纯属性 CG 不得把 COPY 区域写进绘制尺寸"
    );
    let info = |flag: &str| {
        q(
            &gfx,
            "CGINFO",
            vec![
                arg(&table, "CGINFO", "ID", s("DST")),
                arg(&table, "CGINFO", flag, i(1)),
            ],
        )
    };
    assert_eq!(info("SX"), Value::Int(66));
    assert_eq!(info("SY"), Value::Int(18));
}

/// 任务 01 回归 #3:全幅 `COPY2`(无区域)的目标矩形 = 源表面尺寸,
/// 即使目标层 COPY 前有别的显式尺寸。
#[test]
fn full_copy2_rect_is_source_surface_size() {
    let table = table();
    let mut gfx = state();
    gfx.set_asset_size(
        b"wide",
        yu_gfx::SizeU32 {
            width: 320,
            height: 200,
        },
    );
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("SRC")),
                arg(&table, "CG", "FILE", s("wide")),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("DST")),
                arg(&table, "CG", "FILE", s("")),
                arg(&table, "CG", "SX", i(1)),
                arg(&table, "CG", "SY", i(1)),
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
    let info = |flag: &str| {
        q(
            &gfx,
            "CGINFO",
            vec![
                arg(&table, "CGINFO", "ID", s("DST")),
                arg(&table, "CGINFO", flag, i(1)),
            ],
        )
    };
    assert_eq!(info("SX"), Value::Int(320));
    assert_eq!(info("SY"), Value::Int(200));
}

/// 任务 01 回归 #5:从局部快照再整幅复制,显示尺寸不双重换算
/// (源链按 `snapshot_display_size` 解析,不把快照区域当源表面尺寸二次裁剪)。
#[test]
fn nested_copy2_of_partial_snapshot_keeps_display_size() {
    let table = table();
    let mut gfx = state();
    gfx.set_asset_size(
        b"big2",
        yu_gfx::SizeU32 {
            width: 200,
            height: 100,
        },
    );
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("SRC")),
                arg(&table, "CG", "FILE", s("big2")),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("A")),
                arg(&table, "CG", "FILE", s("")),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("SRC")),
                arg(&table, "CGACT", "ID2", s("A")),
                arg(&table, "CGACT", "COPY2", i(1)),
                arg(&table, "CGACT", "X", i(50)),
                arg(&table, "CGACT", "Y", i(10)),
                arg(&table, "CGACT", "X2", i(149)),
                arg(&table, "CGACT", "Y2", i(59)),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("B")),
                arg(&table, "CG", "FILE", s("")),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("A")),
                arg(&table, "CGACT", "ID2", s("B")),
                arg(&table, "CGACT", "COPY2", i(1)),
            ],
        ),
        frame_end(1),
    ]);
    let info = |id: &str, flag: &str| {
        q(
            &gfx,
            "CGINFO",
            vec![
                arg(&table, "CGINFO", "ID", s(id)),
                arg(&table, "CGINFO", flag, i(1)),
            ],
        )
    };
    assert_eq!(info("A", "SX"), Value::Int(100));
    assert_eq!(info("A", "SY"), Value::Int(50));
    assert_eq!(info("B", "SX"), Value::Int(100), "整幅嵌套不变成 200");
    assert_eq!(info("B", "SY"), Value::Int(50), "整幅嵌套不变成 100");
}

/// 任务 01 回归(RESIZE 基数):`COPY2` 后 `RESIZE` 的基数取 COPY 矩形
/// (官方 `FUN_0043c971` 读目标 `+0x78/+0x7c`),不是 COPY 前的旧显式尺寸。
#[test]
fn resize_after_copy_uses_copy_rect_as_base() {
    let table = table();
    let mut gfx = state();
    gfx.set_asset_size(
        b"big3",
        yu_gfx::SizeU32 {
            width: 800,
            height: 600,
        },
    );
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("SRC")),
                arg(&table, "CG", "FILE", s("big3")),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("DST")),
                arg(&table, "CG", "FILE", s("")),
                arg(&table, "CG", "SX", i(800)),
                arg(&table, "CG", "SY", i(600)),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("SRC")),
                arg(&table, "CGACT", "ID2", s("DST")),
                arg(&table, "CGACT", "COPY2", i(1)),
                arg(&table, "CGACT", "X", i(0)),
                arg(&table, "CGACT", "Y", i(0)),
                arg(&table, "CGACT", "X2", i(99)),
                arg(&table, "CGACT", "Y2", i(49)),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("DST")),
                arg(&table, "CGACT", "RESIZE", i(1)),
                arg(&table, "CGACT", "X", i(5)),
                arg(&table, "CGACT", "X2", i(5)),
            ],
        ),
        frame_end(1),
    ]);
    let info = |flag: &str| {
        q(
            &gfx,
            "CGINFO",
            vec![
                arg(&table, "CGINFO", "ID", s("DST")),
                arg(&table, "CGINFO", flag, i(1)),
            ],
        )
    };
    assert_eq!(info("SX"), Value::Int(110), "基数 100 + X 5 + X2 5");
    assert_eq!(info("SY"), Value::Int(50), "Y/Y2 缺省 0,基数 50");
}

/// `CGEND` 删层后同名重建:COPY 矩形不能残留(矩形随层对象消失)。
#[test]
fn copy_rect_does_not_survive_cgend_and_recreate() {
    let table = table();
    let mut gfx = state();
    gfx.set_asset_size(
        b"big4",
        yu_gfx::SizeU32 {
            width: 264,
            height: 18,
        },
    );
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("SRC")),
                arg(&table, "CG", "FILE", s("big4")),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("DST")),
                arg(&table, "CG", "FILE", s("")),
            ],
        ),
        call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s("SRC")),
                arg(&table, "CGACT", "ID2", s("DST")),
                arg(&table, "CGACT", "COPY2", i(1)),
                arg(&table, "CGACT", "X", i(66)),
                arg(&table, "CGACT", "X2", i(131)),
            ],
        ),
        call(1, "CGEND", vec![arg(&table, "CGEND", "ID", s("DST"))]),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("DST")),
                arg(&table, "CG", "FILE", s("")),
                arg(&table, "CG", "SX", i(30)),
                arg(&table, "CG", "SY", i(20)),
            ],
        ),
        frame_end(1),
    ]);
    let info = |flag: &str| {
        q(
            &gfx,
            "CGINFO",
            vec![
                arg(&table, "CGINFO", "ID", s("DST")),
                arg(&table, "CGINFO", flag, i(1)),
            ],
        )
    };
    assert_eq!(info("SX"), Value::Int(30));
    assert_eq!(info("SY"), Value::Int(20));
}

/// 不存在的层:尺寸/像素探测按官方中性值返回,不再制造
/// `gfx.info.missing` UNIMP(按钮 MASK/SLIDER 的可选层每帧探测会刷屏)。
#[test]
fn cginfo_missing_layer_returns_neutral_probe_values() {
    let table = table();
    let gfx = state();
    let ask = |flag: &str| {
        q(
            &gfx,
            "CGINFO",
            vec![
                arg(&table, "CGINFO", "ID", s("MISSING")),
                arg(&table, "CGINFO", flag, i(1)),
            ],
        )
    };
    assert_eq!(ask("EXIST"), Value::Int(0));
    assert_eq!(ask("TEX"), Value::Int(0));
    assert_eq!(ask("SX"), Value::Int(0));
    assert_eq!(ask("SY"), Value::Int(0));
    assert_eq!(ask("COLOR"), Value::Int(-1));
    assert_eq!(ask("ONMOUSE"), Value::Int(-1));
    assert_eq!(ask("ONMOUSE2"), Value::Int(-1));
    assert_eq!(ask("ONMOUSE3"), Value::Int(-1));
    // 未取证中性值的字段仍显式挂号,不 blanket 成 0。
    let err = q_err(
        &gfx,
        "CGINFO",
        vec![
            arg(&table, "CGINFO", "ID", s("MISSING")),
            arg(&table, "CGINFO", "E", i(1)),
        ],
    );
    assert_eq!(err.code, "gfx.info.missing");
}
