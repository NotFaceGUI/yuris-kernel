//! `yu-sound` 状态机单测：手册示例 1..12 的值断言（不是听感断言）。

use std::io::Cursor;

use yu_rt::value::Value;
use yu_sound::decode::decode;
use yu_sound::loader::MemoryLoader;
use yu_sound::state::{probe_names, QueryField, SoundCommand, SoundState};
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
fn example5_negative_start_is_explicitly_unimplemented() {
    let mut s = state_with(&[("abc.wav", wav_ms(5000, 1000, 1))]);
    let mut c = cmd("BGM");
    c.file = Some(b"abc.wav".to_vec());
    c.start = Some(-10000);
    c.play = Some(1);
    s.apply_sound(&c);
    // 负 START 的延迟语义未取证（DIV-SND-002）：不猜排程，位置停在 0。
    assert_eq!(int(&mut s, "BGM", QueryField::Start), -10000);
    assert_eq!(int(&mut s, "BGM", QueryField::Time), 0);
    s.advance_ms(1000);
    assert_eq!(int(&mut s, "BGM", QueryField::Time), 1000);
    assert!(s
        .diags()
        .iter()
        .any(|d| d.code == "snd.delay.unimplemented"));
}
