//! `yu-cli bench`:P0 度量(阈值先声明在 `crates/yu-cli/README.md`)。
//!
//! 度量项:
//! 1. boot(元数据表 + 入口脚本)与首帧 tick(`record_events` on/off);
//! 2. 到首个可绘制帧的累计耗时(信息项);
//! 3. 原生热重启(re-boot 后第 1 个逻辑帧);
//! 4. 稳态 tick+soft 合成(fps / p95);
//! 5. wgpu device 初始化;
//! 6. wasm presenter 增量构建 + wasm-bindgen;
//! 7. 本地服务 WebSocket 帧传输(首帧延迟、fps、输入→下一帧 p95)。

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::args::ArgReader;
use crate::build_info;
use crate::dev::free_port;
use crate::engine::{Engine, EngineOptions};
use crate::output;
use crate::project::Project;
use crate::run::parse_u32;
use crate::sha::sha256_hex;
use crate::ws_client::{Message, WsClient};
use crate::{CliResult, Failure};

pub fn from_args(tokens: Vec<String>) -> CliResult<()> {
    let mut args = ArgReader::new(tokens);
    let root = args
        .positional(0)
        .ok_or_else(|| Failure::usage("用法: yu-cli bench <project-root> [--frames N]"))?;
    let steady_frames = match args.flag("frames") {
        Some(value) => parse_u32("frames", &value)?.max(1),
        None => 120,
    };
    let skip_wasm = args.switch("skip-wasm");
    let skip_server = args.switch("skip-server");
    // debug 语料对拍口径(DIV-L-001):官方 debug 构建 `_DEBUGMODE=1`。
    let debug_mode = match args.flag("debug-mode") {
        Some(value) => ArgReader::on_off("debug-mode", &value)?,
        None => false,
    };
    args.finish()?;

    let project = Project::discover(Path::new(&root), None)?;
    let workspace = workspace_root();
    let mut rows: Vec<Row> = Vec::new();
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    output::line(format!(
        "yu-cli bench · profile={profile} marker={} data={}:{} ({} bytes)",
        build_info::ENGINE_MARKER,
        project.describe().kind,
        project.describe().path.display(),
        project.describe().bytes
    ));

    // 1/2. boot + 逐帧(off)。
    let off = measure_boot(&project, false, debug_mode)?;
    rows.extend(off.rows("record_events=off"));
    output::line(format!(
        "boot(off): {:.2} ms; first tick {:.2} ms; first drawable at frame {} after {:.2} ms",
        off.boot_ms, off.first_tick_ms, off.visible_frame, off.visible_total_ms
    ));
    for (frame, tick, render) in &off.per_frame {
        output::line(format!(
            "  frame {frame}: tick {tick:.2} ms render {render:.2} ms"
        ));
    }

    // 同一条链开事件记录再跑一次(信息项 + 输出守恒检查)。
    let on = measure_boot(&project, true, debug_mode)?;
    rows.extend(on.rows("record_events=on"));
    output::line(format!(
        "boot(on): {:.2} ms; first tick {:.2} ms; first drawable at frame {} after {:.2} ms",
        on.boot_ms, on.first_tick_ms, on.visible_frame, on.visible_total_ms
    ));
    output::line(format!(
        "on/off 守恒: display_sha256 {} (off {} / on {})",
        if off.display_sha256 == on.display_sha256 {
            "一致"
        } else {
            "不一致"
        },
        short(&off.display_sha256),
        short(&on.display_sha256)
    ));

    // 3. 原生热重启。
    let mut engine = off.engine;
    let restart_started = Instant::now();
    engine.restart()?;
    let restart_ms = restart_started.elapsed().as_secs_f64() * 1000.0;
    let first = engine.tick()?;
    let reboot_ready_ms = restart_started.elapsed().as_secs_f64() * 1000.0;
    rows.push(Row::new(
        "reboot_ms",
        restart_ms,
        "ms",
        "丢 Vm+GfxState 后重新 boot",
    ));
    rows.push(Row::new(
        "reboot_first_tick_ms",
        first.report.tick_ms,
        "ms",
        "重启后第 1 个逻辑帧",
    ));
    rows.push(Row::new(
        "reboot_ready_ms",
        reboot_ready_ms,
        "ms",
        "re-boot 后可 tick(任务书门禁口径)",
    ));
    output::line(format!(
        "reboot: boot {restart_ms:.2} ms; first tick {:.2} ms; ready in {reboot_ready_ms:.2} ms",
        first.report.tick_ms
    ));

    // 4. 稳态(从当前帧继续,不重新 boot)。
    let mut wall = Vec::new();
    let mut engine_samples = Vec::new();
    let mut slow: Vec<(u64, f64)> = Vec::new();
    let steady_started = Instant::now();
    for _ in 0..steady_frames {
        let started = Instant::now();
        let rendered = engine.tick()?;
        let elapsed = started.elapsed().as_secs_f64() * 1000.0;
        wall.push(elapsed);
        engine_samples.push(rendered.report.ms);
        slow.push((rendered.report.frame, elapsed));
    }
    let steady_total = steady_started.elapsed().as_secs_f64() * 1000.0;
    let avg_wall = wall.iter().sum::<f64>() / wall.len() as f64;
    let avg_engine = engine_samples.iter().sum::<f64>() / engine_samples.len() as f64;
    let p95 = percentile(&mut engine_samples, 95.0);
    let fps = if avg_wall > 0.0 {
        1000.0 / avg_wall
    } else {
        0.0
    };
    rows.push(Row::new(
        "steady_frames",
        steady_frames as f64,
        "frames",
        "稳态样本数",
    ));
    rows.push(Row::new("steady_avg_ms", avg_wall, "ms", "tick+合成挂钟"));
    rows.push(Row::new(
        "steady_engine_ms",
        avg_engine,
        "ms",
        "引擎报告 tick+render",
    ));
    rows.push(Row::new("steady_p95_ms", p95, "ms", "引擎报告 p95"));
    rows.push(Row::new("steady_fps", fps, "fps", "1000/平均挂钟"));
    slow.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    output::line(format!(
        "steady: {steady_frames} frames in {steady_total:.0} ms -> {fps:.1} fps (avg {avg_wall:.2} ms, engine avg {avg_engine:.2} ms, p95 {p95:.2} ms)"
    ));
    let slowest = slow
        .iter()
        .take(5)
        .map(|(frame, ms)| format!("frame {frame}: {ms:.1} ms"))
        .collect::<Vec<_>>()
        .join(", ");
    output::line(format!("steady slowest 5: {slowest}"));

    // 5. wgpu device 初始化(失败显式记录,不阻断其它项)。
    match measure_wgpu() {
        Ok((ms, adapter, backend)) => {
            rows.push(Row::new("wgpu_init_ms", ms, "ms", "GpuContext::new"));
            output::line(format!(
                "wgpu: {ms:.2} ms adapter={adapter} backend={backend}"
            ));
        }
        Err(message) => {
            rows.push(Row::new("wgpu_init_ms", f64::NAN, "ms", &message));
            output::line(format!("wgpu: SKIP {message}"));
        }
    }

    // 6. wasm presenter 增量构建。
    if skip_wasm {
        output::line("wasm build: skipped (--skip-wasm)");
    } else {
        match measure_wasm(&workspace) {
            Ok(stats) => {
                rows.push(Row::new(
                    "wasm_noop_ms",
                    stats.noop_ms,
                    "ms",
                    "cargo build 无改动",
                ));
                rows.push(Row::new(
                    "wasm_incremental_ms",
                    stats.incremental_ms,
                    "ms",
                    "改 lib.rs mtime 后 cargo build",
                ));
                rows.push(Row::new(
                    "wasm_bindgen_ms",
                    stats.bindgen_ms,
                    "ms",
                    "wasm-bindgen --target web",
                ));
                output::line(format!(
                    "wasm: no-op {:.0} ms; incremental {:.0} ms; bindgen {:.0} ms",
                    stats.noop_ms, stats.incremental_ms, stats.bindgen_ms
                ));
            }
            Err(message) => output::line(format!("wasm: SKIP {message}")),
        }
    }

    // 7. 本地服务帧传输。
    if skip_server {
        output::line("server: skipped (--skip-server)");
    } else {
        match measure_server(Path::new(&root)) {
            Ok(stats) => {
                rows.push(Row::new(
                    "ws_first_frame_ms",
                    stats.first_frame_ms,
                    "ms",
                    "WebSocket 连上→首帧",
                ));
                rows.push(Row::new("ws_fps", stats.fps, "fps", "收到二进制帧的频率"));
                rows.push(Row::new(
                    "ws_input_p95_ms",
                    stats.input_p95_ms,
                    "ms",
                    "输入→下一帧(服务端侧)",
                ));
                rows.push(Row::new(
                    "ws_dropped",
                    stats.dropped as f64,
                    "frames",
                    "/api/status 的 dropped_frames",
                ));
                output::line(format!(
                    "server: first frame {:.0} ms; {:.1} fps; input p95 {:.1} ms; dropped {}",
                    stats.first_frame_ms, stats.fps, stats.input_p95_ms, stats.dropped
                ));
            }
            Err(message) => output::line(format!("server: SKIP {message}")),
        }
    }

    // 门禁(只对照 README 已声明阈值)。
    let mut gates = Vec::new();
    gates.push(gate("boot(off) < 500 ms", off.boot_ms, 500.0, "<="));
    gates.push(gate(
        "re-boot 后可 tick < 1000 ms",
        reboot_ready_ms,
        1000.0,
        "<=",
    ));
    if profile == "release" {
        gates.push(gate("稳态 >= 30 fps", fps, 30.0, ">="));
    }
    let mut all_ok = true;
    for line in &gates {
        if line.contains("FAIL") {
            all_ok = false;
        }
        output::line(line.clone());
    }
    output::line(if all_ok {
        "gates: PASS"
    } else {
        "gates: FAIL(记录实况,不改阈值)"
    });

    let report = report_json(profile, &rows);
    let out_dir = workspace.join("target").join("tmp");
    let _ = std::fs::create_dir_all(&out_dir);
    let out_path = out_dir.join("yu-cli-bench.json");
    std::fs::write(&out_path, report)
        .map_err(|e| Failure::runtime(format!("写 {} 失败: {e}", out_path.display())))?;
    output::line(format!("bench json: {}", out_path.display()));
    Ok(())
}

struct Row {
    name: String,
    value: f64,
    unit: String,
    note: String,
}

impl Row {
    fn new(name: &str, value: f64, unit: &str, note: &str) -> Self {
        Self {
            name: name.to_string(),
            value,
            unit: unit.to_string(),
            note: note.to_string(),
        }
    }
}

struct BootStats {
    engine: Engine,
    boot_ms: f64,
    first_tick_ms: f64,
    visible_frame: u64,
    visible_total_ms: f64,
    per_frame: Vec<(u64, f64, f64)>,
    display_sha256: String,
}

impl BootStats {
    fn rows(&self, tag: &str) -> Vec<Row> {
        vec![
            Row::new(
                &format!("boot_ms[{tag}]"),
                self.boot_ms,
                "ms",
                "表+入口脚本",
            ),
            Row::new(
                &format!("first_tick_ms[{tag}]"),
                self.first_tick_ms,
                "ms",
                "第 1 个逻辑帧",
            ),
            Row::new(
                &format!("visible_total_ms[{tag}]"),
                self.visible_total_ms,
                "ms",
                "到首个可绘制帧(信息项)",
            ),
            Row::new(
                &format!("visible_frame[{tag}]"),
                self.visible_frame as f64,
                "frame",
                "首个可绘制帧号",
            ),
        ]
    }
}

fn measure_boot(project: &Project, record_events: bool, debug_mode: bool) -> CliResult<BootStats> {
    let created = Instant::now();
    let mut engine = Engine::new(
        project.clone(),
        EngineOptions {
            record_events,
            debug_mode,
            ..EngineOptions::default()
        },
    )?;
    let boot_ms = engine.boot_ms();
    let mut per_frame = Vec::new();
    let mut first_tick_ms = 0.0;
    let mut visible_frame = 0;
    let mut visible_total_ms = 0.0;
    for index in 0..12u32 {
        let rendered = engine.tick()?;
        if index == 0 {
            first_tick_ms = rendered.report.tick_ms;
        }
        per_frame.push((
            rendered.report.frame,
            rendered.report.tick_ms,
            rendered.report.render_ms,
        ));
        if rendered.report.drawable {
            visible_frame = rendered.report.frame;
            visible_total_ms = created.elapsed().as_secs_f64() * 1000.0;
            break;
        }
        if index == 11 {
            visible_total_ms = created.elapsed().as_secs_f64() * 1000.0;
            visible_frame = rendered.report.frame;
        }
    }
    let display = engine.display_json().unwrap_or_default();
    Ok(BootStats {
        boot_ms,
        first_tick_ms,
        visible_frame,
        visible_total_ms,
        per_frame,
        display_sha256: sha256_hex(display.as_bytes()),
        engine,
    })
}

fn measure_wgpu() -> Result<(f64, String, String), String> {
    let started = Instant::now();
    let context = yu_backend_wgpu::GpuContext::new(&yu_backend_wgpu::DeviceOptions::default())
        .map_err(|d| format!("GpuContext::new 失败: {d}"))?;
    let ms = started.elapsed().as_secs_f64() * 1000.0;
    Ok((
        ms,
        context.adapter_info.name.clone(),
        context.adapter_info.backend.clone(),
    ))
}

struct WasmStats {
    noop_ms: f64,
    incremental_ms: f64,
    bindgen_ms: f64,
}

fn measure_wasm(workspace: &Path) -> Result<WasmStats, String> {
    let lib = workspace.join("crates/yu-host-wasm/src/lib.rs");
    if !lib.is_file() {
        return Err(format!("{} 不存在", lib.display()));
    }
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let temp = workspace.join("target").join("tmp").join("cargo-temp");
    let _ = std::fs::create_dir_all(&temp);
    let build = |workspace: &Path| -> Result<f64, String> {
        let started = Instant::now();
        let status = Command::new(&cargo)
            .args([
                "build",
                "-p",
                "yu-host-wasm",
                "--target",
                "wasm32-unknown-unknown",
            ])
            .env("TEMP", &temp)
            .env("TMP", &temp)
            .current_dir(workspace)
            .status()
            .map_err(|e| format!("调用 cargo 失败: {e}"))?;
        if !status.success() {
            return Err(format!(
                "wasm 构建失败(status {})",
                status.code().unwrap_or(-1)
            ));
        }
        Ok(started.elapsed().as_secs_f64() * 1000.0)
    };
    let noop_ms = build(workspace)?;
    let original = std::fs::read(&lib).map_err(|e| format!("读 {} 失败: {e}", lib.display()))?;
    std::fs::write(&lib, &original).map_err(|e| format!("写 {} 失败: {e}", lib.display()))?;
    let incremental_ms = build(workspace)?;
    // 内容逐字节还原(只改了 mtime)。
    std::fs::write(&lib, &original).map_err(|e| format!("还原 {} 失败: {e}", lib.display()))?;

    let out_dir = workspace.join("www").join("pkg");
    let _ = std::fs::create_dir_all(&out_dir);
    let wasm = workspace.join("target/wasm32-unknown-unknown/debug/yu_host_wasm.wasm");
    let started = Instant::now();
    let status = Command::new("wasm-bindgen")
        .args([
            "--target",
            "web",
            "--out-dir",
            out_dir.to_string_lossy().as_ref(),
            "--out-name",
            "yu_host_wasm",
            wasm.to_string_lossy().as_ref(),
        ])
        .status()
        .map_err(|e| format!("调用 wasm-bindgen 失败(未安装?): {e}"))?;
    if !status.success() {
        return Err(format!(
            "wasm-bindgen 失败(status {})",
            status.code().unwrap_or(-1)
        ));
    }
    let bindgen_ms = started.elapsed().as_secs_f64() * 1000.0;
    Ok(WasmStats {
        noop_ms,
        incremental_ms,
        bindgen_ms,
    })
}

struct ServerStats {
    first_frame_ms: f64,
    fps: f64,
    input_p95_ms: f64,
    dropped: u64,
}

fn measure_server(root: &Path) -> Result<ServerStats, String> {
    let engine_bin = sibling_engine_binary()?;
    let port = free_port();
    let log_path = workspace_root()
        .join("target")
        .join("tmp")
        .join("bench-engine.log");
    let _ = std::fs::create_dir_all(log_path.parent().unwrap_or(Path::new(".")));
    let log = std::fs::File::create(&log_path)
        .map_err(|e| format!("创建 {} 失败: {e}", log_path.display()))?;
    let log_err = log
        .try_clone()
        .map_err(|e| format!("复制日志句柄失败: {e}"))?;
    let mut child = Command::new(&engine_bin)
        .arg("serve")
        .arg(root)
        .arg("--port")
        .arg(port.to_string())
        .arg("--no-open")
        .arg("--supervised")
        .current_dir(workspace_root())
        .stdin(Stdio::piped())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err))
        .spawn()
        .map_err(|e| format!("启动 {} 失败: {e}", engine_bin.display()))?;
    let child_stdin = child.stdin.take();
    let result = measure_server_inner(port);
    let exited = child.try_wait().ok().flatten();
    drop(child_stdin);
    let _ = child.kill();
    let _ = child.wait();
    if result.is_err() {
        if let Some(status) = exited {
            return Err(format!(
                "{};engine 已退出 {status};日志见 {}",
                result.err().unwrap_or_default(),
                log_path.display()
            ));
        }
    }
    result
}

fn measure_server_inner(port: u16) -> Result<ServerStats, String> {
    let ready = Instant::now();
    while ready.elapsed() < Duration::from_secs(30) {
        if crate::dev::fetch_status(port).is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if crate::dev::fetch_status(port).is_none() {
        return Err("引擎没有在 30 s 内启动".to_string());
    }
    let mut client = WsClient::connect(port)?;
    client.set_timeout(Duration::from_secs(60))?;
    match client.read_message()? {
        Message::Text(_) => {}
        _ => return Err("期望 hello 文本帧".to_string()),
    }
    let connected = Instant::now();
    let _first = read_frame(&mut client, Duration::from_secs(120))?;
    let first_frame_ms = connected.elapsed().as_secs_f64() * 1000.0;

    // 先过掉启动期的大帧(初始化帧/载入转场),再采样 3 s。
    for _ in 0..10 {
        read_frame(&mut client, Duration::from_secs(120))?;
    }

    // 采样 3 s 的帧间隔。
    let sample_started = Instant::now();
    let mut frames = 0u32;
    while sample_started.elapsed() < Duration::from_secs(3) {
        client.set_timeout(Duration::from_secs(5))?;
        read_frame(&mut client, Duration::from_secs(5))?;
        frames += 1;
    }
    let seconds = sample_started.elapsed().as_secs_f64();
    let fps = frames as f64 / seconds;

    // 输入 → 下一帧(20 次)。
    let mut latencies = Vec::new();
    for index in 0..20 {
        let started = Instant::now();
        client.send_text(&format!(
            "{{\"t\":\"cursor\",\"x\":{},\"y\":{}}}",
            100 + index * 10,
            120
        ))?;
        read_frame(&mut client, Duration::from_secs(5))?;
        latencies.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    let input_p95 = percentile(&mut latencies, 95.0);
    let dropped = crate::dev::fetch_status(port)
        .and_then(|text| {
            let key = "\"dropped_frames\":";
            let start = text.find(key)? + key.len();
            let rest = &text[start..];
            let end = rest.find(|c: char| !c.is_ascii_digit())?;
            rest[..end].parse::<u64>().ok()
        })
        .unwrap_or(0);
    Ok(ServerStats {
        first_frame_ms,
        fps,
        input_p95_ms: input_p95,
        dropped,
    })
}

fn read_frame(client: &mut WsClient, timeout: Duration) -> Result<Vec<u8>, String> {
    let deadline = Instant::now() + timeout;
    loop {
        if Instant::now() > deadline {
            return Err("等待二进制帧超时".to_string());
        }
        client.set_timeout(Duration::from_secs(5))?;
        match client.read_message()? {
            Message::Binary(payload) => {
                if payload.len() >= 56 && &payload[0..4] == b"YUF1" {
                    return Ok(payload);
                }
            }
            Message::Text(_) => continue,
            Message::Close => return Err("服务端关闭了 WebSocket".to_string()),
        }
    }
}

fn sibling_engine_binary() -> Result<PathBuf, String> {
    let current = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let name = if cfg!(windows) {
        "yu-engine.exe"
    } else {
        "yu-engine"
    };
    let candidate = current
        .parent()
        .map(|dir| dir.join(name))
        .ok_or_else(|| "current_exe 没有父目录".to_string())?;
    if candidate.is_file() {
        Ok(candidate)
    } else {
        Err(format!(
            "{} 不存在(先 cargo build -p yu-cli --bin yu-engine)",
            candidate.display()
        ))
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/yu-cli -> workspace root")
        .to_path_buf()
}

fn percentile(samples: &mut [f64], pct: f64) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let index = ((pct / 100.0) * (samples.len() - 1) as f64).round() as usize;
    samples[index.min(samples.len() - 1)]
}

fn gate(label: &str, value: f64, threshold: f64, op: &str) -> String {
    let pass = match op {
        ">=" => value >= threshold,
        _ => value <= threshold,
    };
    format!(
        "gate {}: {:.2} {} {:.2} -> {}",
        label,
        value,
        op,
        threshold,
        if pass { "PASS" } else { "FAIL" }
    )
}

fn report_json(profile: &str, rows: &[Row]) -> String {
    let mut out = String::from("{\n");
    out.push_str(&format!("  \"profile\": \"{profile}\",\n"));
    out.push_str(&format!(
        "  \"marker\": \"{}\",\n",
        build_info::ENGINE_MARKER
    ));
    out.push_str("  \"rows\": [\n");
    for (index, row) in rows.iter().enumerate() {
        let value = if row.value.is_finite() {
            format!("{:.3}", row.value)
        } else {
            "null".to_string()
        };
        out.push_str(&format!(
            "    {{\"name\":\"{}\",\"value\":{},\"unit\":\"{}\",\"note\":\"{}\"}}{}\n",
            escape(&row.name),
            value,
            escape(&row.unit),
            escape(&row.note),
            if index + 1 < rows.len() { "," } else { "" }
        ));
    }
    out.push_str("  ]\n}\n");
    out
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

fn short(hash: &str) -> String {
    hash.chars().take(12).collect()
}
