//! 工程表装载(R12 起 = `yu-format` 的**运行核适配层**)。
//!
//! # 解码唯一来源
//!
//! YSCM/YSLB/YSVR/YSTL/YSCF/YSTB 的容器头、魔数、字段解析与异或解密
//! 全部由 [`yu_format`] 完成(架构 `docs/architecture.md` §2:`RT → FMT`);
//! 本模块只做四件事:
//!
//! 1. **运行核投影**:把格式类型适配成运行核形状(`ParamKind`/`Label`/
//!    `VarRecord`/`GroupRec`/`WindowRec`),字段语义与旧实现逐一同源;
//! 2. **运行时专用索引**:`by_name`(命令名/标签字节键)、每脚本
//!    `label → script/pc`、脚本体共享池 `pool = args_data ‖ part4`;
//! 3. **错误码适配**:`yu-format` 的 `fmt.*` 诊断在本层包装为对外的
//!    `table.bad_magic` / `table.incomplete`(原诊断挂 `causes`,见
//!    [`wrap_table`]);缺表仍由 [`TableSet::load_from`] 的 `read` 报
//!    `table.missing`(A5:缺表拒绝启动)。
//! 4. YSCD 编译器配置不在 yu-format 范围,仍在 [`crate::ycd`]。
//!
//! # 实测布局(ERIS 官方 0.495 语料,137 YSTB + 7 元数据表)
//!
//! 布局事实与证据链见 `yu-format` 各模块文档(`yscm/yslb/ysvr/ystl/yscf/
//! ystb`);本文件不再复述字节布局。真值层级:官方产物/引擎 REA >
//! `yu-format` 解码结果 > 本层投影。
//!
//! # YSTL 字段勘误(R12)
//!
//! 旧实现把尾部第 3 个 u32 当 `text_count`、第 5 个当 `file_no`。按
//! `yu-format::ystl` 的字段定性(用户复核;`label_count` 与 ysl 逐条
//! 互证,见 yu-format 语料测试):第 3 = `var_count`、第 4 =
//! `label_count`、第 5 = `text_count`。[`ScriptEntry`] 已随之修正,
//! `SYSTEMINFO[TEXTNUM]` 从此返回真实文本条数。
//!
//! # XOR(脚本主体)
//!
//! 四区段异或(密钥 = `CRC32("Yu-ris")` 小端字节)由
//! `yu_format::ystb::Segments::decrypt` 完成;换键游戏不得猜:显式
//! `fmt.ybn.unsupported`/`fmt.ybn.key` + 包装挂账。

use std::collections::BTreeMap;

use yu_log::Boundary;

use crate::code;
use crate::diag;
use crate::value::ElemType;

/// 默认脚本密钥:`CRC32("Yu-ris")` 的小端字节。**协议常量**,不是游戏常量。
pub const DEFAULT_SCRIPT_KEY: [u8; 4] = yu_format::key::DEFAULT_KEY_BYTES;

// ---------------------------------------------------------------- 错误码适配

/// `yu-format` 诊断 → 对外表码(启动契约):
/// 魔数不符 = `table.bad_magic`,其余结构缺陷 = `table.incomplete`;
/// 原始 `fmt.*` 诊断整体进 `causes`(不吞错,也不改变调用方可判定性)。
fn wrap_table(what: &str, cause: yu_log::Diag) -> yu_log::Diag {
    let code = if cause.code == yu_format::code::MAGIC {
        code::table::BAD_MAGIC
    } else {
        code::table::INCOMPLETE
    };
    diag::fatal(code, Boundary::Table, format!("{what}: {}", cause.detail))
        .at_table(what)
        .caused_by(cause)
}

// ---------------------------------------------------------------- YSCM

/// YSCM 参数的种类码(引擎 `[0x801740]` 第一字节;旁线 §3.17)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamKind {
    /// 0 = 数字表达式。
    Numeric,
    /// 1 = 字符串(载荷在字符串池)。
    String,
    /// 3/8/5/4/6 = 变量引用(输出目标;`LET` 型参数)。
    ///
    /// 语料实测:`LET` 命令两个窗口的槽位种类恒为 0,而 `MATH/VARACT/
    /// VARINFO/SAVE/LOAD/TASKINFO/SYSTEMINFO/LABELINFO/FILEINFO` 的
    /// `LET`/`SET` 参数种类为 4/5/6/8。旁线 §3.17 把 `f1=5` 记为 `Let`,
    /// 本节以「是否为输出变量引用」统一归类。
    VariableRef,
    /// 2 = 数字 + 脚本内偏移(§3.17 `→2` 分支)。
    NumericOffset,
    /// 其它(未取证;保留原值)。
    Other(u8),
}

impl ParamKind {
    pub const fn from_code(b: u8) -> Self {
        match b {
            0 => ParamKind::Numeric,
            1 => ParamKind::String,
            2 => ParamKind::NumericOffset,
            4 | 5 | 6 | 8 => ParamKind::VariableRef,
            other => ParamKind::Other(other),
        }
    }

    pub const fn is_string(self) -> bool {
        matches!(self, ParamKind::String)
    }

    pub const fn is_var_ref(self) -> bool {
        matches!(self, ParamKind::VariableRef)
    }
}

/// YSCM 的一个参数声明。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParamDecl {
    pub name: String,
    /// 参数种类(引擎 `[0x801740]` 的第一字节)。
    pub kind: ParamKind,
    /// 第二字节:范围检查器编号(引擎 `[0x801744]`;0 = 无)。
    pub checker: u8,
}

/// YSCM 的一条命令。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandDecl {
    pub name: String,
    pub params: Vec<ParamDecl>,
}

/// `ysc.ybn` 命令表。**顺序即 opcode**(引擎 `DAT_007ace20[opcode]`,
/// 旁线 §2/§3.17)。解码由 `yu_format::yscm` 完成,本结构是其运行核投影。
#[derive(Clone, Debug)]
pub struct YscmTable {
    pub version: u32,
    pub commands: Vec<CommandDecl>,
    by_name: BTreeMap<String, usize>,
}

impl YscmTable {
    /// 从 `yu-format` 的命令表投影(字节解析唯一入口 = yu-format;R12)。
    ///
    /// 命名交叉注意:`yu_format::yscm::Arg` 的 `flag` 字节 = 运行核的
    /// 种类码(`ParamKind`),`kind` 字节 = 运行核的检查器号。两个 crate
    /// 的字段名恰好错位,以字节序为准。
    pub fn from_format(fmt: yu_format::yscm::CommandTable) -> Result<Self, yu_log::Diag> {
        let mut by_name = BTreeMap::new();
        let mut commands = Vec::with_capacity(fmt.commands.len());
        for (i, c) in fmt.commands.iter().enumerate() {
            if by_name.insert(c.name.to_string(), i).is_some() {
                return Err(diag::fatal(
                    code::table::INCOMPLETE,
                    Boundary::Table,
                    format!("ysc.ybn: 命令名重复 `{}`", c.name),
                )
                .at_table("ysc.ybn"));
            }
            commands.push(CommandDecl {
                name: c.name.to_string(),
                params: c
                    .args
                    .iter()
                    .map(|a| ParamDecl {
                        name: a.name.to_string(),
                        kind: ParamKind::from_code(a.flag),
                        checker: a.kind,
                    })
                    .collect(),
            });
        }
        Ok(Self {
            version: fmt.version,
            commands,
            by_name,
        })
    }

    pub fn command(&self, index: u8) -> Option<&CommandDecl> {
        self.commands.get(index as usize)
    }

    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.by_name.get(name).copied()
    }

    pub fn len(&self) -> usize {
        self.commands.len()
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// 参数声明(按参数序号)。
    pub fn param(&self, index: u8, slot: u8) -> Option<&ParamDecl> {
        self.command(index)?.params.get(slot as usize)
    }
}

// ---------------------------------------------------------------- YSLB

/// 标签记录(引擎 0x10 字节结构;文件尾部 13 字节,旁线 §3.5)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label {
    /// 名字原始字节(CP932;引擎按字节 strcmp 比较,大小写敏感 —— 旁线 §3.5b)。
    pub name_bytes: Vec<u8>,
    /// 名字的 lossy 文本(仅显示/诊断)。
    pub name: String,
    /// 名字的 MurmurHash2(seed 0),引擎查表先比它(文件 `field_a`)。
    pub hash: u32,
    /// 目标组号(pc;文件 `field_b`,yu-format 语料测试证其在指令数内)。
    pub pc: u32,
    /// 目标脚本号(文件 `script_no`)。
    pub script_id: u16,
    /// 变量域基址 A(`task+0x3e`;0..0x40;文件 `field_d`)。
    pub base_a: u8,
    /// 变量域基址 B(`task+0x244`;文件里为 0 —— 引擎装载器不写,旁线 §3.5)。
    pub base_b: u8,
}

/// `ysl.ybn` 标签表(解码 = `yu_format::yslb`;本结构 = 投影 + 字节键索引)。
#[derive(Clone, Debug)]
pub struct LabelTable {
    pub version: u32,
    pub labels: Vec<Label>,
    by_name: BTreeMap<Vec<u8>, usize>,
}

impl LabelTable {
    /// 从 `yu-format` 的标签表投影(字节解析唯一入口 = yu-format;R12)。
    pub fn from_format(fmt: yu_format::yslb::YslLabels) -> Result<Self, yu_log::Diag> {
        let mut by_name = BTreeMap::new();
        let mut labels = Vec::with_capacity(fmt.entries.len());
        for (i, e) in fmt.entries.iter().enumerate() {
            if by_name.insert(e.name.clone(), i).is_some() {
                return Err(diag::fatal(
                    code::table::INCOMPLETE,
                    Boundary::Table,
                    format!("ysl.ybn: 标签名重复 `{}`", String::from_utf8_lossy(&e.name)),
                )
                .at_table("ysl.ybn"));
            }
            labels.push(Label {
                name_bytes: e.name.clone(),
                name: String::from_utf8_lossy(&e.name).into_owned(),
                hash: e.field_a,
                pc: e.field_b,
                script_id: e.script_no,
                base_a: e.field_d,
                base_b: e.field_e,
            });
        }
        Ok(Self {
            version: fmt.version,
            labels,
            by_name,
        })
    }

    /// 按原始字节查(引擎语义:字节级、大小写敏感)。
    pub fn get_bytes(&self, name: &[u8]) -> Option<&Label> {
        self.by_name.get(name).map(|&i| &self.labels[i])
    }

    /// 按文本查(仅 ASCII 名等值于字节查;用于测试/诊断)。
    pub fn get(&self, name: &str) -> Option<&Label> {
        self.by_name.get(name.as_bytes()).map(|&i| &self.labels[i])
    }

    pub fn len(&self) -> usize {
        self.labels.len()
    }

    pub fn is_empty(&self) -> bool {
        self.labels.is_empty()
    }
}

// ---------------------------------------------------------------- YSVR

/// YSVR 初值。
#[derive(Clone, Debug, PartialEq)]
pub enum VarInit {
    Int(i64),
    Float(f64),
    Str(Vec<u8>),
    /// `value_type == 0`:仅声明,无初值。
    None,
}

/// YSVR 一条变量定义记录。
#[derive(Clone, Debug, PartialEq)]
pub struct VarRecord {
    /// 1 = 全局/静态(引擎一次装载)/ 2 = 局部(随脚本装载)/ 3 = 静态
    /// (旁线 `format-ybn.md` §10:`-1` 过滤收 `{1,3}`,文件号过滤收 `2`)。
    pub kind: u8,
    /// 恢复组号 1..4(`G_INT`..`G_INT4`;格式层字段名 `flag`)。
    pub group: u8,
    /// 局部变量的所属脚本号(格式层字段名 `file_id`)。
    pub script_id: u16,
    /// 变量槽 id(格式层字段名 `slot`)。
    pub var_id: u16,
    /// 值类型 1/2/3;0 = 无初值(格式层字段名 `ty`)。
    pub value_type: u8,
    pub dims: Vec<u32>,
    pub init: VarInit,
}

/// `ysv.ybn` 变量镜像(解码 = `yu_format::ysvr`;本结构 = 运行核投影)。
#[derive(Clone, Debug)]
pub struct VarImage {
    pub version: u32,
    pub records: Vec<VarRecord>,
}

impl VarImage {
    /// 从 `yu-format` 的变量声明投影(字节解析唯一入口 = yu-format;R12)。
    pub fn from_format(fmt: yu_format::ysvr::VarDecls) -> Result<Self, yu_log::Diag> {
        use yu_format::Init;
        let records = fmt
            .decls
            .iter()
            .map(|d| VarRecord {
                kind: d.kind,
                group: d.flag,
                script_id: d.file_id,
                var_id: d.slot,
                value_type: d.ty,
                dims: d.dims.clone(),
                init: match &d.init {
                    Init::None => VarInit::None,
                    Init::Int(v) => VarInit::Int(*v),
                    Init::Float(v) => VarInit::Float(*v),
                    Init::Str(s) => VarInit::Str(s.clone()),
                },
            })
            .collect();
        Ok(Self {
            version: fmt.version,
            records,
        })
    }

    /// 全局/静态记录(引擎 `file == -1` 过滤:kind ∈ {1,3})。
    pub fn globals(&self) -> impl Iterator<Item = &VarRecord> {
        self.records.iter().filter(|r| matches!(r.kind, 1 | 3))
    }

    /// 某脚本的局部记录(引擎按文件号过滤:kind == 2 且 script_id 相等)。
    pub fn locals_of(&self, script_id: u16) -> impl Iterator<Item = &VarRecord> {
        self.records
            .iter()
            .filter(move |r| r.kind == 2 && r.script_id == script_id)
    }
}

impl VarRecord {
    pub fn elem_type(&self) -> Option<ElemType> {
        ElemType::from_code(self.value_type)
    }

    pub fn init_value(&self) -> Option<crate::value::Value> {
        match &self.init {
            VarInit::Int(v) => Some(crate::value::Value::Int(*v)),
            VarInit::Float(v) => Some(crate::value::Value::Float(*v)),
            VarInit::Str(v) => Some(crate::value::Value::Str(decode_str_init(v))),
            VarInit::None => None,
        }
    }
}

/// `YSVR` 字符串初值 → 运行值。
///
/// 格式层按契约保留原始字节(`yu-format/src/ysvr.rs`:解释归运行核);其形态是
/// **一条 YSTB 表达式指令**:`0x4d('M') + u16 长度 + "文本"`。这里直接复用
/// 表达式层的字面量解码,统一处理引号、`\\`/`\n`/`\t` 转义与 CP932 前导字节,
/// 避免在表投影层另写一套不完整的字符串解析。
///
/// 非该形态(或损坏)的记录原样返回:YSVR 只有这一种已取证形态,其余不猜测。
fn decode_str_init(raw: &[u8]) -> Vec<u8> {
    let Ok(instrs) = crate::expr::decode(raw) else {
        return raw.to_vec();
    };
    let [ins] = instrs.as_slice() else {
        return raw.to_vec();
    };
    if ins.op != 0x4d {
        return raw.to_vec();
    }
    crate::expr::decode_string_literal(&ins.payload).unwrap_or_else(|_| raw.to_vec())
}

// ---------------------------------------------------------------- YSTL

/// `yst_list.ybn` 的一条脚本清单项(解码 = `yu_format::ystl`)。
///
/// 字段定性(R12 勘误,见模块文档):尾部 20 字节 =
/// `file_time(8) + var_count + label_count + text_count`;
/// `label_count` 与 ysl 逐条互证(yu-format 语料测试),`text_count` 是
/// 该脚本显示文本条数(`SYSTEMINFO[TEXTNUM]` 查它)。旧实现的
/// `meta`/`file_no` 命名错位,已删除。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptEntry {
    pub index: u32,
    /// 源码路径(CP932 字节,原样保留;格式层字段名 `name`)。
    pub path: Vec<u8>,
    /// 源文件修改时间(FILETIME 64 位拼合)。
    pub file_time: u64,
    /// 该脚本变量声明数(无脚本产物时 = `u32::MAX` 哨兵)。
    pub var_count: u32,
    /// 该脚本标签数(与 ysl 逐条互证;无脚本产物时 = `u32::MAX`)。
    pub label_count: u32,
    /// 该脚本显示文本数(无脚本产物时 = 0)。
    pub text_count: u32,
}

/// `yst_list.ybn` 脚本清单(解码 = `yu_format::ystl`;本结构 = 投影)。
#[derive(Clone, Debug)]
pub struct ScriptList {
    pub version: u32,
    pub entries: Vec<ScriptEntry>,
}

impl ScriptList {
    /// 从 `yu-format` 的脚本清单投影(字段定性以 yu-format 为准,见
    /// [`ScriptEntry`] 文档;字节解析唯一入口 = yu-format;R12)。
    pub fn from_format(fmt: yu_format::ystl::ScriptList) -> Result<Self, yu_log::Diag> {
        let entries = fmt
            .entries
            .iter()
            .map(|e| ScriptEntry {
                index: e.index,
                path: e.name.clone(),
                file_time: e.file_time,
                var_count: e.var_count,
                label_count: e.label_count,
                text_count: e.text_count,
            })
            .collect();
        Ok(Self {
            version: fmt.version,
            entries,
        })
    }

    /// 脚本号 → 文本条数(`SYSTEMINFO[TEXTNUM]` 的数据源)。
    pub fn text_count(&self, script_id: u16) -> Option<u32> {
        self.entries.get(script_id as usize).map(|e| e.text_count)
    }

    /// 脚本号 → 源码路径(脚本号即清单下标;见 [`Self::entries`] 的顺序)。
    pub fn path_of(&self, script_id: u16) -> Option<&[u8]> {
        self.entries
            .get(script_id as usize)
            .map(|e| e.path.as_slice())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ---------------------------------------------------------------- YSTB

/// 一条命令实例(解密后指令索引区的类型化视图;0.495 实测)。
///
/// 字段语义(旁线 `format-ybn.md` §3 + 本仓库语料实测):
/// - `opcode`:YSCM 下标;
/// - `window_count`:本组窗口数(`Σ window_count × 12 == 参数项区长度`,
///   137/137 文件成立);
/// - `param`:命令参数 u16(声明/调用类命令编码数组维数位图;`GOSUB` 实测
///   `param == 参数项数 - 1`,见 [`ScriptFile::group_windows`])。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroupRec {
    pub opcode: u8,
    pub window_count: u8,
    pub param: u16,
}

/// 12 字节窗口记录。
///
/// ```text
/// +0 u8  kind   ← 该命令的参数序号(0-based;旁线 §3.2c 更正)
/// +1 u8  flag
/// +2 u16 slot
/// +4 u32 length  ← 载荷长度;对 IF/ELSE/LOOP 等跳转槽 = 目标组号
/// +8 u32 offset  ← 载荷在 (content ‖ part4) 共享池内的偏移
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowRec {
    pub kind: u8,
    pub flag: u8,
    pub slot: u16,
    pub length: u32,
    pub offset: u32,
}

impl WindowRec {
    pub fn from_bytes(b: &[u8]) -> Self {
        Self {
            kind: b[0],
            flag: b[1],
            slot: u16::from_le_bytes([b[2], b[3]]),
            length: u32::from_le_bytes([b[4], b[5], b[6], b[7]]),
            offset: u32::from_le_bytes([b[8], b[9], b[10], b[11]]),
        }
    }
}

/// 已解码脚本体(解密/自检 = `yu_format::ystb::Segments`;本结构 =
/// 区段的类型化视图 + 运行核索引)。引擎按 `ysl` 直接落 pc,不用
/// `yu_format::DecodedScript` 的静态标签表,故运行核取 `Segments` 视图。
#[derive(Clone, Debug)]
pub struct ScriptFile {
    pub script_id: u32,
    pub version: u32,
    pub groups: Vec<GroupRec>,
    pub windows: Vec<WindowRec>,
    /// 每组的首窗口下标(累计 `window_count`)。
    first_window: Vec<usize>,
    /// 共享池 = args_data ‖ line_numbers(content ‖ part4)。
    pool: Vec<u8>,
    /// 数据区 B(旁线:任务 `+0x38` 起按 pc 做 dword 索引 = 源码位置)。
    part4: Vec<u8>,
}

impl ScriptFile {
    /// 解析单个 `yst%05d.ybn`(解密 + 区段自检由 yu-format 完成)。
    pub fn parse(script_id: u32, data: &[u8], key: [u8; 4]) -> Result<Self, yu_log::Diag> {
        let origin = format!("ysbin/yst{script_id:05}.ybn");
        let segs = yu_format::ystb::Segments::decrypt(data, &key, origin.as_bytes())
            .map_err(|d| wrap_table(&origin, d))?;
        Self::from_segments(script_id, segs)
    }

    /// 从 `yu-format` 的解密区段投影:组/窗口记录、共享池与 part4。
    ///
    /// 结构自检(`Σ window_count × 12 == 参数索引区长度`、非零 type 窗口的
    /// `offset+size` 边界)已在 `Segments::decrypt` 内完成,这里不再重复。
    fn from_segments(
        script_id: u32,
        segs: yu_format::ystb::Segments,
    ) -> Result<Self, yu_log::Diag> {
        let count = segs.count as usize;
        let mut groups = Vec::with_capacity(count);
        let mut first_window = Vec::with_capacity(count);
        let mut acc = 0usize;
        for i in 0..count {
            let b = &segs.inst[i * 4..i * 4 + 4];
            let g = GroupRec {
                opcode: b[0],
                window_count: b[1],
                param: u16::from_le_bytes([b[2], b[3]]),
            };
            first_window.push(acc);
            acc += g.window_count as usize;
            groups.push(g);
        }
        let windows: Vec<WindowRec> = segs
            .args_index
            .chunks_exact(12)
            .map(WindowRec::from_bytes)
            .collect();
        let mut pool = segs.args_data;
        pool.extend_from_slice(&segs.line_numbers);
        Ok(Self {
            script_id,
            version: segs.version,
            groups,
            windows,
            first_window,
            pool,
            part4: segs.line_numbers,
        })
    }

    pub fn group_count(&self) -> usize {
        self.groups.len()
    }

    pub fn group(&self, pc: usize) -> Option<&GroupRec> {
        self.groups.get(pc)
    }

    /// 某组的窗口记录切片。
    pub fn group_windows(&self, pc: usize) -> &[WindowRec] {
        let Some(&start) = self.first_window.get(pc) else {
            return &[];
        };
        let n = self.groups[pc].window_count as usize;
        &self.windows[start..start + n]
    }

    /// 窗口载荷(共享池切片;越界返回 `None`,由调用方报 `fmt.out_of_bounds`)。
    pub fn window_payload(&self, w: &WindowRec) -> Option<&[u8]> {
        let start = w.offset as usize;
        let end = start.checked_add(w.length as usize)?;
        self.pool.get(start..end)
    }

    /// 数据区 B 的原始字节。
    pub fn part4(&self) -> &[u8] {
        &self.part4
    }

    /// 共享池(content ‖ part4)原始字节(诊断/倾倒工具用)。
    pub fn pool(&self) -> &[u8] {
        &self.pool
    }

    /// 脚本内源码位置(`task+0x38` 起按 pc 取 dword;旁线 §3.2 表)。
    ///
    /// 注意:该区在 0.495 语料里长度等于 part1(实测 137/137
    /// `part4_len == part1_len`),语义为「逐组源码行号/位置」——本仓库
    /// **只原样提供**,不参与执行语义。
    pub fn source_position(&self, pc: usize) -> Option<u32> {
        let off = pc * 4;
        let b = self.part4.get(off..off + 4)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
}

// ---------------------------------------------------------------- 装配

/// 启动所需全部表。
#[derive(Clone, Debug)]
pub struct BootTables {
    pub yscm: YscmTable,
    pub labels: LabelTable,
    pub vars: VarImage,
    pub scripts: ScriptList,
    /// 引擎设置(直接采用 `yu_format::yscf::Yscf`;宽高为 **u32**,
    /// 引擎 `FUN_0045dbb0` 按 dword 读并做无符号范围判定)。
    pub config: yu_format::yscf::Yscf,
    /// 编译器语言定义(**可选**:商业发行版不带编译器)。
    pub ycd: Option<crate::ycd::YcdTable>,
    /// 系统变量 ABI 表(名字/类型/维数;顺序即 id)。
    ///
    /// 来源优先级:工程 `YSCom.ycd` → 内嵌引擎 ABI 数据表;两者都用工程
    /// 自己的 `ysv.ybn` 逐项校验([`crate::sysvar_abi::SysVarTable::verify_against_ysvr`])。
    pub sysvars: crate::sysvar_abi::SysVarTable,
}

/// 表集合 + 已解码脚本。缺表即拒绝启动(A5)。
///
/// `Clone` 供测试构造“字段被清零”的内存快照(如 A5 的探测序拒绝用例);
/// 生产路径只持有一份。
#[derive(Clone)]
pub struct TableSet {
    pub boot: BootTables,
    pub scripts: BTreeMap<u32, ScriptFile>,
    /// 已加载脚本的源码路径(诊断用;CP932 字节)。
    pub script_paths: BTreeMap<u32, Vec<u8>>,
    pub key: [u8; 4],
}

impl TableSet {
    /// 从数据源装载全部表。
    ///
    /// - `ysbin/` 固定为编译产物目录(引擎 `sprintf("ysbin\\yst%05d.ybn")`,
    ///   旁线 §4)——这是**协议路径**,不是游戏常量。
    /// - `system/YSCom/YSCom.ycd` 是编译器语言定义(工程自带)。
    /// - 缺任一张表 → `table.missing`,装载失败(拒绝启动)。
    ///
    /// 单根装载(`ysbin/*` 与 `system/YSCom/YSCom.ycd` 同根)。
    pub fn load(src: &dyn yu_vfs::DataSource, key: [u8; 4]) -> Result<Self, yu_log::Diag> {
        Self::load_from(src, None, key)
    }

    /// 双根装载:**编译产物在工程数据目录,编译器表在引擎/SDK 目录**。
    ///
    /// 现实布局(A1 的散盘形态):
    /// - 散盘工程:`<project>/data/ysbin/*.ybn` + `<project>/system/YSCom/YSCom.ycd`;
    /// - 官方 SDK:`<sdk>/システム/data/...`(无 ysbin)+ `<sdk>/システム/system/YSCom/`.
    ///
    /// 所以 `ysbin/*` 从 `src` 读,`YSCom.ycd` 先试 `src`、再试 `aux`
    /// (SDK 目录或工程根)。两处都没有 ⇒ `table.missing`,拒绝启动
    /// (系统变量名没有第二个一手来源,见 DIV-RT-016)。
    pub fn load_from(
        src: &dyn yu_vfs::DataSource,
        aux: Option<&dyn yu_vfs::DataSource>,
        key: [u8; 4],
    ) -> Result<Self, yu_log::Diag> {
        let yscm = YscmTable::from_format(
            yu_format::yscm::CommandTable::parse(
                &read(src, b"ysbin/ysc.ybn", "ysc")?,
                b"ysbin/ysc.ybn",
            )
            .map_err(|d| wrap_table("ysc.ybn", d))?,
        )?;
        let labels = LabelTable::from_format(
            yu_format::yslb::YslLabels::parse(
                &read(src, b"ysbin/ysl.ybn", "ysl")?,
                b"ysbin/ysl.ybn",
            )
            .map_err(|d| wrap_table("ysl.ybn", d))?,
        )?;
        let vars = VarImage::from_format(
            yu_format::ysvr::VarDecls::parse(
                &read(src, b"ysbin/ysv.ybn", "ysv")?,
                b"ysbin/ysv.ybn",
            )
            .map_err(|d| wrap_table("ysv.ybn", d))?,
        )?;
        let scripts = ScriptList::from_format(
            yu_format::ystl::ScriptList::parse(
                &read(src, b"ysbin/yst_list.ybn", "yst_list")?,
                b"ysbin/yst_list.ybn",
            )
            .map_err(|d| wrap_table("yst_list.ybn", d))?,
        )?;
        let config = yu_format::yscf::Yscf::parse(
            &read(src, b"ysbin/yscfg.ybn", "yscfg")?,
            b"ysbin/yscfg.ybn",
        )
        .map_err(|d| wrap_table("yscfg.ybn", d))?;
        // 编译器定义(可选):商业包不带它。
        let ycd_bytes = match read(src, b"system/YSCom/YSCom.ycd", "YSCom.ycd") {
            Ok(b) => Some((b, "system/YSCom/YSCom.ycd".to_string())),
            Err(primary) => match aux {
                Some(aux) => match read(aux, b"system/YSCom/YSCom.ycd", "YSCom.ycd (aux)") {
                    Ok(b) => Some((b, "system/YSCom/YSCom.ycd (aux)".to_string())),
                    Err(_) => {
                        // 商业发行版:没有编译器定义不是错误(DIV-RT-016)。
                        let _ = primary;
                        None
                    }
                },
                None => None,
            },
        };
        let (ycd, sysvars) = match ycd_bytes {
            Some((bytes, path)) => {
                let table = crate::ycd::YcdTable::parse(&bytes)?;
                let sha = crate::sysvar_abi::sha256_hex(&bytes);
                let sysvars = crate::sysvar_abi::SysVarTable::from_ycd(&table, path, sha);
                (Some(table), sysvars)
            }
            // 商业发行版无编译器定义:按工程 ysv.ybn 指纹从已知 ABI 族中
            // 选表(数据驱动,不看版本号;全不匹配 ⇒ 拒绝启动)。
            None => (None, crate::sysvar_abi::SysVarTable::select_for(&vars)?),
        };

        // 系统变量 ABI 校验(两条独立来源交叉一致才放行):
        // 0.495 SDK 实测 548 项 ↔ ysv 前 548 条 kind=1 逐项吻合;
        // 0.494 商业包实测同样 548/548 吻合(DIV-RT-016)。
        sysvars.verify_against_ysvr(&vars)?;
        // ② 运行核语义依赖的命令名必须能在工程表里解析(分派按名字;
        //    这份名字集就是处理器注册表本身,见 `vm::handlers::required_commands`)。
        crate::vm::handlers::required_commands(&yscm)?;

        let mut script_paths = BTreeMap::new();
        for e in &scripts.entries {
            script_paths.insert(e.index, e.path.clone());
        }
        Ok(Self {
            boot: BootTables {
                yscm,
                labels,
                vars,
                scripts,
                config,
                ycd,
                sysvars,
            },
            scripts: BTreeMap::new(),
            script_paths,
            key,
        })
    }

    /// 按需装载脚本(`ysbin/yst%05d.ybn`)。
    pub fn load_script(
        &mut self,
        src: &dyn yu_vfs::DataSource,
        script_id: u32,
    ) -> Result<&ScriptFile, yu_log::Diag> {
        if !self.scripts.contains_key(&script_id) {
            let path = format!("ysbin/yst{script_id:05}.ybn");
            let bytes = read(src, path.as_bytes(), "脚本")?;
            let file = ScriptFile::parse(script_id, &bytes, self.key)?;
            self.scripts.insert(script_id, file);
        }
        Ok(self.scripts.get(&script_id).expect("刚刚插入"))
    }

    pub fn script(&self, script_id: u32) -> Option<&ScriptFile> {
        self.scripts.get(&script_id)
    }

    pub fn script_path(&self, script_id: u32) -> String {
        self.script_paths
            .get(&script_id)
            .map(|p| String::from_utf8_lossy(p).into_owned())
            .unwrap_or_else(|| format!("yst{script_id:05}.ybn"))
    }
}

fn read(src: &dyn yu_vfs::DataSource, path: &[u8], what: &str) -> Result<Vec<u8>, yu_log::Diag> {
    let key = yu_vfs::LookupKey::new(path, yu_vfs::Codec::Cp932).map_err(|d| {
        diag::fatal(
            code::table::MISSING,
            Boundary::Table,
            format!("{what}: 路径非法({})", String::from_utf8_lossy(path)),
        )
        .caused_by(d)
    })?;
    src.open(&key).map_err(|d| {
        diag::fatal(
            code::table::MISSING,
            Boundary::Table,
            format!(
                "{what}: 必需表不存在 `{}`(A5:缺表拒绝启动)",
                String::from_utf8_lossy(path)
            ),
        )
        .caused_by(d)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R12 适配层最小夹具:合成 YSTB 走 `yu_format::Segments` 解密,
    /// 投影后的组/窗口/池与构造时逐字节一致。
    #[test]
    fn script_file_projection_roundtrip() {
        // 一条 GO 组 + 一个 'M' 形载荷窗口。
        let mut payload = vec![b'M'];
        payload.extend_from_slice(&7u16.to_le_bytes());
        payload.extend_from_slice(b"\"x\"");
        let mut windows = Vec::new();
        windows.extend_from_slice(&[43u8, 0]); // kind=43, flag=0
        windows.extend_from_slice(&1u16.to_le_bytes()); // slot=1
        windows.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        windows.extend_from_slice(&0u32.to_le_bytes());
        let mut inst = vec![43u8, 1, 0, 0];
        inst.extend_from_slice(&(windows.len() as u32).to_le_bytes());
        let mut data = Vec::new();
        data.extend_from_slice(b"YSTB");
        data.extend_from_slice(&495u32.to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&(inst.len() as u32).to_le_bytes());
        data.extend_from_slice(&(windows.len() as u32).to_le_bytes());
        data.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        data.extend_from_slice(&4u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        let k = [0x11u8, 0x22, 0x33, 0x44];
        let xor = |d: &[u8]| {
            d.iter()
                .enumerate()
                .map(|(i, b)| b ^ k[i % 4])
                .collect::<Vec<u8>>()
        };
        let line_no = 9u32.to_le_bytes();
        data.extend_from_slice(&xor(&inst));
        data.extend_from_slice(&xor(&windows));
        data.extend_from_slice(&xor(&payload));
        data.extend_from_slice(&xor(line_no.as_slice()));
        let sf = ScriptFile::parse(7, &data, k).expect("合成 YSTB 投影");
        assert_eq!(sf.version, 495);
        assert_eq!(sf.group_count(), 1);
        assert_eq!(sf.group(0).unwrap().opcode, 43);
        assert_eq!(sf.windows.len(), 1);
        let w = sf.group_windows(0)[0];
        assert_eq!((w.kind, w.slot, w.length), (43, 1, payload.len() as u32));
        assert_eq!(sf.window_payload(&w), Some(payload.as_slice()));
        assert_eq!(sf.part4(), line_no.as_slice());
        assert_eq!(sf.source_position(0), Some(9));
    }

    /// 真实形态:`M` + u16 长度(含引号)+ 引号串。
    #[test]
    fn ysvr_string_init_decodes_ystb_literal() {
        assert_eq!(
            decode_str_init(b"M\x10\x00\"SCENARIO_START\""),
            b"SCENARIO_START"
        );
        assert_eq!(
            decode_str_init(b"M\x13\x00\"es.ScenarioScript\""),
            b"es.ScenarioScript"
        );
        // 空串:`M` + len=2 + `""`
        assert_eq!(decode_str_init(b"M\x02\x00\"\""), b"");
    }

    /// 字面量转义走表达式层同一套规则(`\n` / `\\` / `\t`)。
    #[test]
    fn ysvr_string_init_keeps_escapes() {
        assert_eq!(decode_str_init(b"M\x06\x00\"a\\nb\""), b"a\nb");
        assert_eq!(decode_str_init(b"M\x06\x00\"a\\\\b\""), b"a\\b");
    }

    /// 非 YSTB 字面量形态:原样保留,不猜测。
    #[test]
    fn ysvr_string_init_non_literal_is_verbatim() {
        assert_eq!(decode_str_init(b"plain"), b"plain");
        assert_eq!(decode_str_init(b"M\x00\x00"), b"M\x00\x00");
    }
}
