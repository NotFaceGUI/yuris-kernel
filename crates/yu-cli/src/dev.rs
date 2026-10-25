//! `yu-cli dev`:HMR 监督进程。
//!
//! 监督进程本身**不重建**(Windows 上运行中的 exe 不可覆盖)。它只负责:
//!
//! 1. 构建并启动 `yu-serve serve` 子进程(stdin 作为控制通道);
//! 2. 500 ms 指纹轮询 `crates/**/*.rs` + `Cargo.toml`;
//! 3. Rust 变更 → 结束子进程 → 重建 → 重启(浏览器 WebSocket 自动重连);
//! 4. `www/**` 变更 → 向子进程 stdin 发 `reload` → SSE → 页面刷新;
//! 5. 每轮记录构建/端到端耗时并打印。
//!
//! 架构 B 下浏览器不重建 wasm:代码在 native 引擎里,页面只是呈现端。

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, UNIX_EPOCH};

use crate::args::ArgReader;
use crate::engine::Backend;
use crate::output;
use crate::{CliResult, Failure};

const POLL: Duration = Duration::from_millis(500);
const DEBOUNCE: Duration = Duration::from_millis(300);
const HEALTH_TIMEOUT: Duration = Duration::from_secs(30);

pub fn from_args(tokens: Vec<String>) -> CliResult<()> {
    let mut args = ArgReader::new(tokens);
    let root = args
        .positional(0)
        .ok_or_else(|| Failure::usage("用法: yu-cli dev <project-root> [...]"))?;
    let port = match args.flag("port") {
        Some(value) => value
            .parse::<u16>()
            .map_err(|_| Failure::usage(format!("--port 需要 0..65535,收到 `{value}`")))?,
        None => 4173,
    };
    let no_open = args.switch("no-open");
    let backend = match args.flag("backend") {
        Some(value) => Backend::parse(&value)?,
        None => Backend::Soft,
    };
    // 默认 release:实测 debug 的 ERIS 初始化帧 ~14 s,页面首帧不可接受。
    // `--debug` 显式换回 debug(要符号/断点时用)。
    let release = !args.switch("debug");
    // `@_DEBUGMODE` 注入(DIV-L-001;debug 语料对拍口径),透传给引擎子进程。
    let debug_mode = match args.flag("debug-mode") {
        Some(value) => ArgReader::on_off("debug-mode", &value)?,
        None => false,
    };
    args.finish()?;

    let workspace = workspace_root();
    let engine_bin = engine_binary(&workspace, release);
    let watch = collect_watch_files(&workspace)?;
    if watch.is_empty() {
        return Err(Failure::runtime(format!(
            "没有找到可监视的 Rust 源文件:{}",
            workspace.join("crates").display()
        )));
    }

    output::line(format!(
        "dev: workspace={} engine={} profile={} port={} watch_files={}",
        workspace.display(),
        engine_bin.display(),
        if release { "release" } else { "debug" },
        port,
        watch.len()
    ));

    let mut state = Supervisor {
        workspace,
        engine_bin,
        root: PathBuf::from(root),
        port,
        backend,
        no_open,
        release,
        debug_mode,
        child: None,
        stdin: None,
        first_spawn: true,
        rust_fingerprint: 0,
        www_fingerprint: 0,
        builds: 0,
    };
    state.rust_fingerprint = fingerprint(&watch);
    state.www_fingerprint = fingerprint(&collect_www_files(&state.workspace.join("www")));
    state.start_child()?;
    state.wait_healthy()?;
    output::line(format!(
        "dev ready: http://127.0.0.1:{}/ (改 Rust 会自动重建;Ctrl-C 退出)",
        state.port
    ));

    loop {
        thread::sleep(POLL);
        let rust_changed = {
            let current = fingerprint(&watch);
            if current != state.rust_fingerprint {
                // 去抖:等文件集合稳定。
                let mut current = current;
                loop {
                    thread::sleep(DEBOUNCE);
                    let next = fingerprint(&watch);
                    if next == current {
                        break;
                    }
                    current = next;
                }
                state.rust_fingerprint = current;
                true
            } else {
                false
            }
        };
        let www_changed = {
            let files = collect_www_files(&state.workspace.join("www"));
            let current = fingerprint(&files);
            if current != state.www_fingerprint {
                state.www_fingerprint = current;
                true
            } else {
                false
            }
        };
        if rust_changed {
            state.rebuild_and_restart()?;
        }
        if www_changed {
            // 页面资源变了:让子进程广播 SSE reload。
            if let Some(stdin) = state.stdin.as_mut() {
                let _ = stdin.write_all(b"reload\n");
                let _ = stdin.flush();
                output::line("dev: www changed -> reload");
            }
        }
    }
}

struct Supervisor {
    workspace: PathBuf,
    engine_bin: PathBuf,
    root: PathBuf,
    port: u16,
    backend: Backend,
    no_open: bool,
    release: bool,
    debug_mode: bool,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    first_spawn: bool,
    rust_fingerprint: u64,
    www_fingerprint: u64,
    builds: u32,
}

impl Supervisor {
    fn start_child(&mut self) -> CliResult<()> {
        let mut command = Command::new(&self.engine_bin);
        command
            .arg("serve")
            .arg(&self.root)
            .arg("--port")
            .arg(self.port.to_string())
            .arg("--backend")
            .arg(self.backend.as_str())
            .arg("--supervised")
            .current_dir(&self.workspace)
            .stdin(Stdio::piped())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        if self.no_open || !self.first_spawn {
            command.arg("--no-open");
        }
        if self.debug_mode {
            command.arg("--debug-mode").arg("on");
        }
        let mut child = command.spawn().map_err(|e| {
            Failure::runtime(format!(
                "启动 {} 失败: {e}(先执行 cargo build -p yu-cli --bin yu-serve?)",
                self.engine_bin.display()
            ))
        })?;
        self.stdin = child.stdin.take();
        self.child = Some(child);
        self.first_spawn = false;
        Ok(())
    }

    fn wait_healthy(&self) -> CliResult<()> {
        let started = Instant::now();
        while started.elapsed() < HEALTH_TIMEOUT {
            if http_ok(self.port, "/healthz") {
                output::line(format!(
                    "dev: engine healthy in {:.0} ms",
                    started.elapsed().as_secs_f64() * 1000.0
                ));
                return Ok(());
            }
            thread::sleep(Duration::from_millis(100));
        }
        Err(Failure::runtime(format!(
            "引擎在 {:.0} s 内没有通过 /healthz(端口 {});看上面的错误输出",
            HEALTH_TIMEOUT.as_secs_f64(),
            self.port
        )))
    }

    fn stop_child(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.stdin = None;
    }

    fn rebuild_and_restart(&mut self) -> CliResult<()> {
        let detected = Instant::now();
        self.builds += 1;
        output::line(format!(
            "dev: change detected (#{}) -> rebuild",
            self.builds
        ));
        self.stop_child();
        let build_started = Instant::now();
        let temp = self.workspace.join("target").join("tmp").join("cargo-temp");
        let _ = fs::create_dir_all(&temp);
        let mut command = Command::new(cargo());
        command.args(["build", "-p", "yu-cli", "--bin", "yu-serve"]);
        if self.release {
            command.arg("--release");
        }
        // 系统 TEMP 在本机不可写(AGENTS §2);链接器需要可写的临时目录。
        command.env("TEMP", &temp).env("TMP", &temp);
        let status = command
            .current_dir(&self.workspace)
            .status()
            .map_err(|e| Failure::runtime(format!("调用 cargo 失败: {e}")))?;
        if !status.success() {
            output::error(format!(
                "dev: 构建失败(status {});保持当前页面,修正后再次修改触发重试",
                status.code().unwrap_or(-1)
            ));
            // 重新拉起旧二进制,避免服务完全消失。
            self.start_child()?;
            self.wait_healthy()?;
            return Ok(());
        }
        let build_ms = build_started.elapsed().as_secs_f64() * 1000.0;
        self.start_child()?;
        self.wait_healthy()?;
        let total_ms = detected.elapsed().as_secs_f64() * 1000.0;
        output::line(format!(
            "dev: rebuilt + restarted in {:.0} ms (build {:.0} ms)",
            total_ms, build_ms
        ));
        Ok(())
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        self.stop_child();
    }
}

fn cargo() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string())
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/yu-cli -> workspace root")
        .to_path_buf()
}

fn engine_binary(workspace: &Path, release: bool) -> PathBuf {
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace.join("target"));
    let name = if cfg!(windows) {
        "yu-serve.exe"
    } else {
        "yu-serve"
    };
    target
        .join(if release { "release" } else { "debug" })
        .join(name)
}

fn collect_watch_files(workspace: &Path) -> CliResult<Vec<PathBuf>> {
    let mut files = Vec::new();
    walk(&workspace.join("crates"), &mut files, true)?;
    let root_manifest = workspace.join("Cargo.toml");
    if root_manifest.is_file() {
        files.push(root_manifest);
    }
    files.sort();
    Ok(files)
}

fn collect_www_files(www: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let _ = walk(www, &mut files, false);
    files.sort();
    files
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>, rust_only: bool) -> CliResult<()> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return Ok(()),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "target" || name == ".git" {
            continue;
        }
        if path.is_dir() {
            walk(&path, out, rust_only)?;
        } else if path.is_file() {
            let take = if rust_only {
                path.extension().and_then(|e| e.to_str()) == Some("rs") || name == "Cargo.toml"
            } else {
                true
            };
            if take {
                out.push(path);
            }
        }
    }
    Ok(())
}

fn fingerprint(files: &[PathBuf]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |value: u64| {
        hash ^= value;
        hash = hash.wrapping_mul(0x100_0000_01b3);
    };
    for path in files {
        if let Ok(meta) = fs::metadata(path) {
            mix(meta.len());
            if let Ok(modified) = meta.modified() {
                if let Ok(since) = modified.duration_since(UNIX_EPOCH) {
                    mix(since.as_nanos() as u64);
                }
            }
        }
        for byte in path.to_string_lossy().as_bytes() {
            mix(*byte as u64);
        }
    }
    hash
}

fn http_ok(port: u16, path: &str) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(250)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    let request = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    response.starts_with("HTTP/1.1 200") && response.contains("ok")
}

/// `/api/status` 轮询(HMR 冒烟测试复用)。
pub fn fetch_status(port: u16) -> Option<String> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(500)).ok()?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let request = "GET /api/status HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n";
    stream.write_all(request.as_bytes()).ok()?;
    let mut response = String::new();
    stream.read_to_string(&mut response).ok()?;
    let body = response.split("\r\n\r\n").nth(1)?;
    Some(body.to_string())
}

/// 环境变量里可用的端口(测试用;冲突时由调用方换)。
pub fn free_port() -> u16 {
    use std::net::TcpListener;
    TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .map(|addr| addr.port())
        .unwrap_or(4173)
}

/// 供测试/诊断:当前监视文件名列表。
pub fn watch_summary(workspace: &Path) -> BTreeMap<String, u64> {
    let mut out = BTreeMap::new();
    if let Ok(files) = collect_watch_files(workspace) {
        for path in files {
            let stamp = fs::metadata(&path)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0);
            out.insert(path.display().to_string(), stamp);
        }
    }
    out
}
