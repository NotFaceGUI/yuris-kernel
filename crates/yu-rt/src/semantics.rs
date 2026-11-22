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
//! - `VARINFO[SJISCODE]` 按 [`SemanticProfile::varinfo_sjis`] 门控:
//!   `FirstChar` 只在 `ysc_version == 555` 时生效。
//! - `TEXT` / `TEXTINFO` 按 [`SemanticProfile::text`] /
//!   [`SemanticProfile::textinfo`] 门控:v555 的
//!   `FileArray` / `Inert` 由 `crate::other::handlers::dispatch` 消费,
//!   共享 `text.rs` 不分支;其余版本保持 legacy 形态。
//!
//! 555 是独立商业分支,精确匹配;556/600 没有取证,按 legacy 形态处理。

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

/// 脚本局部声明(`INT`/`FLT`/`STR`)的生效时机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptDeclMode {
    /// 按执行流在命令点声明(0.495 及更早的既有行为)。
    Runtime,
    /// 脚本装载时先处理入口标签之前的声明(v555 商业作:
    /// `es_scenario_macro_変数n.yst` pc0/1 声明 `@0x0c60/61`,入口在 pc2,
    /// GOSUB 跳过声明导致 `@3168` 未声明 Fatal)。
    LoadTime,
}

/// 整数参数窗口是**变量引用**时的传参方式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefIntParamMode {
    /// 按值求值(0.495 及更早:`PINT1=$array[0]` 传元素值)。
    Value,
    /// 传变量**槽号**(v555 剧本宏链):
    /// `es_scenario_parser.yst` 用
    /// `GOSUB es.ScenarioMacroExec PINT2=$0x0b63[0]` 把宏参数数组的
    /// 槽号传给 `es_scenario_macro.yst`;被调方 `_STR(_PINT(2), i)`
    /// 再用该槽号读取参数。若按值传 "666",`_STR` 会去访问槽 666。
    ///
    /// 证据:官方引擎启动 hook,`PRODUCT[COM=22]` 的 `PINT2/PINT3`
    /// 实测为 2918/2919(STR[1024]/INT[1024] 的槽号);
    /// 样本 3 `es.ScenarioMacroExec` 的 `_STR[_PINT(2), 0]` 能取到
    /// `$0x0b63` 的首元素 "666"。
    SlotId,
}

/// `TEXT` 命令族的语义形态。
///
/// 同名命令在两代引擎里含义不同,必须按 `ysc_version` 门控,不能看
/// 参数表里有没有 `FILE`:
/// - 0.495 及更早:`TEXT` 是文本属性设置(处理体 `0x453448`),命令表
///   没有 `FILE/LET` 槽;
/// - v555:`TEXT` 是场景文本装载(处理体 `0x452a0c`),参数表 =
///   `FILE/LET/CLEAR`,与 0.495 的属性设置**不是同一语义**。
///
/// 556/600 没有证据,不得继承 v555 形态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextMode {
    /// 文本属性设置(0.495 及更早)。
    Attributes,
    /// 场景文本文件 → 字符串数组(v555)。
    FileArray,
}

/// `TEXTINFO` 命令族的语义形态。
///
/// - 0.495 及更早:10 参数查询,`LET` 是必需目标(缺 LET 报 `CMD_SLOT`);
/// - v555:参数表为 0,处理体 `0x452d54` 只装载参数后 `return 0`,
///   因此任何调用都是 no-op,不存在「缺 LET」。
///
/// 556/600 没有证据,不得继承 v555 形态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextInfoMode {
    /// 旧版属性查询。
    Query,
    /// v555 空处理体。
    Inert,
}

/// 表达式窗口载荷的取用方式(窗口记录 `+4 length` 与字节码记录的边界关系)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowPayloadMode {
    /// 严格按 12 字节窗口记录的 `length` 切片(0.495 及更早)。
    Exact,
    /// v555:窗口 `length` 只作交叉校验;某条表达式记录**自声明**的长度
    /// 越过窗口时,把这**一条**记录按自身边界补读完(上限 = 共享池内下一个
    /// 窗口的 offset / 池尾),并留下 verbose 分歧记录。
    ///
    /// 证据(<v555-sample>,2026-11-22):`data\script\userdefine\` 的
    /// 汉化脚本 `s273 pc13` 窗口 `offset=1035 length=11`,但窗口内是
    /// `4d 0c 00 …` —— 字符串记录自声明 12 字节负载,整条 15 字节。
    /// 日文原名 3 字(CP932 6 字节,整条 11 字节)与旧 length 吻合;
    /// 汉化名 5 字(GBK 10 字节)只更新了记录头,没有更新窗口记录。
    /// 发行版数据即此形态 ⇒ 官方读取不把过期 length 当硬边界。
    /// 旧版没有该形态证据,保持 `Exact`。
    RecordDelimited,
}

/// 本工程已装载表的语义形态(启动时派生一次)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SemanticProfile {
    /// 工程 `ysbin/ysc.ybn` 头的目标引擎版本(如实记录,不做换算)。
    ///
    /// 这是**门控输入**,不是展示字段;处理器不得再直接比较
    /// `tables.boot.yscm.version`。
    pub ysc_version: u32,
    /// 表达式窗口载荷的取用方式。
    pub window_payload: WindowPayloadMode,
    /// `TEXT` 命令族形态。
    pub text: TextMode,
    /// `TEXTINFO` 命令族形态。
    pub textinfo: TextInfoMode,
    /// `VARINFO[SJISCODE]` 形态。
    pub varinfo_sjis: SjisCodeMode,
    /// 脚本局部声明的生效时机。
    pub script_decls: ScriptDeclMode,
    /// 整数参数窗口是变量引用时的传参方式。
    pub ref_int_param: RefIntParamMode,
    /// 多维数组允许用**较少下标**访问(缺省 rank = 提供下标数)。
    ///
    /// v555 商业剧本把 `[256,256]` 的 `@1177` 当一维表用
    /// (`@1177[129]` 读写语言号);官方线性化循环的 rank 取自
    /// “已收下标个数”,因此 `@1177[129]` 落在 flat=129。
    pub partial_index: bool,
}

impl SemanticProfile {
    /// 由工程命令表版本派生。
    ///
    /// 版本阈值来自 v555 处理体的实测差异(见模块文档);0.495 与更早版本
    /// 一律 `Inert` / `Runtime`。
    #[must_use]
    pub fn from_yscm_version(version: u32) -> Self {
        // 555 是独立商业分支;556/600 没有取证,不得继承 555 形态。
        // (2026-11-21 全量反编译:6 个 555 构建的 121 槽分发表一致,
        //  但处理体语义分三族;更高版本无样本。)
        let v555 = version == 555;
        Self {
            ysc_version: version,
            window_payload: if v555 {
                WindowPayloadMode::RecordDelimited
            } else {
                WindowPayloadMode::Exact
            },
            text: if v555 {
                TextMode::FileArray
            } else {
                TextMode::Attributes
            },
            textinfo: if v555 {
                TextInfoMode::Inert
            } else {
                TextInfoMode::Query
            },
            varinfo_sjis: if v555 {
                SjisCodeMode::FirstChar
            } else {
                SjisCodeMode::Inert
            },
            script_decls: if v555 {
                ScriptDeclMode::LoadTime
            } else {
                ScriptDeclMode::Runtime
            },
            ref_int_param: if v555 {
                RefIntParamMode::SlotId
            } else {
                RefIntParamMode::Value
            },
            partial_index: v555,
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
            assert_eq!(p.window_payload, WindowPayloadMode::Exact, "{version}");
            assert_eq!(p.text, TextMode::Attributes, "{version}");
            assert_eq!(p.textinfo, TextInfoMode::Query, "{version}");
            assert_eq!(p.varinfo_sjis, SjisCodeMode::Inert, "{version}");
            assert_eq!(p.script_decls, ScriptDeclMode::Runtime, "{version}");
            assert_eq!(p.ref_int_param, RefIntParamMode::Value, "{version}");
        }
        let p = SemanticProfile::from_yscm_version(555);
        assert_eq!(p.ysc_version, 555);
        assert_eq!(p.window_payload, WindowPayloadMode::RecordDelimited);
        assert_eq!(p.text, TextMode::FileArray);
        assert_eq!(p.textinfo, TextInfoMode::Inert);
        assert_eq!(p.varinfo_sjis, SjisCodeMode::FirstChar);
        assert_eq!(p.script_decls, ScriptDeclMode::LoadTime);
        assert_eq!(p.ref_int_param, RefIntParamMode::SlotId);

        // 555 是独立分支;556/600 没有证据,不得自动继承。
        for version in [556, 600] {
            let p = SemanticProfile::from_yscm_version(version);
            assert_eq!(p.ysc_version, version);
            assert_eq!(p.window_payload, WindowPayloadMode::Exact, "{version}");
            assert_eq!(p.text, TextMode::Attributes, "{version}");
            assert_eq!(p.textinfo, TextInfoMode::Query, "{version}");
            assert_eq!(p.varinfo_sjis, SjisCodeMode::Inert, "{version}");
            assert_eq!(p.script_decls, ScriptDeclMode::Runtime, "{version}");
            assert_eq!(p.ref_int_param, RefIntParamMode::Value, "{version}");
        }
    }
}
