//! 常开测试:合成字节 + 小型真实 fixture(不依赖语料环境变量)。
//!
//! 语料相关验收在 `corpus_eris.rs`;本文件的 fixture 是 ERIS 语料中
//! 三个最小产物的**原样字节切片**(16/72/90 字节,官方 SDK 样例数据)。
//! 码表全表遍历在 `src/codec/mod.rs` 的内部单测(表为本 crate 私有)。

use std::borrow::Cow;

use yu_format::code;
use yu_format::codec::auto;
use yu_format::key::{crc32, xor_segment, DEFAULT_KEY_BYTES};
use yu_format::yscm::{Arg, Command, CommandTable, KEY_TABLE_BYTES, MESSAGE_COUNT};
use yu_format::yslb::YslLabels;
use yu_format::ystb::{self, Segments};
use yu_format::ystl::ScriptList;
use yu_format::ysvr::{Init, VarDecls};
use yu_format::ystd::YstdHeader;
use yu_format::yscf::Yscf;
use yu_format::yser::YserTable;
use yu_format::{Codec, Group, Window};

fn load_fix(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("fixture {} 读取失败:{e}", path.display()))
}

// ---------------------------------------------------------------------------
// 错误码注册表
// ---------------------------------------------------------------------------

#[test]
fn codes_are_unique_and_namespaced() {
    let mut seen = std::collections::BTreeSet::new();
    for c in code::ALL_CODES {
        assert!(c.starts_with("fmt."), "码缺少 fmt 前缀:{c}");
        assert!(seen.insert(*c), "重复码:{c}");
    }
    assert!(code::is_registered(code::YBN_KEY));
    assert!(!code::is_registered("fmt.no_such"));
}

// ---------------------------------------------------------------------------
// 密钥
// ---------------------------------------------------------------------------

#[test]
fn crc32_and_default_key() {
    assert_eq!(crc32(b"Yu-ris"), 0xD36F_AC96);
    assert_eq!(DEFAULT_KEY_BYTES, [0xD3, 0x6F, 0xAC, 0x96]);
    let mut seg = *b"\x00\x00\x00\x00";
    xor_segment(&mut seg, &DEFAULT_KEY_BYTES);
    assert_eq!(seg, DEFAULT_KEY_BYTES);
}

// ---------------------------------------------------------------------------
// YSCM(命令表)
// ---------------------------------------------------------------------------

/// 合成 ysc 的命令规格:(命令名, &[(参数名, flag, kind)])。
type Spec<'a> = [(&'a str, &'a [(&'a str, u8, u8)])];

/// 合成一张 ysc.ybn。
fn build_ysc(spec: &Spec<'_>) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"YSCM");
    out.extend_from_slice(&495u32.to_le_bytes());
    out.extend_from_slice(&(spec.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    for (name, args) in spec {
        out.extend_from_slice(name.as_bytes());
        out.push(0);
        out.push(args.len() as u8);
        for (an, flag, kind) in *args {
            out.extend_from_slice(an.as_bytes());
            out.push(0);
            out.push(*flag);
            out.push(*kind);
        }
    }
    out.extend(std::iter::repeat_n(0u8, MESSAGE_COUNT)); // 空消息
    out.extend_from_slice(&[0xABu8; KEY_TABLE_BYTES]);
    out
}

#[test]
fn yscm_roundtrip_and_lookup() {
    let spec: &Spec<'_> = &[
        ("GO", &[("#", 0, 3)]),
        ("CG", &[("ID", 1, 0), ("SX", 0, 0x14)]),
        ("END", &[]),
    ];
    let bytes = build_ysc(spec);
    let table = CommandTable::parse(&bytes, b"synthetic/ysc.ybn").unwrap();
    assert_eq!(table.len(), 3);
    assert_eq!(table.name_of(0), Some("GO"));
    assert_eq!(table.name_of(2), Some("END"));
    assert_eq!(table.name_of(3), None);
    assert_eq!(table.index_of("CG"), Some(1));
    assert_eq!(table.index_of("NOPE"), None);
    assert_eq!(table.commands[1].args[1].kind, 0x14);
    assert_eq!(table.messages.len(), MESSAGE_COUNT);
    assert!(table.key_table.iter().all(|&b| b == 0xAB));

    // 逐字节截断安全:任何前缀都不 panic
    for cut in 0..bytes.len() {
        let _ = CommandTable::parse(&bytes[..cut], b"cut");
    }
}

// ---------------------------------------------------------------------------
// YSTL / YSLB / YSVR / YSTD / YSCF / YSER
// ---------------------------------------------------------------------------

#[test]
fn ystl_entries_and_sentinel() {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"YSTL");
    buf.extend_from_slice(&495u32.to_le_bytes());
    buf.extend_from_slice(&2u32.to_le_bytes());
    for (index, name, varc, lblc, txtc) in [
        (0u32, "data\\a.yst", 3u32, 2u32, 1u32),
        (1, "data\\b.yst", u32::MAX, u32::MAX, 0),
    ] {
        buf.extend_from_slice(&index.to_le_bytes());
        buf.extend_from_slice(&(name.len() as u32).to_le_bytes());
        buf.extend_from_slice(name.as_bytes());
        buf.extend_from_slice(&0x01D5_0000u32.to_le_bytes()); // filetime hi
        buf.extend_from_slice(&0xA000_0000u32.to_le_bytes()); // filetime lo
        buf.extend_from_slice(&varc.to_le_bytes());
        buf.extend_from_slice(&lblc.to_le_bytes());
        buf.extend_from_slice(&txtc.to_le_bytes());
    }
    let list = ScriptList::parse(&buf, b"synthetic/yst_list.ybn").unwrap();
    assert_eq!(list.entries.len(), 2);
    assert!(list.entries[0].has_script());
    assert_eq!(list.entries[0].text_count, 1);
    assert!(!list.entries[1].has_script(), "哨兵 0xFFFFFFFF = 无脚本产物");
    assert_eq!(list.entries[1].file_time, 0x01D5_0000_A000_0000);
    for cut in 0..buf.len() {
        let _ = ScriptList::parse(&buf[..cut], b"cut");
    }
}

#[test]
fn yslb_records() {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"YSLB");
    buf.extend_from_slice(&495u32.to_le_bytes());
    buf.extend_from_slice(&2u32.to_le_bytes());
    buf.extend_from_slice(&[7u8; 0x400]); // 索引块紧跟 12 字节头
    for (name, a, b, script, d, e) in
        [("es.ERIS", 9u32, 5u32, 8u16, 0u8, 0u8), ("あ", 1, 2, 130, 3, 4)]
    {
        buf.push(name.len() as u8);
        buf.extend_from_slice(name.as_bytes());
        buf.extend_from_slice(&a.to_le_bytes());
        buf.extend_from_slice(&b.to_le_bytes());
        buf.extend_from_slice(&script.to_le_bytes());
        buf.push(d);
        buf.push(e);
    }
    let labels = YslLabels::parse(&buf, b"synthetic/ysl.ybn").unwrap();
    assert_eq!(labels.entries.len(), 2);
    assert!(labels.hash_block.iter().all(|&b| b == 7));
    let found = labels.find("es.ERIS".as_bytes()).unwrap();
    assert_eq!((found.field_b, found.script_no), (5, 8));
    let kana = labels.find("あ".as_bytes()).unwrap();
    assert_eq!(kana.script_no, 130);
    for cut in 0..buf.len() {
        let _ = YslLabels::parse(&buf[..cut], b"cut");
    }
}

#[test]
fn ysvr_all_ty_shapes() {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"YSVR");
    buf.extend_from_slice(&495u32.to_le_bytes());
    buf.extend_from_slice(&4u16.to_le_bytes());
    // 1) 全局 INT,有初值
    buf.extend_from_slice(&[1, 0]);
    buf.extend_from_slice(&0u16.to_le_bytes());
    buf.extend_from_slice(&10u16.to_le_bytes());
    buf.extend_from_slice(&[1, 0]); // ty=1, dims=0
    buf.extend_from_slice(&400i64.to_le_bytes());
    // 2) 仅声明(ty=0,无初值段——ERIS 实测 452 条的形态)
    buf.extend_from_slice(&[1, 0]);
    buf.extend_from_slice(&0u16.to_le_bytes());
    buf.extend_from_slice(&11u16.to_le_bytes());
    buf.extend_from_slice(&[0, 0]);
    // 3) 局部 STR 数组,串初值
    buf.extend_from_slice(&[2, 1]);
    buf.extend_from_slice(&7u16.to_le_bytes());
    buf.extend_from_slice(&12u16.to_le_bytes());
    buf.extend_from_slice(&[3, 2]); // ty=3, dims=2
    buf.extend_from_slice(&8u32.to_le_bytes());
    buf.extend_from_slice(&5u32.to_le_bytes());
    buf.extend_from_slice(&3u16.to_le_bytes());
    buf.extend_from_slice(b"abc"); // 串内容为原始字节,本层不解释
    // 4) 静态 FLT
    buf.extend_from_slice(&[3, 1]);
    buf.extend_from_slice(&0u16.to_le_bytes());
    buf.extend_from_slice(&13u16.to_le_bytes());
    buf.extend_from_slice(&[2, 1]); // ty=2, dims=1
    buf.extend_from_slice(&4u32.to_le_bytes());
    buf.extend_from_slice(&1.5f64.to_le_bytes());
    let decls = VarDecls::parse(&buf, b"synthetic/ysv.ybn").unwrap();
    assert_eq!(decls.decls.len(), 4);
    assert_eq!(decls.decls[0].init, Init::Int(400));
    assert_eq!(decls.decls[1].init, Init::None);
    assert_eq!(decls.decls[2].dims, vec![8, 5]);
    assert_eq!(decls.decls[3].init, Init::Float(1.5));
    assert_eq!(decls.loaded_at_init().count(), 3, "初始化只装全局+静态");
    assert_eq!(decls.loaded_for_script(7).count(), 1);
    for cut in 0..buf.len() {
        let _ = VarDecls::parse(&buf[..cut], b"cut");
    }
    // 非法 kind → fmt.ysv.field
    let mut bad = buf.clone();
    bad[10] = 9;
    let err = VarDecls::parse(&bad, b"bad").unwrap_err();
    assert_eq!(err.code, code::YSV_FIELD);
}

#[test]
fn yscf_real_fixture_and_engine_checks() {
    let bytes = load_fix("yscfg.ybn");
    let cfg = Yscf::parse(&bytes, b"fixtures/yscfg.ybn").unwrap();
    assert_eq!((cfg.width, cfg.height), (800, 600));
    assert_eq!(cfg.image_ext_order, [1, 2, 3, 4, 5, 0, 0, 0], "png bmp jpg gif psb");
    assert_eq!(cfg.sound_ext_order, [1, 2, 0, 0], "ogg wav");
    assert_eq!(cfg.flag_28, 0);
    // 标题按 CP932 解出(官方样例的占位标题)
    let title = Codec::Cp932.decode(&cfg.title, b"yscfg.title").unwrap();
    assert_eq!(title, "(タイトル名)");
    // 引擎侧校验:宽度超界 → fmt.yscf.field(引擎上界 = 0x2000)
    let mut bad = bytes.clone();
    bad[0x10..0x14].copy_from_slice(&0x2001u32.to_le_bytes()); // > 0x2000
    let err = Yscf::parse(&bad, b"bad").unwrap_err();
    assert_eq!(err.code, code::YSCF_FIELD);
    for cut in 0..bytes.len() {
        let _ = Yscf::parse(&bytes[..cut], b"cut");
    }
}

#[test]
fn ystd_real_fixture_and_version_is_recorded_not_gated() {
    let bytes = load_fix("ystd.ybn");
    let header = YstdHeader::parse(&bytes, b"fixtures/yst.ybn").unwrap();
    assert_eq!(header.version, 495);
    assert_eq!(header.var_count, 4600);
    assert_eq!(header.slot_count(), 4601);
    assert_eq!(header.unknown, 396);
    // 版本只记录:换成 0.481 商业包的值(481)或任意值都应照常解析。
    for version in [481u32, 494, 0x1EF] {
        let mut other = bytes.clone();
        other[4..8].copy_from_slice(&version.to_le_bytes());
        let parsed = YstdHeader::parse(&other, b"other").expect("版本不参与校验");
        assert_eq!(parsed.version, version);
        assert_eq!(parsed.var_count, 4600);
    }
    // 结构缺陷仍然拒绝:varCount = u32::MAX(槽位 +1 溢出)。
    let mut bad = bytes;
    bad[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    let err = YstdHeader::parse(&bad, b"bad").unwrap_err();
    assert_eq!(err.code, code::LIMIT);
}

#[test]
fn yser_records() {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"YSER");
    buf.extend_from_slice(&495u32.to_le_bytes());
    buf.extend_from_slice(&2u32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&100010u32.to_le_bytes());
    buf.extend_from_slice("エラー\0".as_bytes());
    buf.extend_from_slice(&100030u32.to_le_bytes());
    buf.extend_from_slice(b"%s\0");
    let table = YserTable::parse(&buf, b"synthetic/yse.ybn").unwrap();
    assert_eq!(table.entries[0].id, 100010);
    assert_eq!(table.entries[1].name, b"%s");
    for cut in 0..buf.len() {
        let _ = YserTable::parse(&buf[..cut], b"cut");
    }
}

// ---------------------------------------------------------------------------
// YSTB:真实 72 字节 fixture(system_start 的编译产物)+ 合成
// ---------------------------------------------------------------------------

/// 手工搭建的命令表,模拟 ysc:rank 0/1/14 覆盖测试所需命令,
/// 其余补位到 119(真实表条数),保证真实 fixture 的 opcode 43/14 可解析。
fn synthetic_table() -> CommandTable {
    let mut commands: Vec<Command> = (0..119)
        .map(|i| Command {
            name: format!("PAD{i:03}").into(),
            argc: 0,
            args: Vec::new(),
        })
        .collect();
    // 真实 ysc 的关键序位:END=14、GO=43、GOSUB=44(ERIS 语料实证)
    commands[14] = Command {
        name: "END".into(),
        argc: 0,
        args: Vec::new(),
    };
    let hash_arg = || Arg {
        name: "#".into(),
        flag: 0,
        kind: 0,
    };
    commands[43] = Command {
        name: "GO".into(),
        argc: 1,
        args: vec![hash_arg()],
    };
    commands[44] = Command {
        name: "GOSUB".into(),
        argc: 1,
        args: vec![hash_arg()],
    };
    CommandTable {
        version: 495,
        commands,
        messages: vec![Vec::new(); MESSAGE_COUNT],
        key_table: Box::new([0u8; KEY_TABLE_BYTES]),
    }
}

#[test]
fn ystb_real_fixture_system_start() {
    let bytes = load_fix("ystb_00073.ybn");
    let segs =
        Segments::decrypt(&bytes, &DEFAULT_KEY_BYTES, b"fixtures/ystb_00073.ybn").unwrap();
    // 官方 system_start.yst 的编译产物:GO[#=es.ERIS] + END[]
    assert_eq!(segs.inst.len(), 8);
    assert_eq!(segs.args_index.len(), 12);
    assert_eq!(segs.args_data.len(), 12);
    assert_eq!(segs.line_numbers.len(), 8);
    let script =
        ystb::decode(&segs, 73, &synthetic_table(), Codec::Cp932, b"fixtures").unwrap();
    assert_eq!(script.script_id, 73);
    assert_eq!(script.groups.len(), 2);
    assert_eq!(script.groups[0].command, "GO");
    assert_eq!(script.groups[0].windows.len(), 1);
    assert_eq!(script.groups[1].command, "END");
    assert!(script.groups[1].windows.is_empty());
    assert_eq!(
        script.labels.entries.len(),
        1,
        "GO[#=es.ERIS] 是唯一的静态标签引用"
    );
    assert_eq!(script.labels.entries[0].name, "es.ERIS");
    assert_eq!(script.labels.entries[0].group, 0);
    // 异或对合:解密后原样回封 = 原文件
    assert_eq!(segs.encrypt(495, &DEFAULT_KEY_BYTES), bytes);
    // 截断安全
    for cut in 0..bytes.len() {
        let _ = Segments::decrypt(&bytes[..cut], &DEFAULT_KEY_BYTES, b"cut");
    }
}

/// 从零合成一个 V5 YSTB(GO 引号标签 + WAIT 数值参数 + 动态目标)。
#[test]
fn ystb_synthetic_labels_and_windows() {
    // 参数数据区:'M' 形标签 "es.FOO"、'H' 形动态目标、i64 直数
    let mut data = Vec::new();
    // 'M' + u16 长度(含引号) + 引号串
    data.push(b'M');
    data.extend_from_slice(&8u16.to_le_bytes()); // `"es.FOO"` = 8 字节
    data.extend_from_slice(b"\"es.FOO\"");
    let off0 = 0u32;
    let size0 = 11u32; // 'M' + u16 + 引号串 = 完整载荷
    let off1 = data.len() as u32;
    data.extend_from_slice(b"H\x03\x00$\x07"); // 动态目标形态(H + len + 内容)
    let size1 = (data.len() as u32) - off1;
    let off2 = data.len() as u32;
    data.extend_from_slice(&60i64.to_le_bytes());
    let size2 = 8u32;

    let mut inst = Vec::new();
    inst.extend_from_slice(&[43, 1, 0, 0]); // GO, 1 参
    inst.extend_from_slice(&[44, 1, 0, 0]); // GOSUB, 1 参(数值目标位)
    inst.extend_from_slice(&[44, 1, 0, 0]); // GOSUB(H 形动态目标位)
    let mut args_index = Vec::new();
    for (ty, size, off) in [(3u16, size0, off0), (1, size2, off2), (3, size1, off1)] {
        args_index.extend_from_slice(&0u16.to_le_bytes());
        args_index.extend_from_slice(&ty.to_le_bytes());
        args_index.extend_from_slice(&size.to_le_bytes());
        args_index.extend_from_slice(&off.to_le_bytes());
    }
    let segs = Segments {
        version: 495,
        count: 3,
        inst,
        args_index,
        args_data: data,
        line_numbers: vec![0; 12],
        tail_word: 0,
        trailing: Vec::new(),
    };
    let script =
        ystb::decode(&segs, 7, &synthetic_table(), Codec::Cp932, b"synthetic").unwrap();
    let window = |ty: u16, size: u32, off: u32| Window {
        bytes: [
            0,
            0,
            ty as u8,
            (ty >> 8) as u8,
            size as u8,
            (size >> 8) as u8,
            (size >> 16) as u8,
            (size >> 24) as u8,
            off as u8,
            (off >> 8) as u8,
            (off >> 16) as u8,
            (off >> 24) as u8,
        ],
    };
    assert_eq!(
        script.groups,
        vec![
            Group {
                command: Cow::Borrowed("GO"),
                windows: vec![window(3, size0, off0)],
            },
            Group {
                command: Cow::Borrowed("GOSUB"),
                windows: vec![window(1, size2, off2)],
            },
            Group {
                command: Cow::Borrowed("GOSUB"),
                windows: vec![window(3, size1, off1)],
            },
        ]
    );
    // 只有 'M' 引号形态进标签引用;'H' 动态目标被跳过
    assert_eq!(script.labels.entries.len(), 1);
    assert_eq!(script.labels.entries[0].name, "es.FOO");
    assert_eq!(script.labels.entries[0].group, 0);
    // 回封往返
    let rebuilt = segs.encrypt(495, &DEFAULT_KEY_BYTES);
    let again = Segments::decrypt(&rebuilt, &DEFAULT_KEY_BYTES, b"rt").unwrap();
    assert_eq!(again, segs);
}

/// 复现 F1:非首位 `#` 参数(TASK 的 slot 34)必须按**窗口 slot id** 命中,
/// 而不是按窗口在指令里的发射位置。
///
/// 证据:ERIS `yst00008.ybn` 第 232 条 TASK 的 4 个窗口 id = 0/34/3/4,
/// 其中 34 对应 ysc `TASK` 参数表第 34 项 `#`(REA 实测;见
/// `docs/tasks/task-e-fixes.md` F1)。
#[test]
fn ystb_label_refs_index_by_window_slot() {
    let mut commands: Vec<Command> = (0..119)
        .map(|i| Command {
            name: format!("PAD{i:03}").into(),
            argc: 0,
            args: Vec::new(),
        })
        .collect();
    // TASK 的真实参数表:38 项,`#` 位于下标 34
    let mut args: Vec<Arg> = (0..38)
        .map(|i| Arg {
            name: format!("A{i}").into(),
            flag: 0,
            kind: 0,
        })
        .collect();
    args[0].name = "ID".into();
    args[34].name = "#".into();
    commands[93] = Command {
        name: "TASK".into(),
        argc: 38,
        args,
    };
    let table = CommandTable {
        version: 495,
        commands,
        messages: vec![Vec::new(); MESSAGE_COUNT],
        key_table: Box::new([0u8; KEY_TABLE_BYTES]),
    };

    // 参数数据区:两条 'M' 形引号串
    let mut data = Vec::new();
    data.extend_from_slice(b"M\x11\x00\"es.SS.FADE.TASK\"");
    let off_tail = data.len() as u32;
    data.extend_from_slice(b"M\x16\x00\"es.SS.FADE.TASK.MAIN\"");
    let size_tail = (data.len() as u32) - off_tail;

    let mut args_index = Vec::new();
    for (id, ty, size, off) in [
        (0u16, 3u16, 20u32, 0u32),
        (34, 3, size_tail, off_tail),
        (3, 1, 6, 0),
        (4, 1, 14, 0),
    ] {
        args_index.extend_from_slice(&id.to_le_bytes());
        args_index.extend_from_slice(&ty.to_le_bytes());
        args_index.extend_from_slice(&size.to_le_bytes());
        args_index.extend_from_slice(&off.to_le_bytes());
    }
    let segs = Segments {
        version: 495,
        count: 1,
        inst: vec![93, 4, 0, 0],
        args_index,
        args_data: data,
        line_numbers: vec![0; 4],
        tail_word: 0,
        trailing: Vec::new(),
    };
    let script = ystb::decode(&segs, 8, &table, Codec::Cp932, b"synthetic").unwrap();
    let names: Vec<&str> = script.labels.entries.iter().map(|r| r.name.as_str()).collect();
    // slot 0 = `ID`(任务 ID 字符串,不是标签);slot 34 = `#`(唯一标签引用)
    assert_eq!(
        names,
        vec!["es.SS.FADE.TASK.MAIN"],
        "slot 34 的 # 参数必须被收进标签表;slot 0 的 ID 不得误收"
    );
}

/// 复现 F2:头部 count 是唯一指令条数真值;encrypt 必须保留原 count 与尾部字节。
/// 引擎 `FUN_004518e9` 用头 `+0x08` 的 count 迭代,不按 `s1/4`。
#[test]
fn ystb_head_count_and_trailing_are_preserved() {
    let bytes = load_fix("ystb_00073.ybn");
    // 头 count 改成 1(仍保留两条指令的 s1 = 8),并追加 4 字节尾部
    let mut mutated = bytes.clone();
    mutated[8..12].copy_from_slice(&1u32.to_le_bytes());
    mutated.extend_from_slice(b"TAIL");
    let segs = Segments::decrypt(&mutated, &DEFAULT_KEY_BYTES, b"mut").unwrap();
    let seq = ystb::decode(&segs, 73, &synthetic_table(), Codec::Cp932, b"mut").unwrap();
    assert_eq!(seq.groups.len(), 1, "必须按头 count=1 解码");
    assert_eq!(
        segs.encrypt(495, &DEFAULT_KEY_BYTES),
        mutated,
        "回封必须保留头 count 与尾部字节"
    );
}

/// 复现 F5:错误密钥必须在结构自检阶段报 `fmt.ybn.key`,不能滑到
/// `fmt.ybn.opcode` 或静默产出垃圾指令。
#[test]
fn ystb_wrong_key_reports_key_error() {
    let bytes = load_fix("ystb_00073.ybn");
    let segs = Segments::decrypt(&bytes, &DEFAULT_KEY_BYTES, b"ok").unwrap();
    let wrong = [0x11u8, 0x22, 0x33, 0x44];
    let encrypted = segs.encrypt(495, &wrong);
    let err = Segments::decrypt(&encrypted, &DEFAULT_KEY_BYTES, b"bad").unwrap_err();
    assert_eq!(err.code, code::YBN_KEY, "错 key 应报 fmt.ybn.key:{err:?}");
}

/// 复现 F3:宽高在引擎里是 u32(`FUN_0045dbb0` 读 dword),且范围检查按
/// 无符号减法:`width-0x80 <= 0x1F80`(即 0x80..=0x2000)、
/// `height-0x10 <= 0x1FF0`(即 0x10..=0x2000);宽高必须同时为 0 或同时有效。
#[test]
fn yscf_u32_dimensions_match_engine_checks() {
    let bytes = load_fix("yscfg.ybn");
    let mut wide = bytes.clone();
    wide[0x10..0x14].copy_from_slice(&0x2000u32.to_le_bytes());
    assert!(
        Yscf::parse(&wide, b"wide").is_ok(),
        "宽 0x2000 在引擎的无符号范围内,必须接受"
    );
    let mut too_wide = bytes.clone();
    too_wide[0x10..0x14].copy_from_slice(&0x2001u32.to_le_bytes());
    assert_eq!(
        Yscf::parse(&too_wide, b"too-wide").unwrap_err().code,
        code::YSCF_FIELD
    );
    let mut half_zero = bytes.clone();
    half_zero[0x10..0x14].copy_from_slice(&0u32.to_le_bytes());
    assert_eq!(
        Yscf::parse(&half_zero, b"half-zero").unwrap_err().code,
        code::YSCF_FIELD,
        "宽 0 / 高非 0 时引擎按无符号下溢拒绝"
    );
}

/// 复现 F4:32 位 usize 上的未检查算术。
/// 构造 `s2/s3` 巨大到 `0x20 + s1 + s2 + s3` 在 32 位回绕的头,
/// 必须返回 `fmt.limit`/`fmt.truncated`,禁止 panic 或越界切片。
#[test]
fn ystb_oversized_sections_do_not_panic() {
    let mut buf = vec![0u8; 0x20];
    buf[0..4].copy_from_slice(b"YSTB");
    buf[4..8].copy_from_slice(&495u32.to_le_bytes());
    buf[8..12].copy_from_slice(&0u32.to_le_bytes());
    buf[0x0C..0x10].copy_from_slice(&0u32.to_le_bytes());
    buf[0x10..0x14].copy_from_slice(&u32::MAX.to_le_bytes());
    buf[0x14..0x18].copy_from_slice(&u32::MAX.to_le_bytes());
    buf[0x18..0x1C].copy_from_slice(&u32::MAX.to_le_bytes());
    let err = Segments::decrypt(&buf, &DEFAULT_KEY_BYTES, b"big").unwrap_err();
    assert_eq!(err.code, code::TRUNCATED, "{err:?}");
}

/// 复现 F4:YSTL 记录名长度巨大时不得 panic。
#[test]
fn ystl_oversized_name_length_does_not_panic() {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"YSTL");
    buf.extend_from_slice(&495u32.to_le_bytes());
    buf.extend_from_slice(&1u32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes()); // index
    buf.extend_from_slice(&u32::MAX.to_le_bytes()); // name_len
    let err = ScriptList::parse(&buf, b"big").unwrap_err();
    assert_eq!(err.code, code::TRUNCATED, "{err:?}");
}

/// 复现 F4:YSTD varCount = u32::MAX 时槽位数 +1 不得回绕。
#[test]
fn ystd_max_var_count_is_rejected() {
    let mut buf = vec![0u8; 16];
    buf[0..4].copy_from_slice(b"YSTD");
    buf[4..8].copy_from_slice(&495u32.to_le_bytes());
    buf[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    let err = YstdHeader::parse(&buf, b"max").unwrap_err();
    assert_eq!(err.code, code::LIMIT, "{err:?}");
}

#[test]
fn ystb_error_cases() {
    let table = synthetic_table();
    // opcode 越界 → fmt.ybn.opcode
    let segs = Segments {
        version: 495,
        count: 1,
        inst: vec![200, 0, 0, 0],
        args_index: Vec::new(),
        args_data: Vec::new(),
        line_numbers: Vec::new(),
        tail_word: 0,
        trailing: Vec::new(),
    };
    let err = ystb::decode(&segs, 0, &table, Codec::Cp932, b"t").unwrap_err();
    assert_eq!(err.code, code::YBN_OPCODE);
    // 参数索引区多余字节 → fmt.ybn.args
    let segs = Segments {
        version: 495,
        count: 1,
        inst: vec![0, 0, 0, 0],
        args_index: vec![0u8; 13],
        args_data: Vec::new(),
        line_numbers: Vec::new(),
        tail_word: 0,
        trailing: Vec::new(),
    };
    let err = ystb::decode(&segs, 0, &table, Codec::Cp932, b"t").unwrap_err();
    assert_eq!(err.code, code::YBN_ARGS);
    // V2 版本带 → Unimplemented(无语料,禁臆造)
    let mut v2 = vec![0u8; 0x30];
    v2[0..4].copy_from_slice(b"YSTB");
    v2[4..8].copy_from_slice(&250u32.to_le_bytes());
    let err = Segments::decrypt(&v2, &DEFAULT_KEY_BYTES, b"t").unwrap_err();
    assert_eq!(err.code, code::YBN_UNSUPPORTED);
    assert_eq!(err.severity, yu_log::Severity::Unimplemented);
    // 魔数不符 → fmt.magic
    let err = Segments::decrypt(b"XYZB____", &DEFAULT_KEY_BYTES, b"t").unwrap_err();
    assert_eq!(err.code, code::MAGIC);
}

// ---------------------------------------------------------------------------
// 编解码器:锚点 + 非法序列 + auto(全表遍历在 codec 内部单测)
// ---------------------------------------------------------------------------

#[test]
fn codec_anchors_cp932() {
    let anchors: &[(&str, &[u8])] = &[
        ("日", &[0x93, 0xFA]),
        ("本", &[0x96, 0x7B]),
        ("語", &[0x8C, 0xEA]),
        ("ソ", &[0x83, 0x5C]), // 著名的 0x5C 后随字节
        ("表", &[0x95, 0x5C]),
        ("十", &[0x8F, 0x5C]),
        ("ｱ", &[0xB1]),        // 半角假名 = 单字节
        ("～", &[0x81, 0x60]), // 全角波浪
        ("①", &[0x87, 0x40]), // NEC 选定行
        (
            "メモリ不足です。",
            &[
                0x83, 0x81, 0x83, 0x82, 0x83, 0x8a, 0x95, 0x73, 0x91, 0xab, 0x82, 0xc5,
                0x82, 0xb7, 0x81, 0x42,
            ],
        ), // 与 ERIS yse.ybn 首条错误文案逐字节一致
    ];
    for (text, bytes) in anchors {
        assert_eq!(Codec::Cp932.decode(bytes, b"t").unwrap(), *text, "{bytes:?}");
        assert_eq!(Codec::Cp932.encode(text, b"t").unwrap(), *bytes, "{text}");
    }
    assert!(anchors.len() >= 10);
}

#[test]
fn codec_anchors_gbk() {
    let anchors: &[(&str, &[u8])] = &[
        ("你", &[0xC4, 0xE3]),
        ("好", &[0xBA, 0xC3]),
        ("中", &[0xD6, 0xD0]),
        ("国", &[0xB9, 0xFA]),
        ("人", &[0xC8, 0xCB]),
        ("们", &[0xC3, 0xC7]),
        ("戀", &[0x91, 0xD9]),
        ("乜", &[0xD8, 0xBF]),
        ("这", &[0xD5, 0xE2]),
        ("€", &[0x80]), // GBK 单字节 0x80 = 欧元符(Windows CP936 行为)
    ];
    for (text, bytes) in anchors {
        assert_eq!(Codec::Gbk.decode(bytes, b"t").unwrap(), *text);
        assert_eq!(Codec::Gbk.encode(text, b"t").unwrap(), *bytes);
    }
    assert!(anchors.len() >= 10);
}

#[test]
fn codec_invalid_sequences() {
    // CP932:前导字节截断、非法单字节、非法后随
    for bad in [
        [0x81u8].as_slice(),
        &[0xFD],
        &[0xA0],
        &[0x81, 0x7F],
        &[0x81, 0xFD],
        &[0x93],
    ] {
        let err = Codec::Cp932.decode(bad, b"t").unwrap_err();
        assert_eq!(err.code, code::CODEC_INVALID, "{bad:?}");
    }
    // GBK:0x7F 后随非法
    let err = Codec::Gbk.decode(&[0x81, 0x7F], b"t").unwrap_err();
    assert_eq!(err.code, code::CODEC_INVALID);
    // 不可映射字符
    let err = Codec::Cp932.encode("\u{1F600}", b"t").unwrap_err();
    assert_eq!(err.code, code::CODEC_INVALID);
    // 未知名
    assert_eq!(Codec::from_name("big5").unwrap_err().code, code::CODEC_NAME);
    assert_eq!(Codec::from_name("gbk").unwrap(), Codec::Gbk);
    assert_eq!(Codec::from_name("sjis").unwrap(), Codec::Cp932);
}

#[test]
fn codec_auto_detection() {
    // 日文样本(CP932 字节)→ CP932
    let jp = Codec::Cp932
        .encode("これはテストです。「セーブ」します。", b"t")
        .unwrap();
    let d = auto::detect(&[&jp]);
    assert_eq!(d.codec, Codec::Cp932, "{d:?}");
    // 简中样本(GBK 字节)→ GBK
    let zh = Codec::Gbk
        .encode(
            "这是一个测试。请选择要读取的存档。所有的文字都必须按照原来的编码写回文件里。", 
            b"t",
        )
        .unwrap();
    let d = auto::detect(&[&zh]);
    assert_eq!(d.codec, Codec::Gbk, "{d:?}");
    // 假名 + 半角假名混合(真实 ybn 文本池形态)→ CP932 强信号
    let mixed = Codec::Cp932
        .encode("セーブします｡詳細はｵﾌﾞｼﾞｪｸﾄを確認｡", b"t")
        .unwrap();
    assert_eq!(auto::detect(&[&mixed]).codec, Codec::Cp932);
    // 纯 ASCII → 缺省 CP932
    assert_eq!(auto::detect(&[b"SCORE=100"]).codec, Codec::Cp932);
    // 空池 → 缺省 CP932
    let d = auto::detect(&[]);
    assert_eq!(d.codec, Codec::Cp932);
    assert_eq!(d.cp932.points, 0.0);
    // 确定性:同输入恒同结论、同分
    assert_eq!(auto::detect(&[&jp, &zh]), auto::detect(&[&jp, &zh]));
}
