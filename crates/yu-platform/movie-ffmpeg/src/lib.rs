//! 桌面外置影片后端:用系统 `ffmpeg`/`ffprobe` 解码任意容器。
//!
//! 引擎的 `MovieHost` 契约只要求「open 后按时间取 RGBA 帧」;这里把
//! `MOVIE[FILE]` 的字节落到临时文件,`ffprobe` 取宽高/帧率,`ffmpeg`
//! 以 CFR 输出 `rgba` 原始帧。内置 `pl_mpeg` 只支持 MPEG-1 program
//! stream;v555-3 的 `BRANDCALL*.wmv` 等 WMV/H.264 影片必须走本后端。
//!
//! 证据:`MovieBackend::Auto` 的文档约定「有宿主外置能力就用」;桌面
//! `EngineOptions.movie_external_factory` 之前恒为 `None`(只有 Android
//! MediaCodec 注入)。本 crate 只实现宿主任意注入的外置端口,不改 VM
//! 命令语义。
//!
//! 依赖:运行环境 PATH 里要有 `ffmpeg` 与 `ffprobe`。缺失时 `open` 返回
//! 可恢复诊断,`Auto` 选择器回退内置后端(MPEG-1 仍可播)。音频暂不解码
//! (`-an`);画面是 v555 启动链的可见部分。

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use yu_log::{Boundary, Diag};
use yu_rt::event::{MovieAudioInfo, MovieFrame, MovieGeometry, MovieHost};

static SEQ: AtomicU64 = AtomicU64::new(0);

/// 影片尺寸上限(防 `w*h*4` 溢出/超大分配)。4K 有充足余量。
const MAX_MOVIE_DIMENSION: u32 = 8192;

/// 解析 `ffmpeg`/`ffprobe` 的绝对路径。
///
/// Windows `CreateProcess` 的搜索顺序含应用目录与当前目录,而**游戏目录是
/// 不可信数据目录**:不能直接 `Command::new("ffmpeg")`。这里：
/// 1. 显式 `YURIS_FFMPEG` / `YURIS_FFPROBE` 覆盖(要求存在);
/// 2. 否则只搜 PATH 的**绝对目录**项,跳过空项/相对项;
/// 3. 找不到就报可恢复诊断,由 `Auto` 回落内置后端。
fn resolve_tool(name: &str) -> Result<PathBuf, Diag> {
    let env_key = format!("YURIS_{}", name.to_ascii_uppercase());
    if let Some(value) = std::env::var_os(&env_key) {
        let path = PathBuf::from(value);
        if path.is_file() {
            return Ok(path);
        }
        return Err(Diag::recoverable("movie.decode", Boundary::Data).with_detail(format!(
            "{env_key}={} 不存在或不是文件",
            path.display()
        )));
    }
    let exe = format!("{name}.exe");
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            if dir.as_os_str().is_empty() || !dir.is_absolute() {
                continue;
            }
            for candidate in [dir.join(&exe), dir.join(name)] {
                if candidate.is_file() {
                    return Ok(candidate);
                }
            }
        }
    }
    Err(Diag::recoverable("movie.decode", Boundary::Data).with_detail(format!(
        "未找到 {name}:请安装并加入 PATH 的绝对目录,或设置 {env_key}=<绝对路径>"
    )))
}

/// 运行 ffprobe 并带 10s 超时(畸形容器可能让 ffprobe 永不返回)。
fn run_probe(tool: &Path, path: &Path) -> Result<std::process::Output, Diag> {
    let mut command = Command::new(tool);
    command
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,avg_frame_rate,r_frame_rate",
            "-of",
            "csv=p=0:s=,",
        ])
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| {
        Diag::recoverable("movie.decode", Boundary::Data)
            .with_detail(format!("ffprobe 启动失败({}): {e}", tool.display()))
    })?;
    let mut stdout = child.stdout.take().expect("ffprobe stdout piped");
    let mut stderr = child.stderr.take().expect("ffprobe stderr piped");
    let out_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let err_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf);
        buf
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Diag::recoverable("movie.decode", Boundary::Data)
                    .with_detail("ffprobe 超过 10s 未返回(畸形容器?)"));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Diag::recoverable("movie.decode", Boundary::Data)
                    .with_detail(format!("ffprobe 等待失败: {e}")));
            }
        }
    };
    Ok(std::process::Output {
        status,
        stdout: out_thread.join().unwrap_or_default(),
        stderr: err_thread.join().unwrap_or_default(),
    })
}

/// ffmpeg/ffprobe 子进程后端。
pub struct FfmpegMovieHost {
    playing: bool,
    media: Option<(u32, u32)>,
    width: u32,
    height: u32,
    fps_num: u32,
    fps_den: u32,
    frame_size: usize,
    produced: u64,
    has_frame: bool,
    last_frame: Vec<u8>,
    child: Option<Child>,
    stdout: Option<ChildStdout>,
    temp_path: Option<PathBuf>,
    anchor_ms: Option<u64>,
    accum_ms: u64,
    last_now_ms: u64,
    ended: bool,
}

impl Default for FfmpegMovieHost {
    fn default() -> Self {
        Self::new()
    }
}

impl FfmpegMovieHost {
    #[must_use]
    pub fn new() -> Self {
        Self {
            playing: false,
            media: None,
            width: 0,
            height: 0,
            fps_num: 30,
            fps_den: 1,
            frame_size: 0,
            produced: 0,
            has_frame: false,
            last_frame: Vec::new(),
            child: None,
            stdout: None,
            temp_path: None,
            anchor_ms: None,
            accum_ms: 0,
            last_now_ms: 0,
            ended: false,
        }
    }

    fn cleanup(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.stdout = None;
        if let Some(path) = self.temp_path.take() {
            let _ = std::fs::remove_file(path);
        }
        self.media = None;
        self.width = 0;
        self.height = 0;
        self.frame_size = 0;
        self.produced = 0;
        self.has_frame = false;
        self.last_frame.clear();
        self.playing = false;
        self.ended = false;
        self.anchor_ms = None;
        self.accum_ms = 0;
        self.last_now_ms = 0;
    }

    fn current(&self) -> Option<MovieFrame<'_>> {
        if !self.has_frame {
            return None;
        }
        Some(MovieFrame {
            width: self.width,
            height: self.height,
            rgba: &self.last_frame,
        })
    }

    fn probe(path: &PathBuf) -> Result<(u32, u32, u32, u32), Diag> {
        let tool = resolve_tool("ffprobe")?;
        let output = run_probe(&tool, path)?;
        if !output.status.success() {
            return Err(Diag::recoverable("movie.decode", Boundary::Data).with_detail(format!(
                "ffprobe 无法解析影片容器: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let line = text.lines().next().unwrap_or("").trim();
        let mut fields = line.split(',');
        let width: u32 = fields
            .next()
            .and_then(|v| v.trim().parse().ok())
            .filter(|v| *v > 0)
            .ok_or_else(|| {
                Diag::recoverable("movie.decode", Boundary::Data)
                    .with_detail(format!("ffprobe 未给出有效宽度: {line}"))
            })?;
        let height: u32 = fields
            .next()
            .and_then(|v| v.trim().parse().ok())
            .filter(|v| *v > 0)
            .ok_or_else(|| {
                Diag::recoverable("movie.decode", Boundary::Data)
                    .with_detail(format!("ffprobe 未给出有效高度: {line}"))
            })?;
        let avg = fields.next().unwrap_or("").trim();
        let raw = fields.next().unwrap_or("").trim();
        let (num, den) = parse_rate(avg)
            .or_else(|| parse_rate(raw))
            .unwrap_or((30, 1));
        Ok((width, height, num, den))
    }

    fn spawn_ffmpeg(path: &PathBuf, num: u32, den: u32) -> Result<Child, Diag> {
        let tool = resolve_tool("ffmpeg")?;
        Command::new(tool)
            .args(["-v", "error", "-nostdin", "-i"])
            .arg(path)
            .args([
                "-an",
                "-sn",
                "-dn",
                "-r",
                &format!("{num}/{den}"),
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgba",
                "pipe:1",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| {
                Diag::recoverable("movie.decode", Boundary::Data)
                    .with_detail(format!("ffmpeg 启动失败(外置解码需要 PATH 中有 ffmpeg): {e}"))
            })
    }
}

fn write_temp(bytes: &[u8], ext: &str) -> Result<PathBuf, Diag> {
    // 只在系统临时目录落盘:游戏目录/工作目录是不可信数据区,不应写入。
    let mut candidates = vec![std::env::temp_dir()];
    for key in ["TEMP", "TMP", "LOCALAPPDATA", "USERPROFILE"] {
        if let Some(value) = std::env::var_os(key) {
            let base = PathBuf::from(value);
            candidates.push(if key == "LOCALAPPDATA" {
                base.join("Temp")
            } else if key == "USERPROFILE" {
                base.join("AppData").join("Local").join("Temp")
            } else {
                base
            });
        }
    }
    let mut last = String::new();
    for dir in candidates {
        if std::fs::create_dir_all(&dir).is_err() {
            continue;
        }
        for _ in 0..4 {
            let name = format!(
                "yk-movie-{}-{}{ext}",
                std::process::id(),
                SEQ.fetch_add(1, Ordering::Relaxed)
            );
            let path = dir.join(name);
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&path) {
                Ok(mut out) => {
                    if let Err(e) = out.write_all(bytes) {
                        last = format!("{}: {e}", path.display());
                        let _ = out.flush();
                        drop(out);
                        let _ = std::fs::remove_file(&path);
                        break;
                    }
                    return Ok(path);
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => {
                    last = format!("{}: {e}", path.display());
                    break;
                }
            }
        }
    }
    Err(Diag::recoverable("movie.decode", Boundary::Data).with_detail(format!(
        "创建影片临时文件失败(只在系统临时目录尝试): {last}"
    )))
}

fn parse_rate(text: &str) -> Option<(u32, u32)> {
    let (num, den) = text.split_once('/')?;
    let num: u32 = num.trim().parse().ok()?;
    let den: u32 = den.trim().parse().ok()?;
    (num > 0 && den > 0).then_some((num, den))
}

impl MovieHost for FfmpegMovieHost {
    fn open(&mut self, file: &[u8], bytes: &[u8]) -> Result<(), Diag> {
        self.cleanup();
        if bytes.is_empty() {
            return Err(Diag::recoverable("movie.decode", Boundary::Data)
                .with_detail("影片资产为空".to_string()));
        }
        let file_text = String::from_utf8_lossy(file);
        let ext = std::path::Path::new(file_text.as_ref())
            .extension()
            .and_then(|e| e.to_str())
            .filter(|e| e.len() <= 5 && e.chars().all(|c| c.is_ascii_alphanumeric()))
            .map(|e| format!(".{e}"))
            .unwrap_or_else(|| ".bin".to_string());
        let path = write_temp(bytes, &ext)?;
        self.temp_path = Some(path.clone());
        let (width, height, num, den) = Self::probe(&path).inspect_err(|_| {
            let _ = std::fs::remove_file(&path);
        })?;
        let mut child = Self::spawn_ffmpeg(&path, num, den)?;
        let Some(stdout) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(&path);
            return Err(Diag::recoverable("movie.decode", Boundary::Data)
                .with_detail("ffmpeg 没有可读的 stdout".to_string()));
        };
        self.width = width;
        self.height = height;
        if width > MAX_MOVIE_DIMENSION || height > MAX_MOVIE_DIMENSION {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(&path);
            return Err(Diag::recoverable("movie.decode", Boundary::Data).with_detail(format!(
                "影片尺寸 {width}x{height} 超过上限 {MAX_MOVIE_DIMENSION}"
            )));
        }
        let frame_size = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or_else(|| {
                Diag::recoverable("movie.decode", Boundary::Data)
                    .with_detail(format!("影片尺寸 {width}x{height} 的帧缓冲溢出"))
            })?;
        self.fps_num = num;
        self.fps_den = den;
        self.frame_size = frame_size;
        self.last_frame = vec![0u8; self.frame_size];
        self.has_frame = false;
        self.produced = 0;
        self.child = Some(child);
        self.stdout = Some(stdout);
        self.media = Some((width, height));
        self.playing = false;
        self.ended = false;
        self.anchor_ms = None;
        self.accum_ms = 0;
        self.last_now_ms = 0;
        Ok(())
    }

    fn set_playing(&mut self, playing: bool) -> Result<(), Diag> {
        if playing == self.playing {
            return Ok(());
        }
        if !playing {
            let now = self.last_now_ms;
            let anchor = self.anchor_ms.take().unwrap_or(now);
            self.accum_ms = self
                .accum_ms
                .saturating_add(now.saturating_sub(anchor));
            self.playing = false;
        } else if !self.ended {
            self.playing = true;
            self.anchor_ms = None;
        }
        Ok(())
    }

    fn set_volume(&mut self, _volume: i64) {}

    fn set_geometry(&mut self, _geometry: MovieGeometry) {}

    fn is_playing(&mut self) -> bool {
        self.playing && !self.ended
    }

    fn media_size(&mut self) -> Option<(u32, u32)> {
        self.media
    }

    fn frame(&mut self, now_ms: u64) -> Option<MovieFrame<'_>> {
        self.last_now_ms = now_ms;
        if self.playing && !self.ended {
            let anchor = *self.anchor_ms.get_or_insert_with(|| {
                now_ms.saturating_sub(self.accum_ms)
            });
            let elapsed = now_ms.saturating_sub(anchor);
            let target = u128::from(elapsed)
                .saturating_mul(u128::from(self.fps_num))
                / (1000u128 * u128::from(self.fps_den));
            let target = u64::try_from(target).unwrap_or(u64::MAX);
            let mut budget = 240u32;
            while self.produced <= target && budget > 0 && !self.ended {
                budget -= 1;
                let Some(stdout) = self.stdout.as_mut() else {
                    self.ended = true;
                    self.playing = false;
                    break;
                };
                match stdout.read_exact(&mut self.last_frame) {
                    Ok(()) => {
                        self.produced += 1;
                        self.has_frame = true;
                    }
                    Err(_) => {
                        self.ended = true;
                        self.playing = false;
                        break;
                    }
                }
            }
        }
        self.current()
    }

    fn take_audio(&mut self, _out: &mut Vec<i16>) -> Option<MovieAudioInfo> {
        None
    }

    fn backend_label(&self) -> &'static str {
        "ffmpeg"
    }

    fn close(&mut self) {
        self.cleanup();
    }
}

impl Drop for FfmpegMovieHost {
    fn drop(&mut self) {
        self.cleanup();
    }
}
