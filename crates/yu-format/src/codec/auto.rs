//! 编码自动识别(架构 §7:确定性、可复现,平局缺省 CP932)。
//!
//! 思路参考 YU-RIS-Script-Editor `core/encoding.py`(双解码 + 构成统计),
//! 但裁决完全重造以满足 A3:
//! - 输入 = 按固定顺序给出的采样池(ybn 字符串常量 → 资源名 → 配置文本);
//! - 每个候选编解码器独立严格解码,逐字节推进,非法序列计 1 字节并前进
//!   1 字节(固定的重同步策略,保证同一输入永远同一得分);
//! - 构成统计:假名 / 半角假名 / 汉字 / 日文标点 / 控制字符;
//! - 得分为固定次序的浮点算式,无随机、无环境依赖;
//! - 得分差低于 [`TIE_THRESHOLD`] 或全部为零 → 缺省 [`Codec::Cp932`]。

use super::Codec;

/// 平局阈值:得分差低于该值时取 CP932(日文为原生形态)。
pub const TIE_THRESHOLD: f64 = 1.0;

/// 单个编解码器在采样池上的得分与构成统计。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Score {
    /// 严格解码失败的字节总数。
    pub illegal_bytes: usize,
    /// 采样总字节数。
    pub total_bytes: usize,
    /// 解出的假名(U+3041..=U+30FF)。
    pub kana: usize,
    /// 半角假名(U+FF61..=U+FF9F)。
    pub half_kana: usize,
    /// 汉字(U+2E80..=U+9FFF)。
    pub han: usize,
    /// 日文标点(、「」。『』【】・ー〜…！？与全角空格)。
    pub jp_punct: usize,
    /// 控制字符(0x00..0x20 除 \t \r \n)。
    pub controls: usize,
    /// 最终得分。
    pub points: f64,
}

impl Score {
    /// 合法字节占比。
    #[must_use]
    pub fn valid_ratio(&self) -> f64 {
        if self.total_bytes == 0 {
            return 0.0;
        }
        1.0 - self.illegal_bytes as f64 / self.total_bytes as f64
    }

    fn ratio(&self, n: usize) -> f64 {
        if self.total_bytes == 0 {
            return 0.0;
        }
        n as f64 / self.total_bytes as f64
    }
}

/// 自动识别结论(含两个候选的证据分,进 trace / golden 头部)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Detection {
    /// 裁决结果。
    pub codec: Codec,
    /// CP932 证据分。
    pub cp932: Score,
    /// GBK 证据分。
    pub gbk: Score,
}

/// 按架构 §7 的固定流程对采样池做编码识别。
///
/// `samples` 顺序 = 证据优先级(ybn 字符串常量 → 资源名 → 配置文本),
/// 计分按顺序累加,次序固定。
#[must_use]
pub fn detect(samples: &[&[u8]]) -> Detection {
    let mut cp932 = Score {
        illegal_bytes: 0,
        total_bytes: 0,
        kana: 0,
        half_kana: 0,
        han: 0,
        jp_punct: 0,
        controls: 0,
        points: 0.0,
    };
    let mut gbk = cp932;
    for sample in samples {
        score_one(Codec::Cp932, sample, &mut cp932);
        score_one(Codec::Gbk, sample, &mut gbk);
    }
    cp932.points = points(Codec::Cp932, &cp932);
    gbk.points = points(Codec::Gbk, &gbk);
    let codec = if gbk.points - cp932.points >= TIE_THRESHOLD {
        Codec::Gbk
    } else {
        // 差距不足或 CP932 领先:缺省 CP932(A3:平局裁决固定)
        Codec::Cp932
    };
    Detection {
        codec,
        cp932,
        gbk,
    }
}

fn points(codec: Codec, s: &Score) -> f64 {
    // 半角假名是 CP932 信号,但 GBK 字节流被 CP932 误读时也会冒出大量
    // "半角假名";因此无假名佐证(kana==0)时降权为 0.5(实测噪声形态,
    // 见 PROGRESS 成果 B2 的 auto 评分记录)。
    let hw_weight = if s.kana == 0 { 0.5 } else { 2.0 };
    let mut p = 4.0 * s.valid_ratio()
        - 8.0 * s.ratio(s.controls)
        + 6.0 * s.ratio(s.kana)
        + hw_weight * s.ratio(s.half_kana)
        + 1.25 * s.ratio(s.jp_punct)
        + 0.3 * s.ratio(s.han);
    if codec == Codec::Gbk {
        // 汉字加权;假名在 GBK 下只会以非法/乱码形态出现,倒扣
        p += 4.0 * s.ratio(s.han) - 2.0 * s.ratio(s.kana + s.half_kana);
    }
    p
}

fn score_one(codec: Codec, bytes: &[u8], out: &mut Score) {
    out.total_bytes += bytes.len();
    let mut i = 0usize;
    while i < bytes.len() {
        // ASCII 直通(与 Codec::decode 的主循环一致)
        if bytes[i] < 0x80 {
            classify(bytes[i] as char, out);
            i += 1;
            continue;
        }
        let step = match codec {
            Codec::Cp932 => super::CP932_TABLE.step(bytes, i),
            Codec::Gbk => super::GBK_TABLE.step(bytes, i),
        };
        match step {
            Some((unit, width)) => {
                if let Some(ch) = char::from_u32(u32::from(unit)) {
                    classify(ch, out);
                }
                i += width;
            }
            None => {
                out.illegal_bytes += 1;
                i += 1; // 固定重同步:前进 1 字节
            }
        }
    }
}

fn classify(ch: char, out: &mut Score) {
    let u = u32::from(ch);
    match u {
        0x3041..=0x30FF => out.kana += 1,
        0xFF61..=0xFF9F => out.half_kana += 1,
        0x3400..=0x9FFF => out.han += 1,
        0x20..=0x7E => {}
        0x09 | 0x0A | 0x0D => {}
        0x00..=0x1F => out.controls += 1,
        _ => {}
    }
    if matches!(
        ch,
        '、' | '「' | '」' | '。' | '『' | '』' | '【' | '】' | '・' | 'ー' | '〜' | '…' | '！' | '？' | '　'
    ) {
        out.jp_punct += 1;
    }
}
