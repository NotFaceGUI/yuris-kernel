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
//! 2. 否则用 `data/sysvar-abi-*.tsv` 里的**已知 ABI profile 族**,按
//!    **游戏自己 `ysv.ybn` 的指纹**(前 N 条 kind=1 的 id/类型/维数)逐族
//!    校验,选第一个通过的族([`SysVarTable::select_for`]);
//! 3. 无一通过 ⇒ `table.version` Fatal,并列出每个族的失配原因(拒绝启动)。
//!
//! # 为什么按指纹选族,而不是按版本号
//!
//! 官方履历与四份 SDK 的 ycd 实测(见 PROGRESS 成果 E3 附):
//! 系统变量表**在小版本里也会增长**(如 ver0.480/0.01 一次性追加
//! `_PINT/_PFLT/_PSTR/_PFLG 11..16` 等),插入后所有后续 id 平移;
//! 官方不归档小版本 SDK。因此版本号不可用于选表,唯一可靠判据是游戏
//! 自带 `ysv` 的 (id, type, dims) 指纹。已知族:
//!
//! | profile | 变量数 | 覆盖构建 |
//! |---|---|---|
//! | `sysvar-abi-0292.tsv` | 482 | Beta3 线(0.29x) |
//! | `sysvar-abi-0466.tsv` | 513 | 4.x ≤0.469 |
//! | `sysvar-abi-0479.tsv` | 526 | 0.470–0.479 |
//! | `sysvar-abi-0495.tsv` | 548 | ≥0.480(含 v494/v495;v555 若仅尾部追加亦可,见下) |
//!
//! v555 样本(Tyn 旁线实证 `@_SCREEN_SIZE_X`=115、`@_KEY_A`=167)提示
//! 5xx 保留 548 前缀、只在尾部追加:指纹校验只看 `var_id < 表长` 的镜像,
//! 尾部追加不影响 0495 族的匹配(结果按 Likely 记录,待 v555 样本钉死)。

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
}

const ABI_TSV_0495: &str = include_str!("../data/sysvar-abi-0495.tsv");
const ABI_TSV_0479: &str = include_str!("../data/sysvar-abi-0479.tsv");
const ABI_TSV_0466: &str = include_str!("../data/sysvar-abi-0466.tsv");
const ABI_TSV_0292: &str = include_str!("../data/sysvar-abi-0292.tsv");

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
    pub fn known_profiles() -> Vec<Self> {
        vec![
            Self::from_tsv(ABI_TSV_0495, "sysvar-abi-0495.tsv"),
            Self::from_tsv(ABI_TSV_0479, "sysvar-abi-0479.tsv"),
            Self::from_tsv(ABI_TSV_0466, "sysvar-abi-0466.tsv"),
            Self::from_tsv(ABI_TSV_0292, "sysvar-abi-0292.tsv"),
        ]
    }

    /// **按游戏自带 `ysv.ybn` 的指纹选族**(数据驱动,不看版本号)。
    ///
    /// 判据 = [`Self::verify_against_ysvr`]:`var_id < 表长` 的 kind=1 镜像
    /// 必须恰好覆盖 `0..表长`,且 (type, dims) 逐项相等。全部族都不通过 ⇒
    /// `table.version` Fatal,并逐族列出失配原因(未知引擎构建,拒绝启动;
    /// 需要该构建自带 ycd 或从其 exe 取的 ABI 表)。
    pub fn select_for(vars: &VarImage) -> Result<Self, yu_log::Diag> {
        let mut failures = Vec::new();
        for table in Self::known_profiles() {
            match table.verify_against_ysvr(vars) {
                Ok(()) => return Ok(table),
                Err(d) => failures.push(format!(
                    "{}: {}",
                    table.source_name(),
                    d.detail.trim_start_matches("系统变量 ABI 校验失败:")
                )),
            }
        }
        Err(diag::fatal(
            code::table::VERSION,
            Boundary::Table,
            format!(
                "工程 ysv.ybn 的系统变量指纹与所有已知 ABI 族都不符(未知引擎构建);\
                 需要该构建的 ABI 表(自带 YSCom.ycd 或从其引擎 exe 提取)。逐族失配:{}(A5:拒绝启动,不猜)",
                failures.join(" | ")
            ),
        )
        .at_table("ysv.ybn"))
    }

    /// 表名(诊断用)。
    pub fn source_name(&self) -> &str {
        match &self.source {
            SysVarSource::Ycd { .. } => "project YSCom.ycd",
            SysVarSource::AbiProfile { name, .. } => name,
        }
    }

    /// 解析一份内嵌 TSV(`# source_ycd_sha256` + `id/name/type/dims` 行)。
    fn from_tsv(text: &str, name: &'static str) -> Self {
        let mut decls = Vec::new();
        let mut sha = String::new();
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("# source_ycd_sha256\t") {
                sha = rest.trim().to_string();
                continue;
            }
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            let mut parts = line.split('\t');
            let Some(_id) = parts.next() else { continue };
            let Some(name) = parts.next() else { continue };
            let Some(ty) = parts.next() else { continue };
            let dims = parts.next().unwrap_or("");
            decls.push(SysVarDecl {
                name: name.to_string(),
                value_type: ty.parse().unwrap_or(0),
                dims: if dims.trim().is_empty() {
                    Vec::new()
                } else {
                    dims.split(',')
                        .filter_map(|d| d.trim().parse().ok())
                        .collect()
                },
            });
        }
        Self {
            decls,
            source: SysVarSource::AbiProfile { name, sha256: sha },
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

    /// 用工程自己的 `ysv.ybn` 逐项校验:前 `len()` 条 `kind==1` 记录的
    /// `(var_id, value_type, dims)` 必须与本表**逐项相等**。
    ///
    /// 这不是"猜":两条独立来源(`ysc/ycd` 侧与 `ysv` 侧)交叉一致才放行。
    /// 任何不符都说明引擎 ABI 版本与表不匹配 ⇒ 拒绝启动。
    pub fn verify_against_ysvr(&self, vars: &VarImage) -> Result<(), yu_log::Diag> {
        let mirrors: Vec<_> = vars
            .records
            .iter()
            .filter(|r| r.kind == 1 && (r.var_id as usize) < self.len())
            .collect();
        if mirrors.len() != self.len() {
            return Err(mismatch(format!(
                "系统变量镜像条数 {} != ABI 表 {} 项",
                mirrors.len(),
                self.len()
            )));
        }
        for (id, decl) in self.decls.iter().enumerate() {
            let Some(rec) = mirrors.iter().find(|r| r.var_id as usize == id) else {
                return Err(mismatch(format!(
                    "id {id}({}) 在 ysv 中无镜像记录",
                    decl.name
                )));
            };
            if rec.value_type != decl.value_type || rec.dims != decl.dims {
                return Err(mismatch(format!(
                    "id {id}({}) 不匹配:ABI(type {}, dims {:?}) vs ysv(type {}, dims {:?});\
                     说明该工程的引擎 ABI 与本表版本不同",
                    decl.name, decl.value_type, decl.dims, rec.value_type, rec.dims
                )));
            }
        }
        Ok(())
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
        assert_eq!(profiles.len(), 4, "0292/0466/0479/0495 四族");
        for p in &profiles {
            let vars = image_from(p);
            let sel = SysVarTable::select_for(&vars)
                .unwrap_or_else(|e| panic!("{} 指纹应自选中: {e}", p.source_name()));
            assert_eq!(sel.source_name(), p.source_name());
            assert_eq!(sel.len(), p.len());
        }
        // 变量数依次递增(与官方 ycd 实测一致)。
        let lens: Vec<usize> = profiles.iter().map(|p| p.len()).collect();
        assert_eq!(lens, vec![548, 526, 513, 482], "profile 长度");
    }

    /// 任一项失配(改一个维数)⇒ 全部族拒绝,错误列出逐族原因。
    #[test]
    fn fingerprint_rejects_unknown_and_lists_reasons() {
        let mut vars = image_from(&SysVarTable::builtin_abi());
        vars.records[10].dims = vec![99];
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
        assert_eq!(sel.source_name(), "sysvar-abi-0495.tsv");
    }
}
