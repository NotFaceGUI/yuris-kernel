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
        match value.to_ascii_lowercase().as_str() {
            "on" | "true" | "1" => Ok(true),
            "off" | "false" | "0" => Ok(false),
            other => Err(Failure::usage(format!(
                "--{name} 只接受 on|off,收到 `{other}`"
            ))),
        }
    }
}
