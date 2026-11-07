//! 无依赖参数解析(`--name value` 与 `--name=value` 都接受;位置参数按序)。
//!
//! 只覆盖本 crate 的命令行;未知 `--flag` 显式报错,不静默忽略。

use std::collections::BTreeSet;

use crate::{CliResult, Failure};

pub struct ArgReader {
    tokens: Vec<String>,
    used: Vec<bool>,
}

impl ArgReader {
    pub fn new(tokens: Vec<String>) -> Self {
        let used = vec![false; tokens.len()];
        Self { tokens, used }
    }

    /// 第 `index` 个位置参数(跳过 `--flag` 与被 flag 消费的值)。
    pub fn positional(&mut self, index: usize) -> Option<String> {
        let mut seen = 0usize;
        for i in 0..self.tokens.len() {
            if self.used[i] || self.tokens[i].starts_with("--") {
                continue;
            }
            if seen == index {
                self.used[i] = true;
                return Some(self.tokens[i].clone());
            }
            seen += 1;
        }
        None
    }

    /// `--name value` / `--name=value`。
    pub fn flag(&mut self, name: &str) -> Option<String> {
        let eq = format!("--{name}=");
        for i in 0..self.tokens.len() {
            if self.used[i] {
                continue;
            }
            if let Some(value) = self.tokens[i].strip_prefix(&eq) {
                self.used[i] = true;
                return Some(value.to_string());
            }
            if self.tokens[i] == format!("--{name}") {
                self.used[i] = true;
                if i + 1 < self.tokens.len() && !self.tokens[i + 1].starts_with("--") {
                    self.used[i + 1] = true;
                    return Some(self.tokens[i + 1].clone());
                }
                return Some(String::new());
            }
        }
        None
    }

    /// 布尔开关。
    pub fn switch(&mut self, name: &str) -> bool {
        for i in 0..self.tokens.len() {
            if !self.used[i] && self.tokens[i] == format!("--{name}") {
                self.used[i] = true;
                return true;
            }
        }
        false
    }

    /// 未被消费的 `--flag` 一律报错(防止拼写错误静默生效)。
    pub fn finish(&self) -> CliResult<()> {
        let mut unknown = BTreeSet::new();
        for (i, token) in self.tokens.iter().enumerate() {
            if !self.used[i] && token.starts_with("--") {
                unknown.insert(token.split('=').next().unwrap_or(token).to_string());
            }
        }
        if unknown.is_empty() {
            Ok(())
        } else {
            Err(Failure::usage(format!(
                "未知参数:{}",
                unknown.into_iter().collect::<Vec<_>>().join(" ")
            )))
        }
    }

    /// 解析 `on|off`(大小写不敏感);其它值显式报错。
    pub fn on_off(name: &str, value: &str) -> CliResult<bool> {
        parse_on_off_flag(name, value)
    }
}

/// `--flag on|off|true|false|1|0` 的唯一解析(headless/desktop/serve 共用)。
///
/// 以前 [`ArgReader::on_off`] 与 `logging::parse_on_off` 是两份逐字相同的
/// 实现(只差 `trim` 与错误文案里有没有 `--`);现在只有这一份。
pub fn parse_on_off_flag(name: &str, value: &str) -> CliResult<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "on" | "true" | "1" => Ok(true),
        "off" | "false" | "0" => Ok(false),
        other => Err(Failure::usage(format!(
            "--{name} 只接受 on|off,收到 `{other}`"
        ))),
    }
}

/// `--save-codec`:强制存档块压缩层(`zlib|snappy`),`None` = 不改写自动判定。
///
/// 取值解析只有 [`yu_engine::engine::BlockCodec::parse`] 一份(headless /
/// desktop 两个宿主以前各写一段相同 match,Android 是第三段)。
pub fn save_codec_flag(
    value: Option<String>,
) -> CliResult<Option<yu_engine::engine::BlockCodec>> {
    match value {
        Some(value) => yu_engine::engine::BlockCodec::parse(&value)
            .map(Some)
            .map_err(|error| Failure::usage(format!("--save-codec: {error}"))),
        None => Ok(None),
    }
}

/// `--font` 的统一宿主映射:family 名 → `FontChoice::Family`;
/// 字体文件路径 → 读成字节 + `FontChoice::HostFont`。
///
/// 相对路径按工程根解析(字体拖进游戏目录后直接写文件名即可)。
/// 文件 IO 只发生在宿主侧;引擎只接收 `font_bytes` + `FontChoice`,
/// 与 Android 注入 assets 字体完全同一条路径。
pub fn font_options(
    root: &std::path::Path,
    value: Option<String>,
) -> CliResult<(yu_engine::engine::FontChoice, Vec<Vec<u8>>)> {
    use yu_engine::engine::FontChoice;

    let Some(value) = value.filter(|v| !v.trim().is_empty()) else {
        return Ok((FontChoice::Script, Vec::new()));
    };
    let value = value.trim();
    let path = std::path::Path::new(value);
    let lower = value.to_ascii_lowercase();
    let looks_like_file = path.is_file()
        || value.contains('/')
        || value.contains('\\')
        || [".ttf", ".otf", ".ttc", ".otc"]
            .iter()
            .any(|ext| lower.ends_with(ext));
    if !looks_like_file {
        return Ok((FontChoice::Family(value.to_string()), Vec::new()));
    }
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let bytes = std::fs::read(&resolved).map_err(|e| {
        Failure::usage(format!("读取字体 {} 失败: {e}", resolved.display()))
    })?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err(Failure::usage(format!(
            "字体 {} 超过 64MiB",
            resolved.display()
        )));
    }
    Ok((FontChoice::HostFont(0), vec![bytes]))
}
