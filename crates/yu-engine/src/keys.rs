//! 脚本 XOR 密钥解析(YSTB)。
//!
//! 引擎的密钥 = `CRC32(字符串)` 按**大端**落进内存(`yu-format::key`:
//! 默认 `CRC32("Yu-ris") = 0xD36FAC96` → 字节 `D3 6F AC 96`);商业作品可以
//! 换掉这个字符串(实测 夏ノ鎖 = `CRC32("ClockupDl")`)。所以密钥是**每作
//! 可变的输入**,不是协议常量。
//!
//! # 开源实现的取钥路线(本模块照此实现,不发明新算法)
//!
//! | 来源 | 方法 |
//! |---|---|
//! | RxYuris `YSTB_Coder::GetXorKey` | V5:`uiArgsDataSize == 0 → key = 0`;否则读 `0x20 + uiInstIndexSize + 0x8` 处的 u32 |
//! | YU-RIS-Script-Editor `core/ystb.py:_guess_v5_key` + `application/analysis.py` | 逐 12 字节参数窗口:`k = u32@+0`、`size = u32@+4 ^ k`、`off = u32@+8 ^ k`,接受 `0 < size <= 4096 且 off + size <= argsDataSize`,取众数;跨最多 8 个 YBN 投票 |
//! | YurisKeyFinder | 运行期内存模式扫描(Detours 代理 DLL,需要官方进程;本仓库不用) |
//! | GARbro | 每作已知密钥表 + 文件名 key 从扩展名点号反推(不覆盖 YSTB) |
//!
//! 两条静态路线依赖同一事实:**参数窗口的明文里有已知 0 字段**(首窗口
//! `offset = 0`,或窗口首 dword = 0),因此密文直接暴露密钥。候选再用本仓库
//! 已有的结构自检确认(`Σ argc × 12 == s2`、`opcode < 命令数`,与
//! `yu-format::ystb::Segments::decrypt` 同判据),不靠猜。
//!
//! # 兜底
//!
//! 1. 显式 `--script-key`;
//! 2. 默认 `CRC32("Yu-ris")`;
//! 3. 最后才做 exe 字面量扫描(本仓库补充;**没有开源工具用这条**,见
//!    [`scan_exe_keys`] 的说明),同样必须过结构自检。

use std::collections::HashMap;
use std::path::PathBuf;

use yu_format::key::{crc32, DEFAULT_KEY_BYTES};
use yu_vfs::LookupKey;

use crate::project::{Project, CODEC};
use crate::Failure;

/// 单个样本的上限(解压后)。取钥只需要窗口结构,不需要整份大脚本。
const MAX_SAMPLE_BYTES: u64 = 4 * 1024 * 1024;

/// 参与投票的样本数上限(YU-RIS-Script-Editor 用 8;我们按大小取前 4)。
const MAX_SAMPLES: usize = 4;

/// 脚本-编辑器投票的窗口接受上限(其 `_guess_v5_key` 原文常量)。
const MAX_WINDOW_SIZE: u32 = 4096;

/// 密钥来源(诊断留痕)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeySource {
    /// 显式指定(`--script-key`)。
    Explicit,
    /// RxYuris 路线:参数索引区首窗口 `offset` 字段(明文 0 ⇒ 密文即密钥)。
    RxYurisHeader,
    /// Script-Editor 路线:参数窗口众数投票。
    EditorVote,
    /// 协议默认值。
    Default,
    /// 本仓库补充:exe 字面量的 `CRC32`(工具未用此路线)。
    ExeScan { path: PathBuf, text: String },
}

impl KeySource {
    /// 单行描述(日志/报告用)。
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            KeySource::Explicit => "explicit".to_string(),
            KeySource::RxYurisHeader => "rx-yuris:header".to_string(),
            KeySource::EditorVote => "script-editor:vote".to_string(),
            KeySource::Default => "default".to_string(),
            KeySource::ExeScan { path, text } => format!(
                "exe-scan:{}:{text}",
                path.file_name().unwrap_or_default().to_string_lossy()
            ),
        }
    }
}

/// 已解析的脚本密钥。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptKey {
    pub bytes: [u8; 4],
    pub source: KeySource,
}

impl ScriptKey {
    /// `43 2F 06 B0` 形式的十六进制(日志用)。
    #[must_use]
    pub fn hex(&self) -> String {
        self.bytes
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// 用于取钥/验证的样本脚本。
struct Sample {
    data: Vec<u8>,
}

impl Project {
    /// 解析本作的脚本密钥。
    ///
    /// 顺序:显式 → RxYuris 首窗口 → Script-Editor 投票 → 默认值 → exe 兜底。
    /// 每个候选都过结构自检;全部失败显式报错(不静默用错钥启动)。
    pub fn resolve_script_key(
        &self,
        explicit: Option<[u8; 4]>,
    ) -> Result<ScriptKey, Failure> {
        if let Some(bytes) = explicit {
            return Ok(ScriptKey {
                bytes,
                source: KeySource::Explicit,
            });
        }

        let (samples, command_count) = self.load_key_samples()?;
        if samples.is_empty() {
            // 没有脚本可验:保持默认值(缺表/缺脚本会在 boot 阶段显式报错)。
            return Ok(ScriptKey {
                bytes: DEFAULT_KEY_BYTES,
                source: KeySource::Default,
            });
        }

        if let Some(bytes) = rx_yuris_key(&samples)
            .filter(|key| validate_key(*key, &samples, command_count))
        {
            return Ok(ScriptKey {
                bytes,
                source: KeySource::RxYurisHeader,
            });
        }
        if let Some(bytes) =
            editor_vote_key(&samples).filter(|key| validate_key(*key, &samples, command_count))
        {
            return Ok(ScriptKey {
                bytes,
                source: KeySource::EditorVote,
            });
        }
        if validate_key(DEFAULT_KEY_BYTES, &samples, command_count) {
            return Ok(ScriptKey {
                bytes: DEFAULT_KEY_BYTES,
                source: KeySource::Default,
            });
        }
        if let Some(key) = self.scan_exe_keys(&samples, command_count) {
            return Ok(key);
        }
        Err(Failure::runtime(
            "无法解析脚本密钥:首窗口自举、窗口投票、默认值、exe 扫描都未通过结构自检;\
             换钥作品可用 --script-key 显式指定(8 位十六进制)",
        ))
    }

    /// 取验证样本(按解压大小取前 4,跳过超大文件)与命令表条数。
    fn load_key_samples(&self) -> Result<(Vec<Sample>, Option<usize>), Failure> {
        let source = self.open_source()?;
        let dir = LookupKey::new(b"ysbin", CODEC)
            .map_err(|d| Failure::runtime(format!("构造 ysbin 键失败: {d}")))?;
        let entries = source.list(&dir).unwrap_or_default();
        let mut scripts: Vec<(u64, Vec<u8>)> = entries
            .into_iter()
            .filter(|e| is_script_name(&e.name))
            .filter_map(|e| e.size.map(|size| (size, e.name)))
            .filter(|(size, _)| *size <= MAX_SAMPLE_BYTES)
            .collect();
        // 大脚本窗口多,投票更稳(Script-Editor 也按大小挑代表)。
        scripts.sort_by(|a, b| b.0.cmp(&a.0));
        scripts.truncate(MAX_SAMPLES);

        let mut samples = Vec::with_capacity(scripts.len());
        for (_, name) in scripts {
            let mut raw = b"ysbin/".to_vec();
            raw.extend_from_slice(&name);
            let Ok(key) = LookupKey::new(&raw, CODEC) else {
                continue;
            };
            if let Ok(data) = source.open(&key) {
                samples.push(Sample { data });
            }
        }

        let command_count = LookupKey::new(b"ysbin/ysc.ybn", CODEC)
            .ok()
            .and_then(|key| source.open(&key).ok())
            .filter(|data| data.len() >= 12 && &data[..4] == b"YSCM")
            .map(|data| u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize);
        Ok((samples, command_count))
    }

    /// exe 字面量兜底。
    ///
    /// **本仓库补充,不是工具路线**:YurisKeyFinder 走运行期内存扫描,
    /// RxYuris / Script-Editor 走数据自举。这里利用"密钥串在 exe 里是明文"
    /// 这一事实(实测 夏ノ鎖 的 `ClockupDl`),把可打印串的 `CRC32` 逐个
    /// 过结构自检;只在前面所有路线失败时才执行。
    fn scan_exe_keys(
        &self,
        samples: &[Sample],
        command_count: Option<usize>,
    ) -> Option<ScriptKey> {
        let mut exes: Vec<PathBuf> = Vec::new();
        for dir in [self.root(), self.data_root()] {
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue;
            };
            let mut found: Vec<PathBuf> = entries
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.is_file() && has_extension(p, "exe"))
                .collect();
            found.sort();
            for path in found {
                if !exes.contains(&path) {
                    exes.push(path);
                }
            }
        }
        for exe in exes {
            if let Some(key) = scan_one_exe(&exe, samples, command_count) {
                return Some(key);
            }
        }
        None
    }
}

/// RxYuris `YSTB_Coder::GetXorKey`(V5 分支):
/// `argsDataSize == 0` → 0;否则读 `0x20 + instIndexSize + 0x8` 的 u32。
fn rx_yuris_key(samples: &[Sample]) -> Option<[u8; 4]> {
    for sample in samples {
        let data = &sample.data;
        let Some(header) = header(data) else {
            continue;
        };
        if header.args_data == 0 {
            return Some([0, 0, 0, 0]);
        }
        let at = 0x20 + header.inst_index + 8;
        if at + 4 <= data.len() {
            return Some([data[at], data[at + 1], data[at + 2], data[at + 3]]);
        }
    }
    None
}

/// YU-RIS-Script-Editor `_guess_v5_key`:逐窗口把首 dword 当密钥候选,
/// 用 `size`/`offset` 的合理性筛选,取众数。
fn editor_vote_key(samples: &[Sample]) -> Option<[u8; 4]> {
    let mut votes: HashMap<[u8; 4], usize> = HashMap::new();
    for sample in samples {
        let data = &sample.data;
        let Some(header) = header(data) else {
            continue;
        };
        let args_start = 0x20 + header.inst_index;
        if header.args_index == 0
            || header.args_index % 12 != 0
            || args_start + header.args_index > data.len()
        {
            continue;
        }
        let mut at = args_start;
        while at + 12 <= args_start + header.args_index {
            let key = [data[at], data[at + 1], data[at + 2], data[at + 3]];
            let size = u32::from_le_bytes(data[at + 4..at + 8].try_into().unwrap()) ^
                u32::from_le_bytes(key);
            let offset = u32::from_le_bytes(data[at + 8..at + 12].try_into().unwrap()) ^
                u32::from_le_bytes(key);
            if size > 0
                && size <= MAX_WINDOW_SIZE
                && offset <= header.args_data
                && size <= header.args_data - offset
            {
                *votes.entry(key).or_insert(0) += 1;
            }
            at += 12;
        }
    }
    votes
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(key, _)| key)
}

/// YSTB 头(只取取钥需要的四个尺寸)。
struct Header {
    inst_index: usize,
    args_index: usize,
    args_data: u32,
}

fn header(data: &[u8]) -> Option<Header> {
    if data.len() < 0x20 || &data[..4] != b"YSTB" {
        return None;
    }
    Some(Header {
        inst_index: u32::from_le_bytes(data[0x0C..0x10].try_into().unwrap()) as usize,
        args_index: u32::from_le_bytes(data[0x10..0x14].try_into().unwrap()) as usize,
        args_data: u32::from_le_bytes(data[0x14..0x18].try_into().unwrap()),
    })
}

/// 结构自检:`Σ argc × 12 == s2`,且(有命令表时)`opcode < 命令数`。
fn validate_key(key: [u8; 4], samples: &[Sample], command_count: Option<usize>) -> bool {
    if samples.is_empty() {
        return false;
    }
    for sample in samples {
        let data = &sample.data;
        let Some(header) = header(data) else {
            return false;
        };
        let count = u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize;
        let args_index = header.args_index as usize;
        if count == 0 || header.inst_index < count * 4 || data.len() < 0x20 + count * 4 {
            return false;
        }
        let inst = &data[0x20..0x20 + count * 4];
        let mut sum = 0usize;
        for i in 0..count {
            let opcode = inst[i * 4] ^ key[0];
            let argc = inst[i * 4 + 1] ^ key[1];
            if let Some(limit) = command_count {
                if usize::from(opcode) >= limit {
                    return false;
                }
            }
            sum += usize::from(argc);
        }
        if sum * 12 != args_index {
            return false;
        }
    }
    true
}

/// 单个 exe:先试完整可打印串,再试前缀。
fn scan_one_exe(
    path: &std::path::Path,
    samples: &[Sample],
    command_count: Option<usize>,
) -> Option<ScriptKey> {
    const MAX_EXE_BYTES: usize = 64 * 1024 * 1024;
    const MAX_TEXT: usize = 48;
    const MAX_RUNS: usize = 500_000;

    let raw = std::fs::read(path).ok()?;
    let data = &raw[..raw.len().min(MAX_EXE_BYTES)];
    let runs = ascii_runs(data, MAX_TEXT, MAX_RUNS);
    let mut tried: HashMap<[u8; 4], ()> = HashMap::new();
    for run in &runs {
        for len in (1..=run.len()).rev() {
            let text = &run[..len];
            let key = crc32(text).to_be_bytes();
            if tried.insert(key, ()).is_some() {
                continue;
            }
            if validate_key(key, samples, command_count) {
                return Some(ScriptKey {
                    bytes: key,
                    source: KeySource::ExeScan {
                        path: path.to_path_buf(),
                        text: String::from_utf8_lossy(text).into_owned(),
                    },
                });
            }
        }
    }
    None
}

/// 可打印 ASCII 连续串(截断到 `max_text`,最多 `max_runs` 条)。
fn ascii_runs<'a>(data: &'a [u8], max_text: usize, max_runs: usize) -> Vec<&'a [u8]> {
    let mut runs = Vec::new();
    let mut start = 0usize;
    for i in 0..data.len() {
        if (0x20..=0x7E).contains(&data[i]) {
            continue;
        }
        if i > start {
            runs.push(&data[start..(start + max_text).min(i)]);
            if runs.len() >= max_runs {
                return runs;
            }
        }
        start = i + 1;
    }
    if data.len() > start {
        runs.push(&data[start..(start + max_text).min(data.len())]);
    }
    runs
}

/// `ystNNNNN.ybn`(排除 `yst_list.ybn` 等元数据表)。
fn is_script_name(name: &[u8]) -> bool {
    name.len() == 12
        && name.starts_with(b"yst")
        && name.ends_with(b".ybn")
        && name[3..8].iter().all(u8::is_ascii_digit)
}

fn has_extension(path: &std::path::Path, ext: &str) -> bool {
    path.extension()
        .is_some_and(|value| value.eq_ignore_ascii_case(ext))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 合成一个最小 YSTB:count=1,s1=4,s2=12(一个窗口),s3=12,s4=4。
    ///
    /// 窗口明文 = {id/type=0, size=12, offset=0} ⇒ 首 dword 与 offset 字段
    /// 的密文都等于密钥本身,两条自举路线都能命中。
    fn synthetic_script(key: [u8; 4]) -> Vec<u8> {
        fn xor(bytes: &[u8], key: [u8; 4]) -> Vec<u8> {
            bytes
                .iter()
                .enumerate()
                .map(|(i, b)| b ^ key[i & 3])
                .collect()
        }
        let mut out = Vec::new();
        out.extend_from_slice(b"YSTB");
        out.extend_from_slice(&495u32.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes()); // count
        out.extend_from_slice(&4u32.to_le_bytes()); // s1
        out.extend_from_slice(&12u32.to_le_bytes()); // s2
        out.extend_from_slice(&12u32.to_le_bytes()); // s3
        out.extend_from_slice(&4u32.to_le_bytes()); // s4
        out.extend_from_slice(&0u32.to_le_bytes()); // tail
        // 指令:opcode=96(TEXT), argc=1
        out.extend_from_slice(&xor(&[96, 1, 0, 0], key));
        // 参数窗口:{id=0,type=0,size=12,offset=0}
        out.extend_from_slice(&xor(&[0, 0, 0, 0, 12, 0, 0, 0, 0, 0, 0, 0], key));
        // 参数数据 12 字节
        out.extend_from_slice(&xor(&[0u8; 12], key));
        // 行号
        out.extend_from_slice(&xor(&[1, 0, 0, 0], key));
        out
    }

    fn sample(key: [u8; 4]) -> Sample {
        Sample {
            data: synthetic_script(key),
        }
    }

    #[test]
    fn rx_yuris_reads_first_window_offset() {
        let key = [0x43, 0x2F, 0x06, 0xB0];
        let samples = vec![sample(key)];
        assert_eq!(rx_yuris_key(&samples), Some(key));
        assert!(validate_key(key, &samples, Some(119)));
    }

    #[test]
    fn editor_vote_recovers_key() {
        let key = [0x11, 0x22, 0x33, 0x44];
        let samples = vec![sample(key)];
        assert_eq!(editor_vote_key(&samples), Some(key));
    }

    #[test]
    fn wrong_key_fails_self_check() {
        let samples = vec![sample([0x43, 0x2F, 0x06, 0xB0])];
        assert!(!validate_key(DEFAULT_KEY_BYTES, &samples, Some(119)));
    }

    #[test]
    fn exe_scan_finds_crc32_of_string() {
        let text = b"ClockupDl";
        let key = crc32(text).to_be_bytes();
        let samples = vec![sample(key)];

        let dir = std::env::var_os("YURIS_TEST_TMP")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../target/test-tmp")
            });
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join(format!(
            "fake-key-{}-{}.exe",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&exe, b"\x00\x01\x02ClockupDl\x00Yu-ris\x00").unwrap();
        let found = scan_one_exe(&exe, &samples, Some(119)).expect("exe 扫描应命中");
        assert_eq!(found.bytes, key);
        assert!(matches!(found.source, KeySource::ExeScan { .. }));
        std::fs::remove_file(&exe).ok();
    }
}
