//! 稳定错误码表。
//!
//! 码是数据: 可计数、可进 golden、可直链 `docs/divergences/` 档案。
//! 命名空间与 `architecture.md` §8 的七类边界一致。新增码必须同时加入
//! [`ALL_CODES`],单元测试会拒绝重复。

/// 数据源边界。
pub mod vfs {
    /// 查找键在数据源中不存在。
    pub const MISSING: &str = "vfs.missing";
    /// 请求的路径是目录,不能按文件打开。
    pub const NOT_DIR: &str = "vfs.not_dir";
    /// 查找键非法(空组件之外的安全/编码约束)。
    pub const PATH: &str = "vfs.path";
    /// 同一目录内多个条目归一后同名。
    pub const PATH_COLLISION: &str = "vfs.path.collision";
    /// 底层文件系统 I/O 失败。
    pub const IO: &str = "vfs.io";

    /// ypf 魔数不符。
    pub const YPF_BAD_MAGIC: &str = "vfs.ypf.bad_magic";
    /// ypf 版本不在官方接受范围。
    pub const YPF_BAD_VERSION: &str = "vfs.ypf.bad_version";
    /// ypf 头部字段自相矛盾(索引越界、条目数不可信)。
    pub const YPF_BAD_HEADER: &str = "vfs.ypf.bad_header";
    /// ypf 索引条目无法完整走过 / 不闭合。
    pub const YPF_BAD_INDEX: &str = "vfs.ypf.bad_index";
    /// 两种已证索引方言都无法一致解析。
    pub const YPF_UNKNOWN_DIALECT: &str = "vfs.ypf.unknown_dialect";
    /// 已识别但当前未实现的 ypf 语义(如 flag 非 0/1)。
    pub const YPF_UNSUPPORTED: &str = "vfs.ypf.unsupported";
    /// 条目解压失败或解压长度与索引不符。
    pub const YPF_DECOMPRESS: &str = "vfs.ypf.decompress";
    /// 条目自身字段不自洽(flag 与长度矛盾等)。
    pub const YPF_BAD_ENTRY: &str = "vfs.ypf.bad_entry";
    /// 名字 MurmurHash2 与索引存储值不符(可继续,但官方哈希查找可能失败)。
    pub const YPF_HASH_MISMATCH: &str = "vfs.ypf.hash_mismatch";
    /// 负载 MurmurHash2 与索引存储值不符;官方的多段续读语义未知。
    pub const YPF_CHECKSUM: &str = "vfs.ypf.checksum";
    /// NUL-XOR 方言的密钥探测出现并列最优,已按确定性规则取最小 key。
    pub const YPF_AMBIGUOUS_KEY: &str = "vfs.ypf.ambiguous_key";
    /// 包内多个条目归一后同名,查找只保留首个。
    pub const YPF_DUPLICATE_NAME: &str = "vfs.ypf.duplicate_name";
    /// 虚拟根别名与已有主键冲突。
    pub const YPF_ALIAS_COLLISION: &str = "vfs.ypf.alias_collision";
}

/// 日志边界。
pub mod log {
    /// 日志汇写入失败。
    pub const IO: &str = "log.io";
    /// JSONL 记录无法解析。
    pub const JSON: &str = "log.json";
}

/// 全部已登记错误码(顺序稳定)。唯一性由单元测试守卫。
pub const ALL_CODES: &[&str] = &[
    vfs::MISSING,
    vfs::NOT_DIR,
    vfs::PATH,
    vfs::PATH_COLLISION,
    vfs::IO,
    vfs::YPF_BAD_MAGIC,
    vfs::YPF_BAD_VERSION,
    vfs::YPF_BAD_HEADER,
    vfs::YPF_BAD_INDEX,
    vfs::YPF_UNKNOWN_DIALECT,
    vfs::YPF_UNSUPPORTED,
    vfs::YPF_DECOMPRESS,
    vfs::YPF_BAD_ENTRY,
    vfs::YPF_HASH_MISMATCH,
    vfs::YPF_CHECKSUM,
    vfs::YPF_AMBIGUOUS_KEY,
    vfs::YPF_DUPLICATE_NAME,
    vfs::YPF_ALIAS_COLLISION,
    log::IO,
    log::JSON,
];

/// 码是否已登记。用于诊断自检与测试。
pub fn is_registered(code: &str) -> bool {
    ALL_CODES.contains(&code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn codes_are_unique() {
        let mut seen = BTreeSet::new();
        for code in ALL_CODES {
            assert!(seen.insert(*code), "duplicate code: {code}");
        }
    }

    #[test]
    fn codes_have_namespace_prefix() {
        for code in ALL_CODES {
            assert!(
                code.starts_with("vfs.") || code.starts_with("log."),
                "code lacks registered namespace: {code}"
            );
        }
    }
}
