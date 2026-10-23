//! P0 场景验证:注入周期点击,把启动链推进到可见文本,统计文本事件。
//!
//! 用法:
//!   YURIS_SAMPLE_ERIS_ROOT=<project-eris> \
//!     cargo run -p yu-rt --example text_scenario -- [frames] [interval] [hold]
//!
//! 输入模型:`InputTimeline` 在 `f = interval, 2*interval, …` 按下
//! `ENTER` + `MOUSE_L`,`f+1` 释放(等价于“点一下”)。计数器在按下沿 +1、
//! 松开沿转负,`InputState::text_advance_requested` 即可推进文本。
//!
//! 输出:
//! - 首个 `Text` / `ReturnCode` 帧;
//! - `Text` 段数、字节数、`_TEXT_FLAG` 长度;
//! - 前 N 条文本(ASCII 可见,非 ASCII 打 `\xNN`);
//! - `WORD`/`_`/`RETURNCODE`/`TEXT`/`TEXTINFO` 运行触发数;
//! - 结束时 `text_state()` 的输出模式与 AUTO* 设置。
use std::collections::BTreeMap;

use yu_rt::event::VmEvent;
use yu_rt::input::{InputFrame, InputTimeline};
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DataSource, DirSource, YpfSource};

fn eris_root() -> String {
    std::env::var("YURIS_SAMPLE_ERIS_ROOT")
        .or_else(|_| {
            std::env::var("YURIS_SAMPLE_ERIS_YSBIN").map(|p| {
                std::path::Path::new(&p)
                    .ancestors()
                    .nth(2)
                    .expect("ysbin/../..")
                    .display()
                    .to_string()
            })
        })
        .expect("set YURIS_SAMPLE_ERIS_ROOT or YURIS_SAMPLE_ERIS_YSBIN")
}

fn open_source(root: &str) -> Box<dyn DataSource> {
    let ypf = std::path::PathBuf::from(root).join("data.ypf");
    if ypf.is_file() {
        Box::new(YpfSource::open(&ypf, Codec::Cp932).expect("data.ypf"))
    } else {
        Box::new(DirSource::open(format!("{root}/data"), Codec::Cp932).expect("data"))
    }
}

fn show(bytes: &[u8]) -> String {
    let mut s = String::new();
    for &b in bytes.iter().take(48) {
        if (0x20..0x7f).contains(&b) {
            s.push(b as char);
        } else {
            s.push_str(&format!("\\x{b:02x}"));
        }
    }
    if bytes.len() > 48 {
        s.push('…');
    }
    s
}

fn show_value(v: &yu_rt::Value) -> String {
    match v {
        yu_rt::Value::Int(i) => i.to_string(),
        yu_rt::Value::Float(f) => format!("{f}"),
        yu_rt::Value::Str(b) => show(b),
    }
}

fn main() {
    let root = eris_root();
    let frames: u32 = std::env::args()
        .nth(1)
        .and_then(|v| v.parse().ok())
        .unwrap_or(600);
    let interval: u32 = std::env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    let hold: u32 = std::env::args()
        .nth(3)
        .and_then(|v| v.parse().ok())
        .unwrap_or(2);
    let samples: usize = std::env::args()
        .nth(4)
        .and_then(|v| v.parse().ok())
        .unwrap_or(12);
    let dump_frame: Option<u32> = std::env::args().nth(5).and_then(|v| v.parse().ok());
    let cursor: (i64, i64) = (
        std::env::args()
            .nth(6)
            .and_then(|v| v.parse().ok())
            .unwrap_or(400),
        std::env::args()
            .nth(7)
            .and_then(|v| v.parse().ok())
            .unwrap_or(300),
    );

    let mut timeline = InputTimeline::new();
    let mut f = interval.max(1);
    while f < frames {
        timeline.set(
            u64::from(f),
            InputFrame::new(["ENTER", "MOUSE_L"]).with_cursor(cursor.0, cursor.1),
        );
        for k in 1..=hold {
            timeline.set(
                u64::from(f + k),
                InputFrame::new(std::iter::empty::<&str>()).with_cursor(cursor.0, cursor.1),
            );
        }
        f += interval.max(1);
    }

    let src = open_source(&root);
    let aux = DirSource::open(&root, Codec::Cp932).expect("project");
    let entry = std::env::var("YURIS_ENTRY_LABEL").unwrap_or_else(|_| "SYSTEM_START".to_string());
    let cfg = BootConfig {
        entry_label: entry.clone(),
        fps: 60,
        frame_ms: 16,
        screen_color: 32,
        record_events: true,
        ..BootConfig::default()
    };
    let mut vm = Vm::boot_with_aux(src, Some(Box::new(aux)), cfg).expect("boot");
    println!("entry={entry}");
    for name in ["SCENARIO_START", "SCENARIO_TITLE", "SCENARIO_MAIN"] {
        match vm.tables().boot.labels.get(name) {
            Some(l) => println!("label {name}: s{} pc{}", l.script_id, l.pc),
            None => println!("label {name}: MISSING"),
        }
    }
    let mut text_events = 0u64;
    let mut text_bytes = 0u64;
    let mut returncodes = 0u64;
    let mut first_text: Option<u32> = None;
    let mut first_rc: Option<u32> = None;
    let mut samples_out: Vec<(u32, String)> = Vec::new();
    let mut dump_sites: BTreeMap<(u32, u32, String), u64> = BTreeMap::new();
    let mut dump_scripts: BTreeMap<u32, u64> = BTreeMap::new();
    let mut focus_seq: Vec<String> = Vec::new();
    let mut diag_counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut task_log: Vec<String> = Vec::new();
    let mut special: Vec<String> = Vec::new();
    let mut dump_seq: Vec<String> = Vec::new();
    for frame in 0..frames {
        let r = vm.tick_at(frame * 16, &timeline);
        for e in vm.take_events() {
            if let VmEvent::Command {
                frame: ef,
                script,
                pc,
                name,
                args,
            } = &e
            {
                let mut detail = format!("s{script} pc{pc} {name}");
                for av in args {
                    detail.push_str(&format!(" {0}={1}", av.slot, show_value(&av.value)));
                }
                if (name == "TASK" || name == "TASKINFO") && special.len() < 60 {
                    special.push(format!("@{ef} {detail}"));
                }
                if dump_frame == Some(*ef as u32) {
                    *dump_scripts.entry(*script).or_insert(0) += 1;
                    *dump_sites.entry((*script, *pc, name.clone())).or_insert(0) += 1;
                    if dump_seq.len() < 40 {
                        dump_seq.push(detail);
                    } else {
                        dump_seq.remove(0);
                        dump_seq.push(detail);
                    }
                    if matches!(*script, 20 | 52 | 71 | 72) {
                        let line = dump_seq.last().cloned().unwrap_or_default();
                        if focus_seq.len() < 80 {
                            focus_seq.push(line);
                        } else {
                            focus_seq.remove(0);
                            focus_seq.push(line);
                        }
                    }
                }
            }
            match e {
                VmEvent::Text { frame, bytes, .. } => {
                    text_events += 1;
                    text_bytes += bytes.len() as u64;
                    first_text.get_or_insert(frame as u32);
                    if samples_out.len() < samples {
                        samples_out.push((frame as u32, show(&bytes)));
                    }
                }
                VmEvent::ReturnCode { frame, .. } => {
                    returncodes += 1;
                    first_rc.get_or_insert(frame as u32);
                }
                VmEvent::Diag { code, detail, .. } => {
                    *diag_counts.entry(format!("{code}: {detail}")).or_insert(0) += 1;
                }
                VmEvent::TaskSpawn {
                    frame,
                    id,
                    script,
                    pc,
                    ..
                } if task_log.len() < 80 => {
                    task_log.push(format!("@{frame} spawn {id} s{script} pc{pc}"));
                }
                VmEvent::TaskFinish { frame, id } if task_log.len() < 80 => {
                    task_log.push(format!("@{frame} finish {id}"));
                }
                _ => {}
            }
        }
        if r.main_finished {
            println!("main finished at frame {frame}");
            break;
        }
    }
    let hits = vm.command_hits();
    let yscm = &vm.tables().boot.yscm;
    let hit = |name: &str| -> u64 {
        yscm.index_of(name)
            .map(|i| hits.get(i).copied().unwrap_or(0))
            .unwrap_or(0)
    };
    let st = vm.text_state();
    println!("frames={frames} interval={interval} hold={hold}");
    println!(
        "first_text={first_text:?} first_returncode={first_rc:?} text_events={text_events} \
         text_bytes={text_bytes} returncodes={returncodes}"
    );
    println!(
        "hits: WORD={} _={} RETURNCODE={} TEXT={} TEXTINFO={}",
        hit("WORD"),
        hit("_"),
        hit("RETURNCODE"),
        hit("TEXT"),
        hit("TEXTINFO")
    );
    println!(
        "text_state: output_mode={} autoreturn={} autopage={} autoclick={} wait={} \
         bytes={} flags={}",
        st.output_mode,
        st.autoreturn,
        st.autopage,
        st.autoclick,
        st.wait_ms,
        st.bytes.len(),
        st.flags.len()
    );
    println!("flag bytes: {}", show(&st.flags));
    if !diag_counts.is_empty() {
        println!("== diags ==");
        for (d, n) in &diag_counts {
            println!("  {n:>5}  {d}");
        }
    }
    println!("== tasks (first 80) ==");
    for t in &task_log {
        println!("  {t}");
    }
    if !special.is_empty() {
        println!("== TASK/TASKINFO (first 60) ==");
        for s in &special {
            println!("  {s}");
        }
    }
    for (frame, s) in &samples_out {
        println!("  text @{frame}: {s}");
    }
    if let Some(df) = dump_frame {
        println!("== frame {df}: scripts ==");
        for (s, n) in &dump_scripts {
            println!("  {n:>6}  s{s}  ({})", vm.script_name(*s));
        }
        println!("== frame {df}: top sites ==");
        let mut sites: Vec<_> = dump_sites.iter().collect();
        sites.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        for ((s, pc, name), n) in sites.iter().take(30) {
            println!("  {n:>6}  s{s} pc{pc} {name}  ({})", vm.script_name(*s));
        }
        println!("== frame {df}: last 40 commands ==");
        for s in &dump_seq {
            println!("  {s}");
        }
        println!("== frame {df}: focus (s20/s52/s71/s72) last 80 ==");
        for s in &focus_seq {
            println!("  {s}");
        }
    }
    if text_events == 0 {
        std::process::exit(2);
    }
}
