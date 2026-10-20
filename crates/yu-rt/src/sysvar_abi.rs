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
//! 2. 否则用本 crate 的 `data/sysvar-abi-0495.tsv`(由脚本从官方 ycd 生成,
//!    带来源 SHA-256),**并用工程自己的 `ysv.ybn` 逐项校验**;
//! 3. 校验失败 ⇒ `table.version` Fatal(拒绝启动)。这条校验保证"表是引擎
//!    ABI"不是口头声明,而是每次启动都可复核。

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

const ABI_TSV: &str = include_str!("../data/sysvar-abi-0495.tsv");

impl SysVarTable {
    /// 从工程 `YSCom.ycd` 的解析结果构造。
    pub fn from_ycd(ycd: &crate::ycd::YcdTable, path: impl Into<String>, sha256: impl Into<String>) -> Self {
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

    /// 内嵌引擎 ABI 数据表。
    pub fn builtin_abi() -> Self {
        let mut decls = Vec::new();
        let mut sha = String::new();
        for line in ABI_TSV.lines() {
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
            source: SysVarSource::AbiProfile {
                name: "sysvar-abi-0495.tsv",
                sha256: sha,
            },
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
                return Err(mismatch(format!("id {id}({}) 在 ysv 中无镜像记录", decl.name)));
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
}
