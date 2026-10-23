//! 内核侧任务相位:每 tick 记录 `Scheduler` 的任务表(script/pc/wait 状态)。
//!
//! 用途:与 `tools/official_trace/probe_dispatch.py --mode task-phase` 采到的
//! 官方任务列表快照对拍(DIV-L-002:script 60 返回后的任务相位/调度)。
//!
//! 官方侧只能读**任务链表**(指针/script/pc/wait/期限),没有 spawn/finish
//! 事件;这里同样只输出每 tick 的相位快照,不把快照伪造成事件。
//!
//! 用法:
//! ```text
//! cargo run -q -p yu-trace --example emit_kernel_phase -- \
//!   --root <project root(含 data.ypf)> --out target/tmp/kernel_phase.jsonl \
//!   --ticks 600 --click-frame 6
//! ```

use std::process::ExitCode;

use yu_rt::input::{InputFrame, InputTimeline};
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource, YpfSource};

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|item| item == name)
        .and_then(|index| args.get(index + 1))
        .cloned()
}

fn parse_u64(args: &[String], name: &str, default: u64) -> u64 {
    arg(args, name)
        .map(|text| text.parse::<u64>().expect("unsigned integer"))
        .unwrap_or(default)
}

fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let Some(root) = arg(&args, "--root") else {
        eprintln!("missing --root <project root with data.ypf>");
        return ExitCode::from(2);
    };
    let Some(out) = arg(&args, "--out") else {
        eprintln!("missing --out <phase.jsonl>");
        return ExitCode::from(2);
    };
    let ticks = parse_u64(&args, "--ticks", 600);
    let click_frame = parse_u64(&args, "--click-frame", 6);
    let debug_mode = arg(&args, "--debug-mode").as_deref() == Some("true");
    if ticks == 0 {
        eprintln!("--ticks must be >= 1");
        return ExitCode::from(2);
    }

    let pack_path = format!("{root}/data.ypf");
    let pack_bytes = match std::fs::read(&pack_path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("read {pack_path}: {error}");
            return ExitCode::from(2);
        }
    };
    let sample_sha = yu_trace::sha256_hex(&pack_bytes);
    let src = match YpfSource::open(&pack_path, Codec::Cp932) {
        Ok(src) => src,
        Err(diag) => {
            eprintln!("open {pack_path}: {diag}");
            return ExitCode::from(2);
        }
    };
    let aux = match DirSource::open(&root, Codec::Cp932) {
        Ok(aux) => aux,
        Err(diag) => {
            eprintln!("open aux {root}: {diag}");
            return ExitCode::from(2);
        }
    };
    let cfg = BootConfig {
        entry_label: "SYSTEM_START".to_string(),
        fps: 60,
        screen_color: 32,
        debug_mode,
        ..BootConfig::default()
    };
    let mut vm = match Vm::boot_with_aux(Box::new(src), Some(Box::new(aux)), cfg) {
        Ok(vm) => vm,
        Err(diag) => {
            eprintln!("boot: {diag}");
            return ExitCode::from(2);
        }
    };

    let mut timeline = InputTimeline::new();
    timeline.set(
        click_frame,
        InputFrame::new(["MOUSE_L", "ENTER"]).with_cursor(410, 275),
    );
    timeline.set(click_frame + 1, InputFrame::default());

    let mut lines = Vec::with_capacity(ticks as usize);
    for tick in 0..ticks {
        let now_ms = (tick as u32).saturating_mul(16);
        let report = vm.tick_at(now_ms, &timeline);
        let mut tasks = Vec::new();
        for node in vm.scheduler().nodes() {
            if node.finished {
                continue;
            }
            let deadline = match node.deadline_ms {
                Some(value) => value.to_string(),
                None => "null".to_string(),
            };
            tasks.push(format!(
                "{{\"id\":{},\"main\":{},\"script\":{},\"pc\":{},\"wait_frames\":{},\
                 \"deadline_ms\":{},\"enabled\":{},\"text_rest\":{},\"priority\":{},\
                 \"last_frame\":{}}}",
                json_string(&node.id),
                node.main,
                node.script_id,
                node.pc,
                node.wait_frames,
                deadline,
                node.enabled,
                node.text_rest,
                node.priority,
                node.last_frame,
            ));
        }
        lines.push(format!(
            "{{\"tick\":{tick},\"frame\":{},\"now_ms\":{now_ms},\"logical_frames\":{},\
             \"main_finished\":{},\"tasks\":[{}]}}",
            vm.scheduler().frame(),
            report.logical_frames,
            report.main_finished,
            tasks.join(","),
        ));
    }

    let mut text = format!(
        "{{\"kind\":\"kernel_task_phase\",\"schema\":1,\"sample_sha256\":{},\
         \"click_frame\":{click_frame},\"ticks\":{ticks},\"debug_mode\":{debug_mode}}}\n",
        json_string(&sample_sha)
    );
    for line in &lines {
        text.push_str(line);
        text.push('\n');
    }
    if let Some(parent) = std::path::Path::new(&out).parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(error) = std::fs::create_dir_all(parent) {
                eprintln!("create {}: {error}", parent.display());
                return ExitCode::from(2);
            }
        }
    }
    if let Err(error) = std::fs::write(&out, text) {
        eprintln!("write {out}: {error}");
        return ExitCode::from(2);
    }
    println!("wrote {out}: {ticks} ticks, click_frame={click_frame}, sample={sample_sha}");
    ExitCode::SUCCESS
}
