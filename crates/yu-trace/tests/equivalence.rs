//! D2 验收:包装前后 `open/exists/list` 结果逐字节一致,且每次调用都有记录。
//!
//! 三条断言线:
//! 1. 同一组 key 在裸 `MemSource` 与 `TracingSource<MemSource>` 上返回值/错误
//!    **逐字节**一致(含错误码与错误详情);
//! 2. 调用条数严格等于记录条数(成功与失败都记);
//! 3. 记录内容就是调用事实:key 字节、结果身份、帧号标注。

mod support;

use support::header_for;
use yu_trace::record::{ListEntry, Payload, TraceLog};
use yu_trace::{shared_log, EventRecorder, TracingSource};
use yu_vfs::{Codec, DataSource, Entry, LookupKey, MemSource};

fn build_source() -> MemSource {
    let mut src = MemSource::new(Codec::Cp932);
    src.insert(br"ysbin\ysc.ybn", vec![1, 2, 3, 4, 5]).unwrap();
    src.insert(b"ysbin/ysl.ybn", vec![0x83, 0x5c, 0xff])
        .unwrap();
    src.insert(b"data/cg/ev/a.png", (0..255).collect()).unwrap();
    src.insert_dir(b"empty").unwrap();
    src
}

fn keys() -> Vec<LookupKey> {
    let mut keys = vec![
        LookupKey::new(br"ysbin\ysc.ybn", Codec::Cp932).unwrap(),
        LookupKey::new(b"ysbin/ysl.ybn", Codec::Cp932).unwrap(),
        LookupKey::new(br"Y:\BIN\YSC.YBN", Codec::Cp932).unwrap(),
        LookupKey::new(b"data/cg/ev/a.png", Codec::Cp932).unwrap(),
        LookupKey::new(b"ysbin", Codec::Cp932).unwrap(),
        LookupKey::new(b"empty", Codec::Cp932).unwrap(),
        LookupKey::new(b"missing", Codec::Cp932).unwrap(),
        LookupKey::new(b"ysbin/ysc.ybn/nested", Codec::Cp932).unwrap(),
        LookupKey::root(),
    ];
    keys.push(LookupKey::new(&[0x83, 0x5c, b'X'], Codec::Cp932).unwrap());
    keys
}

fn entry_bytes(entries: &[Entry]) -> Vec<u8> {
    let mut out = Vec::new();
    for entry in entries {
        out.extend_from_slice(&(entry.name.len() as u32).to_le_bytes());
        out.extend_from_slice(&entry.name);
        out.push(match entry.kind {
            yu_vfs::EntryKind::File => 0,
            yu_vfs::EntryKind::Dir => 1,
        });
        match entry.size {
            Some(size) => {
                out.push(1);
                out.extend_from_slice(&size.to_le_bytes());
            }
            None => out.push(0),
        }
    }
    out
}

/// `Ok` 与 `Err` 都转成可逐字节比较的规范形式(含完整错误码与详情)。
fn open_repr(result: Result<Vec<u8>, yu_log::Diag>) -> Vec<u8> {
    match result {
        Ok(bytes) => {
            let mut out = vec![0];
            out.extend_from_slice(&bytes);
            out
        }
        Err(diag) => {
            let mut out = vec![1];
            out.extend_from_slice(diag.to_json_line().as_bytes());
            out
        }
    }
}

fn list_repr(result: Result<Vec<Entry>, yu_log::Diag>) -> Vec<u8> {
    match result {
        Ok(entries) => {
            let mut out = vec![0];
            out.extend_from_slice(&entry_bytes(&entries));
            out
        }
        Err(diag) => {
            let mut out = vec![1];
            out.extend_from_slice(diag.to_json_line().as_bytes());
            out
        }
    }
}

#[test]
fn open_exists_list_are_byte_identical_through_the_wrapper() {
    let bare = build_source();
    let log = shared_log();
    let traced = TracingSource::new(build_source(), log.clone());

    let mut calls = 0usize;
    for key in keys() {
        assert_eq!(
            open_repr(bare.open(&key)),
            open_repr(traced.open(&key)),
            "open mismatch for {:?}",
            key.display()
        );
        calls += 1;
        assert_eq!(
            bare.exists(&key),
            traced.exists(&key),
            "exists mismatch for {:?}",
            key.display()
        );
        calls += 1;
        assert_eq!(
            list_repr(bare.list(&key)),
            list_repr(traced.list(&key)),
            "list mismatch for {:?}",
            key.display()
        );
        calls += 1;
    }

    assert_eq!(log.lock().unwrap().len(), calls, "every call must be recorded");
    assert_eq!(traced.source_name(), bare.source_name());
    assert_eq!(traced.source_name(), "mem");
}

#[test]
fn records_describe_the_call_and_its_identity() {
    let log = shared_log();
    let traced = TracingSource::new(build_source(), log);
    let key = LookupKey::new(br"ysbin\ysc.ybn", Codec::Cp932).unwrap();
    let bytes = traced.open(&key).unwrap();

    let missing = LookupKey::new(b"missing", Codec::Cp932).unwrap();
    let _diag = traced.open(&missing).unwrap_err();

    let expected_sha = yu_trace::sha256_hex(&bytes);
    let log = traced.recorder().shared_log();
    let records = log.lock().unwrap();
    assert_eq!(records.len(), 2);

    assert_eq!(records.records()[0].frame(), None, "启动链读取在帧外");
    match records.records()[0].payload() {
        Payload::VfsOpen {
            source,
            key: recorded_key,
            bytes: recorded_bytes,
            sha256,
            diag,
        } => {
            assert_eq!(source, "mem");
            assert_eq!(recorded_key.as_slice(), b"ysbin/ysc.ybn");
            assert_eq!(*recorded_bytes, Some(bytes.len() as u64));
            assert_eq!(sha256.as_deref(), Some(expected_sha.as_str()));
            assert!(diag.is_none());
        }
        other => panic!("unexpected payload: {other:?}"),
    }

    match records.records()[1].payload() {
        Payload::VfsOpen {
            source,
            key: recorded_key,
            bytes,
            sha256,
            diag,
        } => {
            assert_eq!(source, "mem");
            assert_eq!(recorded_key.as_slice(), b"missing");
            assert!(bytes.is_none());
            assert!(sha256.is_none());
            assert_eq!(diag.as_deref(), Some(yu_log::code::vfs::MISSING));
        }
        other => panic!("unexpected payload: {other:?}"),
    }
}

#[test]
fn frame_annotation_is_recorded_verbatim() {
    let log = shared_log();
    let traced = TracingSource::new(build_source(), log);
    let key = LookupKey::new(b"ysbin", Codec::Cp932).unwrap();
    let file = LookupKey::new(b"ysbin/ysc.ybn", Codec::Cp932).unwrap();

    traced.set_frame(Some(42));
    let entries = traced.list(&key).unwrap();
    traced.set_frame(None);
    // `exists` 的语义由数据源自己决定(目录不算存在);这里只验证帧标注。
    assert!(traced.exists(&file));

    let log = traced.recorder().shared_log();
    let records = log.lock().unwrap();
    assert_eq!(records.records()[0].frame(), Some(42));
    match records.records()[0].payload() {
        Payload::VfsList {
            source,
            key: recorded_key,
            entries: recorded_entries,
            diag,
        } => {
            assert_eq!(source, "mem");
            assert_eq!(recorded_key.as_slice(), b"ysbin");
            assert!(diag.is_none());
            let expected: Vec<ListEntry> = entries.iter().map(ListEntry::from_entry).collect();
            assert_eq!(recorded_entries, &expected);
            // 目录项保持数据源给出的顺序(名字升序)。
            assert!(recorded_entries.len() >= 2);
        }
        other => panic!("unexpected payload: {other:?}"),
    }
    assert_eq!(records.records()[1].frame(), None);
}

#[test]
fn error_payloads_keep_the_stable_code_not_the_message() {
    let log = shared_log();
    let traced = TracingSource::new(build_source(), log);
    let key = LookupKey::new(b"ysbin/ysc.ybn", Codec::Cp932).unwrap();
    let diag = traced.list(&key).unwrap_err();
    assert_eq!(diag.code, yu_log::code::vfs::NOT_DIR);

    let log = traced.recorder().shared_log();
    let records = log.lock().unwrap();
    match records.records()[0].payload() {
        Payload::VfsList {
            entries,
            diag: recorded,
            ..
        } => {
            assert!(entries.is_empty());
            assert_eq!(recorded.as_deref(), Some(yu_log::code::vfs::NOT_DIR));
        }
        other => panic!("unexpected payload: {other:?}"),
    }
}

#[test]
fn wrapper_does_not_change_source_identity_or_codec_paths() {
    // 归一后的键(小写、斜杠)在记录里原样出现,不是原始输入。
    let log = shared_log();
    let traced = TracingSource::new(build_source(), log);
    let raw = LookupKey::new(br"YSBIN\YSC.YBN", Codec::Cp932).unwrap();
    traced.open(&raw).unwrap();
    let log = traced.recorder().shared_log();
    let records = log.lock().unwrap();
    match records.records()[0].payload() {
        Payload::VfsOpen { key, .. } => assert_eq!(key.as_slice(), b"ysbin/ysc.ybn"),
        other => panic!("unexpected payload: {other:?}"),
    }
}

#[test]
fn shared_log_can_carry_a_header_and_parse_back() {
    let recorder = EventRecorder::with_header(header_for(vec!["ysc: v495 (105 命令)".into()]));
    let traced = TracingSource::from_recorder(build_source(), recorder);
    let key = LookupKey::new(b"ysbin/ysc.ybn", Codec::Cp932).unwrap();
    traced.open(&key).unwrap();
    let text = traced.recorder().snapshot().to_jsonl();
    let parsed = TraceLog::parse(&text).expect("trace parses");
    assert_eq!(parsed.len(), 2);
    assert_eq!(text, parsed.to_jsonl());
}

#[test]
fn aux_source_shares_the_primary_frame_handle() {
    let recorder = EventRecorder::new(shared_log());
    let primary = TracingSource::from_recorder(build_source(), recorder.clone());
    let frame = primary.frame_handle();
    let aux = TracingSource::from_recorder_with_frame(
        MemSource::new(Codec::Cp932),
        recorder.clone(),
        frame.clone(),
    );

    *frame.lock().unwrap() = Some(42);
    let key = LookupKey::new(b"ysbin/ysc.ybn", Codec::Cp932).unwrap();
    primary.exists(&key);
    aux.exists(&key);
    *frame.lock().unwrap() = None;

    let log = recorder.snapshot();
    let frames: Vec<Option<u64>> = log.records().iter().map(|record| record.frame()).collect();
    assert_eq!(frames, vec![Some(42), Some(42)]);
}
