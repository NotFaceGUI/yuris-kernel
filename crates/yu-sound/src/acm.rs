//! win32 ACM WAVE 转换 —— 官方 WAVE 装载 `0x422e04` 非 PCM 分支的 1:1 实现。
//!
//! 证据（2026-10-24 REA 复核，`docs/sound-ledger.md`）：
//! - 官方 `MSACM32.dll` 全部 7 个导入（`acmFormatSuggest/StreamOpen/Size/
//!   PrepareHeader/Convert/UnprepareHeader/StreamClose`）只被 `0x422e04`
//!   的 else 分支调用（调用点 `0x423086..0x4232a4`，IAT 跳板
//!   `0x47f86e..0x47f892`）；手册「特殊コーデック WAVE には対応していません」
//!   系过时描述 ⇒ 终态 fixed(no-manual)（DIV-SND-004）。
//! - 官方序列：`acmFormatSuggest(NULL, src, dst=PCM, cbwfx=0x12,
//!   ACM_FORMATSUGGESTF_WFORMATTAG)` → `acmStreamOpen(…, fdwOpen=
//!   ACM_STREAMOPENF_NONREALTIME)` → `acmStreamSize(data, &out, 0)` →
//!   `PrepareHeader` → 一次 `acmStreamConvert(…, 0)` → `UnprepareHeader`
//!   → `StreamClose`；任一步非 0 = 装载失败（返回负错误码，层保持旧
//!   元数据，N0-8）。`+0x107`（对外 CODEC）写 2。
//! - 源格式修正（官方 `pch[8]==0 → 0x12`）：fmt chunk 的 `cbSize` 为 0
//!   时改写为 18（含 chunk 不足 18 字节的零填充情形），逐字镜像。
//!
//! 本模块是全仓第一个第一方 unsafe 点：crate 级由 `forbid` 放宽为
//! `deny(unsafe_code)` 并仅在此模块 `allow` —— 理由是 1:1 对齐要求调用
//! 与官方相同的宿主 ACM 编解码器（纯 Rust 重写任何 ADPCM 解码都不是
//! 官方行为）。非 Windows 构建无 MSACM32，官方同为 win32 程序，
//! 按装载失败对齐。
#![allow(unsafe_code)]

use yu_log::Diag;

use crate::decode::{Codec, Pcm};
use crate::error::{self, code};

#[cfg(windows)]
pub use imp::{convert, decode_acm_wave};

/// WAVEFORMATEX（18 字节 POD；仅用于与 ACM API 交换格式）。
#[cfg(windows)]
#[repr(C)]
#[derive(Clone, Copy)]
pub struct WinFormat {
    pub w_format_tag: u16,
    pub n_channels: u16,
    pub n_samples_per_sec: u32,
    pub n_avg_bytes_per_sec: u32,
    pub n_block_align: u16,
    pub w_bits_per_sample: u16,
    pub cb_size: u16,
}

#[cfg(windows)]
impl WinFormat {
    pub const PCM_ZEROED: Self = Self {
        w_format_tag: 1,
        n_channels: 0,
        n_samples_per_sec: 0,
        n_avg_bytes_per_sec: 0,
        n_block_align: 0,
        w_bits_per_sample: 0,
        cb_size: 0,
    };

    pub fn to_bytes(self) -> Vec<u8> {
        let mut out = Vec::with_capacity(18);
        out.extend_from_slice(&self.w_format_tag.to_le_bytes());
        out.extend_from_slice(&self.n_channels.to_le_bytes());
        out.extend_from_slice(&self.n_samples_per_sec.to_le_bytes());
        out.extend_from_slice(&self.n_avg_bytes_per_sec.to_le_bytes());
        out.extend_from_slice(&self.n_block_align.to_le_bytes());
        out.extend_from_slice(&self.w_bits_per_sample.to_le_bytes());
        out.extend_from_slice(&self.cb_size.to_le_bytes());
        out
    }
}

/// 非 Windows 构建：官方是 win32 程序，ACM 形态不存在，按装载失败对齐。
#[cfg(not(windows))]
pub fn decode_acm_wave(name: &[u8], _fmt: &[u8], _data: &[u8]) -> Result<Pcm, Diag> {
    Err(error::recoverable(
        code::DECODE_FAILED,
        format!(
            "非 PCM WAVE({}) 需要 win32 ACM(官方 0x422e04 非 PCM 分支);本平台无 MSACM32,按装载失败对齐",
            String::from_utf8_lossy(name)
        ),
    ))
}

/// 非 Windows 的占位：真实转换只有 win32 宿主可断言。
#[cfg(not(windows))]
pub fn convert(_src: &[u8], _dst: &[u8], _data: &[u8]) -> Result<Vec<u8>, Diag> {
    Err(error::recoverable(
        code::DECODE_FAILED,
        "win32 ACM 仅 Windows 宿主可用".to_string(),
    ))
}

fn attach(name: &[u8], mut d: Diag) -> Diag {
    d.detail = format!("{}: {}", display(name), d.detail);
    d
}

fn display(bytes: &[u8]) -> String {
    let mut s = String::new();
    for &b in bytes {
        if (0x20..0x7f).contains(&b) {
            s.push(b as char);
        } else {
            s.push_str(&format!("\\x{b:02x}"));
        }
    }
    s
}

/// 组装解码结果。位深/声道/采样率取 ACM 建议格式；PCM 样本统一压到 i16。
fn build_pcm(name: &[u8], fmt: &WinFormat, out: &[u8]) -> Result<Pcm, Diag> {
    let channels = fmt.n_channels;
    let rate = fmt.n_samples_per_sec;
    let bits = fmt.w_bits_per_sample;
    if channels == 0 || rate == 0 {
        return Err(error::fatal(
            code::DECODE_FAILED,
            format!("ACM 建议格式非法({}): channels={channels} rate={rate}", display(name)),
        ));
    }
    let bytes_per_sample = (bits / 8) as usize;
    if !matches!(bits, 8 | 16 | 24 | 32) || bytes_per_sample == 0 {
        return Err(error::recoverable(
            code::DECODE_FAILED,
            format!("ACM 建议位深非 8/16/24/32({}): bits={bits}", display(name)),
        ));
    }
    let block = channels as usize * bytes_per_sample;
    let frames = out.len() / block;
    let mut samples = Vec::with_capacity(frames * channels as usize);
    for frame in 0..frames {
        for ch in 0..channels as usize {
            let at = frame * block + ch * bytes_per_sample;
            let v = match bits {
                8 => {
                    // u8(0..255) → i16 满幅：(b-128)*257。
                    (i16::from(out[at]) - 128).wrapping_mul(257)
                }
                16 => i16::from_le_bytes([out[at], out[at + 1]]),
                24 => {
                    let v = i32::from_le_bytes([out[at], out[at + 1], out[at + 2], 0]);
                    (v >> 8) as i16
                }
                _ => {
                    let v = i32::from_le_bytes([out[at], out[at + 1], out[at + 2], out[at + 3]]);
                    (v >> 16) as i16
                }
            };
            samples.push(v);
        }
    }
    Ok(Pcm {
        samples,
        channels,
        bits,
        sample_rate: rate,
        codec: Codec::Acm,
    })
}

#[cfg(windows)]
mod imp {
    //! FFI 面（仅 Windows）。结构体布局按宿主架构（x64 下指针 8 字节，
    //! `cbStruct` 取 `size_of::<AcmStreamHeader>()`；官方 32 位布局的
    //! `cbStruct=0x54` 与此同构不同宽——同一段源码在各自架构编译期取
    //! 本架构大小）。

    use std::os::raw::c_void;

    use yu_log::Diag;

    use super::{attach, build_pcm, display, Pcm, WinFormat};
    use crate::error::{self, code};

    /// `ACM_FORMATSUGGESTF_WFORMATTAG`（msacm.h；官方立即数 `0x10000`）。
    const ACM_FORMATSUGGESTF_WFORMATTAG: u32 = 0x0001_0000;
    /// `ACM_STREAMOPENF_NONREALTIME`（官方 fdwOpen 立即数 4）。
    const ACM_STREAMOPENF_NONREALTIME: u32 = 0x0000_0004;
    /// 官方给输出缓冲加的富余字节数（`GlobalAlloc(size+0x400)`）。
    const OFFICIAL_BUFFER_SLACK: usize = 0x400;

    type Mmresult = u32;
    type Hacmstream = *mut c_void;

    /// 官方 WAVE 非 PCM 分支入口：`fmt ` chunk 原始字节 + `data` chunk 字节
    /// → 转换后的 PCM。任一 ACM 步骤失败都按官方「装载失败」对齐
    /// （`recoverable`，层保持旧元数据）。
    pub fn decode_acm_wave(name: &[u8], fmt_chunk: &[u8], data: &[u8]) -> Result<Pcm, Diag> {
        let src = src_format(fmt_chunk);
        let dst = suggest_pcm_format(&src, name)?;
        let out = convert(&src, &dst.to_bytes(), data).map_err(|d| attach(name, d))?;
        build_pcm(name, &dst, &out)
    }

    #[repr(C)]
    struct AcmStreamHeader {
        cb_struct: u32,
        fdw_status: u32,
        dw_user: usize,
        pb_src: *mut u8,
        cb_src_length: u32,
        cb_src_length_used: u32,
        dw_src_user: usize,
        pb_dst: *mut u8,
        cb_dst_length: u32,
        cb_dst_length_used: u32,
        dw_dst_user: usize,
        dw_reserved_driver: [usize; 10],
    }

    #[link(name = "msacm32")]
    extern "system" {
        fn acmFormatSuggest(
            had: Hacmstream,
            pwfx_src: *const c_void,
            pwfx_dst: *mut c_void,
            cbwfx: u32,
            fdw_suggest: u32,
        ) -> Mmresult;
        fn acmStreamOpen(
            phas: *mut Hacmstream,
            had: Hacmstream,
            pwfx_src: *const c_void,
            pwfx_dst: *const c_void,
            pwfltr: *const c_void,
            dw_callback: usize,
            dw_instance: usize,
            fdw_open: u32,
        ) -> Mmresult;
        fn acmStreamSize(
            has: Hacmstream,
            cb_input: u32,
            pdw_output_byte_size: *mut u32,
            fdw_size: u32,
        ) -> Mmresult;
        fn acmStreamPrepareHeader(
            has: Hacmstream,
            pash: *mut AcmStreamHeader,
            fdw_prepare: u32,
        ) -> Mmresult;
        fn acmStreamConvert(
            has: Hacmstream,
            pash: *mut AcmStreamHeader,
            fdw_convert: u32,
        ) -> Mmresult;
        fn acmStreamUnprepareHeader(
            has: Hacmstream,
            pash: *mut AcmStreamHeader,
            fdw_unprepare: u32,
        ) -> Mmresult;
        fn acmStreamClose(has: Hacmstream, fdw_close: u32) -> Mmresult;
    }

    /// 官方的源格式准备（`0x422e04`）：缓冲 `max(cksize, 0x12)` 字节、
    /// 不足部分为零；`cbSize == 0` 改写为 `0x12`（逐字镜像）。
    pub fn src_format(fmt_chunk: &[u8]) -> Vec<u8> {
        let mut buf = vec![0u8; fmt_chunk.len().max(18)];
        buf[..fmt_chunk.len()].copy_from_slice(fmt_chunk);
        if buf.len() >= 18 {
            let cb_size = u16::from_le_bytes([buf[16], buf[17]]);
            if cb_size == 0 {
                buf[16..18].copy_from_slice(&18u16.to_le_bytes());
            }
        }
        buf
    }

    /// `acmFormatSuggest`，目标 = PCM、只钉 WFORMATTAG（官方 fdw 立即数）。
    pub fn suggest_pcm_format(src: &[u8], name: &[u8]) -> Result<WinFormat, Diag> {
        let mut dst = WinFormat::PCM_ZEROED;
        let rc = unsafe {
            acmFormatSuggest(
                std::ptr::null_mut(),
                src.as_ptr().cast(),
                std::ptr::addr_of_mut!(dst).cast(),
                18,
                ACM_FORMATSUGGESTF_WFORMATTAG,
            )
        };
        if rc != 0 {
            // 官方 -0x27d8：FormatSuggest 失败 = 装载失败。
            return Err(error::recoverable(
                code::DECODE_FAILED,
                format!("acmFormatSuggest 失败 rc={rc}(官方 -0x27d8 同型): {}", display(name)),
            ));
        }
        Ok(dst)
    }

    /// 官方转换链原语：源/目标格式（WAVEFORMATEX 字节串，可含扩展）+
    /// 数据 → 目标格式字节串。`decode_acm_wave` 与 ACM 往返测试共用。
    pub fn convert(src: &[u8], dst: &[u8], data: &[u8]) -> Result<Vec<u8>, Diag> {
        let mut has: Hacmstream = std::ptr::null_mut();
        let rc = unsafe {
            acmStreamOpen(
                &mut has,
                std::ptr::null_mut(),
                src.as_ptr().cast(),
                dst.as_ptr().cast(),
                std::ptr::null(),
                0,
                0,
                ACM_STREAMOPENF_NONREALTIME,
            )
        };
        if rc != 0 {
            return Err(error::recoverable(
                code::DECODE_FAILED,
                format!("acmStreamOpen 失败 rc={rc}"),
            ));
        }
        let result = stream_convert(has, data);
        // Close 是官方收尾步骤；失败只影响资源回收，不改变结果语义。
        unsafe {
            let _ = acmStreamClose(has, 0);
        }
        result
    }

    fn stream_convert(has: Hacmstream, data: &[u8]) -> Result<Vec<u8>, Diag> {
        let mut out_size: u32 = 0;
        let rc = unsafe { acmStreamSize(has, data.len() as u32, &mut out_size, 0) };
        if rc != 0 {
            return Err(error::recoverable(
                code::DECODE_FAILED,
                format!("acmStreamSize 失败 rc={rc}"),
            ));
        }
        let mut out = vec![0u8; out_size as usize + OFFICIAL_BUFFER_SLACK];
        let mut header = AcmStreamHeader {
            cb_struct: std::mem::size_of::<AcmStreamHeader>() as u32,
            fdw_status: 0,
            dw_user: 0,
            pb_src: data.as_ptr() as *mut u8,
            cb_src_length: data.len() as u32,
            cb_src_length_used: 0,
            dw_src_user: 0,
            pb_dst: out.as_mut_ptr(),
            cb_dst_length: out.len() as u32,
            cb_dst_length_used: 0,
            dw_dst_user: 0,
            dw_reserved_driver: [0; 10],
        };
        let rc = unsafe { acmStreamPrepareHeader(has, &mut header, 0) };
        if rc != 0 {
            return Err(error::recoverable(
                code::DECODE_FAILED,
                format!("acmStreamPrepareHeader 失败 rc={rc}"),
            ));
        }
        let rc = unsafe { acmStreamConvert(has, &mut header, 0) };
        let converted = if rc == 0 {
            let used = header.cb_dst_length_used as usize;
            if used <= out.len() {
                out.truncate(used);
                Ok(out)
            } else {
                Err(error::recoverable(
                    code::DECODE_FAILED,
                    format!("acmStreamConvert 输出长度异常 used={used} buf={}", out.len()),
                ))
            }
        } else {
            Err(error::recoverable(
                code::DECODE_FAILED,
                format!("acmStreamConvert 失败 rc={rc}"),
            ))
        };
        // PrepareHeader 成功后必须 Unprepare（两者成对）。
        unsafe {
            let _ = acmStreamUnprepareHeader(has, &mut header, 0);
        }
        converted
    }
}
