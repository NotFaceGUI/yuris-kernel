//! 取证探针:商业包在完整宿主链(Engine)下的诊断面。
//!
//! 用法:`cargo run -p yu-cli --example game_diag_probe -- <project-root> [frames]`
//!
//! 打印:挂号表、等待任务、域计数、gfx 层/纹理、渲染 skip 明细、诊断 top-N。
use std::path::Path;

use yu_cli::engine::{Backend, Engine, EngineOptions};
use yu_cli::project::Project;

fn main() {
    if let Err(e) = run() {
        eprintln!("game_diag_probe: {e}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let root = args.next().ok_or("用法: game_diag_probe <root> [frames]")?;
    let frames: u32 = args
        .next()
        .and_then(|v| v.parse().ok())
        .unwrap_or(120);

    let project = Project::discover(Path::new(&root), None).map_err(|e| e.message.clone())?;
    let opts = EngineOptions {
        backend: Backend::Soft,
        ..EngineOptions::default()
    };
    let mut engine = Engine::new(project, opts).map_err(|e| e.message.clone())?;
    for _ in 0..frames {
        engine.tick().map_err(|e| e.message.clone())?;
    }

    let report = engine.take_diag_report();
    println!("== frame {} ==", report.frame);
    println!("-- unimplemented_cmds --");
    let mut un: Vec<_> = report.unimplemented_cmds.iter().collect();
    un.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    for (name, n) in &un {
        println!("  {name}: {n}");
    }
    println!("-- waiting_tasks --");
    for t in &report.waiting_tasks {
        println!(
            "  id={} main={} s{} pc{} wait_frames={} deadline={:?} text_rest={}",
            t.id, t.main, t.script, t.pc, t.wait_frames, t.deadline_ms, t.text_rest
        );
    }
    println!("-- counters --");
    println!(
        "  save_calls={} load={} saveend={} fileact={} slots={} reservations={}",
        report.save_calls,
        report.load_calls,
        report.saveend_calls,
        report.fileact_calls,
        report.save_slots,
        report.save_reservations
    );
    println!("  sound_layers={}", report.sound_layers);
    println!("  script_errors={}", report.script_errors.len());
    for e in report.script_errors.iter().take(10) {
        println!("     {e}");
    }

    println!("-- diags (top 30 by code) --");
    let mut by_code: std::collections::BTreeMap<String, (usize, String, String)> =
        std::collections::BTreeMap::new();
    for d in &report.diags {
        let e = by_code
            .entry(d.code.to_string())
            .or_insert((0, String::new(), String::new()));
        e.0 += 1;
        e.1 = format!("{:?}", d.where_);
        e.2 = d.detail.clone();
    }
    let mut rows: Vec<_> = by_code.into_iter().collect();
    rows.sort_by_key(|(_, (n, _, _))| std::cmp::Reverse(*n));
    for (code, (n, where_, detail)) in rows.iter().take(30) {
        let detail: String = detail.chars().take(150).collect();
        println!("  {n:6} {code}");
        println!("         at {where_}");
        println!("         {detail}");
    }

    println!("-- render report --");
    println!(
        "  viewport={}x{} layers={} visible={} passes={} ops={} drawn={} skipped={}",
        engine.viewport().width,
        engine.viewport().height,
        engine.last_report().layers,
        engine.last_report().visible,
        engine.last_report().passes,
        engine.last_report().ops,
        engine.last_report().executed,
        engine.last_report().skipped,
    );
    if let Some(r) = engine.render_report() {
        for s in &r.skipped {
            println!(
                "  skip {:<12} x{:<5} {:<50} {}",
                s.kind, s.count, s.reason, s.target
            );
        }
        println!(
            "  missing_assets={} skipped_text={}",
            r.missing_assets.len(),
            r.skipped_text
        );
        for a in r.missing_assets.iter().take(20) {
            println!("     missing {a:?}");
        }
    }
    println!(
        "  textures: decoded={} cached={}",
        engine.textures_decoded(),
        engine.textures_cached()
    );
    Ok(())
}
