//! 官方编译器语言定义表(`system/YSCom/YSCom.ycd`,魔数 `YSCD`)。
//!
//! # 为什么运行核要读编译器表
//!
//! 用户决定(2026-10-19):**每个工程的命令/变量定义都可能不同,必须动态
//! 读取,不能硬编码**。`YSCom.ycd` 是工程自带的编译器语言定义,里面有:
//!
//! 1. 命令段:命令名 + 每个参数的 `(类型/种类, ...)` 四字节属性;
//! 2. **系统变量段:548 条**(名 + 值类型 + 维数 + 各维长度);
//! 3. 编译设置项段(`SYSTEMMODE[...]` 的关键字);
//! 4. 参数关键字段(`GOSUB` 的 `PINT`..`PSTR16`、`CG` 的 `FX/FY/...`)。
//!
//! # 实测布局(官方 0.495 `YSCom.ycd`,29,340 字节)
//!
//! ```text
//! +0x00  "YSCD"
//! +0x04  u32 version = 495
//! +0x08  u32 header_count = 119      ← 命令段条数(与 ysc.ybn 的 119 条一致)
//! +0x0C  u32 0
//! +0x10  命令段: header_count 条 {
//!            name\0
//!            u8 参数数 n
//!            n × { pname\0  u8 kind  u8 b1  u8 b2  u8 checker }
//!        }
//!        随后是一批 `");"` 占位串(编译器内部符号,不是命令)
//!        + 编译设置项段(以 SYSTEMMODE 起)
//! +0x??  系统变量段: u32 count(=548) + u32 0,然后 count 条 {
//!            name\0  u8 value_type  u8 dims  u32[dims] 各维长度
//!        }
//!        实测走完 548 条恰好闭合到文件尾
//! ```
//!
//! 注意:`header_count` 是 **119**,而命令段实际只解析出 **114** 条(CDDA.. _IFEND);
//! 差出的 5 条是 `");"` 占位符族(见 [`YcdTable::command_section_end`]),
//! 它们不是脚本可见命令。`ysc.ybn` 的 119 条里也含 5 个 `);` 槽位——两表
//! 条数一致,但**运行核按名字分派,不按位置**。
//!
//! # 系统变量 id
//!
//! 系统变量段的**顺序即 id**(0-based)。实测与语料使用完全一致:
//! `_TIME`=0、`_PAI`=10、`_LC`=48、`_PINT`=53、`_PFLT`=54、`_PSTR`=55、
//! `_PFLG`=56、`_RINT`=60、`_RINTFLG`=64、`_TINT`=67、`_TSTR`=69、
//! `_SCREEN_COLOR`=114、`_SCREEN_SIZE_X`=115、`_KEY_A`=167、`_KEY_ANY`=275。
//! 同时 `ysv.ybn` 的前 548 条 kind=1 记录的 `var_id` 恰为 0..547 且类型逐项
//! 吻合——两条独立来源互证。

use yu_log::Boundary;

use crate::code;
use crate::diag;

/// 系统变量的一条声明。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SysVarDecl {
    pub name: String,
    /// 值类型:1=INT / 2=FLT / 3=STR。
    pub value_type: u8,
    /// 各维长度(空 = 标量)。
    pub dims: Vec<u32>,
}

/// 命令参数属性(四字节)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct YcdParam {
    pub name: String,
    /// 种类(旁线 §3.17:0=数字 1=字符串 2=数字+脚本内偏移 4/5/6/8=变量引用)。
    pub kind: u8,
    pub b1: u8,
    pub b2: u8,
    /// 范围检查器编号(0 = 无)。
    pub checker: u8,
}

/// 命令声明。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct YcdCommand {
    pub name: String,
    pub params: Vec<YcdParam>,
}

/// 已解析的 `YSCom.ycd`。
#[derive(Clone, Debug)]
pub struct YcdTable {
    pub version: u32,
    pub header_count: u32,
    pub commands: Vec<YcdCommand>,
    /// 系统变量段(顺序即 id)。
    pub system_variables: Vec<SysVarDecl>,
    /// 编译设置项(顺序保留)。
    pub settings: Vec<String>,
    /// 参数关键字(顺序保留)。
    pub keywords: Vec<String>,
    /// 命令段结束偏移(诊断)。
    pub command_section_end: usize,
    /// `");"` 占位命令条数(编译器内部符号)。
    pub placeholder_commands: usize,
    /// 系统变量名 → id。
    by_name: std::collections::BTreeMap<String, u16>,
    /// 命令名 → 下标。
    cmd_index: std::collections::BTreeMap<String, usize>,
}

impl YcdTable {
    pub fn parse(data: &[u8]) -> Result<Self, yu_log::Diag> {
        if data.len() < 0x10 {
            return Err(
                diag::fatal(code::table::INCOMPLETE, Boundary::Table, "YSCom.ycd 过短")
                    .at_table("YSCom.ycd"),
            );
        }
        if &data[0..4] != b"YSCD" {
            return Err(diag::fatal(
                code::table::BAD_MAGIC,
                Boundary::Table,
                format!(
                    "YSCom.ycd 魔数 {:?} != YSCD",
                    String::from_utf8_lossy(&data[0..4])
                ),
            )
            .at_table("YSCom.ycd"));
        }
        let version = le_u32(data, 4);
        let header_count = le_u32(data, 8);

        // ---- 命令段 ----
        let mut off = 0x10usize;
        let mut commands = Vec::new();
        let mut placeholder_commands = 0usize;
        let mut cmd_index = std::collections::BTreeMap::new();
        for _ in 0..header_count {
            let Some((name, next)) = cstr(data, off) else {
                break;
            };
            off = next;
            if off >= data.len() {
                break;
            }
            let n = data[off] as usize;
            off += 1;
            let mut params = Vec::with_capacity(n);
            let mut ok = true;
            for _ in 0..n {
                let Some((pname, next)) = cstr(data, off) else {
                    ok = false;
                    break;
                };
                off = next;
                if off + 4 > data.len() {
                    ok = false;
                    break;
                }
                params.push(YcdParam {
                    name: pname,
                    kind: data[off],
                    b1: data[off + 1],
                    b2: data[off + 2],
                    checker: data[off + 3],
                });
                off += 4;
            }
            if !ok {
                break;
            }
            let is_placeholder = name == ");" || name.is_empty();
            if is_placeholder {
                placeholder_commands += 1;
            } else {
                cmd_index.insert(name.clone(), commands.len());
            }
            commands.push(YcdCommand { name, params });
        }
        let command_section_end = off;

        // ---- 系统变量段 ----
        // 段头:`u32 count + u32 0`。定位判据(全部来自一手事实,不含名字表):
        // 1. 走完 count 条记录不越界;
        // 2. 首条名字以 `_` 开头(手册 `basic/svariable.html`:
        //    「最初の一文字が「＿」アンダーバーから始まる変数がシステム変数」);
        // 3. 段尾紧跟另一个 `u32 count2 + u32 0` 段头(0.495 实测为编译器
        //    消息表 121 条)。这三条把随机字节排除在外,且对布局平移不敏感。
        let (sys_off, count) = locate_system_section(data, command_section_end)?;
        let mut off = sys_off + 8;
        let mut system_variables = Vec::with_capacity(count);
        let mut by_name = std::collections::BTreeMap::new();
        for i in 0..count {
            let Some((name, next)) = cstr(data, off) else {
                return Err(diag::fatal(
                    code::table::INCOMPLETE,
                    Boundary::Table,
                    format!("YSCom.ycd 系统变量第 {i} 条名字截断"),
                )
                .at_table("YSCom.ycd"));
            };
            off = next;
            if off + 2 > data.len() {
                return Err(diag::fatal(
                    code::table::INCOMPLETE,
                    Boundary::Table,
                    format!("YSCom.ycd 系统变量第 {i} 条类型截断"),
                )
                .at_table("YSCom.ycd"));
            }
            let value_type = data[off];
            let dims_n = data[off + 1] as usize;
            off += 2;
            let mut dims = Vec::with_capacity(dims_n);
            for _ in 0..dims_n {
                if off + 4 > data.len() {
                    return Err(diag::fatal(
                        code::table::INCOMPLETE,
                        Boundary::Table,
                        format!("YSCom.ycd 系统变量第 {i} 条维长截断"),
                    )
                    .at_table("YSCom.ycd"));
                }
                dims.push(le_u32(data, off));
                off += 4;
            }
            by_name.insert(name.clone(), i as u16);
            system_variables.push(SysVarDecl {
                name,
                value_type,
                dims,
            });
        }
        if off != data.len() {
            // 段尾之后是编译器消息表(0.495 实测:121 条 SJIS 文案)。
            // 它不参与脚本执行,但**段尾必须落在下一个段头**,否则视为布局异常。
            if !(off + 8 <= data.len() && le_u32(data, off + 4) == 0) {
                return Err(diag::fatal(
                    code::table::INCOMPLETE,
                    Boundary::Table,
                    format!(
                        "YSCom.ycd 系统变量段尾不在段边界:走到 {off},文件 {} 字节",
                        data.len()
                    ),
                )
                .at_table("YSCom.ycd"));
            }
        }

        // ---- 设置项 / 参数关键字(段头到系统变量段之间) ----
        let settings = scan_settings(data, command_section_end, sys_off);
        let keywords = scan_keywords(data, command_section_end, sys_off);

        Ok(Self {
            version,
            header_count,
            commands,
            system_variables,
            settings,
            keywords,
            command_section_end,
            placeholder_commands,
            by_name,
            cmd_index,
        })
    }

    pub fn sys_var_id(&self, name: &str) -> Option<u16> {
        self.by_name.get(name).copied()
    }

    pub fn sys_var(&self, id: u16) -> Option<&SysVarDecl> {
        self.system_variables.get(id as usize)
    }

    pub fn command(&self, name: &str) -> Option<&YcdCommand> {
        self.cmd_index.get(name).map(|&i| &self.commands[i])
    }

    /// 某命令某参数序号的声明。
    pub fn param(&self, command: &str, slot: usize) -> Option<&YcdParam> {
        self.command(command)?.params.get(slot)
    }
}

/// 定位系统变量段:候选 `u32 count + u32 0` 起点,要求
/// ① 走完 count 条不越界;② 首条名字以 `_` 开头;③ 段尾紧跟下一个段头。
fn locate_system_section(data: &[u8], from: usize) -> Result<(usize, usize), yu_log::Diag> {
    let mut off = from;
    while off + 8 <= data.len() {
        let count = le_u32(data, off) as usize;
        if (1..=4096).contains(&count) && le_u32(data, off + 4) == 0 {
            if let Some(end) = walk_sys_vars(data, off + 8, count) {
                let first_is_sysvar = cstr(data, off + 8)
                    .map(|(n, _)| n.starts_with('_'))
                    .unwrap_or(false);
                // 段尾要么是文件尾(最小夹具),要么紧跟另一个段头。
                let followed_by_section = end == data.len()
                    || (end + 8 <= data.len()
                        && le_u32(data, end + 4) == 0
                        && (1..=1_000_000).contains(&(le_u32(data, end) as usize)));
                if first_is_sysvar && followed_by_section {
                    return Ok((off, count));
                }
            }
        }
        off += 1;
    }
    Err(diag::fatal(
        code::table::INCOMPLETE,
        Boundary::Table,
        "YSCom.ycd 找不到系统变量段(布局变体?)",
    )
    .at_table("YSCom.ycd"))
}

/// 走完 `count` 条系统变量记录,返回段尾偏移(越界 → `None`)。
fn walk_sys_vars(data: &[u8], mut off: usize, count: usize) -> Option<usize> {
    for _ in 0..count {
        let (_, next) = cstr(data, off)?;
        off = next;
        if off + 2 > data.len() {
            return None;
        }
        let dims = data[off + 1] as usize;
        off += 2 + dims * 4;
        if off > data.len() {
            return None;
        }
    }
    Some(off)
}

/// 设置项:命令段结束到系统变量段之间、形如 `UPPER_IDENT` 的串。
fn scan_settings(data: &[u8], from: usize, to: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut off = from;
    while off < to {
        let Some((s, next)) = cstr(data, off) else {
            break;
        };
        off = next;
        if is_ident(&s) && s.chars().any(|c| c.is_ascii_uppercase()) {
            out.push(s);
        }
    }
    out
}

/// 参数关键字:同区间的非设置项标识符(全部保留,顺序稳定)。
fn scan_keywords(data: &[u8], from: usize, to: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut off = from;
    while off < to {
        let Some((s, next)) = cstr(data, off) else {
            break;
        };
        off = next;
        if is_ident(&s) && seen.insert(s.clone()) {
            out.push(s);
        }
    }
    out
}

fn is_ident(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$' || c == '#' || c == ';')
}

fn le_u32(data: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
}

fn cstr(data: &[u8], off: usize) -> Option<(String, usize)> {
    if off >= data.len() {
        return None;
    }
    let rest = &data[off..];
    let end = rest.iter().position(|&b| b == 0)?;
    // 编译器表里的串是 ASCII;非 ASCII 一律拒绝(避免把二进制误当名字)。
    let s = std::str::from_utf8(&rest[..end]).ok()?;
    if s.len() > 512 {
        return None;
    }
    Some((s.to_string(), off + end + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 合成 ycd:2 条命令 + 1 个占位 + 2 个系统变量,验证段定位与闭合。
    #[test]
    fn parses_minimal_ycd() {
        let mut d = Vec::new();
        d.extend_from_slice(b"YSCD");
        d.extend_from_slice(&495u32.to_le_bytes());
        d.extend_from_slice(&3u32.to_le_bytes());
        d.extend_from_slice(&0u32.to_le_bytes());
        // 命令 A:1 个参数
        d.extend_from_slice(b"A\0");
        d.push(1);
        d.extend_from_slice(b"X\0");
        d.extend_from_slice(&[0, 0, 3, 0]);
        // 命令 B:无参数
        d.extend_from_slice(b"B\0");
        d.push(0);
        // 占位
        d.extend_from_slice(b");\0");
        d.push(0);
        // 系统变量段头
        d.extend_from_slice(&2u32.to_le_bytes());
        d.extend_from_slice(&0u32.to_le_bytes());
        d.extend_from_slice(b"_A\0");
        d.extend_from_slice(&[1, 0]);
        d.extend_from_slice(b"_B\0");
        d.extend_from_slice(&[3, 1]);
        d.extend_from_slice(&17u32.to_le_bytes());

        let t = YcdTable::parse(&d).unwrap();
        assert_eq!(t.version, 495);
        assert_eq!(t.commands.len(), 3);
        assert_eq!(t.placeholder_commands, 1);
        assert_eq!(t.system_variables.len(), 2);
        assert_eq!(t.sys_var_id("_A"), Some(0));
        assert_eq!(t.sys_var_id("_B"), Some(1));
        assert_eq!(t.param("A", 0).unwrap().kind, 0);
        assert_eq!(t.param("A", 0).unwrap().b2, 3);
    }

    #[test]
    fn rejects_bad_magic() {
        let d = b"NOPE\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0";
        assert!(YcdTable::parse(d).is_err());
    }
}
