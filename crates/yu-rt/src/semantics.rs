//! 已装载表的**语义形态**快照(`SemanticProfile`)。
//!
//! # 为什么不是 `ProjectProfile`
//!
//! 现有「版本工具」各管一段,但没有「已装载表 → 语义形态」的单一派生快照:
//!
//! | 类型 | 位置 | 回答的问题 |
//! |---|---|---|
//! | [`crate::sysvar_abi::AbiProfile`] | `sysvar_abi.rs` | 系统变量 ABI 前缀/槽位来源 |
//! | `PatchProfile` | `yu-engine/src/patch.rs` | 补丁文件集/覆盖顺序 |
//! | `YpfProfile` | `yu-vfs/src/ypf.rs` | 封包格式/key/入口布局 |
//! | `Identity` | `yu-engine/src/identity.rs` | 引擎版本、标题、窗口信息 |
//! | **本类型** | `semantics.rs` | 本工程**已装载表**的语义形态 |
//!
//! 名字刻意避开 `ProjectProfile`:后者与 `PatchProfile`/`AbiProfile` 太容易混。
//!
//! # 纪律
//!
//! - 只放**当前有门控需求**的字段;没有消费者的字段不加,避免变成上帝对象。
//! - 宿主/路径/补丁事实**不进**本类型(那是 `Identity`/`PatchProfile` 的事)。
//! - `Identity.engine_version` 是展示字段,处理器不得反查;行为门控只认本类型。
//! - 本类型由 [`TableSet::load_from`](crate::tables::TableSet::load_from) 的
//!   结果派生一次,存进 [`Vm`](crate::vm::Vm),不在每条命令里重算。
//!
//! # 证据锚点
//!
//! `VARINFO[SJISCODE]` 的语义随工程目标版本分代:
//!
//! - 0.495 及更早:`yu-ris.exe` 处理体 `0x456050..0x456ae4` 对槽常量
//!   `0x68416f/0x684170` 零引用 ⇒ 惰性参数,按 `aligned-stub` 对齐;
//! - v555:处理体 `0x455200` 的 `0x4554a9` 分支读 `SET` 字符串首字符并写
//!   `LET`(单字节 = 字节值;双字节 = `(lead << 8) | trail`,`0x455b9c`);
//!   v555 的剧本词法器 `es_scenario_lexer.yst` 依赖它做字符分类。
//!
//! 判别来源是工程自带的 `ysbin/ysc.ybn` 头版本(`YscmTable::version`),
//! 不是宿主猜的引擎版本。
//!
//! # 当前消费状态
//!
//! 本批（结构重构、行为保持）只派生/保存该快照,`VARINFO` 仍按 0.495
//! 惰性参数处理;v555 批次再把 `SJISCODE` 门控切到
//! [`SemanticProfile::varinfo_sjis`],并实现 `FirstChar` 的取值。

/// `VARINFO[SJISCODE]` 的语义形态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SjisCodeMode {
    /// 官方处理体不读该槽:与「只给惰性参数」的脚本按无操作对齐。
    ///
    /// 证据:0.495 `yu-ris.exe` 处理体 `0x456050..0x456ae4` 对
    /// `0x68416f/0x684170` 零引用。
    Inert,
    /// 真查询项:读 `SET` 字符串首字符,单字节 = 字节值,双字节 =
    /// `(lead << 8) | trail`。
    ///
    /// 证据:v555 处理体 `0x455200` 的 `0x4554a9` 分支与 `0x455b9c`
    /// 的 `SHL EAX,0x8` + `MOVZX ECX,[EDX+1]` + `ADD`。
    FirstChar,
}

/// 本工程已装载表的语义形态(启动时派生一次)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SemanticProfile {
    /// 工程 `ysbin/ysc.ybn` 头的目标引擎版本(如实记录,不做换算)。
    ///
    /// 这是**门控输入**,不是展示字段;处理器不得再直接比较
    /// `tables.boot.yscm.version`。
    pub ysc_version: u32,
    /// `VARINFO[SJISCODE]` 形态。
    pub varinfo_sjis: SjisCodeMode,
}

impl SemanticProfile {
    /// 由工程命令表版本派生。
    ///
    /// 版本阈值来自 v555 处理体的实测差异(见模块文档);0.495 与更早版本
    /// 一律 `Inert`。
    #[must_use]
    pub fn from_yscm_version(version: u32) -> Self {
        let varinfo_sjis = if version >= 555 {
            SjisCodeMode::FirstChar
        } else {
            SjisCodeMode::Inert
        };
        Self {
            ysc_version: version,
            varinfo_sjis,
        }
    }

    /// 由已装载的启动表派生。
    #[must_use]
    pub fn from_boot(boot: &crate::tables::BootTables) -> Self {
        Self::from_yscm_version(boot.yscm.version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 门控表:0.495 与更早版本必须是 `Inert`;v555 起才是 `FirstChar`。
    /// 加新版本门槛时在这里补断言,防止处理器侧再出现第二份版本比较。
    #[test]
    fn sjis_mode_follows_yscm_version() {
        for version in [450, 466, 474, 479, 480, 481, 486, 494, 495] {
            let p = SemanticProfile::from_yscm_version(version);
            assert_eq!(p.ysc_version, version);
            assert_eq!(p.varinfo_sjis, SjisCodeMode::Inert, "{version}");
        }
        for version in [555, 556, 600] {
            let p = SemanticProfile::from_yscm_version(version);
            assert_eq!(p.ysc_version, version);
            assert_eq!(p.varinfo_sjis, SjisCodeMode::FirstChar, "{version}");
        }
    }
}
