//! `MOVIE` 族的内容探测(`MOVIEINFO[CODEC]`)。
//!
//! 官方处理体 `0x44ba2c`(`MOVIEINFO`)的 `CODEC` 分支(`0x44bac9..0x44bcfc`)
//! 只读文件**头 5 字节**:
//!
//! ```text
//! 打不开文件                     → 0
//! 前 4 字节 != 00 00 01 BA       → 0xFF
//! 第 5 字节 & 0xF0 == 0x20       → 1     (MPEG-1 program stream)
//! 第 5 字节 & 0xC0 == 0x40       → 2     (MPEG-2 program stream)
//! 其余                           → 0xFF
//! ```
//!
//! 这与引擎 `fopen`(`0x469df2`)+`fread`(`0x469ebe`)的调用序列一一对应,
//! 也是 ERIS 脚本判分支的依据(`es_MoviePlay.yst`:`==0` = 文件不存在,
//! `>1` = 不是 MPEG-1)。
//!
//! 本模块**只做协议探测**,不解码、不依赖任何平台 API —— 解码归宿主的
//! 平台媒体端口(`yu_rt::event::MovieHost`)。

use crate::code;

/// 影片内容探测结果(对应 `MOVIEINFO[CODEC]` 的官方返回值)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MovieCodec {
    /// 文件不存在/打不开(`0`)。
    Missing,
    /// MPEG-1 program stream(`1`)。
    Mpeg1,
    /// MPEG-2 program stream(`2`)。
    Mpeg2,
    /// 其它容器/编码(官方返回 `0xFF`;AVI 等同样落这里)。
    Other,
}

impl MovieCodec {
    /// 官方 `MOVIEINFO[CODEC]` 的整数值。
    #[must_use]
    pub fn official_code(self) -> i64 {
        match self {
            Self::Missing => 0,
            Self::Mpeg1 => 1,
            Self::Mpeg2 => 2,
            Self::Other => 0xFF,
        }
    }

    /// 反查(测试/诊断用)。
    #[must_use]
    pub fn from_official(code: i64) -> Self {
        match code {
            0 => Self::Missing,
            1 => Self::Mpeg1,
            2 => Self::Mpeg2,
            _ => Self::Other,
        }
    }
}

/// MPEG program-stream pack start code(`00 00 01 BA`,小端 dword = `0xBA010000`)。
pub const PACK_START_CODE: [u8; 4] = [0x00, 0x00, 0x01, 0xBA];

/// 探测头部字节(至少 4 字节;不足按 `Other` 处理)。
///
/// 注意官方先判 `& 0xF0 == 0x20`(MPEG-1)再判 `& 0xC0 == 0x40`(MPEG-2),
/// 两者不冲突(MPEG-2 的 `0x4X & 0xF0 == 0x40`)。
#[must_use]
pub fn probe_codec(head: &[u8]) -> MovieCodec {
    if head.len() < 4 || head[..4] != PACK_START_CODE {
        return MovieCodec::Other;
    }
    let Some(&fifth) = head.get(4) else {
        // 只有 pack start code、没有 pack header:官方同样落 `0xFF`。
        return MovieCodec::Other;
    };
    if fifth & 0xF0 == 0x20 {
        MovieCodec::Mpeg1
    } else if fifth & 0xC0 == 0x40 {
        MovieCodec::Mpeg2
    } else {
        MovieCodec::Other
    }
}

/// 探测所需的最少字节数(官方读 4 + 1)。
pub const PROBE_BYTES: usize = 5;

/// 探测不可达时的诊断码(与 `fmt.*` 同域;由调用方决定是否上报)。
#[allow(dead_code)]
pub(crate) const PROBE_ERROR: &str = code::TRUNCATED;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mpeg1_pack_header() {
        // 官方样例 `data/pac/op_movie.mpg` 的真实头:`00 00 01 BA 21`。
        assert_eq!(
            probe_codec(&[0x00, 0x00, 0x01, 0xBA, 0x21]),
            MovieCodec::Mpeg1
        );
        // 0x2F 也在 MPEG-1 带内。
        assert_eq!(
            probe_codec(&[0x00, 0x00, 0x01, 0xBA, 0x2F]),
            MovieCodec::Mpeg1
        );
    }

    #[test]
    fn mpeg2_pack_header() {
        assert_eq!(
            probe_codec(&[0x00, 0x00, 0x01, 0xBA, 0x44]),
            MovieCodec::Mpeg2
        );
    }

    #[test]
    fn other_and_truncated() {
        // RIFF/AVI 头。
        assert_eq!(probe_codec(b"RIFF\x00\x00\x00\x00AVI "), MovieCodec::Other);
        // 只有 4 字节(pack start code 但缺第 5 字节)。
        assert_eq!(
            probe_codec(&[0x00, 0x00, 0x01, 0xBA]),
            MovieCodec::Other
        );
        assert_eq!(probe_codec(b""), MovieCodec::Other);
    }

    #[test]
    fn official_codes_round_trip() {
        for code in [0i64, 1, 2, 0xFF] {
            assert_eq!(MovieCodec::from_official(code).official_code(), code);
        }
        assert_eq!(MovieCodec::Other.official_code(), 255);
    }

    /// 真实样例(需要 `YURIS_SAMPLE_SDK0495`)。
    #[test]
    fn sdk_sample_is_mpeg1() {
        let Ok(root) = std::env::var("YURIS_SAMPLE_SDK0495") else {
            if std::env::var("YURIS_REQUIRE_SAMPLES").ok().as_deref() == Some("1") {
                panic!("YURIS_REQUIRE_SAMPLES=1 但 YURIS_SAMPLE_SDK0495 未设置");
            }
            return;
        };
        let path = format!("{root}/data/pac/op_movie.mpg");
        let Ok(bytes) = std::fs::read(&path) else {
            if std::env::var("YURIS_REQUIRE_SAMPLES").ok().as_deref() == Some("1") {
                panic!("YURIS_REQUIRE_SAMPLES=1 但 {path} 缺失");
            }
            return;
        };
        let head = &bytes[..bytes.len().min(PROBE_BYTES)];
        assert_eq!(probe_codec(head), MovieCodec::Mpeg1, "{path}");
    }
}
