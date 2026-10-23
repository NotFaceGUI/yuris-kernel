//! `serve_probe` · `yu-cli serve` 的 WS 场景探针(证据工具)。
//!
//! 连接 `ws://127.0.0.1:<port>/ws`,按**逻辑帧**注入指针/滚轮事件,
//! 记录:收到多少帧、帧号是否连续、画面指纹是否变化、最长停住时长、
//! 结束时进程是否仍可服务(`/api/report` 可读)。
//!
//! ```text
//! cargo run -p yu-host-desktop --example serve_probe -- \
//!     4173 --seconds 30 --out target/tmp/p1 [--scenario s.json]
//! ```
//!
//! 场景文件(与桌面宿主的 `--script` 同形,但只认指针/滚轮):
//!
//! ```json
//! { "name": "serve-click", "actions": [
//!     { "at_frame": 300, "pointer": 0, "x": 217, "y": 281 },
//!     { "at_frame": 440, "wheel": -1 }
//! ] }
//! ```
//!
//! 这里不实现 WebSocket 协议本身:复用 `yu_cli::ws_client`(项目自带的
//! 受控客户端,只覆盖本服务端的帧格式)。

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use yu_cli::json::{self, Value};
use yu_cli::ws_client::{Message, WsClient};

const FRAME_HEADER: usize = 56;

fn main() {
    if let Err(error) = run() {
        eprintln!("serve_probe: {error}");
        std::process::exit(2);
    }
}

#[derive(Clone, Debug)]
enum Action {
    Pointer { button: i64, x: i64, y: i64 },
    Wheel { dy: f64 },
}

#[derive(Clone, Debug)]
struct Scheduled {
    at_frame: u64,
    action: Action,
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let port: u16 = args
        .first()
        .ok_or_else(usage)?
        .parse()
        .map_err(|_| format!("端口非法: {}", args[0]))?;
    let mut seconds = 30.0f64;
    let mut out = PathBuf::from("target/tmp/p1");
    let mut scenario: Option<PathBuf> = None;
    let mut name = "serve".to_string();
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--seconds" => {
                seconds = args
                    .get(index + 1)
                    .ok_or_else(usage)?
                    .parse()
                    .map_err(|_| "--seconds 需要数字".to_string())?;
                index += 2;
            }
            "--out" => {
                out = PathBuf::from(args.get(index + 1).ok_or_else(usage)?);
                index += 2;
            }
            "--scenario" => {
                scenario = Some(PathBuf::from(args.get(index + 1).ok_or_else(usage)?));
                index += 2;
            }
            other => return Err(format!("未知参数 `{other}`\n{}", usage())),
        }
    }
    let mut scheduled = Vec::new();
    if let Some(path) = &scenario {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("读 {} 失败: {e}", path.display()))?;
        let value = json::parse(&text).map_err(|e| format!("解析 {} 失败: {e}", path.display()))?;
        name = value
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("serve")
            .to_string();
        if let Some(list) = value.get("actions").and_then(Value::as_array) {
            for item in list {
                let at_frame = item
                    .get("at_frame")
                    .or_else(|| item.get("frame"))
                    .and_then(Value::as_u64)
                    .ok_or_else(|| "actions[].at_frame 缺失".to_string())?;
                let action = if let Some(button) = item.get("pointer").and_then(Value::as_f64) {
                    Action::Pointer {
                        button: button as i64,
                        x: item.get("x").and_then(Value::as_f64).unwrap_or(0.0) as i64,
                        y: item.get("y").and_then(Value::as_f64).unwrap_or(0.0) as i64,
                    }
                } else if let Some(dy) = item.get("wheel").and_then(Value::as_f64) {
                    Action::Wheel { dy }
                } else {
                    return Err("actions[] 需要 pointer 或 wheel".to_string());
                };
                scheduled.push(Scheduled { at_frame, action });
            }
        }
    }
    std::fs::create_dir_all(&out).map_err(|e| format!("创建 {} 失败: {e}", out.display()))?;

    let mut client = WsClient::connect(port)?;
    client.set_timeout(Duration::from_millis(250))?;
    let started = Instant::now();
    let deadline = started + Duration::from_secs_f64(seconds);
    let mut report = Report {
        scenario: name.clone(),
        seconds,
        ..Report::default()
    };
    let mut sent = 0usize;
    let mut last_message = Instant::now();
    let mut status_json: Option<String> = None;

    while Instant::now() < deadline {
        match client.read_message() {
            Ok(Message::Text(text)) => {
                last_message = Instant::now();
                report.text_messages += 1;
                if status_json.is_none() {
                    status_json = Some(text.clone());
                }
                report.last_status = Some(text);
            }
            Ok(Message::Binary(bytes)) => {
                last_message = Instant::now();
                report.observe_binary(&bytes, &started)?;
                // 到帧的动作:注入前先发光标(引擎读 @_MOUSE_X/Y)。
                while sent < scheduled.len() && report.last_frame >= scheduled[sent].at_frame {
                    let item = scheduled[sent].clone();
                    match item.action {
                        Action::Pointer { button, x, y } => {
                            client
                                .send_text(&format!("{{\"t\":\"cursor\",\"x\":{x},\"y\":{y}}}"))?;
                            client.send_text(&format!(
                                "{{\"t\":\"pointer\",\"button\":{button},\"pressed\":true}}"
                            ))?;
                            client.send_text(&format!(
                                "{{\"t\":\"pointer\",\"button\":{button},\"pressed\":false}}"
                            ))?;
                        }
                        Action::Wheel { dy } => {
                            client.send_text(&format!(
                                "{{\"t\":\"cursor\",\"x\":{},\"y\":{}}}",
                                report.last_frame % 800,
                                300
                            ))?;
                            client.send_text(&format!("{{\"t\":\"wheel\",\"dy\":{dy}}}"))?;
                        }
                    }
                    report.actions_sent.push(item.at_frame);
                    sent += 1;
                }
            }
            Ok(Message::Close) => {
                report.closed = true;
                break;
            }
            Err(_) => {
                if last_message.elapsed() > Duration::from_secs(5) {
                    report.silent_5s = true;
                }
            }
        }
    }

    report.api_report = fetch_http(port, "/api/report").ok();
    report.api_status = fetch_http(port, "/api/status").ok();
    report.status_json = status_json;
    let summary = report.to_json();
    let path = out.join(format!("{name}.serve.json"));
    std::fs::write(&path, summary.as_bytes())
        .map_err(|e| format!("写 {} 失败: {e}", path.display()))?;
    println!("{summary}");
    println!("summary: {}", path.display());
    Ok(())
}

fn usage() -> String {
    "用法: serve_probe <port> [--seconds N] [--scenario s.json] [--out DIR]".to_string()
}

#[derive(Default)]
struct Report {
    scenario: String,
    seconds: f64,
    text_messages: u64,
    frames: u64,
    first_frame: Option<u64>,
    last_frame: u64,
    frame_regressions: u64,
    /// 相邻两帧的到达时间差(毫秒)的最大值。
    max_gap_ms: u128,
    /// 画面指纹相同的连续帧最大值。
    stall_frames: u64,
    current_stall: u64,
    last_fingerprint: u64,
    distinct_fingerprints: u64,
    changes: Vec<u64>,
    actions_sent: Vec<u64>,
    closed: bool,
    silent_5s: bool,
    last_status: Option<String>,
    status_json: Option<String>,
    api_report: Option<String>,
    api_status: Option<String>,
    drawable_frames: u64,
    ops_seen: BTreeMap<u32, u64>,
    last_arrival: Option<Instant>,
}

impl Report {
    fn observe_binary(&mut self, bytes: &[u8], _started: &Instant) -> Result<(), String> {
        if bytes.len() < FRAME_HEADER || &bytes[0..4] != b"YUF1" {
            return Ok(());
        }
        let width = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        let height = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
        let frame = u64::from_le_bytes(bytes[12..20].try_into().unwrap());
        let ops = u32::from_le_bytes(bytes[20..24].try_into().unwrap());
        let flags = bytes[49];
        let rgba_len = u32::from_le_bytes(bytes[52..56].try_into().unwrap()) as usize;
        if FRAME_HEADER + rgba_len > bytes.len() {
            return Ok(());
        }
        let rgba = &bytes[FRAME_HEADER..FRAME_HEADER + rgba_len];
        let fingerprint = sample_fingerprint(rgba);
        self.frames += 1;
        self.first_frame.get_or_insert(frame);
        if frame + 1 < self.last_frame {
            self.frame_regressions += 1;
        }
        self.last_frame = frame;
        if flags & 1 != 0 {
            self.drawable_frames += 1;
        }
        *self.ops_seen.entry(ops).or_insert(0) += 1;
        let now = Instant::now();
        if let Some(previous) = self.last_arrival {
            let gap = now.duration_since(previous).as_millis();
            self.max_gap_ms = self.max_gap_ms.max(gap);
        }
        self.last_arrival = Some(now);
        if fingerprint == self.last_fingerprint {
            self.current_stall += 1;
            self.stall_frames = self.stall_frames.max(self.current_stall);
        } else {
            self.current_stall = 0;
            self.last_fingerprint = fingerprint;
            self.distinct_fingerprints += 1;
            self.changes.push(frame);
        }
        let _ = (width, height);
        Ok(())
    }

    fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        out.push_str(&format!(
            "  \"scenario\": {},\n  \"seconds\": {},\n",
            json_string(&self.scenario),
            self.seconds
        ));
        out.push_str(&format!(
            "  \"text_messages\": {},\n  \"frames\": {},\n  \"first_frame\": {},\n  \
             \"last_frame\": {},\n  \"frame_regressions\": {},\n  \"max_gap_ms\": {},\n",
            self.text_messages,
            self.frames,
            self.first_frame
                .map(|v| v.to_string())
                .unwrap_or_else(|| "null".to_string()),
            self.last_frame,
            self.frame_regressions,
            self.max_gap_ms
        ));
        out.push_str(&format!(
            "  \"drawable_frames\": {},\n  \"distinct_fingerprints\": {},\n  \
             \"stall_frames\": {},\n",
            self.drawable_frames, self.distinct_fingerprints, self.stall_frames
        ));
        out.push_str("  \"change_frames\": [");
        for (index, frame) in self.changes.iter().enumerate() {
            if index > 0 {
                out.push_str(", ");
            }
            out.push_str(&frame.to_string());
        }
        out.push_str("],\n  \"actions_sent_at\": [");
        for (index, frame) in self.actions_sent.iter().enumerate() {
            if index > 0 {
                out.push_str(", ");
            }
            out.push_str(&frame.to_string());
        }
        out.push_str("],\n");
        out.push_str(&format!(
            "  \"closed\": {},\n  \"silent_5s\": {},\n  \"ops_histogram\": {{",
            self.closed, self.silent_5s
        ));
        let mut first = true;
        for (ops, count) in &self.ops_seen {
            if !first {
                out.push_str(", ");
            }
            first = false;
            out.push_str(&format!("\"{ops}\": {count}"));
        }
        out.push_str("},\n");
        out.push_str(&format!(
            "  \"last_status\": {},\n  \"api_status\": {},\n  \"api_report\": {}\n",
            json_string(self.last_status.as_deref().unwrap_or("")),
            json_string(self.api_status.as_deref().unwrap_or("")),
            json_string(self.api_report.as_deref().unwrap_or(""))
        ));
        out.push_str("}\n");
        out
    }
}

fn sample_fingerprint(rgba: &[u8]) -> u64 {
    if rgba.is_empty() {
        return 0;
    }
    let stride = (rgba.len() / 512).max(1);
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut index = 0;
    while index < rgba.len() {
        hash ^= u64::from(rgba[index]);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        index += stride;
    }
    hash ^ rgba.len() as u64
}

fn fetch_http(port: u16, path: &str) -> Result<String, String> {
    let mut stream =
        TcpStream::connect(("127.0.0.1", port)).map_err(|e| format!("connect: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| format!("timeout: {e}"))?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )
    .map_err(|e| format!("write: {e}"))?;
    let mut buffer = Vec::new();
    stream
        .read_to_end(&mut buffer)
        .map_err(|e| format!("read: {e}"))?;
    let text = String::from_utf8_lossy(&buffer).to_string();
    let body = text.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
    Ok(body)
}

fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
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
