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
//!   `FirstChar` 只在 5xx 商业族(`500..=555`)生效。
//! - `TEXT` / `TEXTINFO` 按 [`SemanticProfile::text`] /
//!   [`SemanticProfile::textinfo`] 门控:v555 的
//!   `FileArray` / `Inert` 由 `crate::other::handlers::dispatch` 消费,
//!   共享 `text.rs` 不分支;其余版本保持 legacy 形态。
//!
//! **5xx 是一条版本区间(`500..=555`),不是两个点**(见
//! [`is_v555_family_version`]):0.500 / 0.554 / 0.555 共用 121 槽命令表与
//! 同族处理体,差异只在少数命令(如 0.500 的 `MOVIE2*`/`REGINFO` 是空桩)
//! 与缩略图块布局(仅 0.555 有样本)。区间外的 556/600 没有取证,按 legacy
//! 形态处理。

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

/// 存档缩略图块(btype 1)的描述符布局。
///
/// v555 与 0.495 共用容器版本 `0x1E0`,但官方描述符不同:
/// - 0.495 系:A=1600,w/h 在 +0x78;
/// - v555:A=1680,w/h 在 +0x88(官方 `<v555-kuro>` 存档 DNO=3 实测
///   `A=1680 B=55896`,B/4=137×102,A+0x88=137/102)。
///
/// 556/600 没有取证,继续按容器版本档案。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveImageMode {
    /// 按容器版本档案选(0.479=A1536,0.495/0.481=A1600)。
    ContainerProfile,
    /// v555:A=1680,+0x88。
    V555,
}

/// `RETURNCODE` 命令的语义形态。
///
/// 同名命令在两代引擎里行为不同,必须按 `ysc_version` 门控:
/// - 0.495 及更早(处理体 `0x44c390`):自动模式按 `AUTOCLICK/AUTORETURN/
///   AUTOPAGE` 追加 `'C'/'R'/'P'`,最后恒追加 `'E'`;手动模式只置清缓冲
///   请求 `+0x36` 再追加 `'E'`;
/// - v555(处理体 `0x44b920`/`0x44b9e4`/`0x44b824`,三族同型):只追加
///   `'E'`,并**无条件**置清缓冲请求 `state+0x15 = 1`,不追加 C/R/P。
///   逐字控制由旧代 stdlib 的 `es.TX.OUTPUT.LINEEND` 在脚本侧插入。
///
/// 555 是独立商业分支,精确匹配;556/600 没有取证,按 legacy 形态处理。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReturnCodeMode {
    /// 0.495 及更早。
    Legacy,
    /// v555:只写 `'E'` + 无条件清缓冲请求。
    EndOnly,
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
    /// `RETURNCODE` 形态。
    pub returncode: ReturnCodeMode,
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
    /// 存档缩略图块描述符布局。
    pub save_image: SaveImageMode,
    /// `SAVE[MEM=99999]` / `LOAD[FILE DNO=1024]` 的 500 槽内存包形态。
    ///
    /// 证据覆盖四个带 `MEM` 的构建:0.481(`0x447406`)、0.494
    /// (`0x4472c0`)、0.500(`0x474feb`)与 v555(`0x4447c8:346`)的
    /// LOAD 文件分支都有 `DNO == 0x400`;0.495 SDK 表没有 `MEM`,其
    /// LOAD 处理体全段无 0x400 分支,1024 仍是普通块。由
    /// [`Self::from_boot`] 按命令表能力派生,不能只看版本号。
    pub save_mem_pack: bool,
}

impl SemanticProfile {
    /// 由工程命令表版本派生。
    ///
    /// 版本阈值来自 v555 处理体的实测差异(见模块文档);0.495 与更早版本
    /// 一律 `Inert` / `Runtime`。注意 [`Self::save_mem_pack`] 在无命令表时
    /// 只能按 v555 保守置位,真实工程必须用 [`Self::from_boot`] 按 `MEM`
    /// 参数能力覆盖(0.481/0.494/0.500 商业表也带 `MEM`)。
    #[must_use]
    pub fn from_yscm_version(version: u32) -> Self {
        // 5xx 是一条**版本区间**,不是两个点:区间内的构建共用同一套 121 槽
        // 命令表与同族处理体,只有缩略图块布局需要精确 555 的样本
        // (`save_image`,见该字段赋值处)。区间外的 556/600 没有取证,
        // 不得继承 555 形态。
        //
        // 0.500 归入同一商业族(2026-11-24 `<v500-relirium>` 取证):
        // - `ysc.ybn` = 121 槽、与 v555 仅版本字段与 `GRADATION` kind 不同;
        // - 处理体 `TEXT`/`CGINFO`/`VARINFO`/`RETURNCODE`/`GOSUB`/`RETURN`/
        //   `LET`/`VARACT`/`SAVE`/`LOAD` 归一化反编译与 v555 逐行一致;
        // - 0.500 分发表默认体/空操作体名单与 v555 清单逐名一致。
        // 少数差异另由分发表覆盖(`MOVIE2*`/`REGINFO` 在 0.500 是 29B
        // 空桩)。
        //
        // 0.554 的取证(2026-11-25 `<game-554>`):`ysc.ybn` 与 0.555 样本
        // **逐字节相同,仅 +0x04 版本字段差 1**(11102B / 差 1 字节),
        // `yse.ybn` 同样(6645B / 差 1 字节);`TEXT`=FILE/LET/CLEAR、
        // `TEXTINFO` argc=0、`VARINFO` 带 `SJISCODE`/`STRFIRST`。
        // 此前按字面量只认 {500,555},554 静默落回 legacy 形态,
        // 表现为 `CGINFO[LINT2]` 整帧未实现刷屏(按钮号回读被门控关掉)。
        let v555 = is_v555_family_version(version);
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
            returncode: if v555 {
                ReturnCodeMode::EndOnly
            } else {
                ReturnCodeMode::Legacy
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
            // 缩略图块只对**精确 555** 取证(A=1680/+0x88)。0.500 官方
            // 存档(`<v500-relirium>/save/*.sd`,195 块)没有 DNO=3 缩略图块,
            // 也没有 btype=1 配置块;写侧若按 1680 描述符落盘会与官方产物
            // 不同形 ⇒ 0.500 保持容器档案,等有 0.500 DNO=3 样本再裁决。
            save_image: if version == 555 {
                SaveImageMode::V555
            } else {
                SaveImageMode::ContainerProfile
            },
            save_mem_pack: v555,
        }
    }

    /// 由已装载的启动表派生。
    ///
    /// `save_mem_pack` 不能只看版本号:0.481/0.494/0.500/0.555 商业表的
    /// `SAVE/LOAD` 带 `MEM` 参数,0.495 SDK 表没有。四个带 `MEM` 的构建
    /// 都已反汇编确认 `LOAD` 文件分支有 `DNO == 0x400` 的 500 槽读取、
    /// `SAVE` 处理体在 MEM=99999 打包并检查重复 DNO;因此按**命令表能力**
    /// 判定,不按版本字面量。
    #[must_use]
    pub fn from_boot(boot: &crate::tables::BootTables) -> Self {
        let mut profile = Self::from_yscm_version(boot.yscm.version);
        // 真实工程 SAVE/LOAD 两表同时带 MEM;夹具/裁剪表只补一侧也认。
        let has_mem = ["SAVE", "LOAD"].iter().any(|name| {
            boot.yscm
                .index_of(name)
                .and_then(|i| boot.yscm.commands.get(i))
                .is_some_and(|c| c.params.iter().any(|p| p.name == "MEM"))
        });
        profile.save_mem_pack = has_mem;
        profile
    }

    /// 555 独立商业分支(字面匹配;556/600 不继承)。
    ///
    /// 只用于**尚未在 0.500 取证**的 555 专属分支(如 0.500 的静态
    /// 执行编码表);脚本语义/分发表/独立运行面请用
    /// [`Self::is_v555_family`]。
    #[must_use]
    pub fn is_v555(&self) -> bool {
        self.ysc_version == 555
    }

    /// 555 商业族(`500..=555`):121 槽命令表与同族处理体。
    ///
    /// 0.500 / 0.554 的证据与差异清单见 [`Self::from_yscm_version`]。
    #[must_use]
    pub fn is_v555_family(&self) -> bool {
        is_v555_family_version(self.ysc_version)
    }
}

/// 5xx 商业族版本区间(`500..=555`,含两端)。
///
/// 区间内的构建共用同一套 121 槽命令表与同族处理体;版本号是构建戳,
/// 不是语义分叉点——0.554 的 `ysc.ybn`/`yse.ybn` 与 0.555 样本逐字节
/// 相同(仅版本字段差 1)。区间外(≤495 / ≥556)按 legacy 形态处理,
/// 因为 556+ 无样本;精确 555 判据另见 [`SemanticProfile::is_v555`]。
#[must_use]
pub fn is_v555_family_version(version: u32) -> bool {
    (500..=555).contains(&version)
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
            assert_eq!(p.returncode, ReturnCodeMode::Legacy, "{version}");
            assert_eq!(p.varinfo_sjis, SjisCodeMode::Inert, "{version}");
            assert_eq!(p.script_decls, ScriptDeclMode::Runtime, "{version}");
            assert_eq!(p.ref_int_param, RefIntParamMode::Value, "{version}");
            assert_eq!(p.save_image, SaveImageMode::ContainerProfile, "{version}");
            assert!(!p.save_mem_pack, "{version}");
        }
        let p = SemanticProfile::from_yscm_version(555);
        assert_eq!(p.ysc_version, 555);
        assert_eq!(p.window_payload, WindowPayloadMode::RecordDelimited);
        assert_eq!(p.text, TextMode::FileArray);
        assert_eq!(p.textinfo, TextInfoMode::Inert);
        assert_eq!(p.returncode, ReturnCodeMode::EndOnly);
        assert_eq!(p.varinfo_sjis, SjisCodeMode::FirstChar);
        assert_eq!(p.script_decls, ScriptDeclMode::LoadTime);
        assert_eq!(p.ref_int_param, RefIntParamMode::SlotId);
        assert_eq!(p.save_image, SaveImageMode::V555);
        assert!(p.save_mem_pack);

        // 0.500 是 555 商业族:命令表/处理体同族;缩略图块没有 0.500 样本,
        // 保持 `ContainerProfile`(见 `from_yscm_version` 的字段注释)。
        let p = SemanticProfile::from_yscm_version(500);
        assert_eq!(p.ysc_version, 500);
        assert_eq!(p.window_payload, WindowPayloadMode::RecordDelimited);
        assert_eq!(p.text, TextMode::FileArray);
        assert_eq!(p.textinfo, TextInfoMode::Inert);
        assert_eq!(p.returncode, ReturnCodeMode::EndOnly);
        assert_eq!(p.varinfo_sjis, SjisCodeMode::FirstChar);
        assert_eq!(p.script_decls, ScriptDeclMode::LoadTime);
        assert_eq!(p.ref_int_param, RefIntParamMode::SlotId);
        assert!(p.partial_index);
        assert_eq!(p.save_image, SaveImageMode::ContainerProfile);
        assert!(p.save_mem_pack);
        assert!(!p.is_v555(), "精确 555 判据不得吞并 0.500");
        assert!(p.is_v555_family());

        // 0.554 是 5xx 区间里的构建:`ysc.ybn`/`yse.ybn` 与 0.555 样本逐字节
        // 相同(仅版本字段差 1),全部走 555 家族形态;缩略图块只对精确 555
        // 取证,554 保持 `ContainerProfile`。
        for version in [501, 530, 554] {
            let p = SemanticProfile::from_yscm_version(version);
            assert_eq!(p.ysc_version, version);
            assert_eq!(p.window_payload, WindowPayloadMode::RecordDelimited, "{version}");
            assert_eq!(p.text, TextMode::FileArray, "{version}");
            assert_eq!(p.textinfo, TextInfoMode::Inert, "{version}");
            assert_eq!(p.returncode, ReturnCodeMode::EndOnly, "{version}");
            assert_eq!(p.varinfo_sjis, SjisCodeMode::FirstChar, "{version}");
            assert_eq!(p.script_decls, ScriptDeclMode::LoadTime, "{version}");
            assert_eq!(p.ref_int_param, RefIntParamMode::SlotId, "{version}");
            assert!(p.partial_index, "{version}");
            assert_eq!(p.save_image, SaveImageMode::ContainerProfile, "{version}");
            assert!(p.save_mem_pack, "{version}");
            assert!(!p.is_v555(), "{version}:精确 555 判据不得吞并 554");
            assert!(p.is_v555_family(), "{version}");
        }

        // 555 是独立分支;556/600 没有证据,不得自动继承。
        for version in [556, 600] {
            assert!(!is_v555_family_version(version), "{version}");
            let p = SemanticProfile::from_yscm_version(version);
            assert_eq!(p.ysc_version, version);
            assert_eq!(p.window_payload, WindowPayloadMode::Exact, "{version}");
            assert_eq!(p.text, TextMode::Attributes, "{version}");
            assert_eq!(p.textinfo, TextInfoMode::Query, "{version}");
            assert_eq!(p.returncode, ReturnCodeMode::Legacy, "{version}");
            assert_eq!(p.varinfo_sjis, SjisCodeMode::Inert, "{version}");
            assert_eq!(p.script_decls, ScriptDeclMode::Runtime, "{version}");
            assert_eq!(p.ref_int_param, RefIntParamMode::Value, "{version}");
            assert_eq!(p.save_image, SaveImageMode::ContainerProfile, "{version}");
            assert!(!p.save_mem_pack, "{version}");
        }
    }
}
