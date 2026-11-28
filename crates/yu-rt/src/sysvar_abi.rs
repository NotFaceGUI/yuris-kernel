//! 系统变量 ABI 表(名字/类型/维数)与装载校验。
//!
//! # 为什么需要两条来源
//!
//! 商业发行版**不带编译器**:实测 <game-b> `data.ypf`(12,212 条目)里
//! `ycd` / `yscom` 命中数为 **0**,但 7 张 `.ybn` 元数据表齐全。因此
//! `YSCom.ycd` 只能作为**可选**来源,不能作为启动前提(A5 的"缺表拒启"
//! 只适用于游戏必须自带的表)。
//!
//! 系统变量 id 空间是**编译器与引擎之间的 ABI**:编译产物里存的是 id
//! (如 `_LC`=48、`_PINT`=53),所以 id↔语义在给定引擎版本内是固定的,
//! 与具体游戏无关。实测证据:
//!
//! - 0.495 SDK `YSCom.ycd` 的系统变量段:548 项,`_TIME`=0、`_LC`=48;
//! - 0.494 商业包 `ysv.ybn` 的前 548 条 `kind=1` 记录:`var_id` 恰为 0..547,
//!   且 **(value_type, dims) 与上面的 ycd 逐项一致(548/548,0 差异)**。
//!
//! # 装载顺序
//!
//! 1. 工程带 `system/YSCom/YSCom.ycd` → 直接用它(开发工程/SDK);
//! 2. 否则用 `data/sysvar-abi-<range>.tsv` 里的 ABI 表,按游戏自己
//!    `ysv.ybn` 的**前缀指纹**选表(见 [`SysVarTable::select_for`]);
//! 3. 前缀全部不符 → `table.version` Fatal,列出逐族失配原因。
//!
//! 表来源:`YU-RIS_Guide`(官方 SDK 全量归档)的 `YSCom.ycd`,每张表头
//! 记录 `source_ycd_sha256` 与 `# covers` 版本区间。同一 sha 覆盖多个
//! 版本时只保留一份(如 `f7380aba` 覆盖 0.481–0.493)。
//!
//! # 为什么按指纹选表,而不是按版本号
//!
//! 官方履历与 SDK 实测:系统变量表**在小版本里也会增长**(如 ver0.480/0.01
//! 一次性追加 `_PINT/_PFLT/_PSTR/_PFLG 11..16` 等),插入后所有后续 id 平移;
//! 官方不归档小版本 SDK。因此版本号不可用于选表,唯一可靠判据是游戏
//! 自带 `ysv` 的 (id, type, dims) 前缀指纹。
//!
//! 前缀之后的追加项不影响已有 id 语义(官方 `switch` 的 `default` 分支
//! 就是普通槽位),因此只校验前缀,不要求整表相等。
//!
use yu_log::Boundary;

use crate::code;
use crate::diag;
use crate::tables::VarImage;

/// 一条系统变量声明。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SysVarDecl {
    pub name: String,
    /// 1=INT / 2=FLT / 3=STR。
    pub value_type: u8,
    pub dims: Vec<u32>,
}

/// 表来源(进 trace/golden 头,可审计)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SysVarSource {
    /// 工程自带编译器定义。
    Ycd { path: String, sha256: String },
    /// 引擎 ABI 数据表(内嵌,随 crate 分发)。
    AbiProfile { name: &'static str, sha256: String },
}

impl SysVarSource {
    pub fn describe(&self) -> String {
        match self {
            SysVarSource::Ycd { path, sha256 } => {
                format!("project YSCom.ycd ({path}, sha256 {sha256})")
            }
            SysVarSource::AbiProfile { name, sha256 } => {
                format!("engine ABI profile {name} (ycd sha256 {sha256})")
            }
        }
    }
}

/// 系统变量表(顺序即 id)。
#[derive(Clone, Debug)]
pub struct SysVarTable {
    pub decls: Vec<SysVarDecl>,
    pub source: SysVarSource,
    /// 该表覆盖的引擎版本区间(来自表头 `# covers`;空 = 未标注)。
    pub covers: String,
    /// 派生表的来源:工程 `ysv` 系统变量前缀的规范化 SHA-256 指纹
    /// (官方 ycd 表为 `None`)。
    pub source_ysv_sha256: Option<String>,
    /// 派生说明(基线族 + 对齐统计;官方 ycd 表为 `None`)。
    pub derivation: Option<String>,
}

const ABI_TSV_0255: &str = include_str!("../data/sysvar-abi-0255-81a05c0c.tsv");
const ABI_TSV_0292: &str = include_str!("../data/sysvar-abi-0292-e7016b1e.tsv");
const ABI_TSV_0450: &str = include_str!("../data/sysvar-abi-0450-ad2025bf.tsv");
const ABI_TSV_0451: &str = include_str!("../data/sysvar-abi-0451-2caf6952.tsv");
const ABI_TSV_0452: &str = include_str!("../data/sysvar-abi-0452-02bb60dc.tsv");
const ABI_TSV_0453: &str = include_str!("../data/sysvar-abi-0453-0454-5e57c9ca.tsv");
const ABI_TSV_0455: &str = include_str!("../data/sysvar-abi-0455-0459-1ef473cf.tsv");
const ABI_TSV_0460: &str = include_str!("../data/sysvar-abi-0460-0461-f6f00119.tsv");
const ABI_TSV_0462: &str = include_str!("../data/sysvar-abi-0462-e96801fb.tsv");
const ABI_TSV_0463: &str = include_str!("../data/sysvar-abi-0463-13789d04.tsv");
const ABI_TSV_0464: &str = include_str!("../data/sysvar-abi-0464-190ac8df.tsv");
const ABI_TSV_0465: &str = include_str!("../data/sysvar-abi-0465-fbaf84e2.tsv");
const ABI_TSV_0466: &str = include_str!("../data/sysvar-abi-0466-79b8669d.tsv");
const ABI_TSV_0470: &str = include_str!("../data/sysvar-abi-0470-aab15df0.tsv");
const ABI_TSV_0471: &str = include_str!("../data/sysvar-abi-0471-91a558a9.tsv");
const ABI_TSV_0472: &str = include_str!("../data/sysvar-abi-0472-0ef15e64.tsv");
const ABI_TSV_0473: &str = include_str!("../data/sysvar-abi-0473-2f9d3d28.tsv");
const ABI_TSV_0474: &str = include_str!("../data/sysvar-abi-0474-ba66d7ec.tsv");
const ABI_TSV_0475: &str = include_str!("../data/sysvar-abi-0475-35cc725d.tsv");
const ABI_TSV_0476: &str = include_str!("../data/sysvar-abi-0476-13a7f1f1.tsv");
const ABI_TSV_0477: &str = include_str!("../data/sysvar-abi-0477-98a3f7c7.tsv");
const ABI_TSV_0478: &str = include_str!("../data/sysvar-abi-0478-563b7c6c.tsv");
const ABI_TSV_0479: &str = include_str!("../data/sysvar-abi-0479-22755ecd.tsv");
const ABI_TSV_0480: &str = include_str!("../data/sysvar-abi-0480-23251f47.tsv");
const ABI_TSV_0481: &str = include_str!("../data/sysvar-abi-0481-0493-f7380aba.tsv");
const ABI_TSV_0494: &str = include_str!("../data/sysvar-abi-0494-185f14aa.tsv");
const ABI_TSV_0495: &str = include_str!("../data/sysvar-abi-0495-a626e40a.tsv");

/// 结构对齐统计(派生 ABI 表用;进日志/诊断)。
#[derive(Clone, Copy, Debug, Default)]
struct AlignStats {
    matches: usize,
    substitutions: usize,
    insertions: usize,
    deletions: usize,
    cost: u32,
    shift: u64,
}

impl AlignStats {
    fn summary(self) -> String {
        format!(
            "matches={} subs={} inserts={} deletes={} cost={} shift={}",
            self.matches, self.substitutions, self.insertions, self.deletions, self.cost, self.shift
        )
    }
}

/// 工程 `ysv` 的系统变量前缀:从 id 0 起连续存在且 `ty != 0` 的 kind=1
/// 记录。编译器把系统变量排在用户全局变量之前;第一个 `ty == 0`(仅声明
/// 无初值)是用户变量区起点,不参与 ABI 对齐。
fn project_prefix(vars: &VarImage) -> Vec<(u8, Vec<u32>)> {
    let mut by_id: std::collections::BTreeMap<u16, (u8, Vec<u32>)> = std::collections::BTreeMap::new();
    for rec in vars.records.iter().filter(|r| r.kind == 1) {
        by_id
            .entry(rec.var_id)
            .or_insert_with(|| (rec.value_type, rec.dims.clone()));
    }
    let mut out = Vec::new();
    for id in 0u32.. {
        let Ok(id16) = u16::try_from(id) else { break };
        let Some((ty, dims)) = by_id.get(&id16) else { break };
        if *ty == 0 {
            break;
        }
        out.push((*ty, dims.clone()));
    }
    out
}

/// Needleman-Wunsch 对齐 `(type,dims)` 序列。
///
/// 代价:精确匹配 0、同 type 改 dims 1、跨 type 5、插入/删除 2;
/// 同代价时优先总位移最小
/// (`|i-j|` 之和),从而把 0.263 这类"删除 32 个按钮数组但保留方向槽"
/// 的块变化对齐到方向槽,而不是块尾的按钮槽。
fn align_sysvars(
    seq: &[(u8, Vec<u32>)],
    base: &[SysVarDecl],
) -> (Vec<Option<usize>>, AlignStats) {
    const GAP: u32 = 2;
    const SUB: u32 = 1;
    const INF_COST: u32 = u32::MAX / 4;
    const INF_SHIFT: u64 = u64::MAX / 4;
    let n = seq.len();
    let m = base.len();
    let width = m + 1;
    let idx = |i: usize, j: usize| i * width + j;
    let mut dp = vec![(INF_COST, INF_SHIFT); (n + 1) * width];
    let mut bt = vec![0u8; (n + 1) * width];
    dp[idx(0, 0)] = (0, 0);
    for i in 1..=n {
        dp[idx(i, 0)] = (i as u32 * GAP, 0);
        bt[idx(i, 0)] = 1;
    }
    // 系统变量表从 id 0 开始:不允许"跳过基线开头"的对齐,否则
    // 局部改型会把整段前缀平移到后面的同型槽位。工程侧在开头多一个
    // 新槽仍可通过 `dp[i][0]` 的插入路径表达。
    for j in 1..=m {
        dp[idx(0, j)] = (INF_COST, INF_SHIFT);
        bt[idx(0, j)] = 2;
    }
    for i in 1..=n {
        let (ty, dims) = &seq[i - 1];
        for j in 1..=m {
            let decl = &base[j - 1];
            let exact = *ty == decl.value_type && *dims == decl.dims;
            // 同槽改型:同 type 不同 dims(如 `_DROPFILE`)代价 1;跨 type
            // 更可能是"新槽插入 + 后续后移"。插入本身 2,还会让固定基线
            // 前缀多一个删除(+2),所以跨 type 同槽代价取 5,让插入优先。
            let change = if exact {
                0
            } else if *ty == decl.value_type {
                SUB
            } else {
                5
            };
            let prev = dp[idx(i - 1, j - 1)];
            let mut best = (
                prev.0.saturating_add(change),
                prev.1.saturating_add((i as i64 - j as i64).unsigned_abs()),
            );
            let mut op = 0u8;
            let skip_seq = dp[idx(i - 1, j)];
            let cand = (skip_seq.0.saturating_add(GAP), skip_seq.1);
            if cand < best {
                best = cand;
                op = 1;
            }
            let skip_base = dp[idx(i, j - 1)];
            let cand = (skip_base.0.saturating_add(GAP), skip_base.1);
            if cand < best {
                best = cand;
                op = 2;
            }
            dp[idx(i, j)] = best;
            bt[idx(i, j)] = op;
        }
    }
    let mut mapping = vec![None; n];
    let mut stats = AlignStats {
        cost: dp[idx(n, m)].0,
        shift: dp[idx(n, m)].1,
        ..AlignStats::default()
    };
    let (mut i, mut j) = (n, m);
    while i > 0 || j > 0 {
        match bt[idx(i, j)] {
            0 => {
                let (ty, dims) = &seq[i - 1];
                let decl = &base[j - 1];
                mapping[i - 1] = Some(j - 1);
                if *ty == decl.value_type && *dims == decl.dims {
                    stats.matches += 1;
                } else {
                    stats.substitutions += 1;
                }
                i -= 1;
                j -= 1;
            }
            1 => {
                stats.insertions += 1;
                i -= 1;
            }
            _ => {
                stats.deletions += 1;
                j -= 1;
            }
        }
    }
    (mapping, stats)
}

impl SysVarTable {
    /// 从工程 `YSCom.ycd` 的解析结果构造。
    pub fn from_ycd(
        ycd: &crate::ycd::YcdTable,
        path: impl Into<String>,
        sha256: impl Into<String>,
    ) -> Self {
        Self {
            decls: ycd
                .system_variables
                .iter()
                .map(|d| SysVarDecl {
                    name: d.name.clone(),
                    value_type: d.value_type,
                    dims: d.dims.clone(),
                })
                .collect(),
            source: SysVarSource::Ycd {
                path: path.into(),
                sha256: sha256.into(),
            },
            covers: String::new(),
            source_ysv_sha256: None,
            derivation: None,
        }
    }

    /// 内嵌 0.495 引擎 ABI 数据表(兼容入口;装载路径用
    /// [`Self::select_for`] 按指纹选族)。
    pub fn builtin_abi() -> Self {
        Self::from_tsv(ABI_TSV_0495, "sysvar-abi-0495.tsv")
    }

    /// 已知 ABI 族(选择优先级:**新→旧**)。
    ///
    /// 新表在前:若某 5xx 构建只在 548 前缀之后追加变量,其指纹会同时
    /// 通过 0495 族(只看 `var_id < 表长` 的镜像),应当归入 0495。
    /// 表由 `data/sysvar-abi-<ver>[-<sha8>].tsv` 提供,全部来自
    /// `YU-RIS_Guide` 全量 SDK 的 `YSCom.ycd`。没有官方 ycd 的商业构建
    /// **不另建表**:`select_for` 用工程 `ysv` 与这些官方族做结构对齐,
    /// 派生 names/type/dims,见 [`Self::select_for`] 与 DIV-RT-017 §6。
    pub fn known_profiles() -> Vec<Self> {
        vec![
            Self::from_tsv(ABI_TSV_0495, "sysvar-abi-0495-a626e40a.tsv"),
            Self::from_tsv(ABI_TSV_0494, "sysvar-abi-0494-185f14aa.tsv"),
            Self::from_tsv(ABI_TSV_0481, "sysvar-abi-0481-0493-f7380aba.tsv"),
            Self::from_tsv(ABI_TSV_0480, "sysvar-abi-0480-23251f47.tsv"),
            Self::from_tsv(ABI_TSV_0479, "sysvar-abi-0479-22755ecd.tsv"),
            Self::from_tsv(ABI_TSV_0478, "sysvar-abi-0478-563b7c6c.tsv"),
            Self::from_tsv(ABI_TSV_0477, "sysvar-abi-0477-98a3f7c7.tsv"),
            Self::from_tsv(ABI_TSV_0476, "sysvar-abi-0476-13a7f1f1.tsv"),
            Self::from_tsv(ABI_TSV_0475, "sysvar-abi-0475-35cc725d.tsv"),
            Self::from_tsv(ABI_TSV_0474, "sysvar-abi-0474-ba66d7ec.tsv"),
            Self::from_tsv(ABI_TSV_0473, "sysvar-abi-0473-2f9d3d28.tsv"),
            Self::from_tsv(ABI_TSV_0472, "sysvar-abi-0472-0ef15e64.tsv"),
            Self::from_tsv(ABI_TSV_0471, "sysvar-abi-0471-91a558a9.tsv"),
            Self::from_tsv(ABI_TSV_0470, "sysvar-abi-0470-aab15df0.tsv"),
            Self::from_tsv(ABI_TSV_0466, "sysvar-abi-0466-79b8669d.tsv"),
            Self::from_tsv(ABI_TSV_0465, "sysvar-abi-0465-fbaf84e2.tsv"),
            Self::from_tsv(ABI_TSV_0464, "sysvar-abi-0464-190ac8df.tsv"),
            Self::from_tsv(ABI_TSV_0463, "sysvar-abi-0463-13789d04.tsv"),
            Self::from_tsv(ABI_TSV_0462, "sysvar-abi-0462-e96801fb.tsv"),
            Self::from_tsv(ABI_TSV_0460, "sysvar-abi-0460-0461-f6f00119.tsv"),
            Self::from_tsv(ABI_TSV_0455, "sysvar-abi-0455-0459-1ef473cf.tsv"),
            Self::from_tsv(ABI_TSV_0453, "sysvar-abi-0453-0454-5e57c9ca.tsv"),
            Self::from_tsv(ABI_TSV_0452, "sysvar-abi-0452-02bb60dc.tsv"),
            Self::from_tsv(ABI_TSV_0451, "sysvar-abi-0451-2caf6952.tsv"),
            Self::from_tsv(ABI_TSV_0450, "sysvar-abi-0450-ad2025bf.tsv"),
            Self::from_tsv(ABI_TSV_0292, "sysvar-abi-0292-e7016b1e.tsv"),
            Self::from_tsv(ABI_TSV_0255, "sysvar-abi-0255-81a05c0c.tsv"),
        ]
    }

    /// **按游戏自带 `ysv.ybn` 的指纹选族**(数据驱动,不特化版本)。
    ///
    /// 三级裁决:
    ///
    /// 1. 已有 ABI 族前缀逐项全等 → 直接采用(可能带尾部追加项);
    /// 2. 没有全等表时,把工程 `ysv` 的系统变量前缀与每个已知 ABI 族做
    ///    **结构对齐**(匹配 `(type,dims)`,允许插入/删除/同槽改型),
    ///    取"版本距离最近且结构成立"的唯一候选,派生出该构建的表;
    /// 3. 结构对齐不成立 → `table.version` Fatal,列出逐族原因。
    ///
    /// 第 2 级是通用回退:商业构建的 ABI 可能在同一系列内插入/删除少量
    /// 系统变量(实测 0.263/0.290),不要求逐项全等,但要求对齐结果唯一、
    /// 同槽改型数不超过 4 个;名字继承最近官方族的语义槽位,新增槽位用
    /// `_SYSVAR_<id>`,不猜语义。见 DIV-RT-017 §6。
    pub fn select_for(vars: &VarImage) -> Result<Self, yu_log::Diag> {
        let profiles = Self::known_profiles();
        let prefix_len = project_prefix(vars).len();
        let mut failures = Vec::new();

        // 1. 前缀全等。
        let mut exact = Vec::new();
        for table in &profiles {
            match table.verify_against_ysvr(vars) {
                Ok(()) => {
                    if table.covers_version(vars.version) {
                        return Ok(table.clone());
                    }
                    exact.push(table.clone());
                }
                Err(d) => failures.push(format!(
                    "{}: {}",
                    table.source_name(),
                    d.detail.trim_start_matches("系统变量 ABI 校验失败:")
                )),
            }
        }
        if let Some(table) = exact.into_iter().next() {
            return Ok(table);
        }

        // 2. 结构对齐派生。评分 = (版本距离, 主代价, 位移代价);距离相同
        // 且代价相同视为不唯一,拒绝。
        let mut derived: Vec<(u32, u32, u64, usize, Self)> = Vec::new();
        for (idx, base) in profiles.iter().enumerate() {
            match Self::derive_from(base, vars) {
                Some((table, stats)) => {
                    let distance = base.version_distance(vars.version);
                    derived.push((distance, stats.cost, stats.shift, idx, table));
                }
                None => failures.push(format!(
                    "{}: 结构对齐不成立(前缀 {prefix_len} 项)",
                    base.source_name()
                )),
            }
        }
        derived.sort_by_key(|(d, c, s, idx, _)| (*d, *c, *s, *idx));
        if let Some((d, c, sh, _, table)) = derived.first() {
            let first = (*d, *c, *sh);
            let unique = derived
                .get(1)
                .is_none_or(|(d2, c2, s2, _, _)| (*d2, *c2, *s2) != first);
            if unique {
                return Ok(table.clone());
            }
            failures.push(format!(
                "结构对齐有多个同分候选({} 个),拒绝猜测",
                derived
                    .iter()
                    .filter(|(d2, c2, s2, _, _)| (*d2, *c2, *s2) == first)
                    .count()
            ));
        }

        // 3. 旧版"极少数缺项"兜底(仅 wrong==0 且唯一最接近)。
        let mut partial: Vec<(usize, Self)> = Vec::new();
        for table in &profiles {
            let (wrong, missing) = table.diff_counts(vars);
            if wrong == 0 && missing > 0 {
                partial.push((missing, table.clone()));
            }
        }
        const MAX_TOLERATED_MISSING: usize = 2;
        partial.sort_by_key(|(missing, _)| *missing);
        if let Some((missing, table)) = partial.first() {
            let unique = partial.get(1).is_none_or(|(m, _)| *m > *missing);
            if *missing <= MAX_TOLERATED_MISSING && unique {
                return Ok(table.clone());
            }
        }

        // 4. 失败闭合。
        Err(diag::fatal(
            code::table::VERSION,
            Boundary::Table,
            format!(
                "工程 ysv.ybn 的系统变量指纹与所有已知 ABI 族都不符,且结构对齐无可唯一判定的候选;需要该构建的 ABI 表(自带 YSCom.ycd 或从其引擎 exe 提取)。逐族失配:{}(A5:不猜)",
                failures.join(" | ")
            ),
        )
        .at_table("ysv.ybn"))
    }

    /// 给“`_PROJECT_PATH` 与 `_SYSTEM_VERSION` 之间的插入槽”按官方槽位命名。
    ///
    /// 证据:官方 0.450/0.455/0.460 全系在这两个锚点之间按
    /// `_SAVE_PATH → _APPDATA_PATH → _PERSONAL_PATH` 顺序排列;0.290
    /// 商业构建在这两个锚点之间插入了 1 个 STR,存档脚本直接读它
    /// (`es_saveload.yst` 的 `Scene_SAVE` / `Scene_QLOAD`)。按
    /// **锚点 + 类型 + 数量**继承官方名字,不做版本号判断;形状不符
    /// (中间还有已映射槽、非 STR、超过 3 个)就不命名,保留占位符。
    fn name_path_insertions(decls: &mut [SysVarDecl]) {
        let Some(project) = decls.iter().position(|d| d.name == "_PROJECT_PATH") else {
            return;
        };
        let Some(version) = decls
            .iter()
            .skip(project + 1)
            .position(|d| d.name == "_SYSTEM_VERSION")
            .map(|k| project + 1 + k)
        else {
            return;
        };
        if version <= project + 1 {
            return;
        }
        let inserted: Vec<usize> = (project + 1..version)
            .filter(|&i| decls[i].name.starts_with("_SYSVAR_"))
            .collect();
        // 中间必须全部是占位插入槽;有任何继承名说明不是连续插入块。
        if inserted.len() != version - project - 1 || inserted.len() > 3 {
            return;
        }
        if inserted.iter().any(|&i| decls[i].value_type != 3) {
            return;
        }
        for (k, &id) in inserted.iter().enumerate() {
            decls[id].name = ["_SAVE_PATH", "_APPDATA_PATH", "_PERSONAL_PATH"][k].to_string();
        }
    }

    /// 把工程 `ysv` 的系统变量前缀与一个官方 ABI 族做结构对齐,派生该
    /// 构建的表。
    ///
    /// 前缀 = 从 id 0 起连续存在且 `ty != 0` 的 `kind=1` 记录;第一个
    /// `ty == 0` 是用户全局变量区的起点(编译器把系统变量排在前面)。
    fn derive_from(base: &Self, vars: &VarImage) -> Option<(Self, AlignStats)> {
        let seq = project_prefix(vars);
        if seq.is_empty() {
            return None;
        }
        // 只对齐工程前缀附近的基线前缀:基线尾部未在工程声明,不是
        // "删除";保留 +64 余量给少量尾部插入。
        let limit = (seq.len() + 64).min(base.decls.len());
        let (mapping, stats) = align_sysvars(&seq, &base.decls[..limit]);
        // 同槽改型只允许极少数(如 `_DROPFILE` 维数变化);超过 4 个说明
        // 对齐到了错误的族。
        // 同槽改型只允许极少数(如 `_DROPFILE` 维数变化);插入/删除
        // 是块级差异(32 个按钮数组、尾部未声明),不按比例拒绝。
        if stats.substitutions > 4 {
            return None;
        }
        let mut decls = Vec::with_capacity(seq.len());
        for (id, (ty, dims)) in seq.iter().enumerate() {
            let name = match mapping.get(id).copied().flatten() {
                Some(j) => base.decls[j].name.clone(),
                None => format!("_SYSVAR_{id:04}"),
            };
            decls.push(SysVarDecl {
                name,
                value_type: *ty,
                dims: dims.clone(),
            });
        }
        Self::name_path_insertions(&mut decls);
        let mut fingerprint = Vec::new();
        for (ty, dims) in &seq {
            fingerprint.push(*ty);
            fingerprint.push(dims.len() as u8);
            for d in dims {
                fingerprint.extend_from_slice(&d.to_le_bytes());
            }
        }
        let table = Self {
            decls,
            source: base.source.clone(),
            covers: format!("{:04}", vars.version),
            source_ysv_sha256: Some(sha256_hex(&fingerprint)),
            derivation: Some(format!(
                "structurally aligned to {} ({})",
                base.source_name(),
                stats.summary()
            )),
        };
        // 派生表必须自洽:用它自己的 ysv 前缀再验一次。
        if table.verify_against_ysvr(vars).is_err() {
            return None;
        }
        Some((table, stats))
    }

    /// 该表声明的覆盖区间是否包含给定引擎版本(`# covers` 形如 `0481-0493`)。
    #[must_use]
    pub fn covers_version(&self, version: u32) -> bool {
        let Some(rest) = self.covers.split_once('-') else {
            return self.covers.parse::<u32>().is_ok_and(|v| v == version);
        };
        let (lo, hi) = rest;
        match (lo.trim().parse::<u32>(), hi.trim().parse::<u32>()) {
            (Ok(lo), Ok(hi)) => (lo..=hi).contains(&version),
            _ => false,
        }
    }

    /// 工程版本到本表覆盖区间的距离(0 = 区间内;区间外取最近端点距离)。
    fn version_distance(&self, version: u32) -> u32 {
        if self.covers.is_empty() {
            return u32::MAX / 4;
        }
        if let Some((lo, hi)) = self.covers.split_once('-') {
            let (Ok(lo), Ok(hi)) = (lo.trim().parse::<u32>(), hi.trim().parse::<u32>()) else {
                return u32::MAX / 4;
            };
            if (lo..=hi).contains(&version) {
                0
            } else {
                version.saturating_sub(hi).max(lo.saturating_sub(version))
            }
        } else {
            self.covers
                .trim()
                .parse::<u32>()
                .map(|v| v.abs_diff(version))
                .unwrap_or(u32::MAX / 4)
        }
    }

    /// (类型/维数不符项数, 缺失项数)。选族降级只允许 `wrong == 0`。
    #[must_use]
    pub fn diff_counts(&self, vars: &VarImage) -> (usize, usize) {
        let mut wrong = 0usize;
        let mut present = vec![false; self.len()];
        for rec in vars.records.iter().filter(|r| r.kind == 1) {
            let id = rec.var_id as usize;
            if id >= self.len() {
                continue;
            }
            let decl = &self.decls[id];
            if rec.value_type != decl.value_type || rec.dims != decl.dims {
                wrong += 1;
            }
            present[id] = true;
        }
        let missing = present.iter().filter(|hit| !**hit).count();
        (wrong, missing)
    }

    /// 本表中在 ysv 里**没有 kind=1 镜像**的 id 数(选族排序用)。
    #[must_use]
    pub fn missing_ids(&self, vars: &VarImage) -> Vec<u32> {
        let present: std::collections::BTreeSet<u32> = vars
            .records
            .iter()
            .filter(|r| r.kind == 1)
            .map(|r| u32::from(r.var_id))
            .collect();
        (0..self.len() as u32)
            .filter(|id| !present.contains(id))
            .collect()
    }

    /// 面向日志的描述:表名 + 覆盖区间 + 实际校验结果。
    #[must_use]
    pub fn describe(&self) -> String {
        let base = if let Some(ysv_sha) = &self.source_ysv_sha256 {
            match &self.source {
                SysVarSource::AbiProfile { name, .. } => format!(
                    "engine ABI profile {name} (derived from project ysv fingerprint {ysv_sha})"
                ),
                other => other.describe(),
            }
        } else {
            self.source.describe()
        };
        let base = if self.covers.is_empty() {
            base
        } else {
            format!("{base} covers={}", self.covers)
        };
        match &self.derivation {
            Some(extra) => format!("{base} [{extra}]"),
            None => base,
        }
    }

    /// 表名(诊断用)。
    pub fn source_name(&self) -> &str {
        match &self.source {
            SysVarSource::Ycd { .. } => "project YSCom.ycd",
            SysVarSource::AbiProfile { name, .. } => name,
        }
    }

    /// 解析一份内嵌 TSV(`# source_ycd_sha256` + `id/name/type/dims` 行)。
    /// 从 TSV 文本构造(测试/外部表装载用;`name` 进诊断)。
    pub fn from_tsv_text(text: &str, name: &'static str) -> Self {
        Self::from_tsv(text, name)
    }

    fn from_tsv(text: &str, name: &'static str) -> Self {
        let mut decls = Vec::new();
        let mut sha = String::new();
        let mut covers = String::new();
        let mut source_ysv_sha256 = None;
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("# source_ycd_sha256\t") {
                sha = rest.trim().to_string();
                continue;
            }
            if let Some(rest) = line.strip_prefix("# source_ysv_sha256\t") {
                source_ysv_sha256 = Some(rest.trim().to_string());
                continue;
            }
            if let Some(rest) = line.strip_prefix("# covers\t") {
                covers = rest.trim().to_string();
                continue;
            }
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            let mut parts = line.split('\t');
            let Some(_id) = parts.next() else { continue };
            let Some(name) = parts.next() else { continue };
            let Some(ty) = parts.next() else { continue };
            let dims = parts.next().unwrap_or(""); // 缺省:无 dims 列 = 标量
            decls.push(SysVarDecl {
                name: name.to_string(),
                value_type: ty.parse().unwrap_or(0), // 缺省:非法类型视为 0(仅声明)
                dims: if dims.trim().is_empty() {
                    Vec::new()
                } else {
                    dims.split(',')
                        .filter_map(|d| d.trim().parse().ok())
                        .collect()
                },
            });
        }
        if covers.is_empty() {
            // 兼容旧表:从文件名 `sysvar-abi-<range>[-<sha8>].tsv` 推。
            // 缺省:文件名不含版本号时留空(日志只显示 sha)。
            covers = name
                .strip_prefix("sysvar-abi-")
                .and_then(|r| r.strip_suffix(".tsv"))
                .map(|r| r.split('-').next().unwrap_or(r).to_string())
                .unwrap_or_default(); // 缺省:非标准表名
        }
        Self {
            decls,
            source: SysVarSource::AbiProfile { name, sha256: sha },
            covers,
            source_ysv_sha256,
            derivation: None,
        }
    }

    pub fn id(&self, name: &str) -> Option<u16> {
        self.decls
            .iter()
            .position(|d| d.name == name)
            .map(|i| i as u16)
    }

    pub fn get(&self, id: u16) -> Option<&SysVarDecl> {
        self.decls.get(id as usize)
    }

    pub fn len(&self) -> usize {
        self.decls.len()
    }

    pub fn is_empty(&self) -> bool {
        self.decls.is_empty()
    }

    /// 用工程自己的 `ysv.ybn` 校验本表。
    ///
    /// 判据只看 `var_id < 表长` 的 `kind==1` 镜像,并且**逐项相等**。
    /// 表外(`var_id >= 表长`)的 kind=1 记录不参与校验:官方在 0.480 之后
    /// 多次**尾部追加**系统变量,v555 等商业构建也常如此(旁线实证
    /// `@_SCREEN_SIZE_X`=115、`@_KEY_A`=167 与 0495 一致)。尾部追加不影响
    /// 已有 id 的语义,所以不构成失配。
    ///
    /// 任何**前缀**不符都说明表选错了版本 ⇒ 返回 `Err`。
    pub fn verify_against_ysvr(&self, vars: &VarImage) -> Result<(), yu_log::Diag> {
        let mut seen = vec![false; self.len()];
        for rec in vars.records.iter().filter(|r| r.kind == 1) {
            let id = rec.var_id as usize;
            if id >= self.len() {
                continue;
            }
            let decl = &self.decls[id];
            if rec.value_type != decl.value_type || rec.dims != decl.dims {
                return Err(mismatch(format!(
                    "id {id}({}) 不匹配:ABI(type {}, dims {:?}) vs ysv(type {}, dims {:?});                     说明该工程的引擎 ABI 与本表版本不同",
                    decl.name, decl.value_type, decl.dims, rec.value_type, rec.dims
                )));
            }
            seen[id] = true;
        }
        if let Some(missing) = seen.iter().position(|hit| !hit) {
            return Err(mismatch(format!(
                "id {missing}({}) 在 ysv 中无镜像记录",
                self.decls[missing].name
            )));
        }
        Ok(())
    }

    /// 表外追加项(ids `>= 表长` 的 kind=1 记录数)。
    ///
    /// 非零表示该构建在已知表之后追加了系统变量:前缀语义仍可用,追加项的
    /// 名字未知(按普通槽位处理),启动日志必须显式列出。
    #[must_use]
    pub fn appended_ids(&self, vars: &VarImage) -> Vec<u32> {
        let mut out: Vec<u32> = vars
            .records
            .iter()
            .filter(|r| r.kind == 1 && (r.var_id as usize) >= self.len())
            .map(|r| u32::from(r.var_id))
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }
}

fn mismatch(detail: String) -> yu_log::Diag {
    diag::fatal(
        code::table::VERSION,
        Boundary::Table,
        format!("系统变量 ABI 校验失败:{detail}(DIV-RT-016)"),
    )
    .at_table("ysv.ybn")
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    // 零依赖 SHA-256(表装载诊断用;不是安全用途)。
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut msg = bytes.to_vec();
    let bit_len = (bytes.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let mut a = h;
        for i in 0..64 {
            let s1 = a[4].rotate_right(6) ^ a[4].rotate_right(11) ^ a[4].rotate_right(25);
            let ch = (a[4] & a[5]) ^ (!a[4] & a[6]);
            let t1 = a[7]
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a[0].rotate_right(2) ^ a[0].rotate_right(13) ^ a[0].rotate_right(22);
            let maj = (a[0] & a[1]) ^ (a[0] & a[2]) ^ (a[1] & a[2]);
            let t2 = s0.wrapping_add(maj);
            a[7] = a[6];
            a[6] = a[5];
            a[5] = a[4];
            a[4] = a[3].wrapping_add(t1);
            a[3] = a[2];
            a[2] = a[1];
            a[1] = a[0];
            a[0] = t1.wrapping_add(t2);
        }
        for i in 0..8 {
            h[i] = h[i].wrapping_add(a[i]);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn builtin_abi_has_expected_shape() {
        let t = SysVarTable::builtin_abi();
        assert_eq!(t.len(), 548, "0.495 ABI 表 548 项");
        assert_eq!(t.id("_TIME"), Some(0));
        assert_eq!(t.id("_PAI"), Some(10));
        assert_eq!(t.id("_LC"), Some(48));
        assert_eq!(t.id("_PINT"), Some(53));
        assert_eq!(t.id("_KEY_A"), Some(167));
        assert_eq!(t.id("_KEY_ANY"), Some(275));
        assert_eq!(t.get(48).unwrap().value_type, 1);
        assert!(matches!(t.source, SysVarSource::AbiProfile { .. }));
    }

    /// 用某 ABI 族自身生成"游戏 ysv 指纹"(kind=1,var_id=id,type/dims 同表)。
    fn image_from(table: &SysVarTable) -> VarImage {
        let records = table
            .decls
            .iter()
            .enumerate()
            .map(|(i, d)| crate::tables::VarRecord {
                kind: 1,
                group: 0,
                script_id: 0,
                var_id: i as u16,
                value_type: d.value_type,
                dims: d.dims.clone(),
                init: crate::tables::VarInit::None,
            })
            .collect();
        VarImage {
            version: 495,
            records,
        }
    }

    /// 四个已知族的指纹各自选中自己(顺序无歧义)。
    #[test]
    fn fingerprint_selects_each_known_family() {
        let profiles = SysVarTable::known_profiles();
        assert!(profiles.len() >= 4, "至少 0292/0466/0479/0495 四族");
        for p in &profiles {
            let vars = image_from(p);
            let sel = SysVarTable::select_for(&vars)
                .unwrap_or_else(|e| panic!("{} 指纹应自选中: {e}", p.source_name()));
            // 多张 ycd 可能内容相同(如 0450/0451),指纹只能保证选到**等价**族。
            assert_eq!(
                sel.decls
                    .iter()
                    .map(|d| (&d.name, d.value_type, &d.dims))
                    .collect::<Vec<_>>(),
                p.decls
                    .iter()
                    .map(|d| (&d.name, d.value_type, &d.dims))
                    .collect::<Vec<_>>(),
                "{} 被判给了内容不同的 {}",
                p.source_name(),
                sel.source_name()
            );
        }
    }

    /// 任一项失配(改一个维数)⇒ 全部族拒绝,错误列出逐族原因。
    #[test]
    fn fingerprint_rejects_unknown_and_lists_reasons() {
        let mut vars = image_from(&SysVarTable::builtin_abi());
        // 同槽改型 1 项可派生;10 项以上说明不是同一族,必须拒绝。
        for rec in vars.records.iter_mut().take(10) {
            rec.dims = vec![99];
        }
        let e = SysVarTable::select_for(&vars).expect_err("失配必须拒绝");
        assert_eq!(e.code, "table.version");
        for name in ["0292", "0466", "0479", "0495"] {
            assert!(
                e.detail.contains(name),
                "失配原因应含族名 {name}: {}",
                e.detail
            );
        }
    }

    /// 同内容多表(0494/0495)按游戏 `ysv` 版本号挑最贴切的一张。
    #[test]
    fn identical_tables_prefer_matching_version() {
        let base = SysVarTable::builtin_abi();
        let mut vars = image_from(&base);
        vars.version = 494;
        let sel = SysVarTable::select_for(&vars).expect("应按版本选中 0494");
        assert_eq!(sel.source_name(), "sysvar-abi-0494-185f14aa.tsv");
        vars.version = 495;
        let sel = SysVarTable::select_for(&vars).expect("应按版本选中 0495");
        assert_eq!(sel.source_name(), "sysvar-abi-0495-a626e40a.tsv");
    }

    /// 单槽改型(如 `_DROPFILE` 维数变化)走结构派生:保留同槽名字,
    /// 类型/维数以工程 ysv 为准;不把整表判错。
    #[test]
    fn single_slot_shape_change_is_derived() {
        let base = SysVarTable::builtin_abi();
        let mut vars = image_from(&base);
        vars.records[50].dims = Vec::new();
        let sel = SysVarTable::select_for(&vars).expect("单槽改型应派生");
        assert_eq!(sel.get(50).unwrap().name, base.get(50).unwrap().name);
        assert!(sel.get(50).unwrap().dims.is_empty());
        assert!(sel.derivation.is_some(), "派生表必须记录基线/统计");
    }

    /// 0.290 商业构建:Beta 3 的 0.292 表在 id 52 前多一个普通字符串槽,
    /// 后续系统变量整体后移。结构对齐必须把 `_SYSTEM_VERSION` 放到 53,
    /// 新增槽位用 `_SYSVAR_0052`,不按版本号硬编码。
    #[test]
    fn beta3_commercial_insertion_is_derived() {
        let base = SysVarTable::from_tsv(ABI_TSV_0292, "sysvar-abi-0292-e7016b1e.tsv");
        let mut records = Vec::new();
        for id in 0..52usize {
            let d = &base.decls[id];
            records.push(rec(id as u16, d.value_type, d.dims.clone()));
        }
        records.push(rec(52, 3, Vec::new()));
        for id in 52..60usize {
            let d = &base.decls[id];
            records.push(rec((id + 1) as u16, d.value_type, d.dims.clone()));
        }
        let vars = VarImage { version: 290, records };
        let sel = SysVarTable::select_for(&vars).expect("0.290 变体应派生");
        assert_eq!(sel.covers, "0290");
        assert_eq!(
            sel.get(52).unwrap().name,
            "_SAVE_PATH",
            "`_PROJECT_PATH` 后的 STR 插入槽按官方槽位命名"
        );
        assert_eq!(sel.get(53).unwrap().name, "_SYSTEM_VERSION");
        assert_eq!(sel.get(54).unwrap().name, "_SCREEN_COLOR");
        assert!(sel.derivation.as_deref().is_some_and(|d| d.contains("0292")));
    }

    /// 0.263 商业构建:Beta 2 的 0.255 表删掉 32 个 `_PAD_BUTTONn` 与
    /// 32 个 `_PADNUM_BUTTONn`,保留方向槽 + 合并槽。对齐必须落在方向槽
    /// (`_PAD_UP..RIGHT`),而不是块尾的 `_PAD_BUTTON29..32`。
    #[test]
    fn beta2_commercial_block_removal_is_derived() {
        let base = SysVarTable::from_tsv(ABI_TSV_0255, "sysvar-abi-0255-81a05c0c.tsv");
        let mut records = Vec::new();
        let mut next = 0u16;
        for (id, d) in base.decls.iter().enumerate() {
            if (211..=242).contains(&id) || (375..=406).contains(&id) {
                continue;
            }
            let mut dims = d.dims.clone();
            if id == 14 {
                dims = vec![1024];
            }
            records.push(rec(next, d.value_type, dims));
            next += 1;
        }
        let vars = VarImage { version: 263, records };
        let sel = SysVarTable::select_for(&vars).expect("0.263 变体应派生");
        assert_eq!(sel.covers, "0263");
        assert_eq!(sel.get(210).unwrap().name, "_PAD_RIGHT");
        assert_eq!(sel.get(211).unwrap().name, "_PAD_BUTTON");
        assert_eq!(sel.get(212).unwrap().name, "_MOUSENUM_L");
        assert_eq!(sel.get(339).unwrap().name, "_PADNUM_UP");
        assert_eq!(sel.get(343).unwrap().name, "_PADNUM_BUTTON");
        assert_eq!(sel.get(14).unwrap().name, "_DROPFILE");
        assert_eq!(sel.get(14).unwrap().dims, vec![1024]);
    }

    fn rec(id: u16, ty: u8, dims: Vec<u32>) -> crate::tables::VarRecord {
        crate::tables::VarRecord {
            kind: 1,
            group: 0,
            script_id: 0,
            var_id: id,
            value_type: ty,
            dims,
            init: crate::tables::VarInit::None,
        }
    }

    /// v555 形态(Likely):548 前缀 + 尾部追加项 ⇒ 仍归 0495 族
    /// (校验只看 `var_id < 表长` 的镜像,追加项不影响)。
    #[test]
    fn trailing_extra_records_keep_newest_family() {
        let mut vars = image_from(&SysVarTable::builtin_abi());
        for extra in 0..3u16 {
            vars.records.push(crate::tables::VarRecord {
                kind: 1,
                group: 0,
                script_id: 0,
                var_id: 548 + extra,
                value_type: 1,
                dims: Vec::new(),
                init: crate::tables::VarInit::None,
            });
        }
        let sel = SysVarTable::select_for(&vars).expect("前缀匹配应通过");
        assert!(
            sel.len() >= 548,
            "应选中 0495 或更新的族: {}",
            sel.source_name()
        );
    }
}
