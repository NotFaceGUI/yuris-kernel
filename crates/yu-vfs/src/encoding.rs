//! 文本编码:身份、字节结构扫描、确定性检测。
//!
//! # 分层(对应 `docs/architecture.md` §7)
//!
//! | 层 | 职责 | 所在 crate |
//! |---|---|---|
//! | 字节边界 | 资源名归一的跳步表 | `yu-vfs`(本模块的 [`Codec`]) |
//! | 字节边界 | 码表解码/编码(CP932/GBK ↔ Unicode) | `yu-format`(任务书 B) |
//! | 决策 | 启动早期一次性判定"这份数据是什么编码" | 本模块 [`detect`] |
//!
//! 本模块**不含码表**:它只做字节级合法序分析 + 构成统计,因此零依赖、
//! 可审计、结论可复现。码表到位后,`yu-format` 的解码器会给出更强的
//! 汉字/假名证据,可替换 [`Score`] 的两个 `hint` 项;接口不变。
//!
//! # 检测算法(确定性)
//!
//! 1. 固定候选顺序: `Utf8, Cp932, Gbk, Big5`。
//! 2. 每个候选按自己的合法序规则扫描字节,统计:
//!    合法双字节数、CP932 全角假名数(lead 0x82/0x83)、半角假名数
//!    (0xA1–0xDF 单字节)、控制字节数、非法序列数。
//! 3. 整数打分(百分之一为单位,无浮点,无平台差异)。
//! 4. 平局裁决固定: `preferred` 与最高分差 ≤ 100 时取 `preferred`;
//!    否则 `Cp932` 与最高分差 ≤ 100 时取 `Cp932`;再否则按候选顺序。
//! 5. 结论、各候选得分与 `decisive` 标志一并返回,供 trace/golden 记录。
//!
//! 权重来自开源参考实现 `YU-RIS-Script-Editor/core/encoding.py` 的**判据**
//! (假名强信号、CJK 弱正分、SJIS 近分优先、BOM 直判),本仓库重新实现为
//! 字节结构版;
//! 因缺少码表,`simplified_hint`/`traditional_hint` 两项暂记为 0,
//! 结论在短样本上可能不唯一 —— 此时 `decisive == false`,调用方应要求
//! 显式 `--codec`(与 §7 "绝不中途切换" 一致)。

pub use crate::codec::Codec;

/// 检测阶段考虑的文本编码。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TextEncoding {
    Utf8,
    Cp932,
    Gbk,
    Big5,
}

impl TextEncoding {
    /// 固定候选顺序(平局裁决依赖它)。
    pub const CANDIDATES: [TextEncoding; 4] = [
        TextEncoding::Utf8,
        TextEncoding::Cp932,
        TextEncoding::Gbk,
        TextEncoding::Big5,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            TextEncoding::Utf8 => "utf-8",
            TextEncoding::Cp932 => "cp932",
            TextEncoding::Gbk => "gbk",
            TextEncoding::Big5 => "big5",
        }
    }

    /// 资源名归一可用的编解码器;UTF-8/Big5 名不进 VFS 查找键。
    pub const fn as_codec(self) -> Option<Codec> {
        match self {
            TextEncoding::Cp932 => Some(Codec::Cp932),
            TextEncoding::Gbk => Some(Codec::Gbk),
            TextEncoding::Utf8 | TextEncoding::Big5 => None,
        }
    }
}

impl From<Codec> for TextEncoding {
    fn from(codec: Codec) -> Self {
        match codec {
            Codec::Cp932 => TextEncoding::Cp932,
            Codec::Gbk => TextEncoding::Gbk,
        }
    }
}

/// 单个候选的扫描统计与得分。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Score {
    pub encoding: TextEncoding,
    /// 0..=10000 的百分制得分。
    pub points: i64,
    /// 合法双字节字符数。
    pub double_bytes: usize,
    /// CP932 全角假名数(lead 0x82/0x83);其它候选恒 0。
    pub fullwidth_kana: usize,
    /// CP932 半角假名数(0xA1–0xDF 单字节);其它候选恒 0。
    pub halfwidth_kana: usize,
    /// 近似 CJK 计数(双字节且不落在假名块)。
    pub cjk: usize,
    /// 非法序列数。
    pub invalid: usize,
    /// 控制字节数(允许 `\r\n\t`)。
    pub controls: usize,
}

impl Score {
    fn new(encoding: TextEncoding) -> Self {
        Self {
            encoding,
            points: 0,
            double_bytes: 0,
            fullwidth_kana: 0,
            halfwidth_kana: 0,
            cjk: 0,
            invalid: 0,
            controls: 0,
        }
    }
}

/// 检测结论。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    /// 判定结果。
    pub encoding: TextEncoding,
    /// 是否只有这一个候选能解释输入。
    pub only_candidate: bool,
    /// 与次优的差距是否足以支撑自动判定(供 trace 审计)。
    pub decisive: bool,
    /// 是否命中 UTF-8 BOM(命中时跳过打分)。
    pub bom: bool,
    /// 按固定候选顺序排列的得分。
    pub scores: [Score; 4],
}

impl Decision {
    /// 判定结果对应的 VFS 名字编解码器(UTF-8/Big5 名不支持)。
    pub fn codec(&self) -> Option<Codec> {
        self.encoding.as_codec()
    }

    /// 单行证据文本(进 trace/golden 头)。
    pub fn evidence_line(&self) -> String {
        let mut out = format!(
            "encoding={} bom={} only={} decisive={}",
            self.encoding.as_str(),
            self.bom,
            self.only_candidate,
            self.decisive
        );
        for score in &self.scores {
            out.push_str(&format!(
                " | {}={} (2B={} kana={}+{} cjk={} bad={} ctl={})",
                score.encoding.as_str(),
                score.points,
                score.double_bytes,
                score.fullwidth_kana,
                score.halfwidth_kana,
                score.cjk,
                score.invalid,
                score.controls
            ));
        }
        out
    }
}

/// 检测文本编码。
///
/// `preferred` 是调用方显式配置的编码(例如 CLI `--codec`);它在与最高分
/// 接近时优先,但不会推翻明显更优的候选。未指定时按固定候选顺序裁决。
pub fn detect(bytes: &[u8], preferred: Option<TextEncoding>) -> Decision {
    if bytes.starts_with(b"\xef\xbb\xbf") {
        let mut scores = [
            Score::new(TextEncoding::Utf8),
            Score::new(TextEncoding::Cp932),
            Score::new(TextEncoding::Gbk),
            Score::new(TextEncoding::Big5),
        ];
        scores[0].points = 10_000;
        return Decision {
            encoding: TextEncoding::Utf8,
            only_candidate: false,
            decisive: true,
            bom: true,
            scores,
        };
    }

    let scores = [
        scan_utf8(bytes),
        scan_cp932(bytes),
        scan_gbk(bytes),
        scan_big5(bytes),
    ];

    let viable: Vec<usize> = scores
        .iter()
        .enumerate()
        .filter(|(_, s)| s.invalid == 0)
        .map(|(i, _)| i)
        .collect();

    if viable.is_empty() {
        return Decision {
            encoding: preferred.unwrap_or(TextEncoding::Cp932),
            only_candidate: false,
            decisive: false,
            bom: false,
            scores,
        };
    }

    // 稳定排序: 分数降序,同分保持候选顺序。
    let mut ranked: Vec<usize> = viable;
    ranked.sort_by(|&a, &b| scores[b].points.cmp(&scores[a].points));

    let top = ranked[0];
    let runner_up = ranked.get(1).copied();
    let margin = runner_up
        .map(|i| scores[top].points - scores[i].points)
        .unwrap_or(i64::MAX);

    let mut chosen = top;
    if let Some(pref) = preferred {
        if let Some(idx) = TextEncoding::CANDIDATES.iter().position(|e| *e == pref) {
            if scores[idx].invalid == 0 && scores[top].points - scores[idx].points <= 100 {
                chosen = idx;
            }
        }
    } else if scores[top].encoding != TextEncoding::Cp932 {
        let cp = TextEncoding::CANDIDATES
            .iter()
            .position(|e| *e == TextEncoding::Cp932)
            .unwrap();
        if scores[cp].invalid == 0 && scores[top].points - scores[cp].points <= 100 {
            chosen = cp;
        }
    }

    Decision {
        encoding: scores[chosen].encoding,
        only_candidate: ranked.len() == 1,
        decisive: margin >= 100 || ranked.len() == 1,
        bom: false,
        scores,
    }
}

/// ASCII 与可打印性带来的基础分。
fn base_points(bytes: &[u8], invalid: usize, controls: usize) -> i64 {
    let len = bytes.len().max(1) as i64;
    let printable = bytes
        .iter()
        .filter(|b| **b >= 0x20 && **b != 0x7f)
        .count() as i64;
    100 * printable / len - 400 * invalid as i64 - 800 * controls as i64
}

fn is_allowed_control(b: u8) -> bool {
    matches!(b, b'\r' | b'\n' | b'\t')
}

fn scan_utf8(bytes: &[u8]) -> Score {
    let mut score = Score::new(TextEncoding::Utf8);
    let mut controls = 0usize;
    let mut fullwidth_kana = 0usize;
    let mut cjk = 0usize;
    let mut non_ascii = false;

    match std::str::from_utf8(bytes) {
        Ok(text) => {
            for ch in text.chars() {
                let cp = ch as u32;
                match cp {
                    0x3040..=0x30ff => fullwidth_kana += 1,
                    0x31f0..=0x31ff => fullwidth_kana += 1,
                    0xff66..=0xff9f => fullwidth_kana += 1,
                    0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xf900..=0xfaff => cjk += 1,
                    0xac00..=0xd7af => cjk += 1,
                    _ => {}
                }
                if cp < 0x20 && !matches!(ch, '\r' | '\n' | '\t') {
                    controls += 1;
                }
                if cp >= 0x80 {
                    non_ascii = true;
                }
            }
            score.double_bytes = text.chars().filter(|c| (*c as u32) >= 0x80).count();
        }
        Err(err) => {
            score.invalid = 1;
            let valid_up_to = err.valid_up_to();
            if let Ok(prefix) = std::str::from_utf8(&bytes[..valid_up_to]) {
                score.double_bytes = prefix.chars().filter(|c| (*c as u32) >= 0x80).count();
            }
            for &b in &bytes[valid_up_to..] {
                if b < 0x20 && !is_allowed_control(b) {
                    controls += 1;
                } else if b >= 0x80 {
                    score.invalid += 1;
                }
            }
        }
    }

    score.controls = controls;
    score.fullwidth_kana = fullwidth_kana;
    score.cjk = cjk;
    score.points = base_points(bytes, score.invalid, controls)
        + 100
        + 150 * fullwidth_kana as i64
        + 12 * cjk as i64
        + if non_ascii { 200 } else { 0 };
    score
}

fn scan_cp932(bytes: &[u8]) -> Score {
    let mut score = Score::new(TextEncoding::Cp932);
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b < 0x80 {
            if b < 0x20 && !is_allowed_control(b) {
                score.controls += 1;
            }
            i += 1;
        } else if (0xa1..=0xdf).contains(&b) {
            // CP932 半角片假名单字节;GBK 同区间是尾字节 —— 因此只给弱权重。
            score.halfwidth_kana += 1;
            i += 1;
        } else if Codec::Cp932.is_lead(b) && i + 1 < bytes.len() && Codec::Cp932.is_trail(bytes[i + 1])
        {
            score.double_bytes += 1;
            if b == 0x82 || b == 0x83 {
                score.fullwidth_kana += 1;
            } else {
                score.cjk += 1;
            }
            i += 2;
        } else {
            score.invalid += 1;
            i += 1;
        }
    }
    score.points = base_points(bytes, score.invalid, score.controls)
        + 60 * score.fullwidth_kana as i64
        + 15 * score.halfwidth_kana as i64
        + 20 * score.cjk as i64
        + if score.fullwidth_kana > 0 { 400 } else { 0 };
    score
}

fn scan_gbk(bytes: &[u8]) -> Score {
    let mut score = Score::new(TextEncoding::Gbk);
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b < 0x80 {
            if b < 0x20 && !is_allowed_control(b) {
                score.controls += 1;
            }
            i += 1;
        } else if Codec::Gbk.is_lead(b) && i + 1 < bytes.len() && Codec::Gbk.is_trail(bytes[i + 1])
        {
            // GB2312 汉字区(lead 0xB0–0xF7, trail 0xA1–0xFE)是强信号。
            if (0xb0..=0xf7).contains(&b) && (0xa1..=0xfe).contains(&bytes[i + 1]) {
                score.cjk += 1;
            }
            score.double_bytes += 1;
            i += 2;
        } else {
            score.invalid += 1;
            i += 1;
        }
    }
    score.points = base_points(bytes, score.invalid, score.controls)
        + 35 * score.double_bytes as i64
        + 300 * score.cjk as i64;
    score
}

fn scan_big5(bytes: &[u8]) -> Score {
    let mut score = Score::new(TextEncoding::Big5);
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b < 0x80 {
            if b < 0x20 && !is_allowed_control(b) {
                score.controls += 1;
            }
            i += 1;
        } else if (0x81..=0xfe).contains(&b)
            && i + 1 < bytes.len()
            && ((0x40..=0x7e).contains(&bytes[i + 1]) || (0xa1..=0xfe).contains(&bytes[i + 1]))
        {
            score.double_bytes += 1;
            if (0xa4..=0xc6).contains(&b) {
                score.cjk += 1;
            }
            i += 2;
        } else {
            score.invalid += 1;
            i += 1;
        }
    }
    score.points = base_points(bytes, score.invalid, score.controls)
        + 35 * score.double_bytes as i64
        + 280 * score.cjk as i64;
    score
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bom_forces_utf8() {
        let decision = detect("\u{feff}日本語".as_bytes(), None);
        assert_eq!(decision.encoding, TextEncoding::Utf8);
        assert!(decision.bom && decision.decisive);
    }

    #[test]
    fn ascii_is_deterministic_and_defaults_to_cp932() {
        let a = detect(b"ysbin/yst00001.ybn", None);
        let b = detect(b"ysbin/yst00001.ybn", None);
        assert_eq!(a, b, "detection must be deterministic");
        assert_eq!(a.encoding, TextEncoding::Cp932);
        // 纯 ASCII 在四种候选下都合法,因此不是"唯一候选";
        // 结论来自固定候选顺序(UTF-8 分数相同也不抢,缺省 CP932)。
        assert!(!a.only_candidate);
        assert_eq!(a.evidence_line(), b.evidence_line());
    }

    #[test]
    fn utf8_japanese_is_utf8() {
        let decision = detect("こんにちは世界".as_bytes(), None);
        assert_eq!(decision.encoding, TextEncoding::Utf8);
        assert!(decision.decisive);
    }

    #[test]
    fn cp932_fullwidth_kana_wins() {
        // こんにちは (CP932: 82B1 82F1 82C9 82BF 82CD)
        let bytes = [0x82, 0xb1, 0x82, 0xf1, 0x82, 0xc9, 0x82, 0xbf, 0x82, 0xcd];
        let decision = detect(&bytes, None);
        assert_eq!(decision.encoding, TextEncoding::Cp932);
        let cp = &decision.scores[1];
        assert_eq!(cp.fullwidth_kana, 5);
        assert_eq!(cp.invalid, 0);
    }

    #[test]
    fn gbk_long_hanzi_text_prefers_gbk() {
        // 中文测试 (GBK: D6D0 CEC4 B2E2 CAD4)
        let bytes = [0xd6, 0xd0, 0xce, 0xc4, 0xb2, 0xe2, 0xca, 0xd4];
        let decision = detect(&bytes, None);
        assert_eq!(decision.encoding, TextEncoding::Gbk);
        let gbk = &decision.scores[2];
        assert_eq!(gbk.invalid, 0);
        assert_eq!(gbk.cjk, 4);
    }

    #[test]
    fn explicit_preference_breaks_near_ties() {
        let bytes = [0xd6, 0xd0, 0xce, 0xc4];
        let with_pref = detect(&bytes, Some(TextEncoding::Gbk));
        assert_eq!(with_pref.encoding, TextEncoding::Gbk);
    }

    #[test]
    fn invalid_sequences_are_counted() {
        let decision = detect(&[0x81, 0x20, 0xff, 0x00], None);
        assert!(decision.scores.iter().any(|s| s.invalid > 0));
        assert!(!decision.decisive);
    }

    #[test]
    fn scores_cover_every_candidate_in_fixed_order() {
        let decision = detect(b"plain text", None);
        let order: Vec<TextEncoding> = decision.scores.iter().map(|s| s.encoding).collect();
        assert_eq!(order, TextEncoding::CANDIDATES.to_vec());
    }

    #[test]
    fn evidence_line_is_stable() {
        let decision = detect(b"abc", None);
        assert_eq!(decision.evidence_line(), detect(b"abc", None).evidence_line());
        assert!(decision.evidence_line().starts_with("encoding=cp932"));
    }
}
