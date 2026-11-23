//! 文本域:WORD/TEXT/TEXTINFO/TEXT_OUT/RETURNCODE。
//!
//! 由 `tests/fixture.rs` 拆分而来;测试名与断言不变。

use super::common::*;

#[test]
fn text_out_folds_control_bytes_and_preserves_lead_pairs() {
    let mut b = ProjectBuilder::new();
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        // `_` 的式值:0x41、控制 0x01、DEL 0x7F、CP932 0x82B1。
        GroupSpec::new(OP_TEXT_OUT).window(0, ex::sb(b"A\x01\x7F\x82\xB1")),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(
        vm.text_state().bytes,
        vec![0x41, 0x20, 0x20, 0x82, 0xB1],
        "非前导 <0x20/>0x7E 折空格;CP932 前导+尾原样"
    );
}

#[test]
fn text_out_rejects_source_over_0x400_and_buffer_over_0x1ffd() {
    // 源串 1025 字节 > 0x400:官方 0x18e8e,返回 -1,不写入。
    let mut b = ProjectBuilder::new();
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        GroupSpec::new(OP_TEXT_OUT).window(0, ex::sb(&vec![b'a'; 0x401])),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    let _report = vm.tick_at(0, &no_input());
    let diags = vm.take_diags();
    assert!(
        diags
            .iter()
            .any(|d| d.code.as_ref() == "rt.text.out.limit"),
        "{diags:?}"
    );
    assert!(vm.text_state().bytes.is_empty(), "超限不得追加");

    // 缓冲刚好到 0x1FFE(WORD 允许),再 `_` 1 字节即 >=0x1FFE → 让出。
    let mut b = ProjectBuilder::new();
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        GroupSpec::new(OP_WORD).window(0, vec![b'a'; 0x1FFE]),
        GroupSpec::new(OP_TEXT_OUT).window(0, ex::sb(b"b")),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    let _report = vm.tick_at(0, &no_input());
    let diags = vm.take_diags();
    assert!(
        diags
            .iter()
            .any(|d| d.code.as_ref() == "rt.text.out.limit"),
        "{diags:?}"
    );
    assert_eq!(
        vm.text_state().bytes.len(),
        0x1FFE,
        "WORD 恰写到 0x1FFE;`_` 超限不得追加"
    );
}

#[test]
fn word_rejects_buffer_over_0x1ffe() {
    let mut b = ProjectBuilder::new();
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        GroupSpec::new(OP_WORD).window(0, vec![b'a'; 0x1FFF]),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    let _report = vm.tick_at(0, &no_input());
    let diags = vm.take_diags();
    assert!(
        diags
            .iter()
            .any(|d| d.code.as_ref() == "rt.text.out.limit"),
        "{diags:?}"
    );
    assert!(vm.text_state().bytes.is_empty(), "超限不得追加");
}

#[test]
fn word_text_out_and_returncode_form_the_text_stream() {
    // 对齐 ERIS userscript 实测编译形态:
    //   WORD("こん") _(式) WORD("にちは") RETURNCODE(kind=7)
    // ①手册 basic/basic.html ●テキスト表示(`_[式]` 显示式值);
    // ②语料 464 WORD / 396 RETURNCODE / 33 _;
    // ③RxYuris YSTB_TextEditor_V5 以 WORD 为文本单元。
    let mut b = ProjectBuilder::new();
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    // CP932 字节常量(本 crate 不带编码表,编码归任务 B;实测锚点直写):
    // こ=82 B1 ん=82 F1 / 世=90 A2 界=8A 45
    let cp932_kon = vec![0x82u8, 0xB1, 0x82, 0xF1];
    let cp932_sekai = vec![0x90u8, 0xA2, 0x8A, 0x45];
    // WORD 载荷 = CP932 原始字节(无表达式壳;语料 464/464 实测)。
    let word = |bytes: Vec<u8>| GroupSpec::new(OP_WORD).window(0, bytes);
    b.scripts = vec![vec![
        word(cp932_kon.clone()),
        // _["に" + "ちは"] → 文本 "にちは"(表达式串拼接走 0x4d 字面量)
        GroupSpec::new(OP_TEXT_OUT).window(
            0,
            ex::cat(&[
                ex::s("AB"), // ASCII 便捷字面量;语义验证与字节内容无关
                ex::s("CD"),
                ex::op(0x2b),
            ]),
        ),
        word(cp932_sekai.clone()),
        // RETURNCODE 唯一窗口 kind = 行计数(语料实测 1 起逐行 +1)
        GroupSpec::new(OP_RETURNCODE).window(7, Vec::new()),
        GroupSpec::new(OP_END),
    ]];

    let mut vm = boot(b);
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);

    let texts: Vec<&Vec<u8>> = vm
        .events()
        .iter()
        .filter_map(|e| match e {
            yu_rt::VmEvent::Text { bytes, .. } => Some(bytes),
            _ => None,
        })
        .collect();
    assert_eq!(texts.len(), 3, "WORD×2 + _×1 应产出 3 条 Text 事件");
    // CP932 字节原样:"こん" = 0x82 0xB1 0x82 0xF1
    assert_eq!(texts[0], &cp932_kon, "WORD 载荷 CP932 原样透传");
    // `_` 的式值:"ABCD"(0x4d 字面量拼接)
    assert_eq!(texts[1], b"ABCD");
    assert_eq!(texts[2], &cp932_sekai);

    let rc: Vec<u32> = vm
        .events()
        .iter()
        .filter_map(|e| match e {
            yu_rt::VmEvent::ReturnCode { line, .. } => Some(*line),
            _ => None,
        })
        .collect();
    assert_eq!(rc, vec![7], "RETURNCODE 窗口 kind = 编译器写入的行计数");
}

#[test]
fn text_id_sets_following_run_id_and_clear_empties_buffer() {
    // 手册 text/text.html:ID = 自动输出模式的目标文本层 ID;
    // CLEAR=1 = 完全清空系统内部文本缓冲。
    let mut b = ProjectBuilder::new();
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    let text = |slot: u8, payload: Vec<u8>| GroupSpec::new(OP_TEXT).window(slot, payload);
    b.scripts = vec![vec![
        text(0, ex::s("MSG")), // TEXT[ID="MSG"]
        GroupSpec::new(OP_WORD).window(0, b"A".to_vec()),
        text(16, ex::i32_(1)), // TEXT[CLEAR=1](0.495 slot 16)
        GroupSpec::new(OP_WORD).window(0, b"B".to_vec()),
        GroupSpec::new(OP_END),
    ]];

    let mut vm = boot(b);
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);

    let texts: Vec<Vec<u8>> = vm
        .events()
        .iter()
        .filter_map(|e| match e {
            yu_rt::VmEvent::Text { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(texts, vec![b"MSG".to_vec(), b"MSG".to_vec()]);
    assert_eq!(vm.current_text(), b"B", "CLEAR 清掉前面的 A");
    assert_eq!(vm.text_flags(), b"1", "CLEAR 同时清标志流(只剩 B)");
}

#[test]
fn text_id_keeps_cp932_bytes_without_utf8_conversion() {
    // 合成样本:<project-eris> 141 脚本里 `TEXT[ID]` 为 0 次(2026-11-04
    // census),此处显式造「あ」= CP932 0x82 0xA0。
    // REA 0x453467 求值字节后交 0x414559:MurmurHash2 + 逐字节比较,
    // 不校验 UTF-8;事件 ID 与快照层 ID 必须字节精确。
    let mut b = ProjectBuilder::new();
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    let id: &[u8] = b"\x82\xa0";
    b.scripts = vec![vec![
        GroupSpec::new(OP_TEXT).window(0, ex::sb(id)),
        GroupSpec::new(OP_WORD).window(0, b"A".to_vec()),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);

    let ids: Vec<Vec<u8>> = vm
        .events()
        .iter()
        .filter_map(|e| match e {
            yu_rt::VmEvent::Text { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(ids, vec![id.to_vec()], "事件 ID 必须是 CP932 原字节");
    assert_eq!(
        vm.text_state().layer_id,
        id,
        "快照层 ID 必须是同一份字节(不经过 String/UTF-8)"
    );
}

#[test]
fn text_flags_track_cp932_width_and_returncode_auto_defaults() {
    // REA 0x458d44:每字符一个 '1'(半角)/ '2'(全角);
    // REA 0x44c390:自动模式 RETURNCODE 追加 AUTOCLICK 'C'/AUTORETURN 'R'/
    // AUTOPAGE 'P',最后恒追加 'E'。默认 OUTPUTMODE/AUTO* 都是 1。
    let mut b = ProjectBuilder::new();
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        GroupSpec::new(OP_WORD).window(0, b"A\x82\xB1".to_vec()), // A + こ
        GroupSpec::new(OP_RETURNCODE).window(1, Vec::new()),
        GroupSpec::new(OP_END),
    ]];

    let mut vm = boot(b);
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(vm.current_text(), b"A\x82\xB1");
    assert_eq!(vm.text_flags(), b"12CRPE");
}

#[test]
fn text_outputmode_manual_click_continues_and_auto_click_yields() {
    // REA 0x45383f..0x45388b:CLICK=1 先写 'C';自动模式返回 -1(本帧让出),
    // 手动模式继续处理同一条 TEXT 的 RETURN 并执行后续命令。
    let mut b = ProjectBuilder::new();
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    let text = |slot: u8, payload: Vec<u8>| GroupSpec::new(OP_TEXT).window(slot, payload);
    b.scripts = vec![vec![
        text(1, ex::i32_(0)),  // OUTPUTMODE=0 手动
        text(15, ex::i32_(1)), // CLICK=1:手动模式不 yield
        text(11, ex::i32_(1)), // RETURN=1
        GroupSpec::new(OP_END),
    ]];

    let mut vm = boot(b);
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    // 引擎次序:CLICK 'C' 先于 RETURN 'R'。
    assert_eq!(vm.text_flags(), b"CR", "手动模式 CLICK 继续并处理 RETURN");

    // 自动模式:CLICK=1 让出,后续 RETURN 与后续命令都要等下一次 tick。
    let mut b = ProjectBuilder::new();
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        text(1, ex::i32_(1)),  // OUTPUTMODE=1 自动(默认亦为 1)
        text(15, ex::i32_(1)), // CLICK=1 → yield
        text(11, ex::i32_(1)), // 本次 TEXT 里 RETURN 被跳过
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    let r1 = vm.tick_at(0, &no_input());
    assert!(r1.errors.is_empty(), "{:?}", r1.errors);
    assert_eq!(vm.text_flags(), b"C", "自动模式 CLICK 后立即让出");
    let r2 = vm.tick_at(16, &no_input());
    assert!(r2.errors.is_empty(), "{:?}", r2.errors);
    assert_eq!(vm.text_flags(), b"CR", "下一帧继续执行被跳过的后续命令");
}

#[test]
fn textinfo_queries_text_state_by_parameter_presence() {
    // 手册 text/textinfo.html;REA 0x4538cc:查询判据是参数**出现**,
    // 不是值是否为 1;次序 OUTPUTMODE/CX/CY/CLICK/AUTO*。
    let mut b = ProjectBuilder::new();
    if let Some((_, params)) = b.commands.iter_mut().find(|(n, _)| *n == "TEXTINFO") {
        *params = vec![
            ("ID", 1, 0),
            ("OUTPUTMODE", 0, 3),
            ("CX", 0, 3),
            ("CY", 0, 3),
            ("CLICK", 0, 3),
            ("AUTORETURN", 0, 3),
            ("AUTOPAGE", 0, 3),
            ("AUTOCLICK", 0, 3),
            ("LET", 0, 3),
            ("SET", 0, 3),
        ];
    }
    let v1 = with_int_var(&mut b, 0, -1);
    let v2 = with_int_var(&mut b, 1, -1);
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    let text = |slot: u8, payload: Vec<u8>| GroupSpec::new(OP_TEXT).window(slot, payload);
    let info = |slot: u8, target: VarRef| {
        GroupSpec::new(OP_TEXTINFO)
            .window(slot, ex::i32_(1))
            .window(8, ex::r#ref(0x40, target.id))
    };
    b.scripts = vec![vec![
        text(1, ex::i32_(0)),  // OUTPUTMODE=0
        text(17, ex::i32_(0)), // AUTORETURN=0
        info(1, v1),           // TEXTINFO[OUTPUTMODE=1 LET=@v1]
        info(5, v2),           // TEXTINFO[AUTORETURN=1 LET=@v2]
        GroupSpec::new(OP_END),
    ]];

    let mut vm = boot(b);
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(vm.store().read(v1, &[]).unwrap(), Value::Int(0));
    assert_eq!(vm.store().read(v2, &[]).unwrap(), Value::Int(0));
}

#[test]
fn text_flag_sysvar_reads_the_control_stream() {
    // REA 0x446b93:id 0x7b(_TEXT_FLAG)→ [state+0x3c]。WORD 写入后,
    // 脚本用 LET[$v = $_TEXT_FLAG] 必须拿到同一字节流。
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    let flag_id = b
        .sysvars
        .iter()
        .position(|(n, _, _)| *n == "_TEXT_FLAG")
        .expect("fixture 系统变量含 _TEXT_FLAG") as u16;
    b.vars.push(VarSpec {
        kind: 1,
        group: 0,
        script_id: 0,
        var_id: sys,
        value_type: 3,
        dims: vec![],
        init: Init::Str(b""),
    });
    let v = VarRef::new(VarSpace::Dollar, sys);
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        GroupSpec::new(OP_WORD).window(0, b"A\x82\xB1".to_vec()),
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x24, v.id))
            .window(1, ex::get(0x40, flag_id)),
        GroupSpec::new(OP_END),
    ]];

    let mut vm = boot(b);
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(
        vm.store().read(v, &[]).unwrap(),
        Value::Str(b"12".to_vec()),
        "$_TEXT_FLAG = 半角/全角控制流"
    );
}

/// R3 回归:`RETURNCODE` 是文本让出点。
///
/// REA 0.495 `0x44c390`:`MOV byte [task+9],1` + `MOV EAX,-1`;泵 `0x40442e`
/// 检查 `task+9` 非零即跳过该任务。`FUN_004046fc` 在推进输入
/// (MOUSE_L/ENTER/CTRL/手柄)为正时清标志。
///
/// 断言:① 一帧只推进一行;② 宿主收到 `Text`;③ 点击前不进入下一行;
/// ④ 注入点击后进入下一行。
#[test]
fn one_text_line_per_frame_until_advance_input() {
    let mut b = ProjectBuilder::new();
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    const OP_RETURNCODE: u8 = 79;
    const OP_WORD: u8 = 106;
    let word = |s: &str| GroupSpec::new(OP_WORD).window(0, s.as_bytes().to_vec());
    b.scripts = vec![vec![
        word("A"),
        GroupSpec::new(OP_RETURNCODE).window(1, Vec::new()),
        word("B"),
        GroupSpec::new(OP_RETURNCODE).window(2, Vec::new()),
        word("C"),
        GroupSpec::new(OP_RETURNCODE).window(3, Vec::new()),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    vm.set_text_window(Some((800, 600)));
    // 帧 1:只出 "A" 行。
    vm.tick_at(0, &no_input());
    let texts = |vm: &Vm| -> Vec<Vec<u8>> {
        vm.events()
            .iter()
            .filter_map(|e| match e {
                yu_rt::VmEvent::Text { bytes, .. } => Some(bytes.clone()),
                _ => None,
            })
            .collect()
    };
    assert_eq!(texts(&vm), vec![b"A".to_vec()], "一帧只推进一行");
    let rc_count = |vm: &Vm| {
        vm.events()
            .iter()
            .filter(|e| e.kind() == "return_code")
            .count()
    };
    assert_eq!(rc_count(&vm), 1);

    // 帧 2/3:无输入 ⇒ 仍停在第一行。
    vm.tick_at(16, &no_input());
    vm.tick_at(32, &no_input());
    assert_eq!(texts(&vm), vec![b"A".to_vec()], "点击前不进入下一行");
    assert_eq!(rc_count(&vm), 1);

    // 帧 4:注入点击(MOUSE_L)⇒ post-layout 只消费 'C';R/P/E 下一帧。
    let mut tl = InputTimeline::new();
    tl.set(4, InputFrame::new(["MOUSE_L"]));
    vm.tick_at(48, &tl);
    assert_eq!(texts(&vm), vec![b"A".to_vec()], "点击当帧尚未进入下一行");
    // 帧 5:R/P/E 处理完 ⇒ 恢复并执行第二行。
    vm.tick_at(64, &no_input());
    assert_eq!(
        texts(&vm),
        vec![b"A".to_vec(), b"B".to_vec()],
        "推进输入后进入下一行(且一次只一行)"
    );
    assert_eq!(rc_count(&vm), 2);

    // 帧 6..8:第二行停在 'C',模拟长期松开。
    vm.tick_at(80, &no_input());
    vm.tick_at(96, &no_input());
    vm.tick_at(112, &no_input());
    // 帧 9:第二次点击;帧 10:R/P/E 后进入第三行。按手册
    // 「押された瞬間から 1」,长期松开后的点击必须重新从 1 起算。
    let mut tl = InputTimeline::new();
    tl.set(9, InputFrame::new(["MOUSE_L"]));
    vm.tick_at(128, &tl);
    vm.tick_at(144, &no_input());
    assert_eq!(
        texts(&vm),
        vec![b"A".to_vec(), b"B".to_vec(), b"C".to_vec()],
        "长期松开后的第二次点击仍能推进"
    );
    assert_eq!(rc_count(&vm), 3);
}

/// R3 回归(手动模式):`TEXT[OUTPUTMODE=0]` 下 `RETURNCODE` 只休憩**一帧**,
/// 不等待点击。
///
/// 引擎 `FUN_004046fc`(`0x4046fc..0x404835`)对 `task+9` 的恢复判据:
/// - `0x404711 CMP byte[state+0x1c],1`(文本状态 = `TEXT[OUTPUTMODE]`);
/// - `0x404715 JNE 0x404828` ⇒ 手动(0)/其它值**无条件恢复**,不经按钮表;
/// - `== 1`(自动)才读 CLICKBUTTON/SKIPBUTTON(`0x5b48ac..0x5b48bc`)。
///
/// v494 同构(`0x44c20c` 头部逐指令对齐,`[0x802ad8]` vs `[0x801800]`)。
///
/// 语料意义:ERIS 框架 `es_first` 首指令 `TEXT[OUTPUTMODE=0]`,若把手动模式
/// 也当"等点击",`ES.CHARNAME.DEFINE`(s84,256 组)这类定义链会永久停住 ——
/// 实测 `<game-b>` 表现为"必须手动点击才推进、只画出半套 GAMEMAIN 皮肤"。
#[test]
fn manual_outputmode_returncode_resumes_without_input() {
    let mut b = ProjectBuilder::new();
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    const OP_RETURNCODE: u8 = 79;
    const OP_WORD: u8 = 106;
    const OP_TEXT: u8 = 96;
    let word = |s: &str| GroupSpec::new(OP_WORD).window(0, s.as_bytes().to_vec());
    b.scripts = vec![vec![
        GroupSpec::new(OP_TEXT).window(1, ex::i32_(0)), // OUTPUTMODE=0 手动
        word("A"),
        GroupSpec::new(OP_RETURNCODE).window(1, Vec::new()),
        word("B"),
        GroupSpec::new(OP_RETURNCODE).window(2, Vec::new()),
        word("C"),
        GroupSpec::new(OP_RETURNCODE).window(3, Vec::new()),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    let texts = |vm: &Vm| -> Vec<Vec<u8>> {
        vm.events()
            .iter()
            .filter_map(|e| match e {
                yu_rt::VmEvent::Text { bytes, .. } => Some(bytes.clone()),
                _ => None,
            })
            .collect()
    };

    // 全程零输入:每帧一行,不需要任何点击。
    vm.tick_at(0, &no_input());
    assert_eq!(texts(&vm), vec![b"A".to_vec()], "第一帧出 A");
    vm.tick_at(16, &no_input());
    assert_eq!(
        texts(&vm),
        vec![b"A".to_vec(), b"B".to_vec()],
        "手动模式:无输入也进入下一行"
    );
    vm.tick_at(32, &no_input());
    assert_eq!(
        texts(&vm),
        vec![b"A".to_vec(), b"B".to_vec(), b"C".to_vec()],
        "手动模式:继续自动推进"
    );
}

#[test]
fn text_reveal_advances_by_wait_and_full_half_delay() {
    // H-P2-02 合成样本:WAIT=15;半角 A + 全角 あ。
    // REA `FUN_00404c0c`(0x404c0c):行首字符立即显示(+0x68==0 门),
    // 之后 `now-last < +0x48[+0x6c]` 则停;`last += delay`(u32 回绕)。
    // `WORD 0x458e78`:全角 WAIT,半角 WAIT/2 ⇒ A=7ms、あ=15ms。
    let mut b = ProjectBuilder::new();
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    const OP_RETURNCODE: u8 = 79;
    const OP_WORD: u8 = 106;
    const OP_TEXT: u8 = 96;
    b.scripts = vec![vec![
        GroupSpec::new(OP_TEXT).window(10, ex::i32_(15)), // WAIT=15
        GroupSpec::new(OP_WORD).window(0, b"A\x82\xa0".to_vec()),
        GroupSpec::new(OP_RETURNCODE).window(1, Vec::new()),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    // 合成样本没有 gfx 层;注入目标层表面尺寸(引擎在 tick 前从 gfx 查询)。
    // 真实路径要求 `TEXT[ID]` 命中已存在层(官方 `state+0x00 != 0`)。
    vm.set_text_window(Some((800, 600)));

    // tick 1(0ms):命令执行到 RETURNCODE 让出;本 tick 尚无显示段推进。
    vm.tick_at(0, &no_input());
    let s = vm.text_state();
    assert_eq!(s.output_mode, 1);
    assert_eq!((s.reveal_flags, s.reveal_bytes), (0, 0));
    assert_eq!(vm.text_steps_len(), 0);

    // tick 2(16ms):行首 A 立即显示;あ 的 15ms 未到。
    vm.tick_at(16, &no_input());
    let s = vm.text_state();
    assert_eq!((s.reveal_flags, s.reveal_bytes), (1, 1));
    assert!(!s.click);

    // tick 3(30ms):now-last=14 < 15,仍只 A。
    vm.tick_at(30, &no_input());
    assert_eq!(vm.text_state().reveal_bytes, 1, "全角延迟 15ms 未到");

    // tick 4(31ms):满 15ms,あ 显示;随后遇到 'C' 停。
    vm.tick_at(31, &no_input());
    let s = vm.text_state();
    assert_eq!((s.reveal_flags, s.reveal_bytes), (2, 3));
    assert!(s.click, "'C' = 点击等待");

    // tick 5(47ms):点击消费 'C' → R/P/E;默认 AUTOPAGE=1 ⇒ 'P' 清页。
    let mut tl = InputTimeline::new();
    tl.set(5, InputFrame::new(["MOUSE_L"]));
    vm.tick_at(47, &tl);
    // 点击在 post-layout 只消费 'C';R/P/E 下一帧处理(官方 FUN_004046fc)。
    vm.tick_at(63, &no_input());
    let s = vm.text_state();
    assert!(!s.click);
    assert_eq!((s.cursor_x, s.cursor_y), (0, 0), "'P' 复位光标");
    assert_eq!(s.page_clear_seq, 1, "'P' 换页代数 +1");
    assert_eq!(s.reveal_flags, 0, "行完成清待显示缓冲");
}

#[test]
fn text_reveal_autoclick_off_resumes_without_input() {
    // H-P2-02:自动模式但 `AUTOCLICK=0` 时行尾没有 'C';官方
    // `FUN_004046fc` 在 +0x6c 消费完 +0x64 后直接恢复(0x404818),
    // 不需要输入。内核 gate 用 `text_reveal_complete` 复刻。
    let mut b = ProjectBuilder::new();
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    const OP_RETURNCODE: u8 = 79;
    const OP_WORD: u8 = 106;
    const OP_TEXT: u8 = 96;
    b.scripts = vec![vec![
        GroupSpec::new(OP_TEXT)
            .window(19, ex::i32_(0)) // AUTOCLICK=0(0.495 slot 19)
            .window(10, ex::i32_(15)), // WAIT=15
        GroupSpec::new(OP_WORD).window(0, b"A".to_vec()),
        GroupSpec::new(OP_RETURNCODE).window(1, Vec::new()),
        GroupSpec::new(OP_WORD).window(0, b"B".to_vec()),
        GroupSpec::new(OP_RETURNCODE).window(2, Vec::new()),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    vm.set_text_window(Some((800, 600)));

    // tick 1:第一行让出。
    vm.tick_at(0, &no_input());
    assert_eq!(vm.text_state().reveal_flags, 0);
    // tick 2(16ms):A 显示 → R/P/E 消费完;默认 AUTOPAGE=1 ⇒ 'P' 清页,
    // 无输入恢复 → WORD B 让出。
    vm.tick_at(16, &no_input());
    let s = vm.text_state();
    assert_eq!(s.page_clear_seq, 1, "第一行结束清页");
    assert_eq!((s.cursor_x, s.cursor_y), (0, 0));
    assert_eq!(s.reveal_flags, 0, "行完成清待显示缓冲");
    // tick 3(32ms):B 显示 → 行尾再清页 → 恢复 → END。
    vm.tick_at(32, &no_input());
    assert_eq!(vm.text_state().page_clear_seq, 2);
}

/// `$_TEXT_FLAG` 写入 = 替换控制字节流,不重建文本(REA 0x446b98..)。
#[test]
fn text_flag_write_replaces_stream_without_rebuilding_text() {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![
        VarSpec {
            kind: 1,
            group: 0,
            script_id: 0,
            var_id: sys,
            value_type: 3,
            dims: vec![],
            init: Init::Str(b""),
        },
        VarSpec {
            kind: 1,
            group: 0,
            script_id: 0,
            var_id: sys + 1,
            value_type: 3,
            dims: vec![],
            init: Init::Str(b""),
        },
    ];
    let flag_id = b
        .sysvars
        .iter()
        .position(|(n, _, _)| *n == "_TEXT_FLAG")
        .expect("_TEXT_FLAG") as u16;
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        GroupSpec::new(OP_WORD).window(0, b"A\x82\xB1".to_vec()),
        // LET[$_TEXT_FLAG = "9"]
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x24, flag_id))
            .window(1, ex::s("9")),
        // LET[$v = $_TEXT_FLAG]
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x24, sys))
            .window(1, ex::get(0x24, flag_id)),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    let r = vm.tick_at(0, &no_input());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(vm.current_text(), b"A\x82\xB1", "写标志不得改文本");
    assert_eq!(vm.text_flags(), b"9", "写入替换整个控制流");
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::Dollar, sys), &[])
            .unwrap(),
        Value::Str(b"9".to_vec())
    );
}

/// `TEXT[ID]` 未命中:官方 `0x4538a7` 报错并返回 -1,`state+0x00` 保留旧层,
/// 同一命令后续参数跳过。合成探针模拟“MSG 存在、NOPE 不存在”。
#[test]
fn text_id_unknown_keeps_previous_layer_and_reports() {
    const OP_RETURNCODE: u8 = 79;
    const OP_TEXT: u8 = 96;
    const OP_WORD: u8 = 106;
    let mut b = ProjectBuilder::new();
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        GroupSpec::new(OP_TEXT).window(0, ex::s("MSG")),
        GroupSpec::new(OP_TEXT).window(0, ex::s("NOPE")),
        GroupSpec::new(OP_WORD).window(0, b"A".to_vec()),
        GroupSpec::new(OP_RETURNCODE).window(1, Vec::new()),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    vm.set_text_layer_probe(Some(Box::new(|id: &[u8]| {
        if id == b"MSG" {
            Some((320, 120))
        } else {
            None
        }
    })));
    let _ = vm.tick_at(0, &no_input());
    assert!(
        vm.diags().iter().any(|d| d.code == "rt.text.layer"),
        "未命中必须留诊断: {:?}",
        vm.diags()
    );
    assert_eq!(vm.text_layer_id(), b"MSG", "未命中保留旧层");
    assert_eq!(
        vm.text_state().window,
        Some((320, 120)),
        "旧层表面尺寸不变"
    );
}

/// `TEXTINFO[CX/CY]` 必须反映布局游标(官方 `0x4538cc` 读 `+0x14/+0x18`)。
///
/// 合成:`AUTOCLICK=0 AUTOPAGE=0 AUTORETURN=1`,一行 "AB" + `R`:
/// 布局完成后 `R` 复位 X、`Y += SY(20)`;任务恢复后 TEXTINFO 读到 (0,20)。
#[test]
fn textinfo_cx_cy_follow_layout_cursor() {
    const OP_RETURNCODE: u8 = 79;
    const OP_TEXT: u8 = 96;
    const OP_TEXTINFO: u8 = 97;
    const OP_WORD: u8 = 106;
    let mut b = ProjectBuilder::new();
    if let Some((_, params)) = b.commands.iter_mut().find(|(n, _)| *n == "TEXTINFO") {
        *params = vec![
            ("ID", 1, 0),
            ("OUTPUTMODE", 0, 3),
            ("CX", 0, 3),
            ("CY", 0, 3),
            ("CLICK", 0, 3),
            ("AUTORETURN", 0, 3),
            ("AUTOPAGE", 0, 3),
            ("AUTOCLICK", 0, 3),
            ("LET", 0, 3),
            ("SET", 0, 3),
        ];
    }
    let cx = with_int_var(&mut b, 0, -1);
    let cy = with_int_var(&mut b, 1, -1);
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    let info = |slot: u8, target: VarRef| {
        GroupSpec::new(OP_TEXTINFO)
            .window(slot, ex::i32_(1))
            .window(8, ex::r#ref(0x40, target.id))
    };
    b.scripts = vec![vec![
        GroupSpec::new(OP_TEXT)
            .window(19, ex::i32_(0)) // AUTOCLICK=0
            .window(18, ex::i32_(0)) // AUTOPAGE=0
            .window(17, ex::i32_(1)), // AUTORETURN=1
        GroupSpec::new(OP_WORD).window(0, b"AB".to_vec()),
        GroupSpec::new(OP_RETURNCODE).window(1, Vec::new()),
        info(2, cx),
        info(3, cy),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    // 合成样本没有 gfx 层;注入目标层表面尺寸(引擎在 tick 前从 gfx 查询)。
    vm.set_text_window(Some((800, 600)));
    vm.tick_at(0, &no_input());
    assert_eq!(
        vm.text_state().flags,
        vec![0x31, 0x31, b'R', b'E'],
        "flags after tick1"
    );
    let after1 = vm.text_state();
    assert_eq!((after1.cursor_x, after1.cursor_y), (0, 0), "tick1 尚未布局");
    vm.tick_at(16, &no_input());
    let dbg = vm.text_state();
    assert_eq!(
        (
            dbg.cursor_x,
            dbg.cursor_y,
            dbg.reveal_flags,
            dbg.bytes.len()
        ),
        (0, 20, 0, 0),
        "layout cursor/flags after tick2"
    );
    let got = (
        vm.store().read(cx, &[]).unwrap(),
        vm.store().read(cy, &[]).unwrap(),
    );
    assert_eq!(got, (Value::Int(0), Value::Int(20)), "CX/CY after layout");
}
