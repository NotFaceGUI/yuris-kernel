//! 测试夹具:合成一个完整的 YU-RIS 工程(全部表 + 脚本 + 编译器定义)。
//!
//! 目的:让运行核的每条行为都能在**已知输入**上断言,同时保持与真实语料
//! 同一套解析路径(`TableSet::load` / `ScriptFile::parse` / `YcdTable::parse`)。
//!
//! 合成数据严格按真实布局(`docs/` 与 `crates/yu-rt/src/tables.rs` 的实测笔记):
//! YSCM 命令表、YSLB 标签表(0x40C 起、13 字节步长、MurmurHash2)、
//! YSVR(0x0A 起、u16 count)、YSTL(0x0C 起、0x1C+名长)、YSCF、YSTB(四段异或)、
//! YSCD(命令段 + 系统变量段 + 消息段)。

#![allow(dead_code)]

use yu_rt::tables::DEFAULT_SCRIPT_KEY;
use yu_rt::value::VarRef;
use yu_vfs::{Codec, MemSource};

/// 合成命令声明:`(名字, [(参数名, 种类, 检查器)])`。
pub type CommandSpec = (&'static str, Vec<(&'static str, u8, u8)>);

/// 合成标签:`(名字, pc, script_id, base_a)`。
pub type LabelSpec = (&'static str, u32, u16, u8);

/// 合成变量记录:`(kind, group, script_id, var_id, value_type, dims, init)`。
#[derive(Clone, Debug)]
pub enum Init {
    Int(i64),
    Float(f64),
    Str(&'static [u8]),
    None,
}

#[derive(Clone, Debug)]
pub struct VarSpec {
    pub kind: u8,
    pub group: u8,
    pub script_id: u16,
    pub var_id: u16,
    pub value_type: u8,
    pub dims: Vec<u32>,
    pub init: Init,
}

/// 一个窗口声明:`(kind, flag, slot, length, payload)`。
pub type WindowSpec = (u8, u8, u16, Option<u32>, Vec<u8>);

/// 合成一条命令组 + 其窗口。
#[derive(Clone, Debug)]
pub struct GroupSpec {
    pub opcode: u8,
    /// `(kind, flag, slot, length, payload)`。`length` 由 payload 长度自动算,
    /// 但**跳转窗口**要用 `jump_len` 显式给组号。
    pub windows: Vec<WindowSpec>,
}

impl GroupSpec {
    pub fn new(opcode: u8) -> Self {
        Self {
            opcode,
            windows: Vec::new(),
        }
    }

    pub fn window(mut self, kind: u8, payload: Vec<u8>) -> Self {
        self.windows.push((kind, 0, 1, None, payload));
        self
    }

    /// 跳转窗口:`length` = 目标组号(引擎语义,payload 通常为空)。
    pub fn jump(mut self, target_pc: u32) -> Self {
        self.windows.push((0, 0, 0, Some(target_pc), Vec::new()));
        self
    }

    /// 带 flag 的窗口(LET 复合赋值码在 `slot` 高字节)。
    pub fn window_tagged(mut self, kind: u8, flag: u8, slot: u16, payload: Vec<u8>) -> Self {
        self.windows.push((kind, flag, slot, None, payload));
        self
    }
}

/// 表达式构造助手(与 `yu_rt::expr` 的记录格式一致)。
pub mod ex {
    pub fn i8_(v: i8) -> Vec<u8> {
        vec![0x42, 1, 0, v as u8]
    }
    pub fn i16_(v: i16) -> Vec<u8> {
        let b = v.to_le_bytes();
        vec![0x57, 2, 0, b[0], b[1]]
    }
    pub fn i32_(v: i32) -> Vec<u8> {
        let b = v.to_le_bytes();
        vec![0x49, 4, 0, b[0], b[1], b[2], b[3]]
    }
    pub fn i64_(v: i64) -> Vec<u8> {
        let b = v.to_le_bytes();
        let mut out = vec![0x4c, 8, 0];
        out.extend_from_slice(&b);
        out
    }
    pub fn f64_(v: f64) -> Vec<u8> {
        let b = v.to_le_bytes();
        let mut out = vec![0x46, 8, 0];
        out.extend_from_slice(&b);
        out
    }
    /// 字符串字面量(载荷含界定符,引擎 0x4d 语义)。
    pub fn s(text: &str) -> Vec<u8> {
        let mut body = vec![b'"'];
        body.extend_from_slice(text.as_bytes());
        body.push(b'"');
        let mut out = vec![0x4d];
        out.extend_from_slice(&(body.len() as u16).to_le_bytes());
        out.extend_from_slice(&body);
        out
    }
    /// 取变量值(0x48)。
    pub fn get(space: u8, id: u16) -> Vec<u8> {
        let b = id.to_le_bytes();
        vec![0x48, 3, 0, space, b[0], b[1]]
    }
    /// 取变量引用(0x56)。
    pub fn r#ref(space: u8, id: u16) -> Vec<u8> {
        let b = id.to_le_bytes();
        vec![0x56, 3, 0, space, b[0], b[1]]
    }
    /// 数组取值(0x29)。
    pub fn aload() -> Vec<u8> {
        vec![0x29, 1, 0, 0]
    }
    pub fn op(code: u8) -> Vec<u8> {
        vec![code, 0, 0]
    }
    pub fn cat(parts: &[Vec<u8>]) -> Vec<u8> {
        let mut out = Vec::new();
        for p in parts {
            out.extend_from_slice(p);
        }
        out
    }
}

/// 便捷:构造带用户整型变量的工程(变量 id 从系统变量数起)。
pub fn with_int_var(b: &mut ProjectBuilder, offset: u16, init: i64) -> VarRef {
    let sys = b.sysvars.len() as u16;
    b.vars.push(VarSpec {
        kind: 1,
        group: 0,
        script_id: 0,
        var_id: sys + offset,
        value_type: 1,
        dims: vec![],
        init: Init::Int(init),
    });
    yu_rt::value::VarRef::new(yu_rt::value::VarSpace::At, sys + offset)
}

/// 工程构建器。
pub struct ProjectBuilder {
    pub commands: Vec<CommandSpec>,
    pub sysvars: Vec<(&'static str, u8, Vec<u32>)>,
    pub labels: Vec<LabelSpec>,
    pub vars: Vec<VarSpec>,
    pub scripts: Vec<Vec<GroupSpec>>,
    pub entry_script: u16,
    pub entry_pc: u32,
    /// 用内嵌引擎 ABI 的 548 项系统变量(模拟商业包:没有 ycd 时运行核回退到它)。
    pub sysvars_abi: bool,
    /// 故意省略的表(测"缺表拒启"):`"ysc" | "ysl" | "ysv" | "yst_list" | "yscfg" | "ycd"`。
    pub omit: Vec<&'static str>,
}

impl ProjectBuilder {
    /// 默认命令表:序号与 0.495 的 `ysc.ybn` **一致**(仅测试夹具用;
    /// 生产路径永远读工程表)。
    pub fn new() -> Self {
        Self {
            commands: default_commands(),
            sysvars: default_sysvars(),
            labels: Vec::new(),
            vars: Vec::new(),
            scripts: Vec::new(),
            entry_script: 0,
            entry_pc: 0,
            sysvars_abi: false,
            omit: Vec::new(),
        }
    }

    pub fn with_labels(mut self, labels: Vec<LabelSpec>) -> Self {
        self.labels = labels;
        self
    }

    pub fn with_vars(mut self, vars: Vec<VarSpec>) -> Self {
        self.vars = vars;
        self
    }

    pub fn with_scripts(mut self, scripts: Vec<Vec<GroupSpec>>) -> Self {
        self.scripts = scripts;
        self
    }

    /// 设置入口(就地修改,便于先建 builder 再解析 sysvar id)。
    pub fn entry(&mut self, script: u16, pc: u32) -> &mut Self {
        self.entry_script = script;
        self.entry_pc = pc;
        self
    }

    /// YSVR 全量记录:先放系统变量的镜像声明(id = ycd 顺序),
    /// 再放用户声明的变量(id ≥ 系统变量项数)。
    /// 实际使用的系统变量声明(商业形态用内嵌 ABI 的 548 项)。
    pub fn effective_sysvars(&self) -> Vec<(String, u8, Vec<u32>)> {
        if self.sysvars_abi {
            yu_rt::sysvar_abi::SysVarTable::builtin_abi()
                .decls
                .into_iter()
                .map(|d| (d.name, d.value_type, d.dims))
                .collect()
        } else {
            self.sysvars
                .iter()
                .map(|(n, t, d)| (n.to_string(), *t, d.clone()))
                .collect()
        }
    }

    fn all_vars(&self) -> Vec<VarSpec> {
        let mut out: Vec<VarSpec> = self
            .effective_sysvars()
            .iter()
            .enumerate()
            .map(|(i, (_, ty, dims))| VarSpec {
                kind: 1,
                group: 0,
                script_id: 0,
                var_id: i as u16,
                value_type: *ty,
                dims: dims.clone(),
                init: match ty {
                    1 => Init::Int(0),
                    2 => Init::Float(0.0),
                    3 => Init::Str(b""),
                    _ => Init::None,
                },
            })
            .collect();
        out.extend(self.vars.iter().cloned());
        out
    }

    /// 产出 `MemSource`(含 `ysbin/*` 与 `system/YSCom/YSCom.ycd`)。
    pub fn build(&self) -> MemSource {
        let mut src = MemSource::new(Codec::Cp932);
        let omit = |k: &str| self.omit.contains(&k);
        if !omit("ysc") {
            src.insert(b"ysbin/ysc.ybn", build_yscm(&self.commands))
                .unwrap();
        }
        if !omit("ysl") {
            src.insert(b"ysbin/ysl.ybn", build_yslb(&self.labels))
                .unwrap();
        }
        if !omit("ysv") {
            src.insert(b"ysbin/ysv.ybn", build_ysvr(&self.all_vars()))
                .unwrap();
        }
        if !omit("yst_list") {
            src.insert(b"ysbin/yst_list.ybn", build_ystl(self.scripts.len()))
                .unwrap();
        }
        if !omit("yscfg") {
            src.insert(b"ysbin/yscfg.ybn", build_yscfg(640, 480, b"fixture"))
                .unwrap();
        }
        if !omit("ycd") {
            src.insert(
                b"system/YSCom/YSCom.ycd",
                build_ycd(&self.commands, &self.effective_sysvars()),
            )
            .unwrap();
        }
        for (i, groups) in self.scripts.iter().enumerate() {
            src.insert(
                format!("ysbin/yst{i:05}.ybn").as_bytes(),
                build_ystb(groups, DEFAULT_SCRIPT_KEY),
            )
            .unwrap();
        }
        src
    }
}

impl Default for ProjectBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// 与 0.495 实测一致的命令子集(序号来自 `ysc.ybn`;夹具专用)。
pub fn default_commands() -> Vec<CommandSpec> {
    vec![
        ("CDDA", vec![]),
        ("CG", vec![("ID", 1, 0), ("IDNO", 0, 1)]),
        ("CGACT", vec![]),
        ("CGEND", vec![("ID", 1, 0), ("IDNO", 0, 1)]),
        ("CGINFO", vec![]),
        ("CLIPACT", vec![]),
        ("CLIPINFO", vec![]),
        ("CSV", vec![]),
        ("CSVINFO", vec![]),
        ("DEBUGLIST", vec![]),
        (
            "DIALOG",
            vec![
                ("CAPTION", 1, 0),
                ("STR", 1, 0),
                ("STR2", 1, 0),
                ("STR3", 1, 0),
                ("STR4", 1, 0),
                ("STR5", 1, 0),
                ("STR6", 1, 0),
                ("STR7", 1, 0),
                ("STR8", 1, 0),
                ("DEFSTR", 1, 0),
                ("LETSTR", 0, 0),
                ("LET", 0, 0),
                ("OK", 0, 0),
                ("OKCANCEL", 0, 0),
                ("YESNO", 0, 0),
                ("YESNOCANCEL", 0, 0),
                ("INPUT", 0, 0),
                ("INPUTLIMIT", 0, 0),
                ("DEFBUTTON", 0, 0),
                ("ICON", 0, 0),
                ("FILEOPEN", 1, 0),
                ("FILEEXT", 1, 0),
                ("FOLDER", 1, 0),
                ("CHECKBOX", 0, 0),
                ("SLIDER", 0, 0),
                ("A", 0, 0),
                ("A2", 0, 0),
                ("A3", 0, 0),
                ("A4", 0, 0),
                ("A5", 0, 0),
                ("A6", 0, 0),
                ("A7", 0, 0),
                ("A8", 0, 0),
                ("E", 0, 0),
                ("E2", 0, 0),
                ("E3", 0, 0),
                ("E4", 0, 0),
                ("E5", 0, 0),
                ("E6", 0, 0),
                ("E7", 0, 0),
                ("E8", 0, 0),
                ("CHECK", 0, 0),
                ("CHECK2", 0, 0),
                ("CHECK3", 0, 0),
                ("CHECK4", 0, 0),
                ("CHECK5", 0, 0),
                ("CHECK6", 0, 0),
                ("CHECK7", 0, 0),
                ("CHECK8", 0, 0),
                ("NOWAIT", 0, 0),
            ],
        ),
        ("ELSE", vec![]),
        ("EMOTE", vec![]),
        ("EMOTEINFO", vec![]),
        ("END", vec![("RESTART", 0, 3)]),
        ("ERROR", vec![("NO", 0, 3)]),
        ("ERRORINFO", vec![("NO", 0, 2), ("LET", 0, 3)]),
        ("F_FLT", vec![]),
        ("F_INT", vec![]),
        ("F_STR", vec![]),
        ("F_VAR", vec![]),
        ("FILEACT", vec![("FILE", 1, 0)]),
        (
            "FILEINFO",
            vec![("FILE", 1, 0), ("EXIST", 0, 3), ("LET", 0, 0)],
        ),
        ("FLASH", vec![]),
        ("FLASHEND", vec![]),
        ("FLASHINFO", vec![]),
        ("FLT", vec![("SET", 3, 0)]),
        ("FONT", vec![]),
        ("FONTINFO", vec![]),
        ("FPS", vec![("SET", 0, 13), ("SKIP", 0, 3)]),
        ("G_FLT", vec![]),
        ("G_FLT2", vec![]),
        ("G_FLT3", vec![]),
        ("G_FLT4", vec![]),
        ("G_INT", vec![]),
        ("G_INT2", vec![]),
        ("G_INT3", vec![]),
        ("G_INT4", vec![]),
        ("G_STR", vec![]),
        ("G_STR2", vec![]),
        ("G_STR3", vec![]),
        ("G_STR4", vec![]),
        ("G_VAR", vec![]),
        ("GO", vec![("#", 2, 0)]),
        ("GOSUB", gosub_params()),
        ("IF", vec![]),
        ("IFBLEND", vec![]),
        ("IFBREAK", vec![("LV", 0, 1)]),
        ("IFCONTINUE", vec![("LV", 0, 1)]),
        ("IFEND", vec![]),
        ("INPUT", vec![]),
        ("INT", vec![("SET", 3, 0)]),
        ("LABEL", vec![("#", 0, 0)]),
        (
            "LABELINFO",
            vec![("#", 1, 0), ("LET", 0, 0), ("EXIST", 0, 3)],
        ),
        ("LET", vec![]),
        (
            "LOAD",
            vec![
                ("FILE", 1, 0),
                ("DNO", 3, 0),
                ("LET", 0, 0),
                ("ID", 1, 0),
                ("IDNO", 3, 0),
                ("G_INT", 0, 1),
                ("G_INT2", 0, 1),
                ("G_INT3", 0, 1),
                ("G_INT4", 0, 1),
                ("G_FLT", 0, 1),
                ("G_FLT2", 0, 1),
                ("G_FLT3", 0, 1),
                ("G_FLT4", 0, 1),
                ("G_STR", 0, 1),
                ("G_STR2", 0, 1),
                ("G_STR3", 0, 1),
                ("G_STR4", 0, 1),
            ],
        ),
        ("LOOP", vec![("SET", 3, 0)]),
        ("LOOPBREAK", vec![("LV", 0, 1)]),
        ("LOOPCONTINUE", vec![("LV", 0, 1)]),
        ("LOOPEND", vec![]),
        ("MACRO", vec![]),
        ("MATH", math_params()),
        ("MATH3D", vec![]),
        ("MENU", vec![]),
        ("MENUACT", vec![]),
        ("MENUEND", vec![]),
        ("MENUINFO", vec![]),
        ("MIDI", vec![]),
        ("MIDIEND", vec![]),
        ("MIDIINFO", vec![]),
        ("MOUSE", vec![]),
        ("MOVIE", vec![]),
        ("MOVIEEND", vec![]),
        ("MOVIEINFO", vec![]),
        ("PLUG", vec![]),
        ("PREP", vec![]),
        ("REGINFO", vec![]),
        ("RETURN", return_params()),
        ("RETURNBREAK", vec![]),
        ("RETURNCODE", vec![]),
        ("S_FLT", vec![]),
        ("S_INT", vec![]),
        ("S_STR", vec![]),
        ("S_VAR", vec![]),
        (
            "SAVE",
            vec![
                ("FILE", 1, 0),
                ("DNO", 3, 0),
                ("SET", 0, 0),
                ("ID", 1, 0),
                ("IDNO", 3, 0),
                ("G_INT", 0, 1),
                ("G_INT2", 0, 1),
                ("G_INT3", 0, 1),
                ("G_INT4", 0, 1),
                ("G_FLT", 0, 1),
                ("G_FLT2", 0, 1),
                ("G_FLT3", 0, 1),
                ("G_FLT4", 0, 1),
                ("G_STR", 0, 1),
                ("G_STR2", 0, 1),
                ("G_STR3", 0, 1),
                ("G_STR4", 0, 1),
            ],
        ),
        ("SAVEEND", vec![]),
        ("SAVEINFO", vec![]),
        ("SOUND", vec![]),
        ("SOUNDEND", vec![]),
        ("SOUNDINFO", vec![]),
        ("STR", vec![("SET", 3, 0)]),
        ("SYSTEM", vec![]),
        ("SYSTEMINFO", vec![("LET", 0, 0)]),
        ("TASK", task_params()),
        ("TASKEND", vec![("ID", 1, 0)]),
        (
            "TASKINFO",
            vec![("ID", 1, 0), ("LET", 0, 0), ("EXIST", 0, 3)],
        ),
        ("TEXT", text_params()),
        ("TEXTINFO", vec![]),
        ("TRAY", vec![]),
        ("VAR", vec![("", 3, 0)]),
        ("VARACT", varact_params()),
        ("VARINFO", varinfo_params()),
        ("WAIT", vec![("FRAME", 0, 1), ("TIME", 0, 1)]),
        ("WINDOW", vec![]),
        ("WINDOWEND", vec![]),
        ("WINDOWINFO", vec![]),
        ("WORD", vec![("", 3, 0)]),
        ("YMV", vec![]),
        ("YMVEND", vec![]),
        ("YMVINFO", vec![]),
        ("_", vec![("", 3, 0)]),
        ("_IFDEF", vec![("STR", 0, 0)]),
        ("_IFNDEF", vec![("STR", 0, 0)]),
        ("_IFEND", vec![]),
        (");", vec![]),
        ("SYSTEMMODE", vec![]),
        ("COMPILEMODE", vec![]),
        ("RELEASEMODE", vec![]),
        ("PROJECTFOLDER", vec![]),
    ]
}

fn gosub_params() -> Vec<(&'static str, u8, u8)> {
    let mut p = vec![("#", 1, 0)];
    for n in 1..=16 {
        p.push(pint_name(n));
    }
    for n in 1..=16 {
        p.push(pflt_name(n));
    }
    for n in 1..=16 {
        p.push(pstr_name(n));
    }
    p
}

fn pint_name(n: usize) -> (&'static str, u8, u8) {
    match n {
        1 => ("PINT", 0, 0),
        2 => ("PINT2", 0, 0),
        3 => ("PINT3", 0, 0),
        4 => ("PINT4", 0, 0),
        5 => ("PINT5", 0, 0),
        6 => ("PINT6", 0, 0),
        7 => ("PINT7", 0, 0),
        8 => ("PINT8", 0, 0),
        9 => ("PINT9", 0, 0),
        10 => ("PINT10", 0, 0),
        11 => ("PINT11", 0, 0),
        12 => ("PINT12", 0, 0),
        13 => ("PINT13", 0, 0),
        14 => ("PINT14", 0, 0),
        15 => ("PINT15", 0, 0),
        _ => ("PINT16", 0, 0),
    }
}

fn pflt_name(n: usize) -> (&'static str, u8, u8) {
    match n {
        1 => ("PFLT", 2, 0),
        2 => ("PFLT2", 2, 0),
        3 => ("PFLT3", 2, 0),
        4 => ("PFLT4", 2, 0),
        5 => ("PFLT5", 2, 0),
        6 => ("PFLT6", 2, 0),
        7 => ("PFLT7", 2, 0),
        8 => ("PFLT8", 2, 0),
        9 => ("PFLT9", 2, 0),
        10 => ("PFLT10", 2, 0),
        11 => ("PFLT11", 2, 0),
        12 => ("PFLT12", 2, 0),
        13 => ("PFLT13", 2, 0),
        14 => ("PFLT14", 2, 0),
        15 => ("PFLT15", 2, 0),
        _ => ("PFLT16", 2, 0),
    }
}

fn pstr_name(n: usize) -> (&'static str, u8, u8) {
    match n {
        1 => ("PSTR", 1, 0),
        2 => ("PSTR2", 1, 0),
        3 => ("PSTR3", 1, 0),
        4 => ("PSTR4", 1, 0),
        5 => ("PSTR5", 1, 0),
        6 => ("PSTR6", 1, 0),
        7 => ("PSTR7", 1, 0),
        8 => ("PSTR8", 1, 0),
        9 => ("PSTR9", 1, 0),
        10 => ("PSTR10", 1, 0),
        11 => ("PSTR11", 1, 0),
        12 => ("PSTR12", 1, 0),
        13 => ("PSTR13", 1, 0),
        14 => ("PSTR14", 1, 0),
        15 => ("PSTR15", 1, 0),
        _ => ("PSTR16", 1, 0),
    }
}

fn return_params() -> Vec<(&'static str, u8, u8)> {
    let mut p = Vec::new();
    for n in 1..=16 {
        p.push(match n {
            1 => ("RINT", 0, 0),
            2 => ("RINT2", 0, 0),
            3 => ("RINT3", 0, 0),
            4 => ("RINT4", 0, 0),
            5 => ("RINT5", 0, 0),
            6 => ("RINT6", 0, 0),
            7 => ("RINT7", 0, 0),
            8 => ("RINT8", 0, 0),
            9 => ("RINT9", 0, 0),
            10 => ("RINT10", 0, 0),
            11 => ("RINT11", 0, 0),
            12 => ("RINT12", 0, 0),
            13 => ("RINT13", 0, 0),
            14 => ("RINT14", 0, 0),
            15 => ("RINT15", 0, 0),
            _ => ("RINT16", 0, 0),
        });
    }
    for n in 1..=16 {
        p.push(match n {
            1 => ("RFLT", 2, 0),
            2 => ("RFLT2", 2, 0),
            3 => ("RFLT3", 2, 0),
            4 => ("RFLT4", 2, 0),
            5 => ("RFLT5", 2, 0),
            6 => ("RFLT6", 2, 0),
            7 => ("RFLT7", 2, 0),
            8 => ("RFLT8", 2, 0),
            9 => ("RFLT9", 2, 0),
            10 => ("RFLT10", 2, 0),
            11 => ("RFLT11", 2, 0),
            12 => ("RFLT12", 2, 0),
            13 => ("RFLT13", 2, 0),
            14 => ("RFLT14", 2, 0),
            15 => ("RFLT15", 2, 0),
            _ => ("RFLT16", 2, 0),
        });
    }
    for n in 1..=16 {
        p.push(match n {
            1 => ("RSTR", 1, 0),
            2 => ("RSTR2", 1, 0),
            3 => ("RSTR3", 1, 0),
            4 => ("RSTR4", 1, 0),
            5 => ("RSTR5", 1, 0),
            6 => ("RSTR6", 1, 0),
            7 => ("RSTR7", 1, 0),
            8 => ("RSTR8", 1, 0),
            9 => ("RSTR9", 1, 0),
            10 => ("RSTR10", 1, 0),
            11 => ("RSTR11", 1, 0),
            12 => ("RSTR12", 1, 0),
            13 => ("RSTR13", 1, 0),
            14 => ("RSTR14", 1, 0),
            15 => ("RSTR15", 1, 0),
            _ => ("RSTR16", 1, 0),
        });
    }
    p
}

fn math_params() -> Vec<(&'static str, u8, u8)> {
    vec![
        ("LET", 0, 0),
        ("SET", 2, 0),
        ("SET2", 2, 0),
        ("SETSTR", 1, 0),
        ("SIN", 0, 3),
        ("ASIN", 0, 3),
        ("SINH", 0, 3),
        ("COS", 0, 3),
        ("ACOS", 0, 3),
        ("COSH", 0, 3),
        ("TAN", 0, 3),
        ("ATAN", 0, 3),
        ("ATAN2", 0, 3),
        ("TANH", 0, 3),
        ("SQRT", 0, 3),
        ("ABS", 0, 3),
        ("EXP", 0, 3),
        ("LOG", 0, 3),
        ("LOG10", 0, 3),
        ("POW", 0, 3),
        ("RND", 0, 3),
        ("HASH", 0, 3),
    ]
}

fn varact_params() -> Vec<(&'static str, u8, u8)> {
    let mut p = vec![
        ("SET", 0, 0),
        ("LET", 0, 0),
        ("CUT", 0, 3),
        ("COPY", 0, 3),
        ("POS", 0, 2),
        ("LENGTH", 0, 2),
        ("TYPE", 0, 3),
        ("UPPER", 0, 3),
        ("UPPER2", 0, 3),
        ("LOWER", 0, 3),
        ("LOWER2", 0, 3),
        ("HANTOZEN", 0, 3),
        ("ZENTOHAN", 0, 3),
        ("DIMSIZE", 0, 2),
        ("PUSH", 0, 1),
        ("POP", 0, 1),
        ("INIT", 0, 3),
    ];
    for n in 0..4 {
        p.push(match n {
            0 => ("G_INT", 0, 3),
            1 => ("G_INT2", 0, 3),
            2 => ("G_INT3", 0, 3),
            _ => ("G_INT4", 0, 3),
        });
    }
    for n in 0..4 {
        p.push(match n {
            0 => ("G_FLT", 0, 3),
            1 => ("G_FLT2", 0, 3),
            2 => ("G_FLT3", 0, 3),
            _ => ("G_FLT4", 0, 3),
        });
    }
    for n in 0..4 {
        p.push(match n {
            0 => ("G_STR", 0, 3),
            1 => ("G_STR2", 0, 3),
            2 => ("G_STR3", 0, 3),
            _ => ("G_STR4", 0, 3),
        });
    }
    p
}

fn varinfo_params() -> Vec<(&'static str, u8, u8)> {
    vec![
        ("SET", 0, 0),
        ("LET", 0, 0),
        ("TYPE", 0, 3),
        ("STRTYPE", 0, 3),
        ("DIMNUM", 0, 3),
        ("DIMSIZE", 0, 3),
        ("DIMSIZE2", 0, 3),
        ("DIMSIZE3", 0, 3),
        ("DIMSIZE4", 0, 3),
        ("DIMSIZE5", 0, 3),
        ("DIMSIZE6", 0, 3),
        ("DIMSIZE7", 0, 3),
        ("DIMSIZE8", 0, 3),
        ("LENGTH", 0, 3),
        ("SEARCH", 0, 3),
        ("SJISCODE", 0, 3),
        ("UTF8CODE", 0, 3),
        ("INT", 0, 0),
        ("FLT", 0, 0),
        ("STR", 1, 0),
        ("NO", 0, 1),
    ]
}

/// `TASK` 参数表 = **真实 ysc 的 38 项布局**(v494 与 0.495 逐项相同)。
///
/// 2026-10-25 审查:此表此前被压缩成 18 项(只留 TINT 十项),而夹具用
/// 真实槽号构造窗口(`kind=34` 是入口标签 `#`、`35` 是 `SCRIPTPOS`)。
/// 旧代码按槽号硬匹配真实布局,恰好"能过";改成按名解析后 `kind=34`
/// 在 18 项表里越界 ⇒ 三个 TASK 用例同时红。**是夹具失真,不是实现错**。
/// 现按真实布局展开,槽号与名字一致,两条路径都对。
///
/// | 槽 | 名字 |
/// |---|---|
/// | 0..4 | `ID` `E` `A` `Z` |
/// | 4..13 | `TINT` `TINT2..TINT10` |
/// | 14..23 | `TFLT` `TFLT2..TFLT10` |
/// | 24..33 | `TSTR` `TSTR2..TSTR10` |
/// | 34 | `#`(入口标签) |
/// | 35..37 | `SCRIPTPOS` `TEXTPOS` `RESTART` |
fn task_params() -> Vec<(&'static str, u8, u8)> {
    let mut p = vec![("ID", 1, 0), ("E", 0, 3), ("A", 0, 3), ("Z", 0, 2)];
    for i in 0..10u8 {
        p.push((TINT_NAMES[i as usize], 0, 0));
    }
    for i in 0..10u8 {
        p.push((TFLT_NAMES[i as usize], 0, 0));
    }
    for i in 0..10u8 {
        p.push((TSTR_NAMES[i as usize], 0, 0));
    }
    p.push(("#", 1, 0));
    p.push(("SCRIPTPOS", 1, 0));
    p.push(("TEXTPOS", 1, 0));
    p.push(("RESTART", 0, 3));
    p
}

const TINT_NAMES: [&str; 10] = [
    "TINT", "TINT2", "TINT3", "TINT4", "TINT5", "TINT6", "TINT7", "TINT8", "TINT9", "TINT10",
];
const TFLT_NAMES: [&str; 10] = [
    "TFLT", "TFLT2", "TFLT3", "TFLT4", "TFLT5", "TFLT6", "TFLT7", "TFLT8", "TFLT9", "TFLT10",
];
const TSTR_NAMES: [&str; 10] = [
    "TSTR", "TSTR2", "TSTR3", "TSTR4", "TSTR5", "TSTR6", "TSTR7", "TSTR8", "TSTR9", "TSTR10",
];

fn text_params() -> Vec<(&'static str, u8, u8)> {
    vec![
        ("ID", 1, 0),
        ("OUTPUTMODE", 0, 3),
        ("CLICKBUTTON", 0, 15),
        ("CLICKBUTTON2", 0, 15),
        ("CLICKBUTTON3", 0, 15),
        ("CLICKBUTTON4", 0, 15),
        ("SKIPBUTTON", 0, 15),
        ("SKIPBUTTON2", 0, 15),
        ("SKIPBUTTON3", 0, 15),
        ("SKIPBUTTON4", 0, 15),
        ("WAIT", 0, 18),
        ("RETURN", 0, 3),
        ("PAGE", 0, 3),
        ("CLICKRETURN", 0, 3),
        ("CLICKPAGE", 0, 3),
        ("CLICK", 0, 3),
        ("CLEAR", 0, 3),
        ("AUTORETURN", 0, 3),
        ("AUTOPAGE", 0, 3),
        ("AUTOCLICK", 0, 3),
    ]
}

/// 系统变量子集(顺序即 id;测试夹具只需覆盖被断言的项)。
/// 真实语料用工程 `YSCom.ycd` 的 548 项;这里保持**官方 0.495 的前缀顺序**。
pub fn default_sysvars() -> Vec<(&'static str, u8, Vec<u32>)> {
    let mut v = vec![
        ("_TIME", 1, vec![2, 8]),
        ("_OS_TIME", 1, vec![5]),
        ("_OS_VERSION", 1, vec![]),
        ("_OS_DXVERSION", 1, vec![]),
        ("_OS_SOUND", 1, vec![]),
        ("_OS_MEMTOTAL", 1, vec![]),
        ("_OS_MEMUSE", 1, vec![]),
        ("_OS_MEMAVAIL", 1, vec![]),
        ("_MEDIA_TYPE", 1, vec![]),
        ("_MEDIA_READONLY", 1, vec![]),
        ("_PAI", 2, vec![]),
        ("_DEBUGMODE", 1, vec![]),
        ("_ARGV", 3, vec![9]),
        ("_ARGC", 1, vec![]),
        ("_DROPFILE", 3, vec![1024]),
        ("_DROPFILE_NUM", 1, vec![]),
        ("_SERIAL_GNO", 1, vec![]),
        ("_SERIAL_GNO2", 1, vec![]),
        ("_SERIAL_GNO3", 1, vec![]),
        ("_SERIAL_GNO4", 1, vec![]),
        ("_SERIAL_GNO5", 1, vec![]),
        ("_SERIAL_GNO6", 1, vec![]),
        ("_SERIAL_GNO7", 1, vec![]),
        ("_SERIAL_GNO8", 1, vec![]),
        ("_SERIAL_LNO", 1, vec![]),
        ("_SERIAL_LNO2", 1, vec![]),
        ("_SERIAL_LNO3", 1, vec![]),
        ("_SERIAL_LNO4", 1, vec![]),
        ("_SERIAL_LNO5", 1, vec![]),
        ("_SERIAL_LNO6", 1, vec![]),
        ("_SERIAL_LNO7", 1, vec![]),
        ("_SERIAL_LNO8", 1, vec![]),
        ("_LC", 1, vec![]),
        ("_RET", 1, vec![]),
        ("_INT", 1, vec![]),
        ("_FLT", 2, vec![]),
        ("_STR", 3, vec![]),
        ("_PINT", 1, vec![17]),
        ("_PFLT", 2, vec![17]),
        ("_PSTR", 3, vec![17]),
        ("_PFLG", 1, vec![17]),
        ("_PINTFLG", 1, vec![17]),
        ("_PFLTFLG", 1, vec![17]),
        ("_PSTRFLG", 1, vec![17]),
        ("_RINT", 1, vec![17]),
        ("_RFLT", 2, vec![17]),
        ("_RSTR", 3, vec![17]),
        ("_RFLG", 1, vec![17]),
        ("_RINTFLG", 1, vec![17]),
        ("_RFLTFLG", 1, vec![17]),
        ("_RSTRFLG", 1, vec![17]),
        ("_TINT", 1, vec![17]),
        ("_TFLT", 2, vec![17]),
        ("_TSTR", 3, vec![17]),
        ("_VARTYPE", 1, vec![]),
        ("_SYSTEM_PATH", 3, vec![]),
        ("_SAVE_PATH", 3, vec![]),
        ("_APPDATA_PATH", 3, vec![]),
        ("_PERSONAL_PATH", 3, vec![]),
        ("_SYSTEM_VERSION", 1, vec![]),
        ("_SYSTEM_FRAME", 1, vec![]),
        ("_SYSTEM_TASKFRAME", 1, vec![]),
        ("_SYSTEM_TIME", 1, vec![]),
        ("_SAVE_PATH_TYPE", 1, vec![]),
        ("_PROJECT_PATH", 3, vec![]),
        ("_DXMODE", 1, vec![]),
        ("_SCREEN_COLOR", 1, vec![]),
        ("_SCREEN_SIZE_X", 1, vec![]),
        ("_SCREEN_SIZE_Y", 1, vec![]),
        ("_WINDOW_EXEC_MAX", 1, vec![]),
        ("_WINDOW_EXEC_MIN", 1, vec![]),
        ("_WINDOW_EXEC_CLOSE", 1, vec![]),
        ("_WINDOW_EXEC_CAPTION", 3, vec![]),
        ("_WINDOW_INIT_POS", 1, vec![]),
        ("_TEXT", 3, vec![]),
        ("_TEXT_FLAG", 3, vec![]),
        ("_TEXT_NUM", 1, vec![]),
        ("_TASK", 3, vec![]),
        ("_TASK_NUM", 1, vec![]),
        ("_CHR", 3, vec![256]),
        ("_CGSET", 1, vec![]),
        ("_SOUNDSET", 1, vec![]),
        ("_TASKSET", 1, vec![]),
        ("_MENUSET", 1, vec![]),
        ("_MENUCODE", 1, vec![]),
        ("_MOUSE_X", 1, vec![]),
        ("_MOUSE_X0", 1, vec![]),
        ("_MOUSE_X1", 1, vec![]),
        ("_MOUSE_X2", 1, vec![]),
        ("_MOUSE_X3", 1, vec![]),
        ("_MOUSE_Y", 1, vec![]),
        ("_MOUSE_Y0", 1, vec![]),
        ("_MOUSE_Y1", 1, vec![]),
        ("_MOUSE_Y2", 1, vec![]),
        ("_MOUSE_Y3", 1, vec![]),
        ("_MOUSE_L", 1, vec![]),
        ("_MOUSE_L1", 1, vec![]),
        ("_MOUSE_L2", 1, vec![]),
        ("_MOUSE_L3", 1, vec![]),
        ("_MOUSE_R", 1, vec![]),
        ("_MOUSE_R1", 1, vec![]),
        ("_MOUSE_R2", 1, vec![]),
        ("_MOUSE_R3", 1, vec![]),
        ("_MOUSE_M", 1, vec![]),
        ("_MOUSE_M1", 1, vec![]),
        ("_MOUSE_M2", 1, vec![]),
        ("_MOUSE_M3", 1, vec![]),
        ("_MOUSE_U", 1, vec![]),
        ("_MOUSE_U1", 1, vec![]),
        ("_MOUSE_U2", 1, vec![]),
        ("_MOUSE_U3", 1, vec![]),
        ("_MOUSE_D", 1, vec![]),
        ("_MOUSE_D1", 1, vec![]),
        ("_MOUSE_D2", 1, vec![]),
        ("_MOUSE_D3", 1, vec![]),
        ("_INP", 1, vec![1024]),
        ("_KEYLAST", 1, vec![]),
        ("_MOUSELAST", 1, vec![]),
        ("_KEY_A", 1, vec![]),
        ("_KEY_B", 1, vec![]),
        ("_KEY_ENTER", 1, vec![]),
        ("_KEY_ANY", 1, vec![]),
    ];
    // 让 id 401 起仍是占位(测试只需要覆盖到 _KEY_ANY 附近)。
    v.truncate(300);
    v
}

// ---------------------------------------------------------------- 序列化

pub fn build_yscm(commands: &[CommandSpec]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"YSCM");
    out.extend_from_slice(&495u32.to_le_bytes());
    out.extend_from_slice(&(commands.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    for (name, params) in commands {
        out.extend_from_slice(name.as_bytes());
        out.push(0);
        out.push(params.len() as u8);
        for (pname, kind, checker) in params {
            out.extend_from_slice(pname.as_bytes());
            out.push(0);
            out.push(*kind);
            out.push(*checker);
        }
    }
    // 尾部:37 个串 + 256 字节表(与实测同形;运行核不解析)
    for i in 0..37u8 {
        out.extend_from_slice(format!("msg{i}").as_bytes());
        out.push(0);
    }
    out.extend_from_slice(&[0u8; 256]);
    out
}

pub fn murmur2(data: &[u8], seed: u32) -> u32 {
    const M: u32 = 0x5bd1_e995;
    let mut h = seed ^ (data.len() as u32);
    let mut chunks = data.chunks_exact(4);
    for c in &mut chunks {
        let mut k = u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
        k = k.wrapping_mul(M);
        k ^= k >> 24;
        k = k.wrapping_mul(M);
        h = h.wrapping_mul(M);
        h ^= k;
    }
    let rest = chunks.remainder();
    match rest.len() {
        3 => {
            h ^= (rest[2] as u32) << 16;
            h ^= (rest[1] as u32) << 8;
            h ^= rest[0] as u32;
            h = h.wrapping_mul(M);
        }
        2 => {
            h ^= (rest[1] as u32) << 8;
            h ^= rest[0] as u32;
            h = h.wrapping_mul(M);
        }
        1 => {
            h ^= rest[0] as u32;
            h = h.wrapping_mul(M);
        }
        _ => {}
    }
    h ^= h >> 13;
    h = h.wrapping_mul(M);
    h ^= h >> 15;
    h
}

pub fn build_yslb(labels: &[LabelSpec]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"YSLB");
    out.extend_from_slice(&495u32.to_le_bytes());
    out.extend_from_slice(&(labels.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 0x3FC]); // 0x0C..0x40C 桶表(运行核不解析)
    for (name, pc, script, base_a) in labels {
        out.push(name.len() as u8);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&murmur2(name.as_bytes(), 0).to_le_bytes());
        out.extend_from_slice(&pc.to_le_bytes());
        out.extend_from_slice(&script.to_le_bytes());
        out.push(*base_a);
        out.push(0);
    }
    out
}

pub fn build_ysvr(vars: &[VarSpec]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"YSVR");
    out.extend_from_slice(&495u32.to_le_bytes());
    out.extend_from_slice(&(vars.len() as u16).to_le_bytes());
    for v in vars {
        out.push(v.kind);
        out.push(v.group);
        out.extend_from_slice(&v.script_id.to_le_bytes());
        out.extend_from_slice(&v.var_id.to_le_bytes());
        out.push(v.value_type);
        out.push(v.dims.len() as u8);
        for d in &v.dims {
            out.extend_from_slice(&d.to_le_bytes());
        }
        match (&v.init, v.value_type) {
            (Init::Int(n), 1) => out.extend_from_slice(&n.to_le_bytes()),
            (Init::Float(f), 2) => out.extend_from_slice(&f.to_le_bytes()),
            (Init::Str(s), 3) => {
                out.extend_from_slice(&(s.len() as u16).to_le_bytes());
                out.extend_from_slice(s);
            }
            _ => {}
        }
    }
    out
}

pub fn build_ystl(script_count: usize) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"YSTL");
    out.extend_from_slice(&495u32.to_le_bytes());
    out.extend_from_slice(&(script_count as u32).to_le_bytes());
    // 记录区紧跟 12 字节头(ERIS 实测);0 条时按容器头 16 字节下界补齐
    // (yu-format read_header 契约;真实语料不存在 0 条清单)。
    if script_count == 0 {
        out.extend_from_slice(&0u32.to_le_bytes());
    }
    for i in 0..script_count {
        let name = format!("data\\script\\fixture{i:04}.yst");
        out.extend_from_slice(&(i as u32).to_le_bytes());
        out.extend_from_slice(&(name.len() as u32).to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&[0u8; 16]);
        out.extend_from_slice(&0u32.to_le_bytes());
    }
    out
}

pub fn build_yscfg(w: u16, h: u16, caption: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; 0x4E];
    out[0..4].copy_from_slice(b"YSCF");
    out[4..8].copy_from_slice(&495u32.to_le_bytes());
    out[0x10..0x12].copy_from_slice(&w.to_le_bytes());
    out[0x14..0x16].copy_from_slice(&h.to_le_bytes());
    out[0x1C] = 1; // png
    out[0x1D] = 2; // bmp
    out[0x1E] = 3; // jpg
    out[0x24] = 1; // ogg
    out[0x25] = 2; // wav
    out[0x4C..0x4E].copy_from_slice(&(caption.len() as u16).to_le_bytes());
    out.extend_from_slice(caption);
    out
}

/// 组装 YSTB(四段各自异或)。
pub fn build_ystb(groups: &[GroupSpec], key: [u8; 4]) -> Vec<u8> {
    let mut part1 = Vec::new();
    let mut window_bytes = Vec::new();
    let mut content = Vec::new();
    let mut part4 = Vec::new();
    for g in groups {
        part1.push(g.opcode);
        part1.push(g.windows.len() as u8);
        part1.extend_from_slice(&0u16.to_le_bytes());
        for (kind, flag, slot, jump_len, payload) in &g.windows {
            window_bytes.push(*kind);
            window_bytes.push(*flag);
            window_bytes.extend_from_slice(&slot.to_le_bytes());
            let len = jump_len.unwrap_or(payload.len() as u32);
            window_bytes.extend_from_slice(&len.to_le_bytes());
            let off = if payload.is_empty() {
                0
            } else {
                let o = content.len() as u32;
                content.extend_from_slice(payload);
                o
            };
            window_bytes.extend_from_slice(&off.to_le_bytes());
        }
    }
    // part4 与 part1 等长(0.495 实测 137/137);内容置零。
    part4.resize(part1.len(), 0);

    let mut header = Vec::new();
    header.extend_from_slice(b"YSTB");
    header.extend_from_slice(&495u32.to_le_bytes());
    header.extend_from_slice(&(groups.len() as u32).to_le_bytes());
    header.extend_from_slice(&(part1.len() as u32).to_le_bytes());
    header.extend_from_slice(&(window_bytes.len() as u32).to_le_bytes());
    header.extend_from_slice(&(content.len() as u32).to_le_bytes());
    header.extend_from_slice(&(part4.len() as u32).to_le_bytes());
    header.extend_from_slice(&0u32.to_le_bytes());
    debug_assert_eq!(header.len(), 0x20);

    let mut out = header;
    out.extend_from_slice(&xor(&part1, &key));
    out.extend_from_slice(&xor(&window_bytes, &key));
    out.extend_from_slice(&xor(&content, &key));
    out.extend_from_slice(&xor(&part4, &key));
    out
}

fn xor(data: &[u8], key: &[u8; 4]) -> Vec<u8> {
    data.iter()
        .enumerate()
        .map(|(i, b)| b ^ key[i % 4])
        .collect()
}

/// 组装 YSCD(命令段 + 设置项/关键字区 + 系统变量段 + 消息段)。
pub fn build_ycd(commands: &[CommandSpec], sysvars: &[(String, u8, Vec<u32>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"YSCD");
    out.extend_from_slice(&495u32.to_le_bytes());
    out.extend_from_slice(&(commands.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    for (name, params) in commands {
        out.extend_from_slice(name.as_bytes());
        out.push(0);
        out.push(params.len() as u8);
        for (pname, kind, checker) in params {
            out.extend_from_slice(pname.as_bytes());
            out.push(0);
            out.extend_from_slice(&[*kind, 0, 0, *checker]);
        }
    }
    // 关键字区(运行核只做诊断计数;格式为 NUL 串流)
    for kw in ["SYSTEMMODE", "SX", "SY", "PROJECTFOLDER", "PINT", "PSTR"] {
        out.extend_from_slice(kw.as_bytes());
        out.push(0);
    }
    // 系统变量段:u32 count + u32 0 + 记录
    out.extend_from_slice(&(sysvars.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    for (name, ty, dims) in sysvars {
        out.extend_from_slice(name.as_bytes());
        out.push(0);
        out.push(*ty);
        out.push(dims.len() as u8);
        for d in dims {
            out.extend_from_slice(&d.to_le_bytes());
        }
    }
    // 消息段:u32 count + u32 0 + 记录(0.495 实测为 121 条)
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    for (id, msg) in [(1u32, "e1"), (2u32, "e2")] {
        out.extend_from_slice(&id.to_le_bytes());
        out.extend_from_slice(msg.as_bytes());
        out.push(0);
    }
    out
}
