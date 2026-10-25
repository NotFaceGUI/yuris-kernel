//! 取证探针:商业包启动后逐帧打印调度器任务面 + 首帧诊断分布。
//!
//! 用法:`cargo run -p yu-rt --example probe_game_tasks -- <game_root> [frames]`
//!
//! 目的:定位"首帧后无命令"是哪个任务、卡在什么等待条件(pc/脚本/等待态)。
use std::collections::BTreeMap;

use yu_rt::input::{InputFrame, InputTimeline};
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource, YpfSource};

fn main() {
    let root = std::env::args().nth(1).expect("game root (含 data.ypf)");
    let frames: u32 = std::env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    let pack = YpfSource::open(format!("{root}/data.ypf"), Codec::Cp932).expect("data.ypf");
    let aux = DirSource::open(&root, Codec::Cp932).expect("game root");
    let cfg = BootConfig {
        entry_label: "SYSTEM_START".into(),
        fps: 60,
        screen_color: 32,
        dx_mode: 0,
        media_readonly: false,
        os_sound: true,
        paths: yu_rt::vm::HostPaths {
            system_path: Some(format!("{root}/").into_bytes()),
            save_path: Some(format!("{root}/save/").into_bytes()),
            appdata_path: Some(format!("{root}/").into_bytes()),
            personal_path: Some(format!("{root}/").into_bytes()),
            project_path: Some(format!("{root}/").into_bytes()),
        },
        ..BootConfig::default()
    };
    let mut vm = Vm::boot_with_aux(Box::new(pack), Some(Box::new(aux)), cfg).expect("boot");
    // 脚本号 → 路径(定位卡点脚本;yst_list 160 条)。
    println!("== script paths ==");
    for sid in 0..200u32 {
        let path = vm.script_name(sid);
        if path.is_empty() || path.starts_with("yst") && path.ends_with(".ybn") {
            continue;
        }
        println!("  s{sid:<4} {path}");
    }
    // 输入时间线:每 10 帧一次单帧 MOUSE_L 脉冲(推进 RETURNCODE/文本等待)。
    let mut tl = InputTimeline::new();
    for f in 0..u64::from(frames) {
        let frame = if f % 10 == 5 {
            InputFrame::new(Vec::<String>::new()).with_pulse("MOUSE_L")
        } else {
            InputFrame::new(Vec::<String>::new())
        };
        tl.set(f, frame);
    }
    for f in 0..frames {
        let r = vm.tick_at(f * 16, &tl);
        println!(
            "== frame {f}: cmds={} errors={} main_finished={}",
            r.commands_pumped, r.errors.len(), r.main_finished
        );
        let nodes = vm.scheduler().nodes();
        println!("   tasks={}", nodes.len());
        for n in nodes {
            println!(
                "     id={:<20} main={} s{:<3} pc{:<6} wait_frames={:<4} deadline={:?} \
                 text_rest={} enabled={} finished={} steps={} last_frame={} Z={}",
                n.id.to_string(),
                n.main,
                n.script_id,
                n.pc,
                n.wait_frames,
                n.deadline_ms,
                n.text_rest,
                n.enabled,
                n.finished,
                n.steps,
                n.last_frame,
                n.priority
            );
        }
    }
    // 最后一条命令落点(能看出脚本停在哪)。
    println!("== last commands ==");
    let cmds: Vec<_> = vm
        .events()
        .iter()
        .filter_map(|e| match e {
            yu_rt::VmEvent::Command {
                script, pc, name, ..
            } => Some((*script, *pc, name.clone())),
            _ => None,
        })
        .collect();
    for (s, pc, name) in cmds.iter().rev().take(25).collect::<Vec<_>>().iter().rev() {
        println!("   s{s} pc{pc} {name}");
    }
    println!("== diag by code ==");
    let mut by_code: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_detail: BTreeMap<String, usize> = BTreeMap::new();
    for e in vm.events() {
        if let yu_rt::VmEvent::Diag { code, detail, .. } = e {
            *by_code.entry(code.clone()).or_insert(0) += 1;
            let key: String = detail.chars().take(110).collect();
            *by_detail.entry(format!("{code} | {key}")).or_insert(0) += 1;
        }
    }
    for (k, v) in &by_code {
        println!("  {v:6}  {k}");
    }
    println!("== diag by detail (top 30) ==");
    let mut rows: Vec<_> = by_detail.into_iter().collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.1));
    for (k, v) in rows.iter().take(30) {
        println!("  {v:6}  {k}");
    }
    println!("== unimplemented ==");
    let mut rows: Vec<_> = vm.unimplemented_cmds().iter().collect();
    rows.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
    for (name, n) in rows.iter().take(30) {
        println!("  {name}: {n}");
    }
}
