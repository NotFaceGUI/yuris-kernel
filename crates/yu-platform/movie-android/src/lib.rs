//! Android **外置**影片后端:把解码交给设备系统媒体栈。
//!
//! # 定位
//!
//! 内置后端(`yu-movie-plmpeg`)只解 MPEG-1 program stream;商业游戏里出现
//! MP4/AVI/MKV 时,官方引擎本来就是交给系统媒体栈(Windows MCI/COM)。
//! 本 crate 在 Android 上做同构的事:
//!
//! ```text
//! MOVIE[FILE] bytes
//!   → 临时文件(fd)
//!   → AMediaExtractor 拆包 / 选轨
//!   → AMediaCodec 解码
//!       视频: ByteBuffer(线性 YUV) → stride/slice/crop → RGBA
//!       音频: ByteBuffer → i16 → 立体声降混
//!   → yu_rt::event::MovieHost 端口(内核只做语义)
//! ```
//!
//! # 为什么最终是 ByteBuffer
//!
//! 最初按 Android 文档实现的是 `AImageReader`(Surface)取原始帧,在高通
//! 真机上失败:`AImage_getFormat` 报 `0x23/YUV_420_888`、planes=3,但
//! `AImage_getPlaneData(plane1)` 返回的 data 指针为 null(底层实际是
//! UBWC `0x7fa30c06`),U/V 平面无法 CPU 读取,画面全黑;改用
//! `AImageReader_newWithUsage(CPU_READ_OFTEN)` 仍被厂商格式覆写。
//! ByteBuffer 输出是线性 YUV(实测 `color-format=0x15`,即 NV12),
//! 由输出格式的 `stride`/`slice-height`/crop 转换,跨设备稳定;代价是
//! 一次 CPU 拷贝,影片帧率下可接受。`flexible`/未知厂商格式先按 NV12
//! 尝试,长度不够再按 I420。
//!
//! # 设备能力
//!
//! - 逐文件:extractor 能打开容器、`AMediaCodec_createDecoderByType` 能
//!   建解码器,才认为外置可用;否则 `open` 返回诊断,Auto 模式回退内置。
//! - 设备清单:[`probe_video_codecs`] 对固定 MIME 表实测(API 21+ 可用)。
//!   Android 的 `AMediaCodecInfo` 细粒度查询要到 API 36,所以硬件/软件
//!   标记由 Java 壳的 `MediaCodecList` 补足(见 `MovieCodecProbe.java`)。
//!
//! # 限制
//!
//! - Android 官方保证的容器不含 MPEG-PS;官方 MPEG-1 语料应走内置回退。
//! - AVI 是否可解取决于设备 extractor,不做保证。
//! - 影片整段字节会落一个临时文件(extractor 只接 fd/URI);文件在
//!   `close`/Drop 时删除。
//! - 运行时解码错误无法从 `MovieHost::frame` 返回 `Diag`(trait 签名无
//!   错误通道),按「结束播放 + 日志」处理;`open` 阶段失败才触发回退。
//!
//! 非 Android 构建只保留纯转换函数与空的能力探测,供桌面单测。

#![cfg_attr(target_os = "android", allow(unsafe_code))]
#![allow(clippy::result_large_err)]

pub mod convert;

/// 一种视频编码在本设备上是否有解码器(外置后端逐项探测结果)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MovieCodecSupport {
    /// Android MIME(如 `video/avc`)。
    pub mime: &'static str,
    /// 设备能否创建该 MIME 的解码器。
    pub decoder: bool,
}

/// 本设备外置后端可尝试的视频编码清单。
///
/// 顺序固定,便于日志/壳展示逐字节稳定。探测只 `createDecoderByType`
/// 后立即释放,不 configure/start,不占用长时间资源。
#[cfg(target_os = "android")]
#[must_use]
pub fn probe_video_codecs() -> Vec<MovieCodecSupport> {
    android::probe_video_codecs()
}

/// 非 Android 宿主没有系统 MediaCodec;返回空清单(外置工厂也不会注入)。
#[cfg(not(target_os = "android"))]
#[must_use]
pub fn probe_video_codecs() -> Vec<MovieCodecSupport> {
    Vec::new()
}

#[cfg(target_os = "android")]
mod android;

#[cfg(target_os = "android")]
pub use android::AndroidMovieHost;
