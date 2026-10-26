//! 稳定错误码表(`yu-rt` 命名空间)。
//!
//! `yu-log::code` 目前只有 `vfs.*` / `log.*`(会话 A 所有)。本任务书要求
//! `rt.*` / `table.*` / `fmt.*` 码域;为避免跨会话改动 `yu-log`,这里**先在
//! 本 crate 定义并登记**,由仓库主人后续并入 `yu-log::code`(文件头已注明)。
//!
//! 码是数据:可计数、可进 golden、可直链 `crates/yu-rt/docs/divergences/`。

/// 表装载边界(A5:缺表拒绝启动)。
pub mod table {
    /// 必需表不存在。
    pub const MISSING: &str = "table.missing";
    /// 表存在但结构不完整/自相矛盾。
    pub const INCOMPLETE: &str = "table.incomplete";
    /// 表魔数不符。
    pub const BAD_MAGIC: &str = "table.bad_magic";
    /// 表版本字段与引擎/工程不一致。
    pub const VERSION: &str = "table.version";
    /// 表内引用越界(标签→脚本、变量 id 越界等)。
    pub const REFERENCE: &str = "table.reference";
    /// 表要求的运行期数据缺失(如系统变量未实现取值来源)。
    pub const UNSUPPORTED: &str = "table.unsupported";
    /// 工程数据缺失,运行核启用内嵌兜底(非 Fatal;兜底值已用工程数据校验)。
    pub const FALLBACK: &str = "table.fallback";
}

/// 格式边界(`.ybn` 家族解码)。
pub mod fmt {
    /// 文件头/区段长度不闭合。
    pub const LAYOUT: &str = "fmt.layout";
    /// 区段越界。
    pub const OUT_OF_BOUNDS: &str = "fmt.out_of_bounds";
    /// 记录流无法完整走过。
    pub const RECORD: &str = "fmt.record";
    /// 未取证的文件变体。
    pub const UNSUPPORTED: &str = "fmt.unsupported";
}

/// 运行核边界。
pub mod rt {
    /// 未实现/未取证的命令(挂号留痕)。
    pub const CMD_UNIMPLEMENTED: &str = "rt.cmd.unimplemented";
    /// 官方 0.495 处理体为空桩的命令(MIDI 族 `0x44aa40/60/80`、CDDA
    /// `0x429fb0`),内核同型对齐(aligned-stub,不计挂号)。
    pub const CMD_ALIGNED_STUB: &str = "rt.cmd.aligned_stub";
    /// 当前表里不存在的命令号(YSCM 覆盖不到)。
    pub const CMD_UNKNOWN: &str = "rt.cmd.unknown";
    /// 命令窗口数与处理器要求不符。
    pub const CMD_WINDOWS: &str = "rt.cmd.windows";
    /// 参数槽缺失(处理器要求的窗口不存在/为空)。
    pub const CMD_SLOT: &str = "rt.cmd.slot";
    /// 任务 ID 重复/不存在等任务语义问题。
    pub const TASK: &str = "rt.task";
    /// `SCRIPTPOS` 执行位快照损坏/脚本名无法解析。
    pub const SCRIPT_POS: &str = "rt.scriptpos";
    /// 任务嵌套/循环嵌套超过引擎上限。
    pub const NESTING: &str = "rt.nesting";
    /// 标签不存在。
    pub const LABEL: &str = "rt.label";
    /// 变量未声明。
    pub const VAR_UNDECLARED: &str = "rt.var.undeclared";
    /// 目标不是数组 / 维数不匹配 / 下标越界。
    pub const VAR_SHAPE: &str = "rt.var.shape";
    /// 变量类型不匹配(数值/字符串混用)。
    pub const VAR_TYPE: &str = "rt.var.type";
    /// 表达式记录流损坏(操作数长度越界)。
    pub const EXPR_MALFORMED: &str = "rt.expr.malformed";
    /// 表达式操作码未取证。
    pub const EXPR_UNSUPPORTED: &str = "rt.expr.unsupported";
    /// 整数除零(引擎为硬件异常,内核按 Fatal 处理并留档)。
    pub const EXPR_DIV_ZERO: &str = "rt.expr.div_zero";
    /// 系统变量读取路径未实现。
    pub const SYSVAR_UNIMPLEMENTED: &str = "rt.sysvar.unimplemented";
    /// 系统变量只读却发生写入。
    pub const SYSVAR_READONLY: &str = "rt.sysvar.readonly";
    /// 可赋值系统变量(手册标 ○)的**写入语义**未取证。
    pub const SYSVAR_WRITE: &str = "rt.sysvar.write";
    /// 脚本执行错误(官方错误策略未取证时的统一出口)。
    pub const SCRIPT_ERROR: &str = "rt.script.error";
    /// 运行核内部不变量被破坏(应为不可能)。
    pub const INTERNAL: &str = "rt.internal";
    /// 未取证的语义细节(类型提升/溢出/格式等),已按确定性规则实现。
    pub const UNDETERMINED: &str = "rt.undetermined";
}

/// 全部已登记错误码(顺序稳定)。唯一性由单元测试守卫。
pub const ALL_CODES: &[&str] = &[
    table::MISSING,
    table::INCOMPLETE,
    table::BAD_MAGIC,
    table::VERSION,
    table::REFERENCE,
    table::UNSUPPORTED,
    table::FALLBACK,
    fmt::LAYOUT,
    fmt::OUT_OF_BOUNDS,
    fmt::RECORD,
    fmt::UNSUPPORTED,
    rt::CMD_UNIMPLEMENTED,
    rt::CMD_UNKNOWN,
    rt::CMD_WINDOWS,
    rt::CMD_SLOT,
    rt::TASK,
    rt::SCRIPT_POS,
    rt::NESTING,
    rt::LABEL,
    rt::VAR_UNDECLARED,
    rt::VAR_SHAPE,
    rt::VAR_TYPE,
    rt::EXPR_MALFORMED,
    rt::EXPR_UNSUPPORTED,
    rt::EXPR_DIV_ZERO,
    rt::SYSVAR_UNIMPLEMENTED,
    rt::SYSVAR_READONLY,
    rt::SYSVAR_WRITE,
    rt::SCRIPT_ERROR,
    rt::INTERNAL,
    rt::UNDETERMINED,
];

/// 码是否已登记。
pub fn is_registered(code: &str) -> bool {
    ALL_CODES.contains(&code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn codes_are_unique_and_namespaced() {
        let mut seen = BTreeSet::new();
        for code in ALL_CODES {
            assert!(seen.insert(*code), "duplicate code: {code}");
            assert!(
                code.starts_with("table.") || code.starts_with("fmt.") || code.starts_with("rt."),
                "code lacks yu-rt namespace: {code}"
            );
        }
    }
}
