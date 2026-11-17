//! 全屏影片后端装配:内置 pl_mpeg + 宿主注入的外置后端。
//!
//! 内核只做 `MOVIE` 语义(`yu-rt`),解码/呈现全部在
//! [`yu_rt::event::MovieHost`] 之后。这里负责**选择**:
//!
//! - `Builtin` → [`yu_movie_plmpeg::PlmMovieHost`](pl_mpeg 纯软件 MPEG-1);
//! - `External` → 宿主工厂创建的实例(Android: MediaExtractor + MediaCodec);
//! - `Auto` → 外置优先,`open` 失败回退内置。
//!
//! `External` **不静默回退**:用户显式选了设备解码,文件不受支持时必须
//! 在诊断/日志里可见,否则会误以为“外置能用”。`Auto` 才做回退,并在
//! 回退时记一条 info(官方 MPEG-1 语料在 Android 上走的就是这条路)。

use yu_log::{Boundary, Diag};
use yu_rt::event::{MovieAudioInfo, MovieBackend, MovieFrame, MovieGeometry, MovieHost};

use crate::engine::EngineOptions;

/// 按 [`EngineOptions`] 装配一个影片后端。
///
/// 每次 boot/restart 都调用一次(Auto 会同时创建内置与外置各一个实例),
/// 保证新会话没有上一部影片的残留状态。
#[must_use]
pub fn build_movie_host(opts: &EngineOptions) -> Box<dyn MovieHost> {
    let builtin = || -> Box<dyn MovieHost> { Box::new(yu_movie_plmpeg::PlmMovieHost::new()) };
    match opts.movie_backend {
        MovieBackend::Builtin => builtin(),
        MovieBackend::External => match &opts.movie_external_factory {
            Some(factory) => factory.create(),
            None => {
                log::warn!(
                    target: "yu-kernel",
                    "[movie] movie_backend=external,但本宿主没有注入外置解码后端;播片时会显式报诊断"
                );
                Box::new(UnavailableMovieHost)
            }
        },
        MovieBackend::Auto => match &opts.movie_external_factory {
            Some(factory) => Box::new(MovieHostSelector::new(
                MovieBackend::Auto,
                builtin(),
                Some(factory.create()),
            )),
            None => builtin(),
        },
    }
}

/// 当前被选中的实际后端。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Active {
    Builtin,
    External,
}

/// 按模式分派到内置/外置后端的选择器(`Auto` 的回退逻辑在这里)。
pub struct MovieHostSelector {
    mode: MovieBackend,
    builtin: Box<dyn MovieHost>,
    external: Option<Box<dyn MovieHost>>,
    active: Option<Active>,
    /// `MOVIE` 在 `FILE` 之前就会下发几何;先缓存,选中后端后补发。
    pending_geometry: Option<MovieGeometry>,
    /// 同上:`VOLUME` 可能先于 `FILE`。
    pending_volume: Option<i64>,
}

impl MovieHostSelector {
    fn new(
        mode: MovieBackend,
        builtin: Box<dyn MovieHost>,
        external: Option<Box<dyn MovieHost>>,
    ) -> Self {
        Self {
            mode,
            builtin,
            external,
            active: None,
            pending_geometry: None,
            pending_volume: None,
        }
    }

    fn active_host_mut(&mut self) -> Option<&mut (dyn MovieHost + 'static)> {
        match self.active {
            Some(Active::Builtin) => Some(self.builtin.as_mut()),
            Some(Active::External) => self.external.as_mut().map(|host| host.as_mut()),
            None => None,
        }
    }

    /// 选中后端后补发 open 之前缓存的命令。
    fn apply_pending(&mut self) {
        // 先把缓存值复制出来,再借用后端(否则 `self` 会同时被可变借用)。
        let geometry = self.pending_geometry;
        let volume = self.pending_volume;
        let Some(host) = self.active_host_mut() else {
            return;
        };
        if let Some(geometry) = geometry {
            host.set_geometry(geometry);
        }
        if let Some(volume) = volume {
            host.set_volume(volume);
        }
    }

    /// 关闭两个后端并清掉“当前选中”状态,但保留 pending 几何/音量。
    ///
    /// 换片时只清 `active` 不够:旧后端的 codec/extractor/临时文件以及
    /// 整份影片字节会一直活到 `MOVIEEND`,Auto 在 MPEG-1 ↔ MP4 间切换
    /// 时尤其明显。pending 值在 `open` 成功后仍要补发,所以不能走
    /// `close()`(它会按官方 `MOVIEEND` 语义清音量缓存)。
    fn close_hosts(&mut self) {
        self.builtin.close();
        if let Some(host) = self.external.as_mut() {
            host.close();
        }
        self.active = None;
    }

    fn open_builtin(&mut self, file: &[u8], bytes: &[u8]) -> Result<(), Diag> {
        self.builtin.open(file, bytes)?;
        self.active = Some(Active::Builtin);
        self.apply_pending();
        Ok(())
    }

    fn open_external(&mut self, file: &[u8], bytes: &[u8]) -> Result<(), Diag> {
        let Some(host) = self.external.as_mut() else {
            return Err(
                Diag::recoverable("movie.decode", Boundary::Platform).with_detail(
                    "movie_backend=external,但本宿主没有注入外置视频解码后端".to_string(),
                ),
            );
        };
        host.open(file, bytes)?;
        self.active = Some(Active::External);
        self.apply_pending();
        Ok(())
    }
}

impl MovieHost for MovieHostSelector {
    fn open(&mut self, file: &[u8], bytes: &[u8]) -> Result<(), Diag> {
        // 换片/重开:上一部影片的状态不能带进新会话。官方 `FILE` 成功时
        // 清播放位;`PLAY` 在命令内排在 `FILE` 之后,因此不需要缓存/补发
        // (几何与音量按官方语义保留,先缓存后补发)。旧后端的 codec/
        // extractor/临时文件必须在这里真正关闭,不能只丢 active。
        self.close_hosts();
        // Auto 的 MPEG-1 program stream 直接走内置:Android 官方保证格式
        // 不含 MPEG-PS,外置 extractor 基本必然打不开;先落临时文件再失败
        // 会让每次 OP 播放都白付一次 I/O。官方 `CODEC=1` 的判定与这里用的
        // `yu_format::movie::probe_codec` 是同一份 5 字节规则。
        if self.mode == MovieBackend::Auto
            && self.external.is_some()
            && yu_format::movie::probe_codec(
                &bytes[..bytes.len().min(yu_format::movie::PROBE_BYTES)],
            ) == yu_format::movie::MovieCodec::Mpeg1
        {
            match self.open_builtin(file, bytes) {
                Ok(()) => return Ok(()),
                Err(error) => {
                    // 头是 MPEG-1 PS 但内置解不动(损坏/非标准):仍给外置
                    // 一次机会(将来桌面 FFmpeg 后端可能能救);外置也失败
                    // 时保留内置错误为主因。
                    log::info!(
                        target: "yu-kernel",
                        "[movie] 内置 pl_mpeg 打不开 MPEG-1 PS {}，尝试外置: {}",
                        String::from_utf8_lossy(file),
                        error
                    );
                    if self.open_external(file, bytes).is_ok() {
                        return Ok(());
                    }
                    return Err(error);
                }
            }
        }
        match self.mode {
            MovieBackend::Builtin => self.open_builtin(file, bytes),
            MovieBackend::External => self.open_external(file, bytes),
            MovieBackend::Auto => {
                if self.external.is_some() {
                    match self.open_external(file, bytes) {
                        Ok(()) => return Ok(()),
                        Err(error) => {
                            // 外置打不开不是致命:官方 MPEG-1 语料在 Android
                            // 上正是这条路径。回退内置并留痕,便于排查“为什么
                            // 没走硬解”。失败后主动 close 一次,清掉部分
                            // 初始化的 codec/extractor/临时文件。
                            if let Some(host) = self.external.as_mut() {
                                host.close();
                            }
                            log::info!(
                                target: "yu-kernel",
                                "[movie] 外置后端打不开 {}，回退内置 pl_mpeg: {}",
                                String::from_utf8_lossy(file),
                                error
                            );
                        }
                    }
                }
                match self.open_builtin(file, bytes) {
                    Ok(()) => Ok(()),
                    Err(mut builtin_error) => {
                        // 外置失败原因已在上面的 info 日志里;这里只补一句
                        // 合并语义,不重复调用外置 open(避免二次副作用)。
                        if builtin_error.detail.is_empty() {
                            builtin_error.detail =
                                "内置 pl_mpeg 与可选外置后端都无法解析该影片".to_string();
                        }
                        Err(builtin_error)
                    }
                }
            }
        }
    }

    fn set_playing(&mut self, playing: bool) -> Result<(), Diag> {
        // 没有后端时只保留 VM 的播放意图;后端一旦 `open`,`FILE` 会清播放位,
        // 之后的 `PLAY` 再走这里转发。
        match self.active_host_mut() {
            Some(host) => host.set_playing(playing),
            None => Ok(()),
        }
    }

    fn set_volume(&mut self, volume: i64) {
        self.pending_volume = Some(volume);
        if let Some(host) = self.active_host_mut() {
            host.set_volume(volume);
        }
    }

    fn set_geometry(&mut self, geometry: MovieGeometry) {
        self.pending_geometry = Some(geometry);
        if let Some(host) = self.active_host_mut() {
            host.set_geometry(geometry);
        }
    }

    fn is_playing(&mut self) -> bool {
        self.active_host_mut().is_some_and(|host| host.is_playing())
    }

    fn media_size(&mut self) -> Option<(u32, u32)> {
        self.active_host_mut().and_then(|host| host.media_size())
    }

    fn frame(&mut self, now_ms: u64) -> Option<MovieFrame<'_>> {
        self.active_host_mut()?.frame(now_ms)
    }

    fn take_audio(&mut self, out: &mut Vec<i16>) -> Option<MovieAudioInfo> {
        self.active_host_mut()?.take_audio(out)
    }

    fn backend_label(&self) -> &'static str {
        match self.active {
            Some(Active::Builtin) => self.builtin.backend_label(),
            Some(Active::External) => self
                .external
                .as_deref()
                .map_or("external", |host| host.backend_label()),
            None => "none",
        }
    }

    fn close(&mut self) {
        self.close_hosts();
        // `MOVIEEND` 把官方音量重置 256;丢掉缓存,下一部影片没给 VOLUME
        // 时让后端用自己的默认值(而不是上一部的残留)。
        self.pending_volume = None;
    }
}

/// 显式选择 `External` 但宿主没有注入后端时的占位实现。
///
/// 不假装播放;`open` 返回可恢复诊断,脚本层与日志都能看到原因。
struct UnavailableMovieHost;

impl MovieHost for UnavailableMovieHost {
    fn open(&mut self, _file: &[u8], _bytes: &[u8]) -> Result<(), Diag> {
        Err(Diag::recoverable("movie.decode", Boundary::Platform).with_detail(
            "movie_backend=external,但本宿主没有外置视频解码后端;请改选 auto/builtin,或在宿主注入外置后端"
                .to_string(),
        ))
    }

    fn set_playing(&mut self, _playing: bool) -> Result<(), Diag> {
        Ok(())
    }

    fn set_volume(&mut self, _volume: i64) {}

    fn set_geometry(&mut self, _geometry: MovieGeometry) {}

    /// 没有解码器就不可能是“正在播放”;否则 `es_MoviePlay` 的
    /// `MOVIEINFO[PLAY]` 轮询会永远等下去。
    fn is_playing(&mut self) -> bool {
        false
    }

    fn backend_label(&self) -> &'static str {
        "external(unavailable)"
    }

    fn close(&mut self) {}
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    #[derive(Default)]
    struct FakeHost {
        label: &'static str,
        open_ok: bool,
        playing: bool,
        /// 跨 Box<dyn MovieHost> 共享的调用记录(测试断言用)。
        events: Rc<RefCell<Vec<String>>>,
    }

    impl FakeHost {
        fn new(label: &'static str, open_ok: bool) -> Self {
            Self {
                label,
                open_ok,
                ..Self::default()
            }
        }

        fn events(&self) -> Rc<RefCell<Vec<String>>> {
            self.events.clone()
        }

        fn record(&self, event: impl Into<String>) {
            self.events.borrow_mut().push(event.into());
        }
    }

    impl MovieHost for FakeHost {
        fn open(&mut self, _file: &[u8], _bytes: &[u8]) -> Result<(), Diag> {
            self.record("open");
            if self.open_ok {
                Ok(())
            } else {
                Err(Diag::recoverable("movie.decode", Boundary::Data)
                    .with_detail(format!("{} open failed", self.label)))
            }
        }

        fn set_playing(&mut self, playing: bool) -> Result<(), Diag> {
            self.playing = playing;
            self.record(format!("play:{playing}"));
            Ok(())
        }

        fn set_volume(&mut self, volume: i64) {
            self.record(format!("volume:{volume}"));
        }

        fn set_geometry(&mut self, geometry: MovieGeometry) {
            self.record(format!("geometry:{}/{}", geometry.x, geometry.y));
        }

        fn is_playing(&mut self) -> bool {
            self.playing
        }

        fn backend_label(&self) -> &'static str {
            self.label
        }

        fn close(&mut self) {
            self.record("close");
        }
    }

    fn selector_with(
        mode: MovieBackend,
        builtin: FakeHost,
        external: Option<FakeHost>,
    ) -> MovieHostSelector {
        MovieHostSelector::new(
            mode,
            Box::new(builtin),
            external.map(|host| Box::new(host) as Box<dyn MovieHost>),
        )
    }

    #[test]
    fn auto_falls_back_to_builtin_when_external_fails() {
        let external = FakeHost::new("external", false);
        let events = external.events();
        let mut host = selector_with(
            MovieBackend::Auto,
            FakeHost::new("builtin", true),
            Some(external),
        );
        host.open(b"a.mp4", b"data").expect("builtin 应接住");
        assert_eq!(host.backend_label(), "builtin");
        assert!(
            events.borrow().iter().any(|event| event == "close"),
            "外置失败后必须 close 掉部分初始化的资源"
        );
    }

    #[test]
    fn auto_mpeg1_program_stream_skips_external() {
        // 官方样例头:00 00 01 BA 21。Android 不保证 MPEG-PS,自动模式
        // 应直接走内置,避免先落临时文件再失败。
        let external = FakeHost::new("external", true);
        let events = external.events();
        let mut host = selector_with(
            MovieBackend::Auto,
            FakeHost::new("builtin", true),
            Some(external),
        );
        let bytes = [0x00, 0x00, 0x01, 0xBA, 0x21, 0x00];
        host.open(b"a.mpg", &bytes).expect("builtin");
        assert_eq!(host.backend_label(), "builtin");
        assert_eq!(
            events.borrow().as_slice(),
            ["close"],
            "MPEG-1 PS 只应做换片前清理,不应尝试外置 open"
        );
    }

    #[test]
    fn auto_prefers_external_when_it_opens() {
        let mut host = selector_with(
            MovieBackend::Auto,
            FakeHost::new("builtin", true),
            Some(FakeHost::new("external", true)),
        );
        host.open(b"a.mp4", b"data").expect("external");
        assert_eq!(host.backend_label(), "external");
    }

    #[test]
    fn reopen_closes_previous_backend() {
        let external = FakeHost::new("external", true);
        let events = external.events();
        let mut host = selector_with(
            MovieBackend::Auto,
            FakeHost::new("builtin", true),
            Some(external),
        );
        host.open(b"a.mp4", b"data").expect("external");
        assert_eq!(host.backend_label(), "external");
        host.open(b"b.mp4", b"data").expect("external");
        assert_eq!(
            events.borrow().as_slice(),
            ["close", "open", "close", "open"],
            "首次 open 前的清理 close 无害;真正的换片必须先 close 再 open"
        );
    }

    #[test]
    fn explicit_external_does_not_fall_back() {
        let mut host = selector_with(
            MovieBackend::External,
            FakeHost::new("builtin", true),
            Some(FakeHost::new("external", false)),
        );
        let error = host.open(b"a.mp4", b"data").expect_err("必须显式失败");
        assert_eq!(error.code, "movie.decode");
        assert_eq!(host.backend_label(), "none");
    }

    #[test]
    fn external_without_factory_reports_capability_gap() {
        let mut host = selector_with(MovieBackend::External, FakeHost::new("builtin", true), None);
        let error = host
            .open(b"a.mp4", b"data")
            .expect_err("没有外置后端必须失败");
        assert_eq!(error.code, "movie.decode");
        assert!(error.detail.contains("外置视频解码后端"));
    }

    #[test]
    fn pending_geometry_and_volume_are_replayed_after_open() {
        let external = FakeHost::new("external", true);
        let events = external.events();
        let mut host = selector_with(
            MovieBackend::Auto,
            FakeHost::new("builtin", true),
            Some(external),
        );
        let geometry = MovieGeometry {
            x: 3.0,
            y: 4.0,
            sx: 0.0,
            sy: 0.0,
        };
        // 官方顺序:MOVIE 先写几何,再 FILE(open)、VOLUME、PLAY。这里把
        // 几何与音量放在 open 之前,验证选择器会在选中后端后补发;PLAY
        // 按官方顺序在 open 之后,直接转发。
        host.set_geometry(geometry);
        host.set_volume(128);
        host.open(b"a.mp4", b"data").expect("external");

        let log = events.borrow();
        assert!(log.iter().any(|event| event == "open"));
        assert!(log.iter().any(|event| event == "volume:128"));
        assert!(log.iter().any(|event| event == "geometry:3/4"));
        drop(log);

        host.set_playing(true).expect("play");
        assert!(events.borrow().iter().any(|event| event == "play:true"));
        host.set_volume(64);
        host.set_playing(false).expect("pause");
        assert_eq!(host.backend_label(), "external");
        assert!(!host.is_playing());
    }

    #[test]
    fn builtin_mode_ignores_external() {
        let mut host = selector_with(
            MovieBackend::Builtin,
            FakeHost::new("builtin", true),
            Some(FakeHost::new("external", true)),
        );
        host.open(b"a.mpg", b"data").expect("builtin");
        assert_eq!(host.backend_label(), "builtin");
    }

    #[test]
    fn unavailable_host_has_clear_diagnostic() {
        let mut host = UnavailableMovieHost;
        let error = host.open(b"a.mp4", b"data").expect_err("必须失败");
        assert_eq!(error.code, "movie.decode");
        assert_eq!(error.boundary, Boundary::Platform);
        assert!(error.detail.contains("auto/builtin"));
        assert!(!host.is_playing(), "没有解码器不能报“正在播放”");
    }
}
