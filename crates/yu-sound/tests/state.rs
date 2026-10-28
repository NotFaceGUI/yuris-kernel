//! `yu-sound` 状态机单测：手册示例 1..12 的值断言（不是听感断言）。

use std::io::Cursor;

use yu_rt::value::Value;
use yu_sound::decode::decode;
use yu_sound::loader::MemoryLoader;
use yu_sound::state::{normalize_name, probe_names, QueryField, SoundCommand, SoundState};
use yu_sound::{FileType, PlayState};

fn wav_ms(duration_ms: u32, rate: u32, channels: u16) -> Vec<u8> {
    let spec = hound::WavSpec {
        channels,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let frames = (u64::from(duration_ms) * u64::from(rate) / 1000) as usize;
    let mut out = Cursor::new(Vec::new());
    {
        let mut w = hound::WavWriter::new(&mut out, spec).expect("wav writer");
        for i in 0..frames {
            let v = ((i as f32 / 32.0).sin() * 12000.0) as i16;
            for _ in 0..channels {
                w.write_sample(v).expect("sample");
            }
        }
        w.finalize().expect("finalize");
    }
    out.into_inner()
}

/// 斜坡 WAV：第 i 帧第 ch 声道 = `(i * mult[ch]) as i16`（确定性、无削顶）。
fn ramp_wav(duration_ms: u32, rate: u32, mults: &[i64]) -> Vec<u8> {
    let spec = hound::WavSpec {
        channels: mults.len() as u16,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let frames = (u64::from(duration_ms) * u64::from(rate) / 1000) as usize;
    let mut out = Cursor::new(Vec::new());
    {
        let mut w = hound::WavWriter::new(&mut out, spec).expect("wav writer");
        for i in 0..frames {
            for m in mults {
                w.write_sample((i as i64 * m) as i16).expect("sample");
            }
        }
        w.finalize().expect("finalize");
    }
    out.into_inner()
}

fn state_with(files: &[(&str, Vec<u8>)]) -> SoundState {
    let mut loader = MemoryLoader::new();
    for (name, bytes) in files {
        loader.insert(name.as_bytes().to_vec(), bytes.clone());
    }
    SoundState::new(Box::new(loader))
}

fn cmd(id: &str) -> SoundCommand {
    SoundCommand {
        id: id.as_bytes().to_vec(),
        ..SoundCommand::default()
    }
}

fn query(state: &mut SoundState, id: &str, field: QueryField) -> Value {
    state.query(id.as_bytes(), &[field]).expect("query")
}

fn int(state: &mut SoundState, id: &str, field: QueryField) -> i64 {
    match query(state, id, field) {
        Value::Int(v) => v,
        other => panic!("expected int, got {other:?}"),
    }
}

#[test]
fn probe_order_is_engine_order() {
    assert_eq!(
        probe_names(b"bgm/bgm01"),
        vec![
            b"bgm/bgm01".to_vec(),
            b"bgm/bgm01.ogg".to_vec(),
            b"bgm/bgm01.wav".to_vec()
        ]
    );
    // 已带扩展名：不再追加（引擎查找序的第一项就是原样名字）。
    assert_eq!(probe_names(b"x.ogg"), vec![b"x.ogg".to_vec()]);
    // 后缀比较大小写不敏感，但不改写名字字节（`.OGG` 原样保留）。
    assert_eq!(probe_names(b"X.OGG"), vec![b"X.OGG".to_vec()]);
}

/// `コトネイロ` = `83 52 83 67 83 6C 83 43 83 8D`：尾字节 `0x52`(`'R'`)、
/// `0x43`(`'C'`) 落在 ASCII 大写区间。逐字节折小写会把它改写成
/// `ビトネツロ`(`83 72 83 63`)，包内存在的资产因此永远查不到。
#[test]
fn normalize_keeps_cp932_double_byte_characters_intact() {
    let cp932 = b"\x83\x52\x83\x67\x83\x6c\x83\x43\x83\x8d";
    let mut raw = b"VOICE\\".to_vec();
    raw.extend_from_slice(cp932);
    let mut want = b"voice/".to_vec();
    want.extend_from_slice(cp932);
    assert_eq!(normalize_name(&raw), want);

    // 尾字节 `0x5C`(`\`) 的双字节字符(`ソ`=`83 5C`)不能被当成分隔符。
    assert_eq!(
        normalize_name(b"VOICE\\\x83\x5c"),
        b"voice/\x83\x5c".to_vec()
    );
}

/// 回归护栏：**所有**尾字节落在 ASCII 大写区间(`0x41..=0x5A`)的 CP932
/// 双字节字符，以及尾字节恰为 `\`(`0x5C`) 的双字节字符，归一化后必须逐字节
/// 不变。逐字节折小写的实现在这里会成片失败。
#[test]
fn normalize_never_rewrites_double_byte_trail_bytes() {
    for lead in (0x81u8..=0x9f).chain(0xe0..=0xfc) {
        for trail in 0x41u8..=0x5a {
            let raw = [b'T', lead, trail, b'X'];
            // 只有两侧的单字节 ASCII 折小写；中间的双字节整对原样保留。
            let want = vec![b't', lead, trail, b'x'];
            assert_eq!(
                normalize_name(&raw),
                want,
                "lead={lead:#04x} trail={trail:#04x} 被改写"
            );
        }
        // 尾字节 0x5C 是 `\`，不能被折成路径分隔符。
        let raw = [b'T', lead, 0x5c];
        assert_eq!(
            normalize_name(&raw),
            vec![b't', lead, 0x5c],
            "lead={lead:#04x} trail=0x5c 被改写"
        );
    }
}

/// 失败诊断必须列出**归一化后实际尝试**的候选名，否则“名字被改写”与
/// “文件确实不存在”在日志里完全同形（本缺陷曾因此难以定位）。
#[test]
fn missing_diag_lists_normalized_candidates() {
    let mut s = SoundState::new(Box::new(MemoryLoader::new()));
    let mut c = cmd("V");
    c.file = Some(b"VOICE\\\x83\x52\x83\x67\x83\x6c\x83\x43\x83\x8d".to_vec());
    s.apply_sound(&c);

    let diags = s.diags();
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, "snd.file.missing");
    let detail = &diags[0].detail;
    assert!(
        detail.contains(r"voice/\x83R\x83g\x83l\x83C\x83\x8d.ogg"),
        "应列出归一化后的候选名: {detail}"
    );
    assert!(
        !detail.contains(r"\x83r"),
        "候选名里的双字节尾字节被改写: {detail}"
    );
}

/// 端到端：CP932 文件名经归一化 + 扩展名探测后必须命中真实资产。
#[test]
fn cp932_file_name_resolves_through_probe_order() {
    let cp932 = b"\x83\x52\x83\x67\x83\x6c\x83\x43\x83\x8d";
    let mut resolved = b"voice/".to_vec();
    resolved.extend_from_slice(cp932);
    resolved.extend_from_slice(b".wav");
    let mut loader = MemoryLoader::new();
    loader.insert(resolved, wav_ms(100, 1000, 1));
    let mut s = SoundState::new(Box::new(loader));

    let mut c = cmd("V");
    let mut file = b"voice/".to_vec();
    file.extend_from_slice(cp932);
    c.file = Some(file);
    c.play = Some(1);
    s.apply_sound(&c);

    assert_eq!(int(&mut s, "V", QueryField::Exist), 1);
    assert_eq!(int(&mut s, "V", QueryField::TotalTime), 100);
    assert!(
        s.diags().is_empty(),
        "不应产生诊断: {:?}",
        s.diags().iter().map(|d| &d.detail).collect::<Vec<_>>()
    );
}

#[test]
fn example1_infinite_loop_and_time() {
    let mut s = state_with(&[("bgm01.wav", wav_ms(1000, 1000, 1))]);
    let mut c = cmd("BGM");
    c.file = Some(b"bgm01".to_vec());
    c.loop_ = Some(0);
    c.play = Some(1);
    s.apply_sound(&c);

    assert_eq!(int(&mut s, "BGM", QueryField::Exist), 1);
    assert_eq!(int(&mut s, "BGM", QueryField::TotalTime), 1000);
    assert_eq!(int(&mut s, "BGM", QueryField::Play), 1);
    assert_eq!(int(&mut s, "BGM", QueryField::Loop), 0);

    s.advance_ms(500);
    assert_eq!(int(&mut s, "BGM", QueryField::Time), 500);
    s.advance_ms(1000);
    // 到达 END：LOOPCOUNT +1，跳回 RESTART=0。
    assert_eq!(int(&mut s, "BGM", QueryField::LoopCount), 1);
    assert_eq!(int(&mut s, "BGM", QueryField::Time), 0);
    s.advance_ms(2000);
    assert_eq!(int(&mut s, "BGM", QueryField::LoopCount), 2);
    assert_eq!(int(&mut s, "BGM", QueryField::Play), 1);
}

#[test]
fn example2_fade_in_and_three_plays() {
    let mut s = state_with(&[("abc.wav", wav_ms(1000, 1000, 1))]);
    let mut c = cmd("BGM");
    c.file = Some(b"ABC.WAV".to_vec());
    c.fade = Some(1500);
    c.loop_ = Some(3);
    c.play = Some(1);
    s.apply_sound(&c);

    s.advance_ms(750);
    // 线性淡入：0.5 秒 / 1.5 秒 = 50% × 256。
    assert!((int(&mut s, "BGM", QueryField::Volume) - 128).abs() <= 1);
    s.advance_ms(1500);
    assert_eq!(int(&mut s, "BGM", QueryField::Volume), 256);

    // 跨 END 的推进不携带余量（引擎把位置直接置为 RESTART）。
    assert_eq!(int(&mut s, "BGM", QueryField::LoopCount), 1);
    s.advance_ms(1000);
    assert_eq!(int(&mut s, "BGM", QueryField::LoopCount), 2);
    assert_eq!(int(&mut s, "BGM", QueryField::Play), 1);
    s.advance_ms(1000);
    assert_eq!(int(&mut s, "BGM", QueryField::Play), 0);
    assert_eq!(int(&mut s, "BGM", QueryField::Loop), 0);
    assert_eq!(int(&mut s, "BGM", QueryField::LoopCount), 0);
    assert_eq!(int(&mut s, "BGM", QueryField::Time), 0);
}

#[test]
fn example3_start_at_3200() {
    let mut s = state_with(&[("abc.wav", wav_ms(5000, 1000, 1))]);
    let mut c = cmd("BGM");
    c.file = Some(b"ABC.WAV".to_vec());
    c.start = Some(3200);
    c.play = Some(1);
    s.apply_sound(&c);
    assert_eq!(int(&mut s, "BGM", QueryField::Time), 3200);
    s.advance_ms(100);
    assert_eq!(int(&mut s, "BGM", QueryField::Time), 3300);
    assert_eq!(int(&mut s, "BGM", QueryField::Start), 3200);
}

#[test]
fn example4_restart_from_8000() {
    let mut s = state_with(&[("abc.wav", wav_ms(10000, 1000, 1))]);
    let mut c = cmd("BGM");
    c.file = Some(b"ABC.WAV".to_vec());
    c.start = Some(3200);
    c.restart = Some(8000);
    c.play = Some(1);
    s.apply_sound(&c);
    s.advance_ms(10_000); // 3200 → 10000(END) → RESTART 8000
    assert_eq!(int(&mut s, "BGM", QueryField::Time), 8000);
    assert_eq!(int(&mut s, "BGM", QueryField::LoopCount), 1);
    assert_eq!(int(&mut s, "BGM", QueryField::Restart), 8000);
}

#[test]
fn example6_pan_and_example7_pitch() {
    let mut s = state_with(&[("abc.wav", wav_ms(2000, 1000, 1))]);
    let mut c = cmd("BGM");
    c.file = Some(b"ABC.WAV".to_vec());
    c.pan = Some(512);
    c.pitch = Some(512);
    c.play = Some(1);
    s.apply_sound(&c);
    assert_eq!(int(&mut s, "BGM", QueryField::Pan), 512);
    assert_eq!(int(&mut s, "BGM", QueryField::Pitch), 512);
    s.advance_ms(1000);
    // 1.5 倍速：1 秒墙钟 → 1500ms 播放位置（引擎 SetFrequency 语义）。
    assert_eq!(int(&mut s, "BGM", QueryField::Time), 1500);
}

#[test]
fn example8_two_layers_at_once() {
    let mut s = state_with(&[
        ("1.wav", wav_ms(1000, 1000, 1)),
        ("2.wav", wav_ms(1000, 1000, 1)),
    ]);
    let mut a = cmd("BGM");
    a.file = Some(b"1.ogg".to_vec());
    let mut b = cmd("BGM2");
    b.file = Some(b"2.ogg".to_vec());
    // 只有 .WAV 夹具：探测序会试 1.ogg → 1.ogg.OGG/1.ogg.WAV，均不存在。
    // 用不带扩展名的名字验证双轨。
    a.file = Some(b"1".to_vec());
    b.file = Some(b"2".to_vec());
    s.apply_sound(&a);
    s.apply_sound(&b);
    let mut pa = cmd("BGM");
    pa.play = Some(1);
    let mut pb = cmd("BGM2");
    pb.play = Some(1);
    s.apply_sound(&pa);
    s.apply_sound(&pb);
    s.advance_ms(300);
    assert_eq!(int(&mut s, "BGM", QueryField::Time), 300);
    assert_eq!(int(&mut s, "BGM2", QueryField::Time), 300);
    assert_eq!(s.layers_len(), 2);
}

#[test]
fn example9_e0_is_mute_not_stop() {
    let mut s = state_with(&[("abc.wav", wav_ms(1000, 1000, 1))]);
    let mut c = cmd("BGM");
    c.file = Some(b"ABC.WAV".to_vec());
    c.play = Some(1);
    s.apply_sound(&c);
    let mut mute = cmd("BGM");
    mute.e = Some(0);
    s.apply_sound(&mute);
    assert_eq!(int(&mut s, "BGM", QueryField::E), 0);
    assert_eq!(int(&mut s, "BGM", QueryField::Play), 1);
    assert_eq!(s.layer(b"BGM").unwrap().audible_volume(), 0);
    s.advance_ms(400);
    assert_eq!(int(&mut s, "BGM", QueryField::Time), 400);
    assert_eq!(int(&mut s, "BGM", QueryField::Volume), 256);
}

#[test]
fn example10_11_pause_freezes_time() {
    let mut s = state_with(&[("abc.wav", wav_ms(2000, 1000, 1))]);
    let mut c = cmd("BGM");
    c.file = Some(b"ABC.WAV".to_vec());
    c.play = Some(1);
    s.apply_sound(&c);
    s.advance_ms(300);
    let mut pause = cmd("BGM");
    pause.pause = Some(1);
    s.apply_sound(&pause);
    s.advance_ms(1000);
    assert_eq!(int(&mut s, "BGM", QueryField::Time), 300);
    assert_eq!(int(&mut s, "BGM", QueryField::Pause), 1);
    let mut resume = cmd("BGM");
    resume.pause = Some(0);
    s.apply_sound(&resume);
    s.advance_ms(300);
    assert_eq!(int(&mut s, "BGM", QueryField::Time), 600);
    assert_eq!(int(&mut s, "BGM", QueryField::Pause), 0);
}

#[test]
fn example12_fade_to_target_volume() {
    let mut s = state_with(&[("abc.wav", wav_ms(5000, 1000, 1))]);
    let mut c = cmd("BGM");
    c.file = Some(b"ABC.WAV".to_vec());
    c.volume = Some(256);
    c.play = Some(1);
    s.apply_sound(&c);
    s.advance_ms(1000);
    let mut fade = cmd("BGM");
    fade.fade = Some(1000);
    fade.volume = Some(128);
    fade.play = Some(1);
    s.apply_sound(&fade);
    s.advance_ms(500);
    assert!((int(&mut s, "BGM", QueryField::Volume) - 192).abs() <= 1);
    s.advance_ms(1000);
    assert_eq!(int(&mut s, "BGM", QueryField::Volume), 128);
}

#[test]
fn soundend_removes_layer() {
    let mut s = state_with(&[("abc.wav", wav_ms(1000, 1000, 1))]);
    let mut c = cmd("BGM");
    c.file = Some(b"ABC.WAV".to_vec());
    c.play = Some(1);
    s.apply_sound(&c);
    assert_eq!(int(&mut s, "BGM", QueryField::Exist), 1);
    assert!(s.remove_layer(b"BGM"));
    assert_eq!(int(&mut s, "BGM", QueryField::Exist), 0);
    assert_eq!(int(&mut s, "BGM", QueryField::Time), 0);
    assert_eq!(s.layers_len(), 0);
}

#[test]
fn missing_layer_clears_let_to_zero() {
    let mut s = state_with(&[]);
    assert_eq!(int(&mut s, "NOPE", QueryField::Play), 0);
    assert_eq!(int(&mut s, "NOPE", QueryField::TotalTime), 0);
    match query(&mut s, "NOPE", QueryField::File) {
        Value::Str(v) => assert!(v.is_empty()),
        other => panic!("expected empty string, got {other:?}"),
    }
}

#[test]
fn file_probe_prefers_ogg_then_wav() {
    let mut s = state_with(&[
        ("x.ogg", wav_ms(10, 1000, 1)),
        ("x.wav", wav_ms(20, 1000, 1)),
    ]);
    let mut c = cmd("A");
    c.file = Some(b"x".to_vec());
    s.apply_sound(&c);
    // 第一个候选 "x" 不存在，第二个 "x.ogg" 存在（WAV 内容按魔数解码）。
    assert_eq!(s.layers_len(), 1);
    let layer = s.layer(b"A").unwrap();
    assert_eq!(layer.file_type(), FileType::Wav);
    // REA `0x40c9b4`：`+0x000` 是最终候选名（含扩展名、已小写）。
    assert_eq!(
        query(&mut s, "A", QueryField::File),
        Value::Str(b"x.ogg".to_vec())
    );
}

#[test]
fn wav_decode_metadata() {
    let bytes = wav_ms(250, 8000, 2);
    let pcm = decode(b"x.wav", &bytes).expect("decode");
    assert_eq!(pcm.channels, 2);
    assert_eq!(pcm.bits, 16);
    assert_eq!(pcm.sample_rate, 8000);
    assert_eq!(pcm.total_ms(), 250);
    assert_eq!(pcm.codec.value(), 0);
}

#[test]
fn gated_ogg_decode_from_sdk_sample() {
    let Ok(root) = std::env::var("YURIS_SAMPLE_SDK0495") else {
        eprintln!("SKIP snd-ogg: YURIS_SAMPLE_SDK0495 not set");
        return;
    };
    let root = std::path::Path::new(&root);
    let candidates = [
        root.join("data/bgm/bgm03.ogg"),
        root.join("システム/data/bgm/bgm03.ogg"),
    ];
    let Some(path) = candidates.iter().find(|p| p.is_file()) else {
        eprintln!("SKIP snd-ogg: bgm03.ogg not under {}", root.display());
        return;
    };
    let bytes = std::fs::read(path).expect("read bgm03.ogg");
    let pcm = decode(b"bgm03.ogg", &bytes).expect("ogg decode");
    assert_eq!(pcm.codec, yu_sound::decode::Codec::Ogg);
    assert!(pcm.channels >= 1);
    assert!(pcm.sample_rate >= 8000);
    assert!(pcm.total_ms() > 1000);
}

#[test]
fn render_is_deterministic_and_respects_volume() {
    let mut a = state_with(&[("x.wav", wav_ms(1000, 8000, 1))]);
    let mut b = state_with(&[("x.wav", wav_ms(1000, 8000, 1))]);
    for s in [&mut a, &mut b] {
        let mut c = cmd("BGM");
        c.file = Some(b"x".to_vec());
        c.play = Some(1);
        s.apply_sound(&c);
        s.advance_ms(100);
    }
    let mut out_a = vec![0.0f32; 512];
    let mut out_b = vec![0.0f32; 512];
    a.render(&mut out_a, 8000, 1);
    b.render(&mut out_b, 8000, 1);
    assert_eq!(
        out_a.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
        out_b.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
    );
    assert!(out_a.iter().any(|v| *v != 0.0));

    // E=0：同一位置渲染应全零（静音）。
    let mut mute = cmd("BGM");
    mute.e = Some(0);
    a.apply_sound(&mute);
    let mut out_mute = vec![0.0f32; 512];
    a.render(&mut out_mute, 8000, 1);
    assert!(out_mute.iter().all(|v| *v == 0.0));
}

#[test]
fn state_is_steadily_exposed_for_loop_snapshot() {
    let mut s = state_with(&[("x.wav", wav_ms(1000, 1000, 1))]);
    let mut c = cmd("BGM");
    c.file = Some(b"x".to_vec());
    c.play = Some(1);
    s.apply_sound(&c);
    assert_eq!(s.layer(b"BGM").map(|l| l.state()), Some(PlayState::Playing));
}

#[test]
fn start_alone_does_not_move_time() {
    let mut s = state_with(&[("abc.wav", wav_ms(5000, 1000, 1))]);
    let mut load = cmd("BGM");
    load.file = Some(b"abc.wav".to_vec());
    s.apply_sound(&load);
    let mut start = cmd("BGM");
    start.start = Some(3200);
    s.apply_sound(&start);
    // 引擎 type 9 只写 +0x128，不碰 +0x118（TIME）。
    assert_eq!(int(&mut s, "BGM", QueryField::Time), 0);
    assert_eq!(int(&mut s, "BGM", QueryField::Start), 3200);
    let mut play = cmd("BGM");
    play.play = Some(1);
    s.apply_sound(&play);
    assert_eq!(int(&mut s, "BGM", QueryField::Time), 3200);
}

#[test]
fn play_and_pause_same_command_ends_paused() {
    let mut s = state_with(&[("abc.wav", wav_ms(1000, 1000, 1))]);
    let mut c = cmd("BGM");
    c.file = Some(b"abc.wav".to_vec());
    c.play = Some(1);
    c.pause = Some(1);
    s.apply_sound(&c);
    // 引擎顺序：PLAY 先、PAUSE 后；PLAY 不清 +0x109。
    assert_eq!(int(&mut s, "BGM", QueryField::Play), 1);
    assert_eq!(int(&mut s, "BGM", QueryField::Pause), 1);
    s.advance_ms(500);
    assert_eq!(int(&mut s, "BGM", QueryField::Time), 0);
}

#[test]
fn start_restart_end_use_engine_bounds() {
    let mut s = state_with(&[("abc.wav", wav_ms(1000, 1000, 1))]);
    let mut load = cmd("BGM");
    load.file = Some(b"abc.wav".to_vec());
    s.apply_sound(&load);
    // START/RESTART >= total → 0；END > total → total。
    let mut c = cmd("BGM");
    c.start = Some(1000);
    c.restart = Some(9999);
    c.end = Some(5000);
    s.apply_sound(&c);
    assert_eq!(int(&mut s, "BGM", QueryField::Start), 0);
    assert_eq!(int(&mut s, "BGM", QueryField::Restart), 0);
    assert_eq!(int(&mut s, "BGM", QueryField::End), 1000);
}

#[test]
fn play_resets_start_when_end_is_not_after_start() {
    let mut s = state_with(&[("abc.wav", wav_ms(1000, 1000, 1))]);
    let mut c = cmd("BGM");
    c.file = Some(b"abc.wav".to_vec());
    c.start = Some(500);
    c.end = Some(400);
    c.play = Some(1);
    s.apply_sound(&c);
    // 引擎 type 2 state 0：`if (END <= START) START = 0;`（0x424950）。
    assert_eq!(int(&mut s, "BGM", QueryField::Start), 0);
    assert_eq!(int(&mut s, "BGM", QueryField::Time), 0);
}

#[test]
fn volume_without_play_applies_immediately() {
    let mut s = state_with(&[("abc.wav", wav_ms(1000, 1000, 1))]);
    let mut play = cmd("BGM");
    play.file = Some(b"abc.wav".to_vec());
    play.play = Some(1);
    s.apply_sound(&play);
    s.advance_ms(100);

    // 无 PLAY：引擎 type 5 立即写 +0x148/+0x150（0x42511c），不停止播放。
    let mut vol = cmd("BGM");
    vol.volume = Some(128);
    s.apply_sound(&vol);
    assert_eq!(int(&mut s, "BGM", QueryField::Volume), 128);
    assert_eq!(int(&mut s, "BGM", QueryField::Play), 1);
}

#[test]
fn volume_with_play_fades_to_new_target() {
    let mut s = state_with(&[("abc.wav", wav_ms(1000, 1000, 1))]);
    let mut play = cmd("BGM");
    play.file = Some(b"abc.wav".to_vec());
    play.volume = Some(256);
    play.play = Some(1);
    s.apply_sound(&play);

    // 手册例 12：播放中 FADE=1000 VOLUME=128 PLAY=1。
    let mut fade = cmd("BGM");
    fade.fade = Some(1000);
    fade.volume = Some(128);
    fade.play = Some(1);
    s.apply_sound(&fade);
    s.advance_ms(500);
    assert!((int(&mut s, "BGM", QueryField::Volume) - 192).abs() <= 1);
    s.advance_ms(500);
    assert_eq!(int(&mut s, "BGM", QueryField::Volume), 128);
    assert_eq!(int(&mut s, "BGM", QueryField::Play), 1);
}

#[test]
fn load_resets_pan_pitch_start_restart_but_keeps_volume_and_loop() {
    let mut s = state_with(&[
        ("a.wav", wav_ms(1000, 1000, 1)),
        ("b.wav", wav_ms(1000, 1000, 1)),
    ]);
    let mut first = cmd("BGM");
    first.file = Some(b"a.wav".to_vec());
    first.pan = Some(512);
    first.pitch = Some(512);
    first.start = Some(200);
    first.restart = Some(100);
    first.volume = Some(200);
    first.loop_ = Some(2);
    first.play = Some(1);
    s.apply_sound(&first);
    assert_eq!(int(&mut s, "BGM", QueryField::Pan), 512);
    assert_eq!(int(&mut s, "BGM", QueryField::Pitch), 512);

    // type 1（0x424614）：重装时 PAN/PITCH/START/RESTART/位置清零；
    // E/VOLUME/LOOP 保留。
    let mut second = cmd("BGM");
    second.file = Some(b"b.wav".to_vec());
    s.apply_sound(&second);
    assert_eq!(int(&mut s, "BGM", QueryField::Pan), 256);
    assert_eq!(int(&mut s, "BGM", QueryField::Pitch), 256);
    assert_eq!(int(&mut s, "BGM", QueryField::Start), 0);
    assert_eq!(int(&mut s, "BGM", QueryField::Restart), 0);
    assert_eq!(int(&mut s, "BGM", QueryField::Time), 0);
    assert_eq!(int(&mut s, "BGM", QueryField::Volume), 200);
    assert_eq!(int(&mut s, "BGM", QueryField::Loop), 2);
}

#[test]
fn loop_count_survives_stop_and_play() {
    let mut s = state_with(&[("abc.wav", wav_ms(1000, 1000, 1))]);
    let mut c = cmd("BGM");
    c.file = Some(b"abc.wav".to_vec());
    c.loop_ = Some(0);
    c.play = Some(1);
    s.apply_sound(&c);
    s.advance_ms(1000);
    s.advance_ms(1000);
    assert_eq!(int(&mut s, "BGM", QueryField::LoopCount), 2);

    // type 4/2 都不写 +0x110（只有 0x464960 的循环边界写）。
    let mut stop = cmd("BGM");
    stop.play = Some(0);
    s.apply_sound(&stop);
    let mut resume = cmd("BGM");
    resume.play = Some(1);
    s.apply_sound(&resume);
    assert_eq!(int(&mut s, "BGM", QueryField::LoopCount), 2);
    assert_eq!(int(&mut s, "BGM", QueryField::Play), 1);
}

#[test]
fn play_during_fade_out_stops_like_engine_state4_branch() {
    let mut s = state_with(&[("abc.wav", wav_ms(1000, 1000, 1))]);
    let mut play = cmd("BGM");
    play.file = Some(b"abc.wav".to_vec());
    play.play = Some(1);
    s.apply_sound(&play);
    let mut stop = cmd("BGM");
    stop.fade = Some(1000);
    stop.play = Some(0);
    s.apply_sound(&stop);
    s.advance_ms(500);
    assert_eq!(int(&mut s, "BGM", QueryField::Play), 1);

    // 引擎 type 2 的 state==4 分支：立即停止（0x424950）。
    let mut play_again = cmd("BGM");
    play_again.play = Some(1);
    s.apply_sound(&play_again);
    assert_eq!(int(&mut s, "BGM", QueryField::Play), 0);
}

#[test]
fn multi_field_query_short_circuits_early_fields() {
    let mut s = state_with(&[("abc.wav", wav_ms(1000, 1000, 1))]);
    let mut c = cmd("BGM");
    c.file = Some(b"abc.wav".to_vec());
    c.play = Some(1);
    s.apply_sound(&c);
    s.advance_ms(500);

    // EXIST..PITCH 按固定顺序首个命中即返回（REA 0x44f4bc 的独立 ret）。
    let v = s
        .query(
            b"BGM",
            &[QueryField::Time, QueryField::Pitch, QueryField::TotalTime],
        )
        .expect("query");
    assert_eq!(v, Value::Int(256)); // PITCH 默认值，不是 TIME=500

    let v = s
        .query(b"BGM", &[QueryField::Time, QueryField::TotalTime])
        .expect("query");
    // 不含短路字段时按顺序全部写，最后 TOTALTIME 胜出。
    assert_eq!(v, Value::Int(1000));
}

#[test]
fn file_query_returns_resolved_lowercase_name() {
    let mut s = state_with(&[("bgm01.wav", wav_ms(100, 1000, 1))]);
    let mut c = cmd("BGM");
    c.file = Some(b"BGM01".to_vec());
    s.apply_sound(&c);
    assert_eq!(
        query(&mut s, "BGM", QueryField::File),
        Value::Str(b"bgm01.wav".to_vec())
    );
}

#[test]
fn missing_file_unloads_clears_name_but_keeps_metadata() {
    let mut s = state_with(&[("abc.wav", wav_ms(500, 1000, 1))]);
    let mut ok = cmd("BGM");
    ok.file = Some(b"abc.wav".to_vec());
    ok.play = Some(1);
    s.apply_sound(&ok);
    s.advance_ms(100);
    assert_eq!(int(&mut s, "BGM", QueryField::Exist), 1);

    // 无候选文件：引擎先 type 0xe 卸载（清文件名/START/RESTART、释放 PCM、
    // state=0），但保留 +0x105/+0x107/+0x120/+0x118；随后 SOUND 处理体
    // 直接返回，PLAY/VOLUME 等参数不应用。
    let mut bad = cmd("BGM");
    bad.file = Some(b"NOPE".to_vec());
    bad.play = Some(1);
    bad.volume = Some(0);
    s.apply_sound(&bad);
    assert_eq!(int(&mut s, "BGM", QueryField::Exist), 1);
    assert_eq!(int(&mut s, "BGM", QueryField::Play), 0);
    assert_eq!(int(&mut s, "BGM", QueryField::TotalTime), 500);
    assert_eq!(int(&mut s, "BGM", QueryField::Time), 100);
    assert_eq!(int(&mut s, "BGM", QueryField::Start), 0);
    assert_eq!(int(&mut s, "BGM", QueryField::Volume), 256);
    assert_eq!(
        query(&mut s, "BGM", QueryField::File),
        Value::Str(Vec::new())
    );
}

#[test]
fn missing_file_on_stopped_layer_changes_nothing() {
    let mut s = state_with(&[("abc.wav", wav_ms(500, 1000, 1))]);
    let mut ok = cmd("BGM");
    ok.file = Some(b"abc.wav".to_vec());
    ok.play = Some(1);
    s.apply_sound(&ok);
    s.advance_ms(50);
    let mut stop = cmd("BGM");
    stop.play = Some(0);
    s.apply_sound(&stop);

    let mut bad = cmd("BGM");
    bad.file = Some(b"NOPE".to_vec());
    s.apply_sound(&bad);
    // state==0 时连 type 0xe 都不发：原文件名/元数据/位置完全不变。
    assert_eq!(
        query(&mut s, "BGM", QueryField::File),
        Value::Str(b"abc.wav".to_vec())
    );
    assert_eq!(int(&mut s, "BGM", QueryField::Exist), 1);
    assert_eq!(int(&mut s, "BGM", QueryField::TotalTime), 500);
    assert_eq!(int(&mut s, "BGM", QueryField::Time), 50);
    assert_eq!(int(&mut s, "BGM", QueryField::Play), 0);
}

#[test]
fn volume_gain_matches_dsbvolume_curve() {
    assert!((yu_sound::mixer::volume_gain(256) - 1.0).abs() < 1e-6);
    assert!((yu_sound::mixer::volume_gain(128) - 0.25).abs() < 1e-6);
    assert!((yu_sound::mixer::volume_gain(64) - 0.0625).abs() < 1e-6);
    assert_eq!(yu_sound::mixer::volume_gain(0), 0.0);
}

fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811c9dc5;
    for &b in bytes {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x01000193);
    }
    h
}

/// Golden：循环状态时间线 + render 快照哈希（不含音频文件/听感）。
#[test]
fn golden_loop_timeline_and_render_hash() {
    let fixture = || state_with(&[("abc.wav", wav_ms(1000, 1000, 1))]);
    let start = |s: &mut SoundState| {
        let mut c = cmd("BGM");
        c.file = Some(b"abc.wav".to_vec());
        c.loop_ = Some(2);
        c.play = Some(1);
        s.apply_sound(&c);
    };

    let mut s = fixture();
    start(&mut s);
    let mut out = String::new();
    let mut now = 0u32;
    for target in [0u32, 250, 500, 750, 1000, 1250, 1500, 2000] {
        s.advance_ms(target - now);
        now = target;
        out.push_str(&format!(
            "ms={target} time={} loop={} count={} play={} volume={}\n",
            int(&mut s, "BGM", QueryField::Time),
            int(&mut s, "BGM", QueryField::Loop),
            int(&mut s, "BGM", QueryField::LoopCount),
            int(&mut s, "BGM", QueryField::Play),
            int(&mut s, "BGM", QueryField::Volume),
        ));
    }

    let mut r = fixture();
    start(&mut r);
    r.advance_ms(250);
    let mut buf = vec![0.0f32; 256];
    r.render(&mut buf, 1000, 1);
    let bytes: Vec<u8> = buf.iter().flat_map(|v| v.to_bits().to_le_bytes()).collect();
    out.push_str(&format!(
        "render ms=250 frames=256 fnv1a32=0x{:08x}\n",
        fnv1a32(&bytes)
    ));

    let golden = include_str!("golden/loop_timeline.txt");
    assert_eq!(out, golden);
}

#[test]
fn dataleft_dataright_read_play_cursor_frame() {
    // REA 0x44fe3c/0x44ff58（手册无页，fixed(no-manual)）：
    // LEFT = 播放光标帧首采样的 |abs|；单声道 RIGHT = -1；
    // **16-bit 立体声 RIGHT 官方读 `ptr+4` = 下一帧左声道**（越锁区怪癖）。
    let mut mono = state_with(&[("m.wav", ramp_wav(1000, 1000, &[3]))]);
    let mut c = cmd("BGM");
    c.file = Some(b"m.wav".to_vec());
    c.play = Some(1);
    mono.apply_sound(&c);
    assert_eq!(int(&mut mono, "BGM", QueryField::DataLeft), 0);
    mono.advance_ms(100);
    // 第 100 帧 = 300。
    assert_eq!(int(&mut mono, "BGM", QueryField::DataLeft), 300);
    assert_eq!(int(&mut mono, "BGM", QueryField::DataRight), -1);

    let mut stereo = state_with(&[("s.wav", ramp_wav(1000, 1000, &[3, 7]))]);
    let mut c = cmd("BGM");
    c.file = Some(b"s.wav".to_vec());
    c.play = Some(1);
    stereo.apply_sound(&c);
    stereo.advance_ms(100);
    assert_eq!(int(&mut stereo, "BGM", QueryField::DataLeft), 300);
    // +4 字节 = 下一帧（101）左声道 = 303；不是同帧右声道 700。
    assert_eq!(int(&mut stereo, "BGM", QueryField::DataRight), 303);

    // 缺层走引擎缺层分支：LET 写 0（不是 -1）。
    let mut empty = state_with(&[]);
    assert_eq!(int(&mut empty, "NOPE", QueryField::DataLeft), 0);
}

#[test]
fn acm_wave_roundtrip_through_system_adpcm() {
    // 官方 `0x422e04` 非 PCM 分支 1:1：宿主 MSACM32 编解码（DIV-SND-004）。
    // 用系统 MS-ADPCM 编码器把 PCM 压成 CODEC=2 形态，再走 decode() 解回。
    if !cfg!(windows) {
        eprintln!("SKIP snd-acm: win32 ACM only");
        return;
    }
    const RATE: u32 = 8000;
    const MS: u32 = 250;
    let pcm_ms = wav_ms(MS, RATE, 1);
    // 源 PCM WAVEFORMATEX（18 字节）。
    let mut src_fmt: Vec<u8> = Vec::new();
    src_fmt.extend_from_slice(&1u16.to_le_bytes()); // PCM
    src_fmt.extend_from_slice(&1u16.to_le_bytes()); // mono
    src_fmt.extend_from_slice(&(RATE as u16).to_le_bytes());
    src_fmt.extend_from_slice(&(RATE * 2).to_le_bytes());
    src_fmt.extend_from_slice(&2u16.to_le_bytes()); // block align
    src_fmt.extend_from_slice(&16u16.to_le_bytes()); // bits
    src_fmt.extend_from_slice(&0u16.to_le_bytes()); // cbSize
    // MS-ADPCM 目标格式（tag=2，blockAlign=128，cbSize=32 扩展含标准系数表）。
    let coef: [i16; 14] = [256, 0, 512, -256, 0, 0, 192, 64, 240, 0, 460, -208, 392, -232];
    let block_align: u16 = 128;
    let samples_per_block: u16 = (block_align - 7) * 2 + 2;
    let avg_bytes = u32::from(RATE) / u32::from(block_align)
        * u32::from(samples_per_block)
        * u32::from(block_align / 2);
    let mut adpcm_fmt: Vec<u8> = Vec::new();
    adpcm_fmt.extend_from_slice(&2u16.to_le_bytes());
    adpcm_fmt.extend_from_slice(&1u16.to_le_bytes()); // mono
    adpcm_fmt.extend_from_slice(&(RATE as u16).to_le_bytes());
    adpcm_fmt.extend_from_slice(&avg_bytes.to_le_bytes());
    adpcm_fmt.extend_from_slice(&block_align.to_le_bytes());
    adpcm_fmt.extend_from_slice(&4u16.to_le_bytes()); // bits
    adpcm_fmt.extend_from_slice(&32u16.to_le_bytes()); // cbSize
    adpcm_fmt.extend_from_slice(&samples_per_block.to_le_bytes());
    adpcm_fmt.extend_from_slice(&7u16.to_le_bytes()); // wNumCoef
    for c in coef {
        adpcm_fmt.extend_from_slice(&c.to_le_bytes());
    }
    for c in coef {
        adpcm_fmt.extend_from_slice(&c.to_le_bytes());
    }
    // 取 data chunk（hound 产物按 RIFF 布局找 data）。
    let data = wav_data_chunk(&pcm_ms).expect("data chunk");
    let encoded = match yu_sound::acm::convert(&src_fmt, &adpcm_fmt, &data) {
        Ok(v) => v,
        Err(d) => {
            eprintln!("SKIP snd-acm: 系统无 MS-ADPCM 编码器（{d}）");
            return;
        }
    };
    // 拼回 RIFF WAVE（fmt 40 字节 + data）。
    let mut wav: Vec<u8> = b"RIFF".to_vec();
    let riff_size = 4 + 8 + adpcm_fmt.len() + 8 + encoded.len();
    wav.extend_from_slice(&(riff_size as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&(adpcm_fmt.len() as u32).to_le_bytes());
    wav.extend_from_slice(&adpcm_fmt);
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(encoded.len() as u32).to_le_bytes());
    wav.extend_from_slice(&encoded);

    let pcm = decode(b"probe.wav", &wav).expect("acm decode");
    assert_eq!(pcm.codec, yu_sound::decode::Codec::Acm);
    assert_eq!(pcm.channels, 1);
    assert_eq!(pcm.sample_rate, RATE);
    assert_eq!(pcm.bits, 16);
    // ADPCM 有损：结构对齐 + 粗粒度保真界（静音会让峰值误差≈12000）。
    let frame0 = u64::from(MS) * u64::from(RATE) / 1000;
    let drift_ms = (pcm.frames() as i64 - frame0 as i64).abs() * 1000 / i64::from(RATE);
    assert!(drift_ms <= 40, "时长漂移过大: {drift_ms}ms");
    let original = wav_samples_i16(&pcm_ms);
    let max_err = pcm
        .samples
        .iter()
        .zip(original.iter())
        .map(|(a, b)| i32::from(*a - *b).unsigned_abs())
        .max()
        .unwrap_or(0);
    assert!(max_err < 6000, "MS-ADPCM 往返误差过大: {max_err}");
}

/// 从 hound 产物取 `data` chunk 字节。
fn wav_data_chunk(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut off = 12usize;
    while off + 8 <= bytes.len() {
        let id = &bytes[off..off + 4];
        let size = u32::from_le_bytes([
            bytes[off + 4],
            bytes[off + 5],
            bytes[off + 6],
            bytes[off + 7],
        ]) as usize;
        if id == b"data" {
            return Some(bytes[off + 8..off + 8 + size].to_vec());
        }
        off += 8 + size + (size & 1);
    }
    None
}

fn wav_samples_i16(bytes: &[u8]) -> Vec<i16> {
    wav_data_chunk(bytes)
        .expect("data chunk")
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect()
}

#[test]
fn example5_negative_start_delays_with_silence() {
    // 手册例 5（负 START = 遅延再生）+ REA `0x423f9c/0x424424`：延迟以
    // 静音区间兑现，期间 TIME 冻结在 0、PLAY 保持 1（Likely，DIV-SND-002；
    // 动态曲线归 DIV-SND-003 校准）。
    let mut s = state_with(&[("abc.wav", wav_ms(5000, 1000, 1))]);
    let mut c = cmd("BGM");
    c.file = Some(b"abc.wav".to_vec());
    c.start = Some(-1000);
    c.play = Some(1);
    s.apply_sound(&c);
    assert_eq!(int(&mut s, "BGM", QueryField::Start), -1000);
    assert_eq!(int(&mut s, "BGM", QueryField::Time), 0);
    assert_eq!(int(&mut s, "BGM", QueryField::Play), 1);

    s.advance_ms(999);
    assert_eq!(int(&mut s, "BGM", QueryField::Time), 0);
    s.advance_ms(1); // 延迟恰好烧完；本 tick 不推进位置。
    assert_eq!(int(&mut s, "BGM", QueryField::Time), 0);
    s.advance_ms(500);
    assert_eq!(int(&mut s, "BGM", QueryField::Time), 500);
    // 延迟是已实现的官方语义，不再产生任何挂号诊断。
    assert!(!s.diags().iter().any(|d| d.code == "snd.delay.unimplemented"));
    assert!(s.diags().iter().all(|d| d.severity != yu_log::Severity::Unimplemented));
}

#[test]
fn negative_restart_delays_each_loop_wrap() {
    let mut s = state_with(&[("abc.wav", wav_ms(1000, 1000, 1))]);
    let mut c = cmd("BGM");
    c.file = Some(b"abc.wav".to_vec());
    c.loop_ = Some(0);
    c.restart = Some(-500);
    c.play = Some(1);
    s.apply_sound(&c);
    // 到 END 边界：LOOPCOUNT+1，RESTART=-500 ⇒ 回 0 并静音 500ms。
    s.advance_ms(1000);
    assert_eq!(int(&mut s, "BGM", QueryField::LoopCount), 1);
    assert_eq!(int(&mut s, "BGM", QueryField::Time), 0);
    s.advance_ms(499);
    assert_eq!(int(&mut s, "BGM", QueryField::Time), 0);
    s.advance_ms(1);
    assert_eq!(int(&mut s, "BGM", QueryField::Time), 0);
    s.advance_ms(250);
    assert_eq!(int(&mut s, "BGM", QueryField::Time), 250);
}

#[test]
fn delay_window_renders_silence_on_device_path() {
    // 设备路径（render_and_advance：先渲染后推进）在延迟窗口内输出静音。
    let mut s = state_with(&[("abc.wav", wav_ms(5000, 8000, 1))]);
    let mut c = cmd("BGM");
    c.file = Some(b"abc.wav".to_vec());
    c.start = Some(-1000);
    c.play = Some(1);
    s.apply_sound(&c);
    let mut buf = vec![0.0f32; 800]; // 8000Hz × 800 帧 = 100ms
    s.render_and_advance(&mut buf, 8000, 1);
    assert!(buf.iter().all(|v| *v == 0.0), "延迟前 100ms 应为静音");
    // 烧完剩余 900ms 延迟后再取一块，应已出声。
    for _ in 0..9 {
        s.render_and_advance(&mut buf, 8000, 1);
    }
    s.render_and_advance(&mut buf, 8000, 1);
    assert!(
        buf.iter().any(|v| *v != 0.0),
        "延迟结束后应恢复出声（本块 TIME={}）",
        int(&mut s, "BGM", QueryField::Time)
    );
}

/// OGG 装载 = 官方 `0x422ab0` 架构:只读头部(总长立即精确),不解码;
/// 播放/预取才按需解 packet。`TOTALTIME` 用 granule(`ov_time_total` 同口径)。
#[test]
fn gated_ogg_load_reads_header_only_then_streams_on_play() {
    let Ok(root) = std::env::var("YURIS_SAMPLE_SDK0495") else {
        eprintln!("SKIP snd-stream: YURIS_SAMPLE_SDK0495 not set");
        return;
    };
    let root = std::path::Path::new(&root);
    let candidates = [
        root.join("data/bgm/bgm03.ogg"),
        root.join("システム/data/bgm/bgm03.ogg"),
    ];
    let Some(path) = candidates.iter().find(|p| p.is_file()) else {
        eprintln!("SKIP snd-stream: bgm03.ogg not under {}", root.display());
        return;
    };
    let bytes = std::fs::read(path).expect("read bgm03.ogg");
    let header = yu_sound::meta::probe(b"bgm03.ogg", &bytes).expect("header probe");
    assert!(header.is_exact(), "SDK 样本应有精确 granule");
    let mut s = state_with(&[("bgm03.ogg", bytes)]);

    let mut c = cmd("BGM");
    c.file = Some(b"bgm03.ogg".to_vec());
    c.play = Some(1);
    s.apply_sound(&c);
    // 装载当帧:PLAY 立即为 1(官方语义),TOTALTIME 来自 granule,零解码。
    assert_eq!(int(&mut s, "BGM", QueryField::Play), 1);
    assert_eq!(
        int(&mut s, "BGM", QueryField::TotalTime),
        header.total_ms().expect("total")
    );
    let stats = s.take_decode_stats();
    assert_eq!(stats.full_decodes, 0, "OGG 装载不得整段解码");
    assert_eq!(stats.decoded_frames, 0, "装载当帧不得解码音频");
    assert!(stats.decode_us < 5_000, "装载只解析头部:{}us", stats.decode_us);

    // 同一文件重复 SOUND(另一层):装载仍零解码(官方每次重新开流)。
    let mut c2 = cmd("SE");
    c2.file = Some(b"bgm03.ogg".to_vec());
    c2.play = Some(1);
    s.apply_sound(&c2);
    let stats = s.take_decode_stats();
    assert_eq!(stats.full_decodes, 0);
    assert_eq!(stats.decoded_frames, 0, "重复 SOUND 不得整段解码");
    assert_eq!(int(&mut s, "SE", QueryField::Play), 1);

    // 混音一个块:按需解码(流式),输出非静音。
    let mut buf = vec![0.0f32; 4096 * 2];
    s.render(&mut buf, 44_100, 2);
    assert!(
        buf.iter().any(|v| v.abs() > 0.0),
        "流式解码后应有声音"
    );
    let stats = s.take_decode_stats();
    assert!(stats.decoded_frames > 0, "播放时必须解出帧");

    // tick 预取 0.5s:再次解码并保持 PLAY/时间语义。
    s.advance_ms(16);
    let stats = s.take_decode_stats();
    assert!(stats.decoded_frames > 0, "预取应继续填充窗口");
}

/// WAVE PCM:官方 mmio 直读,整段样本一次读入;PLAY 当帧即可听。
#[test]
fn wav_load_plays_immediately_and_keeps_totaltime() {
    let bytes = wav_ms(1000, 8000, 1);
    let expected = decode(b"abc.wav", &bytes).expect("decode").total_ms();
    let mut s = state_with(&[("abc.wav", bytes)]);
    let mut c = cmd("BGM");
    c.file = Some(b"abc.wav".to_vec());
    c.play = Some(1);
    s.apply_sound(&c);
    assert_eq!(int(&mut s, "BGM", QueryField::Play), 1);
    assert_eq!(int(&mut s, "BGM", QueryField::TotalTime), expected);
    assert_eq!(s.take_decode_stats().full_decodes, 1);
    let mut buf = vec![0.0f32; 800];
    s.render_and_advance(&mut buf, 8000, 1);
    assert!(buf.iter().any(|v| *v != 0.0));
}

/// OGG 流式混音输出必须与整段解码逐样本一致(含 START 定位)。
/// 这是「装载只读头部、播放按需解码」不改变可观测音频的直接证据。
#[test]
fn gated_ogg_stream_render_matches_full_decode() {
    let Ok(root) = std::env::var("YURIS_SAMPLE_SDK0495") else {
        eprintln!("SKIP snd-stream-render: YURIS_SAMPLE_SDK0495 not set");
        return;
    };
    let root = std::path::Path::new(&root);
    let candidates = [
        root.join("data/bgm/bgm03.ogg"),
        root.join("システム/data/bgm/bgm03.ogg"),
    ];
    let Some(path) = candidates.iter().find(|p| p.is_file()) else {
        eprintln!("SKIP snd-stream-render: bgm03.ogg not under {}", root.display());
        return;
    };
    let bytes = std::fs::read(path).expect("read bgm03.ogg");
    let header = yu_sound::meta::probe(b"bgm03.ogg", &bytes).expect("header");
    let pcm = decode(b"bgm03.ogg", &bytes).expect("decode");
    let ch = usize::from(pcm.channels);
    let rate = pcm.sample_rate;
    let total = header.total_frames.unwrap_or(0).min(pcm.frames());
    let frames = 50_000usize.min(total as usize);

    let render = |start_ms: Option<i64>| {
        let mut s = state_with(&[("bgm03.ogg", bytes.clone())]);
        let mut c = cmd("BGM");
        c.file = Some(b"bgm03.ogg".to_vec());
        c.start = start_ms;
        c.play = Some(1);
        s.apply_sound(&c);
        let mut buf = vec![0.0f32; frames * 2];
        s.render(&mut buf, rate, 2);
        buf
    };
    let plain = render(None);
    for i in 0..frames {
        for c in 0..2usize {
            let src = if c < ch { pcm.samples[i * ch + c] } else { pcm.samples[i * ch] };
            let expected = src as f32 / 32768.0;
            assert!(
                (plain[i * 2 + c] - expected).abs() < 2e-4,
                "frame={i} ch={c} stream={} decode={expected}",
                plain[i * 2 + c]
            );
        }
    }
    // START=5000ms:定位后从该帧继续。
    let start_ms = 5000i64;
    let start_frame = (start_ms as u128 * u128::from(rate) / 1000) as usize;
    let seeked = render(Some(start_ms));
    for i in 0..frames.min(total as usize - start_frame) {
        let src = if ch >= 2 {
            pcm.samples[(start_frame + i) * ch]
        } else {
            pcm.samples[start_frame + i]
        };
        let expected = src as f32 / 32768.0;
        assert!(
            (seeked[i * 2] - expected).abs() < 2e-4,
            "seek frame={i} stream={} decode={expected}",
            seeked[i * 2]
        );
    }
}

/// 循环回绕到 0 必须真的重开 OGG reader(窗口已在末尾且为空时不能
/// 误判为「reader 就在 0」),否则 sample(0) 会欠载。
#[test]
fn gated_ogg_loop_wrap_back_to_zero_reopens_stream() {
    let Ok(root) = std::env::var("YURIS_SAMPLE_SDK0495") else {
        eprintln!("SKIP snd-loop-wrap: YURIS_SAMPLE_SDK0495 not set");
        return;
    };
    let root = std::path::Path::new(&root);
    let candidates = [
        root.join("data/bgm/bgm03.ogg"),
        root.join("システム/data/bgm/bgm03.ogg"),
    ];
    let Some(path) = candidates.iter().find(|p| p.is_file()) else {
        eprintln!("SKIP snd-loop-wrap: bgm03.ogg not under {}", root.display());
        return;
    };
    let bytes = std::fs::read(path).expect("read bgm03.ogg");
    let mut s = state_with(&[("bgm03.ogg", bytes)]);
    let mut c = cmd("BGM");
    c.file = Some(b"bgm03.ogg".to_vec());
    c.end = Some(500);
    c.restart = Some(0);
    c.loop_ = Some(0);
    c.play = Some(1);
    s.apply_sound(&c);
    // 先解码一段(让 reader/窗口离开 0),再推进到 END 之外回绕。
    let mut warm = vec![0.0f32; 4096];
    s.render(&mut warm, 44_100, 2);
    s.advance_ms(700);
    let stats = s.take_decode_stats();
    assert!(stats.seeks >= 1, "循环回绕必须触发 seek(实际 {})", stats.seeks);
    let mut buf = vec![0.0f32; 2048];
    s.render(&mut buf, 44_100, 2);
    let stats = s.take_decode_stats();
    assert_eq!(stats.underruns, 0, "回绕后不应欠载");
    assert!(buf.iter().any(|v| v.abs() > 0.0), "回绕后应继续出声");
}
