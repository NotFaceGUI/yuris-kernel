//! 错误码与 `yu-log::Diag` 构造。
//!
//! 命名空间固定为 `trace.*`,与 architecture §8 的"错误是结构化数据"一致。
//! 本 crate 内不存在任何直接写终端的路径;调用方决定把 `Diag` 送到哪个
//! [`yu_log::LogSink`](yu_log::LogSink)。

use yu_log::{Boundary, Diag};

/// 全部已登记错误码(顺序稳定)。
pub mod code {
    /// 文件读写失败。
    pub const IO: &str = "trace.io";
    /// 行不是合法 JSON,或字段类型/结构不符合本行 kind。
    pub const PARSE: &str = "trace.parse";
    /// schema 版本未知、kind 未知、sub 与载荷不一致、出现未声明字段。
    pub const SCHEMA: &str = "trace.schema";
    /// 行数不等或文件末尾缺少换行(录制被截断)。
    pub const TRUNCATED: &str = "trace.truncated";
    /// 两份文件各自合法,但记录条数不同(不是截断)。
    pub const LENGTH: &str = "trace.length";
    /// 回放输入(启动配置/时间线摘要)与录制时不一致。
    pub const REPLAY: &str = "trace.replay";
    /// 两条流在某一行的某一字段出现首分歧。
    pub const DIFF: &str = "trace.diff";
    /// 样本缺失,用例显式跳过(Info)。
    pub const SAMPLE_MISSING: &str = "trace.sample.missing";

    /// 全部码(唯一性由单元测试守卫)。
    pub const ALL_CODES: &[&str] = &[
        IO,
        PARSE,
        SCHEMA,
        TRUNCATED,
        LENGTH,
        REPLAY,
        DIFF,
        SAMPLE_MISSING,
    ];
}

/// 是否为本 crate 已登记的码。
pub fn is_registered(code: &str) -> bool {
    code::ALL_CODES.contains(&code)
}

pub(crate) fn io(detail: impl Into<String>) -> Diag {
    Diag::fatal(code::IO, Boundary::Data).with_detail(detail.into())
}

pub(crate) fn parse(line: u64, detail: impl Into<String>) -> Diag {
    Diag::fatal(code::PARSE, Boundary::Format)
        .with_detail(format!("line {line}: {}", detail.into()))
}

pub(crate) fn schema(line: u64, detail: impl Into<String>) -> Diag {
    Diag::fatal(code::SCHEMA, Boundary::Format)
        .with_detail(format!("line {line}: {}", detail.into()))
}

pub(crate) fn truncated(detail: impl Into<String>) -> Diag {
    Diag::fatal(code::TRUNCATED, Boundary::Format).with_detail(detail.into())
}

pub(crate) fn length(detail: impl Into<String>) -> Diag {
    Diag::fatal(code::LENGTH, Boundary::Format).with_detail(detail.into())
}

/// 样本缺失的显式跳过标记(Info;调用方把它送进自己的 `LogSink`)。
pub fn sample_missing(sample: &str, env: &str) -> Diag {
    Diag::info(code::SAMPLE_MISSING, Boundary::Data).with_detail(format!(
        "sample not found, skip: {sample} (env {env} is unset or not a file)"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_unique_and_namespaced() {
        let mut seen = std::collections::BTreeSet::new();
        for item in code::ALL_CODES {
            assert!(seen.insert(*item), "duplicate code: {item}");
            assert!(item.starts_with("trace."), "bad namespace: {item}");
        }
        for item in code::ALL_CODES {
            assert!(is_registered(item));
        }
        assert!(!is_registered("vfs.missing"));
    }
}
