//! `ysv.ybn`(魔数 `YSVR`)· 变量声明记录(无名字;变量 = 纯槽位 id)。
//!
//! 布局(ERIS 语料 2568/2568 条**逐字节精确闭合**;旧接手线
//! `<repo-old>/crates/yuris-format/src/ysvr.rs` 在 v555 商业游戏上
//! 3362/3362 闭合,双语料互证;字段语义另见旁线 `format-ybn.md` §8/§10):
//!
//! ```text
//! +0x00 char[4] "YSVR"
//! +0x04 u32     版本
//! +0x08 u16     条数
//! +0x0A 记录[条数](变长,无对齐填充):
//!        +0x00 u8        kind     1=全局(G_*) 2=局部(随脚本装载) 3=静态(S_*)
//!        +0x01 u8        flag     恢复组号(G_* 族名字里的数字;0 = 基础组)
//!        +0x02 u16       file_id  局部变量所属脚本号(kind==2 时有意义)
//!        +0x04 u16       slot     变量槽位(必须 < yst.ybn varCount+1)
//!        +0x06 u8        ty       1=INT 2=FLT 3=STR **0=仅声明无初值**
//!        +0x07 u8        dims     维数;>0 时随后 dims × u32 各维大小
//!        初值:ty=1 → i64;ty=2 → f64;ty=3 → u16 长度 + 字节;
//!              ty=0 → 无初值段(实测 ERIS 语料 452 条)
//! ```
//!
//! 字符串初值内嵌的是 YSTB 参数数据同款形态(如 `4D 10 00 "SCENARIO_START"`,
//! `M` + u16 长度 + 引号串);本层只存原始字节,解释归运行核。

use crate::container::{read_header, truncated, u16_at};
use crate::code;
use yu_log::{Boundary, Diag};

/// 变量初值。
#[derive(Clone, Debug, PartialEq)]
pub enum Init {
    /// `ty = 0`:仅声明,无初值段。
    None,
    /// `ty = 1`:i64。
    Int(i64),
    /// `ty = 2`:f64(按位参与比较)。
    Float(f64),
    /// `ty = 3`:原始字节(u16 长度之后的内容)。
    Str(Vec<u8>),
}

/// 一条变量声明。
#[derive(Clone, Debug, PartialEq)]
pub struct VarDecl {
    /// `kind` 原始字节(1=全局 / 2=局部 / 3=静态;其余 = `fmt.ysv.field`)。
    pub kind: u8,
    /// `flag` 原始字节(恢复组号;G_* 族 0..=3 已实证,4 未见)。
    pub flag: u8,
    /// 局部变量所属脚本号。
    pub file_id: u16,
    /// 变量槽位。
    pub slot: u16,
    /// `ty` 原始字节(0..=3;其余 = `fmt.ysv.field`)。
    pub ty: u8,
    /// 各维大小(空 = 标量)。
    pub dims: Vec<u32>,
    /// 初值。
    pub init: Init,
}

/// 已解码的变量声明表。
#[derive(Clone, Debug, PartialEq)]
pub struct VarDecls {
    /// 容器头版本(`+0x04`;ERIS 语料 495。R12 起 yu-rt 适配层需要)。
    pub version: u32,
    /// 声明(文件顺序)。
    pub decls: Vec<VarDecl>,
}

impl VarDecls {
    /// 解码 `ysv.ybn` 字节。
    pub fn parse(buf: &[u8], origin: &[u8]) -> Result<Self, Diag> {
        let header = read_header(buf, origin, b"YSVR")?;
        let count = u16_at(buf, 8) as usize;
        let mut at = 0x0Ausize;
        let mut decls = Vec::with_capacity(count.min(8192));
        for _ in 0..count {
            let head_end = at
                .checked_add(8)
                .ok_or_else(|| limit(origin, at as u64, "YSVR 记录头"))?;
            if head_end > buf.len() {
                return Err(truncated(origin, head_end, buf.len(), "YSVR 记录头"));
            }
            let kind = buf[at];
            let flag = buf[at + 1];
            let file_id = u16_at(buf, at + 2);
            let slot = u16_at(buf, at + 4);
            let ty = buf[at + 6];
            let dims = usize::from(buf[at + 7]);
            if !(1..=3).contains(&kind) {
                return Err(Diag::fatal(code::YSV_FIELD, Boundary::Format)
                    .at_data(origin, at as u64)
                    .with_detail(format!("变量种类字节非法:{kind}")));
            }
            if !(0..=3).contains(&ty) {
                return Err(Diag::fatal(code::YSV_FIELD, Boundary::Format)
                    .at_data(origin, (at + 6) as u64)
                    .with_detail(format!("变量类型字节非法:{ty}")));
            }
            at = head_end;
            let mut dim_sizes = Vec::with_capacity(dims);
            for _ in 0..dims {
                let dim_end = at
                    .checked_add(4)
                    .ok_or_else(|| limit(origin, at as u64, "YSVR 维度表"))?;
                if dim_end > buf.len() {
                    return Err(truncated(origin, dim_end, buf.len(), "YSVR 维度表"));
                }
                dim_sizes.push(crate::container::u32_at(buf, at));
                at = dim_end;
            }
            let init = match ty {
                0 => Init::None,
                1 | 2 => {
                    let value_end = at
                        .checked_add(8)
                        .ok_or_else(|| limit(origin, at as u64, "YSVR 数值初值"))?;
                    if value_end > buf.len() {
                        return Err(truncated(origin, value_end, buf.len(), "YSVR 数值初值"));
                    }
                    let raw = &buf[at..value_end];
                    at = value_end;
                    if ty == 1 {
                        Init::Int(i64::from_le_bytes(raw.try_into().unwrap()))
                    } else {
                        Init::Float(f64::from_le_bytes(raw.try_into().unwrap()))
                    }
                }
                _ => {
                    let body = at
                        .checked_add(2)
                        .ok_or_else(|| limit(origin, at as u64, "YSVR 串初值长度"))?;
                    if body > buf.len() {
                        return Err(truncated(origin, body, buf.len(), "YSVR 串初值长度"));
                    }
                    let len = usize::from(u16_at(buf, at));
                    let end = body
                        .checked_add(len)
                        .ok_or_else(|| limit(origin, body as u64, "YSVR 串初值"))?;
                    if end > buf.len() {
                        return Err(truncated(origin, end, buf.len(), "YSVR 串初值"));
                    }
                    let content = buf[body..end].to_vec();
                    at = end;
                    Init::Str(content)
                }
            };
            decls.push(VarDecl {
                kind,
                flag,
                file_id,
                slot,
                ty,
                dims: dim_sizes,
                init,
            });
        }
        Ok(Self {
            version: header.version,
            decls,
        })
    }

    /// 引擎初始化装载视角(引擎传 `-1`):只收全局与静态。
    pub fn loaded_at_init(&self) -> impl Iterator<Item = &VarDecl> {
        self.decls.iter().filter(|d| matches!(d.kind, 1 | 3))
    }

    /// 引擎按脚本装载视角:只收该脚本号的局部声明。
    pub fn loaded_for_script(&self, file_id: u16) -> impl Iterator<Item = &VarDecl> {
        self.decls
            .iter()
            .filter(move |d| d.kind == 2 && d.file_id == file_id)
    }
}

/// 记录长度/偏移超出可表示范围 = `fmt.limit`(32 位目标不回绕)。
fn limit(origin: &[u8], at: u64, what: &str) -> Diag {
    Diag::fatal(code::LIMIT, Boundary::Format)
        .at_data(origin, at)
        .with_detail(format!("{what} 超出可表示范围"))
}
