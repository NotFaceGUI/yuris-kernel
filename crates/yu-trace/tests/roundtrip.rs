//! D1 验收:写入 → 读回 → 再写,两次字节完全一致。
//!
//! 覆盖:header、全部载荷形状、非 UTF-8 字节、浮点特殊值、文件系统往返。

mod support;

use support::header_for;
use yu_trace::record::{ListEntry, Payload, TraceArg, TraceLog, TraceValue};
use yu_trace::Sub;
use yu_vfs::EntryKind;

fn sample_log() -> TraceLog {
    let mut log = TraceLog::new();
    log.push_header(header_for(vec![
        "ysc: v495 (105 命令)".to_string(),
        "ysl: v495 (1941 标签)".to_string(),
    ]));
    log.push(
        None,
        Payload::VfsOpen {
            source: "ypf".to_string(),
            key: b"ysbin/ysc.ybn".to_vec(),
            bytes: Some(11156),
            sha256: Some(
                "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08".to_string(),
            ),
            diag: None,
        },
    );
    log.push(
        None,
        Payload::VfsOpen {
            source: "ypf".to_string(),
            key: b"ysbin/nope.ybn".to_vec(),
            bytes: None,
            sha256: None,
            diag: Some("vfs.missing".to_string()),
        },
    );
    log.push(
        None,
        Payload::VfsExists {
            source: "ypf".to_string(),
            key: b"ysbin/ysl.ybn".to_vec(),
            exists: true,
        },
    );
    log.push(
        None,
        Payload::VfsList {
            source: "ypf".to_string(),
            key: b"ysbin".to_vec(),
            entries: vec![
                ListEntry {
                    name: b"ysc.ybn".to_vec(),
                    kind: EntryKind::File,
                    size: Some(11156),
                },
                ListEntry {
                    name: b"sub".to_vec(),
                    kind: EntryKind::Dir,
                    size: None,
                },
            ],
            diag: None,
        },
    );
    log.push(Some(1), Payload::VmFrame { start: true });
    log.push(
        Some(1),
        Payload::VmCommand {
            script: 29,
            pc: 107,
            name: "TEXT".to_string(),
            args: vec![
                TraceArg {
                    slot: 0,
                    value: TraceValue::Int(i64::MIN),
                },
                TraceArg {
                    slot: 1,
                    value: TraceValue::Float(0.1),
                },
                TraceArg {
                    slot: 2,
                    value: TraceValue::Str(vec![0x83, 0x5c, 0x83, 0x41, 0xff, 0x00]),
                },
            ],
        },
    );
    log.push(
        Some(1),
        Payload::VmJump {
            script: 29,
            pc: 107,
            to_script: 30,
            to_pc: 3,
            jump: "gosub".to_string(),
        },
    );
    log.push(
        Some(1),
        Payload::VmTaskSpawn {
            id: "SYSTEM".to_string(),
            script: 29,
            pc: 0,
            enabled: true,
            main: true,
        },
    );
    log.push(
        Some(1),
        Payload::VmTaskFinish {
            id: "SYSTEM".to_string(),
        },
    );
    log.push(
        Some(1),
        Payload::VmWait {
            task: "SYSTEM".to_string(),
            frames: Some(1),
            time_ms: None,
        },
    );
    log.push(
        Some(1),
        Payload::VmText {
            id: String::new(),
            bytes: Vec::new(),
        },
    );
    log.push(
        Some(1),
        Payload::VmFps {
            fps: 60,
            skip: false,
        },
    );
    log.push(
        Some(1),
        Payload::VmCall {
            domain: None,
            name: "SYSTEM".to_string(),
            args: vec![],
        },
    );
    log.push(
        Some(1),
        Payload::VmCall {
            domain: Some("graphics".to_string()),
            name: "CG".to_string(),
            args: vec![],
        },
    );
    log.push(Some(1), Payload::VmEnd { restart: false });
    log.push(
        Some(1),
        Payload::VmDiag {
            code: "rt.cmd.unknown".to_string(),
            detail: "opcode 250 不在工程 ysc".to_string(),
        },
    );
    log.push(Some(1), Payload::VmFrame { start: false });
    log
}

#[test]
fn jsonl_roundtrip_is_byte_identical() {
    let log = sample_log();
    let first = log.to_jsonl();
    let parsed = TraceLog::parse(&first).expect("parse canonical trace");
    let second = parsed.to_jsonl();
    assert_eq!(first, second, "write→read→write must be byte-identical");
    assert_eq!(parsed, log, "parsed log must equal the original");
    assert_eq!(first.lines().count(), log.len());
    assert!(!first.contains('\r'));
}

#[test]
fn header_is_always_seq_zero_and_meta() {
    let log = sample_log();
    let records = log.records();
    assert_eq!(records[0].seq(), 0);
    assert_eq!(records[0].sub(), Sub::Meta);
    assert_eq!(records[0].kind().as_str(), "header");
    assert!(records[0].frame().is_none());
    assert!(records[1..].iter().all(|r| r.seq() >= 1));
    assert!(records[1..].iter().all(|r| r.sub() != Sub::Meta));
}

#[test]
fn file_roundtrip_is_byte_identical() {
    let dir = support::fresh_dir("roundtrip");
    let path = dir.join("trace.jsonl");
    let log = sample_log();
    log.write_to(&path).expect("write trace");
    let bytes_first = std::fs::read(&path).expect("read raw bytes");
    let parsed = TraceLog::read_from(&path).expect("read trace");
    parsed.write_to(&path).expect("rewrite trace");
    let bytes_second = std::fs::read(&path).expect("read raw bytes");
    assert_eq!(bytes_first, bytes_second);
    assert_eq!(String::from_utf8(bytes_first).unwrap(), log.to_jsonl());
}

#[test]
fn payload_kinds_cover_every_defined_kind() {
    // 本用例防止新增 Kind 后忘记写载荷往返覆盖。
    let log = sample_log();
    let seen: std::collections::BTreeSet<&str> = log
        .records()
        .iter()
        .map(|record| record.kind().as_str())
        .collect();
    for kind in [
        "header",
        "open",
        "exists",
        "list",
        "frame_start",
        "frame_end",
        "command",
        "jump",
        "task_spawn",
        "task_finish",
        "wait",
        "text",
        "fps",
        "system_call",
        "domain_call",
        "end",
        "diag",
    ] {
        assert!(seen.contains(kind), "missing kind {kind}");
    }
    assert_eq!(seen.len(), 17);
}

#[test]
fn bytes_and_floats_survive_text_encoding() {
    let log = sample_log();
    let text = log.to_jsonl();
    // 非 ASCII 字节在文件里表现为 \u00xx,不直接写成 UTF-8;`\` 按 JSON 转义。
    assert!(text.contains("\\u0083\\\\\\u0083A\\u00ff\\u0000"));
    assert!(!text.contains('あ'));
    assert!(text.contains("\"v\":0.1"));
    let parsed = TraceLog::parse(&text).unwrap();
    let command = parsed
        .records()
        .iter()
        .find(|record| record.kind().as_str() == "command")
        .expect("command record");
    match command.payload() {
        Payload::VmCommand { args, .. } => {
            assert_eq!(
                args[2].value,
                TraceValue::Str(vec![0x83, 0x5c, 0x83, 0x41, 0xff, 0x00])
            );
        }
        other => panic!("unexpected payload: {other:?}"),
    }
}

#[test]
fn trace_dir_default_is_relative_target_path() {
    // 不读取/修改环境变量;默认值必须是相对路径(不把绝对路径带进产物)。
    if std::env::var_os("YURIS_TRACE_DIR").is_none() {
        assert_eq!(
            yu_trace::trace_dir(),
            std::path::PathBuf::from("target").join("tmp").join("trace")
        );
        assert!(yu_trace::trace_dir().is_relative());
    }
}

#[test]
fn write_trace_rejects_path_injection() {
    let log = sample_log();
    let err = yu_trace::write_trace(&log, "../escape.jsonl").unwrap_err();
    assert_eq!(err.code, yu_trace::code::IO);
    assert_eq!(
        yu_trace::write_trace(&log, "a/b.jsonl").unwrap_err().code,
        yu_trace::code::IO
    );
    assert_eq!(
        yu_trace::write_trace(&log, "..").unwrap_err().code,
        yu_trace::code::IO
    );
    assert_eq!(
        yu_trace::write_trace(&log, ".").unwrap_err().code,
        yu_trace::code::IO
    );
}
