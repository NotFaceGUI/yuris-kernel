//! `ysv.ybn`(魔数 `YSVR`)· 变量声明记录(无名字;变量 = 纯槽位 id)。
//!
//! 两个记录布局,由**结构闭合**判定(不按版本号硬编码):
//!
//! - `WithFlag`(0.495 ERIS 2568/2568、v555 3362/3362 精确闭合):
//!   `kind u8, flag u8, file_id u16, slot u16, ty u8, dims u8`(8 字节头)。
//! - `NoFlag`(v474 商业样本 2736/2736 精确闭合):没有 `flag` 字节,
//!   `kind u8, file_id u16, slot u16, ty u8, dims u8`(7 字节头)。
//!
//! `flag` 在 `NoFlag` 布局下按 0(基础恢复组)处理。
//!
//! ```text
//! +0x00 char[4] "YSVR"
//! +0x04 u32     版本(诊断字段;不参与布局选择)
//! +0x08 u16     条数
//! +0x0A 记录[条数](变长,无对齐填充):
//!        kind     1=全局(G_*) 2=局部(随脚本装载) 3=静态(S_*)
//!        [flag]   恢复组号;仅 WithFlag 布局存在
//!        file_id  u16,局部变量所属脚本号(kind==2 时有意义)
//!        slot     u16,变量槽位(必须 < yst.ybn varCount+1)
//!        ty       1=INT 2=FLT 3=STR **0=仅声明无初值**
//!        dims     维数;>0 时随后 dims × u32 各维大小
//!        初值:ty=1 → i64;ty=2 → f64;ty=3 → u16 长度 + 字节;
//!              ty=0 → 无初值段(实测 ERIS 语料 452 条)
//! ```
//!
//! 字符串初值内嵌的是 YSTB 参数数据同款形态(如 `4D 10 00 "SCENARIO_START"`,
//! `M` + u16 长度 + 引号串);本层只存原始字节,解释归运行核。

use crate::code;
use crate::container::{read_header, truncated, u16_at};
use yu_log::{Boundary, Diag};

/// YSVR 记录头布局。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum YsvLayout {
    /// 8 字节头:kind + flag + file_id + slot + ty + dims(0.495 / v555)。
    WithFlag,
    /// 7 字节头:无 flag(v474 样本)。
    NoFlag,
}

impl YsvLayout {
    fn header_len(self) -> usize {
        match self {
            YsvLayout::WithFlag => 8,
            YsvLayout::NoFlag => 7,
        }
    }

    fn field_offsets(self) -> (usize, usize, usize) {
        match self {
            // (file_id, slot, ty)
            YsvLayout::WithFlag => (2, 4, 6),
            YsvLayout::NoFlag => (1, 3, 5),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            YsvLayout::WithFlag => "with-flag",
            YsvLayout::NoFlag => "no-flag",
        }
    }
}

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
    /// 容器头版本(`+0x04`;诊断字段,不参与布局选择)。
    pub version: u32,
    /// 判定出的记录头布局。
    pub layout: YsvLayout,
    /// 声明(文件顺序)。
    pub decls: Vec<VarDecl>,
}

impl VarDecls {
    /// 解码 `ysv.ybn` 字节。
    ///
    /// 两种记录头布局都作为候选;只有"条数走完 + 恰好闭合(允许尾部零填充)"
    /// 的布局才被接受。版本号只用于诊断,不参与判定。
    pub fn parse(buf: &[u8], origin: &[u8]) -> Result<Self, Diag> {
        let header = read_header(buf, origin, b"YSVR")?;
        let count = u16_at(buf, 8) as usize;

        let mut first_error: Option<Diag> = None;
        for layout in [YsvLayout::WithFlag, YsvLayout::NoFlag] {
            match parse_records(buf, count, origin, layout) {
                Ok((decls, consumed)) => {
                    if buf[consumed..].iter().all(|&b| b == 0) {
                        return Ok(Self {
                            version: header.version,
                            layout,
                            decls,
                        });
                    }
                    if first_error.is_none() {
                        first_error = Some(
                            Diag::fatal(code::YSV_FIELD, Boundary::Format)
                                .at_data(origin, consumed as u64)
                                .with_detail(format!(
                                    "{} 布局走完 {count} 条后仍有 {} 字节非零尾部",
                                    layout.as_str(),
                                    buf.len() - consumed
                                )),
                        );
                    }
                }
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }
        Err(first_error.unwrap_or_else(|| {
            Diag::fatal(code::YSV_FIELD, Boundary::Format)
                .at_data(origin, 0x0A)
                .with_detail("ysv.ybn 无任何结构闭合的记录布局")
        }))
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

fn parse_records(
    buf: &[u8],
    count: usize,
    origin: &[u8],
    layout: YsvLayout,
) -> Result<(Vec<VarDecl>, usize), Diag> {
    let head_len = layout.header_len();
    let (fid_off, slot_off, ty_off) = layout.field_offsets();
    let mut at = 0x0Ausize;
    let mut decls = Vec::with_capacity(count.min(8192));
    for _ in 0..count {
        let head_end = at
            .checked_add(head_len)
            .ok_or_else(|| limit(origin, at as u64, "YSVR 记录头"))?;
        if head_end > buf.len() {
            return Err(truncated(origin, head_end, buf.len(), "YSVR 记录头"));
        }
        let kind = buf[at];
        let flag = if layout == YsvLayout::WithFlag {
            buf[at + 1]
        } else {
            0
        };
        let file_id = u16_at(buf, at + fid_off);
        let slot = u16_at(buf, at + slot_off);
        let ty = buf[at + ty_off];
        let dims = usize::from(buf[at + ty_off + 1]);
        if !(1..=3).contains(&kind) {
            return Err(Diag::fatal(code::YSV_FIELD, Boundary::Format)
                .at_data(origin, at as u64)
                .with_detail(format!("变量种类字节非法:{kind}({} 布局)", layout.as_str())));
        }
        if !(0..=3).contains(&ty) {
            return Err(Diag::fatal(code::YSV_FIELD, Boundary::Format)
                .at_data(origin, (at + ty_off) as u64)
                .with_detail(format!("变量类型字节非法:{ty}({} 布局)", layout.as_str())));
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
    Ok((decls, at))
}

/// 记录长度/偏移超出可表示范围 = `fmt.limit`(32 位目标不回绕)。
fn limit(origin: &[u8], at: u64, what: &str) -> Diag {
    Diag::fatal(code::LIMIT, Boundary::Format)
        .at_data(origin, at)
        .with_detail(format!("{what} 超出可表示范围"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_flag_layout_parses_v474_shape() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"YSVR");
        buf.extend_from_slice(&474u32.to_le_bytes());
        buf.extend_from_slice(&2u16.to_le_bytes());
        // 1) 全局 INT:kind,file_id,slot,ty,dims,init(i64)
        buf.extend_from_slice(&[1]);
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&10u16.to_le_bytes());
        buf.extend_from_slice(&[1, 0]);
        buf.extend_from_slice(&400i64.to_le_bytes());
        // 2) 局部 STR 数组:kind,file_id,slot,ty,dims,dim[1],len+bytes
        buf.extend_from_slice(&[2]);
        buf.extend_from_slice(&7u16.to_le_bytes());
        buf.extend_from_slice(&11u16.to_le_bytes());
        buf.extend_from_slice(&[3, 1]);
        buf.extend_from_slice(&5u32.to_le_bytes());
        buf.extend_from_slice(&3u16.to_le_bytes());
        buf.extend_from_slice(b"abc");

        let decls = VarDecls::parse(&buf, b"v474/ysv.ybn").unwrap();
        assert_eq!(decls.version, 474);
        assert_eq!(decls.layout, YsvLayout::NoFlag);
        assert_eq!(decls.decls.len(), 2);
        assert_eq!(decls.decls[0].flag, 0, "no-flag 布局 flag 恒 0");
        assert_eq!(decls.decls[0].slot, 10);
        assert_eq!(decls.decls[0].init, Init::Int(400));
        assert_eq!(decls.decls[1].file_id, 7);
        assert_eq!(decls.decls[1].dims, vec![5]);
        assert_eq!(decls.decls[1].init, Init::Str(b"abc".to_vec()));
    }

    #[test]
    fn with_flag_layout_still_parses() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"YSVR");
        buf.extend_from_slice(&495u32.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.extend_from_slice(&[1, 2]); // kind, flag
        buf.extend_from_slice(&0u16.to_le_bytes()); // file_id
        buf.extend_from_slice(&3u16.to_le_bytes()); // slot
        buf.extend_from_slice(&[1, 0]); // ty, dims
        buf.extend_from_slice(&9i64.to_le_bytes());

        let decls = VarDecls::parse(&buf, b"v495/ysv.ybn").unwrap();
        assert_eq!(decls.layout, YsvLayout::WithFlag);
        assert_eq!(decls.decls[0].flag, 2);
        assert_eq!(decls.decls[0].init, Init::Int(9));
    }
}
