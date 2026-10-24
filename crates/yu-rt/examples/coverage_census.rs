//! 覆盖普查(任务书 M-1):boot ERIS → 固定时钟 tick N 帧 → 逐命令频度、
//! 未实现挂号、域事件参数组合、诊断码分布。
//!
//! 用法:
//!   YURIS_SAMPLE_ERIS_ROOT=<project-eris> \
//!     cargo run -p yu-rt --example coverage_census -- [frames] [samples]
//!
//! - 数据入口:封包 `<root>/data.ypf` 优先,散盘 `<root>/data/` 为备
//!   (AGENTS.md §2 唯一约定)。
//! - 时钟:`tick_at(f * 16, …)`,f = 0..frames-1,固定注入,不用墙钟。
//! - `record_events=false`:逐命令频度来自 `Vm::command_hits`(与事件流无关);
//!   域事件/文本事件/诊断仍照常产生。这样 60 万级命令帧不会挤爆事件队列,
//!   普查结果也可复现。
//! - 输出:
//!   1. 场景头(语料 hash / 帧范围 / 数据形态);
//!   2. 逐命令矩阵(静态语料频度 × 运行时触发 × 处理器类别);
//!   3. 运行时未实现命令;
//!   4. 域事件(domain/name → 次数 + 参数组合样本);
//!   5. 事件种类与诊断码分布。
//!
//! 语料 hash 用 FNV-1a 64(自带实现,不引依赖):封包 = `data.ypf` 全字节;
//! 散盘 = `data/` 下按相对路径排序的 `(路径, 字节)` 流。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use yu_rt::event::{ArgValue, Domain, VmEvent};
use yu_rt::input::InputTimeline;
use yu_rt::tables::{TableSet, DEFAULT_SCRIPT_KEY};
use yu_rt::value::Value;
use yu_rt::vm::HandlerKind;
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DataSource, DirSource, YpfSource};

const FRAME_MS: u32 = 16;
/// 每个 domain/command 保留的参数组合样本上限(其余只计数)。
const MAX_SIGNATURES: usize = 16;

fn eris_root() -> String {
    std::env::var("YURIS_SAMPLE_ERIS_ROOT")
        .or_else(|_| {
            std::env::var("YURIS_SAMPLE_ERIS_YSBIN").map(|p| {
                Path::new(&p)
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
    let ypf = PathBuf::from(root).join("data.ypf");
    if ypf.is_file() {
        Box::new(YpfSource::open(&ypf, Codec::Cp932).expect("data.ypf"))
    } else {
        Box::new(DirSource::open(format!("{root}/data"), Codec::Cp932).expect("data"))
    }
}

fn open_aux(root: &str) -> Box<dyn DataSource> {
    Box::new(DirSource::open(root, Codec::Cp932).expect("project"))
}

fn fnv1a64(state: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *state ^= u64::from(b);
        *state = state.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

/// 语料指纹(形态, 值)。返回 (说明, FNV-1a 64)。
fn corpus_fingerprint(root: &str) -> (String, u64) {
    let ypf = PathBuf::from(root).join("data.ypf");
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    if ypf.is_file() {
        let bytes = std::fs::read(&ypf).expect("read data.ypf");
        fnv1a64(&mut h, &bytes);
        (format!("data.ypf ({} bytes)", bytes.len()), h)
    } else {
        let dir = PathBuf::from(root).join("data");
        let mut files = Vec::new();
        collect_files(&dir, &dir, &mut files);
        files.sort();
        for (rel, path) in &files {
            fnv1a64(&mut h, rel.as_bytes());
            let bytes = std::fs::read(path).expect("read corpus file");
            fnv1a64(&mut h, &bytes);
        }
        (format!("data/ ({} files)", files.len()), h)
    }
}

fn collect_files(base: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(base, &path, out);
        } else if path.is_file() {
            let rel = path
                .strip_prefix(base)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            out.push((rel, path));
        }
    }
}

fn value_text(v: &Value) -> String {
    match v {
        Value::Int(i) => i.to_string(),
        Value::Float(f) => format!("{f}"),
        Value::Str(b) => {
            let mut s = String::new();
            for &x in b.iter().take(64) {
                if (0x20..0x7f).contains(&x) {
                    s.push(x as char);
                } else {
                    s.push_str(&format!("\\x{x:02x}"));
                }
            }
            if b.len() > 64 {
                s.push('…');
            }
            s
        }
    }
}

fn kind_label(kind: HandlerKind) -> String {
    match kind {
        HandlerKind::Unknown => "无处理器".to_string(),
        HandlerKind::Domain(d) => format!("域事件:{}", d.as_str()),
        HandlerKind::Noop => "no-op(官方0x4634d8)".to_string(),
        HandlerKind::ScriptError => "脚本错误槽(官方默认0x459e90)".to_string(),
        _ => "已实现".to_string(),
    }
}

#[derive(Default)]
struct DomainStat {
    total: u64,
    sigs: BTreeMap<String, u64>,
    overflow: u64,
}

fn main() {
    let root = eris_root();
    let mut args = std::env::args().skip(1);
    let frames: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(600);
    let samples: usize = args.next().and_then(|v| v.parse().ok()).unwrap_or(6);

    let (corpus_desc, corpus_hash) = corpus_fingerprint(&root);

    // ---- 静态语料频度(全量 yst 组扫描;与 corpus 测试同口径) ----
    let scan_src = open_source(&root);
    let scan_aux = open_aux(&root);
    let mut tables = TableSet::load_from(
        scan_src.as_ref(),
        Some(scan_aux.as_ref()),
        DEFAULT_SCRIPT_KEY,
    )
    .expect("tables");
    let yscm = tables.boot.yscm.clone();
    let script_list_len = tables.boot.scripts.len();
    let script_ids: Vec<u32> = (0..tables.boot.scripts.len() as u32).collect();
    let mut static_freq = vec![0u64; yscm.len()];
    let mut static_groups = 0u64;
    let mut static_windows = 0u64;
    let mut static_scripts = 0usize;
    let mut missing_scripts = 0usize;
    for sid in script_ids {
        match tables.load_script(scan_src.as_ref(), sid) {
            Ok(script) => {
                static_scripts += 1;
                static_groups += script.group_count() as u64;
                for pc in 0..script.group_count() {
                    let g = script.group(pc).expect("group");
                    static_windows += g.window_count as u64;
                    if let Some(slot) = static_freq.get_mut(g.opcode as usize) {
                        *slot += 1;
                    }
                }
            }
            Err(_) => missing_scripts += 1,
        }
    }
    drop(tables);
    drop(scan_src);
    drop(scan_aux);

    // ---- 运行期 census ----
    let src = open_source(&root);
    let aux = open_aux(&root);
    let cfg = BootConfig {
        entry_label: "SYSTEM_START".to_string(),
        fps: 60,
        frame_ms: FRAME_MS,
        screen_color: 32,
        // 频度用 command_hits;关事件流的 Command/Jump 预求值(热路径)。
        record_events: false,
        ..BootConfig::default()
    };
    let mut vm = Vm::boot_with_aux(src, Some(aux), cfg).expect("boot");
    let yscm = vm.tables().boot.yscm.clone();
    let timeline = InputTimeline::new();

    let mut event_kinds: BTreeMap<String, u64> = BTreeMap::new();
    let mut domains: BTreeMap<(String, String), DomainStat> = BTreeMap::new();
    let mut diag_codes: BTreeMap<String, u64> = BTreeMap::new();
    let mut diag_samples: BTreeMap<String, Vec<String>> = BTreeMap::new();
    // (code, 命令, 脚本:pc) → (次数, 首条 detail);把未实现精确归因到命令/位置。
    let mut diag_sites: BTreeMap<(String, String, String), (u64, String)> = BTreeMap::new();
    let mut text_bytes: u64 = 0;
    let mut text_events: u64 = 0;
    let mut frame_commands: Vec<u64> = Vec::new();
    let mut first_nonempty_tick: Option<u64> = None;
    let mut first_visible_frame: Option<u64> = None;
    let mut main_finished_at: Option<u64> = None;
    let mut total_pumped = 0u64;
    let mut buf = Vec::new();

    for f in 0..frames {
        let r = vm.tick_at(f * FRAME_MS, &timeline);
        total_pumped += r.commands_pumped;
        frame_commands.push(r.commands_pumped);
        if first_nonempty_tick.is_none() && r.commands_pumped > 0 {
            first_nonempty_tick = Some(u64::from(f));
        }
        if r.main_finished && main_finished_at.is_none() {
            main_finished_at = Some(u64::from(f));
        }
        buf.clear();
        vm.drain_events(&mut buf);
        for ev in &buf {
            *event_kinds.entry(ev.kind().to_string()).or_insert(0) += 1;
            // 首个可见帧 = 第一条 Graphics/Window 域事件或 Text 事件所在 tick。
            if first_visible_frame.is_none()
                && matches!(
                    ev,
                    VmEvent::Text { .. }
                        | VmEvent::DomainCall {
                            domain: Domain::Graphics | Domain::Window,
                            ..
                        }
                )
            {
                first_visible_frame = Some(u64::from(f));
            }
            match ev {
                VmEvent::DomainCall {
                    domain, name, args, ..
                } => {
                    let key = (domain.as_str().to_string(), name.clone());
                    let stat = domains.entry(key).or_default();
                    stat.total += 1;
                    let sig = signature(&yscm, name, args);
                    if stat.sigs.contains_key(&sig) || stat.sigs.len() < MAX_SIGNATURES {
                        *stat.sigs.entry(sig).or_insert(0) += 1;
                    } else {
                        stat.overflow += 1;
                    }
                }
                VmEvent::Text { bytes, .. } => {
                    text_events += 1;
                    text_bytes += bytes.len() as u64;
                }
                VmEvent::Diag { code, detail, .. } => {
                    *diag_codes.entry(code.clone()).or_insert(0) += 1;
                    let list = diag_samples.entry(code.clone()).or_default();
                    if list.len() < samples && !list.contains(detail) {
                        list.push(detail.clone());
                    }
                }
                _ => {}
            }
        }
        // 诊断记录(MemorySink)带 Loc;按 (code, 命令, 脚本:pc) 归因。
        for d in vm.take_diags() {
            let (cmd, site) = match &d.where_ {
                yu_log::Loc::Script {
                    file,
                    pc,
                    cmd,
                    label: _,
                } => (
                    cmd.clone().unwrap_or_else(|| "-".to_string()),
                    format!("{file}:{pc}"),
                ),
                _ => ("-".to_string(), "-".to_string()),
            };
            let entry = diag_sites
                .entry((d.code.to_string(), cmd, site))
                .or_insert_with(|| (0, d.detail.clone()));
            entry.0 += 1;
        }
    }

    let hits = vm.command_hits();
    let unimpl = vm.unimplemented_cmds().clone();
    let dropped = vm.events_dropped();

    // ---- 输出 ----
    println!("# yu-rt 覆盖普查(M-1)");
    println!();
    println!("- 语料: `{}`,FNV-1a64 = `{corpus_hash:016x}`", corpus_desc);
    println!(
        "- 静态扫描: {} 个 yst,{} 组,{} 窗口(脚本清单 {} 条,缺文件 {} 条;命令表 {} 条)",
        static_scripts,
        static_groups,
        static_windows,
        script_list_len,
        missing_scripts,
        yscm.len()
    );
    let logical_frames = event_kinds.get("frame_start").copied().unwrap_or(0);
    println!(
        "- 运行: `SYSTEM_START`,tick 0..{} 共 {} 次(每次 vsync 链可含多逻辑帧),逻辑帧 {};{} ms/帧固定注入,首个非空 tick = {:?},首个可见帧(Graphics/Window/Text)= {:?},主线结束 tick = {:?}",
        frames,
        frame_commands.len(),
        logical_frames,
        FRAME_MS,
        first_nonempty_tick,
        first_visible_frame,
        main_finished_at
    );
    println!(
        "- 合计: 命令 {} 次(报告累计 {}),事件丢弃 {},文本 {} 条 / {} 字节",
        total_pumped,
        vm.executed_commands(),
        dropped,
        text_events,
        text_bytes
    );
    println!();

    println!("## 1. 逐命令矩阵(按静态语料频度排序)");
    println!();
    println!("| 命令 | opcode | 静态频度 | 运行触发 | 处理器 | 运行时未实现 |");
    println!("|---|---:|---:|---:|---|---:|");
    let mut rows: Vec<(usize, &str, u64, u64, String, u64)> = Vec::new();
    for i in 0..yscm.len() {
        let Some(decl) = yscm.command(i as u8) else {
            continue;
        };
        let kind = vm
            .handler_kind(i as u8)
            .map(kind_label)
            .unwrap_or_else(|| "越界".to_string());
        let hit = hits.get(i).copied().unwrap_or(0);
        let bad = unimpl.get(&decl.name).copied().unwrap_or(0);
        rows.push((
            i,
            decl.name.as_str(),
            static_freq.get(i).copied().unwrap_or(0),
            hit,
            kind,
            bad,
        ));
    }
    rows.sort_by(|a, b| b.2.cmp(&a.2).then(b.3.cmp(&a.3)).then(a.1.cmp(b.1)));
    for (i, name, sf, hit, kind, bad) in &rows {
        println!("| `{name}` | {i} | {sf} | {hit} | {kind} | {bad} |");
    }
    println!();

    println!("## 2. 运行时未实现命令");
    println!();
    if unimpl.is_empty() {
        println!("(无)");
    } else {
        println!("| 命令 | 触发次数 | 处理器 |");
        println!("|---|---:|---|");
        for (name, n) in &unimpl {
            let kind = yscm
                .index_of(name)
                .and_then(|i| vm.handler_kind(i as u8))
                .map(kind_label)
                .unwrap_or_else(|| "不在 yscm".to_string());
            println!("| `{name}` | {n} | {kind} |");
        }
    }
    println!();

    println!("## 3. 域事件(参数组合样本 ≤ {MAX_SIGNATURES}/命令)");
    println!();
    for ((domain, name), stat) in &domains {
        println!("### {domain} / {name} — {} 次", stat.total);
        println!();
        println!("| 参数组合 | 次数 |");
        println!("|---|---:|");
        for (sig, n) in &stat.sigs {
            println!("| `{sig}` | {n} |");
        }
        if stat.overflow > 0 {
            println!("| (其余 {} 种组合) | {} |", MAX_SIGNATURES, stat.overflow);
        }
        println!();
    }

    println!("## 4. 事件种类");
    println!();
    println!("| kind | 次数 |");
    println!("|---|---:|");
    for (k, n) in &event_kinds {
        println!("| `{k}` | {n} |");
    }
    println!();

    println!("## 5. 诊断码");
    println!();
    println!("| code | 次数 | 样本 |");
    println!("|---|---:|---|");
    for (code, n) in &diag_codes {
        let samples = diag_samples
            .get(code)
            .map(|v| {
                v.iter()
                    .map(|s| s.replace('|', "\\|"))
                    .collect::<Vec<_>>()
                    .join("<br>")
            })
            .unwrap_or_default();
        println!("| `{code}` | {n} | {samples} |");
    }
    println!();

    println!("## 5b. 诊断归因(code × 命令 × 脚本:pc,按次数排序)");
    println!();
    println!("| code | 命令 | 位置 | 次数 | 首条 detail |");
    println!("|---|---|---|---:|---|");
    let mut sites: Vec<_> = diag_sites.iter().collect();
    sites.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(a.0.cmp(b.0)));
    for ((code, cmd, site), (n, detail)) in sites {
        println!(
            "| `{code}` | `{cmd}` | `{site}` | {n} | {} |",
            detail.replace('|', "\\|")
        );
    }
    println!();

    println!("## 6. 每帧命令数(前 5 / 后 5)");
    println!();
    println!("```text");
    for (f, n) in frame_commands.iter().enumerate().take(5) {
        println!("frame {f}: {n}");
    }
    if frame_commands.len() > 10 {
        println!("…");
    }
    let tail_start = frame_commands.len().saturating_sub(5);
    for (f, n) in frame_commands.iter().enumerate().skip(tail_start) {
        println!("frame {f}: {n}");
    }
    println!("```");
}

fn signature(yscm: &yu_rt::tables::YscmTable, name: &str, args: &[ArgValue]) -> String {
    let opcode = yscm.index_of(name);
    let mut out = String::new();
    for ArgValue { slot, value, .. } in args {
        if !out.is_empty() {
            out.push(' ');
        }
        let pname = opcode
            .and_then(|i| yscm.param(i as u8, *slot))
            .map(|p| p.name.as_str().to_string())
            .unwrap_or_else(|| format!("@{slot}"));
        out.push_str(&pname);
        out.push('=');
        out.push_str(&value_text(value));
    }
    if out.len() > 240 {
        out.truncate(240);
        out.push('…');
    }
    out
}
